use anyhow::Result;
use chron_db::{BalanceChange, BalanceChangeReason};
use chrono::{DateTime, Utc};
use subxt::{
    events::{EventDetails, Events},
    OnlineClient, PolkadotConfig,
};
use tracing::{debug, info};

/// Balance decoder for extracting balance changes from blockchain events
pub struct BalanceDecoder {
    client: OnlineClient<PolkadotConfig>,
}

impl BalanceDecoder {
    /// Create a new balance decoder
    pub fn new(client: OnlineClient<PolkadotConfig>) -> Self {
        Self { client }
    }

    /// Process events from a block and extract balance changes
    pub async fn decode_balance_changes(
        &self,
        events: Events<PolkadotConfig>,
        block_number: i64,
        block_timestamp: DateTime<Utc>,
    ) -> Result<Vec<BalanceChange>> {
        let mut balance_changes = Vec::new();
        let mut event_index = 0i32;

        for event in events.iter() {
            let event = event?;

            // Get pallet and event names
            let pallet_name = event.pallet_name();
            let event_name = event.variant_name();

            debug!(
                "Processing event: {}::{} at block {} index {}",
                pallet_name, event_name, block_number, event_index
            );

            // Get the extrinsic hash if this event is part of an extrinsic
            // For now, we'll use None as getting the actual extrinsic hash
            // requires accessing the block's extrinsics which would need
            // additional context beyond just the Events object
            let extrinsic_hash: Option<Vec<u8>> = None;

            // Extract balance changes based on event type
            let changes = match (pallet_name, event_name) {
                // Balances pallet events
                ("Balances", "Transfer") => self.decode_transfer_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,
                ("Balances", "Endowed") => self.decode_endowed_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,
                ("Balances", "Deposit") => self.decode_deposit_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,
                ("Balances", "Withdraw") => self.decode_withdraw_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,
                ("Balances", "Slashed") => self.decode_slashed_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,
                ("Balances", "Reserved") => self.decode_reserved_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,
                ("Balances", "Unreserved") => self.decode_unreserved_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,

                // System pallet events that affect balances
                ("System", "NewAccount") => self.decode_new_account_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,
                ("System", "KilledAccount") => self.decode_killed_account_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,

                // TransactionPayment pallet events
                ("TransactionPayment", "TransactionFeePaid") => self.decode_fee_paid_event(
                    &event,
                    block_number,
                    event_index,
                    block_timestamp,
                    extrinsic_hash,
                )?,

                // Staking rewards (if applicable)
                ("Staking", "Rewarded") | ("Staking", "Reward") => self
                    .decode_staking_reward_event(
                        &event,
                        block_number,
                        event_index,
                        block_timestamp,
                        extrinsic_hash,
                    )?,

                // Skip other events
                _ => vec![],
            };

            balance_changes.extend(changes);
            event_index += 1;
        }

        Ok(balance_changes)
    }

    /// Decode a Transfer event
    fn decode_transfer_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        event_index: i32,
        block_timestamp: DateTime<Utc>,
        extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        use subxt::ext::scale_value::{Composite, Primitive, Value, ValueDef};

        let decoded = event.as_root_event::<Value>()?;
        let mut changes = Vec::new();

        // Transfer event structure: { from: AccountId, to: AccountId, amount: Balance }
        if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
            let mut from_account: Option<Vec<u8>> = None;
            let mut to_account: Option<Vec<u8>> = None;
            let mut amount: Option<u128> = None;

            for (name, value) in fields {
                match name.as_str() {
                    "from" => {
                        if let ValueDef::Composite(Composite::Unnamed(vals)) = &value.value {
                            if let Some(first_val) = vals.first() {
                                if let ValueDef::Primitive(Primitive::U128(bytes)) =
                                    &first_val.value
                                {
                                    from_account = Some(bytes.to_le_bytes().to_vec());
                                }
                            }
                        }
                    }
                    "to" => {
                        if let ValueDef::Composite(Composite::Unnamed(vals)) = &value.value {
                            if let Some(first_val) = vals.first() {
                                if let ValueDef::Primitive(Primitive::U128(bytes)) =
                                    &first_val.value
                                {
                                    to_account = Some(bytes.to_le_bytes().to_vec());
                                }
                            }
                        }
                    }
                    "amount" => {
                        if let ValueDef::Primitive(Primitive::U128(val)) = &value.value {
                            amount = Some(*val);
                        }
                    }
                    _ => {}
                }
            }

            if let (Some(from), Some(to), Some(amt)) = (from_account, to_account, amount) {
                // Create negative balance change for sender
                changes.push(BalanceChange {
                    id: None,
                    account: from.clone(),
                    block_number,
                    event_index,
                    delta: (-(amt as i128)).to_string(),
                    reason: BalanceChangeReason::Transfer,
                    extrinsic_hash: extrinsic_hash.clone(),
                    event_pallet: "Balances".to_string(),
                    event_variant: "Transfer".to_string(),
                    block_ts: block_timestamp,
                });

                // Create positive balance change for receiver
                changes.push(BalanceChange {
                    id: None,
                    account: to.clone(),
                    block_number,
                    event_index: event_index + 1,
                    delta: (amt as i128).to_string(),
                    reason: BalanceChangeReason::Transfer,
                    extrinsic_hash,
                    event_pallet: "Balances".to_string(),
                    event_variant: "Transfer".to_string(),
                    block_ts: block_timestamp,
                });

                debug!(
                    "Decoded Transfer at block {}: {} tokens from {} to {}",
                    block_number,
                    amt,
                    hex::encode(&from),
                    hex::encode(&to)
                );
            }
        }

        Ok(changes)
    }

    /// Decode an Endowed event (account received initial balance)
    fn decode_endowed_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        event_index: i32,
        block_timestamp: DateTime<Utc>,
        extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        use subxt::ext::scale_value::{Composite, Primitive, Value, ValueDef};

        // Decode the event dynamically
        let decoded = event.as_root_event::<Value>()?;

        let mut changes = Vec::new();

        // Endowed event structure: { account: AccountId, free_balance: Balance }
        if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
            let mut account: Option<Vec<u8>> = None;
            let mut balance: Option<i128> = None;

            for (name, value) in fields {
                match name.as_str() {
                    "account" => {
                        if let ValueDef::Composite(Composite::Unnamed(vals)) = &value.value {
                            if let Some(first_val) = vals.first() {
                                if let ValueDef::Primitive(Primitive::U128(bytes)) =
                                    &first_val.value
                                {
                                    account = Some(bytes.to_le_bytes().to_vec());
                                }
                            }
                        }
                    }
                    "free_balance" => {
                        if let ValueDef::Primitive(Primitive::U128(val)) = &value.value {
                            balance = Some(*val as i128);
                        }
                    }
                    _ => {}
                }
            }

            if let (Some(acc), Some(bal)) = (account, balance) {
                changes.push(BalanceChange {
                    id: None,
                    account: acc,
                    block_number,
                    event_index,
                    delta: bal.to_string(),
                    reason: BalanceChangeReason::Endowment,
                    extrinsic_hash,
                    event_pallet: "Balances".to_string(),
                    event_variant: "Endowed".to_string(),
                    block_ts: block_timestamp,
                });

                debug!("Decoded Endowed at block {}: {} tokens", block_number, bal);
            }
        }

        Ok(changes)
    }

    /// Decode a Deposit event
    fn decode_deposit_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        event_index: i32,
        block_timestamp: DateTime<Utc>,
        extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        use subxt::ext::scale_value::{Composite, Primitive, Value, ValueDef};

        let decoded = event.as_root_event::<Value>()?;
        let mut changes = Vec::new();

        // Deposit event structure: { who: AccountId, amount: Balance }
        if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
            let mut account: Option<Vec<u8>> = None;
            let mut amount: Option<i128> = None;

            for (name, value) in fields {
                match name.as_str() {
                    "who" => {
                        if let ValueDef::Composite(Composite::Unnamed(vals)) = &value.value {
                            if let Some(first_val) = vals.first() {
                                if let ValueDef::Primitive(Primitive::U128(bytes)) =
                                    &first_val.value
                                {
                                    account = Some(bytes.to_le_bytes().to_vec());
                                }
                            }
                        }
                    }
                    "amount" => {
                        if let ValueDef::Primitive(Primitive::U128(val)) = &value.value {
                            amount = Some(*val as i128);
                        }
                    }
                    _ => {}
                }
            }

            if let (Some(acc), Some(amt)) = (account, amount) {
                changes.push(BalanceChange {
                    id: None,
                    account: acc,
                    block_number,
                    event_index,
                    delta: amt.to_string(),
                    reason: BalanceChangeReason::Deposit,
                    extrinsic_hash,
                    event_pallet: "Balances".to_string(),
                    event_variant: "Deposit".to_string(),
                    block_ts: block_timestamp,
                });

                debug!("Decoded Deposit at block {}: {} tokens", block_number, amt);
            }
        }

        Ok(changes)
    }

    /// Decode a Withdraw event
    fn decode_withdraw_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        event_index: i32,
        block_timestamp: DateTime<Utc>,
        extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        use subxt::ext::scale_value::{Composite, Primitive, Value, ValueDef};

        let decoded = event.as_root_event::<Value>()?;
        let mut changes = Vec::new();

        // Withdraw event structure: { who: AccountId, amount: Balance }
        if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
            let mut account: Option<Vec<u8>> = None;
            let mut amount: Option<i128> = None;

            for (name, value) in fields {
                match name.as_str() {
                    "who" => {
                        if let ValueDef::Composite(Composite::Unnamed(vals)) = &value.value {
                            if let Some(first_val) = vals.first() {
                                if let ValueDef::Primitive(Primitive::U128(bytes)) =
                                    &first_val.value
                                {
                                    account = Some(bytes.to_le_bytes().to_vec());
                                }
                            }
                        }
                    }
                    "amount" => {
                        if let ValueDef::Primitive(Primitive::U128(val)) = &value.value {
                            amount = Some(*val as i128);
                        }
                    }
                    _ => {}
                }
            }

            if let (Some(acc), Some(amt)) = (account, amount) {
                changes.push(BalanceChange {
                    id: None,
                    account: acc,
                    block_number,
                    event_index,
                    delta: (-(amt as i128)).to_string(), // Withdrawal is negative
                    reason: BalanceChangeReason::Withdrawal,
                    extrinsic_hash,
                    event_pallet: "Balances".to_string(),
                    event_variant: "Withdraw".to_string(),
                    block_ts: block_timestamp,
                });

                debug!("Decoded Withdraw at block {}: {} tokens", block_number, amt);
            }
        }

        Ok(changes)
    }

    /// Decode a Slashed event
    fn decode_slashed_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        let bytes = event.bytes();
        debug!(
            "Slashed event at block {} (would decode {} bytes)",
            block_number,
            bytes.len()
        );
        Ok(vec![])
    }

    /// Decode a Reserved event
    fn decode_reserved_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        let bytes = event.bytes();
        debug!(
            "Reserved event at block {} (would decode {} bytes)",
            block_number,
            bytes.len()
        );
        Ok(vec![])
    }

    /// Decode an Unreserved event
    fn decode_unreserved_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        let bytes = event.bytes();
        debug!(
            "Unreserved event at block {} (would decode {} bytes)",
            block_number,
            bytes.len()
        );
        Ok(vec![])
    }

    /// Decode a NewAccount event
    fn decode_new_account_event(
        &self,
        _event: &EventDetails<PolkadotConfig>,
        _block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        // NewAccount doesn't directly change balances but signals account creation
        // We might want to track this for completeness
        Ok(vec![])
    }

    /// Decode a KilledAccount event
    fn decode_killed_account_event(
        &self,
        _event: &EventDetails<PolkadotConfig>,
        _block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        // KilledAccount means the account balance went to zero
        // The actual balance change would be in another event
        Ok(vec![])
    }

    /// Decode a TransactionFeePaid event
    fn decode_fee_paid_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        let bytes = event.bytes();
        debug!(
            "TransactionFeePaid event at block {} (would decode {} bytes)",
            block_number,
            bytes.len()
        );
        Ok(vec![])
    }

    /// Decode a staking reward event
    fn decode_staking_reward_event(
        &self,
        event: &EventDetails<PolkadotConfig>,
        block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        let bytes = event.bytes();
        debug!(
            "Staking reward event at block {} (would decode {} bytes)",
            block_number,
            bytes.len()
        );
        Ok(vec![])
    }

    /// Query genesis endowments from storage at block 0
    ///
    /// This is a simplified implementation. For a full implementation,
    /// you would need to:
    /// 1. Use the metadata to understand the storage layout
    /// 2. Decode the storage values properly based on the chain's types
    /// 3. Handle different account representations (AccountId32, etc.)
    pub async fn query_genesis_endowments(&self) -> Result<Vec<BalanceChange>> {
        let endowments = Vec::new();

        // Get genesis block hash
        let genesis_hash = self.client.genesis_hash();

        info!(
            "Querying genesis endowments at block {}",
            hex::encode(genesis_hash)
        );

        // In a real implementation, you would:
        // 1. Query System.Account storage entries at genesis
        // 2. Decode the AccountInfo structure to get balances
        // 3. Create BalanceChange entries for non-zero balances

        // For now, return empty as this requires chain-specific implementation
        Ok(endowments)
    }

    /// Query miner rewards for PoW chains
    pub async fn decode_miner_rewards(
        &self,
        _block_hash: [u8; 32],
        _block_number: i64,
        _block_timestamp: DateTime<Utc>,
    ) -> Result<Vec<BalanceChange>> {
        let rewards = Vec::new();

        // For PoW chains, you would:
        // 1. Extract the block author from the seal/digest
        // 2. Calculate or query the block reward
        // 3. Create a BalanceChange for the miner

        // This is highly chain-specific and depends on the consensus mechanism

        Ok(rewards)
    }
}

/// Example implementation for decoding specific event types
/// This would need to be customized for your specific runtime
impl BalanceDecoder {
    /// Example of how to decode a Transfer event with known structure
    pub fn decode_transfer_manual(
        &self,
        event_bytes: &[u8],
        block_number: i64,
        _event_index: i32,
        _block_timestamp: DateTime<Utc>,
        _extrinsic_hash: Option<Vec<u8>>,
    ) -> Result<Vec<BalanceChange>> {
        // This is a placeholder showing how you might manually decode
        // You would need to know your runtime's exact encoding

        // Typically Transfer is encoded as:
        // - from: AccountId (32 bytes)
        // - to: AccountId (32 bytes)
        // - amount: Balance (u128, compact encoded)

        if event_bytes.len() < 64 {
            return Ok(vec![]);
        }

        let from = event_bytes[0..32].to_vec();
        let to = event_bytes[32..64].to_vec();
        // Amount would need compact decoding from remaining bytes

        debug!(
            "Transfer from {} to {} at block {}",
            hex::encode(&from),
            hex::encode(&to),
            block_number
        );

        Ok(vec![])
    }
}

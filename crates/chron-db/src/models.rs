use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a block in the blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Block number (height)
    pub number: i64,
    /// Block hash (32 bytes)
    pub hash: Vec<u8>,
    /// Parent block hash (32 bytes)
    pub parent_hash: Vec<u8>,
    /// Block timestamp
    pub timestamp: DateTime<Utc>,
    /// Whether this block is part of the canonical chain
    pub is_canonical: bool,
    /// Runtime specification version
    pub runtime_spec: i64,
}

impl Block {
    /// Create a new block
    pub fn new(
        number: i64,
        hash: Vec<u8>,
        parent_hash: Vec<u8>,
        timestamp: DateTime<Utc>,
        runtime_spec: i64,
    ) -> Self {
        Self {
            number,
            hash,
            parent_hash,
            timestamp,
            is_canonical: true, // Default to canonical
            runtime_spec,
        }
    }

    /// Get block hash as hex string
    pub fn hash_hex(&self) -> String {
        ::hex::encode(&self.hash)
    }

    /// Get parent hash as hex string
    pub fn parent_hash_hex(&self) -> String {
        ::hex::encode(&self.parent_hash)
    }
}

/// Reasons for balance changes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BalanceChangeReason {
    /// Initial endowment at genesis
    Endowment,
    /// Mining/block reward
    MinerReward,
    /// Transaction fee paid
    Fee,
    /// Transaction fee refund
    FeeRefund,
    /// Transfer between accounts
    Transfer,
    /// Deposit/reservation
    Deposit,
    /// Withdrawal/unreservation
    Withdrawal,
    /// Slashing penalty
    Slash,
    /// Staking reward
    StakingReward,
    /// Other reason (with description)
    Other(String),
}

impl BalanceChangeReason {
    /// Convert to string representation for database storage
    pub fn as_str(&self) -> &str {
        match self {
            Self::Endowment => "endowment",
            Self::MinerReward => "miner_reward",
            Self::Fee => "fee",
            Self::FeeRefund => "fee_refund",
            Self::Transfer => "transfer",
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Slash => "slash",
            Self::StakingReward => "staking_reward",
            Self::Other(reason) => reason,
        }
    }

    /// Parse from string representation
    pub fn from_str(s: &str) -> Self {
        match s {
            "endowment" => Self::Endowment,
            "miner_reward" => Self::MinerReward,
            "fee" => Self::Fee,
            "fee_refund" => Self::FeeRefund,
            "transfer" => Self::Transfer,
            "deposit" => Self::Deposit,
            "withdrawal" => Self::Withdrawal,
            "slash" => Self::Slash,
            "staking_reward" => Self::StakingReward,
            other => Self::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for BalanceChangeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Represents a balance change event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceChange {
    /// Auto-incrementing ID (None for new records)
    pub id: Option<i64>,
    /// Account address (32 bytes typically)
    pub account: Vec<u8>,
    /// Block number where change occurred
    pub block_number: i64,
    /// Event index within the block
    pub event_index: i32,
    /// Balance delta (positive for credit, negative for debit)
    /// Using string to handle arbitrary precision
    pub delta: String,
    /// Reason for the balance change
    pub reason: BalanceChangeReason,
    /// Optional extrinsic hash that triggered this change
    pub extrinsic_hash: Option<Vec<u8>>,
    /// Pallet that emitted the event
    pub event_pallet: String,
    /// Event variant name
    pub event_variant: String,
    /// Block timestamp
    pub block_ts: DateTime<Utc>,
}

impl BalanceChange {
    /// Create a new balance change record
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account: Vec<u8>,
        block_number: i64,
        event_index: i32,
        delta: String,
        reason: BalanceChangeReason,
        extrinsic_hash: Option<Vec<u8>>,
        event_pallet: String,
        event_variant: String,
        block_ts: DateTime<Utc>,
    ) -> Self {
        Self {
            id: None,
            account,
            block_number,
            event_index,
            delta,
            reason,
            extrinsic_hash,
            event_pallet,
            event_variant,
            block_ts,
        }
    }

    /// Get account as hex string
    pub fn account_hex(&self) -> String {
        ::hex::encode(&self.account)
    }

    /// Get extrinsic hash as hex string
    pub fn extrinsic_hash_hex(&self) -> Option<String> {
        self.extrinsic_hash.as_ref().map(|h| ::hex::encode(h))
    }

    /// Check if this is a credit (positive balance change)
    pub fn is_credit(&self) -> bool {
        self.delta.starts_with('+') || (!self.delta.starts_with('-') && self.delta != "0")
    }

    /// Check if this is a debit (negative balance change)
    pub fn is_debit(&self) -> bool {
        self.delta.starts_with('-')
    }
}

/// Statistics for an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStats {
    /// Account address
    pub account: Vec<u8>,
    /// Current balance (at latest indexed block)
    pub balance: String,
    /// First seen at block number
    pub first_seen_block: i64,
    /// Last activity at block number
    pub last_activity_block: i64,
    /// Total number of balance changes
    pub total_changes: i64,
}

/// Chain indexing progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexProgress {
    /// Chain ID (base58 encoded genesis hash)
    pub chain_id: String,
    /// Latest indexed block number
    pub latest_block: i64,
    /// Latest indexed block hash
    pub latest_block_hash: Vec<u8>,
    /// Timestamp of latest indexed block
    pub latest_block_ts: DateTime<Utc>,
    /// Total blocks indexed
    pub blocks_indexed: i64,
    /// Total balance changes recorded
    pub balance_changes_recorded: i64,
    /// Indexing started at
    pub started_at: DateTime<Utc>,
    /// Last updated at
    pub updated_at: DateTime<Utc>,
}

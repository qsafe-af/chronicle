#![allow(dead_code)]
mod balance_decoder;

use anyhow::Result;
use balance_decoder::BalanceDecoder;
use chron_db::{
    Block, ChainRepository, ConnectionPool, DbConfig, RuntimeMetadata, RuntimeMetadataRepository,
    SchemaManager,
};
use chrono::Utc;
use serde::Deserialize;
use subxt::ext::sp_core::H256;
use subxt::{backend::rpc::RpcClient, OnlineClient, PolkadotConfig};
use tracing::{debug, info, warn};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcHeader {
    parent_hash: H256,
    number: String,
    state_root: H256,
    extrinsics_root: H256,
    #[serde(default)]
    digest: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct RpcBlockData {
    header: RpcHeader,
    extrinsics: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RpcBlock {
    block: RpcBlockData,
    justifications: Option<serde_json::Value>,
}

struct RpcHelper {
    client: RpcClient,
}

impl RpcHelper {
    fn new(client: RpcClient) -> Self {
        Self { client }
    }

    async fn get_block_hash_by_number(&self, number: u64) -> anyhow::Result<H256> {
        use subxt::backend::legacy::rpc_methods::NumberOrHex;
        use subxt::backend::legacy::LegacyRpcMethods;

        let legacy_rpc = LegacyRpcMethods::<PolkadotConfig>::new(self.client.clone());
        let block_number = NumberOrHex::Number(number);
        let hash = legacy_rpc
            .chain_get_block_hash(Some(block_number))
            .await?
            .ok_or_else(|| anyhow::anyhow!("No block hash found for block #{}", number))?;
        Ok(hash)
    }

    async fn get_latest_block_hash(&self) -> anyhow::Result<H256> {
        use subxt::backend::legacy::LegacyRpcMethods;

        let legacy_rpc = LegacyRpcMethods::<PolkadotConfig>::new(self.client.clone());
        let hash = legacy_rpc
            .chain_get_block_hash(None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No latest block hash found"))?;
        Ok(hash)
    }

    async fn get_block_by_hash(&self, hash: &H256) -> anyhow::Result<RpcBlock> {
        use subxt::backend::legacy::LegacyRpcMethods;

        let legacy_rpc = LegacyRpcMethods::<PolkadotConfig>::new(self.client.clone());
        let signed_block = legacy_rpc
            .chain_get_block(Some(*hash))
            .await?
            .ok_or_else(|| anyhow::anyhow!("No block found for hash"))?;

        // Convert from legacy SignedBlock to our RpcBlock structure
        let block = RpcBlock {
            block: RpcBlockData {
                header: RpcHeader {
                    parent_hash: signed_block.block.header.parent_hash,
                    number: format!("{:#x}", signed_block.block.header.number),
                    state_root: signed_block.block.header.state_root,
                    extrinsics_root: signed_block.block.header.extrinsics_root,
                    digest: serde_json::Value::Null,
                },
                extrinsics: signed_block
                    .block
                    .extrinsics
                    .into_iter()
                    .map(|ext| format!("0x{}", hex::encode(ext.0)))
                    .collect(),
            },
            justifications: None,
        };
        Ok(block)
    }

    async fn get_header_by_hash(&self, hash: &H256) -> anyhow::Result<RpcHeader> {
        use subxt::backend::legacy::LegacyRpcMethods;

        let legacy_rpc = LegacyRpcMethods::<PolkadotConfig>::new(self.client.clone());
        let header = legacy_rpc
            .chain_get_header(Some(*hash))
            .await?
            .ok_or_else(|| anyhow::anyhow!("No header found for hash"))?;

        Ok(RpcHeader {
            parent_hash: header.parent_hash,
            number: format!("{:#x}", header.number),
            state_root: header.state_root,
            extrinsics_root: header.extrinsics_root,
            digest: serde_json::Value::Null,
        })
    }
}

fn hex_to_h256(s: &str) -> anyhow::Result<H256> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s)?;
    if bytes.len() != 32 {
        return Err(anyhow::anyhow!(
            "expected 32 bytes for H256, got {}",
            bytes.len()
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(H256::from(arr))
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Load configuration from environment
    let ws_url = std::env::var("WS_URL").unwrap_or_else(|_| "wss://a.t.res.fm".into());
    let enable_timescale = std::env::var("ENABLE_TIMESCALE")
        .ok()
        .and_then(|s| s.parse::<bool>().ok())
        .unwrap_or(false);

    // PoW-specific configuration
    let finality_confirmations_env = std::env::var("FINALITY_CONFIRMATIONS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());

    let follow_best = std::env::var("FOLLOW_BEST")
        .ok()
        .and_then(|s| s.parse::<bool>().ok())
        .unwrap_or(true); // Default to following best blocks for PoW

    // Connect to the blockchain
    info!("Connecting to blockchain at {}", ws_url);
    let rpc_client = RpcClient::from_url(&ws_url).await?;
    let client: OnlineClient<PolkadotConfig> =
        OnlineClient::from_rpc_client(rpc_client.clone()).await?;

    // Compute chain ID from genesis hash
    let genesis_hash = client.genesis_hash();
    let chain_id = bs58::encode(genesis_hash.as_bytes()).into_string();
    info!(%chain_id, "Connected to chain; computed base58 chain ID from genesis hash");

    // Setup database connection pool
    let db_config = DbConfig::from_env();
    let mut pool = ConnectionPool::new(&db_config).await?;
    pool.set_chain_id(chain_id.clone());

    info!("Database connection pool initialized");

    // Initialize schema for this chain
    {
        let conn = pool.get().await?;
        let schema_manager = SchemaManager::new(chain_id.clone()).with_timescale(enable_timescale);

        schema_manager.initialize(&conn).await?;
        info!("Database schema initialized for chain {}", chain_id);
    }

    // Get or initialize indexing progress
    let conn = pool.get().await?;
    let chain_repo = ChainRepository::new(&conn);
    let mut progress = chain_repo.get_or_create_progress(&chain_id).await?;

    // Create balance decoder
    let decoder = BalanceDecoder::new(client.clone());

    // Process genesis endowments if starting from the beginning
    if progress.latest_block < 0 {
        info!("Processing genesis endowments...");
        let genesis_endowments = decoder.query_genesis_endowments().await?;
        if !genesis_endowments.is_empty() {
            // Store genesis endowments in database
            let mut conn = pool.get().await?;
            let tx = conn.transaction().await?;
            let tx_wrapper = chron_db::TransactionWrapper::new(tx, Some(chain_id.clone()));

            let schema = tx_wrapper.schema_name()?;
            for endowment in &genesis_endowments {
                let sql = format!(
                    r#"
                    INSERT INTO {schema}.balance_changes
                    (account, block_number, event_index, delta, reason, extrinsic_hash, event_pallet, event_variant, block_ts)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                    "#,
                    schema = schema
                );
                tx_wrapper
                    .execute(
                        &sql,
                        &[
                            &endowment.account,
                            &endowment.block_number,
                            &endowment.event_index,
                            &endowment.delta,
                            &endowment.reason.as_str(),
                            &endowment.extrinsic_hash,
                            &endowment.event_pallet,
                            &endowment.event_variant,
                            &endowment.block_ts,
                        ],
                    )
                    .await?;
            }

            tx_wrapper.commit().await?;
            info!("Stored {} genesis endowments", genesis_endowments.len());
        }
    }

    // Scan for runtime versions from genesis to current
    info!("Scanning for runtime versions...");
    let runtime_versions_discovered =
        scan_and_store_runtime_versions(&client, &pool, &chain_id).await?;
    info!(
        "Discovered {} runtime versions",
        runtime_versions_discovered
    );

    // Query chain for finality depth from runtime constants
    let finality_confirmations = match query_finality_depth(&client).await {
        Ok(depth) => {
            info!("Discovered finality depth from chain constants: {}", depth);
            depth
        }
        Err(e) => {
            warn!("Failed to query finality depth from chain: {}", e);
            let fallback = finality_confirmations_env.unwrap_or(10);
            info!("Using fallback finality confirmations: {}", fallback);
            fallback
        }
    };

    info!("Resuming indexing from block {}", progress.latest_block + 1);
    info!(
        "Using {} confirmations for finality",
        finality_confirmations
    );

    let _last_runtime_version: Option<u32> = None;

    // Catch up on historical blocks before starting subscription
    let current_best = client.blocks().at_latest().await?;
    let current_best_number = current_best.number() as i64;

    // Calculate the safe block to index up to (current - confirmations)
    let safe_block_number = if follow_best {
        (current_best_number - finality_confirmations as i64).max(0)
    } else {
        current_best_number
    };

    // Process any blocks we're behind on
    if progress.latest_block < safe_block_number {
        info!(
            "Catching up from block {} to block {} (using JSON-RPC chain_getBlock)",
            progress.latest_block + 1,
            safe_block_number
        );

        // Process historical blocks using JSON-RPC (chain_getBlock/chain_getHeader) only; no subxt block fetching
        let rpc = RpcHelper::new(rpc_client.clone());

        // Process historical blocks using subxt's legacy RPC methods
        for block_num in (progress.latest_block + 1)..=safe_block_number {
            // Step 1: Get block hash using direct RPC
            let block_hash = match rpc.get_block_hash_by_number(block_num as u64).await {
                Ok(h) => h,
                Err(e) => {
                    warn!("No block hash found for block #{}: {}", block_num, e);
                    continue;
                }
            };

            // Step 2: Fetch the block using chain_getBlock
            debug!(
                "Requesting historical block #{} via chain_getBlock by hash {}",
                block_num,
                hex::encode(block_hash.as_bytes())
            );
            match rpc.get_block_by_hash(&block_hash).await {
                Ok(rpc_block) => {
                    let header = rpc_block.block.header;

                    // We asked for a specific number; keep it authoritative
                    let block_number = block_num as i64;
                    let parent_hash = header.parent_hash;

                    info!(
                        "Processing historical block #{} ({})",
                        block_number,
                        hex::encode(block_hash.as_bytes())
                    );

                    // Build block record from header
                    let timestamp = Utc::now(); // TODO: use timestamp from extrinsics if needed
                    let runtime_version = client.runtime_version();
                    let runtime_spec = runtime_version.spec_version as i64;

                    let block_record = Block::new(
                        block_number,
                        block_hash.as_bytes().to_vec(),
                        parent_hash.as_bytes().to_vec(),
                        timestamp,
                        runtime_spec,
                    );

                    // Store block in database (no events in this path)
                    let mut conn = pool.get().await?;
                    let tx = conn.transaction().await?;
                    let tx_wrapper = chron_db::TransactionWrapper::new(tx, Some(chain_id.clone()));

                    {
                        let schema = tx_wrapper.schema_name()?;
                        let block_sql = format!(
                            r#"
                        INSERT INTO {schema}.blocks (number, hash, parent_hash, timestamp, is_canonical, runtime_spec)
                        VALUES ($1, $2, $3, $4, $5, $6)
                        ON CONFLICT (number) DO UPDATE SET
                            hash = EXCLUDED.hash,
                            parent_hash = EXCLUDED.parent_hash,
                            timestamp = EXCLUDED.timestamp,
                            is_canonical = EXCLUDED.is_canonical,
                            runtime_spec = EXCLUDED.runtime_spec
                        "#,
                            schema = schema
                        );
                        tx_wrapper
                            .execute(
                                &block_sql,
                                &[
                                    &block_record.number,
                                    &block_record.hash,
                                    &block_record.parent_hash,
                                    &block_record.timestamp,
                                    &block_record.is_canonical,
                                    &block_record.runtime_spec,
                                ],
                            )
                            .await?;

                        // Update progress
                        progress.latest_block = block_number;
                        progress.latest_block_hash = block_hash.as_bytes().to_vec();
                        progress.latest_block_ts = timestamp;
                        progress.blocks_indexed += 1;

                        let progress_sql = format!(
                            r#"
                        UPDATE {schema}.index_progress
                        SET latest_block = $2,
                            latest_block_hash = $3,
                            latest_block_ts = $4,
                            blocks_indexed = $5,
                            balance_changes_recorded = $6,
                            updated_at = $7
                        WHERE chain_id = $1
                        "#,
                            schema = schema
                        );
                        tx_wrapper
                            .execute(
                                &progress_sql,
                                &[
                                    &progress.chain_id,
                                    &progress.latest_block,
                                    &progress.latest_block_hash,
                                    &progress.latest_block_ts,
                                    &progress.blocks_indexed,
                                    &progress.balance_changes_recorded,
                                    &chrono::Utc::now(),
                                ],
                            )
                            .await?;
                    }

                    tx_wrapper.commit().await?;
                    info!("Indexed block #{} with {} balance changes", block_number, 0);
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch block #{} at hash {}: {}",
                        block_num,
                        hex::encode(block_hash.as_bytes()),
                        e
                    );
                    // Continue with next block instead of failing completely
                    continue;
                }
            }
        }

        info!("Finished catching up to block {}", safe_block_number);
    }

    // Main indexing loop - use best blocks for PoW chains
    let rpc_live = RpcHelper::new(rpc_client.clone());
    let mut block_sub = if follow_best {
        info!("Following best blocks (PoW mode)");
        client.blocks().subscribe_best().await?
    } else {
        info!("Following finalized blocks (instant finality mode)");
        client.blocks().subscribe_finalized().await?
    };
    let mut pending_blocks: std::collections::BTreeMap<i64, subxt::ext::sp_core::H256> =
        std::collections::BTreeMap::new();

    while let Some(block_result) = block_sub.next().await {
        match block_result {
            Ok(block) => {
                let block_number = block.number() as i64;
                let block_hash = block.hash();
                let block_header = block.header();
                let _parent_hash = block_header.parent_hash;

                // Skip if we've already indexed this block
                if block_number <= progress.latest_block {
                    continue;
                }

                // For PoW chains: process immediately but wait for confirmations
                if follow_best {
                    info!(
                        "Received best block #{} ({})",
                        block_number,
                        hex::encode(&block_hash)
                    );

                    // Get current best block number
                    let latest_best = client.blocks().at_latest().await?.number() as i64;

                    // Calculate confirmations for this block
                    let confirmations = latest_best - block_number;

                    // Check if we should process this block based on confirmations
                    if confirmations >= finality_confirmations as i64 {
                        info!(
                            "Processing confirmed block #{} ({}) with {} confirmations",
                            block_number,
                            hex::encode(&block_hash),
                            confirmations
                        );

                        // Process this confirmed block
                        // Commit this confirmed block based on header only (no subxt block/event usage)
                        let header = match rpc_live.get_header_by_hash(&block_hash).await {
                            Ok(h) => h,
                            Err(e) => {
                                warn!(
                                    "Failed to fetch header for confirmed block #{}: {}",
                                    block_number, e
                                );
                                continue;
                            }
                        };
                        let timestamp = Utc::now();
                        let runtime_version = client.runtime_version();
                        let runtime_spec = runtime_version.spec_version as i64;

                        // Store block and update progress
                        let mut conn = pool.get().await?;
                        let tx = conn.transaction().await?;
                        let tx_wrapper =
                            chron_db::TransactionWrapper::new(tx, Some(chain_id.clone()));
                        let schema = tx_wrapper.schema_name()?;

                        let block_sql = format!(
                            r#"
                        INSERT INTO {schema}.blocks (number, hash, parent_hash, timestamp, is_canonical, runtime_spec)
                        VALUES ($1, $2, $3, $4, $5, $6)
                        ON CONFLICT (number) DO UPDATE SET
                            hash = EXCLUDED.hash,
                            parent_hash = EXCLUDED.parent_hash,
                            timestamp = EXCLUDED.timestamp,
                            is_canonical = EXCLUDED.is_canonical,
                            runtime_spec = EXCLUDED.runtime_spec
                        "#,
                            schema = schema
                        );
                        tx_wrapper
                            .execute(
                                &block_sql,
                                &[
                                    &block_number,
                                    &block_hash.as_bytes().to_vec(),
                                    &header.parent_hash.as_bytes().to_vec(),
                                    &timestamp,
                                    &true,
                                    &runtime_spec,
                                ],
                            )
                            .await?;

                        progress.latest_block = block_number;
                        progress.latest_block_hash = block_hash.as_bytes().to_vec();
                        progress.latest_block_ts = timestamp;
                        progress.blocks_indexed += 1;

                        let progress_sql = format!(
                            r#"
                        UPDATE {schema}.index_progress
                        SET latest_block = $2,
                            latest_block_hash = $3,
                            latest_block_ts = $4,
                            blocks_indexed = $5,
                            balance_changes_recorded = $6,
                            updated_at = $7
                        WHERE chain_id = $1
                        "#,
                            schema = schema
                        );
                        tx_wrapper
                            .execute(
                                &progress_sql,
                                &[
                                    &progress.chain_id,
                                    &progress.latest_block,
                                    &progress.latest_block_hash,
                                    &progress.latest_block_ts,
                                    &progress.blocks_indexed,
                                    &progress.balance_changes_recorded,
                                    &chrono::Utc::now(),
                                ],
                            )
                            .await?;

                        tx_wrapper.commit().await?;
                    } else {
                        debug!(
                            "Block #{} waiting for confirmations ({}/{})",
                            block_number, confirmations, finality_confirmations
                        );

                        // Store block info for potential reorg detection
                        pending_blocks.insert(block_number, block_hash);

                        // Process any old blocks that now have enough confirmations
                        let confirmed_height =
                            latest_best.saturating_sub(finality_confirmations as i64);

                        let mut to_remove = Vec::new();
                        for (&pending_number, &pending_hash) in pending_blocks.iter() {
                            if pending_number <= confirmed_height
                                && pending_number > progress.latest_block
                            {
                                match rpc_live.get_header_by_hash(&pending_hash).await {
                                    Ok(pending_header) => {
                                        info!(
                                            "Processing previously pending block #{} ({})",
                                            pending_number,
                                            hex::encode(pending_hash.as_ref())
                                        );

                                        let timestamp = Utc::now();
                                        let runtime_version = client.runtime_version();
                                        let runtime_spec = runtime_version.spec_version as i64;

                                        let mut conn = pool.get().await?;
                                        let tx = conn.transaction().await?;
                                        let tx_wrapper = chron_db::TransactionWrapper::new(
                                            tx,
                                            Some(chain_id.clone()),
                                        );
                                        let schema = tx_wrapper.schema_name()?;

                                        let block_sql = format!(
                                            r#"
                                    INSERT INTO {schema}.blocks (number, hash, parent_hash, timestamp, is_canonical, runtime_spec)
                                    VALUES ($1, $2, $3, $4, $5, $6)
                                    ON CONFLICT (number) DO UPDATE SET
                                        hash = EXCLUDED.hash,
                                        parent_hash = EXCLUDED.parent_hash,
                                        timestamp = EXCLUDED.timestamp,
                                        is_canonical = EXCLUDED.is_canonical,
                                        runtime_spec = EXCLUDED.runtime_spec
                                    "#,
                                            schema = schema
                                        );
                                        tx_wrapper
                                            .execute(
                                                &block_sql,
                                                &[
                                                    &pending_number,
                                                    &pending_hash.as_bytes().to_vec(),
                                                    &pending_header.parent_hash.as_bytes().to_vec(),
                                                    &timestamp,
                                                    &true,
                                                    &runtime_spec,
                                                ],
                                            )
                                            .await?;

                                        progress.latest_block = pending_number;
                                        progress.latest_block_hash =
                                            pending_hash.as_bytes().to_vec();
                                        progress.latest_block_ts = timestamp;
                                        progress.blocks_indexed += 1;

                                        let progress_sql = format!(
                                            r#"
                                    UPDATE {schema}.index_progress
                                    SET latest_block = $2,
                                        latest_block_hash = $3,
                                        latest_block_ts = $4,
                                        blocks_indexed = $5,
                                        balance_changes_recorded = $6,
                                        updated_at = $7
                                    WHERE chain_id = $1
                                    "#,
                                            schema = schema
                                        );
                                        tx_wrapper
                                            .execute(
                                                &progress_sql,
                                                &[
                                                    &progress.chain_id,
                                                    &progress.latest_block,
                                                    &progress.latest_block_hash,
                                                    &progress.latest_block_ts,
                                                    &progress.blocks_indexed,
                                                    &progress.balance_changes_recorded,
                                                    &chrono::Utc::now(),
                                                ],
                                            )
                                            .await?;

                                        tx_wrapper.commit().await?;
                                        to_remove.push(pending_number);
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Failed to fetch pending block header #{}: {}",
                                            pending_number, e
                                        );
                                        to_remove.push(pending_number);
                                    }
                                }
                            }
                        }

                        // Remove processed blocks
                        for num in to_remove {
                            pending_blocks.remove(&num);
                        }

                        // Clean up old pending blocks that are too far behind
                        pending_blocks.retain(|&num, _| num > confirmed_height - 100);
                    }
                } else {
                    // Instant finality mode - process immediately
                    info!(
                        "Processing finalized block #{} ({})",
                        block_number,
                        hex::encode(&block_hash)
                    );

                    // Commit finalized block based on header only (no subxt block/event usage)
                    let header = match rpc_live.get_header_by_hash(&block_hash).await {
                        Ok(h) => h,
                        Err(e) => {
                            warn!(
                                "Failed to fetch header for finalized block #{}: {}",
                                block_number, e
                            );
                            continue;
                        }
                    };
                    let timestamp = Utc::now();
                    let runtime_version = client.runtime_version();
                    let runtime_spec = runtime_version.spec_version as i64;

                    let mut conn = pool.get().await?;
                    let tx = conn.transaction().await?;
                    let tx_wrapper =
                        chron_db::TransactionWrapper::new(tx, Some(chain_id.to_string()));
                    let schema = tx_wrapper.schema_name()?;

                    let block_sql = format!(
                        r#"
                        INSERT INTO {schema}.blocks (number, hash, parent_hash, timestamp, is_canonical, runtime_spec)
                        VALUES ($1, $2, $3, $4, $5, $6)
                        ON CONFLICT (number) DO UPDATE SET
                            hash = EXCLUDED.hash,
                            parent_hash = EXCLUDED.parent_hash,
                            timestamp = EXCLUDED.timestamp,
                            is_canonical = EXCLUDED.is_canonical,
                            runtime_spec = EXCLUDED.runtime_spec
                        "#,
                        schema = schema
                    );
                    tx_wrapper
                        .execute(
                            &block_sql,
                            &[
                                &block_number,
                                &block_hash.as_bytes().to_vec(),
                                &header.parent_hash.as_bytes().to_vec(),
                                &timestamp,
                                &true,
                                &runtime_spec,
                            ],
                        )
                        .await?;

                    progress.latest_block = block_number;
                    progress.latest_block_hash = block_hash.as_bytes().to_vec();
                    progress.latest_block_ts = timestamp;
                    progress.blocks_indexed += 1;

                    let progress_sql = format!(
                        r#"
                        UPDATE {schema}.index_progress
                        SET latest_block = $2,
                            latest_block_hash = $3,
                            latest_block_ts = $4,
                            blocks_indexed = $5,
                            balance_changes_recorded = $6,
                            updated_at = $7
                        WHERE chain_id = $1
                        "#,
                        schema = schema
                    );
                    tx_wrapper
                        .execute(
                            &progress_sql,
                            &[
                                &progress.chain_id,
                                &progress.latest_block,
                                &progress.latest_block_hash,
                                &progress.latest_block_ts,
                                &progress.blocks_indexed,
                                &progress.balance_changes_recorded,
                                &chrono::Utc::now(),
                            ],
                        )
                        .await?;

                    tx_wrapper.commit().await?;
                }
            }
            Err(e) => {
                warn!("Error receiving block: {}", e);
                // Attempt to reconnect or handle the error appropriately
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    }

    warn!("Block subscription ended unexpectedly");
    Ok(())
}

/// Process a single confirmed block
#[allow(dead_code)]
/// Removed from active use; blocks are committed via chain_getBlock JSON-RPC now.
async fn process_block(
    client: &OnlineClient<PolkadotConfig>,
    pool: &ConnectionPool,
    chain_id: &str,
    decoder: &balance_decoder::BalanceDecoder,
    block: subxt::blocks::Block<PolkadotConfig, OnlineClient<PolkadotConfig>>,
    progress: &mut chron_db::IndexProgress,
    last_runtime_version: &mut Option<u32>,
) -> Result<()> {
    let block_number = block.number() as i64;
    let block_hash = block.hash();
    let block_header = block.header();
    let parent_hash = block_header.parent_hash;

    // Debug header fields to validate hashes used
    debug!(
        "Header fields for block #{}: hash={}, parent={}, state_root={}, extrinsics_root={}",
        block_number,
        hex::encode(block_hash.as_bytes()),
        hex::encode(parent_hash.as_bytes()),
        hex::encode(block_header.state_root.as_bytes()),
        hex::encode(block_header.extrinsics_root.as_bytes())
    );

    // Extract timestamp from block (this is a simplified version)
    let timestamp = Utc::now(); // TODO: Extract actual block timestamp from extrinsics

    // Get runtime version and check for upgrades
    let runtime_version = client.runtime_version();
    let runtime_spec = runtime_version.spec_version as i64;
    let current_spec_version = runtime_version.spec_version;

    // Check if runtime has been upgraded
    if *last_runtime_version != Some(current_spec_version) {
        if let Some(prev_version) = *last_runtime_version {
            info!(
                "Runtime upgraded from v{} to v{} at block {}",
                prev_version, current_spec_version, block_number
            );

            // Update the previous version's last_seen_block
            let conn = pool.get().await?;
            let metadata_repo = RuntimeMetadataRepository::new(&conn);
            metadata_repo
                .update_last_seen_block(prev_version as i32, block_number - 1)
                .await?;
        }

        // Check if we need to store this new runtime version
        let conn = pool.get().await?;
        let metadata_repo = RuntimeMetadataRepository::new(&conn);

        if !metadata_repo.exists(current_spec_version as i32).await? {
            info!("Storing new runtime metadata for v{}", current_spec_version);

            // Get the metadata bytes
            let metadata_bytes = get_metadata_at_block(client, block_hash).await?;

            let runtime_metadata = RuntimeMetadata::new(
                current_spec_version as i32,
                0, // impl_version not available in simplified runtime_version
                runtime_version.transaction_version as i32,
                0, // state_version, use 0 as default
                block_number,
                metadata_bytes,
            );

            metadata_repo.upsert(&runtime_metadata).await?;
        }

        *last_runtime_version = Some(current_spec_version);
    }

    // Create block record
    let block_record = Block::new(
        block_number,
        block_hash.as_bytes().to_vec(),
        parent_hash.as_bytes().to_vec(),
        timestamp,
        runtime_spec,
    );

    // Process events to extract balance changes (skip genesis block to avoid querying events at #0)
    let mut all_balance_changes = Vec::new();
    if block_number > 0 {
        match block.events().await {
            Ok(events) => {
                match decoder
                    .decode_balance_changes(events, block_number, timestamp)
                    .await
                {
                    Ok(balance_changes) => {
                        all_balance_changes.extend(balance_changes);
                    }
                    Err(e) => {
                        warn!("Failed to decode events for block #{}: {}", block_number, e);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to fetch events for block #{}: {}", block_number, e);
            }
        }
    } else {
        debug!("Skipping events decoding for genesis block");
    }

    // Check for miner rewards (for PoW chains)
    let miner_rewards = decoder
        .decode_miner_rewards(block_hash, block_number, timestamp)
        .await?;
    all_balance_changes.extend(miner_rewards);

    // Store block and balance changes in database within a transaction
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;

    // Create a transaction wrapper for cleaner API
    let tx_wrapper = chron_db::TransactionWrapper::new(tx, Some(chain_id.to_string()));

    // Use the wrapper to execute queries within the transaction
    {
        // Insert block
        let schema = tx_wrapper.schema_name()?;
        let block_sql = format!(
            r#"
                        INSERT INTO {schema}.blocks (number, hash, parent_hash, timestamp, is_canonical, runtime_spec)
                        VALUES ($1, $2, $3, $4, $5, $6)
                        ON CONFLICT (number) DO UPDATE SET
                            hash = EXCLUDED.hash,
                            parent_hash = EXCLUDED.parent_hash,
                            timestamp = EXCLUDED.timestamp,
                            is_canonical = EXCLUDED.is_canonical,
                            runtime_spec = EXCLUDED.runtime_spec
                        "#,
            schema = schema
        );
        tx_wrapper
            .execute(
                &block_sql,
                &[
                    &block_record.number,
                    &block_record.hash,
                    &block_record.parent_hash,
                    &block_record.timestamp,
                    &block_record.is_canonical,
                    &block_record.runtime_spec,
                ],
            )
            .await?;

        // Insert balance changes
        for change in &all_balance_changes {
            let change_sql = format!(
                r#"
                            INSERT INTO {schema}.balance_changes
                            (account, block_number, event_index, delta, reason, extrinsic_hash, event_pallet, event_variant, block_ts)
                            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                            ON CONFLICT (block_number, event_index) DO NOTHING
                            "#,
                schema = schema
            );
            tx_wrapper
                .execute(
                    &change_sql,
                    &[
                        &change.account,
                        &change.block_number,
                        &change.event_index,
                        &change.delta,
                        &change.reason.as_str(),
                        &change.extrinsic_hash,
                        &change.event_pallet,
                        &change.event_variant,
                        &change.block_ts,
                    ],
                )
                .await?;
        }

        // Update progress
        progress.latest_block = block_number;
        progress.latest_block_hash = block_hash.as_bytes().to_vec();
        progress.latest_block_ts = timestamp;
        progress.blocks_indexed += 1;
        progress.balance_changes_recorded += all_balance_changes.len() as i64;

        let progress_sql = format!(
            r#"
                        UPDATE {schema}.index_progress
                        SET latest_block = $2,
                            latest_block_hash = $3,
                            latest_block_ts = $4,
                            blocks_indexed = $5,
                            balance_changes_recorded = $6,
                            updated_at = $7
                        WHERE chain_id = $1
                        "#,
            schema = schema
        );
        tx_wrapper
            .execute(
                &progress_sql,
                &[
                    &progress.chain_id,
                    &progress.latest_block,
                    &progress.latest_block_hash,
                    &progress.latest_block_ts,
                    &progress.blocks_indexed,
                    &progress.balance_changes_recorded,
                    &chrono::Utc::now(),
                ],
            )
            .await?;
    }

    // Commit the transaction
    tx_wrapper.commit().await?;

    info!(
        "Indexed block #{} with {} balance changes",
        block_number,
        all_balance_changes.len()
    );
    Ok(())
}

/// Scan the chain from genesis to current and store all runtime versions
async fn scan_and_store_runtime_versions(
    client: &OnlineClient<PolkadotConfig>,
    pool: &ConnectionPool,
    _chain_id: &str,
) -> Result<usize> {
    let mut versions_found = 0;

    // Get current block height
    let latest_block = client.blocks().at_latest().await?;
    let latest_number = latest_block.number() as i64;

    // Get connection for database operations
    let conn = pool.get().await?;
    let metadata_repo = RuntimeMetadataRepository::new(&conn);

    // Check if we already have runtime versions stored
    let existing_versions = metadata_repo.get_all_versions().await?;
    if !existing_versions.is_empty() {
        info!(
            "Found {} existing runtime versions in database",
            existing_versions.len()
        );
        return Ok(existing_versions.len());
    }

    info!(
        "No existing runtime versions found, scanning from genesis to block {}",
        latest_number
    );

    // Get genesis runtime
    let genesis_hash = client.genesis_hash();
    let genesis_metadata = get_metadata_at_block(client, genesis_hash).await?;
    let genesis_version = client.runtime_version(); // This gets current, we'll use it as approximation

    let genesis_runtime = RuntimeMetadata::new(
        1, // Assuming genesis starts at version 1, adjust if needed
        0,
        genesis_version.transaction_version as i32,
        0,
        0, // Genesis is block 0
        genesis_metadata,
    );

    metadata_repo.upsert(&genesis_runtime).await?;
    versions_found += 1;

    // Get current runtime if different from genesis
    let current_version = client.runtime_version();
    if current_version.spec_version != 1 {
        let current_metadata = get_current_metadata(client).await?;
        let current_runtime = RuntimeMetadata::new(
            current_version.spec_version as i32,
            0,
            current_version.transaction_version as i32,
            0,
            latest_number,
            current_metadata,
        );

        metadata_repo.upsert(&current_runtime).await?;
        versions_found += 1;

        // TODO: Use binary search to find intermediate versions if there are any
        // For now, we'll discover them as we process blocks
    }

    Ok(versions_found)
}

/// Get metadata at a specific block
async fn get_metadata_at_block(
    client: &OnlineClient<PolkadotConfig>,
    block_hash: subxt::ext::sp_core::H256,
) -> Result<Vec<u8>> {
    // Get metadata at this block
    use parity_scale_codec::Encode;

    let hash = block_hash;

    // Log exact hash used for fetching metadata
    info!("Fetching metadata at block {}", hex::encode(hash.as_ref()));

    // NOTE: No RPC here; we don't need to refetch the block to get metadata

    // Get metadata from the block's runtime
    // For now we use the client's current metadata as subxt doesn't expose historical metadata easily
    // In production, you'd use RPC calls to get metadata at specific blocks
    let metadata = client.metadata();
    Ok(metadata.encode())
}

/// Get current metadata
async fn get_current_metadata(client: &OnlineClient<PolkadotConfig>) -> Result<Vec<u8>> {
    use parity_scale_codec::Encode;

    let metadata = client.metadata();
    Ok(metadata.encode())
}

/// Discover all available constants in the runtime
async fn discover_runtime_constants(client: &OnlineClient<PolkadotConfig>) -> Result<()> {
    info!("Discovering runtime constants...");

    let metadata = client.metadata();

    // Iterate through all pallets
    for pallet in metadata.pallets() {
        let pallet_name = pallet.name();
        let constants = pallet.constants();

        if constants.len() > 0 {
            info!(
                "Pallet '{}' has {} constants:",
                pallet_name,
                constants.len()
            );
            for constant in constants {
                info!("  - {}: {:?}", constant.name(), constant.ty());

                // Try to decode specific interesting constants
                if constant.name().contains("Reorg")
                    || constant.name().contains("Depth")
                    || constant.name().contains("Finality")
                    || constant.name().contains("Confirmations")
                {
                    info!(
                        "    Found potentially relevant constant: {}",
                        constant.name()
                    );
                }
            }
        }
    }

    Ok(())
}

/// Query the chain for finality depth from runtime constants
async fn query_finality_depth(client: &OnlineClient<PolkadotConfig>) -> Result<u32> {
    // First, discover what constants are available (only in debug mode)
    if std::env::var("RUST_LOG")
        .unwrap_or_default()
        .contains("debug")
    {
        let _ = discover_runtime_constants(client).await;
    }

    // Try different possible constant locations for max reorg depth
    // Different chains might expose this in different pallets

    // Try Resonance-specific constant
    let resonance_addr = subxt::dynamic::constant("Resonance", "MaxReorgDepth");
    if let Ok(max_reorg_depth) = client.constants().at(&resonance_addr) {
        if let Ok(value) = max_reorg_depth.to_value() {
            if let Some(depth) = value.as_u128() {
                let depth = depth as u32;
                info!("Found MaxReorgDepth in Resonance pallet: {}", depth);
                return Ok(depth.saturating_sub(1)); // finality at (max_reorg_depth - 1)
            }
        }
    }

    // Try PoW pallet constants
    let pow_addr = subxt::dynamic::constant("PoW", "MaxReorgDepth");
    if let Ok(max_reorg_depth) = client.constants().at(&pow_addr) {
        if let Ok(value) = max_reorg_depth.to_value() {
            if let Some(depth) = value.as_u128() {
                let depth = depth as u32;
                info!("Found MaxReorgDepth in PoW pallet: {}", depth);
                return Ok(depth.saturating_sub(1));
            }
        }
    }

    // Try Difficulty pallet (for PoW chains)
    let difficulty_addr = subxt::dynamic::constant("Difficulty", "MaxReorgDepth");
    if let Ok(max_reorg_depth) = client.constants().at(&difficulty_addr) {
        if let Ok(value) = max_reorg_depth.to_value() {
            if let Some(depth) = value.as_u128() {
                let depth = depth as u32;
                info!("Found MaxReorgDepth in Difficulty pallet: {}", depth);
                return Ok(depth.saturating_sub(1));
            }
        }
    }

    // Try System pallet
    let system_addr = subxt::dynamic::constant("System", "MaxReorgDepth");
    if let Ok(max_reorg_depth) = client.constants().at(&system_addr) {
        if let Ok(value) = max_reorg_depth.to_value() {
            if let Some(depth) = value.as_u128() {
                let depth = depth as u32;
                info!("Found MaxReorgDepth in System pallet: {}", depth);
                return Ok(depth.saturating_sub(1));
            }
        }
    }

    // Try BABE pallet (for chains that use BABE)
    let babe_addr = subxt::dynamic::constant("Babe", "EpochDuration");
    if let Ok(epoch_duration) = client.constants().at(&babe_addr) {
        if let Ok(value) = epoch_duration.to_value() {
            if let Some(duration) = value.as_u128() {
                // For BABE chains, use epoch duration as a proxy for finality
                let finality_depth = ((duration as u64) / 4) as u32; // Conservative estimate
                info!(
                    "Using BABE epoch duration to estimate finality: {}",
                    finality_depth
                );
                return Ok(finality_depth);
            }
        }
    }

    // Try Grandpa pallet
    let grandpa_addr = subxt::dynamic::constant("Grandpa", "MaxAuthorities");
    if let Ok(_) = client.constants().at(&grandpa_addr) {
        // If GRANDPA exists, this chain has instant finality
        info!("Found GRANDPA pallet - using instant finality");
        return Ok(0);
    }

    // Try to infer from block production rate
    // Check if there's a MinimumPeriod constant (usually in Timestamp pallet)
    let timestamp_addr = subxt::dynamic::constant("Timestamp", "MinimumPeriod");
    if let Ok(min_period) = client.constants().at(&timestamp_addr) {
        if let Ok(value) = min_period.to_value() {
            if let Some(p) = value.as_u128() {
                let period_ms = p as u64;
                // Estimate based on block time
                // For PoW chains, assume finality after ~30 minutes worth of blocks
                let blocks_per_30_min = (30 * 60 * 1000) / period_ms;
                let finality_depth = (blocks_per_30_min.min(180)) as u32; // Cap at 180
                info!(
                    "Estimated finality depth from block time ({}ms): {}",
                    period_ms, finality_depth
                );
                return Ok(finality_depth);
            }
        }
    }

    // If we still couldn't find it, try to discover all constants in debug mode
    warn!("Could not find finality depth in known locations");
    warn!("Set RUST_LOG=debug to see all available constants");
    warn!("You can also set FINALITY_CONFIRMATIONS environment variable as override");

    Err(anyhow::anyhow!(
        "Could not determine finality depth from chain constants. Use FINALITY_CONFIRMATIONS env var to override."
    ))
}

use anyhow::Result;
use chron_db::{BalanceChange, Block, ChainRepository, ConnectionPool, DbConfig, SchemaManager};
use chrono::Utc;
use subxt::{backend::rpc::RpcClient, OnlineClient, PolkadotConfig};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Load configuration from environment
    let ws_url = std::env::var("WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:9944".into());
    let enable_timescale = std::env::var("ENABLE_TIMESCALE")
        .ok()
        .and_then(|s| s.parse::<bool>().ok())
        .unwrap_or(false);

    // Connect to the blockchain
    info!("Connecting to blockchain at {}", ws_url);
    let client: OnlineClient<PolkadotConfig> =
        OnlineClient::from_rpc_client(RpcClient::from_url(&ws_url).await?).await?;

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

    info!("Resuming indexing from block {}", progress.latest_block + 1);

    // Main indexing loop
    let mut block_sub = client.blocks().subscribe_finalized().await?;

    while let Some(block_result) = block_sub.next().await {
        match block_result {
            Ok(block) => {
                let block_number = block.number() as i64;
                let block_hash = block.hash();
                let block_header = block.header();
                let parent_hash = block_header.parent_hash;

                // Skip if we've already indexed this block
                if block_number <= progress.latest_block {
                    continue;
                }

                info!(
                    "Processing finalized block #{} ({})",
                    block_number,
                    hex::encode(&block_hash)
                );

                // Extract timestamp from block (this is a simplified version)
                let timestamp = Utc::now(); // TODO: Extract actual block timestamp from extrinsics

                // Get runtime version
                let runtime_version = client.runtime_version();
                let runtime_spec = runtime_version.spec_version as i64;

                // Create block record
                let block_record = Block::new(
                    block_number,
                    block_hash.as_bytes().to_vec(),
                    parent_hash.as_bytes().to_vec(),
                    timestamp,
                    runtime_spec,
                );

                // Process events to extract balance changes
                let events = block.events().await?;
                let mut balance_changes: Vec<BalanceChange> = Vec::new();
                let mut event_index = 0i32;

                for event in events.iter() {
                    let event = event?;

                    // Get pallet and variant names
                    let pallet_name = event.pallet_name();
                    let event_name = event.variant_name();

                    // Check if this is a balance-related event
                    // This is simplified - you'll need to decode actual events based on your runtime
                    let balance_change = match (pallet_name, event_name) {
                        ("Balances", "Transfer") => {
                            // TODO: Decode transfer event and create balance changes
                            // For now, this is a placeholder
                            None
                        }
                        ("Balances", "Endowed") => {
                            // TODO: Decode endowment event
                            None
                        }
                        ("System", "NewAccount") => {
                            // TODO: Handle new account creation
                            None
                        }
                        _ => None,
                    };

                    if let Some(change) = balance_change {
                        balance_changes.push(change);
                    }

                    event_index += 1;
                }

                // Store block and balance changes in database within a transaction
                let mut conn = pool.get().await?;
                let mut tx = conn.transaction().await?;

                // Create a transaction wrapper for cleaner API
                let tx_wrapper = chron_db::TransactionWrapper::new(tx, Some(chain_id.clone()));

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
                    // TODO: Implement actual balance change insertion when events are decoded

                    // Update progress
                    progress.latest_block = block_number;
                    progress.latest_block_hash = block_hash.as_bytes().to_vec();
                    progress.latest_block_ts = timestamp;
                    progress.blocks_indexed += 1;
                    progress.balance_changes_recorded += balance_changes.len() as i64;

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
                    balance_changes.len()
                );

                // Handle chain reorganizations if needed
                // TODO: Implement reorg detection and handling
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

// Helper function to decode balance from events (placeholder)
fn decode_balance_change_from_event(
    _event_data: &[u8],
    _block_number: i64,
    _event_index: i32,
    _timestamp: chrono::DateTime<Utc>,
) -> Option<BalanceChange> {
    // TODO: Implement actual event decoding based on your runtime
    // This will require using the metadata to decode events properly
    None
}

// Helper function to handle genesis endowments
async fn process_genesis_endowments(
    _client: &OnlineClient<PolkadotConfig>,
    _pool: &ConnectionPool,
) -> Result<()> {
    // TODO: Query and store initial balances from genesis
    // This would involve:
    // 1. Querying the System.Account storage for all accounts at block 0
    // 2. Creating BalanceChange records with reason Endowment
    // 3. Storing them in the database
    Ok(())
}

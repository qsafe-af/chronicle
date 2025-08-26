use crate::{
    connection::DbConnection,
    error::{DbError, Result},
    models::{AccountStats, BalanceChange, BalanceChangeReason, Block, IndexProgress},
};
use chrono::Utc;
use tracing::info;

/// Repository for managing blocks
pub struct BlockRepository<'a> {
    conn: &'a DbConnection,
}

impl<'a> BlockRepository<'a> {
    /// Create a new block repository
    pub fn new(conn: &'a DbConnection) -> Self {
        Self { conn }
    }

    /// Insert a new block
    pub async fn insert(&self, block: &Block) -> Result<()> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
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

        self.conn
            .execute(
                &sql,
                &[
                    &block.number,
                    &block.hash,
                    &block.parent_hash,
                    &block.timestamp,
                    &block.is_canonical,
                    &block.runtime_spec,
                ],
            )
            .await?;

        Ok(())
    }

    /// Batch insert multiple blocks
    pub async fn insert_batch(&self, blocks: &[Block]) -> Result<u64> {
        if blocks.is_empty() {
            return Ok(0);
        }

        let mut inserted = 0;

        // Use COPY for better performance with large batches
        // For now, we'll use regular inserts in a transaction
        for block in blocks {
            self.insert(block).await?;
            inserted += 1;
        }

        Ok(inserted)
    }

    /// Get a block by number
    pub async fn get_by_number(&self, number: i64) -> Result<Option<Block>> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            SELECT number, hash, parent_hash, timestamp, is_canonical, runtime_spec
            FROM {schema}.blocks
            WHERE number = $1
            "#,
            schema = schema
        );

        match self.conn.query_opt(&sql, &[&number]).await? {
            Some(row) => Ok(Some(Block {
                number: row.get(0),
                hash: row.get(1),
                parent_hash: row.get(2),
                timestamp: row.get(3),
                is_canonical: row.get(4),
                runtime_spec: row.get(5),
            })),
            None => Ok(None),
        }
    }

    /// Get a block by hash
    pub async fn get_by_hash(&self, hash: &[u8]) -> Result<Option<Block>> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            SELECT number, hash, parent_hash, timestamp, is_canonical, runtime_spec
            FROM {schema}.blocks
            WHERE hash = $1
            "#,
            schema = schema
        );

        match self.conn.query_opt(&sql, &[&hash]).await? {
            Some(row) => Ok(Some(Block {
                number: row.get(0),
                hash: row.get(1),
                parent_hash: row.get(2),
                timestamp: row.get(3),
                is_canonical: row.get(4),
                runtime_spec: row.get(5),
            })),
            None => Ok(None),
        }
    }

    /// Get the latest canonical block
    pub async fn get_latest_canonical(&self) -> Result<Option<Block>> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            SELECT number, hash, parent_hash, timestamp, is_canonical, runtime_spec
            FROM {schema}.blocks
            WHERE is_canonical = true
            ORDER BY number DESC
            LIMIT 1
            "#,
            schema = schema
        );

        match self.conn.query_opt(&sql, &[]).await? {
            Some(row) => Ok(Some(Block {
                number: row.get(0),
                hash: row.get(1),
                parent_hash: row.get(2),
                timestamp: row.get(3),
                is_canonical: row.get(4),
                runtime_spec: row.get(5),
            })),
            None => Ok(None),
        }
    }

    /// Mark blocks as non-canonical starting from a specific height
    pub async fn mark_non_canonical_from(&self, from_height: i64) -> Result<u64> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            UPDATE {schema}.blocks
            SET is_canonical = false
            WHERE number >= $1 AND is_canonical = true
            "#,
            schema = schema
        );

        self.conn.execute(&sql, &[&from_height]).await
    }

    /// Check if a block exists
    pub async fn exists(&self, number: i64) -> Result<bool> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            "SELECT EXISTS (SELECT 1 FROM {schema}.blocks WHERE number = $1)",
            schema = schema
        );

        let row = self.conn.query_one(&sql, &[&number]).await?;
        Ok(row.get(0))
    }
}

/// Repository for managing balance changes
pub struct BalanceChangeRepository<'a> {
    conn: &'a DbConnection,
}

impl<'a> BalanceChangeRepository<'a> {
    /// Create a new balance change repository
    pub fn new(conn: &'a DbConnection) -> Self {
        Self { conn }
    }

    /// Insert a new balance change
    pub async fn insert(&self, change: &BalanceChange) -> Result<i64> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            INSERT INTO {schema}.balance_changes
            (account, block_number, event_index, delta, reason, extrinsic_hash, event_pallet, event_variant, block_ts)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
            schema = schema
        );

        let row = self
            .conn
            .query_one(
                &sql,
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

        Ok(row.get(0))
    }

    /// Batch insert multiple balance changes
    pub async fn insert_batch(&self, changes: &[BalanceChange]) -> Result<u64> {
        if changes.is_empty() {
            return Ok(0);
        }

        let mut inserted = 0;

        // TODO: Use COPY for better performance with large batches
        for change in changes {
            self.insert(change).await?;
            inserted += 1;
        }

        Ok(inserted)
    }

    /// Get balance changes for an account
    pub async fn get_by_account(
        &self,
        account: &[u8],
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<BalanceChange>> {
        let schema = self.conn.schema_name()?;
        let mut sql = format!(
            r#"
            SELECT id, account, block_number, event_index, delta, reason,
                   extrinsic_hash, event_pallet, event_variant, block_ts
            FROM {schema}.balance_changes
            WHERE account = $1
            ORDER BY block_number DESC, event_index DESC
            "#,
            schema = schema
        );

        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let rows = self.conn.query(&sql, &[&account]).await?;

        Ok(rows
            .into_iter()
            .map(|row| BalanceChange {
                id: Some(row.get(0)),
                account: row.get(1),
                block_number: row.get(2),
                event_index: row.get(3),
                delta: row.get(4),
                reason: BalanceChangeReason::from_str(row.get(5)),
                extrinsic_hash: row.get(6),
                event_pallet: row.get(7),
                event_variant: row.get(8),
                block_ts: row.get(9),
            })
            .collect())
    }

    /// Get balance changes for a block
    pub async fn get_by_block(&self, block_number: i64) -> Result<Vec<BalanceChange>> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            SELECT id, account, block_number, event_index, delta, reason,
                   extrinsic_hash, event_pallet, event_variant, block_ts
            FROM {schema}.balance_changes
            WHERE block_number = $1
            ORDER BY event_index
            "#,
            schema = schema
        );

        let rows = self.conn.query(&sql, &[&block_number]).await?;

        Ok(rows
            .into_iter()
            .map(|row| BalanceChange {
                id: Some(row.get(0)),
                account: row.get(1),
                block_number: row.get(2),
                event_index: row.get(3),
                delta: row.get(4),
                reason: BalanceChangeReason::from_str(row.get(5)),
                extrinsic_hash: row.get(6),
                event_pallet: row.get(7),
                event_variant: row.get(8),
                block_ts: row.get(9),
            })
            .collect())
    }

    /// Get balance at a specific block for an account
    pub async fn get_balance_at_block(&self, account: &[u8], block_number: i64) -> Result<String> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            SELECT COALESCE(SUM(delta::NUMERIC), 0)::TEXT
            FROM {schema}.balance_changes
            WHERE account = $1 AND block_number <= $2
            "#,
            schema = schema
        );

        let row = self
            .conn
            .query_one(&sql, &[&account, &block_number])
            .await?;
        Ok(row.get(0))
    }

    /// Delete balance changes for blocks at or after a specific height
    pub async fn delete_from_block(&self, from_block: i64) -> Result<u64> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            DELETE FROM {schema}.balance_changes
            WHERE block_number >= $1
            "#,
            schema = schema
        );

        self.conn.execute(&sql, &[&from_block]).await
    }
}

/// Repository for managing chain-wide operations
pub struct ChainRepository<'a> {
    conn: &'a DbConnection,
}

impl<'a> ChainRepository<'a> {
    /// Create a new chain repository
    pub fn new(conn: &'a DbConnection) -> Self {
        Self { conn }
    }

    /// Get or create index progress record
    pub async fn get_or_create_progress(&self, chain_id: &str) -> Result<IndexProgress> {
        let schema = self.conn.schema_name()?;

        // Try to get existing progress
        let sql = format!(
            r#"
            SELECT chain_id, latest_block, latest_block_hash, latest_block_ts,
                   blocks_indexed, balance_changes_recorded, started_at, updated_at
            FROM {schema}.index_progress
            WHERE chain_id = $1
            "#,
            schema = schema
        );

        match self.conn.query_opt(&sql, &[&chain_id]).await? {
            Some(row) => Ok(IndexProgress {
                chain_id: row.get(0),
                latest_block: row.get(1),
                latest_block_hash: row.get(2),
                latest_block_ts: row.get(3),
                blocks_indexed: row.get(4),
                balance_changes_recorded: row.get(5),
                started_at: row.get(6),
                updated_at: row.get(7),
            }),
            None => {
                // Create new progress record starting from genesis (block 0)
                let now = Utc::now();
                let progress = IndexProgress {
                    chain_id: chain_id.to_string(),
                    latest_block: -1,               // Start before genesis
                    latest_block_hash: vec![0; 32], // Placeholder
                    latest_block_ts: now,
                    blocks_indexed: 0,
                    balance_changes_recorded: 0,
                    started_at: now,
                    updated_at: now,
                };

                let insert_sql = format!(
                    r#"
                    INSERT INTO {schema}.index_progress
                    (chain_id, latest_block, latest_block_hash, latest_block_ts,
                     blocks_indexed, balance_changes_recorded, started_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    "#,
                    schema = schema
                );

                self.conn
                    .execute(
                        &insert_sql,
                        &[
                            &progress.chain_id,
                            &progress.latest_block,
                            &progress.latest_block_hash,
                            &progress.latest_block_ts,
                            &progress.blocks_indexed,
                            &progress.balance_changes_recorded,
                            &progress.started_at,
                            &progress.updated_at,
                        ],
                    )
                    .await?;

                Ok(progress)
            }
        }
    }

    /// Update index progress
    pub async fn update_progress(&self, progress: &IndexProgress) -> Result<()> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
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

        self.conn
            .execute(
                &sql,
                &[
                    &progress.chain_id,
                    &progress.latest_block,
                    &progress.latest_block_hash,
                    &progress.latest_block_ts,
                    &progress.blocks_indexed,
                    &progress.balance_changes_recorded,
                    &Utc::now(),
                ],
            )
            .await?;

        Ok(())
    }

    /// Update or insert account statistics
    pub async fn update_account_stats(&self, stats: &AccountStats) -> Result<()> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            INSERT INTO {schema}.account_stats
            (account, balance, first_seen_block, last_activity_block, total_changes, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (account) DO UPDATE SET
                balance = EXCLUDED.balance,
                last_activity_block = EXCLUDED.last_activity_block,
                total_changes = EXCLUDED.total_changes,
                updated_at = EXCLUDED.updated_at
            "#,
            schema = schema
        );

        self.conn
            .execute(
                &sql,
                &[
                    &stats.account,
                    &stats.balance,
                    &stats.first_seen_block,
                    &stats.last_activity_block,
                    &stats.total_changes,
                    &Utc::now(),
                ],
            )
            .await?;

        Ok(())
    }

    /// Get account statistics
    pub async fn get_account_stats(&self, account: &[u8]) -> Result<Option<AccountStats>> {
        let schema = self.conn.schema_name()?;
        let sql = format!(
            r#"
            SELECT account, balance::TEXT, first_seen_block, last_activity_block, total_changes
            FROM {schema}.account_stats
            WHERE account = $1
            "#,
            schema = schema
        );

        match self.conn.query_opt(&sql, &[&account]).await? {
            Some(row) => Ok(Some(AccountStats {
                account: row.get(0),
                balance: row.get(1),
                first_seen_block: row.get(2),
                last_activity_block: row.get(3),
                total_changes: row.get(4),
            })),
            None => Ok(None),
        }
    }

    /// Begin a reorganization from a specific block height
    pub async fn begin_reorg(&self, from_block: i64) -> Result<()> {
        info!("Beginning reorganization from block {}", from_block);

        // Mark blocks as non-canonical
        let blocks_repo = BlockRepository::new(self.conn);
        blocks_repo.mark_non_canonical_from(from_block).await?;

        // Delete balance changes
        let changes_repo = BalanceChangeRepository::new(self.conn);
        changes_repo.delete_from_block(from_block).await?;

        // Update progress to reflect the reorg
        let chain_id = self
            .conn
            .chain_id()
            .ok_or_else(|| DbError::Configuration("Chain ID not set".into()))?;

        let mut progress = self.get_or_create_progress(&chain_id).await?;
        progress.latest_block = from_block - 1;
        self.update_progress(&progress).await?;

        Ok(())
    }
}

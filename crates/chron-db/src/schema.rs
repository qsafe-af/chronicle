use crate::{connection::DbConnection, error::Result};
use tracing::{debug, info, warn};

/// Schema manager for creating and maintaining database schemas
pub struct SchemaManager {
    chain_id: String,
    enable_timescale: bool,
}

impl SchemaManager {
    /// Create a new schema manager for a specific chain
    pub fn new(chain_id: String) -> Self {
        Self {
            chain_id,
            enable_timescale: false,
        }
    }

    /// Enable TimescaleDB features (if available)
    pub fn with_timescale(mut self, enable: bool) -> Self {
        self.enable_timescale = enable;
        self
    }

    /// Get the properly quoted schema name
    pub fn schema_name(&self) -> String {
        format!("\"{}\"", self.chain_id)
    }

    /// Initialize the complete schema for a chain
    pub async fn initialize(&self, conn: &DbConnection) -> Result<()> {
        info!("Initializing schema for chain: {}", self.chain_id);

        // Create schema
        self.create_schema(conn).await?;

        // Create tables
        self.create_blocks_table(conn).await?;
        self.create_balance_changes_table(conn).await?;
        self.create_index_progress_table(conn).await?;
        self.create_account_stats_table(conn).await?;

        // Create indexes
        self.create_indexes(conn).await?;

        // Enable TimescaleDB if requested and available
        if self.enable_timescale {
            self.setup_timescale(conn).await?;
        }

        info!(
            "Schema initialization complete for chain: {}",
            self.chain_id
        );
        Ok(())
    }

    /// Create the schema if it doesn't exist
    pub async fn create_schema(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();
        let sql = format!("CREATE SCHEMA IF NOT EXISTS {schema}");

        debug!("Creating schema: {}", schema);
        conn.execute(&sql, &[]).await?;
        Ok(())
    }

    /// Drop the schema and all its contents (USE WITH CAUTION)
    pub async fn drop_schema(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();
        let sql = format!("DROP SCHEMA IF EXISTS {schema} CASCADE");

        warn!("Dropping schema and all contents: {}", schema);
        conn.execute(&sql, &[]).await?;
        Ok(())
    }

    /// Create the blocks table
    pub async fn create_blocks_table(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();
        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {schema}.blocks (
                number BIGINT PRIMARY KEY,
                hash BYTEA NOT NULL UNIQUE,
                parent_hash BYTEA NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL,
                is_canonical BOOLEAN NOT NULL DEFAULT true,
                runtime_spec BIGINT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
            schema = schema
        );

        debug!("Creating blocks table");
        conn.batch_execute(&sql).await?;
        Ok(())
    }

    /// Create the balance_changes table
    pub async fn create_balance_changes_table(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();
        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {schema}.balance_changes (
                id BIGSERIAL PRIMARY KEY,
                account BYTEA NOT NULL,
                block_number BIGINT NOT NULL,
                event_index INT NOT NULL,
                delta NUMERIC(78,0) NOT NULL,
                reason TEXT NOT NULL,
                extrinsic_hash BYTEA,
                event_pallet TEXT NOT NULL,
                event_variant TEXT NOT NULL,
                block_ts TIMESTAMPTZ NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(block_number, event_index)
            )
            "#,
            schema = schema
        );

        debug!("Creating balance_changes table");
        conn.batch_execute(&sql).await?;
        Ok(())
    }

    /// Create the index_progress table for tracking indexing state
    pub async fn create_index_progress_table(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();
        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {schema}.index_progress (
                chain_id TEXT PRIMARY KEY,
                latest_block BIGINT NOT NULL,
                latest_block_hash BYTEA NOT NULL,
                latest_block_ts TIMESTAMPTZ NOT NULL,
                blocks_indexed BIGINT NOT NULL DEFAULT 0,
                balance_changes_recorded BIGINT NOT NULL DEFAULT 0,
                started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
            schema = schema
        );

        debug!("Creating index_progress table");
        conn.batch_execute(&sql).await?;
        Ok(())
    }

    /// Create the account_stats table for aggregated account data
    pub async fn create_account_stats_table(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();
        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {schema}.account_stats (
                account BYTEA PRIMARY KEY,
                balance NUMERIC(78,0) NOT NULL DEFAULT 0,
                first_seen_block BIGINT NOT NULL,
                last_activity_block BIGINT NOT NULL,
                total_changes BIGINT NOT NULL DEFAULT 0,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
            schema = schema
        );

        debug!("Creating account_stats table");
        conn.batch_execute(&sql).await?;
        Ok(())
    }

    /// Create indexes for better query performance
    pub async fn create_indexes(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();
        let indexes = vec![
            // Blocks indexes
            format!("CREATE INDEX IF NOT EXISTS idx_{}_blocks_timestamp ON {schema}.blocks (timestamp DESC)", self.chain_id),
            format!("CREATE INDEX IF NOT EXISTS idx_{}_blocks_canonical ON {schema}.blocks (is_canonical) WHERE is_canonical = true", self.chain_id),

            // Balance changes indexes
            format!("CREATE INDEX IF NOT EXISTS idx_{}_balance_changes_account ON {schema}.balance_changes (account)", self.chain_id),
            format!("CREATE INDEX IF NOT EXISTS idx_{}_balance_changes_block ON {schema}.balance_changes (block_number)", self.chain_id),
            format!("CREATE INDEX IF NOT EXISTS idx_{}_balance_changes_account_block ON {schema}.balance_changes (account, block_number DESC)", self.chain_id),
            format!("CREATE INDEX IF NOT EXISTS idx_{}_balance_changes_ts ON {schema}.balance_changes (block_ts DESC)", self.chain_id),
            format!("CREATE INDEX IF NOT EXISTS idx_{}_balance_changes_reason ON {schema}.balance_changes (reason)", self.chain_id),
            format!("CREATE INDEX IF NOT EXISTS idx_{}_balance_changes_extrinsic ON {schema}.balance_changes (extrinsic_hash) WHERE extrinsic_hash IS NOT NULL", self.chain_id),

            // Account stats indexes
            format!("CREATE INDEX IF NOT EXISTS idx_{}_account_stats_balance ON {schema}.account_stats (balance DESC)", self.chain_id),
            format!("CREATE INDEX IF NOT EXISTS idx_{}_account_stats_activity ON {schema}.account_stats (last_activity_block DESC)", self.chain_id),
        ];

        debug!("Creating {} indexes", indexes.len());
        for index_sql in indexes {
            if let Err(e) = conn.execute(&index_sql, &[]).await {
                warn!("Failed to create index (may already exist): {}", e);
            }
        }

        Ok(())
    }

    /// Setup TimescaleDB features if available
    pub async fn setup_timescale(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();

        // Check if TimescaleDB extension is available
        let check_sql = "SELECT * FROM pg_extension WHERE extname = 'timescaledb'";
        match conn.query_opt(check_sql, &[]).await? {
            Some(_) => {
                info!("TimescaleDB extension found, creating hypertables");

                // Create hypertable for balance_changes
                let sql = format!(
                    "SELECT create_hypertable('{schema}.balance_changes', by_range('block_ts'), if_not_exists => TRUE)",
                    schema = schema
                );

                match conn.execute(&sql, &[]).await {
                    Ok(_) => info!("Created hypertable for balance_changes"),
                    Err(e) => warn!("Failed to create hypertable (may already exist): {}", e),
                }

                // Add compression policy (compress chunks older than 30 days)
                let compress_sql = format!(
                    "SELECT add_compression_policy('{schema}.balance_changes', INTERVAL '30 days', if_not_exists => TRUE)",
                    schema = schema
                );

                match conn.execute(&compress_sql, &[]).await {
                    Ok(_) => info!("Added compression policy for balance_changes"),
                    Err(e) => warn!("Failed to add compression policy: {}", e),
                }
            }
            None => {
                warn!("TimescaleDB extension not found, skipping hypertable creation");
            }
        }

        Ok(())
    }

    /// Check if the schema exists
    pub async fn schema_exists(&self, conn: &DbConnection) -> Result<bool> {
        let sql =
            "SELECT EXISTS (SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)";
        let row = conn.query_one(sql, &[&self.chain_id]).await?;
        Ok(row.get(0))
    }

    /// Get table statistics for monitoring
    pub async fn get_table_stats(&self, conn: &DbConnection) -> Result<TableStats> {
        let schema = self.schema_name();

        // Count blocks
        let blocks_sql = format!("SELECT COUNT(*) FROM {schema}.blocks");
        let blocks_count: i64 = conn.query_one(&blocks_sql, &[]).await?.get(0);

        // Count balance changes
        let changes_sql = format!("SELECT COUNT(*) FROM {schema}.balance_changes");
        let changes_count: i64 = conn.query_one(&changes_sql, &[]).await?.get(0);

        // Count unique accounts
        let accounts_sql = format!("SELECT COUNT(DISTINCT account) FROM {schema}.balance_changes");
        let accounts_count: i64 = conn.query_one(&accounts_sql, &[]).await?.get(0);

        // Get latest block
        let latest_sql = format!("SELECT MAX(number) FROM {schema}.blocks");
        let latest_block: Option<i64> = conn.query_one(&latest_sql, &[]).await?.get(0);

        Ok(TableStats {
            blocks_count,
            balance_changes_count: changes_count,
            unique_accounts_count: accounts_count,
            latest_block_number: latest_block,
        })
    }

    /// Vacuum and analyze tables for performance
    pub async fn vacuum_analyze(&self, conn: &DbConnection) -> Result<()> {
        let schema = self.schema_name();

        info!("Running VACUUM ANALYZE on schema {}", schema);

        // Note: VACUUM cannot be run inside a transaction block
        let tables = vec!["blocks", "balance_changes", "account_stats"];

        for table in tables {
            let sql = format!("VACUUM ANALYZE {schema}.{table}");
            if let Err(e) = conn.batch_execute(&sql).await {
                warn!("Failed to vacuum analyze {}.{}: {}", schema, table, e);
            }
        }

        Ok(())
    }
}

/// Statistics about tables in a schema
#[derive(Debug, Clone)]
pub struct TableStats {
    pub blocks_count: i64,
    pub balance_changes_count: i64,
    pub unique_accounts_count: i64,
    pub latest_block_number: Option<i64>,
}

impl TableStats {
    /// Check if the schema has any data
    pub fn has_data(&self) -> bool {
        self.blocks_count > 0 || self.balance_changes_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_name() {
        let manager = SchemaManager::new("test_chain".to_string());
        assert_eq!(manager.schema_name(), "\"test_chain\"");
    }

    #[test]
    fn test_with_timescale() {
        let manager = SchemaManager::new("test".to_string()).with_timescale(true);
        assert!(manager.enable_timescale);
    }
}

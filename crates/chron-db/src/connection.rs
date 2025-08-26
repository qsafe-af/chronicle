use crate::{
    config::DbConfig,
    error::{DbError, Result},
};
use deadpool_postgres::{Client, Manager, ManagerConfig, Pool, RecyclingMethod};
use std::time::Duration;
use tokio_postgres::NoTls;
use tracing::{debug, info, warn};

/// Database connection pool wrapper
#[derive(Clone)]
pub struct ConnectionPool {
    pool: Pool,
    chain_id: Option<String>,
}

impl ConnectionPool {
    /// Create a new connection pool from configuration
    pub async fn new(config: &DbConfig) -> Result<Self> {
        let pg_config = config
            .dsn
            .parse::<tokio_postgres::Config>()
            .map_err(|e| DbError::Configuration(format!("Invalid DSN: {}", e)))?;

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let mgr = Manager::from_config(pg_config, NoTls, mgr_config);

        let pool = Pool::builder(mgr)
            .max_size(config.max_connections as usize)
            .create_timeout(Some(Duration::from_secs(config.connection_timeout_secs)))
            .wait_timeout(Some(Duration::from_secs(config.connection_timeout_secs)))
            .recycle_timeout(Some(Duration::from_secs(5)))
            .build()
            .map_err(|_| DbError::Configuration("Failed to create pool".into()))?;

        // Test the connection
        let _ = pool.get().await?;
        info!(
            "Database connection pool initialized with {} max connections",
            config.max_connections
        );

        Ok(Self {
            pool,
            chain_id: None,
        })
    }

    /// Set the chain ID for this connection pool
    pub fn set_chain_id(&mut self, chain_id: String) {
        self.chain_id = Some(chain_id);
    }

    /// Get the chain ID
    pub fn chain_id(&self) -> Option<&String> {
        self.chain_id.as_ref()
    }

    /// Get a connection from the pool
    pub async fn get(&self) -> Result<DbConnection> {
        let client = self.pool.get().await?;
        Ok(DbConnection {
            client,
            chain_id: self.chain_id.clone(),
        })
    }

    /// Get pool status
    pub fn status(&self) -> PoolStatus {
        let status = self.pool.status();
        PoolStatus {
            size: status.size,
            available: status.available,
            waiting: status.waiting,
        }
    }

    /// Check if the pool is healthy
    pub async fn health_check(&self) -> Result<bool> {
        match self.pool.get().await {
            Ok(conn) => match conn.simple_query("SELECT 1").await {
                Ok(_) => Ok(true),
                Err(e) => {
                    warn!("Health check query failed: {}", e);
                    Ok(false)
                }
            },
            Err(e) => {
                warn!("Failed to get connection for health check: {}", e);
                Ok(false)
            }
        }
    }
}

/// Wrapper around a pooled database connection
pub struct DbConnection {
    client: Client,
    chain_id: Option<String>,
}

impl DbConnection {
    /// Create from a deadpool client (for internal use)
    pub fn from_client(client: Client, chain_id: Option<String>) -> Self {
        Self { client, chain_id }
    }

    /// Get the chain ID for this connection
    pub fn chain_id(&self) -> Option<&String> {
        self.chain_id.as_ref()
    }

    /// Get the schema name for the current chain
    pub fn schema_name(&self) -> Result<String> {
        match &self.chain_id {
            Some(id) => Ok(format!("\"{}\"", id)),
            None => Err(DbError::Configuration("Chain ID not set".into())),
        }
    }

    /// Execute a statement
    pub async fn execute(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<u64> {
        debug!("Executing: {}", statement);
        self.client
            .execute(statement, params)
            .await
            .map_err(Into::into)
    }

    /// Execute a batch of statements
    pub async fn batch_execute(&self, statements: &str) -> Result<()> {
        debug!("Batch executing: {} bytes", statements.len());
        self.client
            .batch_execute(statements)
            .await
            .map_err(Into::into)
    }

    /// Query and return rows
    pub async fn query(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        debug!("Querying: {}", statement);
        self.client
            .query(statement, params)
            .await
            .map_err(Into::into)
    }

    /// Query and return a single row
    pub async fn query_one(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<tokio_postgres::Row> {
        debug!("Querying one: {}", statement);
        self.client
            .query_one(statement, params)
            .await
            .map_err(Into::into)
    }

    /// Query and return an optional single row
    pub async fn query_opt(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Option<tokio_postgres::Row>> {
        debug!("Querying optional: {}", statement);
        self.client
            .query_opt(statement, params)
            .await
            .map_err(Into::into)
    }

    /// Build and start a transaction
    pub fn build_transaction(&mut self) -> deadpool_postgres::TransactionBuilder<'_> {
        self.client.build_transaction()
    }

    /// Start a transaction with default settings
    pub async fn transaction(&mut self) -> Result<deadpool_postgres::Transaction<'_>> {
        self.client.transaction().await.map_err(Into::into)
    }

    /// Prepare a statement
    pub async fn prepare(&self, statement: &str) -> Result<tokio_postgres::Statement> {
        self.client.prepare(statement).await.map_err(Into::into)
    }

    /// Check if connection is valid
    pub async fn is_valid(&self) -> bool {
        self.client.simple_query("SELECT 1").await.is_ok()
    }
}

/// Transaction wrapper for cleaner API
pub struct TransactionWrapper<'a> {
    tx: deadpool_postgres::Transaction<'a>,
    chain_id: Option<String>,
}

impl<'a> TransactionWrapper<'a> {
    /// Create a new transaction wrapper
    pub fn new(tx: deadpool_postgres::Transaction<'a>, chain_id: Option<String>) -> Self {
        Self { tx, chain_id }
    }

    /// Get the schema name for the current chain
    pub fn schema_name(&self) -> Result<String> {
        match &self.chain_id {
            Some(id) => Ok(format!("\"{}\"", id)),
            None => Err(DbError::Configuration("Chain ID not set".into())),
        }
    }

    /// Execute a statement
    pub async fn execute(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<u64> {
        self.tx.execute(statement, params).await.map_err(Into::into)
    }

    /// Execute a batch of statements
    pub async fn batch_execute(&self, statements: &str) -> Result<()> {
        self.tx.batch_execute(statements).await.map_err(Into::into)
    }

    /// Query and return rows
    pub async fn query(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        self.tx.query(statement, params).await.map_err(Into::into)
    }

    /// Query and return a single row
    pub async fn query_one(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<tokio_postgres::Row> {
        self.tx
            .query_one(statement, params)
            .await
            .map_err(Into::into)
    }

    /// Query and return an optional single row
    pub async fn query_opt(
        &self,
        statement: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Option<tokio_postgres::Row>> {
        self.tx
            .query_opt(statement, params)
            .await
            .map_err(Into::into)
    }

    /// Commit the transaction
    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await.map_err(Into::into)
    }

    /// Rollback the transaction
    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await.map_err(Into::into)
    }
}

/// Pool status information
#[derive(Debug, Clone)]
pub struct PoolStatus {
    /// Total size of the pool
    pub size: usize,
    /// Number of available connections
    pub available: usize,
    /// Number of tasks waiting for a connection
    pub waiting: usize,
}

impl PoolStatus {
    /// Check if pool is under pressure
    pub fn is_under_pressure(&self) -> bool {
        self.available == 0 || self.waiting > 0
    }

    /// Get utilization percentage
    pub fn utilization_percent(&self) -> f64 {
        if self.size == 0 {
            return 0.0;
        }
        ((self.size - self.available) as f64 / self.size as f64) * 100.0
    }
}

// Re-export for convenience

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_status() {
        let status = PoolStatus {
            size: 10,
            available: 3,
            waiting: 0,
        };

        assert!(!status.is_under_pressure());
        assert_eq!(status.utilization_percent(), 70.0);
    }

    #[test]
    fn test_schema_name() {
        let client = unsafe { std::mem::zeroed() }; // Just for testing
        let conn = DbConnection {
            client,
            chain_id: Some("test_chain".into()),
        };

        assert_eq!(conn.schema_name().unwrap(), "\"test_chain\"");
    }
}

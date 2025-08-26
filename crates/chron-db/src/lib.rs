mod config;
mod connection;
mod error;
mod models;
mod repository;
mod schema;

pub use config::DbConfig;
pub use connection::{ConnectionPool, DbConnection, TransactionWrapper};
pub use error::{DbError, Result};
pub use models::{AccountStats, BalanceChange, BalanceChangeReason, Block, IndexProgress};
pub use repository::{BalanceChangeRepository, BlockRepository, ChainRepository};
pub use schema::SchemaManager;

// Re-export commonly used types
pub use deadpool_postgres::Transaction;

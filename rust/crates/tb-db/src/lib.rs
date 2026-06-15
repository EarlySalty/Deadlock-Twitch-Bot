//! Persistenzschicht des Twitch-Bots: sqlx-Pool, Migrationen, Row-Mapping.

pub mod error;
pub mod migrate;
pub mod pool;
pub mod retry;
pub mod rows;

pub use error::DbError;
pub use migrate::{run_migrations, MIGRATOR};
pub use pool::connect;
pub use retry::{
    repeatable_read_transaction, run_transaction, serializable_transaction, IsolationLevel,
    RetryPolicy, Tx, TxFuture,
};

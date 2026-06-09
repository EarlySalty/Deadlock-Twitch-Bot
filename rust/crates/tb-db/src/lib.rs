//! Persistenzschicht des Twitch-Bots: sqlx-Pool, Migrationen, Row-Mapping.

pub mod error;
pub mod migrate;
pub mod pool;
pub mod rows;

pub use error::DbError;
pub use migrate::{run_migrations, MIGRATOR};
pub use pool::connect;

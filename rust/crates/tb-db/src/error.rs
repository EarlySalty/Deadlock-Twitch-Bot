//! Fehler der Persistenzschicht. Eigenständig (wickelt sqlx), damit `tb-error`
//! (Fundament) nicht an sqlx gekoppelt wird.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("config error: {0}")]
    Config(#[from] tb_error::ConfigError),

    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

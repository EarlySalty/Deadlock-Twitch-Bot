//! Behaviour is loaded only from PostgreSQL. Missing or invalid settings
//! stop collection; they never enable a guessed default configuration.
use sqlx::PgPool;
use std::time::Duration;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SammlerKonfig {
    pub enabled: bool,
    pub deadlock_game_id: String,
    pub discovery_interval_seconds: i32,
    pub chat_enabled: bool,
    pub roster_decay_seconds: i32,
    pub retention_days: i32,
    pub vod_metadata_enabled: bool,
}
impl SammlerKonfig {
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.discovery_interval_seconds as u64)
    }
}

pub async fn lade(pool: &PgPool) -> Result<SammlerKonfig, sqlx::Error> {
    let cfg = sqlx::query_as::<_, SammlerKonfig>(
        "SELECT enabled, deadlock_game_id, discovery_interval_seconds, chat_enabled,
                roster_decay_seconds, retention_days, vod_metadata_enabled
         FROM category_collector_config WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    if !(1..=90).contains(&cfg.retention_days)
        || !(30..=3600).contains(&cfg.discovery_interval_seconds)
        || !(60..=900).contains(&cfg.roster_decay_seconds)
    {
        return Err(sqlx::Error::Protocol(
            "invalid category collector settings".into(),
        ));
    }
    Ok(cfg)
}

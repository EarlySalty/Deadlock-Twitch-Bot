//! Gate: Engagement nur, wenn der Channel GERADE live ist UND Deadlock streamt
//! (Port von `bot/engagement/stream_state.py`).
//!
//! Liest `twitch_live_state` (vom Live-Monitoring gepflegt: `is_live` +
//! `last_game`). So redet der Bot nicht in offline-Streams oder bei einem anderen
//! Spiel. 60s gecacht, damit nicht pro Nachricht die DB getroffen wird.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::PgPool;

const TTL: Duration = Duration::from_secs(60);

/// Live-Status-Gate für die Engagement-Pipeline.
pub struct StreamState {
    pool: PgPool,
    cache: Mutex<HashMap<String, (Instant, bool)>>,
}

impl StreamState {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, cache: Mutex::new(HashMap::new()) }
    }

    async fn check(&self, cl: &str) -> Result<bool, sqlx::Error> {
        // `is_live` ist INTEGER (DEFAULT 0); `bool(row[0])` = != 0.
        let row = sqlx::query!(
            r#"SELECT is_live AS "is_live?", last_game
               FROM twitch_live_state
               WHERE streamer_login = $1"#,
            cl
        )
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => {
                let live = row.is_live.unwrap_or(0) != 0;
                let game = row.last_game.unwrap_or_default().trim().to_lowercase();
                Ok(live && game == "deadlock")
            }
            None => Ok(false),
        }
    }

    /// True, wenn der Channel gerade live ist UND Deadlock streamt. 60s gecacht.
    pub async fn is_streaming_deadlock(&self, channel_login: &str) -> bool {
        let cl = channel_login.trim().to_lowercase();
        if cl.is_empty() {
            return false;
        }
        {
            let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((at, val)) = cache.get(&cl) {
                if at.elapsed() < TTL {
                    return *val;
                }
            }
        }
        match self.check(&cl).await {
            Ok(val) => {
                let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
                cache.insert(cl, (Instant::now(), val));
                val
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    channel = cl,
                    "Engagement: stream-state DB-Fehler - fail-closed (false)"
                );
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn closed_pool() -> PgPool {
        let pool = PgPoolOptions::new().max_connections(1).connect_lazy_with(PgConnectOptions::new());
        pool.close().await;
        pool
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_live_state (\
             twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, \
             is_live INTEGER DEFAULT 0, last_game TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn gate_live_und_deadlock() {
        let Some(pool) = make_pool("t_eng_streamstate").await else { return };
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES \
             ('1','live_dl', 1, 'Deadlock'), \
             ('2','live_other', 1, 'Just Chatting'), \
             ('3','offline_dl', 0, 'Deadlock')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let ss = StreamState::new(pool);
        // live + Deadlock (case-insensitiv) → true
        assert!(ss.is_streaming_deadlock("live_dl").await);
        // Großschreibung im Input wird lowercased
        assert!(ss.is_streaming_deadlock("Live_DL").await);
        // live aber anderes Spiel → false
        assert!(!ss.is_streaming_deadlock("live_other").await);
        // Deadlock aber offline → false
        assert!(!ss.is_streaming_deadlock("offline_dl").await);
        // kein Eintrag → false
        assert!(!ss.is_streaming_deadlock("unbekannt").await);
        // leer → false
        assert!(!ss.is_streaming_deadlock("  ").await);
    }

    #[tokio::test]
    async fn db_error_fail_closed_und_wird_nicht_gecached() {
        let ss = StreamState::new(closed_pool().await);
        assert!(!ss.is_streaming_deadlock("live_dl").await);
        let cache = ss.cache.lock().unwrap_or_else(|p| p.into_inner());
        assert!(cache.is_empty());
    }
}

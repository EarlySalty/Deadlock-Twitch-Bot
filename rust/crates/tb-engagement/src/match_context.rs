//! Live-Match-Kontext (Deadlock) für den Engagement-Layer (Port von
//! `bot/engagement/match_context.py`).
//!
//! Die Pipeline liest den Snapshot synchron aus `twitch_channel_match_state`
//! ([`MatchContext::get_match_state`]) und hängt einen kurzen „Streamer spielt
//! aktuell X"-Hint in den System-Prompt. Der Hintergrund-Poll (API → DB,
//! `poll_match_state`) folgt in 12b.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Snapshot des aktuellen Match-States eines Channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchSnapshot {
    pub channel_login: String,
    pub hero_id: Option<i64>,
    pub hero_name: Option<String>,
    pub match_id: Option<String>,
    pub match_started_at: Option<DateTime<Utc>>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub is_live: bool,
}

impl MatchSnapshot {
    /// Prompt-Hint, wenn der Streamer gerade spielt; sonst "".
    pub fn to_prompt_fragment(&self) -> String {
        self.fragment_at(Utc::now())
    }

    /// Wie [`Self::to_prompt_fragment`], aber mit explizitem „jetzt" (testbar).
    fn fragment_at(&self, now: DateTime<Utc>) -> String {
        if !self.is_live {
            return String::new();
        }
        let hero = match self.hero_name.as_deref().filter(|n| !n.is_empty()) {
            Some(name) => name.to_string(),
            None => match self.hero_id {
                Some(id) => format!("Hero #{id}"),
                None => "einem unbekannten Hero".to_string(),
            },
        };
        let duration = match self.match_started_at {
            Some(started) => {
                let elapsed_min = (now - started).num_seconds() / 60;
                format!(" Match läuft seit ~{elapsed_min} Min.")
            }
            None => String::new(),
        };
        format!("Streamer spielt aktuell {hero}.{duration}")
    }
}

/// Match-Kontext-Provider.
pub struct MatchContext {
    pool: PgPool,
}

impl MatchContext {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Lädt den Match-Snapshot eines Channels aus der DB (oder None).
    pub async fn get_match_state(&self, channel_login: &str) -> Option<MatchSnapshot> {
        type Row = (
            String,
            Option<i32>,
            Option<String>,
            Option<String>,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            bool,
        );
        let row = sqlx::query_as::<_, Row>(
            "SELECT channel_login, hero_id, hero_name, match_id, \
                    match_started_at, last_synced_at, is_live \
             FROM twitch_channel_match_state WHERE channel_login = $1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;
        let (channel_login, hero_id, hero_name, match_id, match_started_at, last_synced_at, is_live) =
            row;
        Some(MatchSnapshot {
            channel_login,
            hero_id: hero_id.map(i64::from),
            hero_name,
            match_id: match_id.filter(|s| !s.is_empty()),
            match_started_at,
            last_synced_at,
            is_live,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn snap(is_live: bool, hero_name: Option<&str>, hero_id: Option<i64>, started: Option<DateTime<Utc>>) -> MatchSnapshot {
        MatchSnapshot {
            channel_login: "nani".to_string(),
            hero_id,
            hero_name: hero_name.map(str::to_string),
            match_id: None,
            match_started_at: started,
            last_synced_at: None,
            is_live,
        }
    }

    #[test]
    fn fragment_varianten() {
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        // nicht live → leer
        assert_eq!(snap(false, Some("Haze"), None, None).fragment_at(now), "");
        // live mit Name + Dauer
        let s = snap(true, Some("Haze"), Some(5), Some(now - Duration::minutes(30)));
        assert_eq!(s.fragment_at(now), "Streamer spielt aktuell Haze. Match läuft seit ~30 Min.");
        // live ohne Name, mit id
        let s2 = snap(true, None, Some(7), None);
        assert_eq!(s2.fragment_at(now), "Streamer spielt aktuell Hero #7.");
        // live ohne Name + ohne id
        let s3 = snap(true, None, None, None);
        assert_eq!(s3.fragment_at(now), "Streamer spielt aktuell einem unbekannten Hero.");
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
            "CREATE TABLE twitch_channel_match_state (\
             channel_login TEXT PRIMARY KEY, hero_id INT, hero_name TEXT, match_id TEXT, \
             match_started_at TIMESTAMPTZ, last_synced_at TIMESTAMPTZ NOT NULL, \
             is_live BOOLEAN NOT NULL DEFAULT FALSE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn get_match_state_aus_db() {
        let Some(pool) = make_pool("t_eng_match").await else { return };
        sqlx::query(
            "INSERT INTO twitch_channel_match_state \
             (channel_login, hero_id, hero_name, match_id, match_started_at, last_synced_at, is_live) \
             VALUES ('nani', 5, 'Haze', 'm1', NOW(), NOW(), TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let ctx = MatchContext::new(pool);
        let s = ctx.get_match_state("nani").await.unwrap();
        assert_eq!(s.hero_id, Some(5));
        assert_eq!(s.hero_name.as_deref(), Some("Haze"));
        assert!(s.is_live);
        // unbekannter Channel → None
        assert!(ctx.get_match_state("other").await.is_none());
    }
}

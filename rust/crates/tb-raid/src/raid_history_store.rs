//! Raid-History-Store (`twitch_raid_history`) — Schreib- und Lesepfad für
//! ausgeführte Raids. Schritt 6d.
//!
//! Prod-Schema (verifiziert):
//!
//! | Spalte                    | Typ          | Rust                    |
//! |---------------------------|--------------|-------------------------|
//! | id                        | bigint       | i64 (FromRow)           |
//! | from_broadcaster_id       | text         | String                  |
//! | from_broadcaster_login    | text         | String                  |
//! | to_broadcaster_id         | text         | String                  |
//! | to_broadcaster_login      | text         | String                  |
//! | viewer_count              | int          | i32                     |
//! | stream_duration_sec       | int          | i32                     |
//! | reason                    | text         | Option<String>          |
//! | executed_at               | timestamptz  | DateTime<Utc>           |
//! | success                   | boolean      | bool                    |
//! | error_message             | text         | Option<String>          |
//! | target_stream_started_at  | timestamptz  | Option<DateTime<Utc>>   |
//! | candidates_count          | int          | i32                     |

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Eingabe für [`RaidHistoryStore::record_raid`].
#[derive(Debug, Clone)]
pub struct RecordRaidInput {
    pub from_broadcaster_id: String,
    pub from_broadcaster_login: String,
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub viewer_count: i32,
    pub stream_duration_sec: i32,
    /// Optionaler fachlicher Grund (z. B. `"auto_raid_on_offline"`).
    pub reason: Option<String>,
    pub success: bool,
    pub error_message: Option<String>,
    /// Wann der Ziel-Stream gestartet ist (aus Twitch-Live-Daten).
    pub target_stream_started_at: Option<DateTime<Utc>>,
    /// Anzahl Kandidaten, die zur Auswahl standen.
    pub candidates_count: i32,
}

/// Schreib-/Lesezugriff auf `twitch_raid_history`.
#[derive(Clone)]
pub struct RaidHistoryStore {
    pool: PgPool,
}

impl RaidHistoryStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Schreibt einen Raid-Eintrag in die History.
    ///
    /// `executed_at` wird serverseitig auf `NOW()` gesetzt — kein
    /// Clock-Skew durch den Bot-Prozess.
    pub async fn record_raid(&self, input: &RecordRaidInput) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO twitch_raid_history (
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                viewer_count,
                stream_duration_sec,
                reason,
                executed_at,
                success,
                error_message,
                target_stream_started_at,
                candidates_count
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), $8, $9, $10, $11)
            RETURNING id
            "#,
        )
        .bind(&input.from_broadcaster_id)
        .bind(&input.from_broadcaster_login)
        .bind(&input.to_broadcaster_id)
        .bind(&input.to_broadcaster_login)
        .bind(input.viewer_count)
        .bind(input.stream_duration_sec)
        .bind(&input.reason)
        .bind(input.success)
        .bind(&input.error_message)
        .bind(input.target_stream_started_at)
        .bind(input.candidates_count)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Gibt die IDs aller Ziele zurück, die `from_broadcaster_id` in den
    /// letzten `days` Tagen erfolgreich angeraidet hat.
    ///
    /// Port von `CandidateSelector.get_recent_raid_targets` in
    /// `bot/raid/services/candidate_selection.py` (Z. 171–208).
    ///
    /// SQL-Äquivalent (Python Z. 191–198):
    /// ```sql
    /// SELECT DISTINCT to_broadcaster_id
    /// FROM twitch_raid_history
    /// WHERE from_broadcaster_id = $1
    ///   AND COALESCE(success, FALSE) IS TRUE
    ///   AND executed_at >= NOW() - ($2 days)::interval
    /// ```
    pub async fn get_recent_raid_targets(
        &self,
        from_broadcaster_id: &str,
        days: i32,
    ) -> Result<HashSet<String>, sqlx::Error> {
        if from_broadcaster_id.trim().is_empty() || days <= 0 {
            return Ok(HashSet::new());
        }

        // Postgres: `($2 || ' days')::interval` — interpolierter String ist
        // hier sicher, weil `days` ein typisierter i32 ist (kein User-Input).
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT to_broadcaster_id
            FROM twitch_raid_history
            WHERE from_broadcaster_id = $1
              AND COALESCE(success, FALSE) IS TRUE
              AND executed_at >= NOW() - (($2 || ' days')::interval)
            "#,
        )
        .bind(from_broadcaster_id.trim())
        .bind(days.to_string())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }
}

// ---------------------------------------------------------------------------
// Hermetische Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Legt ein frisches Schema + Tabelle an; gibt einen Pool zurück, der
    /// `search_path` auf dieses Schema setzt.
    async fn setup_db(schema: &str) -> PgPool {
        let url =
            std::env::var("TB_TEST_DATABASE_URL").expect("TB_TEST_DATABASE_URL muss gesetzt sein");

        let admin = sqlx::PgPool::connect(&url)
            .await
            .expect("Test-DB-Verbindung fehlgeschlagen");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();

        let pool = sqlx::PgPool::connect(&format!("{url}?options=-c%20search_path%3D{schema}"))
            .await
            .expect("Pool mit Schema fehlgeschlagen");

        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_history (
                id                       BIGSERIAL PRIMARY KEY,
                from_broadcaster_id      TEXT NOT NULL,
                from_broadcaster_login   TEXT NOT NULL,
                to_broadcaster_id        TEXT NOT NULL,
                to_broadcaster_login     TEXT NOT NULL,
                viewer_count             INTEGER NOT NULL DEFAULT 0,
                stream_duration_sec      INTEGER NOT NULL DEFAULT 0,
                reason                   TEXT,
                executed_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success                  BOOLEAN,
                error_message            TEXT,
                target_stream_started_at TIMESTAMPTZ,
                candidates_count         INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    fn sample_input() -> RecordRaidInput {
        RecordRaidInput {
            from_broadcaster_id: "from_001".to_string(),
            from_broadcaster_login: "streamer_a".to_string(),
            to_broadcaster_id: "to_001".to_string(),
            to_broadcaster_login: "streamer_b".to_string(),
            viewer_count: 42,
            stream_duration_sec: 3600,
            reason: Some("auto_raid_on_offline".to_string()),
            success: true,
            error_message: None,
            target_stream_started_at: None,
            candidates_count: 5,
        }
    }

    #[tokio::test]
    async fn record_raid_gibt_id_zurueck() {
        let pool = setup_db("rhs_record").await;
        let store = RaidHistoryStore::new(pool);
        let id = store.record_raid(&sample_input()).await.unwrap();
        assert!(id > 0, "id muss positiv sein, war: {id}");
    }

    #[tokio::test]
    async fn record_raid_schreibt_felder_korrekt() {
        let pool = setup_db("rhs_fields").await;
        let store = RaidHistoryStore::new(pool.clone());
        let input = sample_input();
        let id = store.record_raid(&input).await.unwrap();

        let row: (String, String, bool, Option<String>, i32) = sqlx::query_as(
            "SELECT from_broadcaster_id, to_broadcaster_login, success, reason, candidates_count
             FROM twitch_raid_history WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, "from_001");
        assert_eq!(row.1, "streamer_b");
        assert!(row.2, "success muss TRUE sein");
        assert_eq!(row.3.as_deref(), Some("auto_raid_on_offline"));
        assert_eq!(row.4, 5);
    }

    #[tokio::test]
    async fn get_recent_targets_findet_erfolgreichen_raid() {
        let pool = setup_db("rhs_recent_ok").await;
        let store = RaidHistoryStore::new(pool);
        store.record_raid(&sample_input()).await.unwrap();

        let targets = store.get_recent_raid_targets("from_001", 7).await.unwrap();
        assert!(
            targets.contains("to_001"),
            "to_001 muss in recent targets sein"
        );
    }

    #[tokio::test]
    async fn get_recent_targets_ignoriert_fehlgeschlagenen_raid() {
        let pool = setup_db("rhs_recent_fail").await;
        let store = RaidHistoryStore::new(pool);
        let mut input = sample_input();
        input.success = false;
        input.error_message = Some("cannot be raided".to_string());
        store.record_raid(&input).await.unwrap();

        let targets = store.get_recent_raid_targets("from_001", 7).await.unwrap();
        assert!(
            !targets.contains("to_001"),
            "fehlgeschlagener Raid darf nicht in recent targets erscheinen"
        );
    }

    #[tokio::test]
    async fn get_recent_targets_leere_eingaben_geben_leeres_set() {
        let pool = setup_db("rhs_empty").await;
        let store = RaidHistoryStore::new(pool);
        assert!(store
            .get_recent_raid_targets("", 7)
            .await
            .unwrap()
            .is_empty());
        assert!(store
            .get_recent_raid_targets("from_001", 0)
            .await
            .unwrap()
            .is_empty());
        assert!(store
            .get_recent_raid_targets("  ", 7)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn get_recent_targets_mehrere_ziele_dedupliziert() {
        let pool = setup_db("rhs_dedup").await;
        let store = RaidHistoryStore::new(pool);

        // Dasselbe Ziel zweimal anraiden — soll nur einmal im Set erscheinen.
        store.record_raid(&sample_input()).await.unwrap();
        store.record_raid(&sample_input()).await.unwrap();

        // Zweites Ziel
        let mut input2 = sample_input();
        input2.to_broadcaster_id = "to_002".to_string();
        store.record_raid(&input2).await.unwrap();

        let targets = store.get_recent_raid_targets("from_001", 7).await.unwrap();
        assert_eq!(targets.len(), 2, "exakt 2 eindeutige Ziele erwartet");
        assert!(targets.contains("to_001"));
        assert!(targets.contains("to_002"));
    }

    #[tokio::test]
    async fn get_recent_targets_nur_vom_eigenen_broadcaster() {
        let pool = setup_db("rhs_own_broadcaster").await;
        let store = RaidHistoryStore::new(pool);

        // Raid von from_001
        store.record_raid(&sample_input()).await.unwrap();

        // Raid von anderem Broadcaster
        let mut other = sample_input();
        other.from_broadcaster_id = "from_999".to_string();
        other.to_broadcaster_id = "to_999".to_string();
        store.record_raid(&other).await.unwrap();

        let targets = store.get_recent_raid_targets("from_001", 7).await.unwrap();
        assert!(
            !targets.contains("to_999"),
            "Ziele anderer Broadcaster dürfen nicht erscheinen"
        );
    }
}

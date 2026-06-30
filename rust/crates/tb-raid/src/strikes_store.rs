//! Strikes-Store (`twitch_raid_disabled_strikes`) — zählt wie oft ein Kanal
//! einen Raid abgelehnt hat. Schritt 6d.
//!
//! Prod-Schema (verifiziert):
//!
//! | Spalte        | Typ          | Rust           |
//! |---------------|--------------|----------------|
//! | target_id     | text         | Option<String> |
//! | target_login  | text         | String (PK)    |
//! | strike_count  | int          | i32            |
//! | last_seen_at  | timestamptz  | DateTime<Utc>  |
//! | last_reason   | text         | Option<String> |
//!
//! UPSERT-Konflikt auf `target_login` (exakt wie Python `_increment_raid_disabled_strikes`
//! in `bot/raid/services/raid_blacklist.py`, Z. 530–541).

use sqlx::PgPool;

/// Schreibzugriff auf `twitch_raid_disabled_strikes`.
#[derive(Clone)]
pub struct StrikesStore {
    pool: PgPool,
}

impl StrikesStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Erhöht den Strike-Zähler für ein Raid-Ziel um 1 (UPSERT).
    ///
    /// Port von `_increment_raid_disabled_strikes` in
    /// `bot/raid/services/raid_blacklist.py` (Z. 530–541).
    ///
    /// UPSERT-Logik (Python Z. 534–541):
    /// - INSERT mit `strike_count = 1`, `last_seen_at = NOW()`.
    /// - Bei Konflikt auf `target_login`:
    ///   - `target_id` = `COALESCE(EXCLUDED.target_id, bestehender Wert)`
    ///   - `strike_count` += 1
    ///   - `last_seen_at` = NOW()
    ///   - `last_reason` = neuer Wert
    /// - Gibt den neuen `strike_count` zurück.
    ///
    /// Python-Fallback: bei fehlendem DB-Ergebnis → 2. Hier wird stattdessen
    /// ein `sqlx::Error` propagiert; Fallback-Logik liegt beim Aufrufer.
    pub async fn increment(
        &self,
        target_id: Option<&str>,
        target_login: &str,
        reason: &str,
    ) -> Result<i32, sqlx::Error> {
        // target_id: leerer String → NULL (identisch zu Python `target_id or None`).
        let tid: Option<&str> = target_id.filter(|s| !s.trim().is_empty());

        let strike_count: i32 = sqlx::query_scalar!(
            r#"
            INSERT INTO twitch_raid_disabled_strikes
                (target_id, target_login, strike_count, last_seen_at, last_reason)
            VALUES ($1, $2, 1, NOW(), $3)
            ON CONFLICT (target_login) DO UPDATE SET
                target_id    = COALESCE(EXCLUDED.target_id, twitch_raid_disabled_strikes.target_id),
                strike_count = twitch_raid_disabled_strikes.strike_count + 1,
                last_seen_at = NOW(),
                last_reason  = EXCLUDED.last_reason
            RETURNING strike_count AS "strike_count!"
            "#,
            tid,
            target_login,
            reason
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(strike_count)
    }
}

// ---------------------------------------------------------------------------
// Hermetische Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_db(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());

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
            CREATE TABLE twitch_raid_disabled_strikes (
                target_id    TEXT,
                target_login TEXT NOT NULL,
                strike_count INTEGER NOT NULL DEFAULT 1,
                last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_reason  TEXT,
                CONSTRAINT twitch_raid_disabled_strikes_pkey PRIMARY KEY (target_login)
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn erster_strike_gibt_eins_zurueck() {
        let pool = setup_db("ss_first").await;
        let store = StrikesStore::new(pool);
        let count = store
            .increment(Some("uid_001"), "streamer_b", "raids_are_disabled")
            .await
            .unwrap();
        assert_eq!(count, 1, "erster Strike muss 1 zurückgeben");
    }

    #[tokio::test]
    async fn zweiter_strike_gibt_zwei_zurueck() {
        let pool = setup_db("ss_second").await;
        let store = StrikesStore::new(pool);
        store
            .increment(Some("uid_001"), "streamer_b", "raids_are_disabled")
            .await
            .unwrap();
        let count = store
            .increment(Some("uid_001"), "streamer_b", "cannot be raided")
            .await
            .unwrap();
        assert_eq!(count, 2, "zweiter Strike muss 2 zurückgeben");
    }

    #[tokio::test]
    async fn strike_count_akkumuliert_korrekt() {
        let pool = setup_db("ss_accum").await;
        let store = StrikesStore::new(pool.clone());

        for i in 1..=5_i32 {
            let count = store
                .increment(Some("uid_001"), "streamer_b", "reason")
                .await
                .unwrap();
            assert_eq!(count, i, "Strike {i} muss {i} zurückgeben");
        }
    }

    #[tokio::test]
    async fn target_id_null_bei_leerem_string() {
        let pool = setup_db("ss_null_id").await;
        let store = StrikesStore::new(pool.clone());

        // Leerer String soll als NULL gespeichert werden.
        store
            .increment(Some(""), "streamer_b", "reason")
            .await
            .unwrap();

        let tid: Option<Option<String>> = sqlx::query_scalar(
            "SELECT target_id FROM twitch_raid_disabled_strikes WHERE target_login = 'streamer_b'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(
            tid.flatten(),
            None,
            "leerer target_id String muss als NULL gespeichert werden"
        );
    }

    #[tokio::test]
    async fn upsert_aktualisiert_last_reason() {
        let pool = setup_db("ss_reason").await;
        let store = StrikesStore::new(pool.clone());

        store
            .increment(Some("uid_001"), "streamer_b", "erster_grund")
            .await
            .unwrap();
        store
            .increment(Some("uid_001"), "streamer_b", "zweiter_grund")
            .await
            .unwrap();

        let reason: Option<String> =
            sqlx::query_scalar("SELECT last_reason FROM twitch_raid_disabled_strikes WHERE target_login = 'streamer_b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(reason.as_deref(), Some("zweiter_grund"));
    }

    #[tokio::test]
    async fn verschiedene_logins_unabhaengig() {
        let pool = setup_db("ss_indep").await;
        let store = StrikesStore::new(pool);

        store
            .increment(Some("uid_001"), "streamer_a", "reason")
            .await
            .unwrap();
        store
            .increment(Some("uid_001"), "streamer_a", "reason")
            .await
            .unwrap();
        let count_b = store
            .increment(Some("uid_002"), "streamer_b", "reason")
            .await
            .unwrap();
        assert_eq!(
            count_b, 1,
            "streamer_b hat nur einen Strike, unabhängig von streamer_a"
        );
    }
}

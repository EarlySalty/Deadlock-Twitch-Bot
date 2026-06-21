//! Ad-Schedule-Snapshot-Writer (Port: `mixin.py:_collect_ads_schedule_for_user`,
//! 833–917).
//!
//! Holt den Werbe-Schedule eines Broadcasters via Helix (`GET /channels/ads`),
//! normalisiert die Zeit-Felder auf ISO-8601 (UTC) — exakt wie der Python-Poller
//! über `_safe_time_text` — und schreibt eine Zeile in
//! `twitch_ads_schedule_snapshot` (9 Spalten). Der 6h-Loop, der diese Funktion je
//! Live-Partner tickt, wird in `bin/tb-bot` verdrahtet (WIRING-TODO).
//!
//! **Zeit-Normalisierung (Befund P1.22):** Helix liefert `next_ad_at`/`last_ad_at`/
//! `snooze_refresh_at` als Unix-Sekunden-Zahl ODER ISO-String. Roh gespeichert
//! würden Epoch-Strings das Dashboard-Rendering/-Sortieren brechen. Die
//! Normalisierung (`tb_transport_twitch::streams::normalize_ad_time`) wandelt
//! Epochs zu ISO, teilt Millisekunden durch 1000 und verwirft `ts <= 0`.

use sqlx::PgPool;
use tb_transport_twitch::streams::normalize_ad_time;
use tb_transport_twitch::{AdSchedule, HelixClient, HelixError};

/// Holt den Ad-Schedule via Helix und schreibt einen Snapshot. `Ok(false)` =
/// Helix lieferte kein `data[0]` (kein Schedule) → nichts geschrieben (Parität
/// zum Python-`return False`-Pfad). `Ok(true)` = Snapshot geschrieben.
pub async fn collect_ads_schedule_for_user(
    pool: &PgPool,
    helix: &HelixClient,
    user_id: &str,
    login: &str,
    user_token: &str,
) -> Result<bool, CollectError> {
    let schedule = match helix.get_ad_schedule(user_id, user_token).await {
        Ok(Some(schedule)) => schedule,
        Ok(None) => return Ok(false),
        Err(err) => return Err(CollectError::Helix(err)),
    };
    write_ads_schedule_snapshot(pool, user_id, login, &schedule).await?;
    Ok(true)
}

/// Schreibt einen Ad-Schedule-Snapshot mit normalisierten Zeit-Feldern.
///
/// Spalten (9, Python-Parität): `twitch_user_id, twitch_login, next_ad_at,
/// last_ad_at, duration, preroll_free_time, snooze_count, snooze_refresh_at,
/// snapshot_at`. Zeit-Felder werden über [`normalize_ad_time`] zu ISO-8601 (UTC)
/// gemacht; `snapshot_at` ist `NOW()` als ISO-String (TIMESTAMPTZ-clean).
pub async fn write_ads_schedule_snapshot(
    pool: &PgPool,
    user_id: &str,
    login: &str,
    schedule: &AdSchedule,
) -> Result<(), CollectError> {
    let next_ad_at = schedule.next_ad_at.as_deref().and_then(normalize_ad_time);
    let last_ad_at = schedule.last_ad_at.as_deref().and_then(normalize_ad_time);
    let snooze_refresh_at = schedule
        .snooze_refresh_at
        .as_deref()
        .and_then(normalize_ad_time);

    sqlx::query(
        "INSERT INTO twitch_ads_schedule_snapshot \
         (twitch_user_id, twitch_login, next_ad_at, last_ad_at, duration, \
          preroll_free_time, snooze_count, snooze_refresh_at, snapshot_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, to_char(NOW() AT TIME ZONE 'UTC', \
                 'YYYY-MM-DD\"T\"HH24:MI:SS.US+00:00'))",
    )
    .bind(user_id)
    .bind(login)
    .bind(next_ad_at)
    .bind(last_ad_at)
    .bind(schedule.duration)
    .bind(schedule.preroll_free_time)
    .bind(schedule.snooze_count)
    .bind(snooze_refresh_at)
    .execute(pool)
    .await
    .map_err(CollectError::Db)?;
    Ok(())
}

/// Fehler des Ad-Schedule-Collectors.
#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    /// Helix-Anfrage fehlgeschlagen.
    #[error("helix: {0}")]
    Helix(#[from] HelixError),
    /// DB-Insert fehlgeschlagen.
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use tb_transport_twitch::AdSchedule;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("create schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");
        // Schema wie in der Baseline-Migration (Zeit-Felder = TEXT/legacy).
        sqlx::query(
            r#"
            CREATE TABLE twitch_ads_schedule_snapshot (
                id                SERIAL PRIMARY KEY,
                twitch_user_id    TEXT NOT NULL,
                twitch_login      TEXT,
                next_ad_at        TEXT,
                last_ad_at        TEXT,
                duration          INTEGER,
                preroll_free_time INTEGER,
                snooze_count      INTEGER,
                snooze_refresh_at TEXT,
                snapshot_at       TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("ddl");
        pool
    }

    #[tokio::test]
    async fn writer_normalisiert_epoch_zu_iso() {
        let Some(dsn) = test_dsn() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = make_pool(&dsn, "test_ads_iso").await;

        // Helix liefert next_ad_at als Millisekunden-Epoch-Zahl (-> String "1750000000000").
        let schedule = AdSchedule {
            duration: 60,
            next_ad_at: Some("1750000000000".to_string()),
            last_ad_at: Some("1749990000".to_string()),
            preroll_free_time: 90,
            snooze_count: 2,
            snooze_refresh_at: Some("0".to_string()), // ts<=0 -> gedroppt
        };
        write_ads_schedule_snapshot(&pool, "42", "kanal", &schedule)
            .await
            .expect("write");

        let row: (String, String, Option<String>, Option<String>, i32, i32, i32, Option<String>) =
            sqlx::query_as(
                "SELECT twitch_user_id, twitch_login, next_ad_at, last_ad_at, duration, \
                 preroll_free_time, snooze_count, snooze_refresh_at \
                 FROM twitch_ads_schedule_snapshot ORDER BY id DESC LIMIT 1",
            )
            .fetch_one(&pool)
            .await
            .expect("row");

        assert_eq!(row.0, "42");
        assert_eq!(row.1, "kanal");
        // next_ad_at: ms -> s -> ISO (kein roher Zahlenstring).
        assert_eq!(row.2.as_deref(), Some("2025-06-15T15:06:40+00:00"));
        assert!(
            !row.2.as_deref().unwrap().chars().all(|c| c.is_ascii_digit()),
            "ISO-String statt roher Epoch-Zahl"
        );
        // last_ad_at: Sekunden-Epoch -> ISO.
        assert_eq!(row.3.as_deref(), Some("2025-06-15T12:20:00+00:00"));
        assert_eq!(row.4, 60);
        assert_eq!(row.5, 90);
        assert_eq!(row.6, 2);
        // snooze_refresh_at ts<=0 -> gedroppt (NULL).
        assert_eq!(row.7, None);
    }

    #[tokio::test]
    async fn writer_schreibt_snapshot_at_als_iso() {
        let Some(dsn) = test_dsn() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = make_pool(&dsn, "test_ads_snapat").await;
        let schedule = AdSchedule::default();
        write_ads_schedule_snapshot(&pool, "7", "x", &schedule)
            .await
            .expect("write");
        let snapshot_at: String =
            sqlx::query_scalar("SELECT snapshot_at FROM twitch_ads_schedule_snapshot LIMIT 1")
                .fetch_one(&pool)
                .await
                .expect("snapshot_at");
        // ISO-8601 UTC (TIMESTAMPTZ-clean): enthält 'T' und endet auf +00:00.
        assert!(snapshot_at.contains('T'), "snapshot_at ISO: {snapshot_at}");
        assert!(snapshot_at.ends_with("+00:00"), "UTC-Offset: {snapshot_at}");
    }
}

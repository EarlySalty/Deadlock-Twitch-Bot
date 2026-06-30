//! Subscriptions-Snapshot-Poller (Port: `mixin.py:_collect_subs_for_user`,
//! 807–831).
//!
//! Holt die Sub-Übersicht eines Broadcasters via Helix (`GET /subscriptions`,
//! `total`/`points` aus der Wurzel) und schreibt eine Zeile in
//! `twitch_subscriptions_snapshot` (5 Spalten, wie Python: `twitch_user_id,
//! twitch_login, total, points, snapshot_at`). Der 6h-Loop, der diese Funktion je
//! Live-Partner tickt, wird in `bin/tb-bot` verdrahtet (WIRING-TODO).

use sqlx::PgPool;
use tb_transport_twitch::{BroadcasterSubscriptions, HelixClient, HelixError};

/// Holt die Sub-Übersicht via Helix und schreibt einen Snapshot.
pub async fn collect_subs_for_user(
    pool: &PgPool,
    helix: &HelixClient,
    user_id: &str,
    login: &str,
    user_token: &str,
) -> Result<(), CollectError> {
    let subs = helix
        .get_broadcaster_subscriptions(user_id, user_token)
        .await
        .map_err(CollectError::Helix)?;
    write_subs_snapshot(pool, user_id, login, &subs).await
}

/// Schreibt einen Subscriptions-Snapshot.
///
/// Spalten (Python-Parität): `twitch_user_id, twitch_login, total, points,
/// snapshot_at`. `snapshot_at` ist `NOW()`; tier1/2/3 bleiben auf ihren
/// Spalten-Defaults (Python schreibt sie nicht).
pub async fn write_subs_snapshot(
    pool: &PgPool,
    user_id: &str,
    login: &str,
    subs: &BroadcasterSubscriptions,
) -> Result<(), CollectError> {
    let total = i32::try_from(subs.total).map_err(|_| {
        CollectError::Db(sqlx::Error::InvalidArgument(format!(
            "subscriptions total out of int4 range: {}",
            subs.total
        )))
    })?;
    let points = i32::try_from(subs.points).map_err(|_| {
        CollectError::Db(sqlx::Error::InvalidArgument(format!(
            "subscriptions points out of int4 range: {}",
            subs.points
        )))
    })?;

    sqlx::query!(
        "INSERT INTO twitch_subscriptions_snapshot \
         (twitch_user_id, twitch_login, total, points, snapshot_at) \
         VALUES ($1, $2, $3, $4, NOW())",
        user_id,
        login,
        total,
        points
    )
    .execute(pool)
    .await
    .map_err(CollectError::Db)?;
    Ok(())
}

/// Fehler des Subs-Snapshot-Collectors.
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
        sqlx::query(
            r#"
            CREATE TABLE twitch_subscriptions_snapshot (
                id             SERIAL PRIMARY KEY,
                twitch_user_id TEXT NOT NULL,
                twitch_login   TEXT,
                total          INTEGER DEFAULT 0,
                tier1          INTEGER DEFAULT 0,
                tier2          INTEGER DEFAULT 0,
                tier3          INTEGER DEFAULT 0,
                points         INTEGER DEFAULT 0,
                snapshot_at    TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("ddl");
        pool
    }

    #[tokio::test]
    async fn writer_schreibt_total_points_und_iso_snapshot() {
        let Some(dsn) = test_dsn() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = make_pool(&dsn, "test_subs_write").await;

        let subs = BroadcasterSubscriptions {
            data: Vec::new(),
            total: 137,
            points: 412,
        };
        write_subs_snapshot(&pool, "99", "partner", &subs)
            .await
            .expect("write");

        let row: (String, String, i32, i32, String) = sqlx::query_as(
            "SELECT twitch_user_id, twitch_login, total, points, \
             to_char(snapshot_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US+00:00') \
             FROM twitch_subscriptions_snapshot ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("row");

        assert_eq!(row.0, "99");
        assert_eq!(row.1, "partner");
        assert_eq!(row.2, 137, "total");
        assert_eq!(row.3, 412, "points");
        assert!(
            row.4.contains('T') && row.4.ends_with("+00:00"),
            "ISO UTC: {}",
            row.4
        );
    }
}

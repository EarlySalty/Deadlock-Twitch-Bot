//! Queries für `GET /twitch/api/admin/system/health`.
//!
//! Liefert letzten DB-Tick aus `twitch_live_state` und
//! Raw-Chat-Ingest-Gesundheit aus `twitch_raw_chat_ingest_health`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Letzter bekannter Tick aus `twitch_live_state`.
///
/// Gibt `MAX(COALESCE(last_seen_at, last_started_at))` zurück,
/// oder `None` wenn die Tabelle leer ist.
pub async fn system_last_tick(pool: &PgPool) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let row: (Option<DateTime<Utc>>,) = sqlx::query_as(
        r#"
        SELECT MAX(COALESCE(last_seen_at, last_started_at))
        FROM twitch_live_state
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Raw-Chat-Ingest-Gesundheit aus `twitch_raw_chat_ingest_health`.
#[derive(Debug)]
pub struct RawChatHealth {
    /// Login-Name des Streamers (bevorzugt live).
    pub streamer_login: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_insert_ok_at: Option<DateTime<Utc>>,
    pub last_insert_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// Sekunden seit dem neuesten der vier Zeitstempel (last_message_at,
    /// last_insert_ok_at, last_insert_error_at, updated_at).
    pub lag_seconds: Option<i64>,
    /// Ob die Zeile aus dem Live-Scope kommt (Streamer gerade live).
    /// Nur wenn `true` soll ein RAW_CHAT_LAG-Warning ausgelöst werden.
    pub is_live_scope: bool,
}

type RawChatRow = (
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<i64>,
    Option<bool>,
);

/// Bevorzugt live Streamer (JOIN `twitch_live_state WHERE is_live = 1 AND
/// last_seen_at >= NOW() - 4h`), Fallback auf neueste Zeile in der Tabelle.
/// Bug B: `is_live = 1` statt `TRUE` (INTEGER-Spalte).
/// Bug C: LOWER()-JOIN für case-insensitiven Vergleich.
/// Bug D: `is_live_scope`-Spalte in beiden CTEs.
pub async fn raw_chat_health(pool: &PgPool) -> Result<Option<RawChatHealth>, sqlx::Error> {
    let row: Option<RawChatRow> = sqlx::query_as(
        r#"
        WITH live_scope AS (
            SELECT
                h.streamer_login,
                h.last_raw_chat_message_at,
                h.last_raw_chat_insert_ok_at,
                h.last_raw_chat_insert_error_at,
                h.last_raw_chat_error AS last_error,
                GREATEST(
                    h.last_raw_chat_message_at,
                    h.last_raw_chat_insert_ok_at,
                    h.last_raw_chat_insert_error_at,
                    h.updated_at
                ) AS newest_signal_at,
                TRUE AS is_live_scope
            FROM twitch_raw_chat_ingest_health h
            JOIN twitch_live_state ls
                ON LOWER(ls.streamer_login) = LOWER(h.streamer_login)
            WHERE ls.is_live = 1
              AND ls.last_seen_at >= NOW() - INTERVAL '4 hours'
        ),
        fallback AS (
            SELECT
                h.streamer_login,
                h.last_raw_chat_message_at,
                h.last_raw_chat_insert_ok_at,
                h.last_raw_chat_insert_error_at,
                h.last_raw_chat_error AS last_error,
                GREATEST(
                    h.last_raw_chat_message_at,
                    h.last_raw_chat_insert_ok_at,
                    h.last_raw_chat_insert_error_at,
                    h.updated_at
                ) AS newest_signal_at,
                FALSE AS is_live_scope
            FROM twitch_raw_chat_ingest_health h
        ),
        chosen AS (
            SELECT * FROM live_scope
            UNION ALL
            SELECT * FROM fallback
            WHERE NOT EXISTS (SELECT 1 FROM live_scope)
        )
        SELECT
            streamer_login,
            last_raw_chat_message_at,
            last_raw_chat_insert_ok_at,
            last_raw_chat_insert_error_at,
            last_error,
            CASE
                WHEN newest_signal_at IS NOT NULL
                THEN EXTRACT(EPOCH FROM (NOW() - newest_signal_at))::BIGINT
                ELSE NULL
            END AS lag_seconds,
            is_live_scope
        FROM chosen
        ORDER BY newest_signal_at DESC NULLS LAST
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(login, msg, ok, err, last_err, lag, is_live)| RawChatHealth {
            streamer_login: login,
            last_message_at: msg,
            last_insert_ok_at: ok,
            last_insert_error_at: err,
            last_error: last_err,
            lag_seconds: lag,
            is_live_scope: is_live.unwrap_or(false),
        },
    ))
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
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");
        // Bug B: INTEGER NOT NULL DEFAULT 0 statt BOOLEAN
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login  TEXT PRIMARY KEY,
                is_live         INTEGER NOT NULL DEFAULT 0,
                last_seen_at    TIMESTAMPTZ,
                last_started_at TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raw_chat_ingest_health (
                streamer_login                TEXT PRIMARY KEY,
                last_raw_chat_message_at      TIMESTAMPTZ,
                last_raw_chat_insert_ok_at    TIMESTAMPTZ,
                last_raw_chat_insert_error_at TIMESTAMPTZ,
                last_raw_chat_error           TEXT,
                updated_at                    TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL raw_chat_health");
        sqlx::query("TRUNCATE twitch_live_state, twitch_raw_chat_ingest_health")
            .execute(&pool)
            .await
            .expect("TRUNCATE");
        pool
    }

    #[tokio::test]
    async fn leere_tabellen_liefern_none() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_syshealth_leer").await;
        let tick = system_last_tick(&pool).await.unwrap();
        assert!(tick.is_none());
        let chat = raw_chat_health(&pool).await.unwrap();
        assert!(chat.is_none());
    }

    #[tokio::test]
    async fn tick_wird_korrekt_gelesen() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_syshealth_tick").await;
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_seen_at) \
             VALUES ('test_s', 1, NOW() - INTERVAL '30 seconds')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let tick = system_last_tick(&pool).await.unwrap();
        assert!(tick.is_some());
    }

    #[tokio::test]
    async fn raw_chat_health_bevorzugt_live_streamer() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_syshealth_chat").await;
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_seen_at) \
             VALUES ('live_s', 1, NOW() - INTERVAL '1 minute')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
             (streamer_login, last_raw_chat_message_at, updated_at) \
             VALUES ('live_s', NOW() - INTERVAL '10 seconds', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Offline-Streamer mit neuerem Signal — darf nicht bevorzugt werden
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
             (streamer_login, last_raw_chat_message_at, updated_at) \
             VALUES ('offline_s', NOW() - INTERVAL '1 second', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        let health = raw_chat_health(&pool).await.unwrap().unwrap();
        assert_eq!(health.streamer_login.as_deref(), Some("live_s"));
        assert!(health.is_live_scope);
        assert!(health.lag_seconds.is_some());
        assert!(health.lag_seconds.unwrap() < 60);
    }

    #[tokio::test]
    async fn raw_chat_health_fallback_hat_is_live_scope_false() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_syshealth_fallback").await;
        // Nur Offline-Streamer in health-Tabelle, kein Eintrag in live_state
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
             (streamer_login, last_raw_chat_message_at, updated_at) \
             VALUES ('offline_x', NOW() - INTERVAL '5 seconds', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        let health = raw_chat_health(&pool).await.unwrap().unwrap();
        assert!(!health.is_live_scope);
    }
}

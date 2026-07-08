//! Queries für `GET /twitch/api/admin/system/health`.
//!
//! Liefert letzten DB-Tick aus `twitch_live_state` und
//! Raw-Chat-Ingest-Gesundheit aus `twitch_raw_chat_ingest_health`.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

/// Letzter bekannter Tick aus `twitch_live_state`.
///
/// Gibt `MAX(COALESCE(last_seen_at, last_started_at))` zurück,
/// oder `None` wenn die Tabelle leer ist.
///
/// `last_seen_at` und `last_started_at` sind in Prod TEXT → expliziter Cast auf timestamptz.
pub async fn system_last_tick(pool: &PgPool) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let last_tick = sqlx::query_scalar!(
        r#"
        SELECT MAX(COALESCE(last_seen_at::timestamptz, last_started_at::timestamptz)) AS last_tick
        FROM twitch_live_state
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(last_tick)
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
    /// Ingest-Lag in Sekunden, wie vom Raw-Chat-Writer gemeldet.
    pub lag_seconds: Option<i64>,
    /// Ob die Zeile aus dem Live-Scope kommt (Streamer gerade live).
    /// Nur wenn `true` soll ein RAW_CHAT_LAG-Warning ausgelöst werden.
    pub is_live_scope: bool,
}

/// Bevorzugt live Streamer (JOIN `twitch_live_state WHERE is_live = 1 AND
/// last_seen_at >= NOW() - 4h`), Fallback auf neueste Zeile in der Tabelle.
/// Bug B: `is_live = 1` statt `TRUE` (INTEGER-Spalte).
/// Bug C: LOWER()-JOIN für case-insensitiven Vergleich.
/// Bug D: `is_live_scope`-Spalte in beiden CTEs.
///
/// Auswahl-Richtung: live Streamer zuerst, darin der höchste gemeldete
/// Ingest-Lag. Alte Chat-Stille allein ist kein Raw-Chat-Lag.
///
/// Alle Timestamp-Spalten in `twitch_raw_chat_ingest_health` und
/// `last_seen_at` in `twitch_live_state` sind in Prod TEXT → expliziter
/// Cast auf timestamptz für Vergleiche und EXTRACT.
pub async fn raw_chat_health(pool: &PgPool) -> Result<Option<RawChatHealth>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        WITH live_scope AS (
            SELECT
                h.streamer_login,
                h.last_raw_chat_message_at::timestamptz    AS last_raw_chat_message_at,
                h.last_raw_chat_insert_ok_at::timestamptz  AS last_raw_chat_insert_ok_at,
                h.last_raw_chat_insert_error_at::timestamptz AS last_raw_chat_insert_error_at,
                h.last_raw_chat_error AS last_error,
                h.raw_chat_lag_seconds::BIGINT AS lag_seconds,
                GREATEST(
                    h.last_raw_chat_message_at::timestamptz,
                    h.last_raw_chat_insert_ok_at::timestamptz,
                    h.last_raw_chat_insert_error_at::timestamptz,
                    h.updated_at::timestamptz
                ) AS newest_signal_at,
                TRUE AS is_live_scope
            FROM twitch_raw_chat_ingest_health h
            JOIN twitch_live_state ls
                ON LOWER(ls.streamer_login) = LOWER(h.streamer_login)
            WHERE ls.is_live = 1
              AND ls.last_seen_at::timestamptz >= NOW() - INTERVAL '4 hours'
        ),
        fallback AS (
            SELECT
                h.streamer_login,
                h.last_raw_chat_message_at::timestamptz    AS last_raw_chat_message_at,
                h.last_raw_chat_insert_ok_at::timestamptz  AS last_raw_chat_insert_ok_at,
                h.last_raw_chat_insert_error_at::timestamptz AS last_raw_chat_insert_error_at,
                h.last_raw_chat_error AS last_error,
                h.raw_chat_lag_seconds::BIGINT AS lag_seconds,
                GREATEST(
                    h.last_raw_chat_message_at::timestamptz,
                    h.last_raw_chat_insert_ok_at::timestamptz,
                    h.last_raw_chat_insert_error_at::timestamptz,
                    h.updated_at::timestamptz
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
            lag_seconds,
            is_live_scope
        FROM chosen
        ORDER BY is_live_scope DESC, lag_seconds DESC NULLS LAST, newest_signal_at DESC NULLS LAST
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| RawChatHealth {
        streamer_login: row.try_get("streamer_login").ok(),
        last_message_at: row.try_get("last_raw_chat_message_at").ok().flatten(),
        last_insert_ok_at: row.try_get("last_raw_chat_insert_ok_at").ok().flatten(),
        last_insert_error_at: row.try_get("last_raw_chat_insert_error_at").ok().flatten(),
        last_error: row.try_get("last_error").ok().flatten(),
        lag_seconds: row.try_get("lag_seconds").ok().flatten(),
        is_live_scope: row.try_get("is_live_scope").unwrap_or(false),
    }))
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
        // Prod-Typen: last_seen_at/last_started_at sind TEXT (kein timestamptz)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login  TEXT PRIMARY KEY,
                is_live         INTEGER NOT NULL DEFAULT 0,
                last_seen_at    TEXT,
                last_started_at TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");
        // Prod-Typen: alle Timestamp-Spalten sind TEXT (kein timestamptz)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raw_chat_ingest_health (
                streamer_login                TEXT PRIMARY KEY,
                last_raw_chat_message_at      TEXT,
                last_raw_chat_insert_ok_at    TEXT,
                last_raw_chat_insert_error_at TEXT,
                last_raw_chat_error           TEXT,
                raw_chat_lag_seconds          INTEGER,
                updated_at                    TEXT NOT NULL DEFAULT TO_CHAR(NOW(), 'YYYY-MM-DD"T"HH24:MI:SS"Z"')
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
        // last_seen_at ist TEXT in Prod → ISO-String einsetzen
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_seen_at) \
             VALUES ('test_s', 1, (NOW() - INTERVAL '30 seconds')::TEXT)",
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
        // last_seen_at ist TEXT in Prod → ISO-String
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_seen_at) \
             VALUES ('live_s', 1, (NOW() - INTERVAL '1 minute')::TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Timestamp-Spalten in twitch_raw_chat_ingest_health sind TEXT in Prod
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
             (streamer_login, last_raw_chat_message_at, raw_chat_lag_seconds, updated_at) \
             VALUES ('live_s', (NOW() - INTERVAL '10 seconds')::TEXT, 0, NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Offline-Streamer mit neuerem Signal — darf nicht bevorzugt werden
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
             (streamer_login, last_raw_chat_message_at, raw_chat_lag_seconds, updated_at) \
             VALUES ('offline_s', (NOW() - INTERVAL '1 second')::TEXT, 0, NOW()::TEXT)",
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
    async fn raw_chat_health_waehlt_staelesten_live_streamer() {
        // P2.76: Bei zwei live Streamern muss der mit dem ÄLTESTEN Signal
        // (höchster Lag) gewählt werden, damit RAW_CHAT_LAG anschlägt.
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_syshealth_stale").await;
        // Beide Streamer live
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_seen_at) VALUES \
             ('fresh_s', 1, (NOW() - INTERVAL '1 minute')::TEXT), \
             ('stale_s', 1, (NOW() - INTERVAL '1 minute')::TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // fresh_s: kein Lag; stale_s: hoher gemeldeter Ingest-Lag.
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
                (streamer_login, last_raw_chat_message_at, raw_chat_lag_seconds, updated_at) VALUES \
             ('fresh_s', (NOW() - INTERVAL '10 seconds')::TEXT, 0,    (NOW() - INTERVAL '10 seconds')::TEXT), \
             ('stale_s', (NOW() - INTERVAL '2 hours')::TEXT,    7200, (NOW() - INTERVAL '2 hours')::TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let health = raw_chat_health(&pool).await.unwrap().unwrap();
        assert_eq!(
            health.streamer_login.as_deref(),
            Some("stale_s"),
            "der live Streamer mit dem ältesten Signal muss gewählt werden"
        );
        assert!(health.is_live_scope);
        assert!(health.lag_seconds.unwrap() > 3600, "Lag muss > 1h sein");
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
        // Timestamp-Spalten sind TEXT in Prod → expliziter Cast
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
             (streamer_login, last_raw_chat_message_at, updated_at) \
             VALUES ('offline_x', (NOW() - INTERVAL '5 seconds')::TEXT, NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let health = raw_chat_health(&pool).await.unwrap().unwrap();
        assert!(!health.is_live_scope);
    }
}

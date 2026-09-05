//! Roh-Chat-Verfügbarkeit + Ingestion-Lücken-Erkennung (`rawChatStatus`).
//!
//! Port von `bot/analytics/raw_chat_status.py:build_raw_chat_status` (+ Scope-Helfer).
//! Geteilter Diagnose-Block für die `chat-*`-Endpunkte: meldet, ob im Zeitfenster
//! echte Roh-Chat-Nachrichten vorliegen, ob eine Ingestion-Lücke vermutet wird
//! (Presence-/Rollup-Daten ohne Roh-Nachrichten) und den Backfill-Status.
//!
//! **Graceful wie Python:** Die Diagnose-Tabellen `twitch_raw_chat_ingest_health`
//! (live) und `twitch_raw_chat_backfill_runs` (Legacy, evtl. fehlend) sowie die
//! Fallback-Query sind in Python try/except-gekapselt → bei Fehler Default. Die
//! Scope-Queries über `twitch_chat_messages`/`twitch_session_chatters` laufen
//! ungekapselt (Tabellen existieren live).

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

/// Auswahl des Zeitfensters: entweder Session-IDs ODER ein `since`-Datum (Python
/// `session_ids is not None` entscheidet — leere Liste = explizit „keine Sessions").
pub enum Scope<'a> {
    Sessions(&'a [i64]),
    Since(DateTime<Utc>),
}

/// Python `_coerce_timestamp`: ISO-String (mit/ohne Offset, `Z`) oder naiv → UTC.
fn coerce_ts(text: &str) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let normalized = match text.strip_suffix('Z') {
        Some(stripped) => format!("{stripped}+00:00"),
        None => text.to_string(),
    };
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, fmt) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    None
}

/// Python `datetime.isoformat()`: Micros nur wenn vorhanden, Offset `+00:00`.
fn emit_iso(dt: DateTime<Utc>) -> String {
    if dt.timestamp_subsec_nanos() == 0 {
        dt.to_rfc3339_opts(SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(SecondsFormat::Micros, false)
    }
}

fn iso_or_none_dt(dt: Option<DateTime<Utc>>) -> Value {
    dt.map(emit_iso).map(Value::String).unwrap_or(Value::Null)
}

fn iso_or_none_str(text: Option<String>) -> Value {
    text.as_deref()
        .and_then(coerce_ts)
        .map(emit_iso)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

struct PresenceStats {
    presence_rows: i64,
    gap_sessions: i64,
    gap_start: Value,
}

async fn query_scope_presence(
    pool: &PgPool,
    streamer: &str,
    scope: &Scope<'_>,
    coverage_start: Option<DateTime<Utc>>,
) -> Result<PresenceStats, sqlx::Error> {
    let empty = || PresenceStats {
        presence_rows: 0,
        gap_sessions: 0,
        gap_start: Value::Null,
    };
    let Some(coverage_start) = coverage_start else {
        return Ok(empty());
    };
    match scope {
        Scope::Sessions([]) => Ok(empty()),
        Scope::Sessions(ids) => {
            let stats: (i64, i64) = sqlx::query_as(
                "SELECT COUNT(*)::bigint, COUNT(DISTINCT sc.session_id)::bigint \
                   FROM twitch_session_chatters sc \
                   JOIN twitch_stream_sessions s ON s.id = sc.session_id \
                  WHERE LOWER(s.streamer_login) = $1 AND sc.session_id = ANY($2::bigint[]) \
                    AND s.started_at >= $3",
            )
            .bind(streamer)
            .bind(ids)
            .bind(coverage_start)
            .fetch_one(pool)
            .await?;
            let gap: (i64, Option<DateTime<Utc>>) = sqlx::query_as(
                "SELECT COUNT(*)::bigint, MIN(s.started_at) FROM twitch_stream_sessions s \
                  WHERE LOWER(s.streamer_login) = $1 AND s.id = ANY($2::bigint[]) \
                    AND s.started_at >= $3 \
                    AND EXISTS (SELECT 1 FROM twitch_session_chatters sc WHERE sc.session_id = s.id) \
                    AND NOT EXISTS (SELECT 1 FROM twitch_chat_messages m WHERE m.session_id = s.id)",
            )
            .bind(streamer)
            .bind(ids)
            .bind(coverage_start)
            .fetch_one(pool)
            .await?;
            Ok(PresenceStats {
                presence_rows: stats.0,
                gap_sessions: gap.0,
                gap_start: iso_or_none_dt(gap.1),
            })
        }
        Scope::Since(since) => {
            let effective_since = (*since).max(coverage_start);
            let stats: (i64, i64) = sqlx::query_as(
                "SELECT COUNT(*)::bigint, COUNT(DISTINCT sc.session_id)::bigint \
                   FROM twitch_session_chatters sc \
                   JOIN twitch_stream_sessions s ON s.id = sc.session_id \
                  WHERE LOWER(s.streamer_login) = $1 AND s.started_at >= $2",
            )
            .bind(streamer)
            .bind(effective_since)
            .fetch_one(pool)
            .await?;
            let gap: (i64, Option<DateTime<Utc>>) = sqlx::query_as(
                "SELECT COUNT(*)::bigint, MIN(s.started_at) FROM twitch_stream_sessions s \
                  WHERE LOWER(s.streamer_login) = $1 AND s.started_at >= $2 \
                    AND EXISTS (SELECT 1 FROM twitch_session_chatters sc WHERE sc.session_id = s.id) \
                    AND NOT EXISTS (SELECT 1 FROM twitch_chat_messages m WHERE m.session_id = s.id)",
            )
            .bind(streamer)
            .bind(effective_since)
            .fetch_one(pool)
            .await?;
            Ok(PresenceStats {
                presence_rows: stats.0,
                gap_sessions: gap.0,
                gap_start: iso_or_none_dt(gap.1),
            })
        }
    }
}

struct RawStats {
    raw_rows: i64,
    sessions_with_raw: i64,
    last_message_at: Value,
}

async fn query_scope_raw(
    pool: &PgPool,
    streamer: &str,
    scope: &Scope<'_>,
) -> Result<RawStats, sqlx::Error> {
    let (rows, sessions, last): (i64, i64, Option<DateTime<Utc>>) = match scope {
        Scope::Sessions([]) => {
            return Ok(RawStats {
                raw_rows: 0,
                sessions_with_raw: 0,
                last_message_at: Value::Null,
            })
        }
        Scope::Sessions(ids) => {
            let row = sqlx::query!(
                "SELECT COUNT(*)::bigint AS \"rows!\", COUNT(DISTINCT m.session_id)::bigint AS \"sessions!\", MAX(m.message_ts) AS last_message_at \
               FROM twitch_chat_messages m \
              WHERE LOWER(m.streamer_login) = $1 AND m.session_id = ANY($2::bigint[])",
                streamer,
                ids
            )
            .fetch_one(pool)
            .await?;
            (row.rows, row.sessions, row.last_message_at)
        }
        Scope::Since(since) => {
            let row = sqlx::query!(
                "SELECT COUNT(*)::bigint AS \"rows!\", COUNT(DISTINCT m.session_id)::bigint AS \"sessions!\", MAX(m.message_ts) AS last_message_at \
               FROM twitch_chat_messages m \
              WHERE LOWER(m.streamer_login) = $1 AND m.message_ts >= $2",
                streamer,
                since
            )
            .fetch_one(pool)
            .await?;
            (row.rows, row.sessions, row.last_message_at)
        }
    };
    Ok(RawStats {
        raw_rows: rows,
        sessions_with_raw: sessions,
        last_message_at: iso_or_none_dt(last),
    })
}

/// Baut den `rawChatStatus`-Block (Python `build_raw_chat_status`).
pub async fn build_raw_chat_status(
    pool: &PgPool,
    streamer_login: &str,
    scope: Scope<'_>,
) -> Result<Value, sqlx::Error> {
    let streamer = streamer_login.trim().to_lowercase();
    if streamer.is_empty() {
        return Ok(json!({
            "available": false,
            "coverageStart": Value::Null,
            "lastMessageAt": Value::Null,
            "gapStart": Value::Null,
            "suspectedIngestionIssue": false,
            "backfillState": "not_needed",
            "note": Value::Null,
        }));
    }

    let coverage_start: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT MIN(message_ts) FROM twitch_chat_messages WHERE LOWER(streamer_login) = $1",
    )
    .bind(&streamer)
    .fetch_one(pool)
    .await?;

    // Health-Tabelle (live, aber graceful wie Pythons try/except).
    let health = sqlx::query!(
        "SELECT last_raw_chat_message_at, last_raw_chat_insert_ok_at, last_raw_chat_insert_error_at, last_raw_chat_error \
           FROM twitch_raw_chat_ingest_health WHERE LOWER(streamer_login) = $1 LIMIT 1",
        &streamer
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    // Fallback-Last-Message (graceful).
    let fallback_last: Value = match sqlx::query_scalar!(
        "SELECT MAX(message_ts) AS message_ts FROM twitch_chat_messages WHERE LOWER(streamer_login) = $1",
        &streamer
    )
    .fetch_one(pool)
    .await
    {
        Ok(dt) => iso_or_none_dt(dt),
        Err(_) => Value::Null,
    };

    let health_last_message_at = iso_or_none_str(
        health
            .as_ref()
            .and_then(|h| h.last_raw_chat_message_at.clone()),
    );
    let health_last_insert_ok_at = iso_or_none_str(
        health
            .as_ref()
            .and_then(|h| h.last_raw_chat_insert_ok_at.clone()),
    );
    let health_last_insert_error_at = iso_or_none_str(
        health
            .as_ref()
            .and_then(|h| h.last_raw_chat_insert_error_at.clone()),
    );
    let health_last_error: Option<String> = health
        .as_ref()
        .and_then(|h| h.last_raw_chat_error.clone())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let presence = query_scope_presence(pool, &streamer, &scope, coverage_start).await?;
    let raw = query_scope_raw(pool, &streamer, &scope).await?;

    // Vermutete Ingestion-Lücke.
    let suspected_issue = (presence.presence_rows > 0 && raw.raw_rows == 0)
        // ponytail: Eine einzelne Gap-Session kann Session-/Poll-Randrauschen sein; bei präziserem Ingestion-Signal wieder verschärfen.
        || (presence.gap_sessions > 1 && raw.sessions_with_raw > 0);

    // Backfill-Status (Legacy-Tabelle, graceful).
    let backfill_row = sqlx::query!(
        "SELECT status, note FROM twitch_raw_chat_backfill_runs WHERE LOWER(streamer_login) = $1 \
          ORDER BY COALESCE(finished_at, started_at) DESC LIMIT 1",
        &streamer
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let backfill_state = if let Some(row) = &backfill_row {
        let s = row.status.trim();
        if s.is_empty() {
            "not_started".to_string()
        } else {
            s.to_string()
        }
    } else if suspected_issue {
        "not_started".to_string()
    } else {
        "not_needed".to_string()
    };

    // Hinweistext (gleiche Kaskade wie Python).
    let mut note: Option<String> = None;
    if suspected_issue && raw.raw_rows == 0 {
        note = Some("Presence-/Rollup-Daten vorhanden, aber keine Roh-Chat-Nachrichten im gewählten Zeitraum.".to_string());
    } else if suspected_issue {
        note = Some("Roh-Chat-Nachrichten sind im gewählten Zeitraum nur teilweise vorhanden; message-basierte KPIs sind unvollständig.".to_string());
    } else if raw.raw_rows == 0 {
        note = Some("Keine Roh-Chat-Nachrichten im gewählten Zeitraum.".to_string());
    }
    if note.is_none() {
        if let Some(err) = &health_last_error {
            if !health_last_insert_error_at.is_null() {
                note = Some(format!("Letzter Roh-Chat-Insert-Fehler: {err}"));
            }
        }
    }

    // lastMessageAt = scope_raw.lastMessageAt or health_last_message_at or fallback (Python `or`-Kaskade).
    let last_message_at = first_non_null([
        raw.last_message_at.clone(),
        health_last_message_at,
        fallback_last,
    ]);

    Ok(json!({
        "available": raw.raw_rows > 0,
        "coverageStart": iso_or_none_dt(coverage_start),
        "lastMessageAt": last_message_at,
        "gapStart": presence.gap_start,
        "suspectedIngestionIssue": suspected_issue,
        "backfillState": backfill_state,
        "note": note.map(Value::String).unwrap_or(Value::Null),
        "lastInsertOkAt": health_last_insert_ok_at,
        "lastInsertErrorAt": health_last_insert_error_at,
    }))
}

/// Erstes nicht-null-JSON-String aus der Kaskade (Python `a or b or c`).
fn first_non_null<const N: usize>(values: [Value; N]) -> Value {
    for v in values {
        if !v.is_null() {
            return v;
        }
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str, with_diag: bool) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds BIGINT)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, messages INTEGER DEFAULT 0)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, content TEXT, message_ts TIMESTAMPTZ)").execute(&pool).await.unwrap();
        if with_diag {
            sqlx::query("CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT)").execute(&pool).await.unwrap();
            sqlx::query("CREATE TABLE twitch_raw_chat_backfill_runs (streamer_login TEXT, status TEXT, note TEXT, started_at TIMESTAMPTZ, finished_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn leerer_streamer_default() {
        let Some(pool) = make_pool("t_rcs_empty", true).await else {
            return;
        };
        let v = build_raw_chat_status(&pool, "  ", Scope::Since(Utc::now()))
            .await
            .unwrap();
        assert_eq!(v["available"], false);
        assert_eq!(v["backfillState"], "not_needed");
        assert!(v["note"].is_null());
    }

    #[tokio::test]
    async fn presence_vor_coverage_meldet_keine_luecke() {
        let Some(pool) = make_pool("t_rcs_precoverage", true).await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani',NOW()-INTERVAL '10 days'),(2,'nani',NOW()-INTERVAL '1 day')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login) VALUES (1,'nani','viewer')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (2,'nani','viewer','hi',NOW()-INTERVAL '12 hours')").execute(&pool).await.unwrap();
        let since = Utc::now() - chrono::Duration::days(30);
        let v = build_raw_chat_status(&pool, "nani", Scope::Since(since))
            .await
            .unwrap();
        assert_eq!(v["available"], true);
        assert_eq!(v["suspectedIngestionIssue"], false);
        assert_eq!(v["backfillState"], "not_needed");
        assert!(v["gapStart"].is_null());
        assert!(!v["coverageStart"].is_null());
    }

    #[tokio::test]
    async fn mehrere_luecken_nach_coverage_werden_gemeldet() {
        let Some(pool) = make_pool("t_rcs_multi_gap", true).await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani',NOW()-INTERVAL '4 days'),(2,'nani',NOW()-INTERVAL '2 days'),(3,'nani',NOW()-INTERVAL '1 day')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (1,'nani','viewer','hi',NOW()-INTERVAL '3 days')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login) VALUES (2,'nani','viewer'),(3,'nani','viewer')").execute(&pool).await.unwrap();

        let v = build_raw_chat_status(
            &pool,
            "nani",
            Scope::Since(Utc::now() - chrono::Duration::days(30)),
        )
        .await
        .unwrap();

        assert_eq!(v["available"], true);
        assert_eq!(v["suspectedIngestionIssue"], true);
        assert_eq!(v["backfillState"], "not_started");
        assert!(!v["gapStart"].is_null());
    }

    #[tokio::test]
    async fn raw_vorhanden_available() {
        let Some(pool) = make_pool("t_rcs_ok", true).await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani',NOW()-INTERVAL '1 day')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login) VALUES (1,'nani','viewer')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (1,'nani','viewer','hi',NOW()-INTERVAL '12 hours')").execute(&pool).await.unwrap();
        let since = Utc::now() - chrono::Duration::days(30);
        let v = build_raw_chat_status(&pool, "nani", Scope::Since(since))
            .await
            .unwrap();
        assert_eq!(v["available"], true);
        assert_eq!(v["suspectedIngestionIssue"], false);
        assert_eq!(v["backfillState"], "not_needed");
        assert!(!v["lastMessageAt"].is_null());
        assert!(v["gapStart"].is_null());
    }

    #[tokio::test]
    async fn geistersessions_unter_10min_loesen_keine_luecke_aus() {
        let Some(pool) = make_pool("t_rcs_ghost_gap", true).await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at, duration_seconds) VALUES (3,'nani',NOW()-INTERVAL '10 days',NOW()-INTERVAL '10 days'+INTERVAL '2 hours',7200)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (3,'nani','viewer','hi',NOW()-INTERVAL '10 days')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at, duration_seconds) VALUES (1,'nani',NOW()-INTERVAL '3 days',NOW()-INTERVAL '3 days'+INTERVAL '5 minutes',300),(2,'nani',NOW()-INTERVAL '2 days',NOW()-INTERVAL '2 days'+INTERVAL '5 minutes',300)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login) VALUES (1,'nani','viewer'),(2,'nani','viewer')").execute(&pool).await.unwrap();

        let v = build_raw_chat_status(
            &pool,
            "nani",
            Scope::Since(Utc::now() - chrono::Duration::days(30)),
        )
        .await
        .unwrap();

        assert_eq!(v["available"], true);
        assert_eq!(v["suspectedIngestionIssue"], false);
        assert!(v["gapStart"].is_null());
    }

    #[tokio::test]
    async fn diag_tabellen_fehlen_graceful() {
        // Ohne health/backfill-Tabellen → graceful Default, Scope-Queries laufen normal.
        let Some(pool) = make_pool("t_rcs_nodiag", false).await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani',NOW()-INTERVAL '1 day')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (1,'nani','v','hi',NOW())").execute(&pool).await.unwrap();
        let v = build_raw_chat_status(
            &pool,
            "nani",
            Scope::Since(Utc::now() - chrono::Duration::days(30)),
        )
        .await
        .unwrap();
        assert_eq!(v["available"], true);
        assert!(v["lastInsertOkAt"].is_null());
    }
}

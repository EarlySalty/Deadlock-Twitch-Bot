//! Handler für `GET /internal/twitch/v1/sessions/{session_id}`.
//!
//! Nativer Port von `bot/internal_api/routes/streamers.py:504` und der
//! Business-Logik in `bot/dashboard/dashboard_metrics_mixin.py:326–365`.
//!
//! # Shape-Parität
//!
//! Python führt `SELECT * FROM twitch_stream_sessions WHERE id=%s` und wandelt
//! die Row dynamisch via `_row_to_dict(row)` (Cursor-Keys + `json_default`-
//! Serializer) in JSON um. Damit künftige Spalten automatisch mitkommen, macht
//! dieser Handler dasselbe: dynamischer Spalten-Iterator statt fixierter Struct.
//!
//! # Typ-Mapping (Python `json_default` → Rust)
//!
//! | Postgres-Typ | JSON-Output |
//! |---|---|
//! | `TIMESTAMPTZ` | ISO-8601-String via [`ts_to_iso`] (Python: `datetime.isoformat()`) |
//! | `INT4` / `INT8` | Zahl (i32 / i64) |
//! | `FLOAT8` | Zahl (f64) |
//! | `BOOL` | bool |
//! | `TEXT` | String |
//! | `NULL` | null |
//!
//! # Fehler-Parität (`bot/internal_api/routes/streamers.py:448–467`)
//!
//! | Bedingung | HTTP | Body |
//! |---|---|---|
//! | session_id nicht parsebar | 400 | `{"error":"bad_request","message":"invalid session id"}` |
//! | Session nicht gefunden | 404 | `{"error":"not_found","message":"session not found"}` |
//! | unerwartete Exception | 500 | `{"error":"internal_error","message":"failed to fetch session detail"}` |

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::{Column, PgPool, Row, TypeInfo};
use tb_http_core::{ApiError, AuthLevel};

// ── Timestamp-Serialisierung ──────────────────────────────────────────────────

/// Wandelt ein `DateTime<Utc>` in einen Python-kompatiblen ISO-8601-String um.
///
/// Python: `datetime.isoformat()` mit `tzinfo=UTC` produziert
/// `"2026-06-12T14:30:00+00:00"`. Mikrosekunden werden nur ausgegeben, wenn
/// sie ungleich 0 sind (Python lässt sie sonst weg: `timedelta(0)` → `00:00`).
///
/// Referenz: `bot/internal_api/policy.py:18–27` (`json_default`).
pub fn ts_to_iso(dt: DateTime<Utc>) -> String {
    // Kanonischer Serializer (Block 10): eine Quelle der Wahrheit für die
    // datetime→isoformat-Parität (`crate::security::datetime_to_iso`).
    crate::security::datetime_to_iso(dt)
}

// ── Dynamischer Row→JSON-Mapper ───────────────────────────────────────────────

/// Mappt eine `sqlx::postgres::PgRow` dynamisch auf ein `serde_json::Map`.
///
/// Iteriert über alle Spalten (`row.columns()`), liest den Postgres-Typ-Namen
/// und dekodiert den Wert entsprechend. Neue Spalten kommen automatisch mit,
/// ohne dass der Handler angepasst werden muss — Parität zu Pythons
/// `_row_to_dict` (`bot/dashboard/dashboard_metrics_mixin.py:326–365`).
///
/// Unterstützte Typen laut `twitch_stream_sessions`-DDL
/// (`bot/migrations/twitch_analytics_schema.sql:169–200`):
/// `TIMESTAMPTZ`, `INT4`, `INT8`, `FLOAT8`, `BOOL`, `TEXT`.
fn pg_row_to_json(row: &sqlx::postgres::PgRow) -> Map<String, Value> {
    use sqlx::postgres::PgRow;
    let _ = std::marker::PhantomData::<PgRow>;

    let mut map = Map::new();
    for col in row.columns() {
        let name = col.name();
        let type_name = col.type_info().name();
        let val = decode_pg_column(row, col.ordinal(), type_name);
        map.insert(name.to_string(), val);
    }
    map
}

/// Dekodiert eine einzelne Postgres-Spalte nach Typ-Name in einen JSON-`Value`.
///
/// Unbekannte Typen werden als `null` kodiert (defensiv, statt zu panicen).
fn decode_pg_column(row: &sqlx::postgres::PgRow, idx: usize, type_name: &str) -> Value {
    match type_name {
        // TIMESTAMPTZ → ISO-8601-String (Python: datetime.isoformat())
        "TIMESTAMPTZ" => {
            let v: Option<DateTime<Utc>> = row.try_get(idx).ok().flatten();
            match v {
                Some(dt) => Value::String(ts_to_iso(dt)),
                None => Value::Null,
            }
        }
        // INTEGER / INT4
        "INT4" => {
            let v: Option<i32> = row.try_get(idx).ok().flatten();
            match v {
                Some(n) => json!(n),
                None => Value::Null,
            }
        }
        // BIGINT / INT8 / BIGSERIAL
        "INT8" => {
            let v: Option<i64> = row.try_get(idx).ok().flatten();
            match v {
                Some(n) => json!(n),
                None => Value::Null,
            }
        }
        // DOUBLE PRECISION / FLOAT8
        "FLOAT8" => {
            let v: Option<f64> = row.try_get(idx).ok().flatten();
            match v {
                Some(f) => json!(f),
                None => Value::Null,
            }
        }
        // BOOLEAN
        "BOOL" => {
            let v: Option<bool> = row.try_get(idx).ok().flatten();
            match v {
                Some(b) => json!(b),
                None => Value::Null,
            }
        }
        // TEXT / VARCHAR
        "TEXT" | "VARCHAR" => {
            let v: Option<String> = row.try_get(idx).ok().flatten();
            match v {
                Some(s) => Value::String(s),
                None => Value::Null,
            }
        }
        // Unbekannter Typ → null (defensiv, kein Panic)
        _ => {
            tracing::warn!(
                "session_detail: unbekannter Spalten-Typ '{}' → null",
                type_name
            );
            Value::Null
        }
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/sessions/{session_id}`
///
/// Gibt Session-Details + Timeline + Top-Chatters zurück.
/// Parität zu `bot/dashboard/dashboard_metrics_mixin.py:326–365`.
pub async fn session_detail_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(session_id_raw): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Path-Parameter parsen — `bot/internal_api/routes/streamers.py:448–451`
    // Python: `int(str(raw).strip())` → ValueError → 400 "invalid session id"
    let session_id: i64 = session_id_raw
        .trim()
        .parse()
        .map_err(|_| ApiError::bad_request("invalid session id"))?;

    // ── Haupt-Session-Row (`bot/dashboard/dashboard_metrics_mixin.py:336–340`)
    let session_row = sqlx::query("SELECT * FROM twitch_stream_sessions WHERE id = $1")
        .bind(session_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            tracing::error!("session_detail DB-Fehler (sessions): {e}");
            ApiError::internal_with("failed to fetch session detail")
        })?;

    // 404 wenn kein Row — `bot/internal_api/routes/streamers.py:456–457`
    let session_row = session_row.ok_or_else(|| ApiError::not_found_with("session not found"))?;
    let session_map = pg_row_to_json(&session_row);

    // ── Timeline (`bot/dashboard/dashboard_metrics_mixin.py:342–347`)
    // SQL: SELECT minutes_from_start, viewer_count FROM twitch_session_viewers
    //      WHERE session_id=$1 ORDER BY minutes_from_start ASC
    let timeline_rows = sqlx::query(
        "SELECT minutes_from_start, viewer_count \
         FROM twitch_session_viewers \
         WHERE session_id = $1 \
         ORDER BY minutes_from_start ASC",
    )
    .bind(session_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("session_detail DB-Fehler (timeline): {e}");
        ApiError::internal_with("failed to fetch session detail")
    })?;

    let timeline: Vec<Value> = timeline_rows
        .iter()
        .map(|row| {
            let minutes: Option<i32> = row.try_get("minutes_from_start").ok().flatten();
            let viewers: i32 = row.try_get("viewer_count").unwrap_or(0);
            json!({
                "minutes_from_start": minutes,
                "viewer_count": viewers,
            })
        })
        .collect();

    // ── Top-Chatters (`bot/dashboard/dashboard_metrics_mixin.py:349–355`)
    // SQL: SELECT chatter_login, messages FROM twitch_session_chatters
    //      WHERE session_id=$1 ORDER BY messages DESC LIMIT 10
    let chatter_rows = sqlx::query(
        "SELECT chatter_login, messages \
         FROM twitch_session_chatters \
         WHERE session_id = $1 \
         ORDER BY messages DESC \
         LIMIT 10",
    )
    .bind(session_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("session_detail DB-Fehler (chatters): {e}");
        ApiError::internal_with("failed to fetch session detail")
    })?;

    let top_chatters: Vec<Value> = chatter_rows
        .iter()
        .map(|row| {
            let login: String = row.try_get("chatter_login").unwrap_or_default();
            let messages: i32 = row.try_get("messages").unwrap_or(0);
            json!({
                "chatter_login": login,
                "messages": messages,
            })
        })
        .collect();

    Ok(Json(json!({
        "session": session_map,
        "timeline": timeline,
        "top_chatters": top_chatters,
    })))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::get,
        Extension, Router,
    };
    use chrono::TimeZone;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::net::SocketAddr;
    use std::str::FromStr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
    use tower::ServiceExt;

    // ── Infrastruktur ─────────────────────────────────────────────────────────

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! pool_or_skip {
        ($schema:expr) => {
            match test_dsn() {
                Some(dsn) => make_pool(&dsn, $schema).await,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        // Schema-isolierter Pool — identisch zu raid_blacklist.rs-Tests
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("Schema anlegen");
        admin.close().await;

        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .expect("pool connect");
        apply_ddl(&pool).await;
        pool
    }

    /// Prod-treue DDL laut `bot/migrations/twitch_analytics_schema.sql:169–200`.
    async fn apply_ddl(pool: &PgPool) {
        sqlx::query(
            r#"CREATE TABLE twitch_stream_sessions (
                id                    BIGSERIAL PRIMARY KEY,
                streamer_login        TEXT NOT NULL,
                stream_id             TEXT,
                started_at            TIMESTAMPTZ NOT NULL,
                ended_at              TIMESTAMPTZ,
                duration_seconds      INTEGER DEFAULT 0,
                start_viewers         INTEGER DEFAULT 0,
                peak_viewers          INTEGER DEFAULT 0,
                end_viewers           INTEGER DEFAULT 0,
                avg_viewers           DOUBLE PRECISION DEFAULT 0,
                samples               INTEGER DEFAULT 0,
                retention_5m          DOUBLE PRECISION,
                retention_10m         DOUBLE PRECISION,
                retention_20m         DOUBLE PRECISION,
                dropoff_pct           DOUBLE PRECISION,
                dropoff_label         TEXT,
                unique_chatters       INTEGER DEFAULT 0,
                first_time_chatters   INTEGER DEFAULT 0,
                returning_chatters    INTEGER DEFAULT 0,
                followers_start       INTEGER,
                followers_end         INTEGER,
                follower_delta        INTEGER,
                stream_title          TEXT,
                notification_text     TEXT,
                language              TEXT,
                is_mature             BOOLEAN DEFAULT FALSE,
                tags                  TEXT,
                had_deadlock_in_session BOOLEAN DEFAULT FALSE,
                game_name             TEXT,
                notes                 TEXT
            )"#,
        )
        .execute(pool)
        .await
        .expect("DDL sessions");

        sqlx::query(
            r#"CREATE TABLE twitch_session_viewers (
                id                INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                session_id        BIGINT NOT NULL,
                minutes_from_start INTEGER,
                viewer_count      INTEGER NOT NULL
            )"#,
        )
        .execute(pool)
        .await
        .expect("DDL viewers");

        sqlx::query(
            r#"CREATE TABLE twitch_session_chatters (
                id            INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                session_id    BIGINT NOT NULL,
                chatter_login TEXT NOT NULL,
                messages      INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(pool)
        .await
        .expect("DDL chatters");
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(
                &format!("{base}/sessions/:session_id"),
                get(session_detail_handler),
            )
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(token.to_string(), internal_auth))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(uri: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("GET")
            .uri(uri)
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── Unit-Tests: ts_to_iso ─────────────────────────────────────────────────

    #[test]
    fn ts_iso_ohne_mikrosekunden() {
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 14, 30, 0).unwrap();
        assert_eq!(ts_to_iso(dt), "2026-06-12T14:30:00+00:00");
    }

    #[test]
    fn ts_iso_mit_mikrosekunden() {
        use chrono::NaiveDateTime;
        let dt = NaiveDateTime::parse_from_str("2026-06-12T14:30:00.123456", "%Y-%m-%dT%H:%M:%S%.f")
            .unwrap()
            .and_utc();
        let s = ts_to_iso(dt);
        assert_eq!(s, "2026-06-12T14:30:00.123456+00:00");
    }

    #[test]
    fn ts_iso_mitternacht_keine_mikrosekunden() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(ts_to_iso(dt), "2025-01-01T00:00:00+00:00");
    }

    // ── Integration: Auth ────────────────────────────────────────────────────

    #[tokio::test]
    async fn ohne_token_401() {
        let pool = pool_or_skip!("test_sd_auth_401");
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(&format!("{base}/sessions/1"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Integration: 400 bad_request ─────────────────────────────────────────

    #[tokio::test]
    async fn ungueltige_session_id_400() {
        let pool = pool_or_skip!("test_sd_bad_id");
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(&format!("{base}/sessions/abc"), Some("secret")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
        assert_eq!(j["message"], "invalid session id");
    }

    #[tokio::test]
    async fn leere_id_400() {
        // Leerer Pfad-Segment: axum gibt hier 404 (kein Match), aber testen wir
        // dennoch den negativen Fall mit einer nicht-numerischen ID.
        let pool = pool_or_skip!("test_sd_empty_id");
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(&format!("{base}/sessions/notanumber"), Some("secret")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
    }

    // ── Integration: 404 not_found ───────────────────────────────────────────

    #[tokio::test]
    async fn unbekannte_session_404() {
        let pool = pool_or_skip!("test_sd_404");
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(&format!("{base}/sessions/999999"), Some("secret")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "not_found");
        assert_eq!(j["message"], "session not found");
    }

    // ── Integration: 200 vollständige Response ───────────────────────────────

    #[tokio::test]
    async fn session_mit_allen_feldern_200() {
        let pool = pool_or_skip!("test_sd_full");
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;

        // Test-Session einfügen
        let row = sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (streamer_login, stream_id, started_at, ended_at, duration_seconds, \
              start_viewers, peak_viewers, end_viewers, avg_viewers, samples, \
              retention_5m, retention_10m, retention_20m, dropoff_pct, dropoff_label, \
              unique_chatters, first_time_chatters, returning_chatters, \
              followers_start, followers_end, follower_delta, \
              stream_title, language, is_mature, tags, had_deadlock_in_session, \
              game_name, notes) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,\
                     $19,$20,$21,$22,$23,$24,$25,$26,$27,$28) \
             RETURNING id",
        )
        .bind("teststreamer")
        .bind("stream123")
        .bind(Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap())
        .bind(Utc.with_ymd_and_hms(2026, 6, 12, 11, 0, 0).unwrap())
        .bind(3600_i32)
        .bind(100_i32)
        .bind(200_i32)
        .bind(80_i32)
        .bind(150.5_f64)
        .bind(12_i32)
        .bind(0.95_f64)
        .bind(0.85_f64)
        .bind(0.70_f64)
        .bind(0.25_f64)
        .bind("mild")
        .bind(50_i32)
        .bind(10_i32)
        .bind(40_i32)
        .bind(1000_i32)
        .bind(1010_i32)
        .bind(10_i32)
        .bind("Test Stream Titel")
        .bind("de")
        .bind(false)
        .bind("deadlock,fps")
        .bind(true)
        .bind("Deadlock")
        .bind("Notiz")
        .fetch_one(&pool)
        .await
        .unwrap();
        let session_id: i64 = row.try_get("id").unwrap();

        // Timeline-Punkte
        for (min, viewers) in [(0_i32, 100_i32), (5, 180), (10, 160)] {
            sqlx::query(
                "INSERT INTO twitch_session_viewers (session_id, minutes_from_start, viewer_count) \
                 VALUES ($1, $2, $3)",
            )
            .bind(session_id)
            .bind(min)
            .bind(viewers)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Chatters
        for (login, msgs) in [("chatter_a", 42_i32), ("chatter_b", 7)] {
            sqlx::query(
                "INSERT INTO twitch_session_chatters (session_id, chatter_login, messages) \
                 VALUES ($1, $2, $3)",
            )
            .bind(session_id)
            .bind(login)
            .bind(msgs)
            .execute(&pool)
            .await
            .unwrap();
        }

        let resp = app
            .oneshot(req(
                &format!("{base}/sessions/{session_id}"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;

        // Top-Level-Felder
        assert!(j["session"].is_object(), "session muss ein Objekt sein");
        assert!(j["timeline"].is_array(), "timeline muss ein Array sein");
        assert!(j["top_chatters"].is_array(), "top_chatters muss ein Array sein");

        // Session-Felder
        let s = &j["session"];
        assert_eq!(s["streamer_login"], "teststreamer");
        assert_eq!(s["stream_id"], "stream123");
        assert_eq!(s["duration_seconds"], 3600);
        assert_eq!(s["peak_viewers"], 200);
        assert_eq!(s["avg_viewers"], 150.5);
        assert_eq!(s["is_mature"], false);
        assert_eq!(s["had_deadlock_in_session"], true);
        assert_eq!(s["game_name"], "Deadlock");
        assert_eq!(s["language"], "de");
        assert_eq!(s["tags"], "deadlock,fps");
        assert_eq!(s["stream_title"], "Test Stream Titel");

        // started_at ISO-Format ohne Mikrosekunden
        let started = s["started_at"].as_str().unwrap();
        assert_eq!(started, "2026-06-12T10:00:00+00:00");
        let ended = s["ended_at"].as_str().unwrap();
        assert_eq!(ended, "2026-06-12T11:00:00+00:00");

        // notification_text ist immer null (laut Vertrag §14)
        assert!(s["notification_text"].is_null());

        // Timeline: 3 Punkte, aufsteigend sortiert
        let tl = j["timeline"].as_array().unwrap();
        assert_eq!(tl.len(), 3);
        assert_eq!(tl[0]["minutes_from_start"], 0);
        assert_eq!(tl[0]["viewer_count"], 100);
        assert_eq!(tl[2]["minutes_from_start"], 10);
        assert_eq!(tl[2]["viewer_count"], 160);

        // Top-Chatters: absteigend nach messages, max 10
        let tc = j["top_chatters"].as_array().unwrap();
        assert_eq!(tc.len(), 2);
        assert_eq!(tc[0]["chatter_login"], "chatter_a");
        assert_eq!(tc[0]["messages"], 42);
        assert_eq!(tc[1]["chatter_login"], "chatter_b");
        assert_eq!(tc[1]["messages"], 7);
    }

    #[tokio::test]
    async fn session_ohne_optional_felder_null() {
        let pool = pool_or_skip!("test_sd_nullfelder");
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;

        // Minimal-Insert: nur NOT NULL-Felder + Defaults
        let row = sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at) \
             VALUES ($1, $2) RETURNING id",
        )
        .bind("minimalstreamer")
        .bind(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        let session_id: i64 = row.try_get("id").unwrap();

        let resp = app
            .oneshot(req(
                &format!("{base}/sessions/{session_id}"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        let s = &j["session"];

        // Nullable-Felder müssen null sein
        assert!(s["ended_at"].is_null());
        assert!(s["stream_id"].is_null());
        assert!(s["retention_5m"].is_null());
        assert!(s["retention_10m"].is_null());
        assert!(s["retention_20m"].is_null());
        assert!(s["dropoff_pct"].is_null());
        assert!(s["dropoff_label"].is_null());
        assert!(s["followers_start"].is_null());
        assert!(s["followers_end"].is_null());
        assert!(s["follower_delta"].is_null());
        assert!(s["stream_title"].is_null());
        assert!(s["notification_text"].is_null());
        assert!(s["language"].is_null());
        assert!(s["tags"].is_null());
        assert!(s["game_name"].is_null());
        assert!(s["notes"].is_null());

        // DEFAULT-Felder müssen 0/false sein
        assert_eq!(s["duration_seconds"], 0);
        assert_eq!(s["start_viewers"], 0);
        assert_eq!(s["peak_viewers"], 0);
        assert_eq!(s["end_viewers"], 0);
        assert_eq!(s["avg_viewers"], 0.0);
        assert_eq!(s["samples"], 0);
        assert_eq!(s["unique_chatters"], 0);
        assert_eq!(s["first_time_chatters"], 0);
        assert_eq!(s["returning_chatters"], 0);
        assert_eq!(s["is_mature"], false);
        assert_eq!(s["had_deadlock_in_session"], false);

        // Leere Arrays
        assert_eq!(j["timeline"].as_array().unwrap().len(), 0);
        assert_eq!(j["top_chatters"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn top_chatters_limit_10() {
        let pool = pool_or_skip!("test_sd_limit10");
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;

        let row = sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at) \
             VALUES ($1, $2) RETURNING id",
        )
        .bind("limitstreamer")
        .bind(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        let session_id: i64 = row.try_get("id").unwrap();

        // 15 Chatters einfügen
        for i in 0..15_i32 {
            sqlx::query(
                "INSERT INTO twitch_session_chatters (session_id, chatter_login, messages) \
                 VALUES ($1, $2, $3)",
            )
            .bind(session_id)
            .bind(format!("user{i:02}"))
            .bind(i)
            .execute(&pool)
            .await
            .unwrap();
        }

        let resp = app
            .oneshot(req(
                &format!("{base}/sessions/{session_id}"),
                Some("secret"),
            ))
            .await
            .unwrap();
        let j = json_body(resp).await;
        let tc = j["top_chatters"].as_array().unwrap();
        assert_eq!(tc.len(), 10, "LIMIT 10 muss eingehalten werden");
        // Absteigende Sortierung: höchste messages zuerst (14, 13, 12, ...)
        assert_eq!(tc[0]["messages"], 14);
        assert_eq!(tc[1]["messages"], 13);
    }
}

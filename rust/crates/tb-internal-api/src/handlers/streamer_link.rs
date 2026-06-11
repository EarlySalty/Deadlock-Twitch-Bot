//! Handler für `GET /streamers/link-candidates`.
//!
//! Nativer Port der bislang an Python 8779 proxied Route aus
//! `bot/internal_api/routes/streamer_link.py`.
//!
//! Vertrag:
//! - `GET /internal/twitch/v1/streamers/link-candidates`
//!   → `200 {"ok": true, "entries": [{"twitch_login", "twitch_user_id"?, "is_monitored_only"}]}`
//! - Fehler (DB-Exception) → `500 {"error":"internal_error","message":"failed to list link candidates"}`
//!
//! Keine Query-Parameter, kein Body. Auth: `is_privileged()` → 401.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::streamer_link as db;
use tb_http_core::{ApiError, AuthLevel};

// ── Response-Typen (snake_case wie Python) ────────────────────────────────────

#[derive(Serialize)]
pub struct LinkCandidateEntry {
    pub twitch_login: String,
    /// Immer im JSON präsent (NULL → `null`) — Parität zu Python `dict(row)`,
    /// das den Schlüssel stets enthält. KEIN skip_serializing_if.
    pub twitch_user_id: Option<String>,
    pub is_monitored_only: i32,
}

#[derive(Serialize)]
pub struct LinkCandidatesResponse {
    pub ok: bool,
    pub entries: Vec<LinkCandidateEntry>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/streamers/link-candidates`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Python `list_unlinked_streamers` fängt Exceptions ab und gibt `[]` zurück
    // (Matcher-Resilienz, pg.py:4163-4165) — ein DB-Fehler darf hier KEIN 500
    // werden, sondern eine leere Liste.
    let rows = db::list_unlinked(&pool).await.unwrap_or_else(|e| {
        tracing::error!("streamer_link list DB-Fehler: {e}");
        Vec::new()
    });

    let entries = rows
        .into_iter()
        .map(|r| LinkCandidateEntry {
            twitch_login: r.twitch_login,
            twitch_user_id: r.twitch_user_id,
            is_monitored_only: r.is_monitored_only,
        })
        .collect();

    Ok(Json(LinkCandidatesResponse { ok: true, entries }))
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
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
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
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        sqlx::query(
            r#"
            CREATE TABLE twitch_streamers (
                twitch_login         TEXT PRIMARY KEY,
                twitch_user_id       TEXT,
                discord_user_id      TEXT,
                discord_display_name TEXT,
                is_on_discord        INTEGER DEFAULT 0,
                created_at           TEXT DEFAULT CURRENT_TIMESTAMP,
                archived_at          TEXT,
                is_monitored_only    INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers");

        sqlx::query(
            r#"
            CREATE TABLE twitch_streamer_identities (
                twitch_user_id       TEXT PRIMARY KEY,
                twitch_login         TEXT NOT NULL,
                discord_user_id      TEXT,
                discord_display_name TEXT,
                is_on_discord        INTEGER DEFAULT 0,
                created_at           TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at           TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamer_identities");

        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(
                &format!("{base}/streamers/link-candidates"),
                get(list_handler),
            )
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(token.to_string(), internal_auth))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(method: &str, uri: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn ohne_token_401() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_sl_401").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req("GET", &format!("{base}/streamers/link-candidates"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn leere_tabelle_gibt_ok_true_und_leere_liste() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_sl_leer").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/streamers/link-candidates"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert_eq!(j["entries"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn unverknuepfter_streamer_erscheint_in_entries() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_sl_entry").await;
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only) VALUES ($1, $2, $3)",
        )
        .bind("nanigami")
        .bind("uid_1")
        .bind(0_i32)
        .execute(&pool)
        .await
        .unwrap();

        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/streamers/link-candidates"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        let entries = j["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["twitch_login"], "nanigami");
        assert_eq!(entries[0]["twitch_user_id"], "uid_1");
        assert_eq!(entries[0]["is_monitored_only"], 0);
    }

    #[tokio::test]
    async fn verknuepfter_streamer_erscheint_nicht() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_sl_verknuepft").await;
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, discord_user_id) VALUES ($1, $2)",
        )
        .bind("already_linked")
        .bind("discord_999")
        .execute(&pool)
        .await
        .unwrap();

        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/streamers/link-candidates"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["entries"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn twitch_user_id_null_erscheint_als_null() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_sl_null_uid").await;
        sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ($1)")
            .bind("kein_uid")
            .execute(&pool)
            .await
            .unwrap();

        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/streamers/link-candidates"),
                Some("secret"),
            ))
            .await
            .unwrap();
        let j = json_body(resp).await;
        let entry = &j["entries"][0];
        // Parität zu Python dict(row): Schlüssel IMMER präsent, NULL → null.
        assert!(
            entry.get("twitch_user_id").is_some(),
            "twitch_user_id-Schlüssel muss präsent sein"
        );
        assert!(
            entry["twitch_user_id"].is_null(),
            "twitch_user_id muss null sein wenn DB NULL liefert"
        );
        assert_eq!(entry["is_monitored_only"], 0);
    }

    #[tokio::test]
    async fn is_monitored_only_1_wird_serialisiert() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_sl_monitored").await;
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, is_monitored_only) VALUES ($1, 1)",
        )
        .bind("nur_monitor")
        .execute(&pool)
        .await
        .unwrap();

        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/streamers/link-candidates"),
                Some("secret"),
            ))
            .await
            .unwrap();
        let j = json_body(resp).await;
        assert_eq!(j["entries"][0]["is_monitored_only"], 1);
    }
}

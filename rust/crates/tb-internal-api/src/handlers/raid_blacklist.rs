//! Handler für die 4 Raid-Blacklist-Endpoints (nativer Port der bislang an
//! Python 8779 proxied Routen `/raid/blacklist[...]`).
//!
//! Vertrag (Parität zu `bot/internal_api/routes/raid.py` +
//! `bot/dashboard/mixin.py`):
//! - `POST /raid/blacklist/add`    → `{ok, login, reason}`
//! - `POST /raid/blacklist/remove` → `{ok, login, removed}`
//! - `GET  /raid/blacklist/check`  → `{ok, login, blacklisted[, reason, added_at]}`
//! - `GET  /raid/blacklist`        → `{ok, entries: [{login, reason, added_at}]}`
//!
//! Login wird via `tb_domain::normalize_twitch_login` kanonisiert; leer/ungültig
//! → 400 `{"error":"bad_request","message":"invalid or missing login"}`.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::raid_blacklist as db;
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};

use super::common::{pick_first_truthy, resolve_reason};

// ── Request-Typen ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddRequest {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
}

#[derive(Deserialize)]
pub struct CheckQuery {
    #[serde(default)]
    pub login: Option<String>,
}

// ── Response-Typen (snake_case wie Python) ────────────────────────────────────

#[derive(Serialize)]
pub struct AddResponse {
    pub ok: bool,
    pub login: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct RemoveResponse {
    pub ok: bool,
    pub login: String,
    pub removed: bool,
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub ok: bool,
    pub login: String,
    pub blacklisted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
}

#[derive(Serialize)]
pub struct ListEntry {
    pub login: String,
    pub reason: String,
    pub added_at: String,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub ok: bool,
    pub entries: Vec<ListEntry>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /internal/twitch/v1/raid/blacklist/add`
pub async fn add_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<AddRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let raw = pick_first_truthy(body.login, body.twitch_login);
    let Some(login) = normalize_twitch_login(&raw) else {
        return Err(ApiError::bad_request("invalid or missing login"));
    };
    let reason = resolve_reason(body.reason);

    db::add_manual(&pool, &login, &reason).await.map_err(|e| {
        tracing::error!("raid_blacklist add DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(AddResponse {
        ok: true,
        login,
        reason,
    }))
}

/// `POST /internal/twitch/v1/raid/blacklist/remove`
pub async fn remove_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<RemoveRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let raw = pick_first_truthy(body.login, body.twitch_login);
    let Some(login) = normalize_twitch_login(&raw) else {
        return Err(ApiError::bad_request("invalid or missing login"));
    };

    let removed = db::remove(&pool, &login).await.map_err(|e| {
        tracing::error!("raid_blacklist remove DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(RemoveResponse {
        ok: true,
        login,
        removed,
    }))
}

/// `GET /internal/twitch/v1/raid/blacklist/check?login=<login>`
pub async fn check_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<CheckQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let raw = params.login.unwrap_or_default();
    let Some(login) = normalize_twitch_login(&raw) else {
        return Err(ApiError::bad_request("invalid or missing login"));
    };

    let entry = db::check_entry(&pool, &login).await.map_err(|e| {
        tracing::error!("raid_blacklist check DB-Fehler: {e}");
        ApiError::internal()
    })?;

    let (blacklisted, reason, added_at) = match entry {
        Some((reason, added_at)) => (true, Some(reason), Some(added_at)),
        None => (false, None, None),
    };

    Ok(Json(CheckResponse {
        ok: true,
        login,
        blacklisted,
        reason,
        added_at,
    }))
}

/// `GET /internal/twitch/v1/raid/blacklist`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let rows = db::list_entries(&pool).await.map_err(|e| {
        tracing::error!("raid_blacklist list DB-Fehler: {e}");
        ApiError::internal()
    })?;

    let entries = rows
        .into_iter()
        .map(|e| ListEntry {
            login: e.target_login,
            reason: e.reason.unwrap_or_default(),
            added_at: e.added_at.unwrap_or_default(),
        })
        .collect();

    Ok(Json(ListResponse { ok: true, entries }))
}

// ── Handler-Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::{get, post},
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
            CREATE TABLE twitch_raid_blacklist (
                target_id    TEXT,
                target_login TEXT NOT NULL PRIMARY KEY,
                reason       TEXT,
                added_at     TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL blacklist");

        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(&format!("{base}/raid/blacklist"), get(list_handler))
            .route(&format!("{base}/raid/blacklist/add"), post(add_handler))
            .route(
                &format!("{base}/raid/blacklist/remove"),
                post(remove_handler),
            )
            .route(&format!("{base}/raid/blacklist/check"), get(check_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(token.to_string(), internal_auth))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(method: &str, uri: &str, body: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn ohne_token_401() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_401").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req("GET", &format!("{base}/raid/blacklist"), "", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn add_default_reason_wenn_fehlt() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_add_def").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/add"),
                r#"{"login":"ZielKanal"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert_eq!(j["login"], "zielkanal");
        assert_eq!(j["reason"], "manual_ban:absolut");
    }

    #[tokio::test]
    async fn add_uebernimmt_reason_und_twitch_login_alias() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_add_reason").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/add"),
                r#"{"twitch_login":"@Helmi","reason":"scammer"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["login"], "helmi");
        assert_eq!(j["reason"], "scammer");
    }

    #[tokio::test]
    async fn add_invalid_login_400() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_add_400").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/add"),
                r#"{"login":"ab"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
        assert_eq!(j["message"], "invalid or missing login");
    }

    #[tokio::test]
    async fn check_unbekannt_blacklisted_false_ohne_extra_keys() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_check_false").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/raid/blacklist/check?login=niemand"),
                "",
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["blacklisted"], false);
        assert!(j.get("reason").is_none(), "reason darf fehlen wenn nicht geblacklistet");
        assert!(j.get("added_at").is_none());
    }

    #[tokio::test]
    async fn add_dann_check_case_und_at_insensitiv() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_roundtrip").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        // add @Foo
        app.clone()
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/add"),
                r#"{"login":"@Foo_Bar","reason":"x"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        // check FOO_BAR (anderes Casing) → blacklisted true mit reason
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/raid/blacklist/check?login=FOO_BAR"),
                "",
                Some("secret"),
            ))
            .await
            .unwrap();
        let j = json_body(resp).await;
        assert_eq!(j["login"], "foo_bar");
        assert_eq!(j["blacklisted"], true);
        assert_eq!(j["reason"], "x");
        assert!(j["added_at"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn remove_roundtrip() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_remove").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        app.clone()
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/add"),
                r#"{"login":"weg_damit"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        let resp = app
            .clone()
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/remove"),
                r#"{"login":"weg_damit"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(json_body(resp).await["removed"], true);
        // erneut → removed false
        let resp2 = app
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/remove"),
                r#"{"login":"weg_damit"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(json_body(resp2).await["removed"], false);
    }

    #[tokio::test]
    async fn list_leer_und_befuellt() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_rbl_list").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        // leer
        let resp = app
            .clone()
            .oneshot(req("GET", &format!("{base}/raid/blacklist"), "", Some("secret")))
            .await
            .unwrap();
        assert_eq!(json_body(resp).await["entries"], serde_json::json!([]));
        // einer
        app.clone()
            .oneshot(req(
                "POST",
                &format!("{base}/raid/blacklist/add"),
                r#"{"login":"einer","reason":"r"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        let resp2 = app
            .oneshot(req("GET", &format!("{base}/raid/blacklist"), "", Some("secret")))
            .await
            .unwrap();
        let j = json_body(resp2).await;
        assert_eq!(j["entries"][0]["login"], "einer");
        assert_eq!(j["entries"][0]["reason"], "r");
    }
}

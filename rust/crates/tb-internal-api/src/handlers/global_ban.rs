//! Handler für die 4 Global-Ban-Endpoints.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::global_ban as db;
use tb_http_core::{ApiError, AuthLevel};

// ── Request/Response-Typen ────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddBanRequest {
    pub login: String,
    pub chatter_id: Option<String>,
    pub reason: Option<String>,
    pub added_by: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveBanRequest {
    pub login: String,
}

#[derive(Deserialize)]
pub struct CheckBanQuery {
    pub login: String,
}

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
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
    pub banned: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BanEntryResponse {
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub reason: Option<String>,
    pub added_by: Option<String>,
    pub added_at: Option<String>,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub ok: bool,
    pub entries: Vec<BanEntryResponse>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /internal/twitch/v1/globalban/add`
pub async fn add_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<AddBanRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    db::add_ban(
        &pool,
        &body.login,
        body.chatter_id.as_deref(),
        body.reason.as_deref(),
        body.added_by.as_deref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("add_ban DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(OkResponse { ok: true }))
}

/// `POST /internal/twitch/v1/globalban/remove`
pub async fn remove_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<RemoveBanRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let removed = db::remove_ban(&pool, &body.login).await.map_err(|e| {
        tracing::error!("remove_ban DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(RemoveResponse {
        ok: true,
        login: body.login,
        removed,
    }))
}

/// `GET /internal/twitch/v1/globalban/check?login=<login>`
pub async fn check_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<CheckBanQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let banned = db::check_ban(&pool, &params.login, "").await.map_err(|e| {
        tracing::error!("check_ban DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(CheckResponse {
        ok: true,
        login: params.login,
        banned,
    }))
}

/// `GET /internal/twitch/v1/globalban`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let entries = db::list_bans(&pool).await.map_err(|e| {
        tracing::error!("list_bans DB-Fehler: {e}");
        ApiError::internal()
    })?;

    let response_entries = entries
        .into_iter()
        .map(|e| BanEntryResponse {
            chatter_login: e.chatter_login,
            chatter_id: e.chatter_id,
            reason: e.reason,
            added_by: e.added_by,
            added_at: e.added_at.map(|dt: DateTime<_>| dt.to_rfc3339()),
        })
        .collect();

    Ok(Json(ListResponse {
        ok: true,
        entries: response_entries,
    }))
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
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken};
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Gibt die DSN zurück oder bricht den Test ab.
    /// Mit `TB_TEST_REQUIRE_DB=1` wird statt des stillen Skips ein panic ausgelöst.
    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!(
                            "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt"
                        );
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
            CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban (
                chatter_login  TEXT PRIMARY KEY,
                chatter_id     TEXT,
                reason         TEXT,
                added_by       TEXT,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL ban");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban_applied (
                id            BIGSERIAL PRIMARY KEY,
                chatter_login TEXT NOT NULL,
                applied_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL ban_applied");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_blacklist (
                id           BIGSERIAL PRIMARY KEY,
                target_id    TEXT,
                target_login TEXT NOT NULL,
                reason       TEXT,
                UNIQUE (target_login)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL blacklist");

        sqlx::query(
            "TRUNCATE twitch_chatter_global_ban, twitch_chatter_global_ban_applied, twitch_raid_blacklist",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");

        pool
    }

    /// Baut Test-Router mit Loopback-Adresse als ConnectInfo.
    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        Router::new()
            .route(&format!("{base}/globalban"), get(list_handler))
            .route(&format!("{base}/globalban/add"), post(add_handler))
            .route(&format!("{base}/globalban/remove"), post(remove_handler))
            .route(&format!("{base}/globalban/check"), get(check_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(
                token.to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn loopback_req_json(
        method: &str,
        uri: &str,
        body: &str,
        token: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .extension(ConnectInfo(
                "127.0.0.1:55555".parse::<SocketAddr>().unwrap(),
            ));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_401").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json("GET", &format!("{base}/globalban"), "", None);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn add_returns_200_ok() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_add").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let body = r#"{"login":"testuser","reason":"Spam"}"#;
        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/add"),
            body,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn remove_returns_200_mit_removed_false_bei_unbekanntem_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_remove").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let body = r#"{"login":"nichtvorhanden"}"#;
        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/remove"),
            body,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["removed"], false);
    }

    #[tokio::test]
    async fn check_returns_banned_false_bei_unbekanntem_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_check").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json(
            "GET",
            &format!("{base}/globalban/check?login=niemand"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["banned"], false);
    }

    #[tokio::test]
    async fn list_returns_leeres_array() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_list").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json("GET", &format!("{base}/globalban"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["entries"], serde_json::json!([]));
    }
}

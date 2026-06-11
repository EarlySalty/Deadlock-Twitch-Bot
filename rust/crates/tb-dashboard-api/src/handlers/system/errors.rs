//! Handler für `GET /twitch/api/admin/system/errors`.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::system_errors::error_log_entries;
use tb_http_core::{ApiError, AuthLevel};

#[derive(Deserialize)]
pub struct ErrorsParams {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    25
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEntryResponse {
    pub id: i64,
    pub created_at: String,
    pub level: Option<String>,
    pub message: Option<String>,
    pub context: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorsResponse {
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub has_more: bool,
    pub entries: Vec<ErrorEntryResponse>,
}

/// `GET /twitch/api/admin/system/errors[?page=1&page_size=25]`
pub async fn errors_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ErrorsParams>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let page_size = params.page_size.clamp(1, 100);
    let page = params.page.max(1);

    // Bug A: ApiError::internal() ohne Argument
    let result = error_log_entries(&pool, page, page_size)
        .await
        .map_err(|e| {
            tracing::error!("error_log_entries Fehler: {e}");
            ApiError::internal()
        })?;

    let has_more = (page * page_size) < result.total;

    Ok(Json(ErrorsResponse {
        page,
        page_size,
        total: result.total,
        has_more,
        entries: result
            .entries
            .into_iter()
            .map(|e| ErrorEntryResponse {
                id: e.id,
                created_at: e.created_at.to_rfc3339(),
                level: e.level,
                message: e.message,
                context: e.context,
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
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
        pool
    }

    async fn make_pool_with_table(dsn: &str, schema: &str) -> PgPool {
        let pool = make_pool(dsn, schema).await;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_admin_error_log (
                id         BIGSERIAL PRIMARY KEY,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                level      TEXT,
                message    TEXT,
                context    TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL error_log");
        sqlx::query("TRUNCATE twitch_admin_error_log")
            .execute(&pool)
            .await
            .expect("TRUNCATE");
        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/system/errors", get(errors_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn authed_req(token: &str, uri: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri(uri)
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", token)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_err_unauth").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/errors")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tabelle_nicht_vorhanden_gibt_leeres_ergebnis() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_err_notable").await;
        let res = make_router(pool, "tok")
            .oneshot(authed_req("tok", "/twitch/api/admin/system/errors"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["total"], 0);
        assert!(v["entries"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn paginierung_gibt_has_more() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool_with_table(&dsn, "test_handler_err_pages").await;
        for i in 0..5i64 {
            sqlx::query("INSERT INTO twitch_admin_error_log (level, message) VALUES ('INFO', $1)")
                .bind(format!("msg {i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let res = make_router(pool, "tok")
            .oneshot(authed_req(
                "tok",
                "/twitch/api/admin/system/errors?page=1&page_size=3",
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["total"], 5);
        assert_eq!(v["entries"].as_array().unwrap().len(), 3);
        assert_eq!(v["hasMore"], true);
    }
}

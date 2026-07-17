//! Handler für `GET /twitch/api/admin/system/database`.

use crate::auth::level::DashboardAuthLevel;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::system_database::database_stats;
use tb_http_core::ApiError;

const TRACKED_TABLES: &[&str] = &[
    "twitch_streamers",
    "twitch_live_state",
    "twitch_stream_sessions",
    "twitch_stats_tracked",
    "twitch_stats_category",
    "streamer_plans",
    "twitch_billing_subscriptions",
    "affiliate_accounts",
    "twitch_eventsub_capacity_snapshot",
    "dashboard_sessions",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStatResponse {
    pub table: String,
    pub row_count: i64,
    pub size_bytes: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseResponse {
    pub database_size_bytes: i64,
    pub tables: Vec<TableStatResponse>,
}

/// `GET /twitch/api/admin/system/database`
pub async fn database_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }

    // Bug A: ApiError::internal() ohne Argument
    let stats = database_stats(&pool, TRACKED_TABLES).await.map_err(|e| {
        tracing::error!("database_stats Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(DatabaseResponse {
        database_size_bytes: stats.database_size_bytes,
        tables: stats
            .tables
            .into_iter()
            .map(|t| TableStatResponse {
                table: t.table,
                row_count: t.row_count,
                size_bytes: t.size_bytes,
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
                        panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
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

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/system/database", get(database_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    #[tokio::test]
    async fn returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_db_unauth").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/database")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_200_mit_leerer_tabellenliste() {
        let dsn = db_dsn_or_skip!();
        // Schema ohne tracked tables → tables-Array leer, DB-Size > 0
        let pool = make_pool(&dsn, "test_handler_db_leer").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/database")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v["databaseSizeBytes"].as_i64().unwrap() > 0);
        assert!(v["tables"].as_array().unwrap().is_empty());
    }
}

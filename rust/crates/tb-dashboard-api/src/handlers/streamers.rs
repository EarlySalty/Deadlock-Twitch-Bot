//! Handler für `GET /twitch/api/v2/streamers`.
//!
//! Admin-only: gibt 401 zurück wenn AuthLevel == None.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::streamers::{active_streamers, StreamerListRow};
use tb_http_core::{ApiError, AuthLevel};

#[derive(Serialize)]
pub struct StreamerJson {
    pub login: String,
    pub is_live: bool,
    pub viewer_count: i32,
}

impl From<StreamerListRow> for StreamerJson {
    fn from(r: StreamerListRow) -> Self {
        Self {
            login: r.twitch_login,
            is_live: r.is_live != 0,
            viewer_count: r.viewer_count,
        }
    }
}

#[derive(Serialize)]
pub struct StreamersResponse {
    pub streamers: Vec<StreamerJson>,
}

/// `GET /twitch/api/v2/streamers`
pub async fn streamers_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let rows = active_streamers(&pool)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok(Json(StreamersResponse {
        streamers: rows.into_iter().map(StreamerJson::from).collect(),
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

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen fehlgeschlagen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamers_partner_state (
                twitch_login      TEXT NOT NULL PRIMARY KEY,
                is_partner_active INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers_partner_state fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login    TEXT NOT NULL PRIMARY KEY,
                is_live           INTEGER NOT NULL DEFAULT 0,
                last_viewer_count INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state fehlgeschlagen");
        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/v2/streamers", get(streamers_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn admin_req(token: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri("/twitch/api/v2/streamers")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", token)
            .body(Body::empty())
            .unwrap()
    }

    fn unauth_req() -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri("/twitch/api/v2/streamers")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn returns_401_without_auth() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_handler_streamers_unauth").await;
        let res = make_router(pool, "tok")
            .oneshot(unauth_req())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_streamers_for_admin() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_handler_streamers_admin").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active)
            VALUES ('nani', 1)
            ON CONFLICT (twitch_login) DO UPDATE SET is_partner_active = EXCLUDED.is_partner_active;
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let res = make_router(pool, "tok")
            .oneshot(admin_req("tok"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v["streamers"].is_array());
        assert_eq!(v["streamers"][0]["login"], "nani");
    }
}

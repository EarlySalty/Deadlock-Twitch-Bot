//! Handler für `GET /twitch/api/v2/streamers`.
//!
//! Admin-only: gibt 401 zurück wenn DashboardAuthLevel nicht privileged (Localhost/Admin).

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::streamers::{active_streamers, StreamerListRow};
use tb_http_core::ApiError;

use crate::auth::level::DashboardAuthLevel;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerJson {
    pub login: String,
    pub is_partner: bool,
    pub is_live: bool,
    pub viewer_count: i32,
}

impl From<StreamerListRow> for StreamerJson {
    fn from(r: StreamerListRow) -> Self {
        Self {
            login: r.twitch_login,
            is_partner: r.is_partner,
            is_live: r.is_live != 0,
            viewer_count: r.viewer_count,
        }
    }
}

/// `GET /twitch/api/v2/streamers`
pub async fn streamers_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let rows = active_streamers(&pool)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok(Json(
        rows.into_iter().map(StreamerJson::from).collect::<Vec<_>>(),
    ))
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
    use tower::ServiceExt;

    use crate::auth::session::DashboardAuthState;

    const TEST_FERNET_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

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

    /// Legt Schema + Tabellen für Streamer-Abfragen an.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
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
            .expect("search_path setzen fehlgeschlagen");
        for ddl in [
            r#"CREATE TABLE twitch_streamers_partner_state (
                twitch_login      TEXT NOT NULL PRIMARY KEY,
                is_partner_active INTEGER NOT NULL DEFAULT 0
            )"#,
            r#"CREATE TABLE twitch_live_state (
                streamer_login    TEXT NOT NULL PRIMARY KEY,
                is_live           INTEGER NOT NULL DEFAULT 0,
                last_viewer_count INTEGER NOT NULL DEFAULT 0
            )"#,
            r#"CREATE TABLE twitch_stream_sessions (
                id             BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                started_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("DDL fehlgeschlagen");
        }
        pool
    }

    /// Erweitert den Pool um Session- und Partner-Tabellen (für Cookie-Auth-Tests).
    async fn make_pool_with_sessions(dsn: &str, schema: &str) -> PgPool {
        let pool = make_pool(dsn, schema).await;
        for ddl in [
            r#"CREATE TABLE dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )"#,
            // departnered_at/admin_archived_at/partnered_at: vom Partner-Gate in
            // load_partner_session referenziert (ORDER BY). Fehlten sie, warf die
            // Gate-Query "column does not exist" → Session-Load Err → 401.
            r#"CREATE TABLE twitch_partners (
                twitch_login            TEXT NOT NULL PRIMARY KEY,
                twitch_user_id          TEXT,
                status                  TEXT,
                technical_pause_reason  TEXT,
                departnered_at          TEXT,
                admin_archived_at       TEXT,
                partnered_at            TEXT
            )"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("DDL fehlgeschlagen");
        }
        pool
    }

    fn make_router(pool: PgPool) -> Router {
        Router::new()
            .route("/twitch/api/v2/streamers", get(streamers_handler))
            .with_state(pool)
    }

    fn make_router_with_auth(pool: PgPool, auth_state: DashboardAuthState) -> Router {
        Router::new()
            .route("/twitch/api/v2/streamers", get(streamers_handler))
            .with_state(pool)
            .layer(Extension(auth_state))
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

    fn cookie_req(session_id: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri("/twitch/api/v2/streamers")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header(
                axum::http::header::COOKIE,
                format!("twitch_dash_session={session_id}"),
            )
            .body(Body::empty())
            .unwrap()
    }

    fn admin_cookie_req(session_id: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri("/twitch/api/v2/streamers")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header(
                axum::http::header::COOKIE,
                format!("master_dash_session={session_id}"),
            )
            .body(Body::empty())
            .unwrap()
    }

    fn twitch_admin_cookie_req(session_id: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri("/twitch/api/v2/streamers")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header(
                axum::http::header::COOKIE,
                format!("twitch_dash_session={session_id}; tb_admin_mode=2"),
            )
            .body(Body::empty())
            .unwrap()
    }

    // ── Kein Auth ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn returns_401_without_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_streamers_unauth").await;
        let res = make_router(pool).oneshot(unauth_req()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Discord-Admin (privileged) ───────────────────────────────────────────

    #[tokio::test]
    async fn returns_streamers_for_discord_admin() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool_with_sessions(&dsn, "test_streamers_admin").await;
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active)
             VALUES ('nani', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
        let session = auth_state
            .create_admin_session("discord-1", "Discord Admin")
            .await
            .unwrap();

        let res = make_router_with_auth(pool, auth_state).oneshot(admin_cookie_req(&session.session_id)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v.is_array());
        assert_eq!(v[0]["login"], "nani");
        assert_eq!(v[0]["isPartner"], true);
        assert_eq!(v[0]["isLive"], false);
        assert_eq!(v[0]["viewerCount"], 0);
    }

    // ── Cookie-Auth: normaler Partner → 401 ──────────────────────────────────
    //
    // Sichert, dass Streamer, die ihr eigenes Dashboard sehen dürfen,
    // NICHT die vollständige Partnerliste abrufen können.

    #[tokio::test]
    async fn partner_session_gets_401() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool_with_sessions(&dsn, "test_streamers_partner_blocked").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ('somestreamer', '111', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
        let session = auth_state
            .create_partner_session("somestreamer", "111", "SomeStreamer")
            .await
            .unwrap();

        let res = make_router_with_auth(pool, auth_state)
            .oneshot(twitch_admin_cookie_req(&session.session_id))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Cookie-Auth: Twitch-Admin-Login → 200 ────────────────────────────────
    //
    // earlysalty meldet sich per Twitch-OAuth an, aktiviert Admin-Mode,
    // und darf die Partnerliste abrufen.

    #[tokio::test]
    async fn twitch_admin_login_gets_200() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool_with_sessions(&dsn, "test_streamers_twitch_admin").await;
        for (login, uid) in [("earlysalty", "42"), ("nani", "99")] {
            sqlx::query(
                "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
                 VALUES ($1, $2, 'active')",
            )
            .bind(login)
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active)
             VALUES ('nani', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
        let session = auth_state
            .create_partner_session("earlysalty", "42", "EarlySalty")
            .await
            .unwrap();

        let res = make_router_with_auth(pool, auth_state)
            .oneshot(twitch_admin_cookie_req(&session.session_id))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v.is_array());
        assert_eq!(v[0]["login"], "nani");
        assert_eq!(v[0]["isPartner"], true);
    }
}

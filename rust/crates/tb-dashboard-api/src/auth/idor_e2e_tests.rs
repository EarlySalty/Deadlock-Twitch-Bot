//! End-to-End-Regressionstest für den Partner-Pfad des IDOR-Fixes (#236-Lücke).
//!
//! Der frühere Admin-Modus-Rollback (#236) hinterließ eine Test-Lücke: der
//! Partner-Pfad — ein per Twitch-OAuth eingeloggter Streamer OHNE aktiven
//! Admin-Mode — wurde nie durchgängig (Extractor → CSRF-Layer → Handler) gegen
//! echte Sessions verifiziert. Genau dort entstand der 401-Login-Loop (Backend
//! verlangte einen Streamer-Override, den die Partner-Sicht nie setzte).
//!
//! Die Tests führen earlysalty per Twitch- und Discord-Session ohne
//! `tb_admin_mode` durch den realen Request-Fluss und beweisen die Invarianten
//! des neuen Modells gegen jeweils EINE Session-Identität:
//!   (a) `auth-status` antwortet 200 (kein 401/Loop) und meldet `level=partner`.
//!   (b) Ein streamer-scoped Read mit FREMDEM `?streamer=` → 403 (IDOR-Klemme).
//!   (c) Ein same-origin Schreib-POST (engagement-Mode-Toggle) MIT Session-Cookie,
//!       aber OHNE `X-CSRF-Token`, läuft durch (NICHT `invalid_csrf`) — der
//!       tokenlose SameSite/Origin-Fallback hält den #235-Login-Loop fern.
//!
//! Gated auf `TB_TEST_DATABASE_URL` (echte Postgres-Verbindung nötig).

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
    routing::{get, post},
};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::net::SocketAddr;
use std::str::FromStr;
use tower::ServiceExt;

use crate::auth::csrf::csrf_protect;
use crate::auth::session::{ADMIN_COOKIE_NAME, DashboardAuthState, PARTNER_COOKIE_NAME};
use crate::handlers::{auth_status, engagement_mode, performance};

const TEST_FERNET_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";

/// Nicht-Loopback-Peer, damit der CSRF-Loopback-Bypass NICHT greift und der
/// eigentliche same-origin/Session-Fallback geprüft wird.
const PEER: &str = "203.0.113.7:9999";

/// Same-Origin-Host des simulierten Browsers (kein Loopback → echter Auth-Pfad).
const HOST: &str = "dashboard.example.com";

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

/// Legt ein isoliertes Schema mit genau den Tabellen an, die der Partner-Pfad
/// berührt: Session-Store + Partner-Gate (Extractor) und Engagement-Settings
/// (Schreib-Toggle). Der scoped Read (monthly-stats) klemmt den fremden
/// Streamer VOR jedem DB-Zugriff, braucht also keine Analytics-Tabellen.
async fn make_pool(schema: &str) -> Option<PgPool> {
    let dsn = test_dsn()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("connect test-db");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("Schema droppen");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("Schema anlegen");
    admin.close().await;

    let opts = PgConnectOptions::from_str(&dsn)
        .expect("DSN parsen")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(opts)
        .await
        .expect("connect schema-pool");

    for ddl in [
        r#"CREATE TABLE dashboard_sessions (
            session_id   TEXT NOT NULL PRIMARY KEY,
            session_type TEXT NOT NULL,
            payload_enc  BYTEA NOT NULL,
            created_at   DOUBLE PRECISION NOT NULL,
            expires_at   DOUBLE PRECISION NOT NULL
        )"#,
        // Gate-Spalten (departnered_at/admin_archived_at/partnered_at) werden im
        // ORDER BY von load_partner_session referenziert.
        r#"CREATE TABLE twitch_partners (
            twitch_login            TEXT NOT NULL PRIMARY KEY,
            twitch_user_id          TEXT,
            status                  TEXT,
            technical_pause_reason  TEXT,
            departnered_at          TEXT,
            admin_archived_at       TEXT,
            partnered_at            TEXT
        )"#,
        r#"CREATE TABLE twitch_engagement_settings (
            channel_login TEXT PRIMARY KEY,
            output_mode   TEXT NOT NULL DEFAULT 'off',
            updated_at    TIMESTAMPTZ
        )"#,
    ] {
        sqlx::query(ddl)
            .execute(&pool)
            .await
            .expect("DDL fehlgeschlagen");
    }
    Some(pool)
}

/// Router mit den drei realen Handlern des Partner-Pfads. Der CSRF-Layer liegt
/// auf dem gesamten Router (greift per `is_write_method` nur auf Schreib-POSTs);
/// `DashboardAuthState` als Extension speist sowohl den Auth-Extractor als auch
/// die CSRF-Middleware.
fn make_router(pool: PgPool, auth_state: DashboardAuthState) -> Router {
    Router::new()
        .route(
            "/twitch/api/v2/auth-status",
            get(auth_status::auth_status_handler),
        )
        .route(
            "/twitch/api/v2/monthly-stats",
            get(performance::monthly_stats_handler),
        )
        .route(
            "/twitch/api/v2/engagement/mode",
            post(engagement_mode::post_handler),
        )
        .layer(axum::middleware::from_fn(
            crate::auth::partner_gate::partner_status_gate,
        ))
        .layer(axum::middleware::from_fn(csrf_protect))
        .layer(Extension(auth_state))
        .with_state(pool)
}

/// Partner-Session-Cookie OHNE `tb_admin_mode` → earlysalty bleibt Partner.
fn partner_cookie(session_id: &str) -> String {
    format!("{PARTNER_COOKIE_NAME}={session_id}")
}

fn get_req(uri: &str, cookie: &str) -> Request<Body> {
    let addr: SocketAddr = PEER.parse().unwrap();
    Request::builder()
        .method("GET")
        .uri(uri)
        .extension(ConnectInfo(addr))
        .header(header::HOST, HOST)
        .header("x-dashboard-context", "public")
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn discord_admin_ohne_mode_cookie_hat_im_public_dashboard_partner_scope() {
    let Some(pool) = make_pool("test_idor_discord_admin_user_view").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };

    let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
    let session = auth_state
        .create_admin_session("discord-owner", "Discord Owner")
        .await
        .unwrap();
    let cookie = format!("{ADMIN_COOKIE_NAME}={}", session.session_id);

    let res = make_router(pool.clone(), auth_state.clone())
        .oneshot(get_req("/twitch/api/v2/auth-status", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1 << 16)
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status["level"], "partner");
    assert_eq!(status["twitchLogin"], "earlysalty");
    assert_eq!(status["adminEligible"], true);
    assert_eq!(status["adminMode"], false);

    let res = make_router(pool.clone(), auth_state.clone())
        .oneshot(get_req(
            "/twitch/api/v2/monthly-stats?streamer=earlysalty",
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "öffentliche Nutzeransicht muss den Partner-Status des Owners beachten"
    );

    let res = make_router(pool, auth_state)
        .oneshot(get_req(
            "/twitch/api/v2/monthly-stats?streamer=ismile_e",
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "öffentliche Nutzeransicht darf keine fremden Streamer-Daten lesen"
    );
}

#[tokio::test]
async fn partner_pfad_e2e_auth_status_scope_und_csrf() {
    let Some(pool) = make_pool("test_idor_partner_e2e").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };

    // earlysalty als Partner im Gate hinterlegen (admin-eligibel, aber ohne
    // Admin-Mode-Cookie nur Partner).
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
         VALUES ('earlysalty', '42', 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
    let session = auth_state
        .create_partner_session("earlysalty", "42", "EarlySalty")
        .await
        .unwrap();
    let cookie = partner_cookie(&session.session_id);

    // ── (a) auth-status → 200, level=partner (kein 401/Loop) ─────────────────
    let res = make_router(pool.clone(), auth_state.clone())
        .oneshot(get_req("/twitch/api/v2/auth-status", &cookie))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "auth-status muss für Partner 200 liefern (kein 401-Loop)"
    );
    let body = axum::body::to_bytes(res.into_body(), 1 << 16)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["authenticated"], true);
    assert_eq!(v["level"], "partner");
    assert_eq!(v["twitchLogin"], "earlysalty");
    // Admin-eligibel, aber Admin-Mode ist NICHT aktiv.
    assert_eq!(v["adminEligible"], true);
    assert_eq!(v["adminMode"], false);

    // ── (b) scoped Read mit FREMDEM ?streamer= → 403 (IDOR-Klemme) ───────────
    let res = make_router(pool.clone(), auth_state.clone())
        .oneshot(get_req(
            "/twitch/api/v2/monthly-stats?streamer=ismile_e",
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Partner darf fremde Streamer-Analytics nicht lesen"
    );

    // Gegenprobe: derselbe Read auf den EIGENEN Login wird nicht von der
    // Scope-Klemme blockiert (403 wäre hier ein Fehlalarm).
    let res = make_router(pool.clone(), auth_state.clone())
        .oneshot(get_req(
            "/twitch/api/v2/monthly-stats?streamer=earlysalty",
            &cookie,
        ))
        .await
        .unwrap();
    assert_ne!(
        res.status(),
        StatusCode::FORBIDDEN,
        "eigener Login darf nicht von der IDOR-Klemme abgelehnt werden"
    );

    // ── (c) same-origin Schreib-POST OHNE X-CSRF-Token → NICHT invalid_csrf ─
    // Origin == Host, gültige Partner-Session, kein X-CSRF-Token: der
    // tokenlose SameSite/Origin-Fallback muss durchlassen (Vorfall #235).
    let addr: SocketAddr = PEER.parse().unwrap();
    let write = Request::builder()
        .method("POST")
        .uri("/twitch/api/v2/engagement/mode")
        .extension(ConnectInfo(addr))
        .header(header::HOST, HOST)
        .header("x-dashboard-context", "public")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"output_mode":"off"}"#))
        .unwrap();
    let res = make_router(pool.clone(), auth_state.clone())
        .oneshot(write)
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "same-origin Partner-Write ohne CSRF-Token muss durchlaufen (kein invalid_csrf)"
    );
    let body = axum::body::to_bytes(res.into_body(), 1 << 16)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // Der Write hat den CSRF-Gate passiert UND den Handler erreicht (Upsert ok).
    assert_eq!(v["ok"], true);
    assert_eq!(v["output_mode"], "off");
    // Doppelt absichern: keinesfalls die invalid_csrf-Antwort.
    assert_ne!(v["error"], "invalid_csrf");
}

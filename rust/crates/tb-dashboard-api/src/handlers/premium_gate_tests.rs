//! Beweis für das Premium-Gate des Pricing-Umbaus vom 2026-08-09.
//!
//! Milestone 3 sperrt eine Reihe bislang offener Analyse-Endpunkte hinter
//! Premium. Die Stop-Regel der Spec lautet: „Ein Endpunkt, der im Frontend
//! gesperrt aussieht, aber per API antwortet, gilt als nicht erledigt."
//! Deshalb prüft dieser Test nicht das Frontend, sondern schickt echte
//! HTTP-Requests durch den realen Router (Auth-Extractor → Partner-Gate →
//! Handler) mit einer echten Partner-Session.
//!
//! Zwei Richtungen pro Endpunkt:
//!   (a) Partner OHNE Premium → 403 mit `error = "plan_required"`.
//!   (b) derselbe Partner MIT Premium → NICHT `plan_required`. Ohne diese
//!       Gegenrichtung wäre (a) auch dann grün, wenn der Endpunkt aus einem
//!       ganz anderen Grund 403 liefert.
//!
//! Die Handler laufen nach dem Gate auf Analytics-Tabellen, die dieses Schema
//! nicht anlegt. Richtung (b) prüft deshalb bewusst nur, dass die Antwort
//! nicht mehr das Plan-Gate ist — ob der Handler danach 200 oder 500 liefert,
//! ist Sache der jeweiligen Handler-Tests.
//!
//! Gated auf `TB_TEST_DATABASE_URL`.

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

use crate::auth::session::{DashboardAuthState, PARTNER_COOKIE_NAME};
use crate::handlers::{
    audience, chat_analytics, performance, rankings, session_detail, social_media, stream_report,
    title,
};

const TEST_FERNET_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";
const PEER: &str = "203.0.113.9:9999";
const HOST: &str = "dashboard.example.com";
const LOGIN: &str = "earlysalty";

/// Alle in Milestone 3 neu gegateten Routen, Methode + URI wie im echten
/// Router (`lib.rs`). Wer hier eine Zeile löscht, löscht den Beweis.
const GATED: &[(&str, &str)] = &[
    ("GET", "/twitch/api/v2/monthly-stats?streamer=earlysalty"),
    ("GET", "/twitch/api/v2/weekly-stats?streamer=earlysalty"),
    ("GET", "/twitch/api/v2/hourly-heatmap?streamer=earlysalty"),
    ("GET", "/twitch/api/v2/viewer-overlap?streamer=earlysalty"),
    ("GET", "/twitch/api/v2/rankings?streamer=earlysalty"),
    ("GET", "/twitch/api/v2/chat-analytics?streamer=earlysalty"),
    ("GET", "/twitch/api/v2/session/1/events"),
    ("GET", "/twitch/api/v2/stream-report?streamer=earlysalty"),
    ("GET", "/twitch/api/v2/title/insights?streamer=earlysalty"),
    ("POST", "/twitch/api/v2/title/suggest"),
];

/// Social-Media-Routen: das Gate sitzt in den beiden Chokepoints
/// `require_sm_access` (Lesepfade) und `check_partner_access_guard`
/// (Schreibpfade), nicht im Handler. Je eine Route belegt einen davon.
const GATED_SOCIAL: &[(&str, &str)] = &[
    ("GET", "/social-media/api/streamer/clips"),
    ("GET", "/social-media/api/streamer/templates"),
    ("POST", "/social-media/api/streamer/templates"),
];

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

/// Isoliertes Schema mit genau den Tabellen, die VOR dem Gate gelesen werden:
/// Session-Store, Partner-Gate, Plan-Auflösung und die Social-Freigabe.
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
        r#"CREATE TABLE twitch_partners (
            twitch_login            TEXT NOT NULL PRIMARY KEY,
            twitch_user_id          TEXT,
            status                  TEXT,
            technical_pause_reason  TEXT,
            manual_partner_opt_out  INTEGER NOT NULL DEFAULT 0,
            departnered_at          TEXT,
            admin_archived_at       TEXT,
            partnered_at            TEXT
        )"#,
        r#"CREATE TABLE streamer_plans (
            twitch_user_id           TEXT,
            twitch_login             TEXT,
            manual_plan_id           TEXT,
            manual_plan_expires_at   TEXT,
            manual_plan_notes        TEXT,
            manual_plan_updated_at   TEXT
        )"#,
        r#"CREATE TABLE twitch_billing_subscriptions (
            customer_reference  TEXT,
            plan_id             TEXT,
            status              TEXT,
            current_period_end  TEXT,
            updated_at          TEXT
        )"#,
        r#"CREATE TABLE social_media_partner_access (
            streamer_login TEXT PRIMARY KEY,
            granted        BOOLEAN NOT NULL DEFAULT FALSE,
            granted_by     TEXT,
            granted_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
    ] {
        sqlx::query(ddl).execute(&pool).await.expect("DDL");
    }

    sqlx::query(
        "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status, manual_partner_opt_out)
         VALUES ($1, '42', 'active', 0)",
    )
    .bind(LOGIN)
    .execute(&pool)
    .await
    .expect("Partner anlegen");
    // Social-Freigabe erteilt: der Test soll am PLAN scheitern, nicht an der
    // Freigabe — sonst bewiese er das falsche 403.
    sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ($1, TRUE)")
        .bind(LOGIN)
        .execute(&pool)
        .await
        .expect("Social-Freigabe");

    Some(pool)
}

/// Router mit genau den geprüften Routen, gleiche Handler wie `lib.rs`.
fn make_router(pool: PgPool, auth_state: DashboardAuthState) -> Router {
    Router::new()
        .route(
            "/twitch/api/v2/monthly-stats",
            get(performance::monthly_stats_handler),
        )
        .route(
            "/twitch/api/v2/weekly-stats",
            get(performance::weekly_stats_handler),
        )
        .route(
            "/twitch/api/v2/hourly-heatmap",
            get(performance::hourly_heatmap_handler),
        )
        .route(
            "/twitch/api/v2/viewer-overlap",
            get(audience::viewer_overlap_handler),
        )
        .route("/twitch/api/v2/rankings", get(rankings::rankings_handler))
        .route(
            "/twitch/api/v2/chat-analytics",
            get(chat_analytics::chat_analytics_handler),
        )
        .route(
            "/twitch/api/v2/session/:id/events",
            get(session_detail::session_events_handler),
        )
        .route(
            "/twitch/api/v2/stream-report",
            get(stream_report::stream_report_handler),
        )
        .route(
            "/twitch/api/v2/title/insights",
            get(title::insights_handler),
        )
        .route("/twitch/api/v2/title/suggest", post(title::suggest_handler))
        .route(
            "/social-media/api/streamer/clips",
            get(social_media::clips_handler),
        )
        .route(
            "/social-media/api/streamer/templates",
            get(social_media::templates_streamer_handler)
                .post(social_media::create_template_handler),
        )
        .layer(axum::middleware::from_fn(
            crate::auth::partner_gate::partner_status_gate,
        ))
        .layer(Extension(auth_state))
        .with_state(pool)
}

fn req(method: &str, uri: &str, cookie: &str) -> Request<Body> {
    let addr: SocketAddr = PEER.parse().unwrap();
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .extension(ConnectInfo(addr))
        .header(header::HOST, HOST)
        .header("x-dashboard-context", "public")
        .header(header::COOKIE, cookie);
    if method == "POST" {
        builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"streamer":"earlysalty"}"#))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

/// `(status, error-Feld)` einer Antwort.
async fn call(
    pool: &PgPool,
    auth_state: &DashboardAuthState,
    cookie: &str,
    method: &str,
    uri: &str,
) -> (StatusCode, String) {
    let res = make_router(pool.clone(), auth_state.clone())
        .oneshot(req(method, uri, cookie))
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    let error = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .unwrap_or_default();
    (status, error)
}

async fn partner_cookie(auth_state: &DashboardAuthState) -> String {
    let session = auth_state
        .create_partner_session(LOGIN, "42", "EarlySalty")
        .await
        .unwrap();
    format!("{PARTNER_COOKIE_NAME}={}", session.session_id)
}

/// Richtung (a): Partner ohne Premium bekommt an JEDEM neu gegateten Endpunkt
/// die Absage — über echtes HTTP, nicht über das Frontend.
#[tokio::test]
async fn ohne_premium_liefert_jeder_gegatete_endpunkt_plan_required() {
    let Some(pool) = make_pool("t_premium_gate_deny").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
    let cookie = partner_cookie(&auth_state).await;

    for (method, uri) in GATED {
        let (status, error) = call(&pool, &auth_state, &cookie, method, uri).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} antwortet ohne Premium mit {status}"
        );
        assert_eq!(error, "plan_required", "{method} {uri} → falscher Grund");
    }

    // Social-Media-Chokepoints nutzen dieselbe Fehlerkennung.
    for (method, uri) in GATED_SOCIAL {
        let (status, error) = call(&pool, &auth_state, &cookie, method, uri).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} antwortet ohne Premium mit {status}"
        );
        assert_eq!(error, "plan_required", "{method} {uri} → falscher Grund");
    }
}

/// Richtung (b): mit Premium ist die Antwort nicht mehr das Plan-Gate.
/// Beweist, dass Richtung (a) am Plan hängt und nicht an Auth, Route oder
/// fehlender Freigabe.
#[tokio::test]
async fn mit_premium_ist_die_absage_weg() {
    let Some(pool) = make_pool("t_premium_gate_allow").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    sqlx::query("INSERT INTO streamer_plans (twitch_login, manual_plan_id) VALUES ($1, 'premium')")
        .bind(LOGIN)
        .execute(&pool)
        .await
        .unwrap();
    let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
    let cookie = partner_cookie(&auth_state).await;

    for (method, uri) in GATED.iter().chain(GATED_SOCIAL) {
        let (status, error) = call(&pool, &auth_state, &cookie, method, uri).await;
        assert_ne!(
            (status, error.as_str()),
            (StatusCode::FORBIDDEN, "plan_required"),
            "{method} {uri} sperrt trotz Premium"
        );
    }
}

/// Bestandsschutz: ein alter Plan aus der DB (hier `analysis_dashboard`) öffnet
/// dieselben Endpunkte. Die Migration in Milestone 5 darf nicht die
/// Voraussetzung dafür sein, dass ein zahlender Bestandskunde weiterkommt.
#[tokio::test]
async fn alter_bezahlplan_oeffnet_die_gates_ebenfalls() {
    let Some(pool) = make_pool("t_premium_gate_legacy").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_login, manual_plan_id) VALUES ($1, 'analysis_dashboard')",
    )
    .bind(LOGIN)
    .execute(&pool)
    .await
    .unwrap();
    let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
    let cookie = partner_cookie(&auth_state).await;

    for (method, uri) in GATED.iter().chain(GATED_SOCIAL) {
        let (status, error) = call(&pool, &auth_state, &cookie, method, uri).await;
        assert_ne!(
            (status, error.as_str()),
            (StatusCode::FORBIDDEN, "plan_required"),
            "{method} {uri} sperrt einen alten Bezahlplan aus"
        );
    }
}

/// Gegenprobe ueber einen echten TCP-Socket statt `oneshot`: der Router wird
/// auf einem freien Port serviert und mit `reqwest` angesprochen. Damit haengt
/// der Beweis an keiner Test-Abkuerzung und an keinem Frontend-Code — genau
/// der Weg, den ein Nutzer mit `curl` und seinem Session-Cookie gehen wuerde.
#[tokio::test]
async fn direkter_http_aufruf_am_frontend_vorbei_wird_abgewiesen() {
    let Some(pool) = make_pool("t_premium_gate_socket").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
    let cookie = partner_cookie(&auth_state).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = make_router(pool.clone(), auth_state.clone())
        .into_make_service_with_connect_info::<SocketAddr>();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let client = reqwest::Client::new();
    let res = client
        .get(format!(
            "http://{addr}/twitch/api/v2/monthly-stats?streamer={LOGIN}"
        ))
        .header(reqwest::header::COOKIE, &cookie)
        .header("x-dashboard-context", "public")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 403);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "plan_required");
    assert_eq!(body["required_entitlements"], serde_json::json!(["analytics"]));

    server.abort();
}

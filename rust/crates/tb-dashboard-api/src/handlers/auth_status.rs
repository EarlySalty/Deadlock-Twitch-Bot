//! Handler für `GET /twitch/api/v2/auth-status`.
//!
//! Port von `bot/analytics/api_v2.py:_api_v2_auth_status` (2707–2844 Z.)
//!
//! Gibt 18+ Felder zurück — vollständige Python-Parität für das Streamer-
//! und Admin-Dashboard. Unauthentifizierte Requests werden mit 5s gecachtem
//! All-null-Payload beantwortet.

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use std::sync::OnceLock;
use tokio::sync::Mutex;

use crate::auth::level::DashboardAuthLevel;

// ── Konstanten ──────────────────────────────────────────────────────────────

/// Cache-Control für unauthentifizierte auth-status Antworten.
const UNAUTH_CACHE_CONTROL: &str = "public, max-age=5, stale-while-revalidate=5";

/// TTL für den In-Process-Cache des unauth-Payloads (Sekunden).
const UNAUTH_CACHE_TTL_SECS: u64 = 5;

/// Admin-Login aus Python `_TWITCH_ADMIN_LOGINS = frozenset({"earlysalty"})`.
///
/// Wird beim Admin-Login als `adminDefaultStreamer` zurückgegeben damit das
/// Dashboard auf den eigenen Kanal defaultet statt auf den ersten Partner.
const ADMIN_DEFAULT_STREAMER: &str = "earlysalty";

// ── Unauth-Cache ────────────────────────────────────────────────────────────

struct UnauthCache {
    payload: Option<serde_json::Value>,
    created_at: u64,
}

fn unauth_cache() -> &'static Mutex<UnauthCache> {
    static CACHE: OnceLock<Mutex<UnauthCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(UnauthCache {
            payload: None,
            created_at: 0,
        })
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Handler ─────────────────────────────────────────────────────────────────

/// `GET /twitch/api/v2/auth-status`
///
/// Kein Auth-Gate — antwortet immer 200, aber mit unterschiedlichem Payload
/// je nach Auth-Level.
pub async fn auth_status_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    match &auth {
        DashboardAuthLevel::None => unauth_response().await,
        DashboardAuthLevel::Localhost => admin_response("localhost"),
        DashboardAuthLevel::Admin => admin_response("admin"),
        DashboardAuthLevel::Partner { twitch_login, twitch_user_id } => {
            partner_response(&pool, twitch_login, twitch_user_id).await
        }
    }
}

// ── Unauthentifiziert ───────────────────────────────────────────────────────

async fn unauth_response() -> Response {
    let now = now_secs();
    {
        let mut cache = unauth_cache().lock().await;
        if let Some(ref payload) = cache.payload {
            if now - cache.created_at < UNAUTH_CACHE_TTL_SECS {
                return cached_json_response(payload.clone(), UNAUTH_CACHE_CONTROL);
            }
        }
        let payload = unauth_payload();
        cache.payload = Some(payload.clone());
        cache.created_at = now;
        cached_json_response(payload, UNAUTH_CACHE_CONTROL)
    }
}

fn unauth_payload() -> serde_json::Value {
    json!({
        "authenticated": false,
        "level": "none",
        "authLevel": "none",
        "demoMode": false,
        "isAdmin": false,
        "isLocalhost": false,
        "canViewAllStreamers": false,
        "twitchLogin": null,
        "adminDefaultStreamer": null,
        "displayName": null,
        "partnerStatus": null,
        "technicalPauseReason": null,
        "operationalState": null,
        "canAccessAnalyticsDashboard": false,
        "tokenErrorGraceExpiresAt": null,
        "csrfToken": null,
        "csrf_token": null,
        "plan": null,
        "access": {
            "landing": false,
            "analytics": false,
        },
        "permissions": {
            "viewAllStreamers": false,
            "viewComparison": false,
            "viewChatAnalytics": false,
            "viewOverlap": false,
        },
    })
}

// ── Admin / Localhost ───────────────────────────────────────────────────────

fn admin_response(level: &'static str) -> Response {
    let plan = json!({
        "planId": "analysis_dashboard",
        "planName": "Erweitert (Admin)",
        "tier": "extended",
        "isExtended": true,
        "expiresAt": null,
        "source": "admin",
        "entitlements": tb_analytics::plan::plan_entitlements("bundle_analysis_raid_boost"),
    });
    let is_localhost = level == "localhost";
    let payload = json!({
        "authenticated": true,
        "level": level,
        "authLevel": level,
        "demoMode": false,
        "isAdmin": true,
        "isLocalhost": is_localhost,
        "canViewAllStreamers": true,
        "twitchLogin": null,
        "adminDefaultStreamer": ADMIN_DEFAULT_STREAMER,
        "displayName": null,
        "partnerStatus": "active",
        "technicalPauseReason": null,
        "operationalState": "active",
        "canAccessAnalyticsDashboard": true,
        "tokenErrorGraceExpiresAt": null,
        "csrfToken": null,
        "csrf_token": null,
        "plan": plan,
        "access": {
            "landing": true,
            "analytics": true,
        },
        "permissions": {
            "viewAllStreamers": true,
            "viewComparison": true,
            "viewChatAnalytics": true,
            "viewOverlap": true,
        },
    });
    Json(payload).into_response()
}

// ── Partner-Session ─────────────────────────────────────────────────────────

async fn partner_response(pool: &PgPool, login: &str, user_id: &str) -> Response {
    let access = tb_analytics::partner_access::load_partner_access_state(pool, login, user_id)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("auth-status: Partner-Access-State-Fehler für {login}: {e}");
            // Fail-open: als aktiv behandeln damit der Streamer nicht
            // versehentlich ausgesperrt wird
            tb_analytics::partner_access::AccessState {
                partner_status: "active".to_string(),
                analytics_access_allowed: true,
                landing_access_allowed: true,
                ..Default::default()
            }
        });

    let plan = tb_analytics::plan::resolve_plan_snapshot(pool, login, user_id)
        .await
        .ok()
        .map(|p| {
            json!({
                "planId": p.plan_id,
                "planName": p.plan_name,
                "tier": p.tier,
                "isExtended": p.is_extended,
                "expiresAt": p.expires_at,
                "source": p.source,
                "entitlements": p.entitlements,
            })
        });

    let can_analytics = access.analytics_access_allowed;
    let payload = json!({
        "authenticated": true,
        "level": "partner",
        "authLevel": "partner",
        "demoMode": false,
        "isAdmin": false,
        "isLocalhost": false,
        "canViewAllStreamers": false,
        "twitchLogin": login,
        "adminDefaultStreamer": null,
        "displayName": null,     // in PartnerSession nicht gespeichert; Frontend liest aus twitchLogin
        "partnerStatus": access.partner_status,
        "technicalPauseReason": access.technical_pause_reason,
        "operationalState": access.operational_state,
        "canAccessAnalyticsDashboard": can_analytics,
        "tokenErrorGraceExpiresAt": access.token_error_grace_expires_at,
        "csrfToken": null,
        "csrf_token": null,
        "plan": plan,
        "access": {
            "landing": access.landing_access_allowed,
            "analytics": can_analytics,
        },
        "permissions": {
            "viewAllStreamers": false,
            "viewComparison": can_analytics,
            "viewChatAnalytics": can_analytics,
            "viewOverlap": can_analytics,
        },
    });
    Json(payload).into_response()
}

// ── Hilfsroutine ────────────────────────────────────────────────────────────

fn cached_json_response(payload: serde_json::Value, cache_control: &'static str) -> Response {
    let body = serde_json::to_vec(&payload).unwrap_or_default();
    (
        axum::http::StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, cache_control),
        ],
        body,
    )
        .into_response()
}

// ── Typen (für Tests) ───────────────────────────────────────────────────────

/// Vereinfachte Response-Struktur für Tests.
#[derive(Debug, Serialize)]
pub struct AuthStatusFields {
    pub authenticated: bool,
    pub level: String,
    pub is_admin: bool,
    pub can_access_analytics_dashboard: bool,
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
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    fn make_router(token: &str) -> Router {
        // Kein echtes Pool in Unit-Tests — nur auth-level basierte Tests
        // (Localhost/Admin). Partner-Tests brauchen DB-Integration.
        let pool = sqlx::Pool::connect_lazy_with(
            sqlx::postgres::PgConnectOptions::new(),
        );
        Router::new()
            .route("/twitch/api/v2/auth-status", get(auth_status_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn req(ip: &str, host: &str, token: Option<&str>) -> Request<Body> {
        let addr: SocketAddr = format!("{}:9999", ip).parse().unwrap();
        let mut b = Request::builder()
            .uri("/twitch/api/v2/auth-status")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, host);
        if let Some(t) = token {
            b = b.header("x-internal-token", t);
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn unauth_returns_all_false() {
        let app = make_router("tok");
        let res = app.oneshot(req("1.2.3.4", "example.com", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["authenticated"], false);
        assert_eq!(v["level"], "none");
        assert_eq!(v["isAdmin"], false);
        assert_eq!(v["canAccessAnalyticsDashboard"], false);
        assert!(v["plan"].is_null());
        assert!(v["twitchLogin"].is_null());
    }

    // Hinweis: Token-basierter Admin-Zugang (X-Internal-Token) ist NICHT in
    // DashboardAuthLevel implementiert — auth-status ist ausschließlich Browser-
    // seitig (Cookie-Auth). Python prüft zusätzlich X-Admin-Token, aber dieses
    // Feature ist für auth-status nicht gebraucht (kein interner Konsument).

    #[tokio::test]
    async fn localhost_returns_localhost_level() {
        let app = make_router("tok");
        let addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/v2/auth-status")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "localhost")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["level"], "localhost");
        assert_eq!(v["isLocalhost"], true);
        assert_eq!(v["isAdmin"], true);
        assert_eq!(v["canViewAllStreamers"], true);
    }

    #[tokio::test]
    async fn unauth_has_correct_shape() {
        let app = make_router("tok");
        let res = app.oneshot(req("1.2.3.4", "example.com", None)).await.unwrap();
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        // Alle erwarteten Felder müssen vorhanden sein
        for field in &[
            "authenticated", "level", "authLevel", "demoMode", "isAdmin",
            "isLocalhost", "canViewAllStreamers", "twitchLogin", "adminDefaultStreamer",
            "displayName", "partnerStatus", "technicalPauseReason", "operationalState",
            "canAccessAnalyticsDashboard", "tokenErrorGraceExpiresAt", "csrfToken",
            "csrf_token", "plan", "access", "permissions",
        ] {
            assert!(v.get(field).is_some(), "Feld '{field}' fehlt");
        }
        // access und permissions müssen die richtigen Unterfelder haben
        assert!(v["access"]["landing"].is_boolean());
        assert!(v["access"]["analytics"].is_boolean());
        assert!(v["permissions"]["viewAllStreamers"].is_boolean());
        assert!(v["permissions"]["viewComparison"].is_boolean());
    }
}

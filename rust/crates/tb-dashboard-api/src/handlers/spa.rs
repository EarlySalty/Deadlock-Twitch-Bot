//! SPA-Handler für `/analyse` (Haupt-HTML) und `/analyse/{path:.*}` (Assets).
//!
//! Port von `bot/analytics/api_overview.py:_serve_dashboard_v2` (Z. 542–587)
//! und `_serve_dashboard_v2_assets` / `_resolve_dashboard_v2_asset_response`
//! (Z. 807–879).
//!
//! Auth-Flow:
//! - None       → Redirect → /twitch/auth/login?next=%2Fanalyse
//! - Admin/Localhost → immer erlaubt
//! - Partner    → landing_access_allowed? → analytics_access_allowed?
//!               (bei false → Redirect → /twitch/dashboard)
//!
//! Dist-Pfad: Env `DASHBOARD_V2_DIST_PATH`, Default `bot/analytics/dashboard_v2/dist`
//! (relativ zum WorkingDirectory des Service = Repo-Root).

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use sqlx::PgPool;
use std::path::PathBuf;

use crate::auth::level::DashboardAuthLevel;

// ── Konstanten ──────────────────────────────────────────────────────────────

const LOGIN_URL: &str = "/twitch/auth/login?next=%2Fanalyse";
const LEGACY_DASHBOARD_URL: &str = "/twitch/dashboard";
const DEFAULT_DIST_PATH: &str = "bot/analytics/dashboard_v2/dist";

/// Inline-Script das die React-App über apiBase und demoMode informiert.
/// Python: `_dashboard_runtime_script` + `_inject_dashboard_runtime_config`.
const RUNTIME_SCRIPT: &str = concat!(
    "<script>window.__TWITCH_DASHBOARD_RUNTIME__=Object.freeze(",
    r#"{"apiBase":"/twitch/api/v2","demoMode":false,"allowedDemoProfiles":[]}"#,
    ");</script>",
);

// ── Handler ─────────────────────────────────────────────────────────────────

/// `GET /analyse` — Haupt-HTML mit injizierten Runtime-Daten.
pub async fn analyse_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    if let Some(r) = check_spa_auth(&auth, &pool).await {
        return r;
    }

    let index = dist_root().join("index.html");
    let html = match tokio::fs::read_to_string(&index).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                "Dashboard not built. Run npm run build in dashboard_v2/",
            )
                .into_response()
        }
    };

    // Vite baut die Assets mit Prefix /twitch/dashboard-v2/; für /analyse ersetzen.
    let html = html.replace("/twitch/dashboard-v2/", "/analyse/");
    // Runtime-Script vor </head> injizieren (erstes Vorkommen).
    let html = html.replacen("</head>", &format!("{RUNTIME_SCRIPT}\n  </head>"), 1);

    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

/// `GET /analyse/{path:.*}` — statische Assets aus dist/.
pub async fn analyse_assets_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(asset_path): Path<String>,
) -> Response {
    if let Some(r) = check_spa_auth(&auth, &pool).await {
        return r;
    }
    // axum 0.7 liefert bei `/*path` den Wert mit führendem `/`
    serve_asset(asset_path.trim_start_matches('/')).await
}

// ── Auth-Prüfung ─────────────────────────────────────────────────────────────

/// Gibt `Some(Response)` zurück wenn der Zugriff verweigert wird, sonst `None`.
async fn check_spa_auth(auth: &DashboardAuthLevel, pool: &PgPool) -> Option<Response> {
    match auth {
        DashboardAuthLevel::None => Some(Redirect::to(LOGIN_URL).into_response()),
        DashboardAuthLevel::Localhost | DashboardAuthLevel::Admin => None,
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
        } => {
            let access =
                tb_analytics::partner_access::load_partner_access_state(pool, twitch_login, twitch_user_id)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!("spa: Partner-Access-Fehler für {twitch_login}: {e}");
                        tb_analytics::partner_access::AccessState {
                            analytics_access_allowed: true,
                            landing_access_allowed: true,
                            ..Default::default()
                        }
                    });

            if !access.landing_access_allowed {
                return Some(
                    (
                        StatusCode::FORBIDDEN,
                        "This account is blocked from all dashboard surfaces.",
                    )
                        .into_response(),
                );
            }
            if !access.analytics_access_allowed {
                return Some(Redirect::to(LEGACY_DASHBOARD_URL).into_response());
            }
            None
        }
    }
}

// ── Asset-Serving ─────────────────────────────────────────────────────────────

fn dist_root() -> PathBuf {
    let base = std::env::var("DASHBOARD_V2_DIST_PATH")
        .unwrap_or_else(|_| DEFAULT_DIST_PATH.to_string());
    PathBuf::from(base)
}

/// Dient eine Datei aus `dist/` mit strikter Pfad-Validierung.
///
/// Jedes Segment wird gegen `.`, `..` und `\` geprüft (Python-Parität).
/// Symlink-Angriffe sind bei eigenem Build-Output kein reales Angriffsszenario.
async fn serve_asset(raw_path: &str) -> Response {
    let dist = dist_root();

    // Segmentweise Validierung — verhindert Path-Traversal
    let mut candidate = dist.clone();
    for segment in raw_path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
        candidate.push(segment);
    }

    let data = match tokio::fs::read(&candidate).await {
        Ok(d) => d,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };

    let mime = mime_for_path(&candidate);
    ([( header::CONTENT_TYPE, mime)], data).into_response()
}

fn mime_for_path(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("json") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

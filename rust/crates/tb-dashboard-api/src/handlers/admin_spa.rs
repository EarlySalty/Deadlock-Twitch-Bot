//! Admin-SPA + Entry-/Redirect-Routen nativ servieren (B1-ADMIN-SPA + B1-ENTRY).
//!
//! Bisher fielen `/twitch/admin`, `/twitch/admin/*` und die Entry-Redirects in
//! den Strangler-Proxy → toter Python-Service (8765) → 502 auf dem Admin-Host.
//! Dieses Modul liefert sie nativ:
//!
//! - **Admin-SPA**: `/twitch/admin` (Shell), `/twitch/admin/assets/*` (Bundles),
//!   `/twitch/admin/*` (SPA-Deep-Link-Fallback → liefert die Shell, damit der
//!   Client-Router greift). Auth: nur Admin (forward_auth-gleiche Bedingung);
//!   Partner/None → 401.
//! - **Entry/Redirect-Routen**: `/`, `/twitch`, `/twitch/`, `/twitch/stats`,
//!   diverse `/twitch/dashboard`-Aliase → 302 auf das kanonische Ziel.
//!
//! Python-Referenz:
//! - SPA-Serving: `bot/analytics/api_overview.py:635-660,831-839,932-961`
//!   (`_serve_admin_dashboard`, `_serve_admin_dashboard_path`,
//!   `_serve_admin_dashboard_assets`, `_resolve_admin_dashboard_asset_response`),
//!   Gate `_admin_dashboard_spa_gate` (Z. 400-449).
//! - Entry-Redirects: `bot/dashboard/routes_entry.py:57-104`
//!   (`index`, `public_home`, `legacy_dashboard_redirect`, `legacy_admin*`).
//!
//! **Architektur (Block 1):** Admin als SPA + native JSON-API — KEIN
//! server-gerendertes HTML kopieren. Diese Datei liefert nur die SPA-Shell +
//! Entry-Routen. Die Schreib-Aktionen (`add_streamer`/`verify`/`archive`/
//! `discord_flag`/`manual-plan`) sind SEPARATE Folge-Tickets
//! (B1-ADD-STREAMER / B1-VERIFY / B1-ARCHIVE / B1-DISCORD-LINK / B1-MANUAL-PLAN)
//! und hier bewusst NICHT enthalten.
//!
//! **SPA-Asset-Mechanik** (gleich wie `spa.rs` für `/analyse`): Dist-Verzeichnis
//! `bot/admin_dashboard/dist` (Env-Override `ADMIN_DASHBOARD_DIST_PATH`). Die
//! Vite-Build referenziert Assets bereits absolut unter `/twitch/admin/assets/`
//! (kein HTML-Rewrite nötig, anders als bei `/analyse`). Segmentweise
//! Pfad-Validierung gegen `.`/`..`/`\` schützt vor Path-Traversal.

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use std::path::PathBuf;

use crate::auth::level::DashboardAuthLevel;

/// Default-Dist-Pfad der Admin-SPA (relativ zum Service-WorkingDir = Repo-Root).
const DEFAULT_ADMIN_DIST_PATH: &str = "bot/admin_dashboard/dist";

/// Kanonisches Ziel für `/twitch/stats` + Dashboard-Aliase (Python:
/// `legacy_dashboard_redirect`, routes_entry.py:80-86).
const CANONICAL_DASHBOARD: &str = "/twitch/dashboard";

// ── Entry-/Redirect-Routen (B1-ENTRY) ────────────────────────────────────────

/// `GET /` — Root-Einstieg.
///
/// Python `public_home` (routes_entry.py:66-77) verzweigt für privilegierte
/// Requests zu `/twitch/admin`, sonst → `/twitch/dashboard`. Wir bilden das über das
/// Auth-Level ab (privilegiert → Admin-Landing, sonst Nutzer-Dashboard).
pub async fn root_handler(auth: DashboardAuthLevel) -> Response {
    let target = if auth.is_privileged() {
        "/twitch/admin"
    } else {
        CANONICAL_DASHBOARD
    };
    Redirect::to(target).into_response()
}

/// `GET /twitch` + `/twitch/` — Einstiegspunkt, leitet immer aufs Dashboard
/// (Python `index`, routes_entry.py:57-63).
pub async fn twitch_index_handler() -> Response {
    Redirect::to(CANONICAL_DASHBOARD).into_response()
}

/// `GET /twitch/stats` und Dashboard-Aliase (`/twitch/dashboards`,
/// `/twitch/dashboads`, `/dashboards`, `/dashboads`) → kanonisches Dashboard
/// (Python `legacy_dashboard_redirect`, routes_entry.py:80-86).
pub async fn dashboard_redirect_handler() -> Response {
    Redirect::to(CANONICAL_DASHBOARD).into_response()
}

// ── Admin-SPA (B1-ADMIN-SPA) ──────────────────────────────────────────────────

/// `GET /twitch/admin` — Admin-SPA-Shell (index.html).
///
/// Auth-Gate VOR dem Serving: nur Admin. Partner/None → 401
/// (Caddy gated den Admin-Host schon per forward_auth; dieser Check ist
/// Defense-in-Depth, falls der Endpoint direkt erreicht wird).
pub async fn admin_index_handler(auth: DashboardAuthLevel) -> Response {
    if let Some(denied) = admin_auth_gate(&auth) {
        return denied;
    }
    serve_admin_index().await
}

/// `GET /twitch/admin/{path:.*}` — SPA-Deep-Link-Fallback + statische Assets.
///
/// - `assets/...` oder Pfade mit Datei-Endung → statische Datei aus dist/.
/// - sonst (SPA-Route ohne Punkt im letzten Segment) → index.html (Client-Router).
///
/// Python: `_serve_admin_dashboard_path` (api_overview.py:649-660) +
/// `_admin_dashboard_path_should_serve_index` (Z. 451-458).
pub async fn admin_path_handler(auth: DashboardAuthLevel, Path(raw_path): Path<String>) -> Response {
    if let Some(denied) = admin_auth_gate(&auth) {
        return denied;
    }
    // axum 0.7 liefert bei `/*path` den Wert mit führendem `/`.
    let trimmed = raw_path.trim_start_matches('/');
    if path_should_serve_index(trimmed) {
        serve_admin_index().await
    } else {
        serve_admin_asset(trimmed).await
    }
}

// ── Auth-Gate ─────────────────────────────────────────────────────────────────

/// Gibt `Some(401)` zurück, wenn der Zugriff verweigert wird, sonst `None`.
///
/// Bedingung wie der forward_auth-Endpoint (`is_privileged`): nur Admin darf
/// die Admin-SPA sehen. Python `_admin_dashboard_spa_gate`
/// mischt zusätzlich Host-/Login-Redirect-Logik ein; nativ übernimmt Caddy die
/// Host-Trennung + forward_auth-Redirect, hier bleibt nur der harte Auth-Check.
fn admin_auth_gate(auth: &DashboardAuthLevel) -> Option<Response> {
    if auth.is_privileged() {
        None
    } else {
        Some(
            (
                StatusCode::UNAUTHORIZED,
                "Admin access required.",
            )
                .into_response(),
        )
    }
}

/// Entscheidet, ob für einen SPA-Pfad die index.html (Client-Router) oder eine
/// statische Datei ausgeliefert wird. Port von Python
/// `_admin_dashboard_path_should_serve_index` (api_overview.py:451-458):
/// - leer → index
/// - `assets`/`assets/...` → statische Datei
/// - letztes Segment ohne `.` → index (Deep-Link), sonst statische Datei.
fn path_should_serve_index(raw_path: &str) -> bool {
    let normalized = raw_path.trim_matches('/');
    if normalized.is_empty() {
        return true;
    }
    if normalized == "assets" || normalized.starts_with("assets/") {
        return false;
    }
    let last_segment = normalized.rsplit('/').next().unwrap_or(normalized);
    !last_segment.contains('.')
}

// ── Asset-Serving (Mechanik gespiegelt von spa.rs) ────────────────────────────

fn admin_dist_root() -> PathBuf {
    let base = std::env::var("ADMIN_DASHBOARD_DIST_PATH")
        .unwrap_or_else(|_| DEFAULT_ADMIN_DIST_PATH.to_string());
    PathBuf::from(base)
}

/// Liefert die Admin-SPA-Shell (`index.html`). 404 mit Build-Hinweis, wenn das
/// Dist-Verzeichnis nicht gebaut ist (Python: gleiche Meldung, Z. 644-646).
async fn serve_admin_index() -> Response {
    let index = admin_dist_root().join("index.html");
    match tokio::fs::read(&index).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            "Admin dashboard not built. Run npm run build in bot/admin_dashboard/",
        )
            .into_response(),
    }
}

/// Dient eine statische Datei aus dem Admin-Dist mit strikter Pfad-Validierung.
///
/// Jedes Segment wird gegen leer, `.`, `..` und `\` geprüft (Python-Parität,
/// `_resolve_admin_dashboard_asset_response`). Symlink-Angriffe sind bei eigenem
/// Build-Output kein reales Szenario.
async fn serve_admin_asset(raw_path: &str) -> Response {
    let mut candidate = admin_dist_root();
    for segment in raw_path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
        candidate.push(segment);
    }

    match tokio::fs::read(&candidate).await {
        Ok(data) => {
            let mime = mime_for_path(&candidate);
            ([(header::CONTENT_TYPE, mime)], data).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Entry-Redirects ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn twitch_index_redirectet_aufs_dashboard() {
        let resp = twitch_index_handler().await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            CANONICAL_DASHBOARD
        );
    }

    #[tokio::test]
    async fn stats_redirectet_aufs_dashboard() {
        let resp = dashboard_redirect_handler().await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            CANONICAL_DASHBOARD
        );
    }

    #[tokio::test]
    async fn root_privilegiert_geht_zu_admin() {
        let resp = root_handler(DashboardAuthLevel::admin()).await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/twitch/admin");
    }

    #[tokio::test]
    async fn root_unprivilegiert_geht_zum_dashboard() {
        let resp = root_handler(DashboardAuthLevel::None).await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            CANONICAL_DASHBOARD
        );
    }

    // ── Admin-SPA-Auth-Gate ──────────────────────────────────────────────────

    #[tokio::test]
    async fn admin_index_ohne_admin_401() {
        // None → 401, KEIN 502/Proxy.
        let resp = admin_index_handler(DashboardAuthLevel::None).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_index_partner_401() {
        let resp = admin_index_handler(DashboardAuthLevel::Partner {
            twitch_login: "p".into(),
            twitch_user_id: "1".into(),
            display_name: String::new(),
        })
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_index_admin_liefert_shell_kein_proxy() {
        // Mit echtem Dist liefert es die Shell (200), ohne gebauten Dist 404 —
        // in BEIDEN Fällen KEIN 401/502/Proxy. Der Auth-Gate ist passiert.
        let resp = admin_index_handler(DashboardAuthLevel::admin()).await;
        assert!(
            resp.status() == StatusCode::OK || resp.status() == StatusCode::NOT_FOUND,
            "Admin darf die Shell sehen (200) oder 404 bei fehlendem Build — nie 401/502, war {}",
            resp.status()
        );
        assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_path_deeplink_admin_serviert_index_kein_proxy() {
        // Deep-Link ohne Dateiendung → Shell-Serving-Pfad, Auth passiert.
        let resp = admin_path_handler(
            DashboardAuthLevel::admin(),
            Path("/streamers".to_string()),
        )
        .await;
        assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_path_ohne_admin_401() {
        let resp =
            admin_path_handler(DashboardAuthLevel::None, Path("/assets/x.js".to_string())).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── path_should_serve_index (Python-Parität) ─────────────────────────────

    #[test]
    fn should_serve_index_logik() {
        // Leer / führender Slash → Shell.
        assert!(path_should_serve_index(""));
        assert!(path_should_serve_index("/"));
        // Deep-Link ohne Punkt → Shell.
        assert!(path_should_serve_index("streamers"));
        assert!(path_should_serve_index("billing/overview"));
        // Assets → statische Datei.
        assert!(!path_should_serve_index("assets"));
        assert!(!path_should_serve_index("assets/index-abc.js"));
        // Datei mit Endung (letztes Segment hat Punkt) → statische Datei.
        assert!(!path_should_serve_index("favicon.ico"));
        assert!(!path_should_serve_index("nested/logo.png"));
    }

    // ── Path-Traversal-Schutz ────────────────────────────────────────────────

    #[tokio::test]
    async fn asset_path_traversal_404() {
        let resp = serve_admin_asset("../../etc/passwd").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp2 = serve_admin_asset("assets/../../secret").await;
        assert_eq!(resp2.status(), StatusCode::NOT_FOUND);
    }
}

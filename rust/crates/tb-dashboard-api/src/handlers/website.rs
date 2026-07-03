//! Öffentliche Website (`/streamer`) + Legacy-Redirect (`/website`) — P2.67.
//!
//! Port von `analytics/api_overview.py`:
//! - `_serve_website_dist_asset` / `_resolve_website_dist_asset_response`
//!   (statische Auslieferung aus `website/dist`, Verzeichnis → `index.html`).
//! - `_redirect_public_website_root` (`/streamer` → `/streamer/`).
//! - `_redirect_legacy_website_path` (`/website` + `/website/{path}` → `/streamer/{path}`,
//!   301, Query-String erhalten).
//!
//! `/streamer` ist die öffentliche Streamer-Onboarding-/Info-Seite (kein Login).
//! Die statische Auslieferung erfolgt aus `website/dist` (Env-Override
//! `WEBSITE_DIST_PATH`); ein Verzeichnis-Treffer liefert dessen `index.html`
//! (SPA-Routing). Segmentweise Pfad-Validierung gegen `.`/`..`/`\` schützt vor
//! Path-Traversal.

use axum::{
    extract::Path,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Redirect, Response},
};
use std::path::PathBuf;

/// Default-Dist-Pfad der öffentlichen Website (relativ zum Service-WorkingDir =
/// Repo-Root). Python: `WEBSITE_DIST_ROOT_PATH = .../website/dist`.
const DEFAULT_WEBSITE_DIST_PATH: &str = "website/dist";

/// Kanonischer Basis-Pfad der öffentlichen Website.
const PUBLIC_WEBSITE_BASE_PATH: &str = "/streamer";

// ── /streamer (statische Auslieferung) ────────────────────────────────────────

/// `GET /streamer` — Redirect auf den kanonischen Trailing-Slash-Root.
///
/// Python: `_redirect_public_website_root` (301 → `/streamer/`, Query erhalten).
pub async fn streamer_root_handler(uri: Uri) -> Response {
    Redirect::permanent(&with_query(format!("{PUBLIC_WEBSITE_BASE_PATH}/"), &uri)).into_response()
}

/// `GET /streamer/{path:.*}` — statische Datei aus `website/dist`.
///
/// Verzeichnis-Treffer → `index.html` (SPA-Fallback). Python:
/// `_resolve_website_dist_asset_response`.
pub async fn streamer_asset_handler(Path(raw_path): Path<String>) -> Response {
    // axum 0.7 liefert bei `/*path` den Wert mit führendem `/`.
    serve_website_asset(website_dist_root(), raw_path.trim_start_matches('/')).await
}

// ── /website (Legacy-Redirect) ────────────────────────────────────────────────

/// `GET /website` — 301 auf `/streamer/` (Query erhalten).
///
/// Python: `_redirect_legacy_website_path` mit leerem Pfad.
pub async fn website_root_redirect_handler(uri: Uri) -> Response {
    Redirect::permanent(&with_query(format!("{PUBLIC_WEBSITE_BASE_PATH}/"), &uri)).into_response()
}

/// `GET /website/{path:.*}` — 301 auf `/streamer/{path}` (Query erhalten).
///
/// Python: `_redirect_legacy_website_path` / `_build_public_website_redirect_location`.
pub async fn website_path_redirect_handler(Path(raw_path): Path<String>, uri: Uri) -> Response {
    let normalized = raw_path.trim_start_matches('/');
    let location = if normalized.is_empty() {
        format!("{PUBLIC_WEBSITE_BASE_PATH}/")
    } else {
        format!("{PUBLIC_WEBSITE_BASE_PATH}/{normalized}")
    };
    Redirect::permanent(&with_query(location, &uri)).into_response()
}

// ── Helfer ────────────────────────────────────────────────────────────────────

/// Hängt den Query-String des ursprünglichen Requests an das Redirect-Ziel an
/// (Python erhält `request.query_string`).
fn with_query(location: String, uri: &Uri) -> String {
    match uri.query() {
        Some(q) if !q.is_empty() => format!("{location}?{q}"),
        _ => location,
    }
}

pub(crate) fn website_dist_root() -> PathBuf {
    let base = std::env::var("WEBSITE_DIST_PATH")
        .unwrap_or_else(|_| DEFAULT_WEBSITE_DIST_PATH.to_string());
    PathBuf::from(base)
}

/// Löst eine Datei aus `website/dist` mit strikter Pfad-Validierung auf.
///
/// Jedes Segment wird gegen leer, `.`, `..` und `\` geprüft (Python-Parität).
/// Trifft der aufgelöste Pfad ein Verzeichnis, wird dessen `index.html`
/// ausgeliefert (SPA-Routing). Symlink-Angriffe sind bei eigenem Build-Output
/// kein reales Szenario.
async fn serve_website_asset(dist_root: PathBuf, raw_path: &str) -> Response {
    let mut candidate = dist_root;
    if !raw_path.is_empty() {
        for segment in raw_path.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
                return (StatusCode::NOT_FOUND, "Not found").into_response();
            }
            candidate.push(segment);
        }
    }

    // Verzeichnis-Treffer → index.html.
    let target = if candidate.is_dir() {
        candidate.join("index.html")
    } else {
        candidate
    };

    let data = match tokio::fs::read(&target).await {
        Ok(d) => d,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    let mime = mime_for_path(&target);
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        data,
    )
        .into_response()
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
    use axum::http::Uri;

    #[test]
    fn query_wird_an_redirect_angehaengt() {
        let uri: Uri = "/website/foo?ref=x&y=1".parse().unwrap();
        assert_eq!(
            with_query("/streamer/foo".to_string(), &uri),
            "/streamer/foo?ref=x&y=1"
        );
        let no_q: Uri = "/website/foo".parse().unwrap();
        assert_eq!(with_query("/streamer/foo".to_string(), &no_q), "/streamer/foo");
    }

    #[tokio::test]
    async fn legacy_path_redirect_301_auf_streamer() {
        let uri: Uri = "/website/foo?a=1".parse().unwrap();
        let resp = website_path_redirect_handler(Path("foo".to_string()), uri).await;
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "/streamer/foo?a=1"
        );
    }

    #[tokio::test]
    async fn legacy_root_redirect_301() {
        let uri: Uri = "/website".parse().unwrap();
        let resp = website_root_redirect_handler(uri).await;
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/streamer/");
    }

    #[tokio::test]
    async fn traversal_blockiert() {
        let resp =
            serve_website_asset(std::env::temp_dir().join("tb_website_nope"), "../secret").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn verzeichnis_liefert_index_html() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tb_website_dist_{unique}"));
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("index.html"), b"<!doctype html>root")
            .await
            .unwrap();

        // Leerer Pfad → Root-Verzeichnis → index.html.
        let resp = serve_website_asset(root.clone(), "").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"<!doctype html>root");

        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}

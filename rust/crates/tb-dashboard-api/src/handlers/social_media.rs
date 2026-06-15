//! Social-Media-Dashboard (`/social-media/*`) — Port von
//! `bot/social_media/dashboard.py`.
//!
//! Dieser Slice deckt die HTML-Seiten ab:
//! - `GET /social-media/terms`   — Nutzungsbedingungen (öffentlich, für die
//!   Plattform-OAuth-Reviews von TikTok/YouTube/Instagram).
//! - `GET /social-media/privacy` — Datenschutzerklärung (öffentlich).
//! - `GET /social-media`         — Dashboard-SPA (Auth erforderlich).
//!
//! Die JSON-API-Endpoints (Stats/Clips/Upload/Layout/Vocab/Templates/OAuth)
//! folgen in weiteren Slices und nutzen den hier definierten
//! [`resolve_streamer_scope`]-Helfer.

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde_json::json;
use tb_social_media::rendering::{render_dashboard, render_privacy, render_terms};

use crate::auth::level::DashboardAuthLevel;

/// HTML-Escape (`&`, `<`, `>`, `"`, `'`) — mirror von Pythons `html.escape(..., quote=True)`.
fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

fn forbidden(message: &str) -> Response {
    (StatusCode::FORBIDDEN, message.to_string()).into_response()
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Authentication required." }))).into_response()
}

/// Effektiver Streamer-Scope mit Session-Ownership (Python
/// `_resolve_streamer_scope`). Partner sind auf den eigenen Login beschränkt
/// (Cross-Account-Zugriff → 403); Admin/Localhost dürfen `requested` frei
/// wählen (oder `None` für „alle"). `None`-Auth → 401.
///
/// Wird von allen Daten-Endpoints des Dashboards wiederverwendet.
pub fn resolve_streamer_scope(
    auth: &DashboardAuthLevel,
    requested: Option<&str>,
    required: bool,
) -> Result<Option<String>, Response> {
    let requested = requested.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            let session = twitch_login.to_lowercase();
            if let Some(req) = &requested {
                if *req != session {
                    return Err(forbidden("Du kannst nur auf deinen eigenen Twitch-Account zugreifen."));
                }
            }
            Ok(Some(session))
        }
        DashboardAuthLevel::Localhost | DashboardAuthLevel::Admin => {
            if required && requested.is_none() {
                return Err((StatusCode::BAD_REQUEST, "streamer parameter required").into_response());
            }
            Ok(requested)
        }
        DashboardAuthLevel::None => Err(unauthorized()),
    }
}

/// `GET /social-media/terms` — öffentlich.
pub async fn terms_handler() -> Html<String> {
    Html(render_terms())
}

/// `GET /social-media/privacy` — öffentlich.
pub async fn privacy_handler() -> Html<String> {
    Html(render_privacy())
}

/// Reine Render-Logik der Index-Seite (testbar ohne HTTP).
fn render_index(auth: &DashboardAuthLevel) -> Result<String, Response> {
    // Index nutzt den Scope ohne `requested` → Partner bekommt eigenen Login,
    // Admin/Localhost `None`, None-Auth → 401.
    let streamer = resolve_streamer_scope(auth, None, false)?;
    let label = html_escape(&streamer.as_ref().map(|s| format!("@{s}")).unwrap_or_else(|| "nicht gesetzt".to_string()));
    let data = html_escape(streamer.as_deref().unwrap_or(""));
    Ok(render_dashboard(&label, &data))
}

/// `GET /social-media` — Dashboard-SPA (Auth erforderlich).
pub async fn index_handler(auth: DashboardAuthLevel) -> Response {
    match render_index(&auth) {
        Ok(html) => Html(html).into_response(),
        Err(resp) => resp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner { twitch_login: login.to_string(), twitch_user_id: "1".to_string() }
    }

    #[test]
    fn html_escape_zeichen() {
        assert_eq!(html_escape("a&b<c>\"'"), "a&amp;b&lt;c&gt;&quot;&#x27;");
        assert_eq!(html_escape("nani_123"), "nani_123");
    }

    #[test]
    fn scope_partner_und_admin() {
        // Partner: eigener Login, Cross-Account → 403.
        assert_eq!(resolve_streamer_scope(&partner("Nani"), None, false).unwrap(), Some("nani".to_string()));
        assert_eq!(resolve_streamer_scope(&partner("Nani"), Some("nani"), false).unwrap(), Some("nani".to_string()));
        assert!(resolve_streamer_scope(&partner("Nani"), Some("other"), false).is_err());
        // Admin: frei wählbar / None.
        assert_eq!(resolve_streamer_scope(&DashboardAuthLevel::Admin, None, false).unwrap(), None);
        assert_eq!(resolve_streamer_scope(&DashboardAuthLevel::Admin, Some("xyz"), false).unwrap(), Some("xyz".to_string()));
        // required ohne requested → 400.
        assert!(resolve_streamer_scope(&DashboardAuthLevel::Localhost, None, true).is_err());
        // None-Auth → Fehler (401).
        assert!(resolve_streamer_scope(&DashboardAuthLevel::None, None, false).is_err());
    }

    #[test]
    fn index_render_label() {
        // Admin → „nicht gesetzt".
        let html = render_index(&DashboardAuthLevel::Admin).unwrap();
        assert!(html.contains("nicht gesetzt"));
        // Partner → @login (kleingeschrieben).
        let html = render_index(&partner("Nani")).unwrap();
        assert!(html.contains("@nani"));
        // None → Fehler.
        assert!(render_index(&DashboardAuthLevel::None).is_err());
    }

    #[tokio::test]
    async fn terms_privacy_liefern_html() {
        let t = terms_handler().await;
        assert!(!t.0.is_empty());
        let p = privacy_handler().await;
        assert!(!p.0.is_empty());
    }
}

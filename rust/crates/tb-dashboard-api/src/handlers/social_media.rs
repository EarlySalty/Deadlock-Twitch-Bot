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
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_social_media::clip_analytics::get_analytics_summary;
use tb_social_media::clip_manager::get_clips_for_dashboard;
use tb_social_media::clip_templates::get_last_hashtags;
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

/// `?streamer=` (für scope-gefilterte Endpoints).
#[derive(Debug, Deserialize)]
pub struct StreamerQuery {
    pub streamer: Option<String>,
}

/// `?streamer=&status=&limit=` für die Clip-Liste.
#[derive(Debug, Deserialize)]
pub struct ClipsQuery {
    pub streamer: Option<String>,
    pub status: Option<String>,
    pub limit: Option<String>,
}

fn invalid_limit() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_limit", "allowed_range": [1, 200] }))).into_response()
}

/// Validiert den `limit`-Parameter (Default 50, Bereich 1..=200; Python `int()`).
fn parse_limit(raw: Option<&str>) -> Result<i64, ()> {
    let value = match raw {
        None => 50,
        Some(s) => s.parse::<i64>().map_err(|_| ())?,
    };
    if (1..=200).contains(&value) {
        Ok(value)
    } else {
        Err(())
    }
}

/// `GET /social-media/api/stats` — Analytics-Summary (scope-gefiltert).
pub async fn stats_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<StreamerQuery>) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    Json(get_analytics_summary(&pool, scope.as_deref()).await).into_response()
}

/// `GET /social-media/api/clips` — Clip-Liste (limit 1..200, scope, status).
pub async fn clips_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<ClipsQuery>) -> Response {
    let limit = match parse_limit(q.limit.as_deref()) {
        Ok(l) => l,
        Err(()) => return invalid_limit(),
    };
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let clips = get_clips_for_dashboard(&pool, scope.as_deref(), q.status.as_deref(), limit).await;
    Json(clips).into_response()
}

/// `GET /social-media/api/last-hashtags` — zuletzt genutzte Hashtags.
pub async fn last_hashtags_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<StreamerQuery>) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let hashtags = get_last_hashtags(&pool, scope.as_deref().unwrap_or("")).await;
    Json(json!({ "hashtags": hashtags })).into_response()
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

    #[test]
    fn parse_limit_bereich() {
        assert_eq!(parse_limit(None).unwrap(), 50);
        assert_eq!(parse_limit(Some("100")).unwrap(), 100);
        assert_eq!(parse_limit(Some("1")).unwrap(), 1);
        assert_eq!(parse_limit(Some("200")).unwrap(), 200);
        assert!(parse_limit(Some("0")).is_err());
        assert!(parse_limit(Some("201")).is_err());
        assert!(parse_limit(Some("abc")).is_err());
    }

    async fn make_pool() -> Option<sqlx::PgPool> {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query("DROP SCHEMA IF EXISTS t_dash_sm CASCADE").execute(&admin).await.unwrap();
        sqlx::query("CREATE SCHEMA t_dash_sm").execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", "t_dash_sm")]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT, streamer_login TEXT, status TEXT DEFAULT 'pending', created_at TEXT)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT)").execute(&pool).await.unwrap();
        Some(pool)
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn clips_handler_liste_und_limit() {
        let Some(pool) = make_pool().await else { return };
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status, created_at) VALUES ('c1', 'nani', 'pending', '2026-06-10')").execute(&pool).await.unwrap();

        // Happy-Path: Admin sieht den Clip.
        let resp = clips_handler(
            DashboardAuthLevel::Admin,
            State(pool.clone()),
            Query(ClipsQuery { streamer: None, status: None, limit: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["clip_id"], "c1");
        assert_eq!(v[0]["pending_uploads"], 0);

        // Ungültiges Limit → 400.
        let resp = clips_handler(
            DashboardAuthLevel::Admin,
            State(pool.clone()),
            Query(ClipsQuery { streamer: None, status: None, limit: Some("999".into()) }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Partner mit fremdem streamer → 403.
        let resp = clips_handler(
            partner("nani"),
            State(pool.clone()),
            Query(ClipsQuery { streamer: Some("other".into()), status: None, limit: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

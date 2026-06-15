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
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_social_media::clip_analytics::get_analytics_summary;
use tb_social_media::clip_manager::get_clips_for_dashboard;
use tb_social_media::clip_templates::{apply_template_to_clip, create_streamer_template, get_last_hashtags};
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

/// Spiegelt Pythons `_require_auth` (None-Auth → 401), für Endpoints, deren
/// erste Prüfung NICHT die Scope-Auflösung ist (z.B. apply: clip_id zuerst).
fn require_auth(auth: &DashboardAuthLevel) -> Result<(), Response> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err(unauthorized())
    } else {
        Ok(())
    }
}

/// User-gelieferte ID → positive i32 (Python `_normalize_clip_id`; akzeptiert
/// Zahl oder numerischen String).
fn normalize_id(value: Option<&Value>) -> Option<i32> {
    let n = value?.as_i64().or_else(|| value?.as_str().and_then(|s| s.trim().parse::<i64>().ok()))?;
    if n > 0 && n <= i32::MAX as i64 {
        Some(n as i32)
    } else {
        None
    }
}

async fn clip_owned_by_streamer(pool: &PgPool, clip_id: i32, streamer: &str) -> bool {
    sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM twitch_clips_social_media WHERE id = $1 AND LOWER(streamer_login) = LOWER($2) LIMIT 1",
    )
    .bind(clip_id)
    .bind(streamer)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

async fn streamer_template_owned(pool: &PgPool, template_id: i32, streamer: &str) -> bool {
    sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM clip_templates_streamer WHERE id = $1 AND LOWER(streamer_login) = LOWER($2) LIMIT 1",
    )
    .bind(template_id)
    .bind(streamer)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

/// POST-Body von `…/templates/streamer`.
#[derive(Debug, Deserialize)]
pub struct CreateTemplateBody {
    pub streamer: Option<String>,
    pub template_name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub hashtags: Vec<String>,
    #[serde(default)]
    pub is_default: bool,
}

/// `POST /social-media/api/templates/streamer` — Streamer-Template anlegen/aktualisieren.
pub async fn create_template_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Json(body): Json<CreateTemplateBody>) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    // required=true: Admin muss `streamer` mitgeben, Partner bekommt eigenen.
    let scope = match resolve_streamer_scope(&auth, body.streamer.as_deref(), true) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let streamer = scope.unwrap_or_default();
    let name = body.template_name.unwrap_or_default();
    let description = body.description.unwrap_or_default();
    if name.is_empty() || description.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "template_name and description are required" }))).into_response();
    }
    match create_streamer_template(&pool, &streamer, &name, &description, &body.hashtags, body.is_default).await {
        Ok(template_id) => Json(json!({ "success": true, "template_id": template_id, "message": "Template created/updated successfully" })).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "template_create_failed" }))).into_response(),
    }
}

/// POST-Body von `…/templates/apply`.
#[derive(Debug, Deserialize)]
pub struct ApplyTemplateBody {
    pub clip_id: Option<Value>,
    pub template_id: Option<Value>,
    #[serde(default)]
    pub is_global: bool,
    pub streamer: Option<String>,
}

/// `POST /social-media/api/templates/apply` — Template auf einen Clip anwenden.
pub async fn apply_template_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerQuery>,
    Json(body): Json<ApplyTemplateBody>,
) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    let (Some(clip_id), Some(template_id)) = (normalize_id(body.clip_id.as_ref()), normalize_id(body.template_id.as_ref())) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "clip_id and template_id are required" }))).into_response();
    };
    // Streamer aus Body, sonst Query.
    let requested = body.streamer.as_deref().or(q.streamer.as_deref());
    let scope = match resolve_streamer_scope(&auth, requested, false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if let Some(streamer) = &scope {
        if !clip_owned_by_streamer(&pool, clip_id, streamer).await {
            return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden: clip does not belong to authenticated streamer" }))).into_response();
        }
        if !body.is_global && !streamer_template_owned(&pool, template_id, streamer).await {
            return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden: template does not belong to authenticated streamer" }))).into_response();
        }
    }
    if apply_template_to_clip(&pool, clip_id, template_id, body.is_global).await {
        Json(json!({ "success": true, "message": "Template applied successfully" })).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Failed to apply template" }))).into_response()
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

    async fn make_pool(schema: &str) -> Option<sqlx::PgPool> {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT, streamer_login TEXT, status TEXT DEFAULT 'pending', created_at TEXT, clip_title TEXT, game_name TEXT, custom_description TEXT, hashtags TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT)",
            "CREATE TABLE clip_templates_streamer (id SERIAL PRIMARY KEY, streamer_login TEXT, template_name TEXT, description_template TEXT, hashtags TEXT, is_default INTEGER DEFAULT 0, created_at TEXT DEFAULT CURRENT_TIMESTAMP, updated_at TEXT DEFAULT CURRENT_TIMESTAMP, UNIQUE (streamer_login, template_name))",
            "CREATE TABLE clip_templates_global (id SERIAL PRIMARY KEY, template_name TEXT UNIQUE, description_template TEXT, hashtags TEXT, category TEXT, usage_count INTEGER DEFAULT 0, created_at TEXT DEFAULT CURRENT_TIMESTAMP, created_by TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn clips_handler_liste_und_limit() {
        let Some(pool) = make_pool("t_dash_sm_clips").await else { return };
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

    #[tokio::test]
    async fn create_template_handler_validierung() {
        let Some(pool) = make_pool("t_dash_sm_tpl_create").await else { return };
        // Partner legt eigenes Template an.
        let resp = create_template_handler(
            partner("nani"),
            State(pool.clone()),
            Json(CreateTemplateBody { streamer: None, template_name: Some("Default".into()), description: Some("Desc {{title}}".into()), hashtags: vec!["deadlock".into()], is_default: true }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["success"], true);
        let tid = v["template_id"].as_i64().unwrap();
        let row: (String, String, i32) = sqlx::query_as("SELECT streamer_login, template_name, is_default FROM clip_templates_streamer WHERE id = $1").bind(tid as i32).fetch_one(&pool).await.unwrap();
        assert_eq!(row, ("nani".to_string(), "Default".to_string(), 1));

        // Fehlende description → 400.
        let resp = create_template_handler(
            partner("nani"),
            State(pool.clone()),
            Json(CreateTemplateBody { streamer: None, template_name: Some("X".into()), description: None, hashtags: vec![], is_default: false }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Admin ohne streamer (required) → 400.
        let resp = create_template_handler(
            DashboardAuthLevel::Admin,
            State(pool.clone()),
            Json(CreateTemplateBody { streamer: None, template_name: Some("X".into()), description: Some("Y".into()), hashtags: vec![], is_default: false }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_template_handler_ownership() {
        let Some(pool) = make_pool("t_dash_sm_tpl_apply").await else { return };
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, clip_title, game_name) VALUES ('c1', 'nani', 'Titel', 'Deadlock') RETURNING id").fetch_one(&pool).await.unwrap();
        let tpl: i32 = sqlx::query_scalar("INSERT INTO clip_templates_streamer (streamer_login, template_name, description_template, hashtags) VALUES ('nani', 'T', 'Beschr {{title}}', '[\"a\"]') RETURNING id").fetch_one(&pool).await.unwrap();

        // Partner wendet eigenes Template auf eigenen Clip an → success.
        let resp = apply_template_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(ApplyTemplateBody { clip_id: Some(json!(clip)), template_id: Some(json!(tpl)), is_global: false, streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["success"], true);

        // Fehlende IDs → 400.
        let resp = apply_template_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(ApplyTemplateBody { clip_id: None, template_id: Some(json!(tpl)), is_global: false, streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Fremder Clip (Partner other besitzt clip nicht) → 403.
        let resp = apply_template_handler(
            partner("other"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(ApplyTemplateBody { clip_id: Some(json!(clip)), template_id: Some(json!(tpl)), is_global: false, streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

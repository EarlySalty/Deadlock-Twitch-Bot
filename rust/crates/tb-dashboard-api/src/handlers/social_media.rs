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
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_crypto::FieldCipher;
use tb_social_media::clip_analytics::get_analytics_summary;
use tb_social_media::credentials::{CredentialManager, PlatformStatus};
use tb_social_media::clip_manager::get_clips_for_dashboard;
use tb_social_media::clip_queue::queue_upload;
use tb_social_media::clip_templates::{
    apply_template_to_clip, create_streamer_template, get_global_templates, get_last_hashtags, get_streamer_templates,
    GlobalTemplate, StreamerTemplate,
};
use tb_social_media::layout::{
    default_streamer_layout, get_clip_effective_layout, get_streamer_layout, set_clip_layout_override,
    upsert_streamer_layout, StreamerLayout,
};
use tb_social_media::seed_vocab::seed_vocab;
use tb_social_media::vocab::{delete_vocab_entry, list_vocab, upsert_vocab_entry, VocabEntry};
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

/// `?category=` für die globalen Templates.
#[derive(Debug, Deserialize)]
pub struct CategoryQuery {
    pub category: Option<String>,
}

/// Serialisiert ein globales Template wie Pythons `dict(row)` (hashtags als
/// Liste, DB-Spaltennamen).
fn global_template_json(t: &GlobalTemplate) -> Value {
    json!({
        "id": t.id,
        "template_name": t.template_name,
        "description_template": t.description_template,
        "hashtags": t.hashtags,
        "category": t.category,
        "usage_count": t.usage_count,
        "created_at": t.created_at,
        "created_by": t.created_by,
    })
}

/// Serialisiert ein Streamer-Template wie Python; `is_default` bleibt **int
/// (0/1)** wie die DB-Spalte (Python konvertiert NICHT zu bool).
fn streamer_template_json(t: &StreamerTemplate) -> Value {
    json!({
        "id": t.id,
        "streamer_login": t.streamer_login,
        "template_name": t.template_name,
        "description_template": t.description_template,
        "hashtags": t.hashtags,
        "is_default": i32::from(t.is_default),
        "created_at": t.created_at,
        "updated_at": t.updated_at,
    })
}

/// `GET /social-media/api/templates/global` — globale Templates (optional nach Kategorie).
pub async fn templates_global_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<CategoryQuery>) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    let templates = get_global_templates(&pool, q.category.as_deref()).await;
    let list: Vec<Value> = templates.iter().map(global_template_json).collect();
    Json(json!({ "templates": list })).into_response()
}

/// `GET /social-media/api/templates/streamer` — Templates des (scope-)Streamers.
pub async fn templates_streamer_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<StreamerQuery>) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let templates = get_streamer_templates(&pool, scope.as_deref().unwrap_or("")).await;
    let list: Vec<Value> = templates.iter().map(streamer_template_json).collect();
    Json(json!({ "templates": list })).into_response()
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

/// Spiegelt Pythons `_require_admin`: nur Localhost/Admin; Partner → 403,
/// None → 401.
fn require_admin(auth: &DashboardAuthLevel) -> Result<(), Response> {
    match auth {
        DashboardAuthLevel::Localhost | DashboardAuthLevel::Admin => Ok(()),
        DashboardAuthLevel::Partner { .. } => Err(forbidden("Admin access required.")),
        DashboardAuthLevel::None => Err(unauthorized()),
    }
}

/// Validiert einen Slug (`[A-Za-z0-9_-]+`, nicht leer) — Python
/// `_normalize_safe_slug`. Fehler als Plaintext-400 (wie web.HTTPBadRequest).
fn normalize_safe_slug(raw: Option<&str>, field: &str) -> Result<String, Response> {
    let value = raw.unwrap_or("").trim().to_string();
    if value.is_empty() {
        return Err((StatusCode::BAD_REQUEST, format!("{field} is required")).into_response());
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err((StatusCode::BAD_REQUEST, format!("{field} must match [A-Za-z0-9_-]+")).into_response());
    }
    Ok(value)
}

async fn ensure_streamer_exists(pool: &PgPool, slug: &str) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1")
        .bind(slug)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some()
}

async fn clip_exists(pool: &PgPool, clip_db_id: i32) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM twitch_clips_social_media WHERE id = $1 LIMIT 1")
        .bind(clip_db_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some()
}

fn invalid_layout(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_layout", "message": message }))).into_response()
}

/// Parst den Layout-PUT-Body (Python `_parse_layout_request`): `layout` ist
/// Pflicht, `cam_enabled`/`mode` überschreiben optional.
fn parse_layout_request(payload: &Value) -> Result<StreamerLayout, Response> {
    let Some(lp) = payload.get("layout").filter(|v| !v.is_null()) else {
        return Err(invalid_layout("layout is required".to_string()));
    };
    let cam_enabled = payload.get("cam_enabled").and_then(Value::as_bool);
    let mode = payload.get("mode").and_then(Value::as_str);
    StreamerLayout::from_value(lp, cam_enabled, mode).map_err(|e| invalid_layout(e.to_string()))
}

/// `?streamer_login=` für die Layout-GET-Route.
#[derive(Debug, Deserialize)]
pub struct StreamerLoginQuery {
    pub streamer_login: Option<String>,
}

/// `GET /social-media/api/admin/streamer-layout` — Default-Layout eines Streamers (Admin).
pub async fn streamer_layout_get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<StreamerLoginQuery>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let slug = match normalize_safe_slug(q.streamer_login.as_deref(), "streamer_login") {
        Ok(s) => s.to_lowercase(),
        Err(e) => return e,
    };
    if !ensure_streamer_exists(&pool, &slug).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "unknown_streamer" }))).into_response();
    }
    let stored = get_streamer_layout(&pool, &slug).await;
    let layout = stored.clone().unwrap_or_else(default_streamer_layout);
    let (updated_at, updated_by) = if stored.is_some() {
        sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT updated_at::text, updated_by FROM social_media_streamer_layout WHERE LOWER(streamer_login) = LOWER($1) LIMIT 1",
        )
        .bind(&slug)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .unwrap_or((None, None))
    } else {
        (None, None)
    };
    Json(json!({
        "streamer_login": slug,
        "layout": layout.to_override_json(),
        "cam_enabled": layout.cam_enabled,
        "mode": layout.mode,
        "is_default": stored.is_none(),
        "updated_at": updated_at,
        "updated_by": updated_by,
    }))
    .into_response()
}

/// `PUT /social-media/api/admin/streamer-layout` — Default-Layout setzen (Admin).
pub async fn streamer_layout_put_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Json(payload): Json<Value>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let slug = match normalize_safe_slug(payload.get("streamer_login").and_then(Value::as_str), "streamer_login") {
        Ok(s) => s.to_lowercase(),
        Err(e) => return e,
    };
    if !ensure_streamer_exists(&pool, &slug).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "unknown_streamer" }))).into_response();
    }
    let layout = match parse_layout_request(&payload) {
        Ok(l) => l,
        Err(e) => return e,
    };
    // updated_by: Admin/Localhost tragen im Rust-Auth-Modell keine User-ID.
    let _ = upsert_streamer_layout(&pool, &slug, &layout, None).await;
    Json(json!({
        "streamer_login": slug,
        "layout": layout.to_override_json(),
        "cam_enabled": layout.cam_enabled,
        "mode": layout.mode,
    }))
    .into_response()
}

/// `PUT /social-media/api/admin/clips/{clip_db_id}/layout` — Clip-Override (Admin).
pub async fn clip_layout_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(clip_db_id_raw): Path<String>,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(clip_db_id_raw))) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_clip_db_id" }))).into_response();
    };
    if !clip_exists(&pool, clip_db_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "clip_not_found" }))).into_response();
    }

    // `layout` fehlt/null → Override löschen.
    let Some(layout_payload) = payload.get("layout").filter(|v| !v.is_null()) else {
        let _ = set_clip_layout_override(&pool, clip_db_id, None).await;
        let effective = get_clip_effective_layout(&pool, clip_db_id).await;
        return Json(json!({
            "clip_db_id": clip_db_id,
            "layout_override": Value::Null,
            "effective_layout": effective.to_override_json(),
        }))
        .into_response();
    };
    let layout = match StreamerLayout::from_value(layout_payload, None, None) {
        Ok(l) => l,
        Err(e) => return invalid_layout(e.to_string()),
    };
    let _ = set_clip_layout_override(&pool, clip_db_id, Some(&layout)).await;
    let effective = get_clip_effective_layout(&pool, clip_db_id).await;
    Json(json!({
        "clip_db_id": clip_db_id,
        "layout_override": layout.to_override_json(),
        "effective_layout": effective.to_override_json(),
    }))
    .into_response()
}

/// POST-Body von `…/api/upload` (Queue-Upload). `platforms` ist Array ODER
/// der String `"all"`.
#[derive(Debug, Deserialize)]
pub struct QueueUploadBody {
    pub clip_id: Option<Value>,
    #[serde(default)]
    pub platforms: Value,
    pub title: Option<String>,
    pub description: Option<String>,
    pub hashtags: Option<Vec<String>>,
    #[serde(default)]
    pub priority: i32,
    pub streamer: Option<String>,
}

/// `POST /social-media/api/upload` — Clip auf eine oder mehrere Plattformen in
/// die Upload-Queue legen.
pub async fn queue_upload_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerQuery>,
    Json(body): Json<QueueUploadBody>,
) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    let Some(clip_id) = normalize_id(body.clip_id.as_ref()) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "clip_id required" }))).into_response();
    };
    let requested = body.streamer.as_deref().or(q.streamer.as_deref());
    let scope = match resolve_streamer_scope(&auth, requested, false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if let Some(streamer) = &scope {
        if !clip_owned_by_streamer(&pool, clip_id, streamer).await {
            return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden: clip does not belong to authenticated streamer" }))).into_response();
        }
    }
    // platforms: Array oder "all".
    let platforms: Vec<String> = match &body.platforms {
        Value::String(s) if s == "all" => ["tiktok", "youtube", "instagram"].iter().map(|p| p.to_string()).collect(),
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    };
    let mut queued: Vec<Value> = Vec::new();
    for platform in &platforms {
        match queue_upload(&pool, clip_id, platform, body.title.as_deref(), body.description.as_deref(), body.hashtags.as_deref(), None, body.priority).await {
            Ok(queue_id) => queued.push(json!({ "platform": platform, "queue_id": queue_id })),
            Err(_) => queued.push(json!({ "platform": platform, "error": "queue_failed" })),
        }
    }
    Json(json!({ "queued": queued })).into_response()
}

/// Serialisiert einen Vocab-Eintrag (Python `entry.to_dict()`).
fn vocab_entry_json(e: &VocabEntry) -> Value {
    json!({
        "term": e.term,
        "canonical": e.canonical,
        "category": e.category,
        "source": e.source,
        "aliases": e.aliases,
        "weight": e.weight,
        "updated_at": e.updated_at,
    })
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

/// `?page=&page_size=&category=&q=` für die Vocab-Liste.
#[derive(Debug, Deserialize)]
pub struct VocabListQuery {
    pub page: Option<String>,
    pub page_size: Option<String>,
    pub category: Option<String>,
    pub q: Option<String>,
}

/// `GET /social-media/api/admin/vocab` — paginierte Vokabel-Liste (Admin).
pub async fn vocab_list_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<VocabListQuery>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let page = match q.page.as_deref().unwrap_or("1").parse::<i64>() {
        Ok(n) => n.max(1),
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_pagination" }))).into_response(),
    };
    let page_size = match q.page_size.as_deref().unwrap_or("50").parse::<i64>() {
        Ok(n) => n.clamp(1, 200),
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_pagination" }))).into_response(),
    };
    let category = q.category.as_deref().filter(|s| !s.is_empty());
    let query = q.q.as_deref().filter(|s| !s.is_empty());
    let offset = (page - 1) * page_size;
    let (entries, total) = list_vocab(&pool, category, query, page_size, offset).await;
    let items: Vec<Value> = entries.iter().map(vocab_entry_json).collect();
    Json(json!({ "items": items, "total": total, "page": page, "page_size": page_size })).into_response()
}

/// `POST /social-media/api/admin/vocab` — Vokabel anlegen/aktualisieren (Admin).
pub async fn vocab_upsert_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Json(payload): Json<Value>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    if !payload.is_object() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_payload" }))).into_response();
    }
    let term = payload.get("term").and_then(Value::as_str).unwrap_or("");
    let canonical = payload.get("canonical").and_then(Value::as_str).unwrap_or("");
    let category = payload.get("category").and_then(Value::as_str).unwrap_or("");
    let source = payload.get("source").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("manual");
    let aliases: Vec<String> = payload
        .get("aliases")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let weight = json_i64(payload.get("weight")).filter(|n| *n != 0).unwrap_or(1) as i32;

    match upsert_vocab_entry(&pool, term, canonical, category, source, &aliases, weight).await {
        Ok(entry) => Json(vocab_entry_json(&entry)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_vocab", "message": e.to_string() }))).into_response(),
    }
}

/// `DELETE /social-media/api/admin/vocab/:term` — Vokabel löschen (Admin).
pub async fn vocab_delete_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(term): Path<String>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let term = term.trim();
    if term.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "term_required" }))).into_response();
    }
    match delete_vocab_entry(&pool, term).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json(json!({ "error": "vocab_not_found" }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_term", "message": e.to_string() }))).into_response(),
    }
}

/// `POST /social-media/api/admin/vocab/seed` — Vokabular seeden (Admin).
/// Body optional: `{include_slang, include_api}` (Default beide true).
pub async fn vocab_seed_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, body: String) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let (mut include_slang, mut include_api) = (true, true);
    if !body.trim().is_empty() {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&body) {
            include_slang = map.get("include_slang").and_then(Value::as_bool).unwrap_or(true);
            include_api = map.get("include_api").and_then(Value::as_bool).unwrap_or(true);
        }
    }
    let (written, skipped) = seed_vocab(&pool, include_slang, include_api).await;
    // Legacy-Frontend nutzt {inserted, updated}.
    Json(json!({ "inserted": written, "updated": written, "written": written, "skipped": skipped })).into_response()
}

/// Baut den CredentialManager inline aus dem Master-Key (Pattern wie
/// engagement::build_sender_store). `None`, wenn kein Key im Env.
fn build_credential_manager(pool: PgPool) -> Option<CredentialManager> {
    let cipher = Arc::new(FieldCipher::from_env().ok()?);
    Some(CredentialManager::new(pool, cipher))
}

/// Serialisiert einen Plattform-Status. Bei aktivem Streamer-Scope + globalem
/// Fallback werden username/user_id maskiert (Python-Logik).
fn platform_status_json(s: &PlatformStatus, has_scope: bool) -> Value {
    let mask = has_scope && s.uses_global_fallback;
    json!({
        "platform": s.platform,
        "connected": s.connected,
        "username": if mask { Value::Null } else { json!(s.username) },
        "user_id": if mask { Value::Null } else { json!(s.user_id) },
        "expires_at": s.expires_at,
        "expired": s.expired,
        "uses_global_fallback": s.uses_global_fallback,
    })
}

/// `GET /social-media/api/platforms/status` — Verbindungsstatus je Plattform.
pub async fn platforms_status_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<StreamerQuery>) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(cred_mgr) = build_credential_manager(pool) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "platform_status_failed" }))).into_response();
    };
    let has_scope = scope.is_some();
    let statuses = cred_mgr.get_all_platforms_status(scope.as_deref()).await;
    let platforms: Vec<Value> = statuses.iter().map(|s| platform_status_json(s, has_scope)).collect();
    Json(json!({ "platforms": platforms })).into_response()
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
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT, streamer_login TEXT, status TEXT DEFAULT 'pending', created_at TEXT, clip_title TEXT, game_name TEXT, custom_description TEXT, hashtags TEXT, layout_override_json JSONB)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TEXT, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TEXT, created_at TEXT DEFAULT CURRENT_TIMESTAMP, completed_at TEXT)",
            "CREATE TABLE clip_templates_streamer (id SERIAL PRIMARY KEY, streamer_login TEXT, template_name TEXT, description_template TEXT, hashtags TEXT, is_default INTEGER DEFAULT 0, created_at TEXT DEFAULT CURRENT_TIMESTAMP, updated_at TEXT DEFAULT CURRENT_TIMESTAMP, UNIQUE (streamer_login, template_name))",
            "CREATE TABLE clip_templates_global (id SERIAL PRIMARY KEY, template_name TEXT UNIQUE, description_template TEXT, hashtags TEXT, category TEXT, usage_count INTEGER DEFAULT 0, created_at TEXT DEFAULT CURRENT_TIMESTAMP, created_by TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', aliases JSONB NOT NULL DEFAULT '[]'::jsonb, weight INTEGER NOT NULL DEFAULT 1, updated_at TIMESTAMPTZ DEFAULT NOW())",
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

    #[tokio::test]
    async fn templates_get_handler_shapes() {
        let Some(pool) = make_pool("t_dash_sm_tpl_get").await else { return };
        // Globale Templates (eines mit Kategorie).
        sqlx::query("INSERT INTO clip_templates_global (template_name, description_template, hashtags, category, usage_count) VALUES ('G1', 'd', '[\"a\",\"b\"]', 'gaming', 5), ('G2', 'd', '[]', NULL, 1)").execute(&pool).await.unwrap();
        // Streamer-Templates für nani (eines default).
        sqlx::query("INSERT INTO clip_templates_streamer (streamer_login, template_name, description_template, hashtags, is_default) VALUES ('nani', 'S1', 'd', '[\"x\"]', 1), ('nani', 'S2', 'd', '[]', 0)").execute(&pool).await.unwrap();

        // global: alle 2, hashtags als Array.
        let resp = templates_global_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(CategoryQuery { category: None })).await;
        let v = body_json(resp).await;
        assert_eq!(v["templates"].as_array().unwrap().len(), 2);
        let g1 = &v["templates"][0]; // usage_count DESC → G1 (5) zuerst
        assert_eq!(g1["template_name"], "G1");
        assert_eq!(g1["hashtags"], json!(["a", "b"]));
        // Kategorie-Filter.
        let resp = templates_global_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(CategoryQuery { category: Some("gaming".into()) })).await;
        assert_eq!(body_json(resp).await["templates"].as_array().unwrap().len(), 1);

        // streamer: Partner nani sieht 2, is_default als int (1/0), default zuerst.
        let resp = templates_streamer_handler(partner("nani"), State(pool.clone()), Query(StreamerQuery { streamer: None })).await;
        let v = body_json(resp).await;
        assert_eq!(v["templates"].as_array().unwrap().len(), 2);
        assert_eq!(v["templates"][0]["template_name"], "S1"); // is_default DESC
        assert_eq!(v["templates"][0]["is_default"], 1); // int, nicht true
        assert_eq!(v["templates"][1]["is_default"], 0);

        // None-Auth → 401.
        let resp = templates_global_handler(DashboardAuthLevel::None, State(pool.clone()), Query(CategoryQuery { category: None })).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    fn valid_layout() -> Value {
        json!({
            "version": 1,
            "source": {"width": 1920, "height": 1080},
            "game_crop": {"x": 0, "y": 0, "w": 1080, "h": 1080},
            "cam_crop": {"x": 1500, "y": 50, "w": 380, "h": 380},
            "cam_position": {"x": 0, "y": 0, "w": 1080, "h": 540}
        })
    }

    #[tokio::test]
    async fn streamer_layout_get_put() {
        let Some(pool) = make_pool("t_dash_sm_layout").await else { return };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('nani', '1')").execute(&pool).await.unwrap();

        // GET ohne gespeichertes Layout → Default + is_default true.
        let resp = streamer_layout_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerLoginQuery { streamer_login: Some("nani".into()) })).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["is_default"], true);
        assert_eq!(v["mode"], "pip");

        // PUT setzt ein Layout (mode stacked).
        let resp = streamer_layout_put_handler(
            DashboardAuthLevel::Admin,
            State(pool.clone()),
            Json(json!({ "streamer_login": "nani", "layout": valid_layout(), "mode": "stacked", "cam_enabled": false })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["mode"], "stacked");

        // GET jetzt → is_default false, mode stacked.
        let resp = streamer_layout_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerLoginQuery { streamer_login: Some("nani".into()) })).await;
        let v = body_json(resp).await;
        assert_eq!(v["is_default"], false);
        assert_eq!(v["mode"], "stacked");
        assert_eq!(v["cam_enabled"], false);

        // Unbekannter Streamer → 404.
        let resp = streamer_layout_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerLoginQuery { streamer_login: Some("ghost".into()) })).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // Partner → 403.
        let resp = streamer_layout_get_handler(partner("nani"), State(pool.clone()), Query(StreamerLoginQuery { streamer_login: Some("nani".into()) })).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        // PUT ohne layout-Key → 400 invalid_layout.
        let resp = streamer_layout_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Json(json!({ "streamer_login": "nani" }))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn clip_layout_put_set_und_clear() {
        let Some(pool) = make_pool("t_dash_sm_clip_layout").await else { return };
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('c1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // Override setzen.
        let resp = clip_layout_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), Json(json!({ "layout": valid_layout() }))).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert!(!v["layout_override"].is_null());
        assert!(v["effective_layout"]["version"] == 1);

        // Override löschen (layout null).
        let resp = clip_layout_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), Json(json!({ "layout": Value::Null }))).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_json(resp).await["layout_override"].is_null());

        // Ungültige clip_db_id (Pfad) → 400.
        let resp = clip_layout_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("abc".into()), Json(json!({}))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Nicht existierender Clip → 404.
        let resp = clip_layout_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("99999".into()), Json(json!({}))).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn vocab_crud_und_seed() {
        let Some(pool) = make_pool("t_dash_sm_vocab").await else { return };

        // Upsert: gültiger Eintrag (Kategorie hero).
        let resp = vocab_upsert_handler(
            DashboardAuthLevel::Admin,
            State(pool.clone()),
            Json(json!({ "term": "Haze", "canonical": "Haze", "category": "hero", "aliases": ["hayz"], "weight": 3 })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["term"], "haze"); // normalisiert
        assert_eq!(v["weight"], 3);

        // Ungültige Kategorie → 400 invalid_vocab.
        let resp = vocab_upsert_handler(DashboardAuthLevel::Admin, State(pool.clone()), Json(json!({ "term": "x", "canonical": "X", "category": "bogus" }))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Nicht-Objekt-Body → 400 invalid_payload.
        let resp = vocab_upsert_handler(DashboardAuthLevel::Admin, State(pool.clone()), Json(json!([1, 2]))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // List: findet den Eintrag.
        let resp = vocab_list_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(VocabListQuery { page: None, page_size: None, category: None, q: None })).await;
        let v = body_json(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["items"][0]["term"], "haze");
        // Ungültige Pagination → 400.
        let resp = vocab_list_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(VocabListQuery { page: Some("abc".into()), page_size: None, category: None, q: None })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Delete: vorhanden → 204, dann → 404.
        let resp = vocab_delete_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("haze".into())).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = vocab_delete_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("haze".into())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Seed nur Slang (include_api=false → kein Netzwerk) → 25 geschrieben.
        let resp = vocab_seed_handler(DashboardAuthLevel::Admin, State(pool.clone()), "{\"include_api\": false}".to_string()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["written"], 25);
        assert_eq!(v["inserted"], 25);

        // Partner → 403 (admin-only).
        let resp = vocab_list_handler(partner("nani"), State(pool.clone()), Query(VocabListQuery { page: None, page_size: None, category: None, q: None })).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn platform_status_masking() {
        let status = PlatformStatus {
            platform: "tiktok".into(),
            connected: true,
            username: Some("nani".into()),
            user_id: Some("42".into()),
            expires_at: None,
            expired: false,
            scopes: None,
            uses_global_fallback: true,
        };
        // Scope + globaler Fallback → maskiert.
        let v = platform_status_json(&status, true);
        assert!(v["username"].is_null());
        assert!(v["user_id"].is_null());
        assert_eq!(v["uses_global_fallback"], true);
        // Scope, aber kein Fallback → sichtbar.
        let mut own = status.clone();
        own.uses_global_fallback = false;
        let v = platform_status_json(&own, true);
        assert_eq!(v["username"], "nani");
        // Kein Scope (Admin) → trotz Fallback sichtbar.
        let v = platform_status_json(&status, false);
        assert_eq!(v["username"], "nani");
    }

    #[tokio::test]
    async fn platforms_status_none_auth_401() {
        let Some(pool) = make_pool("t_dash_sm_platforms").await else { return };
        // None-Auth → 401 vor dem Cipher-Aufbau (kein Secret nötig).
        let resp = platforms_status_handler(DashboardAuthLevel::None, State(pool.clone()), Query(StreamerQuery { streamer: None })).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    fn queue_body(clip_id: i32, platforms: Value) -> QueueUploadBody {
        QueueUploadBody { clip_id: Some(json!(clip_id)), platforms, title: None, description: None, hashtags: None, priority: 0, streamer: None }
    }

    #[tokio::test]
    async fn queue_upload_handler_paths() {
        let Some(pool) = make_pool("t_dash_sm_queue").await else { return };
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('c1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // Admin, eine Plattform → ein queue_id.
        let resp = queue_upload_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(queue_body(clip, json!(["tiktok"])))).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["queued"].as_array().unwrap().len(), 1);
        assert_eq!(v["queued"][0]["platform"], "tiktok");
        assert!(v["queued"][0]["queue_id"].is_number());

        // "all" → 3 Plattformen.
        let resp = queue_upload_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(queue_body(clip, json!("all")))).await;
        assert_eq!(body_json(resp).await["queued"].as_array().unwrap().len(), 3);

        // Ungültige Plattform → error queue_failed (kein Crash).
        let resp = queue_upload_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(queue_body(clip, json!(["snapchat"])))).await;
        assert_eq!(body_json(resp).await["queued"][0]["error"], "queue_failed");

        // Fehlende clip_id → 400.
        let resp = queue_upload_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(QueueUploadBody { clip_id: None, platforms: json!(["tiktok"]), title: None, description: None, hashtags: None, priority: 0, streamer: None })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Partner mit fremdem Clip → 403.
        let resp = queue_upload_handler(partner("other"), State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(queue_body(clip, json!(["tiktok"])))).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

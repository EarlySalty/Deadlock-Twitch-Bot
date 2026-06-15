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
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use tb_crypto::FieldCipher;
use tb_social_media::approval::{get_approval_record, handle_decision, serialize_approval_record, ApprovalError};
use tb_social_media::enrichment::{ensure_enrichment_row, get_enrichment, update_manual_edit, EnrichmentRecord};
use tb_social_media::enrich_pipeline::{ClipEnrichmentPipeline, PipelineError, Transcriber};
use tb_social_media::llm_dispatch::LlmDispatcher;
use tb_social_media::whisper::OpenAiTranscriber;
use tb_social_media::retention::mark_clip_discarded;
use tb_social_media::settings::{coerce_bool, get_auto_approve_settings, set_auto_approve_settings, AutoApprove};
use tb_social_media::analytics::{list_clip_analytics, list_reports, ClipAnalyticsSnapshot, SocialMediaReportRecord};
use tb_social_media::clip_analytics::get_analytics_summary;
use tb_social_media::{ClipFetchService, ClipRepository, HelixClipSource};
use tb_transport_twitch::{HelixClient, HelixConfig};
use tb_social_media::credentials::{CredentialManager, PlatformStatus};
use tb_social_media::clip_manager::{batch_upload_all_new, get_clips_for_dashboard, mark_clip_uploaded, register_manual_upload, ManualUploadError};
use tb_social_media::video_processor::VideoProcessor;
use tb_social_media::clip_queue::queue_upload;
use tb_social_media::clip_templates::{
    apply_template_to_clip, create_streamer_template, get_global_templates, get_last_hashtags, get_streamer_templates,
    GlobalTemplate, StreamerTemplate,
};
use tb_social_media::oauth::{OAuthError, OAuthManager};
use tb_social_media::layout::{
    default_streamer_layout, get_clip_effective_layout, get_streamer_layout, set_clip_layout_override,
    upsert_streamer_layout, StreamerLayout,
};
use tb_social_media::seed_vocab::seed_vocab;
use tb_social_media::vocab::{delete_vocab_entry, list_vocab, upsert_vocab_entry, VocabEntry};
use tb_social_media::rendering::{render_dashboard, render_privacy, render_terms};
use tb_social_media::report_writer::SocialMediaReportWriter;

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

/// Slug-Validierung (`[A-Za-z0-9_-]+`, nicht leer) — liefert die Fehlermeldung
/// (Python `_normalize_safe_slug`).
fn slug_message(raw: Option<&str>, field: &str) -> Result<String, String> {
    let value = raw.unwrap_or("").trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("{field} must match [A-Za-z0-9_-]+"));
    }
    Ok(value)
}

/// Wie [`slug_message`], Fehler als Plaintext-400 (wie web.HTTPBadRequest).
fn normalize_safe_slug(raw: Option<&str>, field: &str) -> Result<String, Response> {
    slug_message(raw, field).map_err(|m| (StatusCode::BAD_REQUEST, m).into_response())
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

const UPLOAD_MAX_BYTES: usize = 200 * 1024 * 1024;
const UPLOAD_MAX_DURATION_SECONDS: f64 = 300.0;

/// MP4-Magic: enthält das `ftyp`-Box-Kennzeichen in den ersten 64 Bytes.
fn has_mp4_header(bytes: &[u8]) -> bool {
    bytes.get(..bytes.len().min(64)).map(|h| h.windows(4).any(|w| w == b"ftyp")).unwrap_or(false)
}

/// Verarbeitet einen hochgeladenen Clip (Validierung + Speichern + Registrierung).
/// `base_dir` injizierbar (Tests). Liefert die 201-Antwort oder eine Fehler-Response.
async fn process_uploaded_clip(
    pool: &PgPool,
    base_dir: &str,
    streamer_raw: Option<&str>,
    clip_id_raw: Option<&str>,
    title: Option<&str>,
    bytes: &[u8],
) -> Result<Value, Response> {
    if bytes.len() > UPLOAD_MAX_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "Uploaded file too large").into_response());
    }
    let streamer_login = slug_message(streamer_raw, "streamer_login")
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_streamer_login", "message": m }))).into_response())?
        .to_lowercase();
    if !ensure_streamer_exists(pool, &streamer_login).await {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "unknown_streamer" }))).into_response());
    }
    let clip_id = match clip_id_raw.filter(|s| !s.is_empty()) {
        Some(raw) => normalize_safe_slug(Some(raw), "clip_id")?, // Slug-Fehler → Plaintext-400
        None => tb_crypto::random_hex_token(16),
    };
    let upload_dir = format!("{base_dir}/{streamer_login}");
    let final_path = format!("{upload_dir}/{clip_id}.mp4");
    if std::path::Path::new(&final_path).exists() {
        return Err((StatusCode::CONFLICT, Json(json!({ "error": "duplicate_clip_id" }))).into_response());
    }

    if tokio::fs::create_dir_all(&upload_dir).await.is_err() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "upload_failed" }))).into_response());
    }
    let temp_path = format!("{upload_dir}/{clip_id}.upload.tmp");
    if tokio::fs::write(&temp_path, bytes).await.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "upload_failed" }))).into_response());
    }

    // Validierung (ftyp + ffprobe-Dauer).
    let validation = async {
        if !has_mp4_header(bytes) {
            return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "Only MP4 uploads are supported").into_response());
        }
        let info = VideoProcessor::default()
            .get_video_info(&temp_path)
            .await
            .map_err(|_| (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Uploaded file is not a valid MP4 video").into_response())?;
        if info.duration <= 0.0 {
            return Err((StatusCode::BAD_REQUEST, "Uploaded MP4 must have a positive duration").into_response());
        }
        if info.duration > UPLOAD_MAX_DURATION_SECONDS {
            return Err((StatusCode::BAD_REQUEST, "Uploaded MP4 must be 300 seconds or shorter").into_response());
        }
        Ok(info.duration)
    }
    .await;
    let duration = match validation {
        Ok(d) => d,
        Err(resp) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(resp);
        }
    };

    if tokio::fs::rename(&temp_path, &final_path).await.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "upload_failed" }))).into_response());
    }

    match register_manual_upload(pool, &clip_id, &streamer_login, title, &final_path, duration).await {
        Ok((clip_db_id, retention_until)) => Ok(json!({ "clip_db_id": clip_db_id, "clip_id": clip_id, "retention_until": retention_until })),
        Err(ManualUploadError::AlreadyExists) => {
            let _ = tokio::fs::remove_file(&final_path).await;
            Err((StatusCode::CONFLICT, Json(json!({ "error": "duplicate_clip_id" }))).into_response())
        }
        Err(ManualUploadError::UnknownStreamer) => {
            let _ = tokio::fs::remove_file(&final_path).await;
            Err((StatusCode::NOT_FOUND, Json(json!({ "error": "unknown_streamer" }))).into_response())
        }
        Err(ManualUploadError::Db(_)) => {
            let _ = tokio::fs::remove_file(&final_path).await;
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "upload_failed" }))).into_response())
        }
    }
}

/// `POST /social-media/api/clips/upload` — Multipart-Datei-Upload (Admin).
pub async fn upload_clip_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, mut multipart: Multipart) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let mut bytes: Option<Vec<u8>> = None;
    let mut streamer_login: Option<String> = None;
    let mut clip_id: Option<String> = None;
    let mut title: Option<String> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name().map(str::to_string).as_deref() {
            Some("file") => bytes = field.bytes().await.ok().map(|b| b.to_vec()),
            Some("streamer_login") => streamer_login = field.text().await.ok(),
            Some("clip_id") => clip_id = field.text().await.ok(),
            Some("title") => title = field.text().await.ok(),
            _ => {}
        }
    }
    let Some(bytes) = bytes else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "file is required" }))).into_response();
    };
    let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    match process_uploaded_clip(&pool, "data/clips/uploads", streamer_login.as_deref(), clip_id.as_deref(), title.as_deref(), &bytes).await {
        Ok(payload) => (StatusCode::CREATED, Json(payload)).into_response(),
        Err(resp) => resp,
    }
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

/// Baut den Clip-Fetcher inline aus den Twitch-App-Credentials (`None`, wenn
/// nicht konfiguriert → 503).
fn build_clip_fetch_service(pool: PgPool, limit: u32) -> Option<ClipFetchService> {
    let client_id = std::env::var("TWITCH_CLIENT_ID").ok().filter(|s| !s.is_empty())?;
    let client_secret = std::env::var("TWITCH_CLIENT_SECRET").ok().filter(|s| !s.is_empty())?;
    let helix = HelixClient::new(HelixConfig::new(client_id, client_secret)).ok()?;
    let source = HelixClipSource::new(Arc::new(helix));
    Some(ClipFetchService::new(ClipRepository::new(pool), source).with_clip_limit(limit))
}

/// POST-Body von `…/api/fetch-clips`.
#[derive(Debug, Deserialize)]
pub struct FetchClipsBody {
    pub streamer: Option<String>,
    pub limit: Option<i64>,
    #[allow(dead_code)] // days wird vom Rust-Helix-Fetcher (Recent-Window) nicht genutzt
    pub days: Option<i64>,
}

/// `POST /social-media/api/fetch-clips` — manuell aktuelle Twitch-Clips holen.
pub async fn fetch_clips_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Json(body): Json<FetchClipsBody>) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    // required=true: Admin muss streamer angeben.
    let scope = match resolve_streamer_scope(&auth, body.streamer.as_deref(), true) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let streamer = scope.unwrap_or_default();
    let limit = body.limit.filter(|n| *n > 0).unwrap_or(20).clamp(1, 100) as u32;

    let Some(service) = build_clip_fetch_service(pool, limit) else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "twitch_api_unavailable", "message": "Clip-Fetch ist derzeit nicht verfügbar." }))).into_response();
    };
    let result = service.fetch_for_streamer(&streamer).await;
    let clips_found = result.clips_found.max(0);
    Json(json!({ "success": true, "clips_found": clips_found, "message": format!("Fetched {clips_found} clips") })).into_response()
}

/// POST-Body von `…/api/batch-upload`.
#[derive(Debug, Deserialize)]
pub struct BatchUploadBody {
    pub streamer: Option<String>,
    #[serde(default)]
    pub platforms: Value,
    pub apply_default_template: Option<bool>,
}

/// `POST /social-media/api/batch-upload` — alle neuen Clips eines Streamers einreihen.
pub async fn batch_upload_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(qs): Query<StreamerQuery>, Json(body): Json<BatchUploadBody>) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    let requested = body.streamer.as_deref().or(qs.streamer.as_deref());
    // required=true: Admin muss streamer angeben.
    let scope = match resolve_streamer_scope(&auth, requested, true) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let streamer = scope.unwrap_or_default();
    let platforms: Vec<String> = body.platforms.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    if platforms.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "platforms are required" }))).into_response();
    }
    let apply_default_template = body.apply_default_template.unwrap_or(true);
    let stats = batch_upload_all_new(&pool, &streamer, &platforms, apply_default_template).await;
    Json(json!({
        "success": true,
        "stats": { "queued": stats.queued, "skipped": stats.skipped, "errors": stats.errors },
        "message": format!("Queued {} clips, {} errors", stats.queued, stats.errors),
    }))
    .into_response()
}

/// POST-Body von `…/api/mark-uploaded`.
#[derive(Debug, Deserialize)]
pub struct MarkUploadedBody {
    pub clip_id: Option<Value>,
    #[serde(default)]
    pub platforms: Value,
    pub streamer: Option<String>,
}

/// `POST /social-media/api/mark-uploaded` — Clip manuell als hochgeladen markieren.
pub async fn mark_uploaded_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerQuery>,
    Json(body): Json<MarkUploadedBody>,
) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    let clip_id = normalize_id(body.clip_id.as_ref());
    let platforms: Vec<String> = body.platforms.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let (Some(clip_id), false) = (clip_id, platforms.is_empty()) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "clip_id and platforms are required" }))).into_response();
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
    if mark_clip_uploaded(&pool, clip_id, &platforms, true).await {
        Json(json!({ "success": true, "message": "Clip marked as uploaded" })).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Failed to mark clip as uploaded" }))).into_response()
    }
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

const CLIP_COLUMNS: &str = "id, clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login, \
    created_at, duration_seconds, view_count, game_name, status, source_kind, upload_local_path, \
    retention_until::text AS retention_until, discarded_at::text AS discarded_at, \
    layout_override_json::text AS layout_override_json, uploaded_tiktok, uploaded_youtube, uploaded_instagram";

/// Geladene Clip-Zeile (für `_serialize_clip_record`).
struct ClipRow {
    id: i32,
    clip_id: Option<String>,
    clip_url: Option<String>,
    clip_title: Option<String>,
    clip_thumbnail_url: Option<String>,
    streamer_login: Option<String>,
    created_at: Option<String>,
    duration_seconds: Option<f64>,
    view_count: Option<i32>,
    game_name: Option<String>,
    status: Option<String>,
    source_kind: Option<String>,
    upload_local_path: Option<String>,
    retention_until: Option<String>,
    discarded_at: Option<String>,
    layout_override_json: Option<String>,
    uploaded_tiktok: Option<i32>,
    uploaded_youtube: Option<i32>,
    uploaded_instagram: Option<i32>,
}

fn row_to_clip(r: &PgRow) -> ClipRow {
    ClipRow {
        id: r.try_get("id").unwrap_or(0),
        clip_id: r.try_get("clip_id").unwrap_or(None),
        clip_url: r.try_get("clip_url").unwrap_or(None),
        clip_title: r.try_get("clip_title").unwrap_or(None),
        clip_thumbnail_url: r.try_get("clip_thumbnail_url").unwrap_or(None),
        streamer_login: r.try_get("streamer_login").unwrap_or(None),
        created_at: r.try_get("created_at").unwrap_or(None),
        duration_seconds: r.try_get("duration_seconds").unwrap_or(None),
        view_count: r.try_get("view_count").unwrap_or(None),
        game_name: r.try_get("game_name").unwrap_or(None),
        status: r.try_get("status").unwrap_or(None),
        source_kind: r.try_get("source_kind").unwrap_or(None),
        upload_local_path: r.try_get("upload_local_path").unwrap_or(None),
        retention_until: r.try_get("retention_until").unwrap_or(None),
        discarded_at: r.try_get("discarded_at").unwrap_or(None),
        layout_override_json: r.try_get("layout_override_json").unwrap_or(None),
        uploaded_tiktok: r.try_get("uploaded_tiktok").unwrap_or(None),
        uploaded_youtube: r.try_get("uploaded_youtube").unwrap_or(None),
        uploaded_instagram: r.try_get("uploaded_instagram").unwrap_or(None),
    }
}

async fn load_clip_row(pool: &PgPool, clip_db_id: i32) -> Option<ClipRow> {
    let row = sqlx::query(&format!("SELECT {CLIP_COLUMNS} FROM twitch_clips_social_media WHERE id = $1 LIMIT 1"))
        .bind(clip_db_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()?;
    Some(row_to_clip(&row))
}

/// Baut den Clip-Detail-JSON inkl. Effective-Layout, Enrichment-Summary und
/// Approval (Python `_serialize_clip_record`).
async fn serialize_clip_record(pool: &PgPool, row: &ClipRow) -> Value {
    let layout_override = row
        .layout_override_json
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null);
    let effective_layout = get_clip_effective_layout(pool, row.id).await.to_override_json();

    let (enrichment_status, enrichment_summary) = match get_enrichment(pool, row.id).await {
        Some(e) => {
            // Dedup youtube→tiktok→instagram, erste 5.
            let mut seen = std::collections::HashSet::new();
            let mut top: Vec<String> = Vec::new();
            for tag in e.hashtags_youtube.iter().chain(&e.hashtags_tiktok).chain(&e.hashtags_instagram) {
                if seen.insert(tag.clone()) {
                    top.push(tag.clone());
                    if top.len() == 5 {
                        break;
                    }
                }
            }
            (json!(e.status), json!({ "top_hashtags": top, "provider": e.llm_provider }))
        }
        None => (Value::Null, Value::Null),
    };
    let approval = match get_approval_record(pool, row.id).await {
        Some(rec) => serialize_approval_record(&rec),
        None => Value::Null,
    };

    json!({
        "clip_db_id": row.id,
        "clip_id": row.clip_id,
        "clip_url": row.clip_url,
        "title": row.clip_title,
        "thumbnail_url": row.clip_thumbnail_url,
        "streamer_login": row.streamer_login,
        "created_at": row.created_at,
        "duration_seconds": row.duration_seconds,
        "view_count": row.view_count,
        "game_name": row.game_name,
        "status": row.status,
        "source_kind": row.source_kind,
        "upload_local_path": row.upload_local_path,
        "retention_until": row.retention_until,
        "discarded_at": row.discarded_at,
        "platform_status": {
            "tiktok": row.uploaded_tiktok.unwrap_or(0) != 0,
            "youtube": row.uploaded_youtube.unwrap_or(0) != 0,
            "instagram": row.uploaded_instagram.unwrap_or(0) != 0,
        },
        "layout_override": layout_override,
        "effective_layout": effective_layout,
        "enrichment_status": enrichment_status,
        "enrichment_summary": enrichment_summary,
        "approval": approval,
    })
}

fn invalid_clip_db_id() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_clip_db_id" }))).into_response()
}

fn clip_not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "clip_not_found" }))).into_response()
}

fn invalid_pagination() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_pagination" }))).into_response()
}

/// `?page=&page_size=&status=&streamer=` für die Admin-Clip-Liste.
#[derive(Debug, Deserialize)]
pub struct AdminClipsQuery {
    pub page: Option<String>,
    pub page_size: Option<String>,
    pub status: Option<String>,
    pub streamer: Option<String>,
}

fn push_clips_where(qb: &mut QueryBuilder<Postgres>, streamer: Option<&str>, status: Option<&str>) {
    if let Some(s) = streamer {
        qb.push(" AND LOWER(streamer_login) = LOWER(");
        qb.push_bind(s.to_string());
        qb.push(")");
    }
    if let Some(st) = status {
        if st == "discarded" {
            qb.push(" AND discarded_at IS NOT NULL");
        } else {
            qb.push(" AND LOWER(status) = LOWER(");
            qb.push_bind(st.to_string());
            qb.push(")");
        }
    }
}

/// `GET /social-media/api/admin/clips` — paginierte Clip-Liste (Admin).
pub async fn admin_clips_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<AdminClipsQuery>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let page = match q.page.as_deref().unwrap_or("1").parse::<i64>() {
        Ok(n) => n.max(1),
        Err(_) => return invalid_pagination(),
    };
    let page_size = match q.page_size.as_deref().unwrap_or("20").parse::<i64>() {
        Ok(n) => n.clamp(1, 100),
        Err(_) => return invalid_pagination(),
    };
    let status = q.status.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let streamer = q.streamer.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let offset = (page - 1) * page_size;

    let mut qb_total = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM twitch_clips_social_media WHERE 1=1");
    push_clips_where(&mut qb_total, streamer.as_deref(), status.as_deref());
    let total: i64 = qb_total.build_query_scalar().fetch_one(&pool).await.unwrap_or(0);

    let mut qb = QueryBuilder::<Postgres>::new(&format!("SELECT {CLIP_COLUMNS} FROM twitch_clips_social_media WHERE 1=1"));
    push_clips_where(&mut qb, streamer.as_deref(), status.as_deref());
    qb.push(" ORDER BY created_at DESC, id DESC LIMIT ");
    qb.push_bind(page_size);
    qb.push(" OFFSET ");
    qb.push_bind(offset);
    let rows = qb.build().fetch_all(&pool).await.unwrap_or_default();

    let mut items: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        items.push(serialize_clip_record(&pool, &row_to_clip(r)).await);
    }
    Json(json!({ "items": items, "page": page, "page_size": page_size, "total": total })).into_response()
}

/// `GET /social-media/api/admin/clips/:clip_db_id` — Clip-Detail (Admin).
pub async fn admin_clip_detail_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    match load_clip_row(&pool, clip_db_id).await {
        Some(clip) => Json(serialize_clip_record(&pool, &clip).await).into_response(),
        None => clip_not_found(),
    }
}

/// `POST /social-media/api/admin/clips/:clip_db_id/discard` — Clip verwerfen (Admin).
pub async fn admin_clip_discard_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if !mark_clip_discarded(&pool, clip_db_id).await {
        return clip_not_found();
    }
    match load_clip_row(&pool, clip_db_id).await {
        Some(clip) => Json(serialize_clip_record(&pool, &clip).await).into_response(),
        None => Json(json!({ "clip_db_id": clip_db_id, "discarded": true })).into_response(),
    }
}

fn invalid_payload() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_payload" }))).into_response()
}

/// Serialisiert einen Enrichment-Datensatz (Python `_serialize_enrichment_record`).
fn enrichment_record_json(e: &EnrichmentRecord) -> Value {
    json!({
        "clip_db_id": e.clip_db_id,
        "transcript_raw": e.transcript_raw,
        "transcript_corrected": e.transcript_corrected,
        "transcript_segments": e.transcript_segments,
        "transcript_lang": e.transcript_lang,
        "detected_terms": e.detected_terms,
        "title_youtube": e.title_youtube,
        "title_tiktok": e.title_tiktok,
        "title_instagram": e.title_instagram,
        "description_youtube": e.description_youtube,
        "description_tiktok": e.description_tiktok,
        "description_instagram": e.description_instagram,
        "hashtags_youtube": e.hashtags_youtube,
        "hashtags_tiktok": e.hashtags_tiktok,
        "hashtags_instagram": e.hashtags_instagram,
        "llm_provider": e.llm_provider,
        "llm_model": e.llm_model,
        "cost_usd_estimate": e.cost_usd_estimate,
        "status": e.status,
        "error_message": e.error_message,
        "started_at": e.started_at,
        "completed_at": e.completed_at,
        "edited_by": e.edited_by,
        "updated_at": e.updated_at,
    })
}

/// Serialisiert einen Analytics-Snapshot (Python `_serialize_clip_analytics_record`).
fn clip_analytics_json(a: &ClipAnalyticsSnapshot) -> Value {
    json!({
        "clip_db_id": a.clip_db_id,
        "platform": a.platform,
        "bucket": a.bucket,
        "views": a.views,
        "likes": a.likes,
        "comments": a.comments,
        "shares": a.shares,
        "watch_time_seconds": a.watch_time_seconds,
        "ctr_percent": a.ctr_percent,
        "engagement_rate": a.engagement_rate,
        "provider": a.provider,
        "synced_at": a.synced_at,
        "next_pull_at": a.next_pull_at,
    })
}

/// Serialisiert einen Report-Datensatz (Python `_serialize_report_record`).
fn report_record_json(r: &SocialMediaReportRecord) -> Value {
    json!({
        "id": r.id,
        "kind": r.kind,
        "streamer_login": r.streamer_login,
        "period_start": r.period_start,
        "period_end": r.period_end,
        "content_md": r.content_md,
        "model": r.model,
        "created_at": r.created_at,
    })
}

/// Liest ein title/description-Feld aus dem Payload (Python-Semantik):
/// fehlt → `None` (skip), `null` → `Some(None)` (clear), String → trim-or-null,
/// sonst → 400 invalid_field.
fn parse_string_field<'a>(payload: &'a Value, field: &str) -> Result<Option<Option<&'a str>>, Response> {
    match payload.get(field) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) => {
            let t = s.trim();
            Ok(Some(if t.is_empty() { None } else { Some(t) }))
        }
        Some(_) => Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_field", "field": field }))).into_response()),
    }
}

/// Hashtag-Liste normalisieren (Python `_normalize_hashtag_list`): `#`-Präfix,
/// dedupliziert (case-insensitiv), leere übersprungen. fehlt/null → `None`,
/// nicht-Liste → 400.
fn parse_hashtag_field(payload: &Value, field: &str) -> Result<Option<Vec<String>>, Response> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(arr)) => {
            let mut seen = std::collections::HashSet::new();
            let mut cleaned = Vec::new();
            for entry in arr {
                let token = entry.as_str().map(|s| s.trim().to_string()).unwrap_or_default();
                if token.is_empty() {
                    continue;
                }
                let token = if token.starts_with('#') { token } else { format!("#{}", token.trim_start_matches('#')) };
                if seen.insert(token.to_lowercase()) {
                    cleaned.push(token);
                }
            }
            Ok(Some(cleaned))
        }
        Some(_) => Err((StatusCode::BAD_REQUEST, format!("{field} must be a list")).into_response()),
    }
}

/// `PUT /social-media/api/admin/clips/:clip_db_id/enrichment` — Edits speichern (Admin).
pub async fn enrichment_put_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>, body: String) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if load_clip_row(&pool, clip_db_id).await.is_none() {
        return clip_not_found();
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    macro_rules! field {
        ($name:literal) => {
            match parse_string_field(&payload, $name) {
                Ok(v) => v,
                Err(e) => return e,
            }
        };
    }
    macro_rules! tags {
        ($name:literal) => {
            match parse_hashtag_field(&payload, $name) {
                Ok(v) => v,
                Err(e) => return e,
            }
        };
    }
    // Python-Reihenfolge: erst title/description (invalid_field), dann hashtags.
    let ty = field!("title_youtube");
    let tt = field!("title_tiktok");
    let ti = field!("title_instagram");
    let dy = field!("description_youtube");
    let dt = field!("description_tiktok");
    let di = field!("description_instagram");
    let hy = tags!("hashtags_youtube");
    let ht = tags!("hashtags_tiktok");
    let hi = tags!("hashtags_instagram");
    let result = update_manual_edit(
        &pool,
        clip_db_id,
        None, // edited_by: Admin/Localhost ohne User-ID im Rust-Auth-Modell
        ty, tt, ti, dy, dt, di,
        hy.as_deref(),
        ht.as_deref(),
        hi.as_deref(),
    )
    .await;
    if result.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "save_failed" }))).into_response();
    }
    match get_enrichment(&pool, clip_db_id).await {
        Some(r) => Json(enrichment_record_json(&r)).into_response(),
        None => Json(json!({})).into_response(),
    }
}

/// `GET /social-media/api/admin/clips/:clip_db_id/enrichment` — Enrichment (Admin).
pub async fn enrichment_get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if load_clip_row(&pool, clip_db_id).await.is_none() {
        return clip_not_found();
    }
    let record = ensure_enrichment_row(&pool, clip_db_id).await;
    Json(enrichment_record_json(&record)).into_response()
}

/// `POST /social-media/api/admin/clips/:clip_db_id/enrichment/run` — Enrichment
/// manuell anstoßen (Admin). Optionaler Body `{ "force": true }` reichert auch
/// bereits fertige Clips neu an. Baut Transcriber (OpenAI-Whisper) + LLM-Dispatcher
/// inline aus Env/Settings — fehlt der OpenAI-Key, läuft die Pipeline ohne
/// Transkription weiter (1:1 wie Python).
pub async fn enrichment_run_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>, body: String) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if load_clip_row(&pool, clip_db_id).await.is_none() {
        return clip_not_found();
    }
    // Optionaler Body: ungültiges/leeres JSON → force=false (Python schluckt Fehler).
    let force = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("force").map(coerce_bool))
        .unwrap_or(false);

    let transcriber = OpenAiTranscriber::from_env();
    let llm = LlmDispatcher::new(pool.clone());
    let pipeline = ClipEnrichmentPipeline::new(pool.clone());
    let outcome = match pipeline
        .run(clip_db_id, transcriber.as_ref().map(|t| t as &dyn Transcriber), &llm, force)
        .await
    {
        Ok(o) => o,
        Err(PipelineError::ClipNotFound(_)) => return clip_not_found(),
    };

    let enrichment = match get_enrichment(&pool, clip_db_id).await {
        Some(r) => enrichment_record_json(&r),
        None => json!({}),
    };
    Json(json!({
        "clip_db_id": clip_db_id,
        "outcome": {
            "status": outcome.status,
            "provider": outcome.provider,
            "model": outcome.model,
            "error_message": outcome.error_message,
        },
        "enrichment": enrichment,
    }))
    .into_response()
}

/// `GET /social-media/api/admin/analytics/clips/:clip_db_id` — Analytics (Admin).
pub async fn clip_analytics_get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if load_clip_row(&pool, clip_db_id).await.is_none() {
        return clip_not_found();
    }
    let items: Vec<Value> = list_clip_analytics(&pool, clip_db_id).await.iter().map(clip_analytics_json).collect();
    Json(json!({ "clip_db_id": clip_db_id, "items": items })).into_response()
}

/// `?kind=&streamer=&limit=` für die Report-Liste.
#[derive(Debug, Deserialize)]
pub struct ReportsQuery {
    pub kind: Option<String>,
    pub streamer: Option<String>,
    pub limit: Option<String>,
}

/// `POST /social-media/api/admin/reports/run` — Ad-hoc-Report erzeugen (Admin).
pub async fn reports_run_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, body: String) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    let kind = payload.get("kind").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    let streamer = payload.get("streamer").and_then(Value::as_str).map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    if !matches!(kind.as_str(), "streamer" | "cross") {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_kind" }))).into_response();
    }
    if kind == "streamer" && streamer.is_none() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "streamer_required" }))).into_response();
    }
    let writer = SocialMediaReportWriter::new(pool.clone());
    let result = if kind == "streamer" {
        writer.write_streamer_report(streamer.as_deref().unwrap_or(""), None, None, true).await
    } else {
        writer.write_cross_report(None, None, true).await
    };
    match result {
        Ok(report) => Json(report_record_json(&report)).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "report_generation_failed" }))).into_response(),
    }
}

/// `GET /social-media/api/admin/reports` — Report-Liste (Admin).
pub async fn reports_list_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(q): Query<ReportsQuery>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let kind = q.kind.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    if let Some(k) = &kind {
        if !matches!(k.as_str(), "streamer" | "cross" | "admin") {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_kind" }))).into_response();
        }
    }
    let streamer = q.streamer.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let limit = match q.limit.as_deref().unwrap_or("20").parse::<i64>() {
        Ok(n) => n.clamp(1, 20),
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_limit" }))).into_response(),
    };
    let items: Vec<Value> = list_reports(&pool, kind.as_deref(), streamer.as_deref(), limit).await.iter().map(report_record_json).collect();
    Json(json!({ "items": items })).into_response()
}

fn invalid_json() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_json" }))).into_response()
}

/// Filtert eine Plattform-Liste (Python `_normalize_platform_list`): nur
/// youtube/tiktok/instagram, dedupliziert; nicht-Liste (≠ null) → 400.
fn normalize_platform_list(value: Option<&Value>) -> Result<Vec<String>, Response> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut seen = std::collections::HashSet::new();
            let mut cleaned = Vec::new();
            for entry in arr {
                let token = entry.as_str().map(|s| s.trim().to_lowercase()).unwrap_or_default();
                if matches!(token.as_str(), "youtube" | "tiktok" | "instagram") && seen.insert(token.clone()) {
                    cleaned.push(token);
                }
            }
            Ok(cleaned)
        }
        Some(_) => Err((StatusCode::BAD_REQUEST, "platforms must be a list").into_response()),
    }
}

/// `GET /social-media/api/admin/approval/:clip_db_id` — Approval-State (Admin).
pub async fn approval_get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if load_clip_row(&pool, clip_db_id).await.is_none() {
        return clip_not_found();
    }
    let approval = match get_approval_record(&pool, clip_db_id).await {
        Some(r) => serialize_approval_record(&r),
        None => Value::Null,
    };
    Json(json!({ "clip_db_id": clip_db_id, "approval": approval })).into_response()
}

/// `POST /social-media/api/admin/approval/:clip_db_id/decision` — Entscheidung (Admin).
pub async fn approval_decision_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(raw): Path<String>, body: String) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if load_clip_row(&pool, clip_db_id).await.is_none() {
        return clip_not_found();
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    let decision = payload.get("decision").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    let platforms = match normalize_platform_list(payload.get("platforms")) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match handle_decision(&pool, clip_db_id, &decision, &platforms, None).await {
        Ok(record) => {
            let clip = match load_clip_row(&pool, clip_db_id).await {
                Some(c) => serialize_clip_record(&pool, &c).await,
                None => Value::Null,
            };
            Json(json!({ "clip_db_id": clip_db_id, "approval": serialize_approval_record(&record), "clip": clip })).into_response()
        }
        Err(ApprovalError::Db(_)) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "approval_decision_failed" }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_decision", "message": e.to_string() }))).into_response(),
    }
}

/// `GET /social-media/api/admin/settings/auto-approve` — Auto-Approve-Flags (Admin).
pub async fn auto_approve_get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let s = get_auto_approve_settings(&pool).await;
    Json(json!({ "youtube": s.youtube, "tiktok": s.tiktok, "instagram": s.instagram })).into_response()
}

/// `PUT /social-media/api/admin/settings/auto-approve` — Auto-Approve setzen (Admin).
pub async fn auto_approve_put_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, body: String) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    // Fehlende Keys → false (kein Merge, mirror Python).
    let values = AutoApprove {
        youtube: payload.get("youtube").map(coerce_bool).unwrap_or(false),
        tiktok: payload.get("tiktok").map(coerce_bool).unwrap_or(false),
        instagram: payload.get("instagram").map(coerce_bool).unwrap_or(false),
    };
    match set_auto_approve_settings(&pool, values, None).await {
        Ok(r) => Json(json!({ "youtube": r.youtube, "tiktok": r.tiktok, "instagram": r.instagram })).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "db" }))).into_response(),
    }
}

/// Öffentliche Dashboard-Origin für die OAuth-Redirect-URIs. Konfigurierbar
/// (`SOCIAL_MEDIA_PUBLIC_ORIGIN`), Default = Python-Fallback. (Statt Pythons
/// Request-Header-Ableitung — gleicher Effekt: die bei den Plattformen
/// registrierte Callback-Basis.)
fn oauth_public_origin() -> String {
    std::env::var("SOCIAL_MEDIA_PUBLIC_ORIGIN")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://admin.deutsche-deadlock-community.de".to_string())
}

/// Internes Dashboard-Redirect-Ziel (Python `_dashboard_url`).
fn dashboard_url(key: &str, value: &str) -> String {
    format!("/social-media?{key}={value}")
}

/// 302-Redirect (Python `web.HTTPFound`).
fn redirect_found(url: &str) -> Response {
    (StatusCode::FOUND, [(axum::http::header::LOCATION, url.to_string())]).into_response()
}

/// Baut den OAuthManager inline aus dem Master-Key.
fn build_oauth_manager(pool: PgPool) -> Option<OAuthManager> {
    let cipher = Arc::new(FieldCipher::from_env().ok()?);
    Some(OAuthManager::new(pool, cipher))
}

fn is_supported_platform(p: &str) -> bool {
    matches!(p, "tiktok" | "youtube" | "instagram")
}

/// `GET /social-media/oauth/start/:platform` — OAuth-Flow starten (Redirect).
pub async fn oauth_start_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(platform): Path<String>, Query(q): Query<StreamerQuery>) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if !is_supported_platform(&platform) {
        return (StatusCode::BAD_REQUEST, "Invalid platform").into_response();
    }
    let Some(mgr) = build_oauth_manager(pool) else {
        return redirect_found(&dashboard_url("oauth_error", "oauth_start_failed"));
    };
    let redirect_uri = format!("{}/social-media/oauth/callback/{}", oauth_public_origin(), platform);
    match mgr.generate_auth_url(&platform, scope.as_deref(), &redirect_uri).await {
        Ok(auth_url) => redirect_found(&auth_url),
        Err(_) => redirect_found(&dashboard_url("oauth_error", "oauth_start_failed")),
    }
}

/// `?code=&state=&error=` für den OAuth-Callback.
#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// `GET /social-media/oauth/callback[/:platform]` — Provider-Callback (öffentlich,
/// Security über den State-Token).
pub async fn oauth_callback_handler(State(pool): State<PgPool>, platform: Option<Path<String>>, Query(q): Query<OAuthCallbackQuery>) -> Response {
    if q.error.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
        return redirect_found(&dashboard_url("oauth_error", "provider_error"));
    }
    let (code, state) = match (
        q.code.as_deref().filter(|s| !s.is_empty()),
        q.state.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(c), Some(s)) => (c, s),
        _ => return (StatusCode::BAD_REQUEST, "Missing code or state").into_response(),
    };
    let expected_platform = platform.as_ref().map(|p| p.trim().to_lowercase()).filter(|s| !s.is_empty());
    let callback_redirect_uri = expected_platform.as_ref().map(|p| format!("{}/social-media/oauth/callback/{}", oauth_public_origin(), p));
    let Some(mgr) = build_oauth_manager(pool) else {
        return redirect_found(&dashboard_url("oauth_error", "callback_failed"));
    };
    match mgr.handle_callback(code, state, expected_platform.as_deref(), callback_redirect_uri.as_deref()).await {
        Ok(result) => {
            let platform = if is_supported_platform(&result.platform) { result.platform } else { "unknown".to_string() };
            redirect_found(&dashboard_url("oauth_success", &platform))
        }
        Err(OAuthError::StateInvalid | OAuthError::RedirectMismatch) => redirect_found(&dashboard_url("oauth_error", "invalid_callback")),
        Err(OAuthError::Exchange { .. }) => redirect_found(&dashboard_url("oauth_error", "token_exchange_failed")),
        Err(_) => redirect_found(&dashboard_url("oauth_error", "callback_failed")),
    }
}

/// `POST /social-media/oauth/disconnect/:platform` — Plattform trennen.
pub async fn oauth_disconnect_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Path(platform): Path<String>, Query(q): Query<StreamerQuery>) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if !is_supported_platform(&platform) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Invalid platform" }))).into_response();
    }
    let result = sqlx::query(
        "UPDATE social_media_platform_auth SET enabled = 0 \
         WHERE platform = $1 AND (streamer_login = $2 OR (streamer_login IS NULL AND $2::text IS NULL))",
    )
    .bind(&platform)
    .bind(scope.as_deref())
    .execute(&pool)
    .await;
    match result {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "disconnect_failed" }))).into_response(),
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
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT UNIQUE, clip_url TEXT, clip_thumbnail_url TEXT, streamer_login TEXT, twitch_user_id TEXT, status TEXT DEFAULT 'pending', created_at TEXT, duration_seconds DOUBLE PRECISION, view_count INTEGER, clip_title TEXT, game_name TEXT, source_kind TEXT DEFAULT 'twitch', upload_local_path TEXT, local_file_path TEXT, custom_description TEXT, hashtags TEXT, layout_override_json JSONB, retention_until TIMESTAMPTZ, discarded_at TIMESTAMPTZ, uploaded_tiktok INTEGER DEFAULT 0, uploaded_youtube INTEGER DEFAULT 0, uploaded_instagram INTEGER DEFAULT 0, tiktok_uploaded_at TEXT, youtube_uploaded_at TEXT, instagram_uploaded_at TEXT)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TEXT, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TEXT, created_at TEXT DEFAULT CURRENT_TIMESTAMP, completed_at TEXT)",
            "CREATE TABLE clip_templates_streamer (id SERIAL PRIMARY KEY, streamer_login TEXT, template_name TEXT, description_template TEXT, hashtags TEXT, is_default INTEGER DEFAULT 0, created_at TEXT DEFAULT CURRENT_TIMESTAMP, updated_at TEXT DEFAULT CURRENT_TIMESTAMP, UNIQUE (streamer_login, template_name))",
            "CREATE TABLE clip_templates_global (id SERIAL PRIMARY KEY, template_name TEXT UNIQUE, description_template TEXT, hashtags TEXT, category TEXT, usage_count INTEGER DEFAULT 0, created_at TEXT DEFAULT CURRENT_TIMESTAMP, created_by TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', aliases JSONB NOT NULL DEFAULT '[]'::jsonb, weight INTEGER NOT NULL DEFAULT 1, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE twitch_clips_social_analytics (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, bucket TEXT, views INTEGER, likes INTEGER, comments INTEGER, shares INTEGER, watch_time_seconds INTEGER, ctr_percent NUMERIC(5,2), engagement_rate NUMERIC(5,2), provider TEXT, synced_at TIMESTAMPTZ, next_pull_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_reports (id SERIAL PRIMARY KEY, kind TEXT NOT NULL, streamer_login TEXT, period_start TIMESTAMPTZ NOT NULL, period_end TIMESTAMPTZ NOT NULL, content_md TEXT NOT NULL, model TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)",
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

    fn clips_query(status: Option<&str>) -> AdminClipsQuery {
        AdminClipsQuery { page: None, page_size: None, status: status.map(String::from), streamer: None }
    }

    #[tokio::test]
    async fn admin_clips_list_detail_discard() {
        let Some(pool) = make_pool("t_dash_sm_admin_clips").await else { return };
        // Clip A: tiktok hochgeladen, mit Enrichment + Approval.
        let a: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, clip_title, status, created_at, uploaded_tiktok) VALUES ('a', 'nani', 'Clip A', 'ready', '2026-06-10', 1) RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, status, llm_provider, hashtags_youtube, hashtags_tiktok) VALUES ($1, 'done', 'ollama', '[\"a\",\"b\"]'::jsonb, '[\"b\",\"c\"]'::jsonb)").bind(a).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state) VALUES ($1, 'approved')").bind(a).execute(&pool).await.unwrap();
        // Clip B: verworfen.
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status, created_at, discarded_at) VALUES ('b', 'nani', 'discarded', '2026-06-09', NOW())").execute(&pool).await.unwrap();

        // Liste: total 2, neuester (A) zuerst.
        let resp = admin_clips_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(clips_query(None))).await;
        let v = body_json(resp).await;
        assert_eq!(v["total"], 2);
        assert_eq!(v["items"][0]["clip_db_id"], a);

        // Status-Filter "discarded" → nur B.
        let resp = admin_clips_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(clips_query(Some("discarded")))).await;
        assert_eq!(body_json(resp).await["total"], 1);

        // Detail von A: Merge aus Enrichment + Approval + platform_status + effective_layout.
        let resp = admin_clip_detail_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(a.to_string())).await;
        let v = body_json(resp).await;
        assert_eq!(v["enrichment_status"], "done");
        assert_eq!(v["enrichment_summary"]["top_hashtags"], json!(["a", "b", "c"])); // dedup
        assert_eq!(v["enrichment_summary"]["provider"], "ollama");
        assert_eq!(v["platform_status"]["tiktok"], true);
        assert_eq!(v["platform_status"]["youtube"], false);
        assert_eq!(v["approval"]["state"], "approved");
        assert!(v["effective_layout"]["version"] == 1);

        // Discard von A → discarded_at gesetzt im zurückgegebenen Record.
        let resp = admin_clip_discard_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(a.to_string())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!body_json(resp).await["discarded_at"].is_null());

        // Fehlerpfade.
        assert_eq!(admin_clip_detail_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("abc".into())).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(admin_clip_detail_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("99999".into())).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(admin_clip_discard_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("99999".into())).await.status(), StatusCode::NOT_FOUND);
        // Partner → 403.
        assert_eq!(admin_clips_handler(partner("nani"), State(pool.clone()), Query(clips_query(None))).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn approval_und_auto_approve() {
        let Some(pool) = make_pool("t_dash_sm_approval").await else { return };
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status) VALUES ('a', 'nani', 'awaiting_approval') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, title_tiktok) VALUES ($1, 'TT')").bind(clip).execute(&pool).await.unwrap();

        // approval-get vor Entscheidung → approval null.
        let resp = approval_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string())).await;
        assert!(body_json(resp).await["approval"].is_null());

        // decision approve mit tiktok → state approved + Queue.
        let resp = approval_decision_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), "{\"decision\":\"approve\",\"platforms\":[\"tiktok\"]}".into()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["approval"]["state"], "approved");
        assert_eq!(v["approval"]["approved_platforms"], json!(["tiktok"]));
        assert!(!v["clip"].is_null());

        // decision approve ohne Plattform + ohne Auto-Approve → 400 invalid_decision.
        let resp = approval_decision_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), "{\"decision\":\"approve\",\"platforms\":[]}".into()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // platforms kein Array → 400.
        let resp = approval_decision_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), "{\"decision\":\"approve\",\"platforms\":\"tiktok\"}".into()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // auto-approve get (Defaults false) → put → get.
        let resp = auto_approve_get_handler(DashboardAuthLevel::Admin, State(pool.clone())).await;
        let v = body_json(resp).await;
        assert_eq!(v, json!({ "youtube": false, "tiktok": false, "instagram": false }));
        let resp = auto_approve_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), "{\"youtube\":true,\"tiktok\":\"on\"}".into()).await;
        let v = body_json(resp).await;
        assert_eq!(v, json!({ "youtube": true, "tiktok": true, "instagram": false })); // missing instagram → false
        let resp = auto_approve_get_handler(DashboardAuthLevel::Admin, State(pool.clone())).await;
        assert_eq!(body_json(resp).await["youtube"], true);

        // Partner → 403, invalid clip → 400.
        assert_eq!(auto_approve_get_handler(partner("nani"), State(pool.clone())).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(approval_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("abc".into())).await.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn enrichment_analytics_reports_get() {
        let Some(pool) = make_pool("t_dash_sm_reads").await else { return };
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('a', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // enrichment-get: ensure_enrichment_row legt pending an.
        let resp = enrichment_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], clip);
        assert_eq!(v["status"], "pending");
        assert_eq!(v["hashtags_youtube"], json!([])); // leere Liste, nicht null

        // analytics-get: zwei Snapshots.
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, views, likes, engagement_rate, provider, synced_at) VALUES ($1, 'tiktok', '24h', 100, 10, 12.5, 'tiktok_open_api_v2', NOW())").bind(clip).execute(&pool).await.unwrap();
        let resp = clip_analytics_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string())).await;
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], clip);
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["views"], 100);
        assert_eq!(v["items"][0]["engagement_rate"], 12.5);

        // reports-list: insert + Filter.
        sqlx::query("INSERT INTO social_media_reports (kind, streamer_login, period_start, period_end, content_md) VALUES ('streamer', 'nani', NOW()-INTERVAL '7 days', NOW(), '# R')").execute(&pool).await.unwrap();
        let resp = reports_list_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(ReportsQuery { kind: None, streamer: None, limit: None })).await;
        assert_eq!(body_json(resp).await["items"].as_array().unwrap().len(), 1);
        // invalid_kind → 400.
        let resp = reports_list_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(ReportsQuery { kind: Some("bogus".into()), streamer: None, limit: None })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // invalid_limit → 400.
        let resp = reports_list_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(ReportsQuery { kind: None, streamer: None, limit: Some("x".into()) })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Fehlerpfade: nicht existierender Clip → 404, Partner → 403.
        assert_eq!(enrichment_get_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("99999".into())).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(reports_list_handler(partner("nani"), State(pool.clone()), Query(ReportsQuery { kind: None, streamer: None, limit: None })).await.status(), StatusCode::FORBIDDEN);

        // enrichment-PUT: Titel setzen + hashtags (#-Präfix/dedup).
        let resp = enrichment_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), "{\"title_youtube\":\"YT\",\"hashtags_youtube\":[\"a\",\"#a\",\"b\"]}".into()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["title_youtube"], "YT");
        assert_eq!(v["hashtags_youtube"], json!(["#a", "#b"])); // #-Präfix + dedup
        // ungültiges Feld (Zahl) → 400 invalid_field.
        let resp = enrichment_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), "{\"title_youtube\":5}".into()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // nicht existierender Clip → 404.
        assert_eq!(enrichment_put_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("99999".into()), "{}".into()).await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn enrichment_run_skips_without_transcriber_and_llm() {
        let Some(pool) = make_pool("t_dash_sm_enrich_run").await else { return };
        std::env::set_var("OLLAMA_HOST", "127.0.0.1:59999"); // LLM → Fallback (schnell, deterministisch)

        // Clip ohne Video-Pfad → Transkription übersprungen; LLM scheitert →
        // Pipeline endet bei skipped_no_key.
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('r', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        let resp = enrichment_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), String::new()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], clip);
        assert_eq!(v["outcome"]["status"], "skipped_no_key");
        assert_eq!(v["enrichment"]["clip_db_id"], clip);

        // force im Body wird akzeptiert (kein Parse-Fehler) → weiterhin OK.
        let resp = enrichment_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path(clip.to_string()), "{\"force\":true}".into()).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Fehlerpfade: ungültige ID → 400, fehlender Clip → 404, Partner → 403.
        assert_eq!(enrichment_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("x".into()), String::new()).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(enrichment_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("99999".into()), String::new()).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(enrichment_run_handler(partner("nani"), State(pool.clone()), Path(clip.to_string()), String::new()).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn reports_run_kinds() {
        let Some(pool) = make_pool("t_dash_sm_reports_run").await else { return };
        std::env::set_var("OLLAMA_HOST", "127.0.0.1:59999"); // LLM → Fallback (schnell)

        // kind=streamer → erzeugt einen Streamer-Report (No-Data-Fallback).
        let resp = reports_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), "{\"kind\":\"streamer\",\"streamer\":\"nani\"}".into()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["kind"], "streamer");
        assert_eq!(v["streamer_login"], "nani");
        assert!(v["id"].is_number());

        // kind=cross → Cross-Report.
        let resp = reports_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), "{\"kind\":\"cross\"}".into()).await;
        assert_eq!(body_json(resp).await["kind"], "cross");

        // ungültiger kind → 400 invalid_kind.
        let resp = reports_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), "{\"kind\":\"admin\"}".into()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // streamer ohne streamer-Feld → 400 streamer_required.
        let resp = reports_run_handler(DashboardAuthLevel::Admin, State(pool.clone()), "{\"kind\":\"streamer\"}".into()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Partner → 403.
        assert_eq!(reports_run_handler(partner("nani"), State(pool.clone()), "{\"kind\":\"cross\"}".into()).await.status(), StatusCode::FORBIDDEN);
    }

    fn location(resp: &Response) -> String {
        resp.headers().get(axum::http::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
    }

    #[tokio::test]
    async fn oauth_start_callback_disconnect() {
        let Some(pool) = make_pool("t_dash_sm_oauth").await else { return };

        // start: ungültige Plattform → 400.
        assert_eq!(oauth_start_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("snapchat".into()), Query(StreamerQuery { streamer: None })).await.status(), StatusCode::BAD_REQUEST);
        // start: gültige Plattform → 302 (Auth-URL oder oauth_error-Redirect, je nach Env).
        assert_eq!(oauth_start_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("tiktok".into()), Query(StreamerQuery { streamer: None })).await.status(), StatusCode::FOUND);
        // start: None-Auth → 401, Partner-Cross-Account → 403.
        assert_eq!(oauth_start_handler(DashboardAuthLevel::None, State(pool.clone()), Path("tiktok".into()), Query(StreamerQuery { streamer: None })).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(oauth_start_handler(partner("nani"), State(pool.clone()), Path("tiktok".into()), Query(StreamerQuery { streamer: Some("other".into()) })).await.status(), StatusCode::FORBIDDEN);

        // callback: error-Param → 302 provider_error.
        let resp = oauth_callback_handler(State(pool.clone()), None, Query(OAuthCallbackQuery { code: None, state: None, error: Some("access_denied".into()) })).await;
        assert_eq!(resp.status(), StatusCode::FOUND);
        assert!(location(&resp).contains("oauth_error=provider_error"));
        // callback: fehlender code/state → 400.
        assert_eq!(oauth_callback_handler(State(pool.clone()), None, Query(OAuthCallbackQuery { code: None, state: None, error: None })).await.status(), StatusCode::BAD_REQUEST);

        // disconnect: setzt enabled=0 (kein Cipher nötig).
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login, enabled) VALUES ('tiktok', 'nani', 1)").execute(&pool).await.unwrap();
        let resp = oauth_disconnect_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("tiktok".into()), Query(StreamerQuery { streamer: Some("nani".into()) })).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["success"], true);
        let enabled: i32 = sqlx::query_scalar("SELECT enabled FROM social_media_platform_auth WHERE platform='tiktok' AND streamer_login='nani'").fetch_one(&pool).await.unwrap();
        assert_eq!(enabled, 0);
        // disconnect: ungültige Plattform → 400.
        assert_eq!(oauth_disconnect_handler(DashboardAuthLevel::Admin, State(pool.clone()), Path("snap".into()), Query(StreamerQuery { streamer: None })).await.status(), StatusCode::BAD_REQUEST);
    }

    fn mark_body(clip_id: Option<i32>, platforms: Value) -> MarkUploadedBody {
        MarkUploadedBody { clip_id: clip_id.map(|c| json!(c)), platforms, streamer: None }
    }

    #[tokio::test]
    async fn mark_uploaded_handler_paths() {
        let Some(pool) = make_pool("t_dash_sm_mark").await else { return };
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok', 'nani')").execute(&pool).await.unwrap();
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('c1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // Erfolg.
        let resp = mark_uploaded_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(mark_body(Some(clip), json!(["tiktok"])))).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["success"], true);
        let up: i32 = sqlx::query_scalar("SELECT uploaded_tiktok FROM twitch_clips_social_media WHERE id = $1").bind(clip).fetch_one(&pool).await.unwrap();
        assert_eq!(up, 1);

        // Fehlende clip_id → 400, leere platforms → 400.
        assert_eq!(mark_uploaded_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(mark_body(None, json!(["tiktok"])))).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(mark_uploaded_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(mark_body(Some(clip), json!([])))).await.status(), StatusCode::BAD_REQUEST);
        // Partner mit fremdem Clip → 403.
        assert_eq!(mark_uploaded_handler(partner("other"), State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(mark_body(Some(clip), json!(["tiktok"])))).await.status(), StatusCode::FORBIDDEN);
    }

    /// Erzeugt eine winzige gültige MP4 via ffmpeg; None wenn ffmpeg fehlt.
    async fn tiny_mp4() -> Option<Vec<u8>> {
        let path = std::env::temp_dir().join("tb_upload_test_src.mp4");
        let out = tokio::process::Command::new("ffmpeg")
            .args(["-f", "lavfi", "-i", "testsrc=duration=1:size=128x128:rate=10", "-pix_fmt", "yuv420p", "-y", &path.to_string_lossy()])
            .output()
            .await
            .ok()?;
        if !out.status.success() {
            return None;
        }
        tokio::fs::read(&path).await.ok()
    }

    #[tokio::test]
    async fn upload_clip_validierung() {
        let Some(pool) = make_pool("t_dash_sm_upload").await else { return };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('nani', '1')").execute(&pool).await.unwrap();
        let base = std::env::temp_dir().join("tb_upload_test_dash").to_string_lossy().into_owned();
        let _ = std::fs::remove_dir_all(&base);

        // Ungültiger Streamer-Slug → 400.
        let resp = process_uploaded_clip(&pool, &base, Some("bad slug!"), None, None, b"xxxx").await.unwrap_err();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Unbekannter Streamer → 404.
        let resp = process_uploaded_clip(&pool, &base, Some("ghost"), None, None, b"xxxxftypisom").await.unwrap_err();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // Bekannter Streamer, aber keine MP4 (kein ftyp) → 415.
        let resp = process_uploaded_clip(&pool, &base, Some("nani"), Some("c1"), None, b"not a video at all").await.unwrap_err();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        // Happy-Path mit echter MP4 (best-effort, nur wenn ffmpeg da).
        if let Some(mp4) = tiny_mp4().await {
            let payload = process_uploaded_clip(&pool, &base, Some("nani"), Some("good1"), Some("Mein Clip"), &mp4).await.unwrap();
            assert!(payload["clip_db_id"].as_i64().unwrap() > 0);
            assert_eq!(payload["clip_id"], "good1");
            assert!(std::path::Path::new(&format!("{base}/nani/good1.mp4")).exists());
            // Duplikat → 409.
            let resp = process_uploaded_clip(&pool, &base, Some("nani"), Some("good1"), None, &mp4).await.unwrap_err();
            assert_eq!(resp.status(), StatusCode::CONFLICT);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn batch_upload_handler_paths() {
        let Some(pool) = make_pool("t_dash_sm_batch").await else { return };
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, clip_title, created_at) VALUES ('a', 'nani', 'A', '2026-06-10')").execute(&pool).await.unwrap();

        // Admin mit streamer + tiktok → 1 eingereiht.
        let resp = batch_upload_handler(
            DashboardAuthLevel::Admin,
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(BatchUploadBody { streamer: Some("nani".into()), platforms: json!(["tiktok"]), apply_default_template: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["success"], true);
        assert_eq!(v["stats"]["queued"], 1);

        // platforms leer → 400.
        let resp = batch_upload_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(BatchUploadBody { streamer: Some("nani".into()), platforms: json!([]), apply_default_template: None })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Admin ohne streamer (required) → 400.
        let resp = batch_upload_handler(DashboardAuthLevel::Admin, State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(BatchUploadBody { streamer: None, platforms: json!(["tiktok"]), apply_default_template: None })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Partner cross-account → 403.
        let resp = batch_upload_handler(partner("nani"), State(pool.clone()), Query(StreamerQuery { streamer: None }), Json(BatchUploadBody { streamer: Some("other".into()), platforms: json!(["tiktok"]), apply_default_template: None })).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn fetch_clips_auth_pfade() {
        let Some(pool) = make_pool("t_dash_sm_fetch").await else { return };
        // None-Auth → 401 (vor Scope/Helix).
        assert_eq!(fetch_clips_handler(DashboardAuthLevel::None, State(pool.clone()), Json(FetchClipsBody { streamer: Some("nani".into()), limit: None, days: None })).await.status(), StatusCode::UNAUTHORIZED);
        // Admin ohne streamer (required) → 400.
        assert_eq!(fetch_clips_handler(DashboardAuthLevel::Admin, State(pool.clone()), Json(FetchClipsBody { streamer: None, limit: None, days: None })).await.status(), StatusCode::BAD_REQUEST);
        // Partner cross-account → 403.
        assert_eq!(fetch_clips_handler(partner("nani"), State(pool.clone()), Json(FetchClipsBody { streamer: Some("other".into()), limit: None, days: None })).await.status(), StatusCode::FORBIDDEN);
    }
}

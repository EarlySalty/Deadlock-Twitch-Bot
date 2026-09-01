#![allow(clippy::result_large_err)]

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
    body::{Body, Bytes},
    extract::{multipart::Field, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::path::Path as FsPath;
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use tb_crypto::FieldCipher;
use tb_social_media::analytics::{
    list_clip_analytics, list_reports, ClipAnalyticsSnapshot, SocialMediaReportRecord,
};
use tb_social_media::approval::{
    cancel_scheduled_uploads, get_approval_record, handle_decision, serialize_approval_record,
    ApprovalError, ContentMutationError, DECISION_APPROVE,
};
use tb_social_media::clip_analytics::get_analytics_summary;
use tb_social_media::clip_manager::{
    batch_upload_all_new, get_clips_for_dashboard, reconcile_provider_uploads,
    register_manual_upload_in_transaction, ManualReconciliationError, ManualReconciliationRequest,
    ManualUploadError,
};
use tb_social_media::clip_queue::queue_upload;
use tb_social_media::clip_templates::{
    apply_template_to_clip, create_streamer_template, get_global_templates, get_last_hashtags,
    get_streamer_templates, GlobalTemplate, StreamerTemplate,
};
use tb_social_media::credentials::{CredentialManager, PlatformStatus};
use tb_social_media::enrich_pipeline::{ClipEnrichmentPipeline, PipelineError};
use tb_social_media::enrichment::{
    ensure_enrichment_row_checked, get_enrichment, get_enrichment_checked, update_manual_edit,
    EnrichmentRecord,
};
use tb_social_media::forms::{submit_clip_form, FormKey, FormSubmissionOutcome};
use tb_social_media::layout::{
    apply_default_layout, default_streamer_layout, get_clip_effective_layout, get_streamer_layout,
    set_clip_layout_override, upsert_streamer_layout, LayoutMutationError, StreamerLayout,
};
use tb_social_media::llm_dispatch::LlmDispatcher;
use tb_social_media::oauth::{OAuthError, OAuthManager};
use tb_social_media::partner_access::{
    is_partner_granted, list_partner_access, set_partner_access,
};
use tb_social_media::posting_plan::{
    berechne_vorrat, ensure_streamer_rows, load_categories_checked,
    load_platform_schedules_checked, load_streamer_settings_checked, save_category_setting,
    save_platform_schedule, save_streamer_settings, verfuegbare_clips, ApprovalMode,
    CategoryOption, PlatformSchedule, PoolForecast, ReleaseMode, StreamerSettings,
    StreamerSettingsSaveError, PLATFORMS,
};
use tb_social_media::preparation::{
    ClipPreparationRecord, ClipPreparationService, PreparationError, DEFAULT_CLIPS_DIR,
};
use tb_social_media::rendering::{render_privacy, render_terms};
use tb_social_media::report_writer::SocialMediaReportWriter;
use tb_social_media::retention::discard_clip;
use tb_social_media::scheduler::{next_cadence_slot, next_free_slot};
use tb_social_media::seed_vocab::seed_vocab;
use tb_social_media::settings::{coerce_bool, get_posting_schedule, PostingSchedule};
use tb_social_media::video_processor::VideoProcessor;
use tb_social_media::vocab::{delete_vocab_entry, list_vocab, upsert_vocab_entry, VocabEntry};
use tb_social_media::vod_archive::{
    get_vod_archive_settings, set_vod_archive_settings, VodArchiveSettings,
    VOD_ARCHIVE_PRIVACY_VALUES,
};
use tb_social_media::{ClipFetchService, ClipRepository, HelixClipSource};
use tb_transport_twitch::{HelixClient, HelixConfig};

use crate::auth::level::DashboardAuthLevel;
use crate::auth::resolve_streamer_scope;

fn forbidden(message: &str) -> Response {
    (StatusCode::FORBIDDEN, message.to_string()).into_response()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Authentication required." })),
    )
        .into_response()
}

/// `GET /social-media/terms` — öffentlich.
pub async fn terms_handler() -> Html<String> {
    Html(render_terms())
}

/// `GET /social-media/privacy` — öffentlich.
pub async fn privacy_handler() -> Html<String> {
    Html(render_privacy())
}

/// Twitch-Login-Redirect-Ziel der unauthentifizierten HTML-Index-Seite
/// (B15-FIX-index-redirect / finding social_media-2).
const SOCIAL_MEDIA_LOGIN_URL: &str = "/twitch/auth/login?next=%2Fsocial-media";
const SOCIAL_MEDIA_ADMIN_LOGIN_URL: &str = "/twitch/auth/login?next=%2Fsocial-media-admin";

fn social_media_login_url(uri: &Uri) -> &'static str {
    if uri.path().starts_with("/social-media-admin") {
        SOCIAL_MEDIA_ADMIN_LOGIN_URL
    } else {
        SOCIAL_MEDIA_LOGIN_URL
    }
}

/// `GET /social-media` — Dashboard-SPA (Auth erforderlich).
///
/// B15-FIX-index-redirect: Unauthentifiziert liefert die **HTML-Seite** keinen
/// 401-JSON mehr, sondern einen 302-Redirect auf den Twitch-Login (Browser-UX,
/// Python-Parität). Die JSON-Daten-Endpoints behalten ihr 401 (kein Redirect).
pub async fn index_handler(auth: DashboardAuthLevel, uri: Uri) -> Response {
    if matches!(auth, DashboardAuthLevel::None) {
        return axum::response::Redirect::to(social_media_login_url(&uri)).into_response();
    }
    // Die alte HTML-Seite ist abgeloest. Bestehende Links landen auf der SPA,
    // statt ins Leere zu laufen.
    //
    // Der Query-String muss mit: `oauth_callback_handler` schickt den Browser
    // auf `/social-media?oauth_success=youtube` beziehungsweise
    // `?oauth_error=<code>` zurueck. Ohne Weitergabe verliert der Umweg genau
    // die eine Information, wegen der er stattfindet, und ein gescheiterter
    // Verbindungsversuch endet wortlos auf der Kontenseite.
    match weitergereichte_query(&uri) {
        Some(query) => {
            axum::response::Redirect::to(&format!("/social-media-admin?{query}")).into_response()
        }
        None => axum::response::Redirect::to("/social-media-admin").into_response(),
    }
}

/// Shell-Wrapper für den echten `/social-media-admin`-Einstieg. So behält der
/// Login den ursprünglichen Rücksprung; authentifizierte Requests laufen durch
/// den bestehenden SPA-/Host-Guard.
pub async fn social_media_admin_index_handler(
    headers: HeaderMap,
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    if matches!(auth, DashboardAuthLevel::None) {
        return axum::response::Redirect::to(SOCIAL_MEDIA_ADMIN_LOGIN_URL).into_response();
    }
    crate::handlers::spa::social_media_admin_handler(headers, auth, State(pool)).await
}

/// Parameter, die an die SPA weitergereicht werden. Mehr braucht der Umweg
/// nicht: `oauth_callback_handler` setzt genau diese beiden.
const WEITERGEREICHTE_PARAMETER: [&str; 2] = ["oauth_success", "oauth_error"];

/// Laengstens so lang darf ein weitergereichter Wert sein. Plattformnamen und
/// Fehlerkuerzel bleiben weit darunter.
const MAX_PARAMETER_LAENGE: usize = 64;

/// Query-String, der an die SPA weitergereicht werden darf.
///
/// Erlaubnisliste statt Zeichenklassenfilter: der Wert landet in einem
/// `Location`-Header, und dorthin gehoert nur, was wir selbst gesetzt haben.
/// Frueher wanderte jeder beliebige, nutzerkontrollierte Query-String
/// unveraendert mit; das war zwar nicht ausnutzbar, aber deutlich weiter
/// gefasst als noetig.
///
/// Werte muessen die Form haben, die wir selbst erzeugen (Plattformname oder
/// Fehlerkuerzel). Alles andere faellt weg, damit ohne Neukodierung nichts
/// Fremdes in den Header kommt.
fn weitergereichte_query(uri: &Uri) -> Option<String> {
    let query = uri.query()?;
    let erlaubt: Vec<String> = query
        .split('&')
        .filter_map(|paar| {
            let (schluessel, wert) = paar.split_once('=')?;
            if !WEITERGEREICHTE_PARAMETER.contains(&schluessel) {
                return None;
            }
            if wert.is_empty() || wert.len() > MAX_PARAMETER_LAENGE {
                return None;
            }
            if !wert
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return None;
            }
            Some(format!("{schluessel}={wert}"))
        })
        .collect();
    if erlaubt.is_empty() {
        return None;
    }
    Some(erlaubt.join("&"))
}

/// `?streamer=` (für scope-gefilterte Endpoints).
#[derive(Debug, Deserialize)]
pub struct StreamerQuery {
    pub streamer: Option<String>,
}

/// Ausdrücklicher Marker für die Sammelverbindung, also die Zeile in
/// `social_media_platform_auth` ohne `streamer_login`.
///
/// Vorher zeigte ein *fehlender* `?streamer=`-Parameter für Admin-Sessions
/// stillschweigend auf genau diese Zeile: „Trennen" für einen einzelnen Kanal
/// kappte damit die Verbindung für alle Kanäle. Die Sammelverbindung ist jetzt
/// nur noch erreichbar, wenn jemand `?streamer=__global__` ausdrücklich
/// mitschickt; ohne Parameter gibt es 400.
///
/// Ein Partner erreicht den Marker nie: `resolve_streamer_scope` klemmt jede
/// Session auf den eigenen Login, `__global__` ist für sie ein fremder Kanal
/// und damit 403.
pub const GLOBAL_SCOPE_MARKER: &str = "__global__";

/// Übersetzt den aufgelösten Scope in den Datenbank-Scope für die
/// Plattform-Zugangsdaten: der Marker steht für die Sammelverbindung
/// (`streamer_login IS NULL`), jeder andere Wert für den Kanal selbst.
fn credential_scope(scope: Option<&str>) -> Option<&str> {
    match scope {
        Some(GLOBAL_SCOPE_MARKER) => None,
        other => other,
    }
}

/// `?streamer=&status=&limit=` für die Clip-Liste.
#[derive(Debug, Deserialize)]
pub struct ClipsQuery {
    pub streamer: Option<String>,
    pub status: Option<String>,
    pub limit: Option<String>,
}

fn invalid_limit() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid_limit", "allowed_range": [1, 200] })),
    )
        .into_response()
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
pub async fn stats_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerQuery>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, q.streamer.as_deref()).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    Json(get_analytics_summary(&pool, scope.as_deref()).await).into_response()
}

/// `GET /social-media/api/clips` — Clip-Liste (limit 1..200, scope, status).
pub async fn clips_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<ClipsQuery>,
) -> Response {
    let limit = match parse_limit(q.limit.as_deref()) {
        Ok(l) => l,
        Err(()) => return invalid_limit(),
    };
    let scope = match require_sm_access(&auth, &pool, q.streamer.as_deref()).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let clips = get_clips_for_dashboard(&pool, scope.as_deref(), q.status.as_deref(), limit).await;
    Json(clips).into_response()
}

/// `GET /social-media/api/last-hashtags` — zuletzt genutzte Hashtags.
pub async fn last_hashtags_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerQuery>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, q.streamer.as_deref()).await {
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

/// User-gelieferte ID → positive i64 (Python `_normalize_clip_id`; akzeptiert
/// Zahl oder numerischen String).
fn normalize_id(value: Option<&Value>) -> Option<i64> {
    let n = value?
        .as_i64()
        .or_else(|| value?.as_str().and_then(|s| s.trim().parse::<i64>().ok()))?;
    if n > 0 {
        Some(n)
    } else {
        None
    }
}

async fn clip_owned_by_streamer(pool: &PgPool, clip_id: i64, streamer: &str) -> bool {
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

async fn streamer_template_owned(pool: &PgPool, template_id: i64, streamer: &str) -> bool {
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

/// Partner-Freigabe-Guard für Clip-basierte Admin-Handler: lädt den
/// Clip-Owner und prüft die Freigabe. `None` wenn kein Owner oder OK.
async fn guard_partner_access_for_clip(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    clip_db_id: i64,
) -> Option<Response> {
    let streamer_login: Option<String> = sqlx::query_scalar(
        "SELECT streamer_login FROM twitch_clips_social_media WHERE id = $1 LIMIT 1",
    )
    .bind(clip_db_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    if let Some(ref login) = streamer_login {
        check_partner_access_guard(pool, auth, login).await
    } else {
        None
    }
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
pub async fn templates_global_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<CategoryQuery>,
) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    let templates = get_global_templates(&pool, q.category.as_deref()).await;
    let list: Vec<Value> = templates.iter().map(global_template_json).collect();
    Json(json!({ "templates": list })).into_response()
}

/// `GET /social-media/api/templates/streamer` — Templates des (scope-)Streamers.
pub async fn templates_streamer_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerQuery>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, q.streamer.as_deref()).await {
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
pub async fn create_template_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<CreateTemplateBody>,
) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    // required=true: Admin muss `streamer` mitgeben, Partner bekommt eigenen.
    let scope = match resolve_streamer_scope(&auth, body.streamer.as_deref(), true) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let streamer = scope.unwrap_or_default();
    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung.
    if let Some(guard_response) = check_partner_access_guard(&pool, &auth, &streamer).await {
        return guard_response;
    }
    let name = body.template_name.unwrap_or_default();
    let description = body.description.unwrap_or_default();
    if name.is_empty() || description.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "template_name and description are required" })),
        )
            .into_response();
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
    let (Some(clip_id), Some(template_id)) = (
        normalize_id(body.clip_id.as_ref()),
        normalize_id(body.template_id.as_ref()),
    ) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "clip_id and template_id are required" })),
        )
            .into_response();
    };
    // Streamer aus Body, sonst Query.
    let requested = body.streamer.as_deref().or(q.streamer.as_deref());
    let scope = match resolve_streamer_scope(&auth, requested, false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung.
    if let Some(streamer) = &scope {
        if let Some(guard_response) = check_partner_access_guard(&pool, &auth, streamer).await {
            return guard_response;
        }
    }
    if let Some(streamer) = &scope {
        if !clip_owned_by_streamer(&pool, clip_id, streamer).await {
            return (
                StatusCode::FORBIDDEN,
                Json(
                    json!({ "error": "forbidden: clip does not belong to authenticated streamer" }),
                ),
            )
                .into_response();
        }
        if !body.is_global && !streamer_template_owned(&pool, template_id, streamer).await {
            return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden: template does not belong to authenticated streamer" }))).into_response();
        }
    }
    if apply_template_to_clip(&pool, clip_id, template_id, body.is_global).await {
        Json(json!({ "success": true, "message": "Template applied successfully" })).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to apply template" })),
        )
            .into_response()
    }
}

/// Spiegelt Pythons `_require_admin`: nur Localhost/Admin; Partner → 403,
/// None → 401.
fn require_admin(auth: &DashboardAuthLevel) -> Result<(), Response> {
    match auth {
        DashboardAuthLevel::Admin { .. } => Ok(()),
        DashboardAuthLevel::Partner { .. } => Err(forbidden("Admin access required.")),
        DashboardAuthLevel::None => Err(unauthorized()),
    }
}

/// Zugriff auf das Social-Media-Dashboard: Admin/Localhost frei, Partner nur
/// mit Freigabe in `social_media_partner_access` und nur für den eigenen Kanal.
///
/// Ersetzt `require_admin` an allen Endpoints mit Streamer-Bezug. Der
/// zurückgegebene Scope ist bei Partnern immer der eigene Login, bei Admins der
/// angefragte (oder `None` für alle).
#[allow(clippy::result_large_err)]
async fn require_sm_access(
    auth: &DashboardAuthLevel,
    pool: &PgPool,
    requested: Option<&str>,
) -> Result<Option<String>, Response> {
    if let DashboardAuthLevel::Partner { twitch_login, .. } = auth {
        if !is_partner_granted(pool, twitch_login).await {
            return Err(forbidden(
                "Social Media ist für deinen Kanal noch nicht freigeschaltet.",
            ));
        }
    }
    resolve_streamer_scope(auth, requested, false)
}

/// Ownership-Prüfung für Endpoints, die nur eine `clip_db_id` kennen: Partner
/// dürfen nur eigene Clips sehen und ändern. Admin-Scope (`None`) lässt durch.
#[allow(clippy::result_large_err)]
async fn require_clip_in_scope(
    pool: &PgPool,
    clip_db_id: i64,
    scope: Option<&str>,
) -> Result<(), Response> {
    match scope {
        None => Ok(()),
        Some(streamer) => {
            if clip_owned_by_streamer(pool, clip_db_id, streamer).await {
                Ok(())
            } else {
                Err(forbidden("Dieser Clip gehört nicht zu deinem Kanal."))
            }
        }
    }
}

/// Stufe der Session fuer die Clip-Pipeline.
///
/// Admin/Localhost arbeiten ohne eigenen Kanal und zaehlen deshalb als Pro;
/// ein Partner bekommt die Stufe seines Plans.
async fn sm_stufe(pool: &PgPool, auth: &DashboardAuthLevel) -> tb_analytics::stufe::Stufe {
    crate::auth::stufe_fuer_auth(pool, auth).await
}

/// Clip-Kontingent-Guard fuer alles, was der Streamer selbst anstoesst.
///
/// **Sperrt heute niemanden.** Ob die Monatsgrenze greift, entscheidet
/// `tb_analytics::stufe::sperre_greift(Stufe::Pro)` beim Bau des Kontingents:
/// der Ausweg aus der Grenze ist Creator Pro, und die Stufe steht im Katalog auf
/// `buchbar = false`. Bis dahin ist `ClipKontingent::frei()` immer `true` und
/// `rest()` immer `None`, gezaehlt wird trotzdem. Wird Pro buchbar, greift die
/// Grenze (Free 3 Clips im Monat, Plus 10, Pro unbegrenzt) ohne Codeaenderung.
///
/// Greift sie, kommt 403 mit dem Zaehlerstand, damit das Dashboard es anzeigen
/// kann, statt nur "geht nicht" zu melden. Admin ohne Kanalbezug (`streamer`
/// leer) laeuft immer durch.
///
/// Zaehlung und Sperre decken sich: gebucht wird nur, was durch diese Handler
/// laeuft (`kontingent_verbraucht_at`). Was der Hintergrund-Fetcher holt und
/// was Zuschauer per `!clip` anlegen, zaehlt nicht mit und wird hier auch nicht
/// gesperrt.
async fn clip_kontingent_guard(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    streamer: &str,
) -> Result<tb_analytics::stufe::ClipKontingent, Response> {
    let stufe = sm_stufe(pool, auth).await;
    let kontingent = tb_analytics::stufe::clip_kontingent(pool, stufe, streamer).await;
    if streamer.trim().is_empty() || kontingent.frei() {
        return Ok(kontingent);
    }
    let limit_text = match kontingent.limit {
        Some(limit) => format!("{} von {limit} Clips", kontingent.genutzt),
        None => format!("{} Clips", kontingent.genutzt),
    };
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "clip_limit_erreicht",
            "message": format!(
                "Du hast diesen Monat {limit_text} geholt. Bis zum Monatswechsel kannst du keine neuen anlegen."
            ),
            "kontingent": kontingent.als_json(),
        })),
    )
        .into_response())
}

/// Pro-Guard fuer automatisches Posten (Sammel-Einreihung, Auto-Planung).
///
/// **Sperrt heute niemanden.** `tb_analytics::stufe::auto_posting_erlaubt`
/// haengt an `sperre_greift(Stufe::Pro)`, und Creator Pro steht im Katalog auf
/// `buchbar = false`: `checkout_start_handler` schickt jeden Pro-Kaufversuch auf
/// `/twitch/pricing` zurueck, ein gesperrter Partner koennte sich also nicht
/// freikaufen. Bis Pro buchbar ist, laeuft automatisches Posten wie vor dem
/// Umbau fuer jeden Partner mit Social-Media-Freigabe; danach greift die
/// Pro-Grenze von allein.
async fn auto_posting_guard(pool: &PgPool, auth: &DashboardAuthLevel) -> Option<Response> {
    if tb_analytics::stufe::auto_posting_erlaubt(sm_stufe(pool, auth).await) {
        None
    } else {
        Some(crate::auth::stufe_required_response(
            tb_analytics::stufe::Stufe::Pro,
        ))
    }
}

/// `GET /social-media/api/access/me` — was die eigene Session darf.
///
/// Das Frontend entscheidet damit zwischen Vollansicht, Eigenkanal-Ansicht und
/// dem Hinweis „noch nicht freigeschaltet".
pub async fn my_access_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    match &auth {
        DashboardAuthLevel::Admin { .. } => {
            Json(json!({ "allowed": true, "streamer": null, "isAdmin": true })).into_response()
        }
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            let login = twitch_login.to_lowercase();
            let allowed = is_partner_granted(&pool, &login).await;
            Json(json!({ "allowed": allowed, "streamer": login, "isAdmin": false })).into_response()
        }
        DashboardAuthLevel::None => unauthorized(),
    }
}

/// Actor-ID für die Audit-Felder `updated_by`/`edited_by` (B15-FIX-audit-fields).
///
/// Spiegelt Pythons `_get_editor_user_id` (dashboard.py:173-188): liest die
/// Session-User-ID. Reihenfolge dort: `discord_user_id` → `user_id` →
/// `twitch_user_id`. Im Rust-Auth-Modell trägt nur die Partner-Session eine
/// User-ID (`twitch_user_id`); Localhost hat keine (→ `None`, wie Python). Die
/// Discord-Admin-ID liegt im Session-Payload, wird aber im `Admin`-Unit-Variant
/// nicht mitgeführt — für die aktuell admin-only Schreibpfade ist das NULL (=
/// Python-Localhost-Verhalten); ein künftiger Partner-Schreibpfad erbt korrekt
/// die `twitch_user_id`.
fn editor_user_id(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => {
            let id = twitch_user_id.trim();
            (!id.is_empty()).then(|| id.to_string())
        }
        _ => None,
    }
}

/// Slug-Validierung (`[A-Za-z0-9_-]+`, nicht leer) — liefert die Fehlermeldung
/// (Python `_normalize_safe_slug`).
fn slug_message(raw: Option<&str>, field: &str) -> Result<String, String> {
    let value = raw.unwrap_or("").trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!("{field} must match [A-Za-z0-9_-]+"));
    }
    Ok(value)
}

/// Wie [`slug_message`], Fehler als Plaintext-400 (wie web.HTTPBadRequest).
fn normalize_safe_slug(raw: Option<&str>, field: &str) -> Result<String, Response> {
    slug_message(raw, field).map_err(|m| (StatusCode::BAD_REQUEST, m).into_response())
}

async fn ensure_streamer_exists(pool: &PgPool, slug: &str) -> bool {
    sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

async fn clip_exists(pool: &PgPool, clip_db_id: i64) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM twitch_clips_social_media WHERE id = $1 LIMIT 1")
        .bind(clip_db_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some()
}

fn invalid_layout(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid_layout", "message": message })),
    )
        .into_response()
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
const UPLOAD_MAX_TEXT_BYTES: usize = 500;
const UPLOAD_MAX_SLUG_BYTES: usize = 128;
const UPLOAD_MAX_WIDTH: i64 = 3840;
const UPLOAD_MAX_HEIGHT: i64 = 2160;
const UPLOAD_MAX_FPS: f64 = 120.0;
const UPLOAD_STAGING_WALLCLOCK: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const UPLOAD_STAGING_IDLE: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Clone, Copy)]
struct UploadTiming {
    wallclock: std::time::Duration,
    idle: std::time::Duration,
}

#[derive(Clone, Copy)]
struct UploadLimits {
    file_bytes: usize,
    text_bytes: usize,
    slug_bytes: usize,
}

impl UploadLimits {
    fn production() -> Self {
        Self {
            file_bytes: UPLOAD_MAX_BYTES,
            text_bytes: UPLOAD_MAX_TEXT_BYTES,
            slug_bytes: UPLOAD_MAX_SLUG_BYTES,
        }
    }
}

impl UploadTiming {
    fn production() -> Self {
        Self {
            wallclock: UPLOAD_STAGING_WALLCLOCK,
            idle: UPLOAD_STAGING_IDLE,
        }
    }

    fn operation_deadline(self, total_deadline: tokio::time::Instant) -> tokio::time::Instant {
        total_deadline.min(tokio::time::Instant::now() + self.idle)
    }
}

fn upload_timeout_response() -> Response {
    upload_error(
        StatusCode::REQUEST_TIMEOUT,
        "upload_timeout",
        Some("Der Upload hat zu lange keine Daten geliefert."),
    )
}

fn manual_upload_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Semaphore::new(1))
}

/// Echter MIME-Check über die ISO-BMFF-`ftyp`-Box (B15-FIX-mime).
///
/// libmagic erkennt `video/mp4` strukturiert: die erste Box ist `ftyp`, ihr
/// Tag steht an Offset 4 (nach der 4-Byte-Boxgröße), gefolgt vom Major-Brand
/// (Offset 8). Diese Funktion bildet das nach (statt nur „ftyp irgendwo in den
/// ersten 64 Bytes") und liefert den erkannten MIME-Typ:
/// - `video/mp4` für ISO-/MP4-Brands (`isom`, `mp4*`, `iso*`, `avc1`, `dash`, `M4V `)
/// - `video/quicktime` für den QuickTime-Brand `qt  `
///
/// Python akzeptiert nur `{video/mp4, application/mp4}` (libmagic); wir bleiben
/// gleich streng (QuickTime wird mit-erkannt, aber von [`is_accepted_mp4_mime`]
/// abgelehnt → identische Allowlist).
fn detect_mp4_mime(bytes: &[u8]) -> Option<&'static str> {
    // Mindestens Boxgröße(4) + "ftyp"(4) + Major-Brand(4).
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return None;
    }
    let brand = &bytes[8..12];
    match brand {
        b"qt  " => Some("video/quicktime"),
        b"M4V " | b"M4A " | b"M4P " | b"M4B " => Some("video/mp4"),
        _ => {
            // isom, iso2, iso4-6, mp41/mp42, avc1, dash, mmp4, … → MP4-Familie.
            let b = brand;
            if b.starts_with(b"iso")
                || b.starts_with(b"mp4")
                || b == b"isom"
                || b == b"avc1"
                || b == b"dash"
                || b == b"mmp4"
            {
                Some("video/mp4")
            } else {
                None
            }
        }
    }
}

fn plausible_ftyp_box(bytes: &[u8], file_len: u64) -> bool {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let short_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64;
    let (box_size, minimum_size) = if short_size == 1 {
        if bytes.len() < 20 {
            return false;
        }
        (
            u64::from_be_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            20,
        )
    } else {
        (short_size, 16)
    };
    box_size >= minimum_size && box_size <= file_len
}

/// Python-Allowlist (`{video/mp4, application/mp4}`); QuickTime/andere → false.
fn is_accepted_mp4_mime(mime: &str) -> bool {
    matches!(mime, "video/mp4" | "application/mp4")
}

struct SafeUploadDirectory {
    handle: tokio::fs::File,
    logical_path: std::path::PathBuf,
}

impl SafeUploadDirectory {
    fn child_path(&self, name: &std::ffi::OsStr) -> std::path::PathBuf {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            std::path::PathBuf::from(format!("/proc/self/fd/{}", self.handle.as_raw_fd()))
                .join(name)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.logical_path.join(name)
        }
    }
}

async fn open_or_create_directory_chain(
    path: &FsPath,
    final_mode: u32,
) -> std::io::Result<SafeUploadDirectory> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;

        let mut options = tokio::fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
        let mut current = if path.is_absolute() {
            options.open("/").await?
        } else {
            options.open(".").await?
        };
        for component in path.components() {
            let name = match component {
                std::path::Component::RootDir | std::path::Component::CurDir => continue,
                std::path::Component::Normal(name) => name,
                std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Unsicherer Verzeichnispfad",
                    ));
                }
            };
            use std::os::fd::AsRawFd;
            let child = std::path::PathBuf::from(format!(
                "/proc/self/fd/{}/{}",
                current.as_raw_fd(),
                name.to_string_lossy()
            ));
            let next = match options.open(&child).await {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::fs::create_dir(&child).await?;
                    options.open(&child).await?
                }
                Err(error) => return Err(error),
            };
            current = next;
        }
        current
            .set_permissions(std::fs::Permissions::from_mode(final_mode))
            .await?;
        Ok(SafeUploadDirectory {
            handle: current,
            logical_path: path.to_path_buf(),
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut current = std::path::PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::RootDir => current.push(FsPath::new("/")),
                std::path::Component::CurDir => {}
                std::path::Component::Normal(name) => {
                    current.push(name);
                    match tokio::fs::symlink_metadata(&current).await {
                        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        }
                        Ok(_) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "Symlink-Komponente abgelehnt",
                            ));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            tokio::fs::create_dir(&current).await?;
                        }
                        Err(error) => return Err(error),
                    }
                }
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Unsicherer Verzeichnispfad",
                    ));
                }
            }
        }
        tokio::fs::set_permissions(&current, std::fs::Permissions::from_mode(final_mode)).await?;
        Ok(SafeUploadDirectory {
            handle: tokio::fs::File::open(&current).await?,
            logical_path: current,
        })
    }
}

async fn open_or_create_upload_streamer_directory(
    base_dir: &FsPath,
    streamer_login: &str,
) -> std::io::Result<SafeUploadDirectory> {
    let base = open_or_create_directory_chain(base_dir, 0o2770).await?;
    let child = base.child_path(std::ffi::OsStr::new(streamer_login));
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::PermissionsExt;
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let mut options = tokio::fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
        let directory = match options.open(&child).await {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&child).await?;
                options.open(&child).await?
            }
            Err(error) => return Err(error),
        };
        directory
            .set_permissions(std::fs::Permissions::from_mode(0o2770))
            .await?;
        let logical_path = base.logical_path.join(streamer_login);
        let _ = base.handle.as_raw_fd();
        Ok(SafeUploadDirectory {
            handle: directory,
            logical_path,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        open_or_create_directory_chain(&base.logical_path.join(streamer_login), 0o2770).await
    }
}

async fn open_or_create_upload_work_directory(
    base_dir: &FsPath,
) -> std::io::Result<SafeUploadDirectory> {
    let base = open_or_create_directory_chain(base_dir, 0o2770).await?;
    let child = base.child_path(std::ffi::OsStr::new(".dashboard-work"));
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let mut options = tokio::fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
        let directory = match options.open(&child).await {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&child).await?;
                options.open(&child).await?
            }
            Err(error) => return Err(error),
        };
        let metadata = directory.metadata().await?;
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        // Das Arbeitsverzeichnis enthält die von ffprobe geprüften Inodes und
        // darf deshalb weder von einer anderen UID stammen noch gruppenweit
        // beschreibbar sein.
        if metadata.uid() != unsafe { geteuid() } {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Unsicheres Upload-Arbeitsverzeichnis",
            ));
        }
        directory
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .await?;
        Ok(SafeUploadDirectory {
            handle: directory,
            logical_path: base.logical_path.join(".dashboard-work"),
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        open_or_create_directory_chain(&base.logical_path.join(".dashboard-work"), 0o700).await
    }
}

struct StagedUpload {
    work_directory: SafeUploadDirectory,
    temp_name: String,
    file: tokio::fs::File,
    length: usize,
}

impl StagedUpload {
    async fn create(base_dir: &FsPath) -> Result<Self, Response> {
        let work_directory = open_or_create_upload_work_directory(base_dir)
            .await
            .map_err(|_| upload_error(StatusCode::INTERNAL_SERVER_ERROR, "upload_failed", None))?;
        let temp_name = format!(".upload-{}.tmp.mp4", tb_crypto::random_hex_token(16));
        let temp_path = work_directory.child_path(std::ffi::OsStr::new(&temp_name));
        let mut options = tokio::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(target_os = "linux")]
        options.custom_flags(0o400000);
        let file = options
            .open(&temp_path)
            .await
            .map_err(|_| upload_error(StatusCode::INTERNAL_SERVER_ERROR, "upload_failed", None))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file
                .set_permissions(std::fs::Permissions::from_mode(0o640))
                .await
                .is_err()
            {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(upload_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "upload_failed",
                    None,
                ));
            }
        }
        Ok(Self {
            work_directory,
            temp_name,
            file,
            length: 0,
        })
    }

    async fn cleanup(&self) {
        if let Ok(metadata) = self.file.metadata().await {
            remove_owned_upload_link(
                &self.work_directory,
                std::ffi::OsStr::new(&self.temp_name),
                &metadata,
            )
            .await;
        }
    }
}

async fn validate_staged_upload_inode(
    staged: &StagedUpload,
) -> Result<std::fs::Metadata, Response> {
    let metadata = staged
        .file
        .metadata()
        .await
        .map_err(|_| upload_error(StatusCode::INTERNAL_SERVER_ERROR, "upload_failed", None))?;
    if !metadata.is_file()
        || metadata.len() != staged.length as u64
        || metadata.len() == 0
        || metadata.len() > UPLOAD_MAX_BYTES as u64
    {
        return Err(upload_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_upload_inode",
            None,
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        if metadata.nlink() != 1
            || metadata.uid() != unsafe { geteuid() }
            || metadata.mode() & 0o7777 != 0o640
        {
            return Err(upload_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "invalid_upload_inode",
                None,
            ));
        }
    }
    Ok(metadata)
}

fn upload_error(status: StatusCode, code: &str, message: Option<&str>) -> Response {
    let mut payload = json!({ "error": code });
    if let Some(message) = message {
        payload["message"] = Value::String(message.to_string());
    }
    (status, Json(payload)).into_response()
}

async fn read_limited_multipart_text(
    mut field: Field<'_>,
    max_bytes: usize,
    total_deadline: tokio::time::Instant,
    timing: UploadTiming,
) -> Result<String, Response> {
    let mut bytes = Vec::with_capacity(max_bytes.min(512));
    loop {
        let chunk =
            tokio::time::timeout_at(timing.operation_deadline(total_deadline), field.chunk())
                .await
                .map_err(|_| upload_timeout_response())?
                .map_err(|_| {
                    upload_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_multipart",
                        Some("Die Formulardaten sind ungültig."),
                    )
                })?;
        let Some(chunk) = chunk else {
            break;
        };
        append_limited_metadata(&mut bytes, &chunk, max_bytes)?;
    }
    String::from_utf8(bytes).map_err(|_| {
        upload_error(
            StatusCode::BAD_REQUEST,
            "invalid_metadata",
            Some("Ein Textfeld enthält ungültige Zeichen."),
        )
    })
}

fn append_limited_metadata(
    target: &mut Vec<u8>,
    chunk: &[u8],
    max_bytes: usize,
) -> Result<(), Response> {
    if target.len().saturating_add(chunk.len()) > max_bytes {
        return Err(upload_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "metadata_too_large",
            Some("Ein Textfeld ist zu groß."),
        ));
    }
    target.extend_from_slice(chunk);
    Ok(())
}

async fn write_staged_chunk(
    staged: &mut StagedUpload,
    chunk: &[u8],
    max_bytes: usize,
) -> Result<(), Response> {
    if staged.length.saturating_add(chunk.len()) > max_bytes {
        return Err(upload_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "upload_too_large",
            Some("Die Datei ist zu groß."),
        ));
    }
    staged
        .file
        .write_all(chunk)
        .await
        .map_err(|_| upload_error(StatusCode::INTERNAL_SERVER_ERROR, "upload_failed", None))?;
    staged.length += chunk.len();
    Ok(())
}

async fn stage_multipart_file(
    mut field: Field<'_>,
    base_dir: &FsPath,
    total_deadline: tokio::time::Instant,
    timing: UploadTiming,
    max_bytes: usize,
) -> Result<StagedUpload, Response> {
    let mut staged = StagedUpload::create(base_dir).await?;
    loop {
        let chunk =
            match tokio::time::timeout_at(timing.operation_deadline(total_deadline), field.chunk())
                .await
            {
                Err(_) => {
                    staged.cleanup().await;
                    return Err(upload_timeout_response());
                }
                Ok(result) => match result {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        staged.cleanup().await;
                        return Err(upload_error(
                            StatusCode::BAD_REQUEST,
                            "invalid_multipart",
                            Some("Die Upload-Daten sind unvollständig."),
                        ));
                    }
                },
            };
        let Some(chunk) = chunk else {
            break;
        };
        if let Err(response) = write_staged_chunk(&mut staged, &chunk, max_bytes).await {
            staged.cleanup().await;
            return Err(response);
        }
    }
    if staged.length == 0 || staged.file.sync_all().await.is_err() {
        staged.cleanup().await;
        return Err(upload_error(
            StatusCode::BAD_REQUEST,
            "empty_upload",
            Some("Die Datei ist leer."),
        ));
    }
    Ok(staged)
}

#[cfg(target_os = "linux")]
fn opened_file_path(file: &tokio::fs::File) -> String {
    use std::os::fd::AsRawFd;
    format!("/proc/{}/fd/{}", std::process::id(), file.as_raw_fd())
}

#[cfg(not(target_os = "linux"))]
fn opened_file_path(_: &tokio::fs::File) -> String {
    String::new()
}

#[cfg(target_os = "linux")]
fn link_open_file_noreplace(
    source: &tokio::fs::File,
    destination: &SafeUploadDirectory,
    destination_name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    const AT_FDCWD: i32 = -100;
    const AT_SYMLINK_FOLLOW: i32 = 0x400;
    unsafe extern "C" {
        fn linkat(
            olddirfd: i32,
            oldpath: *const std::os::raw::c_char,
            newdirfd: i32,
            newpath: *const std::os::raw::c_char,
            flags: i32,
        ) -> i32;
    }

    let source_path = CString::new(opened_file_path(source))
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let destination_name = CString::new(destination_name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // /proc/self/fd/<n> verweist auf genau den gehaltenen, bereits geprüften
    // Inode. AT_SYMLINK_FOLLOW folgt nur diesem eigenen FD-Link; der Zielname
    // wird relativ zum gehaltenen no-follow-Verzeichnis und ohne Ersetzen
    // angelegt.
    let result = unsafe {
        linkat(
            AT_FDCWD,
            source_path.as_ptr(),
            destination.handle.as_raw_fd(),
            destination_name.as_ptr(),
            AT_SYMLINK_FOLLOW,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn link_open_file_noreplace(
    _: &tokio::fs::File,
    _: &SafeUploadDirectory,
    _: &std::ffi::OsStr,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Sichere Upload-Veröffentlichung wird auf diesem System nicht unterstützt",
    ))
}

async fn remove_owned_upload_link(
    directory: &SafeUploadDirectory,
    name: &std::ffi::OsStr,
    owned_metadata: &std::fs::Metadata,
) {
    let path = directory.child_path(name);
    let Ok(current) = tokio::fs::symlink_metadata(&path).await else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if current.file_type().is_file()
            && current.dev() == owned_metadata.dev()
            && current.ino() == owned_metadata.ino()
        {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
    #[cfg(not(unix))]
    {
        if current.file_type().is_file() && current.len() == owned_metadata.len() {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
}

/// Verarbeitet einen hochgeladenen Clip (Validierung + Speichern + Registrierung).
/// `base_dir` injizierbar (Tests). Liefert die 201-Antwort oder eine Fehler-Response.
#[cfg(test)]
async fn process_uploaded_clip(
    pool: &PgPool,
    base_dir: &str,
    streamer_raw: Option<&str>,
    _clip_id_raw: Option<&str>,
    title: Option<&str>,
    bytes: &[u8],
) -> Result<Value, Response> {
    if bytes.len() > UPLOAD_MAX_BYTES {
        return Err(upload_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "upload_too_large",
            Some("Die Datei ist zu groß."),
        ));
    }
    let mut staged = StagedUpload::create(FsPath::new(base_dir)).await?;
    if staged.file.write_all(bytes).await.is_err() || staged.file.sync_all().await.is_err() {
        staged.cleanup().await;
        return Err(upload_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "upload_failed",
            None,
        ));
    }
    staged.length = bytes.len();
    process_staged_uploaded_clip(pool, base_dir, streamer_raw, title, staged).await
}

async fn process_staged_uploaded_clip(
    pool: &PgPool,
    base_dir: &str,
    streamer_raw: Option<&str>,
    title: Option<&str>,
    mut staged: StagedUpload,
) -> Result<Value, Response> {
    let streamer_login = match slug_message(streamer_raw, "streamer_login") {
        Ok(login) if login.len() <= UPLOAD_MAX_SLUG_BYTES => login.to_lowercase(),
        Ok(_) => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::BAD_REQUEST,
                "invalid_streamer_login",
                Some("Der Kanalname ist zu lang."),
            ));
        }
        Err(message) => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::BAD_REQUEST,
                "invalid_streamer_login",
                Some(&message),
            ));
        }
    };
    if !ensure_streamer_exists(pool, &streamer_login).await {
        staged.cleanup().await;
        return Err(upload_error(
            StatusCode::NOT_FOUND,
            "unknown_streamer",
            None,
        ));
    }
    let clip_id = format!("manual:{}", uuid::Uuid::new_v4());
    if title.is_some_and(|value| value.len() > UPLOAD_MAX_TEXT_BYTES) {
        staged.cleanup().await;
        return Err(upload_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "metadata_too_large",
            Some("Der Titel ist zu lang."),
        ));
    }
    let owned_metadata = match validate_staged_upload_inode(&staged).await {
        Ok(metadata) => metadata,
        Err(response) => {
            staged.cleanup().await;
            return Err(response);
        }
    };

    if staged.file.seek(std::io::SeekFrom::Start(0)).await.is_err() {
        staged.cleanup().await;
        return Err(upload_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "upload_failed",
            None,
        ));
    }
    let mut header_bytes = [0u8; 20];
    let read = match staged.file.read(&mut header_bytes).await {
        Ok(read) => read,
        Err(_) => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "upload_failed",
                None,
            ));
        }
    };
    if !plausible_ftyp_box(&header_bytes[..read], staged.length as u64)
        || !detect_mp4_mime(&header_bytes[..read]).is_some_and(is_accepted_mp4_mime)
    {
        staged.cleanup().await;
        return Err(upload_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "mp4_required",
            Some("Es werden nur MP4-Videos unterstützt."),
        ));
    }

    let probe_path = opened_file_path(&staged.file);
    let info = VideoProcessor::new("/usr/bin/ffmpeg", "/usr/bin/ffprobe")
        .get_video_info(&probe_path)
        .await
        .map_err(|_| {
            upload_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "invalid_mp4",
                Some("Die Datei ist kein gültiges MP4-Video."),
            )
        });
    let duration = match info {
        Ok(info)
            if info.duration.is_finite()
                && info.duration > 0.0
                && info.duration <= UPLOAD_MAX_DURATION_SECONDS
                && info.fps.is_finite()
                && info.fps > 0.0
                && info.fps <= UPLOAD_MAX_FPS
                && info.width > 0
                && info.height > 0
                && info.width <= UPLOAD_MAX_WIDTH
                && info.height <= UPLOAD_MAX_HEIGHT
                && info.width.saturating_mul(info.height)
                    <= UPLOAD_MAX_WIDTH.saturating_mul(UPLOAD_MAX_HEIGHT) =>
        {
            info.duration
        }
        Ok(info) if !info.duration.is_finite() || info.duration <= 0.0 => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::BAD_REQUEST,
                "invalid_duration",
                Some("Das Video muss eine positive Laufzeit haben."),
            ));
        }
        Ok(info) if info.duration > UPLOAD_MAX_DURATION_SECONDS => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::BAD_REQUEST,
                "video_too_long",
                Some("Das Video darf höchstens 300 Sekunden lang sein."),
            ));
        }
        Ok(info) if !info.fps.is_finite() || info.fps <= 0.0 || info.fps > UPLOAD_MAX_FPS => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::BAD_REQUEST,
                "invalid_frame_rate",
                Some("Die Bildrate des Videos wird nicht unterstützt."),
            ));
        }
        Ok(_) => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::BAD_REQUEST,
                "invalid_video_dimensions",
                Some("Die Videoauflösung wird nicht unterstützt."),
            ));
        }
        Err(response) => {
            staged.cleanup().await;
            return Err(response);
        }
    };

    let upload_directory = match open_or_create_upload_streamer_directory(
        FsPath::new(base_dir),
        &streamer_login,
    )
    .await
    {
        Ok(directory) => directory,
        Err(_) => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "upload_failed",
                None,
            ));
        }
    };
    let final_name = format!("{clip_id}.mp4");

    let final_path = upload_directory
        .logical_path
        .join(&final_name)
        .to_string_lossy()
        .into_owned();

    // Der kurze DB-Abschnitt reserviert die Clip-ID, bevor derselbe geprüfte
    // Inode no-clobber veröffentlicht wird. Der Commit folgt direkt danach;
    // ffprobe und das Einlesen liegen ausdrücklich außerhalb dieser TX.
    let mut transaction = match pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => {
            staged.cleanup().await;
            return Err(upload_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_failed",
                None,
            ));
        }
    };
    let registration = register_manual_upload_in_transaction(
        transaction.as_mut(),
        &clip_id,
        &streamer_login,
        title,
        &final_path,
        duration,
    )
    .await;
    let (clip_db_id, retention_until) = match registration {
        Ok(result) => result,
        Err(ManualUploadError::AlreadyExists) => {
            let _ = transaction.rollback().await;
            remove_owned_upload_link(
                &staged.work_directory,
                std::ffi::OsStr::new(&staged.temp_name),
                &owned_metadata,
            )
            .await;
            return Err(upload_error(
                StatusCode::CONFLICT,
                "duplicate_clip_id",
                None,
            ));
        }
        Err(ManualUploadError::UnknownStreamer) => {
            let _ = transaction.rollback().await;
            remove_owned_upload_link(
                &staged.work_directory,
                std::ffi::OsStr::new(&staged.temp_name),
                &owned_metadata,
            )
            .await;
            return Err(upload_error(
                StatusCode::NOT_FOUND,
                "unknown_streamer",
                None,
            ));
        }
        Err(ManualUploadError::Db(_)) => {
            let _ = transaction.rollback().await;
            remove_owned_upload_link(
                &staged.work_directory,
                std::ffi::OsStr::new(&staged.temp_name),
                &owned_metadata,
            )
            .await;
            return Err(upload_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_failed",
                None,
            ));
        }
    };

    match link_open_file_noreplace(
        &staged.file,
        &upload_directory,
        std::ffi::OsStr::new(&final_name),
    ) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = transaction.rollback().await;
            remove_owned_upload_link(
                &staged.work_directory,
                std::ffi::OsStr::new(&staged.temp_name),
                &owned_metadata,
            )
            .await;
            return Err(upload_error(
                StatusCode::CONFLICT,
                "duplicate_clip_id",
                None,
            ));
        }
        Err(_) => {
            let _ = transaction.rollback().await;
            remove_owned_upload_link(
                &staged.work_directory,
                std::ffi::OsStr::new(&staged.temp_name),
                &owned_metadata,
            )
            .await;
            return Err(upload_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "upload_failed",
                None,
            ));
        }
    }

    if let Err(error) = transaction.commit().await {
        // Ein Commit-Fehler kann nach dem Senden an PostgreSQL unklar sein.
        // Deshalb hier weder Final-Link noch Temp-Inode löschen: So bleibt das
        // Artefakt sichtbar und ein Gewinner wird niemals versehentlich
        // entfernt. Retention/Operator können den Orphan später abgleichen.
        tracing::error!(clip_db_id, code = "upload_commit_uncertain", error = %error, "Manueller Upload: Commit-Ausgang ist unklar");
        return Err(upload_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "upload_commit_uncertain",
            None,
        ));
    }
    remove_owned_upload_link(
        &staged.work_directory,
        std::ffi::OsStr::new(&staged.temp_name),
        &owned_metadata,
    )
    .await;
    if let Err(error) = apply_default_layout(pool, clip_db_id, &streamer_login).await {
        tracing::warn!(clip_db_id, code = "layout_inheritance_failed", error = %error, "Manueller Upload: Layout-Vererbung konnte nicht bestätigt werden");
    }
    Ok(json!({
        "clip_db_id": clip_db_id,
        "clip_id": clip_id,
        "retention_until": retention_until,
    }))
}

/// `POST /social-media/api/clips/upload` — Multipart-Datei-Upload (Admin).
pub async fn upload_clip_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    multipart: Multipart,
) -> Response {
    upload_clip_handler_inner(
        auth,
        pool,
        multipart,
        "data/clips/uploads",
        manual_upload_semaphore(),
        UploadTiming::production(),
        UploadLimits::production(),
    )
    .await
}

async fn upload_clip_handler_inner(
    auth: DashboardAuthLevel,
    pool: PgPool,
    mut multipart: Multipart,
    base_dir: &str,
    semaphore: &Semaphore,
    timing: UploadTiming,
    limits: UploadLimits,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let _permit = match semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return upload_error(
                StatusCode::TOO_MANY_REQUESTS,
                "upload_busy",
                Some("Es wird bereits ein Video verarbeitet. Bitte versuche es gleich erneut."),
            );
        }
    };
    let total_deadline = tokio::time::Instant::now() + timing.wallclock;
    let mut staged: Option<StagedUpload> = None;
    let mut streamer_login: Option<String> = None;
    let mut title: Option<String> = None;
    let mut seen_fields = HashSet::new();
    loop {
        let field = match tokio::time::timeout_at(
            timing.operation_deadline(total_deadline),
            multipart.next_field(),
        )
        .await
        {
            Err(_) => {
                if let Some(upload) = staged.take() {
                    upload.cleanup().await;
                }
                return upload_timeout_response();
            }
            Ok(result) => match result {
                Ok(Some(field)) => field,
                Ok(None) => break,
                Err(_) => {
                    if let Some(upload) = staged.take() {
                        upload.cleanup().await;
                    }
                    return upload_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_multipart",
                        Some("Die Formulardaten sind ungültig."),
                    );
                }
            },
        };
        let Some(name) = field.name().map(str::to_string) else {
            if let Some(upload) = staged.take() {
                upload.cleanup().await;
            }
            return upload_error(StatusCode::BAD_REQUEST, "invalid_multipart", None);
        };
        if !seen_fields.insert(name.clone()) {
            if let Some(upload) = staged.take() {
                upload.cleanup().await;
            }
            return upload_error(
                StatusCode::BAD_REQUEST,
                "duplicate_field",
                Some("Ein Formularfeld wurde mehrfach gesendet."),
            );
        }
        let field_result = match name.as_str() {
            "file" => {
                match stage_multipart_file(
                    field,
                    FsPath::new(base_dir),
                    total_deadline,
                    timing,
                    limits.file_bytes,
                )
                .await
                {
                    Ok(upload) => {
                        staged = Some(upload);
                        Ok(())
                    }
                    Err(response) => Err(response),
                }
            }
            "streamer_login" => {
                read_limited_multipart_text(field, limits.slug_bytes, total_deadline, timing)
                    .await
                    .map(|value| streamer_login = Some(value))
            }
            // Client-IDs werden bewusst nur begrenzt eingelesen und verworfen.
            // Jede API-Anlage erhält serverseitig einen eigenen manual:-Namensraum.
            "clip_id" => {
                read_limited_multipart_text(field, limits.slug_bytes, total_deadline, timing)
                    .await
                    .map(|_| ())
            }
            "title" => {
                read_limited_multipart_text(field, limits.text_bytes, total_deadline, timing)
                    .await
                    .map(|value| title = Some(value))
            }
            _ => Err(upload_error(
                StatusCode::BAD_REQUEST,
                "unknown_field",
                Some("Das Formular enthält ein unbekanntes Feld."),
            )),
        };
        if let Err(response) = field_result {
            if let Some(upload) = staged.take() {
                upload.cleanup().await;
            }
            return response;
        }
    }
    let Some(staged) = staged else {
        return upload_error(
            StatusCode::BAD_REQUEST,
            "file_required",
            Some("Eine MP4-Datei fehlt."),
        );
    };
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    // Partner laden nur in den eigenen Kanal hoch, egal was im Formular steht.
    let streamer_login = match scope {
        Some(own) => Some(own),
        None => streamer_login,
    };
    // Partner-Freigabe-Guard: nach Streamer-Auflösung, vor Wirkung.
    if let Some(ref login) = streamer_login {
        if let Some(guard_response) = check_partner_access_guard(&pool, &auth, login).await {
            staged.cleanup().await;
            return guard_response;
        }
    }
    // Clip-Kontingent der Stufe: ein Upload erzeugt einen neuen Clip.
    if let Err(resp) =
        clip_kontingent_guard(&pool, &auth, streamer_login.as_deref().unwrap_or("")).await
    {
        staged.cleanup().await;
        return resp;
    }
    match process_staged_uploaded_clip(
        &pool,
        base_dir,
        streamer_login.as_deref(),
        title.as_deref(),
        staged,
    )
    .await
    {
        Ok(payload) => (StatusCode::CREATED, Json(payload)).into_response(),
        Err(resp) => resp,
    }
}

/// `?streamer_login=` für die Layout-GET-Route und den Zeitplan.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamerLoginQuery {
    pub streamer_login: Option<String>,
}

/// `GET /social-media/api/admin/streamer-layout` — Default-Layout eines Streamers (Admin).
pub async fn streamer_layout_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerLoginQuery>,
) -> Response {
    if let Err(e) = require_sm_access(&auth, &pool, q.streamer_login.as_deref()).await {
        return e;
    }
    let slug = match normalize_safe_slug(q.streamer_login.as_deref(), "streamer_login") {
        Ok(s) => s.to_lowercase(),
        Err(e) => return e,
    };
    if !ensure_streamer_exists(&pool, &slug).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown_streamer" })),
        )
            .into_response();
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
pub async fn streamer_layout_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(e) = require_sm_access(
        &auth,
        &pool,
        payload.get("streamer_login").and_then(Value::as_str),
    )
    .await
    {
        return e;
    }
    let slug = match normalize_safe_slug(
        payload.get("streamer_login").and_then(Value::as_str),
        "streamer_login",
    ) {
        Ok(s) => s.to_lowercase(),
        Err(e) => return e,
    };
    if !ensure_streamer_exists(&pool, &slug).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown_streamer" })),
        )
            .into_response();
    }
    // Partner-Freigabe-Guard: nach Streamer-Auflösung, vor Wirkung.
    if let Some(guard_response) = check_partner_access_guard(&pool, &auth, &slug).await {
        return guard_response;
    }
    let layout = match parse_layout_request(&payload) {
        Ok(l) => l,
        Err(e) => return e,
    };
    // updated_by (B15-FIX): Session-Actor (Partner→twitch_user_id, sonst NULL).
    let actor = editor_user_id(&auth);
    let outcome = match upsert_streamer_layout(&pool, &slug, &layout, actor.as_deref()).await {
        Ok(outcome) => outcome,
        Err(error) => return layout_mutation_error_response(error),
    };
    Json(json!({
        "streamer_login": slug,
        "layout": layout.to_override_json(),
        "cam_enabled": layout.cam_enabled,
        "mode": layout.mode,
        "changed": outcome.changed,
        "pending_stopped": outcome.pending_stopped,
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
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(clip_db_id_raw))) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_clip_db_id" })),
        )
            .into_response();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    if !clip_exists(&pool, clip_db_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "clip_not_found" })),
        )
            .into_response();
    }

    // Partner-Freigabe-Guard: nach Clip-Auflösung, vor Wirkung.
    if let Some(guard_response) = guard_partner_access_for_clip(&pool, &auth, clip_db_id).await {
        return guard_response;
    }

    // `layout` fehlt/null → Override löschen.
    let Some(layout_payload) = payload.get("layout").filter(|v| !v.is_null()) else {
        let outcome = match set_clip_layout_override(&pool, clip_db_id, None).await {
            Ok(outcome) => outcome,
            Err(error) => return layout_mutation_error_response(error),
        };
        let effective = get_clip_effective_layout(&pool, clip_db_id).await;
        return Json(json!({
            "clip_db_id": clip_db_id,
            "layout_override": Value::Null,
            "effective_layout": effective.to_override_json(),
            "changed": outcome.changed,
            "pending_stopped": outcome.pending_stopped,
        }))
        .into_response();
    };
    let layout = match StreamerLayout::from_value(layout_payload, None, None) {
        Ok(l) => l,
        Err(e) => return invalid_layout(e.to_string()),
    };
    let outcome = match set_clip_layout_override(&pool, clip_db_id, Some(&layout)).await {
        Ok(outcome) => outcome,
        Err(error) => return layout_mutation_error_response(error),
    };
    let effective = get_clip_effective_layout(&pool, clip_db_id).await;
    Json(json!({
        "clip_db_id": clip_db_id,
        "layout_override": layout.to_override_json(),
        "effective_layout": effective.to_override_json(),
        "changed": outcome.changed,
        "pending_stopped": outcome.pending_stopped,
    }))
    .into_response()
}

fn layout_mutation_error_response(error: LayoutMutationError) -> Response {
    match error {
        LayoutMutationError::UploadRunning(already_running) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "upload_already_running",
                "already_running": already_running,
            })),
        )
            .into_response(),
        LayoutMutationError::PreparationRunning => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "preparation_running" })),
        )
            .into_response(),
        LayoutMutationError::Db(error) => {
            tracing::error!(%error, "Layoutänderung konnte nicht gespeichert werden");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "save_failed" })),
            )
                .into_response()
        }
    }
}

/// POST-Body von `…/api/upload` (Queue-Upload). `platforms` ist Array ODER
/// der String `"all"`; `schedule` und `forms` sind optional.
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
    pub schedule: Option<String>,
    #[serde(default)]
    pub forms: Vec<FormKey>,
}

fn scheduled_at_value(
    schedule: Option<&str>,
    now: DateTime<Utc>,
    already_taken: &[DateTime<Utc>],
    posting_schedule: &PostingSchedule,
) -> Result<Option<String>, &'static str> {
    match schedule {
        Some("now") => Ok(Some(now.to_rfc3339())),
        Some("auto") => Ok(Some(
            next_free_slot(now, already_taken, posting_schedule).to_rfc3339(),
        )),
        Some(timestamp) if DateTime::parse_from_rfc3339(timestamp).is_ok() => {
            Ok(Some(timestamp.to_string()))
        }
        None => Ok(None),
        _ => Err("invalid_schedule"),
    }
}

fn form_submission_response(form: FormKey, outcome: FormSubmissionOutcome) -> Value {
    let (status, http_status) = match outcome {
        FormSubmissionOutcome::Submitted(status) => ("submitted", Some(status)),
        FormSubmissionOutcome::Failed(status) => ("failed", status),
        FormSubmissionOutcome::SkippedDisabled => ("skipped_disabled", None),
        FormSubmissionOutcome::SkippedDuplicate => ("skipped_duplicate", None),
    };
    json!({
        "form": form.as_str(),
        "status": status,
        "http_status": http_status,
    })
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
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "clip_id required" })),
        )
            .into_response();
    };
    let requested = body.streamer.as_deref().or(q.streamer.as_deref());
    let scope = match resolve_streamer_scope(&auth, requested, false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung.
    if let Some(streamer) = &scope {
        if let Some(guard_response) = check_partner_access_guard(&pool, &auth, streamer).await {
            return guard_response;
        }
    }
    if let Some(streamer) = &scope {
        if !clip_owned_by_streamer(&pool, clip_id, streamer).await {
            return (
                StatusCode::FORBIDDEN,
                Json(
                    json!({ "error": "forbidden: clip does not belong to authenticated streamer" }),
                ),
            )
                .into_response();
        }
    }
    let form_clip_id = if body.forms.is_empty() {
        None
    } else {
        match i32::try_from(clip_id) {
            Ok(id) => Some(id),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "invalid_clip_id" })),
                )
                    .into_response();
            }
        }
    };
    // platforms: Array oder "all".
    let platforms: Vec<String> = match &body.platforms {
        Value::String(s) if s == "all" => ["tiktok", "youtube", "instagram"]
            .iter()
            .map(|p| p.to_string())
            .collect(),
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    };
    // Automatisches Posten auf TikTok, Instagram und YouTube ist Creator Pro.
    // Die Sperre haengt am Einreihen selbst, nicht am `schedule`-Parameter:
    // vorher griff sie nur bei `schedule="auto"`, ein Aufruf ohne das Feld
    // reihte dieselben Uploads fuer jeden authentifizierten Partner ein.
    // Ohne Plattformen wird nichts eingereiht (reiner Formular-Pfad), dann
    // greift die Sperre nicht.
    if !platforms.is_empty() {
        if let Some(resp) = auto_posting_guard(&pool, &auth).await {
            return resp;
        }
    }
    let (already_taken, posting_schedule) = if body.schedule.as_deref() == Some("auto") {
        let taken = match sqlx::query_scalar::<_, DateTime<Utc>>(
            "SELECT scheduled_at FROM twitch_clips_upload_queue \
             WHERE status = 'pending' AND scheduled_at IS NOT NULL",
        )
        .fetch_all(&pool)
        .await
        {
            Ok(taken) => taken,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "schedule_failed" })),
                )
                    .into_response();
            }
        };
        (taken, get_posting_schedule(&pool).await)
    } else {
        (Vec::new(), PostingSchedule::default())
    };
    let scheduled_at = match scheduled_at_value(
        body.schedule.as_deref(),
        Utc::now(),
        &already_taken,
        &posting_schedule,
    ) {
        Ok(value) => value,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
        }
    };
    let mut queued: Vec<Value> = Vec::new();
    for platform in &platforms {
        match queue_upload(
            &pool,
            clip_id,
            platform,
            body.title.as_deref(),
            body.description.as_deref(),
            body.hashtags.as_deref(),
            scheduled_at.as_deref(),
            body.priority,
        )
        .await
        {
            Ok(queue_id) => queued.push(json!({ "platform": platform, "queue_id": queue_id })),
            Err(_) => queued.push(json!({ "platform": platform, "error": "queue_failed" })),
        }
    }
    let mut forms = Vec::new();
    if let Some(form_clip_id) = form_clip_id {
        let client = reqwest::Client::new();
        for form in body.forms {
            let outcome =
                match submit_clip_form(&pool, &client, form_clip_id, form, body.title.as_deref())
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        tracing::error!(
                            clip_id = form_clip_id,
                            form_key = form.as_str(),
                            http_status = ?Option::<u16>::None,
                            outcome = "failed",
                            %error,
                            "Formular-Submit fehlgeschlagen"
                        );
                        FormSubmissionOutcome::Failed(None)
                    }
                };
            forms.push(form_submission_response(form, outcome));
        }
    }
    Json(json!({ "queued": queued, "forms": forms })).into_response()
}

/// Baut den Clip-Fetcher inline aus den Twitch-App-Credentials (`None`, wenn
/// nicht konfiguriert → 503).
/// Diesen Fetch hat der Streamer selbst ausgeloest, also zaehlen die dabei neu
/// aufgenommenen Clips gegen sein Monatskontingent. Der Hintergrund-Fetcher
/// baut sich seinen Service ueber `build_clip_fetch_task` und bucht nichts.
fn build_clip_fetch_service(pool: PgPool, limit: u32) -> Option<ClipFetchService> {
    let client_id = std::env::var("TWITCH_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    let client_secret = std::env::var("TWITCH_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())?;
    let helix = HelixClient::new(HelixConfig::new(client_id, client_secret)).ok()?;
    let source = HelixClipSource::new(Arc::new(helix));
    Some(
        ClipFetchService::new(ClipRepository::new(pool), source)
            .with_clip_limit(limit)
            .mit_kontingent_verbrauch(true),
    )
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
pub async fn fetch_clips_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<FetchClipsBody>,
) -> Response {
    if let Err(e) = require_auth(&auth) {
        return e;
    }
    // required=true: Admin muss streamer angeben.
    let scope = match resolve_streamer_scope(&auth, body.streamer.as_deref(), true) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let streamer = scope.unwrap_or_default();
    let mut limit = body.limit.filter(|n| *n > 0).unwrap_or(20).clamp(1, 100) as u32;

    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung.
    if let Some(guard_response) = check_partner_access_guard(&pool, &auth, &streamer).await {
        return guard_response;
    }
    // Clip-Kontingent der Stufe: der Fetch legt neue Clips an, deshalb wird die
    // Menge auf den Rest des Monats geklemmt statt die Anfrage abzulehnen.
    let kontingent = match clip_kontingent_guard(&pool, &auth, &streamer).await {
        Ok(k) => k,
        Err(resp) => return resp,
    };
    if let Some(rest) = kontingent.rest() {
        limit = limit.min(rest.max(0) as u32);
    }

    let Some(service) = build_clip_fetch_service(pool.clone(), limit) else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "twitch_api_unavailable", "message": "Clip-Fetch ist derzeit nicht verfügbar." }))).into_response();
    };
    let result = service.fetch_for_streamer(&streamer).await;
    let clips_found = result.clips_found.max(0);
    // Nach dem Fetch neu zaehlen: der Stand von vorhin ist bereits verbraucht,
    // und das Dashboard soll nicht die alte Zahl anzeigen.
    let kontingent = tb_analytics::stufe::clip_kontingent(&pool, kontingent.stufe, &streamer).await;
    Json(json!({
        "success": true,
        "clips_found": clips_found,
        "message": format!("Fetched {clips_found} clips"),
        "kontingent": kontingent.als_json(),
    }))
    .into_response()
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
pub async fn batch_upload_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(qs): Query<StreamerQuery>,
    Json(body): Json<BatchUploadBody>,
) -> Response {
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
    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung.
    if let Some(guard_response) = check_partner_access_guard(&pool, &auth, &streamer).await {
        return guard_response;
    }
    // Sammel-Einreihung aller neuen Clips ist automatisches Posten → Creator Pro.
    if let Some(resp) = auto_posting_guard(&pool, &auth).await {
        return resp;
    }
    let platforms: Vec<String> = body
        .platforms
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if platforms.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "platforms are required" })),
        )
            .into_response();
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
    pub reconciliations: Vec<MarkUploadedReconciliation>,
    pub streamer: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MarkUploadedReconciliation {
    pub reconciliation_id: String,
    pub platform: String,
    pub provider_started_at: DateTime<Utc>,
    pub provider_external_id: Option<String>,
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
    if !matches!(&auth, DashboardAuthLevel::Admin { .. }) {
        return forbidden("Nur Admins dürfen unklare Provider-Versuche bestätigen.");
    }
    let clip_id = normalize_id(body.clip_id.as_ref());
    let (Some(clip_id), false) = (clip_id, body.reconciliations.is_empty()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "reconciliations_required" })),
        )
            .into_response();
    };
    let attempts: Vec<ManualReconciliationRequest> = match body
        .reconciliations
        .into_iter()
        .map(|attempt| {
            let queue_id = attempt
                .reconciliation_id
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or(())?;
            Ok(ManualReconciliationRequest {
                queue_id,
                platform: attempt.platform,
                provider_started_at: attempt.provider_started_at,
                provider_external_id: attempt.provider_external_id,
            })
        })
        .collect::<Result<Vec<_>, ()>>()
    {
        Ok(attempts) => attempts,
        Err(()) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_reconciliation_id" })),
            )
                .into_response();
        }
    };
    let requested = body.streamer.as_deref().or(q.streamer.as_deref());
    let scope = match resolve_streamer_scope(&auth, requested, false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung.
    if let Some(streamer) = &scope {
        if let Some(guard_response) = check_partner_access_guard(&pool, &auth, streamer).await {
            return guard_response;
        }
    }
    if let Some(streamer) = &scope {
        if !clip_owned_by_streamer(&pool, clip_id, streamer).await {
            return (
                StatusCode::FORBIDDEN,
                Json(
                    json!({ "error": "forbidden: clip does not belong to authenticated streamer" }),
                ),
            )
                .into_response();
        }
    }
    match reconcile_provider_uploads(&pool, clip_id, &attempts).await {
        Ok(outcome) => Json(json!({
            "ok": true,
            "message": "Clip manuell als veröffentlicht bestätigt",
            "reconciled_platforms": outcome.reconciled_platforms,
        }))
        .into_response(),
        Err(ManualReconciliationError::UnsupportedPlatform) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unsupported_platform" })),
        )
            .into_response(),
        Err(ManualReconciliationError::ClipNotFound) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "clip_not_found" })),
        )
            .into_response(),
        Err(ManualReconciliationError::NotReconciliable) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "reconciliation_required_not_found" })),
        )
            .into_response(),
        Err(ManualReconciliationError::Db(error)) => {
            tracing::error!(%error, clip_db_id = clip_id, "Provider-Abgleich konnte nicht gespeichert werden");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "database_failed" })),
            )
                .into_response()
        }
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
    value.and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
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
pub async fn vocab_list_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<VocabListQuery>,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let page = match q.page.as_deref().unwrap_or("1").parse::<i64>() {
        Ok(n) => n.max(1),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_pagination" })),
            )
                .into_response()
        }
    };
    let page_size = match q.page_size.as_deref().unwrap_or("50").parse::<i64>() {
        Ok(n) => n.clamp(1, 200),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_pagination" })),
            )
                .into_response()
        }
    };
    let category = q.category.as_deref().filter(|s| !s.is_empty());
    let query = q.q.as_deref().filter(|s| !s.is_empty());
    let offset = (page - 1) * page_size;
    let (entries, total) = list_vocab(&pool, category, query, page_size, offset).await;
    let items: Vec<Value> = entries.iter().map(vocab_entry_json).collect();
    Json(json!({ "items": items, "total": total, "page": page, "page_size": page_size }))
        .into_response()
}

/// `POST /social-media/api/admin/vocab` — Vokabel anlegen/aktualisieren (Admin).
pub async fn vocab_upsert_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    if !payload.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_payload" })),
        )
            .into_response();
    }
    let term = payload.get("term").and_then(Value::as_str).unwrap_or("");
    let canonical = payload
        .get("canonical")
        .and_then(Value::as_str)
        .unwrap_or("");
    let category = payload
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or("");
    let source = payload
        .get("source")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("manual");
    let aliases: Vec<String> = payload
        .get("aliases")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let weight = json_i64(payload.get("weight"))
        .filter(|n| *n != 0)
        .unwrap_or(1) as i32;

    match upsert_vocab_entry(&pool, term, canonical, category, source, &aliases, weight).await {
        Ok(entry) => Json(vocab_entry_json(&entry)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_vocab", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `DELETE /social-media/api/admin/vocab/:term` — Vokabel löschen (Admin).
pub async fn vocab_delete_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(term): Path<String>,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let term = term.trim();
    if term.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "term_required" })),
        )
            .into_response();
    }
    match delete_vocab_entry(&pool, term).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "vocab_not_found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_term", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `POST /social-media/api/admin/vocab/seed` — Vokabular seeden (Admin).
/// Body optional: `{include_slang, include_api}` (Default beide true).
pub async fn vocab_seed_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: String,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let (mut include_slang, mut include_api) = (true, true);
    if !body.trim().is_empty() {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&body) {
            include_slang = map
                .get("include_slang")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            include_api = map
                .get("include_api")
                .and_then(Value::as_bool)
                .unwrap_or(true);
        }
    }
    let (written, skipped) = seed_vocab(&pool, include_slang, include_api).await;
    // Legacy-Frontend nutzt {inserted, updated}.
    Json(json!({ "inserted": written, "updated": written, "written": written, "skipped": skipped }))
        .into_response()
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
        "provider_calls_enabled": s.provider_calls_enabled,
        "provider_release_blocked": s.platform == "tiktok" || !s.provider_calls_enabled,
        "release_block_reason": if s.platform == "tiktok" {
            Some("tiktok_consent_required")
        } else if !s.provider_calls_enabled {
            Some("platform_release_blocked")
        } else {
            None
        },
    })
}

/// `GET /social-media/api/platforms/status?streamer=<kanal>` — Verbindungsstatus
/// je Plattform.
///
/// Der Parameter ist Pflicht (Admin ohne `streamer` → 400). Für die
/// Sammelverbindung ausdrücklich [`GLOBAL_SCOPE_MARKER`] (`__global__`)
/// mitschicken; vorher zeigte ein fehlender Parameter still auf sie.
pub async fn platforms_status_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerQuery>,
) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), true) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(cred_mgr) = build_credential_manager(pool) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "platform_status_failed" })),
        )
            .into_response();
    };
    let db_scope = credential_scope(scope.as_deref());
    let has_scope = db_scope.is_some();
    let statuses = cred_mgr.get_all_platforms_status(db_scope).await;
    let platforms: Vec<Value> = statuses
        .iter()
        .map(|s| platform_status_json(s, has_scope))
        .collect();
    Json(json!({ "platforms": platforms })).into_response()
}

const CLIP_COLUMNS: &str = "id, clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login, \
    created_at::text AS created_at, duration_seconds, view_count, game_name, status, source_kind, upload_local_path, \
    retention_until::text AS retention_until, discarded_at::text AS discarded_at, \
    layout_override_json::text AS layout_override_json, uploaded_tiktok, uploaded_youtube, uploaded_instagram";

/// Geladene Clip-Zeile (für `_serialize_clip_record`).
struct ClipRow {
    id: i64,
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
    uploaded_tiktok: Option<bool>,
    uploaded_youtube: Option<bool>,
    uploaded_instagram: Option<bool>,
}

fn row_to_clip(r: &PgRow) -> Result<ClipRow, sqlx::Error> {
    Ok(ClipRow {
        id: r.try_get("id")?,
        clip_id: r.try_get("clip_id")?,
        clip_url: r.try_get("clip_url")?,
        clip_title: r.try_get("clip_title")?,
        clip_thumbnail_url: r.try_get("clip_thumbnail_url")?,
        streamer_login: r.try_get("streamer_login")?,
        created_at: r.try_get("created_at")?,
        duration_seconds: r.try_get("duration_seconds")?,
        view_count: r.try_get("view_count")?,
        game_name: r.try_get("game_name")?,
        status: r.try_get("status")?,
        source_kind: r.try_get("source_kind")?,
        upload_local_path: r.try_get("upload_local_path")?,
        retention_until: r.try_get("retention_until")?,
        discarded_at: r.try_get("discarded_at")?,
        layout_override_json: r.try_get("layout_override_json")?,
        uploaded_tiktok: r.try_get("uploaded_tiktok")?,
        uploaded_youtube: r.try_get("uploaded_youtube")?,
        uploaded_instagram: r.try_get("uploaded_instagram")?,
    })
}

async fn load_clip_row(pool: &PgPool, clip_db_id: i64) -> Result<Option<ClipRow>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {CLIP_COLUMNS} FROM twitch_clips_social_media WHERE id = $1 LIMIT 1"
    ))
    .bind(clip_db_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!(clip_db_id, error = %e, "failed to query social media clip row");
        e
    })?;
    row.as_ref().map(row_to_clip).transpose().map_err(|e| {
        tracing::error!(clip_db_id, error = %e, "failed to decode social media clip row");
        e
    })
}

/// Stand einer Plattform-Zeile in `twitch_clips_upload_queue`.
#[derive(Debug, Default, Clone)]
struct UploadQueueEntry {
    queue_id: i64,
    status: Option<String>,
    /// Geplanter Termin (`scheduled_at`), RFC-3339.
    scheduled_at: Option<String>,
    /// Letzter Fehlergrund (`last_error`) aus `update_upload_status`.
    last_error: Option<String>,
    provider_started_at: Option<String>,
    provider_external_id: Option<String>,
    provider_accepted_at: Option<String>,
}

/// Queue-Stand einer Clip-Seite: Clip-ID → Plattform → Zeile.
type UploadQueueInfo =
    std::collections::HashMap<i64, std::collections::HashMap<String, UploadQueueEntry>>;

/// Lädt Termin und Fehlergrund für eine ganze Clip-Seite in EINER Abfrage.
///
/// Bewusst nicht je Clip einzeln: bei `page_size=100` wären das sonst hundert
/// Rundreisen zur Datenbank. Je Clip und Plattform gewinnt die zuletzt angelegte
/// Zeile (höchste `id`), das ist der aktuelle Versuch.
async fn load_upload_queue_info(
    pool: &PgPool,
    clip_ids: &[i64],
) -> Result<UploadQueueInfo, sqlx::Error> {
    let mut info: UploadQueueInfo = std::collections::HashMap::new();
    if clip_ids.is_empty() {
        return Ok(info);
    }
    let rows = sqlx::query(
        "SELECT id, clip_id, platform, status, scheduled_at, last_error, \
                provider_started_at, provider_external_id, provider_accepted_at \
         FROM twitch_clips_upload_queue WHERE clip_id = ANY($1) ORDER BY id ASC",
    )
    .bind(clip_ids)
    .fetch_all(pool)
    .await?;
    for r in &rows {
        let (queue_id, clip_id, platform) = (
            r.try_get::<i64, _>("id")?,
            r.try_get::<i64, _>("clip_id"),
            r.try_get::<String, _>("platform"),
        );
        let clip_id = clip_id?;
        let platform = platform?;
        let scheduled_at = r
            .try_get::<Option<DateTime<Utc>>, _>("scheduled_at")?
            .map(|ts| ts.to_rfc3339());
        let status = r
            .try_get::<Option<String>, _>("status")?
            .filter(|value| !value.trim().is_empty());
        let last_error = r
            .try_get::<Option<String>, _>("last_error")?
            .filter(|s| !s.trim().is_empty())
            .map(|value| safe_upload_error_code(&value).to_string());
        let provider_started_at = r
            .try_get::<Option<DateTime<Utc>>, _>("provider_started_at")?
            .map(|ts| ts.to_rfc3339());
        let provider_external_id = r
            .try_get::<Option<String>, _>("provider_external_id")?
            .filter(|value| !value.trim().is_empty());
        let provider_accepted_at = r
            .try_get::<Option<DateTime<Utc>>, _>("provider_accepted_at")?
            .map(|ts| ts.to_rfc3339());
        info.entry(clip_id).or_default().insert(
            platform.trim().to_lowercase(),
            UploadQueueEntry {
                queue_id,
                status,
                scheduled_at,
                last_error,
                provider_started_at,
                provider_external_id,
                provider_accepted_at,
            },
        );
    }
    Ok(info)
}

fn safe_upload_error_code(raw: &str) -> &'static str {
    match raw.trim() {
        "approval_required" => "approval_required",
        "approval_check_failed" => "approval_check_failed",
        "approval_or_preview_changed" => "approval_or_preview_changed",
        "approval_changed" => "approval_changed",
        "approval_skipped" => "approval_skipped",
        "release_disabled" => "release_disabled",
        "release_gate_failed" => "release_gate_failed",
        "clip_discarded" => "clip_discarded",
        "clip_not_found" => "clip_not_found",
        "content_changed" => "content_changed",
        "layout_changed" => "layout_changed",
        "enrichment_changed" => "enrichment_changed",
        "credentials_missing" => "credentials_missing",
        "streamer_missing" => "streamer_missing",
        "uploaded_flag_check_failed" => "uploaded_flag_check_failed",
        "completed_write_failed" => "completed_write_failed",
        "video_validation_failed" => "video_validation_failed",
        "preparation_busy" => "preparation_busy",
        "preparation_transient" => "preparation_transient",
        "source_missing" => "source_missing",
        "invalid_source_url" => "invalid_source_url",
        "download_failed" => "download_failed",
        "download_isolation_required" => "download_isolation_required",
        "render_failed" => "render_failed",
        "layout_invalid" => "layout_invalid",
        "platform_paused" => "platform_paused",
        "platform_release_blocked" => "platform_release_blocked",
        "tiktok_consent_required" => "tiktok_consent_required",
        "provider_attempt_exists" => "provider_attempt_exists",
        "provider_rejected" => "provider_rejected",
        "provider_result_uncertain" => "provider_result_uncertain",
        "provider_result_unknown_after_restart" => "provider_result_unknown_after_restart",
        "legacy_processing_requires_reconciliation" => "legacy_processing_requires_reconciliation",
        "provider_acceptance_write_uncertain" => "provider_acceptance_write_uncertain",
        "completed_write_uncertain" => "completed_write_uncertain",
        "tiktok_publish_uncertain" => "tiktok_publish_uncertain",
        "manually_reconciled" => "manually_reconciled",
        _ => "upload_failed",
    }
}

fn upload_states_value(
    entries: Option<&std::collections::HashMap<String, UploadQueueEntry>>,
) -> Value {
    let mut out = serde_json::Map::with_capacity(PLATFORMS.len());
    for platform in PLATFORMS {
        let entry = entries.and_then(|values| values.get(platform));
        out.insert(
            platform.to_string(),
            json!({
                "reconciliation_id": entry
                    .filter(|value| value.status.as_deref() == Some("reconciliation_required"))
                    .map(|value| value.queue_id.to_string()),
                "status": entry.and_then(|value| value.status.as_deref()),
                "error_code": entry
                    .and_then(|value| value.last_error.as_deref())
                    .map(safe_upload_error_code),
                "provider_started_at": entry.and_then(|value| value.provider_started_at.as_deref()),
                "provider_external_id": entry.and_then(|value| value.provider_external_id.as_deref()),
                "provider_accepted_at": entry.and_then(|value| value.provider_accepted_at.as_deref()),
                "reconciliation_required": entry
                    .and_then(|value| value.status.as_deref())
                    == Some("reconciliation_required"),
            }),
        );
    }
    Value::Object(out)
}

/// Objekt mit einem Schlüssel je Plattform (`youtube`/`tiktok`/`instagram`);
/// ohne Eintrag steht dort `null`.
fn platform_value_map<'a>(
    entries: Option<&'a std::collections::HashMap<String, UploadQueueEntry>>,
    pick: impl Fn(&'a UploadQueueEntry) -> Option<&'a str>,
) -> Value {
    let mut out = serde_json::Map::with_capacity(PLATFORMS.len());
    for platform in PLATFORMS {
        let value = entries
            .and_then(|m| m.get(platform))
            .and_then(&pick)
            .map(|s| Value::String(s.to_string()))
            .unwrap_or(Value::Null);
        out.insert(platform.to_string(), value);
    }
    Value::Object(out)
}

/// Baut den Clip-Detail-JSON inkl. Effective-Layout, Enrichment-Summary und
/// Approval (Python `_serialize_clip_record`).
///
/// Für Einzelclips; die Listen-Endpoints holen den Queue-Stand einmal für die
/// ganze Seite und rufen [`serialize_clip_record_with`] direkt auf.
async fn serialize_clip_record(pool: &PgPool, row: &ClipRow) -> Result<Value, sqlx::Error> {
    let queue = load_upload_queue_info(pool, &[row.id]).await?;
    Ok(serialize_clip_record_with(pool, row, queue.get(&row.id)).await)
}

/// Wie [`serialize_clip_record`], nur mit schon geladenem Queue-Stand.
async fn serialize_clip_record_with(
    pool: &PgPool,
    row: &ClipRow,
    queue: Option<&std::collections::HashMap<String, UploadQueueEntry>>,
) -> Value {
    let layout_override = row
        .layout_override_json
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null);
    let effective_layout = get_clip_effective_layout(pool, row.id)
        .await
        .to_override_json();

    let child_clip_db_id = match i32::try_from(row.id) {
        Ok(id) => Some(id),
        Err(_) => {
            tracing::warn!(
                clip_db_id = row.id,
                "clip id exceeds int4-backed social media child tables; enrichment and approval omitted"
            );
            None
        }
    };

    let (enrichment_status, enrichment_summary) = match child_clip_db_id {
        Some(id) => match get_enrichment(pool, id).await {
            Some(e) => {
                // Dedup youtube→tiktok→instagram, erste 5.
                let mut seen = std::collections::HashSet::new();
                let mut top: Vec<String> = Vec::new();
                for tag in e
                    .hashtags_youtube
                    .iter()
                    .chain(&e.hashtags_tiktok)
                    .chain(&e.hashtags_instagram)
                {
                    if seen.insert(tag.clone()) {
                        top.push(tag.clone());
                        if top.len() == 5 {
                            break;
                        }
                    }
                }
                (
                    json!(e.status),
                    json!({ "top_hashtags": top, "provider": e.llm_provider }),
                )
            }
            None => (Value::Null, Value::Null),
        },
        None => (Value::Null, Value::Null),
    };
    let approval = match child_clip_db_id {
        Some(id) => match get_approval_record(pool, id).await {
            Some(rec) => serialize_approval_record(&rec),
            None => Value::Null,
        },
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
            "tiktok": row.uploaded_tiktok.unwrap_or(false),
            "youtube": row.uploaded_youtube.unwrap_or(false),
            "instagram": row.uploaded_instagram.unwrap_or(false),
        },
        "layout_override": layout_override,
        "effective_layout": effective_layout,
        "enrichment_status": enrichment_status,
        "enrichment_summary": enrichment_summary,
        "approval": approval,
        // Geplanter Termin je Plattform (RFC 3339) und Fehlergrund je
        // Plattform. Ohne Queue-Zeile steht dort `null`. Ohne diese beiden
        // Felder zeigte das Dashboard nur ein nacktes „Fehler" ohne Grund und
        // in den Zeitplan-Modi keinen Termin.
        "scheduled_at": platform_value_map(queue, |e| e.scheduled_at.as_deref()),
        "upload_errors": platform_value_map(queue, |e| e.last_error.as_deref()),
        "upload_states": upload_states_value(queue),
    })
}

fn child_clip_db_id_or_500(clip_db_id: i64, operation: &str) -> Result<i32, Response> {
    i32::try_from(clip_db_id).map_err(|_| {
        tracing::error!(
            clip_db_id,
            operation,
            "clip id exceeds int4-backed social media child table"
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "clip_id_out_of_range" })),
        )
            .into_response()
    })
}

fn clip_load_failed() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "clip_load_failed" })),
    )
        .into_response()
}

async fn require_clip_row(pool: &PgPool, clip_db_id: i64) -> Result<ClipRow, Response> {
    match load_clip_row(pool, clip_db_id).await {
        Ok(Some(clip)) => Ok(clip),
        Ok(None) => Err(clip_not_found()),
        Err(_) => Err(clip_load_failed()),
    }
}

async fn require_clip_child_id(
    pool: &PgPool,
    clip_db_id: i64,
    operation: &str,
) -> Result<i32, Response> {
    require_clip_row(pool, clip_db_id).await?;
    child_clip_db_id_or_500(clip_db_id, operation)
}

async fn optional_approval_json(pool: &PgPool, clip_db_id: i64) -> Value {
    let Ok(child_id) = i32::try_from(clip_db_id) else {
        tracing::warn!(
            clip_db_id,
            "clip id exceeds int4-backed approval table; approval omitted"
        );
        return Value::Null;
    };
    match get_approval_record(pool, child_id).await {
        Some(rec) => serialize_approval_record(&rec),
        None => Value::Null,
    }
}

fn invalid_clip_db_id() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid_clip_db_id" })),
    )
        .into_response()
}

fn clip_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "clip_not_found" })),
    )
        .into_response()
}

fn invalid_pagination() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid_pagination" })),
    )
        .into_response()
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
pub async fn admin_clips_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<AdminClipsQuery>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, q.streamer.as_deref()).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let page = match q.page.as_deref().unwrap_or("1").parse::<i64>() {
        Ok(n) => n.max(1),
        Err(_) => return invalid_pagination(),
    };
    let page_size = match q.page_size.as_deref().unwrap_or("20").parse::<i64>() {
        Ok(n) => n.clamp(1, 100),
        Err(_) => return invalid_pagination(),
    };
    let status = q
        .status
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    // Filter kommt aus dem Scope, nicht roh aus der Query: bei Partnern ist das
    // immer der eigene Login, bei Admins der angefragte.
    let streamer = scope;
    let offset = (page - 1) * page_size;

    let mut qb_total =
        QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM twitch_clips_social_media WHERE 1=1");
    push_clips_where(&mut qb_total, streamer.as_deref(), status.as_deref());
    let total: i64 = match qb_total.build_query_scalar().fetch_one(&pool).await {
        Ok(total) => total,
        Err(e) => {
            tracing::error!(error = %e, "failed to count social media clips");
            return clip_load_failed();
        }
    };

    let mut qb = QueryBuilder::<Postgres>::new(&format!(
        "SELECT {CLIP_COLUMNS} FROM twitch_clips_social_media WHERE 1=1"
    ));
    push_clips_where(&mut qb, streamer.as_deref(), status.as_deref());
    qb.push(" ORDER BY created_at DESC, id DESC LIMIT ");
    qb.push_bind(page_size);
    qb.push(" OFFSET ");
    qb.push_bind(offset);
    let rows = match qb.build().fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "failed to query social media clips");
            return clip_load_failed();
        }
    };

    let mut clips: Vec<ClipRow> = Vec::with_capacity(rows.len());
    for r in &rows {
        match row_to_clip(r) {
            Ok(clip) => clips.push(clip),
            Err(e) => {
                tracing::error!(error = %e, "failed to decode social media clip list row");
                return clip_load_failed();
            }
        }
    }
    // Termin und Fehlergrund einmal für die ganze Seite holen, nicht je Clip
    // (sonst N+1 bei page_size=100).
    let clip_ids: Vec<i64> = clips.iter().map(|c| c.id).collect();
    let queue_info = match load_upload_queue_info(&pool, &clip_ids).await {
        Ok(info) => info,
        Err(error) => {
            tracing::error!(code = "upload_queue_load_failed", error = %error, "Social-Media-Queue-Stand konnte nicht geladen werden");
            return clip_load_failed();
        }
    };
    let mut items: Vec<Value> = Vec::with_capacity(clips.len());
    for clip in &clips {
        items.push(serialize_clip_record_with(&pool, clip, queue_info.get(&clip.id)).await);
    }
    Json(json!({ "items": items, "page": page, "page_size": page_size, "total": total }))
        .into_response()
}

/// `GET /social-media/api/admin/clips/:clip_db_id` — Clip-Detail (Admin).
pub async fn admin_clip_detail_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    match load_clip_row(&pool, clip_db_id).await {
        Ok(Some(clip)) => match serialize_clip_record(&pool, &clip).await {
            Ok(payload) => Json(payload).into_response(),
            Err(error) => {
                tracing::error!(clip_db_id, code = "upload_queue_load_failed", error = %error, "Social-Media-Queue-Stand konnte nicht geladen werden");
                clip_load_failed()
            }
        },
        Ok(None) => clip_not_found(),
        Err(_) => clip_load_failed(),
    }
}

/// `POST /social-media/api/admin/clips/:clip_db_id/discard` — Clip verwerfen (Admin).
pub async fn admin_clip_discard_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    // Partner-Freigabe-Guard: nach Clip-Auflösung, vor Wirkung.
    if let Some(guard_response) = guard_partner_access_for_clip(&pool, &auth, clip_db_id).await {
        return guard_response;
    }
    let outcome = match discard_clip(&pool, clip_db_id).await {
        Ok(Some(outcome)) => outcome,
        Ok(None) => return clip_not_found(),
        Err(error) => {
            tracing::error!(%error, clip_db_id, "Clip konnte nicht transaktional verworfen werden");
            return clip_load_failed();
        }
    };
    let mut payload = match load_clip_row(&pool, clip_db_id).await {
        Ok(Some(clip)) => match serialize_clip_record(&pool, &clip).await {
            Ok(payload) => payload,
            Err(error) => {
                tracing::error!(clip_db_id, code = "upload_queue_load_failed", error = %error, "Social-Media-Queue-Stand konnte nicht geladen werden");
                return clip_load_failed();
            }
        },
        Ok(None) => json!({ "clip_db_id": clip_db_id, "discarded": true }),
        Err(_) => return clip_load_failed(),
    };
    if let Some(object) = payload.as_object_mut() {
        object.insert("discarded".to_string(), json!(outcome.discarded));
        object.insert(
            "pending_stopped".to_string(),
            json!(outcome.pending_stopped),
        );
        object.insert(
            "already_running".to_string(),
            json!(outcome.already_running),
        );
    }
    if outcome.already_running > 0 {
        (StatusCode::CONFLICT, Json(payload)).into_response()
    } else {
        Json(payload).into_response()
    }
}

fn preparation_json(record: &ClipPreparationRecord) -> Value {
    let media_url = format!(
        "/social-media/api/admin/clips/{}/preparation/media",
        record.clip_db_id
    );
    let preview_ready = record.preview_ready();
    json!({
        "clip_db_id": record.clip_db_id,
        "state": record.state,
        "source_ready": record.source_ready(),
        "preview_ready": preview_ready,
        "preview_url": preview_ready.then_some(media_url.clone()),
        "download_url": preview_ready.then_some(format!("{media_url}?download=1")),
        "error_code": record.error_code,
        "error_message": safe_preparation_error_message(record.error_code.as_deref()),
        "requested_at": record.requested_at,
        "started_at": record.started_at,
        "completed_at": record.completed_at,
        "updated_at": record.updated_at,
    })
}

fn safe_preparation_error_message(error_code: Option<&str>) -> Option<&'static str> {
    match error_code {
        Some("clip_not_found") => Some("Der Clip wurde nicht gefunden."),
        Some("clip_discarded") => Some("Der Clip wurde verworfen."),
        Some("source_missing") => Some("Die Clip-Quelle ist nicht verfügbar."),
        Some("invalid_source_url") => Some("Die Clip-Quelle ist ungültig."),
        Some("download_failed") => Some("Die Clip-Quelle konnte nicht geladen werden."),
        Some("download_isolation_required") => {
            Some("Der automatische Twitch-Download ist bis zur sicheren Netzwerkfreigabe gesperrt.")
        }
        Some("render_failed") => Some("Die Vorschau konnte nicht erstellt werden."),
        Some("io_failed") | Some("database_failed") => {
            Some("Die Aufbereitung ist vorübergehend fehlgeschlagen.")
        }
        Some("preparation_busy") => Some("Die Vorschau wird bereits erstellt."),
        Some(_) | None => None,
    }
}

async fn preparation_access(
    auth: &DashboardAuthLevel,
    pool: &PgPool,
    clip_db_id: i64,
    write: bool,
) -> Result<(), Response> {
    let scope = require_sm_access(auth, pool, None).await?;
    require_clip_in_scope(pool, clip_db_id, scope.as_deref()).await?;
    if write {
        if let Some(response) = guard_partner_access_for_clip(pool, auth, clip_db_id).await {
            return Err(response);
        }
    }
    Ok(())
}

/// `GET /social-media/api/admin/clips/:clip_db_id/preparation`.
pub async fn clip_preparation_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(response) = preparation_access(&auth, &pool, clip_db_id, false).await {
        return response;
    }
    let service = ClipPreparationService::new(pool);
    match service.get(clip_db_id).await {
        Ok(Some(record)) => Json(preparation_json(&record)).into_response(),
        Ok(None) => clip_not_found(),
        Err(error) => preparation_error_response(error),
    }
}

/// `POST /social-media/api/admin/clips/:clip_db_id/preparation` legt nur den
/// Auftrag an. yt-dlp/FFmpeg laufen ausschließlich im Bot-Worker mit dessen
/// gebündelten Binaries; der HTTP-Request blockiert nie auf Videoverarbeitung.
pub async fn clip_preparation_post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(response) = preparation_access(&auth, &pool, clip_db_id, true).await {
        return response;
    }
    let service = ClipPreparationService::new(pool);
    match service.request(clip_db_id).await {
        Ok(record) => (StatusCode::ACCEPTED, Json(preparation_json(&record))).into_response(),
        Err(PreparationError::Busy(_)) => match service.get(clip_db_id).await {
            Ok(Some(record)) => {
                (StatusCode::ACCEPTED, Json(preparation_json(&record))).into_response()
            }
            Ok(None) => clip_not_found(),
            Err(error) => preparation_error_response(error),
        },
        Err(error) => preparation_error_response(error),
    }
}

fn preparation_error_response(error: PreparationError) -> Response {
    match error {
        PreparationError::ClipNotFound(_) => clip_not_found(),
        PreparationError::Discarded(_) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "clip_discarded" })),
        )
            .into_response(),
        PreparationError::Busy(_) => (
            StatusCode::ACCEPTED,
            Json(json!({ "error": "preparation_busy" })),
        )
            .into_response(),
        other => {
            tracing::error!(error = %other, "Clip-Aufbereitung fehlgeschlagen");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": other.code() })),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct PreparationMediaQuery {
    download: Option<String>,
}

fn stored_render_path_is_safe(clip_db_id: i64, path: &FsPath) -> bool {
    let rendered_dir = FsPath::new(DEFAULT_CLIPS_DIR).join("rendered");
    let expected_prefix = format!("{clip_db_id}-");
    path.parent() == Some(rendered_dir.as_path())
        && path.extension().and_then(|value| value.to_str()) == Some("mp4")
        && path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with(&expected_prefix))
}

fn parse_byte_range(raw: &str, length: u64) -> Result<(u64, u64), ()> {
    if length == 0 || raw.contains(',') {
        return Err(());
    }
    let value = raw.strip_prefix("bytes=").ok_or(())?;
    let (start, end) = value.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok((length.saturating_sub(suffix.min(length)), length - 1));
    }
    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= length {
        return Err(());
    }
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(length - 1)
    };
    if start > end {
        return Err(());
    }
    Ok((start, end))
}

fn harden_preparation_media_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
}

async fn open_render_file_from_dir_no_follow(
    rendered_dir: &FsPath,
    file_name: &std::ffi::OsStr,
) -> std::io::Result<tokio::fs::File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        // Linux O_DIRECTORY/O_NOFOLLOW. Der gehaltene Dir-FD schließt auch den
        // Austausch von `rendered` zwischen Prüfung und Datei-open; der zweite
        // O_NOFOLLOW-Open lehnt einen finalen Datei-Symlink ab.
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let mut directory_options = tokio::fs::OpenOptions::new();
        directory_options
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
        // Jede Pfadkomponente wird relativ zum jeweils gehaltenen Dir-FD mit
        // O_NOFOLLOW geöffnet. Ein Symlink in `data`, `clips` oder `rendered`
        // kann den authentifizierten Medienpfad damit nicht aus dem Baum
        // herauslenken.
        let mut directory = if rendered_dir.is_absolute() {
            directory_options.open("/").await?
        } else {
            directory_options.open(".").await?
        };
        for component in rendered_dir.components() {
            let name = match component {
                std::path::Component::RootDir | std::path::Component::CurDir => continue,
                std::path::Component::Normal(name) => name,
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Unsicherer Renderpfad",
                    ));
                }
            };
            let held_path = std::path::PathBuf::from(format!(
                "/proc/self/fd/{}/{}",
                directory.as_raw_fd(),
                name.to_string_lossy()
            ));
            directory = directory_options.open(held_path).await?;
        }
        let held_path =
            std::path::PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
                .join(file_name);
        let mut file_options = tokio::fs::OpenOptions::new();
        // O_NONBLOCK verhindert, dass ein vorgepflanztes FIFO schon beim
        // Öffnen einen Tokio-Blocking-Thread festhält. fstat/metadata lehnt es
        // direkt danach als Nicht-Regulärdatei ab.
        file_options
            .read(true)
            .custom_flags(O_NOFOLLOW | O_NONBLOCK);
        let file = file_options.open(held_path).await?;
        drop(directory);
        Ok(file)
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Sicherer Fallback auf Plattformen ohne den hier bekannten Flag:
        // finale Symlinks ablehnen und anschließend nur den geöffneten Handle
        // für Metadaten/Streaming verwenden.
        let directory_metadata = tokio::fs::symlink_metadata(rendered_dir).await?;
        if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Symlink als Renderverzeichnis abgelehnt",
            ));
        }
        let path = rendered_dir.join(file_name);
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Symlink als Renderdatei abgelehnt",
            ));
        }
        tokio::fs::File::open(path).await
    }
}

async fn open_render_file_no_follow(path: &FsPath) -> std::io::Result<tokio::fs::File> {
    let rendered_dir = FsPath::new(DEFAULT_CLIPS_DIR).join("rendered");
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Renderdateiname fehlt")
    })?;
    open_render_file_from_dir_no_follow(&rendered_dir, file_name).await
}

/// Streamt ausschließlich das serverseitig gespeicherte Ready-MP4. Clientseitig
/// eingereichte Pfade werden nicht akzeptiert; Range unterstützt Browser-Scrub.
pub async fn clip_preparation_media_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    Query(query): Query<PreparationMediaQuery>,
    request_headers: HeaderMap,
) -> Response {
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(response) = preparation_access(&auth, &pool, clip_db_id, false).await {
        return response;
    }
    let service = ClipPreparationService::new(pool);
    let record = match service.get(clip_db_id).await {
        Ok(Some(record)) if record.state == "preview_ready" => record,
        Ok(Some(_)) | Ok(None) => return clip_not_found(),
        Err(error) => return preparation_error_response(error),
    };
    let Some(path) = record.render_path.as_deref().map(FsPath::new) else {
        return clip_not_found();
    };
    if !stored_render_path_is_safe(clip_db_id, path) {
        return clip_not_found();
    }
    let mut file = match open_render_file_no_follow(path).await {
        Ok(file) => file,
        Err(_) => return clip_not_found(),
    };
    let opened_metadata = match file.metadata().await {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return clip_not_found(),
    };
    let total = opened_metadata.len();
    if total == 0 {
        return clip_not_found();
    }
    let requested_range = request_headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let (status, start, end) = match requested_range {
        Some(raw_range) => match parse_byte_range(raw_range, total) {
            Ok((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end),
            Err(()) => {
                let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                let headers = response.headers_mut();
                harden_preparation_media_headers(headers);
                if let Ok(value) = HeaderValue::from_str(&format!("bytes */{total}")) {
                    headers.insert(header::CONTENT_RANGE, value);
                }
                return response;
            }
        },
        None => (StatusCode::OK, 0, total - 1),
    };
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return clip_not_found();
    }
    let remaining = end - start + 1;
    let stream =
        futures_util::stream::try_unfold((file, remaining), |(mut file, remaining)| async move {
            if remaining == 0 {
                return Ok::<_, std::io::Error>(None);
            }
            let mut chunk = vec![0_u8; remaining.min(64 * 1024) as usize];
            let read = file.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Ready-MP4 endete vor dem angekündigten Bereich",
                ));
            }
            chunk.truncate(read);
            Ok(Some((Bytes::from(chunk), (file, remaining - read as u64))))
        });
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    harden_preparation_media_headers(headers);
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Ok(value) = HeaderValue::from_str(&(end - start + 1).to_string()) {
        headers.insert(header::CONTENT_LENGTH, value);
    }
    if status == StatusCode::PARTIAL_CONTENT {
        if let Ok(value) = HeaderValue::from_str(&format!("bytes {start}-{end}/{total}")) {
            headers.insert(header::CONTENT_RANGE, value);
        }
    }
    let disposition = if query.download.as_deref().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }) {
        format!("attachment; filename=clip-{clip_db_id}.mp4")
    } else {
        format!("inline; filename=clip-{clip_db_id}.mp4")
    };
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn invalid_payload() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid_payload" })),
    )
        .into_response()
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
fn parse_string_field<'a>(
    payload: &'a Value,
    field: &str,
    max_chars: usize,
) -> Result<Option<Option<&'a str>>, Response> {
    match payload.get(field) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.chars().count() > max_chars {
                return Err(enrichment_limit_response(field, max_chars));
            }
            Ok(Some(if t.is_empty() { None } else { Some(t) }))
        }
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_field", "field": field })),
        )
            .into_response()),
    }
}

/// Hashtag-Liste normalisieren (Python `_normalize_hashtag_list`): `#`-Präfix,
/// dedupliziert (case-insensitiv), leere übersprungen. fehlt/null → `None`,
/// nicht-Liste → 400.
fn parse_hashtag_field(
    payload: &Value,
    field: &str,
    max_count: usize,
    max_chars: usize,
) -> Result<Option<Vec<String>>, Response> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(arr)) => {
            if arr.len() > max_count {
                return Err(enrichment_limit_response(field, max_count));
            }
            let mut seen = std::collections::HashSet::new();
            let mut cleaned = Vec::new();
            for entry in arr {
                let Some(token) = entry.as_str() else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "invalid_field", "field": field })),
                    )
                        .into_response());
                };
                let token = token.trim().to_string();
                if token.is_empty() {
                    continue;
                }
                let token = if token.starts_with('#') {
                    token
                } else {
                    format!("#{}", token.trim_start_matches('#'))
                };
                if token.chars().count() > max_chars {
                    return Err(enrichment_limit_response(field, max_chars));
                }
                if seen.insert(token.to_lowercase()) {
                    cleaned.push(token);
                }
            }
            Ok(Some(cleaned))
        }
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_field", "field": field })),
        )
            .into_response()),
    }
}

fn enrichment_limit_response(field: &str, max: usize) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "enrichment_limits_exceeded",
            "field": field,
            "max": max,
        })),
    )
        .into_response()
}

/// `PUT /social-media/api/admin/clips/:clip_db_id/enrichment` — Edits speichern (Admin).
pub async fn enrichment_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    body: String,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    // Partner-Freigabe-Guard: nach Clip-Auflösung, vor Wirkung.
    if let Some(guard_response) = guard_partner_access_for_clip(&pool, &auth, clip_db_id).await {
        return guard_response;
    }
    let child_clip_db_id = match require_clip_child_id(&pool, clip_db_id, "enrichment_put").await {
        Ok(id) => id,
        Err(e) => return e,
    };
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    macro_rules! field {
        ($name:literal, $max:expr) => {
            match parse_string_field(&payload, $name, $max) {
                Ok(v) => v,
                Err(e) => return e,
            }
        };
    }
    macro_rules! tags {
        ($name:literal, $max:expr) => {
            match parse_hashtag_field(&payload, $name, $max, 100) {
                Ok(v) => v,
                Err(e) => return e,
            }
        };
    }
    // Python-Reihenfolge: erst title/description (invalid_field), dann hashtags.
    let ty = field!("title_youtube", 100);
    let tt = field!("title_tiktok", 150);
    let ti = field!("title_instagram", 125);
    let dy = field!("description_youtube", 5_000);
    let dt = field!("description_tiktok", 2_200);
    let di = field!("description_instagram", 2_200);
    let hy = tags!("hashtags_youtube", 10);
    let ht = tags!("hashtags_tiktok", 12);
    let hi = tags!("hashtags_instagram", 15);
    let result = update_manual_edit(
        &pool,
        child_clip_db_id,
        editor_user_id(&auth).as_deref(), // edited_by (B15-FIX): Session-Actor statt NULL
        ty,
        tt,
        ti,
        dy,
        dt,
        di,
        hy.as_deref(),
        ht.as_deref(),
        hi.as_deref(),
    )
    .await;
    if let Err(error) = result {
        return content_mutation_error_response(error);
    }
    match get_enrichment_checked(&pool, child_clip_db_id).await {
        Ok(Some(record)) => Json(enrichment_record_json(&record)).into_response(),
        Ok(None) => enrichment_persistence_error("enrichment_missing_after_save", child_clip_db_id),
        Err(_) => {
            enrichment_persistence_error("enrichment_read_after_save_failed", child_clip_db_id)
        }
    }
}

/// `GET /social-media/api/admin/clips/:clip_db_id/enrichment` — Enrichment (Admin).
pub async fn enrichment_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    let child_clip_db_id = match require_clip_child_id(&pool, clip_db_id, "enrichment_get").await {
        Ok(id) => id,
        Err(e) => return e,
    };
    match ensure_enrichment_row_checked(&pool, child_clip_db_id).await {
        Ok(record) => Json(enrichment_record_json(&record)).into_response(),
        Err(_) => enrichment_persistence_error("enrichment_ensure_failed", child_clip_db_id),
    }
}

/// `POST /social-media/api/admin/clips/:clip_db_id/enrichment/run` — Enrichment
/// manuell anstoßen (Admin). Optionaler Body `{ "force": true }` reichert auch
/// bereits fertige Clips neu an. Baut den LLM-Dispatcher inline. Transkription
/// ist per Grillme-Entscheidung (Block 15) deaktiviert — es wird KEIN Transcriber
/// injiziert (tb-social-media hat das whisper/OpenAI-Modul entfernt), die Pipeline
/// läuft ohne Transkription weiter.
pub async fn enrichment_run_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    body: String,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    // Partner-Freigabe-Guard: nach Clip-Auflösung, vor Wirkung.
    if let Some(guard_response) = guard_partner_access_for_clip(&pool, &auth, clip_db_id).await {
        return guard_response;
    }
    let child_clip_db_id = match require_clip_child_id(&pool, clip_db_id, "enrichment_run").await {
        Ok(id) => id,
        Err(e) => return e,
    };
    // Optionaler Body: ungültiges/leeres JSON → force=false (Python schluckt Fehler).
    let force = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("force").map(coerce_bool))
        .unwrap_or(false);

    let llm = LlmDispatcher::new(pool.clone());
    let pipeline = ClipEnrichmentPipeline::new(pool.clone());
    let outcome = match pipeline.run(child_clip_db_id, None, &llm, force).await {
        Ok(o) => o,
        Err(PipelineError::ClipNotFound(_)) => return clip_not_found(),
        Err(PipelineError::UploadRunning(already_running)) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "upload_already_running",
                    "already_running": already_running,
                })),
            )
                .into_response();
        }
        Err(PipelineError::Persistence) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "save_failed" })),
            )
                .into_response();
        }
    };

    let enrichment = match get_enrichment_checked(&pool, child_clip_db_id).await {
        Ok(Some(record)) => enrichment_record_json(&record),
        Ok(None) => {
            return enrichment_persistence_error("enrichment_missing_after_run", child_clip_db_id)
        }
        Err(_) => {
            return enrichment_persistence_error(
                "enrichment_read_after_run_failed",
                child_clip_db_id,
            )
        }
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

fn enrichment_persistence_error(code: &'static str, clip_db_id: i32) -> Response {
    tracing::error!(
        code,
        clip_db_id,
        "Social-Media-Enrichment konnte nicht vollständig gelesen oder gespeichert werden"
    );
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "database_failed" })),
    )
        .into_response()
}

fn content_mutation_error_response(error: ContentMutationError) -> Response {
    match error {
        ContentMutationError::UploadRunning(already_running) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "upload_already_running",
                "already_running": already_running,
            })),
        )
            .into_response(),
        ContentMutationError::Db(error) => {
            tracing::error!(%error, "Clip-Inhalt konnte nicht sicher geändert werden");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "save_failed" })),
            )
                .into_response()
        }
    }
}

/// `GET /social-media/api/admin/analytics/clips/:clip_db_id` — Analytics (Admin).
pub async fn clip_analytics_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    if let Err(e) = require_clip_row(&pool, clip_db_id).await {
        return e;
    }
    let items: Vec<Value> = list_clip_analytics(&pool, clip_db_id)
        .await
        .iter()
        .map(clip_analytics_json)
        .collect();
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
pub async fn reports_run_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: String,
) -> Response {
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
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let streamer = payload
        .get("streamer")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    // Partner-Freigabe-Guard: nach Streamer-Auflösung, vor Wirkung.
    if let Some(ref login) = streamer {
        if let Some(guard_response) = check_partner_access_guard(&pool, &auth, login).await {
            return guard_response;
        }
    }
    if !matches!(kind.as_str(), "streamer" | "cross") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_kind" })),
        )
            .into_response();
    }
    if kind == "streamer" && streamer.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "streamer_required" })),
        )
            .into_response();
    }
    let writer = SocialMediaReportWriter::new(pool.clone());
    let result = if kind == "streamer" {
        writer
            .write_streamer_report(streamer.as_deref().unwrap_or(""), None, None, true)
            .await
    } else {
        writer.write_cross_report(None, None, true).await
    };
    match result {
        Ok(report) => Json(report_record_json(&report)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "report_generation_failed" })),
        )
            .into_response(),
    }
}

/// `GET /social-media/api/admin/reports` — Report-Liste (Admin).
pub async fn reports_list_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<ReportsQuery>,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let kind = q
        .kind
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    if let Some(k) = &kind {
        if !matches!(k.as_str(), "streamer" | "cross" | "admin") {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_kind" })),
            )
                .into_response();
        }
    }
    let streamer = q
        .streamer
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let limit = match q.limit.as_deref().unwrap_or("20").parse::<i64>() {
        Ok(n) => n.clamp(1, 20),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_limit" })),
            )
                .into_response()
        }
    };
    let items: Vec<Value> = list_reports(&pool, kind.as_deref(), streamer.as_deref(), limit)
        .await
        .iter()
        .map(report_record_json)
        .collect();
    Json(json!({ "items": items })).into_response()
}

fn invalid_json() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid_json" })),
    )
        .into_response()
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
                let token = entry
                    .as_str()
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                if matches!(token.as_str(), "youtube" | "tiktok" | "instagram")
                    && seen.insert(token.clone())
                {
                    cleaned.push(token);
                }
            }
            Ok(cleaned)
        }
        Some(_) => Err((StatusCode::BAD_REQUEST, "platforms must be a list").into_response()),
    }
}

/// `GET /social-media/api/admin/approval/:clip_db_id` — Approval-State (Admin).
pub async fn approval_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    if let Err(e) = require_clip_row(&pool, clip_db_id).await {
        return e;
    }
    let approval = optional_approval_json(&pool, clip_db_id).await;
    Json(json!({ "clip_db_id": clip_db_id, "approval": approval })).into_response()
}

/// `POST /social-media/api/admin/approval/:clip_db_id/decision` — Entscheidung (Admin).
pub async fn approval_decision_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    body: String,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    // Partner-Freigabe-Guard: nach Clip-Auflösung, vor Wirkung.
    if let Some(guard_response) = guard_partner_access_for_clip(&pool, &auth, clip_db_id).await {
        return guard_response;
    }
    let child_clip_db_id = match require_clip_child_id(&pool, clip_db_id, "approval_decision").await
    {
        Ok(id) => id,
        Err(e) => return e,
    };
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    let raw_decision = payload
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let decision = match raw_decision.as_str() {
        "approve" | "approved" => "approve",
        "skip" | "skipped" => "skip",
        "edit" | "editing" => "edit",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_decision" })),
            )
                .into_response();
        }
    };
    let platforms = match normalize_platform_list(payload.get("platforms")) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Dritter Weg in die Upload-Warteschlange: `handle_decision` ruft bei
    // "approve" `ensure_queued_uploads` auf und schreibt damit selbst in
    // `twitch_clips_upload_queue`. Also dieselbe Pro-Sperre wie beim direkten
    // Einreihen. "skip" und "edit" reihen nichts ein und bleiben ab Free offen.
    if decision == DECISION_APPROVE {
        if let Some(resp) = auto_posting_guard(&pool, &auth).await {
            return resp;
        }
    }
    // user_id (B15-FIX): Session-Actor für das Approval-Audit (sonst NULL).
    let actor = editor_user_id(&auth);
    match handle_decision(
        &pool,
        child_clip_db_id,
        decision,
        &platforms,
        actor.as_deref(),
    )
    .await
    {
        Ok(record) => {
            let clip = match load_clip_row(&pool, clip_db_id).await {
                Ok(Some(c)) => match serialize_clip_record(&pool, &c).await {
                    Ok(payload) => payload,
                    Err(error) => {
                        tracing::error!(clip_db_id, code = "upload_queue_load_failed", error = %error, "Social-Media-Queue-Stand konnte nicht geladen werden");
                        return clip_load_failed();
                    }
                },
                Ok(None) => Value::Null,
                Err(_) => return clip_load_failed(),
            };
            Json(json!({ "clip_db_id": clip_db_id, "approval": serialize_approval_record(&record), "clip": clip })).into_response()
        }
        Err(ApprovalError::UploadRunning(already_running)) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "upload_already_running",
                "already_running": already_running,
            })),
        )
            .into_response(),
        Err(ApprovalError::PreviewNotReady) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "preview_not_ready" })),
        )
            .into_response(),
        Err(ApprovalError::Db(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "approval_decision_failed" })),
        )
            .into_response(),
        // Eigener Code statt `invalid_decision`: die Oberfläche kennt dafür
        // einen übersetzten Satz. `message` trägt nur die Plattformnamen, die
        // dort in den Satz eingesetzt werden; ein fertiger deutscher Satz aus
        // dem Backend stünde im englischen Dashboard auf Deutsch da.
        Err(ApprovalError::NurPausiertePlattformen(plattformen)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "only_paused_platforms", "message": plattformen })),
        )
            .into_response(),
        Err(ApprovalError::TikTokConsentRequired) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "tiktok_consent_required" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_decision", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `POST /social-media/api/approval/:clip_db_id/cancel` — eingeplante Uploads
/// eines Clips stoppen.
///
/// Macht das Versprechen des Modus „Veto-Fenster" wahr: eingeplant heißt dort,
/// dass man bis zum Termin noch eingreifen kann. Gestoppt wird nur, was noch
/// wartet; eine Plattform, die schon läuft oder durch ist, bleibt stehen und
/// wird in `already_running` gemeldet, damit die Oberfläche ehrlich sagen kann,
/// dass eine Plattform schon raus war.
///
/// Abgesichert wie die übrigen clip-bezogenen Routen: Scope, Clip-Ownership,
/// Partner-Freigabe.
///
/// Antwort: `{"cancelled": <gestoppte Zeilen>, "already_running": <Zeilen in
/// processing/completed>}`.
pub async fn approval_cancel_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, clip_db_id, scope.as_deref()).await {
        return e;
    }
    // Partner-Freigabe-Guard: nach Clip-Auflösung, vor Wirkung.
    if let Some(guard_response) = guard_partner_access_for_clip(&pool, &auth, clip_db_id).await {
        return guard_response;
    }
    let child_clip_db_id = match require_clip_child_id(&pool, clip_db_id, "approval_cancel").await {
        Ok(id) => id,
        Err(e) => return e,
    };
    match cancel_scheduled_uploads(&pool, child_clip_db_id).await {
        Ok(outcome) => Json(json!({
            "cancelled": outcome.cancelled,
            "already_running": outcome.already_running,
        }))
        .into_response(),
        Err(ApprovalError::ClipNotFound(_)) => clip_not_found(),
        Err(error) => {
            tracing::error!(%error, clip_db_id, "failed to cancel scheduled social media uploads");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "approval_cancel_failed" })),
            )
                .into_response()
        }
    }
}

/// Erlaubte Freigabe-Modi in Anzeigereihenfolge, von vorsichtig nach automatisch.
const APPROVAL_MODES: [&str; 3] = ["manual", "veto_window", "full_auto"];
const RELEASE_MODES: [&str; 2] = ["prepare_only", "live"];

/// Obergrenzen der Kadenz. Mehr als zehn Posts am Tag ist kein Zeitplan mehr.
const MAX_POSTS_PER_WEEK: i64 = 70;
const MAX_POSTS_PER_DAY: i64 = 10;
const MAX_POST_TIMES: usize = 12;

fn platform_schedule_json(schedule: &PlatformSchedule, next_slot: Option<String>) -> Value {
    json!({
        "platform": schedule.platform,
        "auto_post": schedule.auto_post,
        "posts_per_week": schedule.posts_per_week,
        "max_posts_per_day": schedule.max_posts_per_day,
        "post_times": schedule.post_times,
        "next_slot": next_slot,
        "provider_release_blocked": schedule.platform == "tiktok",
        "release_block_reason": if schedule.platform == "tiktok" {
            Some("tiktok_consent_required")
        } else {
            None
        },
    })
}

fn category_json(category: &CategoryOption) -> Value {
    json!({
        "category_key": category.category_key,
        "display_name": category.display_name,
        "enrichment_enabled": category.enrichment_enabled,
        "auto_post": category.auto_post,
    })
}

fn pool_forecast_json(forecast: &PoolForecast) -> Value {
    json!({
        "verfuegbare_clips": forecast.verfuegbare_clips,
        "aktive_plattformen": forecast.aktive_plattformen,
        "reicht_fuer_posts": forecast.reicht_fuer_posts,
        "posts_pro_woche": forecast.posts_pro_woche,
        "reicht_fuer_tage": forecast.reicht_fuer_tage,
        "warnung": forecast.warnung,
    })
}

/// Schon vergebene Termine je Plattform, in einer Abfrage fuer den ganzen
/// Kanal.
///
/// Gleiche Bedingungen wie `posting_plan::belegte_termine`, nur ohne den
/// Plattform-Filter: der Zeitplan braucht die Termine ohnehin fuer alle drei
/// Plattformen, und drei Einzelabfragen kosten nur Runden.
async fn belegte_termine_je_plattform(
    pool: &PgPool,
    streamer_login: &str,
) -> Result<HashMap<String, Vec<DateTime<Utc>>>, sqlx::Error> {
    let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT q.platform, COALESCE(q.scheduled_at, q.completed_at) AS termin \
           FROM twitch_clips_upload_queue q \
           JOIN twitch_clips_social_media c ON c.id = q.clip_id \
          WHERE LOWER(c.streamer_login) = $1 \
            AND q.status <> 'failed' \
            AND COALESCE(q.scheduled_at, q.completed_at) IS NOT NULL \
            AND COALESCE(q.scheduled_at, q.completed_at) > CURRENT_TIMESTAMP - INTERVAL '14 days'",
    )
    .bind(streamer_login.trim().to_lowercase())
    .fetch_all(pool)
    .await?;

    let mut je_plattform: HashMap<String, Vec<DateTime<Utc>>> = HashMap::new();
    for (platform, termin) in rows {
        je_plattform.entry(platform).or_default().push(termin);
    }
    Ok(je_plattform)
}

/// Baut die vollstaendige Antwort des Zeitplan-Endpoints.
async fn posting_plan_json(pool: &PgPool, streamer_login: &str) -> Result<Value, sqlx::Error> {
    // Feste Zahl an Abfragen, unabhaengig von der Zahl der Plattformen. Vorher
    // holte `plan_next_slot` je Plattform Zeitplan, Einstellungen und belegte
    // Termine erneut, und `pool_forecast` lud die Zeitplaene ein viertes Mal:
    // rund zehn Abfragen pro GET, und jede Zeitplan-Mutation gibt den
    // kompletten Plan zurueck.
    let settings = load_streamer_settings_checked(pool, streamer_login).await?;
    let schedules = load_platform_schedules_checked(pool, streamer_login).await?;
    let categories = load_categories_checked(pool, streamer_login).await?;
    let forecast = berechne_vorrat(verfuegbare_clips(pool, streamer_login).await?, &schedules);
    let belegt = belegte_termine_je_plattform(pool, streamer_login).await?;
    let now = chrono::Utc::now();
    let leer: Vec<DateTime<Utc>> = Vec::new();

    let mut platforms = Vec::with_capacity(schedules.len());
    for schedule in &schedules {
        let next_slot = if schedule.auto_post {
            next_cadence_slot(
                now,
                belegt.get(&schedule.platform).unwrap_or(&leer),
                &schedule.posting_schedule(&settings.timezone),
                &schedule.limits(),
            )
            .map(|slot| slot.to_rfc3339())
        } else {
            None
        };
        platforms.push(platform_schedule_json(schedule, next_slot));
    }

    Ok(json!({
        "streamer_login": streamer_login,
        "approval_mode": settings.approval_mode.as_str(),
        "approval_modes": APPROVAL_MODES,
        "release_mode": settings.release_mode.as_str(),
        "release_enabled": settings.release_mode.release_enabled(),
        "release_modes": RELEASE_MODES,
        "timezone": settings.timezone,
        "platforms": platforms,
        "categories": categories.iter().map(category_json).collect::<Vec<_>>(),
        "pool": pool_forecast_json(&forecast),
    }))
}

async fn posting_plan_response(pool: &PgPool, streamer_login: &str) -> Response {
    match posting_plan_json(pool, streamer_login).await {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => {
            tracing::error!(
                %error,
                streamer = %streamer_login,
                code = "posting_plan_read_failed",
                "Social-Media-Zeitplan konnte nicht vollständig gelesen werden"
            );
            upload_error(StatusCode::INTERNAL_SERVER_ERROR, "database_failed", None)
        }
    }
}

/// `GET /social-media/api/admin/settings/posting-plan?streamer_login=` —
/// Zeitplan, Freigabe-Modus, Kategorien und Vorratsrechnung eines Kanals.
pub async fn posting_plan_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerLoginQuery>,
) -> Response {
    let slug = match required_streamer_scope(&auth, &pool, q.streamer_login.as_deref()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    if let Err(error) = ensure_streamer_rows(&pool, &slug).await {
        tracing::warn!(%error, streamer = %slug, "Zeitplan-Defaults konnten nicht angelegt werden");
    }
    posting_plan_response(&pool, &slug).await
}

/// `PUT /social-media/api/admin/settings/posting-plan` — Freigabe-Modus und
/// Zeitzone eines Kanals setzen.
pub async fn posting_plan_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerLoginQuery>,
    body: String,
) -> Response {
    let slug = match required_streamer_scope(&auth, &pool, q.streamer_login.as_deref()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }

    let current = match load_streamer_settings_checked(&pool, &slug).await {
        Ok(settings) => settings,
        Err(_) => {
            return upload_error(StatusCode::INTERNAL_SERVER_ERROR, "database_failed", None);
        }
    };
    let approval_mode = match payload.get("approval_mode").and_then(Value::as_str) {
        Some(raw) => {
            if !APPROVAL_MODES.contains(&raw) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "unknown approval_mode" })),
                )
                    .into_response();
            }
            ApprovalMode::parse(raw)
        }
        None => current.approval_mode,
    };
    let timezone = match payload.get("timezone").and_then(Value::as_str) {
        Some(raw) => {
            let raw = raw.trim();
            if raw.parse::<chrono_tz::Tz>().is_err() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "unknown timezone" })),
                )
                    .into_response();
            }
            raw.to_string()
        }
        None => current.timezone,
    };
    if payload.get("release_mode").is_some() && !matches!(auth, DashboardAuthLevel::Admin { .. }) {
        return forbidden("Nur Admins dürfen Veröffentlichungen freischalten.");
    }
    let release_mode = match payload.get("release_mode").and_then(Value::as_str) {
        Some(raw) => {
            if !RELEASE_MODES.contains(&raw) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "unknown release_mode" })),
                )
                    .into_response();
            }
            ReleaseMode::parse(raw)
        }
        None => current.release_mode,
    };

    let settings = StreamerSettings {
        approval_mode,
        timezone,
        release_mode,
    };
    let actor = editor_user_id(&auth);
    match save_streamer_settings(&pool, &slug, &settings, actor.as_deref()).await {
        Ok(_) => {}
        Err(StreamerSettingsSaveError::ProviderInFlight(jobs)) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "provider_in_flight",
                    "disabled": true,
                    "release_mode": "prepare_only",
                    "active_jobs": jobs.into_iter().map(|job| json!({
                        "queue_id": job.queue_id,
                        "platform": job.platform,
                        "status": job.status,
                        "provider_external_id": job.provider_external_id,
                    })).collect::<Vec<_>>(),
                })),
            )
                .into_response();
        }
        Err(StreamerSettingsSaveError::BacklogCannotBeScheduled { platform }) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "release_backlog_cannot_be_scheduled",
                    "platform": platform,
                    "release_mode": "prepare_only",
                })),
            )
                .into_response();
        }
        Err(StreamerSettingsSaveError::Db(error)) => {
            tracing::error!(%error, streamer_login = %slug, "Posting-Plan konnte nicht gespeichert werden");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response();
        }
    }
    posting_plan_response(&pool, &slug).await
}

/// Prueft eine Liste von Posting-Zeiten im Format `HH:MM`.
fn parse_post_times(value: &Value) -> Result<Vec<String>, &'static str> {
    let Some(items) = value.as_array() else {
        return Err("post_times must be an array");
    };
    if items.is_empty() {
        return Err("post_times must not be empty");
    }
    if items.len() > MAX_POST_TIMES {
        return Err("too many post_times");
    }
    let mut out: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let Some(raw) = item.as_str() else {
            return Err("post_times must be strings");
        };
        let raw = raw.trim();
        if chrono::NaiveTime::parse_from_str(raw, "%H:%M").is_err() {
            return Err("post_times must use HH:MM");
        }
        if !out.iter().any(|existing| existing == raw) {
            out.push(raw.to_string());
        }
    }
    out.sort();
    Ok(out)
}

/// `PUT /social-media/api/admin/settings/posting-plan/platform/:platform` —
/// Auto-Posting und Kadenz einer Plattform setzen.
pub async fn posting_plan_platform_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(platform): Path<String>,
    Query(q): Query<StreamerLoginQuery>,
    body: String,
) -> Response {
    let slug = match required_streamer_scope(&auth, &pool, q.streamer_login.as_deref()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let platform = platform.trim().to_lowercase();
    if !PLATFORMS.contains(&platform.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unknown platform" })),
        )
            .into_response();
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }

    let current = match load_platform_schedules_checked(&pool, &slug).await {
        Ok(schedules) => schedules,
        Err(_) => {
            return upload_error(StatusCode::INTERNAL_SERVER_ERROR, "database_failed", None);
        }
    }
    .into_iter()
    .find(|s| s.platform == platform);
    let Some(current) = current else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unknown platform" })),
        )
            .into_response();
    };

    let bounded = |key: &str, fallback: i32, max: i64| -> Result<i32, &'static str> {
        match payload.get(key) {
            Some(value) => {
                let Some(number) = value.as_i64() else {
                    return Err("must be a whole number");
                };
                if !(0..=max).contains(&number) {
                    return Err("out of range");
                }
                Ok(number as i32)
            }
            None => Ok(fallback),
        }
    };
    let posts_per_week = match bounded("posts_per_week", current.posts_per_week, MAX_POSTS_PER_WEEK)
    {
        Ok(value) => value,
        Err(reason) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("posts_per_week {reason}") })),
            )
                .into_response()
        }
    };
    let max_posts_per_day = match bounded(
        "max_posts_per_day",
        current.max_posts_per_day,
        MAX_POSTS_PER_DAY,
    ) {
        Ok(value) => value,
        Err(reason) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("max_posts_per_day {reason}") })),
            )
                .into_response()
        }
    };
    let post_times = match payload.get("post_times") {
        Some(value) => match parse_post_times(value) {
            Ok(times) => times,
            Err(reason) => {
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": reason }))).into_response()
            }
        },
        None => current.post_times.clone(),
    };

    let requested_auto_post = payload
        .get("auto_post")
        .map(coerce_bool)
        .unwrap_or(current.auto_post);
    if platform == "tiktok" && requested_auto_post {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "tiktok_consent_required" })),
        )
            .into_response();
    }
    let schedule = PlatformSchedule {
        platform,
        auto_post: requested_auto_post,
        posts_per_week,
        max_posts_per_day,
        post_times,
    };
    let actor = editor_user_id(&auth);
    if save_platform_schedule(&pool, &slug, &schedule, actor.as_deref())
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "db" })),
        )
            .into_response();
    }
    posting_plan_response(&pool, &slug).await
}

/// `PUT /social-media/api/admin/settings/posting-plan/category/:category_key` —
/// Auto-Posting einer Spielkategorie schalten.
pub async fn posting_plan_category_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(category_key): Path<String>,
    Query(q): Query<StreamerLoginQuery>,
    body: String,
) -> Response {
    let slug = match required_streamer_scope(&auth, &pool, q.streamer_login.as_deref()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let category_key = category_key.trim().to_lowercase();
    let categories = match load_categories_checked(&pool, &slug).await {
        Ok(categories) => categories,
        Err(_) => {
            return upload_error(StatusCode::INTERNAL_SERVER_ERROR, "database_failed", None);
        }
    };
    if !categories
        .iter()
        .any(|category| category.category_key == category_key)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unknown category" })),
        )
            .into_response();
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    let auto_post = payload.get("auto_post").map(coerce_bool).unwrap_or(false);
    let actor = editor_user_id(&auth);
    if save_category_setting(&pool, &slug, &category_key, auto_post, actor.as_deref())
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "db" })),
        )
            .into_response();
    }
    posting_plan_response(&pool, &slug).await
}

/// Löst den Streamer eines Aufrufs auf, der genau einen Kanal braucht.
///
/// Sicherheitskern dieser Endpoints: gearbeitet wird ausschließlich mit dem
/// Scope aus [`require_sm_access`], nie mit dem angefragten Login. Für einen
/// Partner ist der Scope immer der eigene Kanal, ein fremder `streamer_login`
/// führt vorher schon zu 403. Ein Admin muss den Kanal benennen, „alle" gibt es
/// hier nicht: VOD-Archiv und Zeitplan sind Entscheidungen je Kanal.
#[allow(clippy::result_large_err)]
async fn required_streamer_scope(
    auth: &DashboardAuthLevel,
    pool: &PgPool,
    requested: Option<&str>,
) -> Result<String, Response> {
    let scope = require_sm_access(auth, pool, requested).await?;
    let Some(login) = scope else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "streamer_login is required" })),
        )
            .into_response());
    };
    let slug = normalize_safe_slug(Some(&login), "streamer_login")?.to_lowercase();
    if !ensure_streamer_exists(pool, &slug).await {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown_streamer" })),
        )
            .into_response());
    }
    Ok(slug)
}

fn vod_archive_json(s: &VodArchiveSettings) -> Value {
    json!({
        "streamer_login": s.streamer_login,
        "enabled": s.enabled,
        "privacy": s.privacy,
        "privacy_options": VOD_ARCHIVE_PRIVACY_VALUES,
        "privacy_forced": privacy_forced(),
    })
}

/// `GET /social-media/api/admin/settings/vod-archive?streamer_login=` —
/// VOD-Archiv eines Streamers lesen.
///
/// Solange das Google-Projekt nicht auditiert ist, dreht YouTube jeden Upload
/// auf `private` zurueck. Nach bestandenem Audit `YOUTUBE_AUDIT_PASSED=1`
/// setzen, dann ist die Sichtbarkeit im Dashboard waehlbar.
fn privacy_forced() -> bool {
    !matches!(
        std::env::var("YOUTUBE_AUDIT_PASSED").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// `privacy_forced` sagt der Oberfläche, dass YouTube ohne auditiertes
/// API-Projekt jeden Upload auf `private` zurückdreht, egal was hier steht.
pub async fn vod_archive_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<StreamerLoginQuery>,
) -> Response {
    let slug = match required_streamer_scope(&auth, &pool, q.streamer_login.as_deref()).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    Json(vod_archive_json(
        &get_vod_archive_settings(&pool, &slug).await,
    ))
    .into_response()
}

/// `PUT /social-media/api/admin/settings/vod-archive` — VOD-Archiv eines
/// Streamers setzen.
///
/// Bewusst `require_sm_access` statt `require_admin`: wer überhaupt ins
/// Social-Media-Dashboard kommt, ist freigeschaltet und darf sein eigenes
/// Archiv schalten. Geschrieben wird nur der Kanal aus dem Scope.
pub async fn vod_archive_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: String,
) -> Response {
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return invalid_json(),
    };
    if !payload.is_object() {
        return invalid_payload();
    }
    let slug = match required_streamer_scope(
        &auth,
        &pool,
        payload.get("streamer_login").and_then(Value::as_str),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return e,
    };
    // Partner-Freigabe-Guard: nach Streamer-Auflösung, vor Wirkung.
    if let Some(guard_response) = check_partner_access_guard(&pool, &auth, &slug).await {
        return guard_response;
    }
    // Fehlende Sichtbarkeit bleibt beim bisherigen Wert, statt still auf einen
    // Default zu springen — ein Toggle-Klick darf die Sichtbarkeit nicht ändern.
    let bisher = get_vod_archive_settings(&pool, &slug).await;
    let privacy = payload
        .get("privacy")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or(bisher.privacy);
    if !VOD_ARCHIVE_PRIVACY_VALUES.contains(&privacy.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_privacy", "allowed": VOD_ARCHIVE_PRIVACY_VALUES })),
        )
            .into_response();
    }
    let values = VodArchiveSettings {
        streamer_login: slug,
        enabled: payload.get("enabled").map(coerce_bool).unwrap_or(false),
        privacy,
    };
    let actor = editor_user_id(&auth);
    match set_vod_archive_settings(&pool, &values, actor.as_deref()).await {
        Ok(r) => Json(vod_archive_json(&r)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "db" })),
        )
            .into_response(),
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
    (
        StatusCode::FOUND,
        [(axum::http::header::LOCATION, url.to_string())],
    )
        .into_response()
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
///
/// Der `streamer`-Parameter bleibt hier optional (Verbinden ist nicht
/// zerstörend), [`GLOBAL_SCOPE_MARKER`] zeigt aber wie beim Trennen auf die
/// Sammelverbindung, damit Verbinden und Trennen dieselbe Zeile meinen.
pub async fn oauth_start_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(platform): Path<String>,
    Query(q): Query<StreamerQuery>,
) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), false) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let scope = credential_scope(scope.as_deref()).map(str::to_string);
    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung.
    if let Some(ref login) = scope {
        if let Some(guard_response) = check_partner_access_guard(&pool, &auth, login).await {
            return guard_response;
        }
    }
    if !is_supported_platform(&platform) {
        return (StatusCode::BAD_REQUEST, "Invalid platform").into_response();
    }
    let Some(mgr) = build_oauth_manager(pool) else {
        return redirect_found(&dashboard_url("oauth_error", "oauth_start_failed"));
    };
    let redirect_uri = format!(
        "{}/social-media/oauth/callback/{}",
        oauth_public_origin(),
        platform
    );
    match mgr
        .generate_auth_url(&platform, scope.as_deref(), &redirect_uri)
        .await
    {
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
pub async fn oauth_callback_handler(
    State(pool): State<PgPool>,
    platform: Option<Path<String>>,
    Query(q): Query<OAuthCallbackQuery>,
) -> Response {
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
    let expected_platform = platform
        .as_ref()
        .map(|p| p.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let callback_redirect_uri = expected_platform.as_ref().map(|p| {
        format!(
            "{}/social-media/oauth/callback/{}",
            oauth_public_origin(),
            p
        )
    });
    let Some(mgr) = build_oauth_manager(pool) else {
        return redirect_found(&dashboard_url("oauth_error", "callback_failed"));
    };
    match mgr
        .handle_callback(
            code,
            state,
            expected_platform.as_deref(),
            callback_redirect_uri.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let platform = if is_supported_platform(&result.platform) {
                result.platform
            } else {
                "unknown".to_string()
            };
            redirect_found(&dashboard_url("oauth_success", &platform))
        }
        Err(OAuthError::StateInvalid | OAuthError::RedirectMismatch) => {
            redirect_found(&dashboard_url("oauth_error", "invalid_callback"))
        }
        Err(OAuthError::Exchange { .. }) => {
            redirect_found(&dashboard_url("oauth_error", "token_exchange_failed"))
        }
        Err(_) => redirect_found(&dashboard_url("oauth_error", "callback_failed")),
    }
}

/// `POST /social-media/oauth/disconnect/:platform?streamer=<kanal>` — Plattform
/// trennen.
///
/// Der Parameter ist Pflicht (Admin ohne `streamer` → 400). Vorher schickte das
/// Frontend ihn nie mit, der Scope war für Admin-Sessions damit leer, und
/// „Trennen" für einen einzelnen Kanal kappte die Sammelverbindung für alle
/// Kanäle. Die Sammelverbindung wird jetzt nur noch getrennt, wenn jemand
/// [`GLOBAL_SCOPE_MARKER`] (`?streamer=__global__`) ausdrücklich mitschickt.
pub async fn oauth_disconnect_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(platform): Path<String>,
    Query(q): Query<StreamerQuery>,
) -> Response {
    let scope = match resolve_streamer_scope(&auth, q.streamer.as_deref(), true) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let db_scope = credential_scope(scope.as_deref());
    // Partner-Freigabe-Guard: nach Scope-Auflösung, vor Wirkung. Der
    // Sammel-Marker gehört keinem Kanal, für ihn greift nur die Admin-Prüfung
    // aus `resolve_streamer_scope`.
    if let Some(login) = db_scope {
        if let Some(guard_response) = check_partner_access_guard(&pool, &auth, login).await {
            return guard_response;
        }
    }
    if !is_supported_platform(&platform) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid platform" })),
        )
            .into_response();
    }
    let result = sqlx::query(
        "UPDATE social_media_platform_auth SET enabled = 0 \
         WHERE platform = $1 AND (streamer_login = $2 OR (streamer_login IS NULL AND $2::text IS NULL))",
    )
    .bind(&platform)
    .bind(db_scope)
    .execute(&pool)
    .await;
    match result {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "disconnect_failed" })),
        )
            .into_response(),
    }
}

// ── Partner-Freigabe-API ─────────────────────────────────────────────────

/// Request-Body für PUT /social-media/api/access.
#[derive(Debug, Deserialize)]
pub struct PartnerAccessPutBody {
    streamer_login: String,
    granted: bool,
}

/// GET /social-media/api/access — Liste aller Streamer mit Freigabestatus.
///
/// Admin-only. Liefert alle Einträge aus `social_media_partner_access`.
pub async fn partner_access_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    match list_partner_access(&pool).await {
        Ok(entries) => Json(json!({ "items": entries })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "partner_access_get: DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db_error" })),
            )
                .into_response()
        }
    }
}

/// PUT /social-media/api/access — Freigabe für einen Streamer setzen/entfernen.
///
/// Admin-only. Body: `{ "streamer_login": "...", "granted": true/false }`.
pub async fn partner_access_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<PartnerAccessPutBody>,
) -> Response {
    if let Err(e) = require_admin(&auth) {
        return e;
    }
    let login = body.streamer_login.trim().to_lowercase();
    if login.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "streamer_login is required" })),
        )
            .into_response();
    }
    let actor = editor_user_id(&auth);
    match set_partner_access(&pool, &login, body.granted, actor.as_deref()).await {
        Ok(entry) => Json(json!({ "item": entry })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, login = %login, "partner_access_put: DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db_error" })),
            )
                .into_response()
        }
    }
}

/// Zentraler Partner-Freigabe-Guard: prüft ob `streamer_login` für
/// Social-Media-Schreibzugriffe freigegeben ist. Liefert `Some(Response)`
/// (403) wenn nicht freigegeben, `None` wenn OK.
///
/// Wird vor jedem Schreibpfad aufgerufen: Post, Draft, Caption, Upload, etc.
pub async fn check_partner_access_guard(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    streamer_login: &str,
) -> Option<Response> {
    // Admin arbeitet für jeden Kanal, auch ohne Freigabe-Eintrag: die Freigabe
    // steuert den Selfservice der Partner, nicht das Admin-Werkzeug. Localhost
    // landet ebenfalls hier, es wird in auth/level.rs zu Admin promotet.
    if matches!(auth, DashboardAuthLevel::Admin { .. }) {
        return None;
    }
    if !is_partner_granted(pool, streamer_login).await {
        Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "partner_access_required",
                    "message": "Dieser Streamer ist nicht für Social-Media-Posts freigegeben."
                })),
            )
                .into_response(),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::FromRequest;
    use axum::http::Request;
    use futures_util::StreamExt;
    use std::sync::Mutex;

    #[test]
    fn upload_error_allowlist_deckt_permanente_prepare_und_provider_codes_ab() {
        for code in [
            "source_missing",
            "invalid_source_url",
            "download_failed",
            "render_failed",
            "provider_rejected",
        ] {
            assert_eq!(safe_upload_error_code(code), code);
        }
        assert_eq!(
            safe_upload_error_code("roher Providertext"),
            "upload_failed"
        );
    }

    #[tokio::test]
    async fn enrichment_grenzen_sind_serverseitig_fail_closed() {
        let too_long_title = json!({ "title_youtube": "x".repeat(101) });
        let response = parse_string_field(&too_long_title, "title_youtube", 100).unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_json(response).await;
        assert_eq!(body["error"], "enrichment_limits_exceeded");
        assert_eq!(body["field"], "title_youtube");
        assert_eq!(body["max"], 100);

        let too_many_tags = json!({
            "hashtags_youtube": (0..11).map(|index| format!("tag{index}")).collect::<Vec<_>>()
        });
        let response =
            parse_hashtag_field(&too_many_tags, "hashtags_youtube", 10, 100).unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(response).await["error"],
            "enrichment_limits_exceeded"
        );

        let too_long_tag = json!({ "hashtags_youtube": ["x".repeat(100)] });
        let response = parse_hashtag_field(&too_long_tag, "hashtags_youtube", 10, 100).unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let wrong_type = json!({ "hashtags_youtube": ["ok", 7] });
        let response = parse_hashtag_field(&wrong_type, "hashtags_youtube", 10, 100).unwrap_err();
        assert_eq!(body_json(response).await["error"], "invalid_field");
    }

    #[test]
    fn form_submission_response_maps_all_outcomes() {
        use tb_social_media::forms::FormSubmissionOutcome;

        for (outcome, status, http_status) in [
            (
                FormSubmissionOutcome::Submitted(201),
                "submitted",
                json!(201),
            ),
            (
                FormSubmissionOutcome::Failed(Some(500)),
                "failed",
                json!(500),
            ),
            (FormSubmissionOutcome::Failed(None), "failed", Value::Null),
            (
                FormSubmissionOutcome::SkippedDisabled,
                "skipped_disabled",
                Value::Null,
            ),
            (
                FormSubmissionOutcome::SkippedDuplicate,
                "skipped_duplicate",
                Value::Null,
            ),
        ] {
            assert_eq!(
                form_submission_response(FormKey::DeadlockHigh, outcome),
                json!({
                    "form": "deadlock_high",
                    "status": status,
                    "http_status": http_status,
                })
            );
        }
    }

    /// Serialisiert die Tests, die `OLLAMA_HOST` per `set_var` auf einen toten
    /// Port zeigen (erzwingt deterministischen LLM-Fallback). Ohne den Lock
    /// schreiben parallele Tests gleichzeitig dieselbe Prozess-Env-Var (Race).
    /// Konvention wie in `tb-llm` (`keys.rs`/`ledger.rs`).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "1".to_string(),
            display_name: String::new(),
        }
    }

    fn lazy_test_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost/tb_multipart_route_test")
            .unwrap()
    }

    fn multipart_payload(parts: &[(&str, Option<&str>, &[u8])], boundary: &str) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, filename, value) in parts {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"").as_bytes(),
            );
            if let Some(filename) = filename {
                body.extend_from_slice(format!("; filename=\"{filename}\"").as_bytes());
            }
            body.extend_from_slice(b"\r\n\r\n");
            body.extend_from_slice(value);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        body
    }

    async fn extract_multipart(body: Body, boundary: &str) -> Multipart {
        let request = Request::builder()
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .unwrap();
        Multipart::from_request(request, &()).await.unwrap()
    }

    fn upload_test_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tb-dashboard-upload-{label}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    async fn upload_work_is_empty(root: &FsPath) -> bool {
        let work = root.join(".dashboard-work");
        let Ok(mut entries) = tokio::fs::read_dir(work).await else {
            return true;
        };
        entries.next_entry().await.unwrap().is_none()
    }

    #[test]
    fn mp4_mime_erkennung_und_allowlist() {
        // B15-FIX: ftyp-Box an Offset 4 + Major-Brand → MIME.
        let mp4 = b"\x00\x00\x00\x18ftypisom....";
        assert_eq!(detect_mp4_mime(mp4), Some("video/mp4"));
        assert!(is_accepted_mp4_mime(detect_mp4_mime(mp4).unwrap()));
        // mp42-Brand → video/mp4.
        assert_eq!(
            detect_mp4_mime(b"\x00\x00\x00\x18ftypmp42...."),
            Some("video/mp4")
        );
        // QuickTime-Brand → video/quicktime, aber NICHT akzeptiert (Allowlist).
        assert_eq!(
            detect_mp4_mime(b"\x00\x00\x00\x18ftypqt  ...."),
            Some("video/quicktime")
        );
        assert!(!is_accepted_mp4_mime("video/quicktime"));
        // ftyp NICHT an Offset 4 (nur irgendwo enthalten) → kein MP4 (strenger
        // als der alte Substring-Check).
        assert_eq!(detect_mp4_mime(b"xxxxxxxxftypisom"), None);
        // Zu kurz / Klartext → None.
        assert_eq!(detect_mp4_mime(b"not a video at all"), None);
        assert_eq!(detect_mp4_mime(b"short"), None);
        assert!(!plausible_ftyp_box(mp4, mp4.len() as u64));
        assert!(plausible_ftyp_box(b"\x00\x00\x00\x10ftypisom....", 16,));
        assert!(!plausible_ftyp_box(
            b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81webm....",
            16,
        ));
    }

    #[test]
    fn upload_textfelder_und_parallelitaet_sind_hart_begrenzt() {
        let mut target = Vec::new();
        append_limited_metadata(&mut target, b"123", 5).unwrap();
        assert_eq!(
            append_limited_metadata(&mut target, b"456", 5)
                .unwrap_err()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );

        let permit = manual_upload_semaphore().try_acquire().unwrap();
        assert!(manual_upload_semaphore().try_acquire().is_err());
        drop(permit);
        assert!(manual_upload_semaphore().try_acquire().is_ok());
    }

    #[tokio::test]
    async fn upload_chunks_werden_direkt_in_den_gehaltenen_inode_begrenzt() {
        let root = std::env::temp_dir().join(format!(
            "tb-dashboard-upload-stream-{}",
            uuid::Uuid::new_v4()
        ));
        let mut staged = StagedUpload::create(&root).await.unwrap();
        write_staged_chunk(&mut staged, b"123", 5).await.unwrap();
        assert_eq!(
            write_staged_chunk(&mut staged, b"456", 5)
                .await
                .unwrap_err()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(staged.length, 3);
        staged.file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
        let mut bytes = Vec::new();
        staged.file.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"123");
        staged.cleanup().await;
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn upload_verzeichnisse_und_inodes_bleiben_no_clobber_und_hart_begrenzt() {
        use std::os::unix::fs::MetadataExt;

        let root = upload_test_root("inode-contract");
        let destination = open_or_create_upload_streamer_directory(&root, "nani")
            .await
            .unwrap();
        assert_eq!(
            destination.handle.metadata().await.unwrap().mode() & 0o7777,
            0o2770,
            "Neue Streamer-Verzeichnisse müssen trotz UMask gruppenschreibbar und setgid sein"
        );

        let mut first = StagedUpload::create(&root).await.unwrap();
        write_staged_chunk(&mut first, b"erster-inode", 1024)
            .await
            .unwrap();
        first.file.sync_all().await.unwrap();
        validate_staged_upload_inode(&first).await.unwrap();
        let mut second = StagedUpload::create(&root).await.unwrap();
        write_staged_chunk(&mut second, b"zweiter-inode", 1024)
            .await
            .unwrap();
        second.file.sync_all().await.unwrap();
        validate_staged_upload_inode(&second).await.unwrap();
        let final_name = std::ffi::OsStr::new("manual:test.mp4");
        link_open_file_noreplace(&first.file, &destination, final_name).unwrap();
        assert_eq!(
            link_open_file_noreplace(&second.file, &destination, final_name)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::AlreadyExists
        );
        first.cleanup().await;
        second.cleanup().await;
        assert_eq!(
            tokio::fs::read(destination.logical_path.join(final_name))
                .await
                .unwrap(),
            b"erster-inode"
        );

        let mut linked = StagedUpload::create(&root).await.unwrap();
        write_staged_chunk(&mut linked, b"hardlink", 1024)
            .await
            .unwrap();
        linked.file.sync_all().await.unwrap();
        let extra_link = linked
            .work_directory
            .logical_path
            .join("unerwarteter-hardlink.mp4");
        std::fs::hard_link(
            linked.work_directory.logical_path.join(&linked.temp_name),
            &extra_link,
        )
        .unwrap();
        assert_eq!(
            validate_staged_upload_inode(&linked)
                .await
                .unwrap_err()
                .status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(linked.file.metadata().await.unwrap().nlink(), 2);
        linked.cleanup().await;
        std::fs::remove_file(extra_link).unwrap();

        let outside = root.join("outside.txt");
        std::fs::write(&outside, b"unveraendert").unwrap();
        let symlink_name = destination.logical_path.join("manual:symlink.mp4");
        std::os::unix::fs::symlink(&outside, &symlink_name).unwrap();
        let staged = StagedUpload::create(&root).await.unwrap();
        assert_eq!(
            link_open_file_noreplace(
                &staged.file,
                &destination,
                std::ffi::OsStr::new("manual:symlink.mp4")
            )
            .unwrap_err()
            .kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(&outside).unwrap(), b"unveraendert");
        staged.cleanup().await;

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn multipart_route_begrenzt_datei_metadaten_und_raeumt_partialdatei_auf() {
        let pool = lazy_test_pool();
        let semaphore = Semaphore::new(1);
        let timing = UploadTiming {
            wallclock: std::time::Duration::from_secs(2),
            idle: std::time::Duration::from_secs(1),
        };
        let limits = UploadLimits {
            file_bytes: 5,
            text_bytes: 5,
            slug_bytes: 16,
        };

        let root = upload_test_root("route-limit");
        let boundary = "tb-limit-boundary";
        let payload = multipart_payload(&[("file", Some("clip.mp4"), b"123456")], boundary);
        let response = upload_clip_handler_inner(
            DashboardAuthLevel::admin(),
            pool.clone(),
            extract_multipart(Body::from(payload), boundary).await,
            root.to_str().unwrap(),
            &semaphore,
            timing,
            limits,
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(upload_work_is_empty(&root).await);

        let boundary = "tb-metadata-boundary";
        let payload = multipart_payload(&[("title", None, b"123456")], boundary);
        let response = upload_clip_handler_inner(
            DashboardAuthLevel::admin(),
            pool,
            extract_multipart(Body::from(payload), boundary).await,
            root.to_str().unwrap(),
            &semaphore,
            timing,
            limits,
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(upload_work_is_empty(&root).await);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn multipart_route_lehnt_unbekannte_und_doppelte_felder_ab() {
        let pool = lazy_test_pool();
        let semaphore = Semaphore::new(1);
        let timing = UploadTiming::production();
        let limits = UploadLimits::production();
        let root = upload_test_root("route-fields");

        let boundary = "tb-unknown-boundary";
        let payload = multipart_payload(&[("unexpected", None, b"x")], boundary);
        let response = upload_clip_handler_inner(
            DashboardAuthLevel::admin(),
            pool.clone(),
            extract_multipart(Body::from(payload), boundary).await,
            root.to_str().unwrap(),
            &semaphore,
            timing,
            limits,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let boundary = "tb-duplicate-boundary";
        let payload = multipart_payload(
            &[
                ("file", Some("clip.mp4"), b"abc"),
                ("title", None, b"eins"),
                ("title", None, b"zwei"),
            ],
            boundary,
        );
        let response = upload_clip_handler_inner(
            DashboardAuthLevel::admin(),
            pool,
            extract_multipart(Body::from(payload), boundary).await,
            root.to_str().unwrap(),
            &semaphore,
            timing,
            limits,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(upload_work_is_empty(&root).await);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn langsamer_multipart_upload_blockiert_nur_einen_slot_und_raeumt_auf() {
        let pool = lazy_test_pool();
        let semaphore = Arc::new(Semaphore::new(1));
        let timing = UploadTiming {
            wallclock: std::time::Duration::from_millis(250),
            idle: std::time::Duration::from_millis(50),
        };
        let limits = UploadLimits::production();
        let root = upload_test_root("route-timeout");
        let boundary = "tb-slow-boundary";
        let first_chunk = Bytes::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.mp4\"\r\n\r\nabc\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\n"
        ));
        let closing = Bytes::from(format!("spaet\r\n--{boundary}--\r\n"));
        let stream = futures_util::stream::once(async move {
            Ok::<Bytes, std::convert::Infallible>(first_chunk)
        })
        .chain(futures_util::stream::once(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            Ok::<Bytes, std::convert::Infallible>(closing)
        }));
        let multipart = extract_multipart(Body::from_stream(stream), boundary).await;
        let first_pool = pool.clone();
        let first_semaphore = semaphore.clone();
        let first_root = root.clone();
        let first = tokio::spawn(async move {
            upload_clip_handler_inner(
                DashboardAuthLevel::admin(),
                first_pool,
                multipart,
                first_root.to_str().unwrap(),
                first_semaphore.as_ref(),
                timing,
                limits,
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while semaphore.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let second_boundary = "tb-busy-boundary";
        let second_payload = multipart_payload(&[("unexpected", None, b"x")], second_boundary);
        let second = upload_clip_handler_inner(
            DashboardAuthLevel::admin(),
            pool,
            extract_multipart(Body::from(second_payload), second_boundary).await,
            root.to_str().unwrap(),
            semaphore.as_ref(),
            timing,
            limits,
        )
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(first.await.unwrap().status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(semaphore.available_permits(), 1);
        assert!(upload_work_is_empty(&root).await);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn editor_actor_aus_partner_session() {
        // B15-FIX: Partner → twitch_user_id; Admin/Localhost/None → None.
        let p = DashboardAuthLevel::Partner {
            twitch_login: "nani".into(),
            twitch_user_id: "777".into(),
            display_name: "NaNi".into(),
        };
        assert_eq!(editor_user_id(&p).as_deref(), Some("777"));
        assert_eq!(editor_user_id(&DashboardAuthLevel::admin()), None);
        assert_eq!(editor_user_id(&DashboardAuthLevel::admin()), None);
        assert_eq!(editor_user_id(&DashboardAuthLevel::None), None);
        // Leere/whitespace user_id → None.
        let empty = DashboardAuthLevel::Partner {
            twitch_login: "x".into(),
            twitch_user_id: "  ".into(),
            display_name: String::new(),
        };
        assert_eq!(editor_user_id(&empty), None);
    }

    #[test]
    fn scope_partner_und_admin() {
        // Partner: eigener Login, Cross-Account → 403.
        assert_eq!(
            resolve_streamer_scope(&partner("Nani"), None, false).unwrap(),
            Some("nani".to_string())
        );
        assert_eq!(
            resolve_streamer_scope(&partner("Nani"), Some("nani"), false).unwrap(),
            Some("nani".to_string())
        );
        assert!(resolve_streamer_scope(&partner("Nani"), Some("other"), false).is_err());
        // Admin: frei wählbar / None.
        assert_eq!(
            resolve_streamer_scope(&DashboardAuthLevel::admin(), None, false).unwrap(),
            None
        );
        assert_eq!(
            resolve_streamer_scope(&DashboardAuthLevel::admin(), Some("xyz"), false).unwrap(),
            Some("xyz".to_string())
        );
        // required ohne requested → 400.
        assert!(resolve_streamer_scope(&DashboardAuthLevel::admin(), None, true).is_err());
        // None-Auth → Fehler (401).
        assert!(resolve_streamer_scope(&DashboardAuthLevel::None, None, false).is_err());
    }

    #[tokio::test]
    async fn index_unauth_redirectet_zum_login() {
        // B15-FIX-index-redirect: None → 302/303-Redirect, kein 401-JSON.
        let resp = index_handler(DashboardAuthLevel::None, "/social-media".parse().unwrap()).await;
        assert!(
            resp.status().is_redirection(),
            "unauth HTML-Index muss redirecten, war {}",
            resp.status()
        );
        assert_eq!(
            resp.headers().get(axum::http::header::LOCATION).unwrap(),
            SOCIAL_MEDIA_LOGIN_URL
        );

        let legacy = index_handler(
            DashboardAuthLevel::None,
            "/social-media-admin".parse().unwrap(),
        )
        .await;
        assert_eq!(
            legacy.headers().get(axum::http::header::LOCATION).unwrap(),
            SOCIAL_MEDIA_ADMIN_LOGIN_URL
        );
    }

    #[test]
    fn preparation_json_gibt_keine_rohen_subprozessdetails_aus() {
        let record = ClipPreparationRecord {
            clip_db_id: 7,
            state: "failed".to_string(),
            source_fingerprint: None,
            render_fingerprint: None,
            render_path: None,
            error_code: Some("render_failed".to_string()),
            error_message: Some("ffmpeg /srv/private/clip.mp4 token=geheim".to_string()),
            requested_at: "2026-09-01T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
            updated_at: "2026-09-01T00:00:01Z".to_string(),
        };
        let payload = preparation_json(&record);
        assert_eq!(
            payload["error_message"],
            "Die Vorschau konnte nicht erstellt werden."
        );
        assert!(!payload.to_string().contains("/srv/private"));
        assert!(!payload.to_string().contains("geheim"));
    }

    #[test]
    fn media_range_parser_deckt_206_und_416_faelle_ab() {
        assert_eq!(parse_byte_range("bytes=0-3", 10), Ok((0, 3)));
        assert_eq!(parse_byte_range("bytes=4-", 10), Ok((4, 9)));
        assert_eq!(parse_byte_range("bytes=-4", 10), Ok((6, 9)));
        assert!(parse_byte_range("bytes=10-", 10).is_err());
        assert!(parse_byte_range("bytes=8-2", 10).is_err());
        assert!(parse_byte_range("bytes=0-1,4-5", 10).is_err());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn media_open_lehnt_symlinkendes_renderverzeichnis_ab() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tb_sm_render_dir_{nonce}"));
        let outside = std::env::temp_dir().join(format!("tb_sm_render_outside_{nonce}"));
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(outside.join("7-preview.mp4"), b"privat")
            .await
            .unwrap();
        let rendered_link = root.join("rendered");
        std::os::unix::fs::symlink(&outside, &rendered_link).unwrap();

        let result = open_render_file_from_dir_no_follow(
            &rendered_link,
            std::ffi::OsStr::new("7-preview.mp4"),
        )
        .await;
        assert!(result.is_err());

        let _ = std::fs::remove_file(rendered_link);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    /// Der OAuth-Umweg endet auf `/social-media?oauth_success=…`. Faellt der
    /// Query-String bei der Weiterleitung auf die SPA weg, gibt es nach einem
    /// Verbindungsversuch ueberhaupt keine Rueckmeldung mehr.
    #[tokio::test]
    async fn index_reicht_die_oauth_rueckmeldung_weiter() {
        async fn ziel(pfad: &str) -> String {
            let resp = index_handler(DashboardAuthLevel::admin(), pfad.parse().unwrap()).await;
            resp.headers()
                .get(axum::http::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        }

        assert_eq!(
            ziel("/social-media?oauth_success=youtube").await,
            "/social-media-admin?oauth_success=youtube"
        );
        assert_eq!(
            ziel("/social-media?oauth_error=token_exchange_failed").await,
            "/social-media-admin?oauth_error=token_exchange_failed"
        );
        assert_eq!(ziel("/social-media").await, "/social-media-admin");
        // Leerer Query-String haengt kein nacktes '?' an.
        assert_eq!(ziel("/social-media?").await, "/social-media-admin");
    }

    #[test]
    fn weitergereichte_query_nimmt_nur_die_erlaubnisliste() {
        let q = |p: &str| weitergereichte_query(&p.parse::<Uri>().unwrap());
        assert_eq!(
            q("/x?oauth_success=youtube"),
            Some("oauth_success=youtube".to_string())
        );
        assert_eq!(
            q("/x?oauth_error=token_exchange_failed"),
            Some("oauth_error=token_exchange_failed".to_string())
        );
        // Fremde Parameter fallen weg, auch neben einem erlaubten.
        assert_eq!(
            q("/x?next=%2Fboese&oauth_success=tiktok&a=1"),
            Some("oauth_success=tiktok".to_string())
        );
        assert_eq!(q("/x?a=1&b=2"), None);
        assert_eq!(q("/x"), None);
        assert_eq!(q("/x?"), None);
        // Werte ausserhalb der erwarteten Form fallen weg.
        assert_eq!(q("/x?oauth_success=you%20tube"), None);
        assert_eq!(q("/x?oauth_success="), None);
        let lang = format!("/x?oauth_success={}", "b".repeat(600));
        assert_eq!(q(&lang), None);
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
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT UNIQUE NOT NULL, clip_url TEXT NOT NULL DEFAULT '', clip_thumbnail_url TEXT, streamer_login TEXT NOT NULL, twitch_user_id TEXT, status TEXT DEFAULT 'pending', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), duration_seconds DOUBLE PRECISION, view_count INTEGER, clip_title TEXT, game_name TEXT, game_id TEXT, category_key TEXT NOT NULL DEFAULT 'other', source_kind TEXT NOT NULL DEFAULT 'twitch', upload_local_path TEXT, local_file_path TEXT, custom_description TEXT, hashtags TEXT, layout_override_json JSONB, retention_until TIMESTAMPTZ, discarded_at TIMESTAMPTZ, kontingent_verbraucht_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_uploaded_at TIMESTAMPTZ, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ, approved_render_fingerprint TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)",
            "CREATE TABLE clip_templates_streamer (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, template_name TEXT, description_template TEXT NOT NULL, hashtags TEXT NOT NULL DEFAULT '[]', is_default BOOLEAN DEFAULT FALSE, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, UNIQUE (streamer_login, template_name))",
            "CREATE TABLE clip_templates_global (id BIGSERIAL PRIMARY KEY, template_name TEXT UNIQUE, description_template TEXT NOT NULL, hashtags TEXT NOT NULL DEFAULT '[]', category TEXT, usage_count INTEGER DEFAULT 0, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, created_by TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', aliases JSONB NOT NULL DEFAULT '[]'::jsonb, weight INTEGER NOT NULL DEFAULT 1, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE twitch_clip_form_submissions (id SERIAL PRIMARY KEY, clip_id INTEGER NOT NULL, form_key TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', http_status INTEGER, error TEXT, submitted_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), UNIQUE (clip_id, form_key))",
            "CREATE TABLE twitch_clips_social_analytics (id BIGSERIAL PRIMARY KEY, clip_id BIGINT, platform TEXT, bucket TEXT, views INTEGER, likes INTEGER, comments INTEGER, shares INTEGER, watch_time_seconds INTEGER, ctr_percent NUMERIC(5,2), engagement_rate DOUBLE PRECISION, provider TEXT, synced_at TIMESTAMPTZ, next_pull_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_reports (id SERIAL PRIMARY KEY, kind TEXT NOT NULL, streamer_login TEXT, period_start TIMESTAMPTZ NOT NULL, period_end TIMESTAMPTZ NOT NULL, content_md TEXT NOT NULL, model TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)",
            "CREATE TABLE social_media_partner_access (streamer_login TEXT PRIMARY KEY, granted BOOLEAN NOT NULL DEFAULT FALSE, granted_by TEXT, granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE social_media_category (category_key TEXT PRIMARY KEY, display_name TEXT NOT NULL, twitch_game_id TEXT, match_game_names TEXT[] NOT NULL DEFAULT '{}', enrichment_enabled BOOLEAN NOT NULL DEFAULT FALSE, sort_order INTEGER NOT NULL DEFAULT 100, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "INSERT INTO social_media_category (category_key, display_name, match_game_names, enrichment_enabled, sort_order) VALUES ('deadlock', 'Deadlock', ARRAY['deadlock'], TRUE, 10), ('other', 'Andere Spiele', ARRAY[]::TEXT[], FALSE, 900)",
            "CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, approval_mode TEXT NOT NULL DEFAULT 'manual', timezone TEXT NOT NULL DEFAULT 'Europe/Berlin', release_mode TEXT NOT NULL DEFAULT 'prepare_only', updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY, state TEXT NOT NULL DEFAULT 'pending', lease_token TEXT, source_fingerprint TEXT, render_fingerprint TEXT, render_path TEXT, error_code TEXT, error_message TEXT, requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE social_media_platform_schedule (streamer_login TEXT NOT NULL, platform TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT FALSE, posts_per_week INTEGER NOT NULL DEFAULT 4, max_posts_per_day INTEGER NOT NULL DEFAULT 1, post_times JSONB NOT NULL DEFAULT '[\"18:00\"]'::jsonb, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_by TEXT, PRIMARY KEY (streamer_login, platform))",
            "CREATE TABLE social_media_category_settings (streamer_login TEXT NOT NULL, category_key TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT FALSE, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_by TEXT, PRIMARY KEY (streamer_login, category_key))",
            "CREATE TABLE social_media_vod_archive (streamer_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT FALSE, privacy TEXT NOT NULL DEFAULT 'private', updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_by TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn preparation_api_und_range_media_bleiben_plattformfrei_und_gescopt() {
        let Some(pool) = make_pool("t_dash_sm_preparation_api").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) \
             VALUES ('prepare-api', 'https://clips.twitch.tv/PrepareApi', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, state) VALUES ($1, 'pending')",
        )
        .bind(clip_db_id)
        .execute(&pool)
        .await
        .unwrap();

        let get_response = clip_preparation_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip_db_id.to_string()),
        )
        .await;
        assert_eq!(get_response.status(), StatusCode::OK);
        let payload = body_json(get_response).await;
        let keys: std::collections::HashSet<&str> = payload
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::HashSet::from([
                "clip_db_id",
                "state",
                "source_ready",
                "preview_ready",
                "preview_url",
                "download_url",
                "error_code",
                "error_message",
                "requested_at",
                "started_at",
                "completed_at",
                "updated_at",
            ])
        );
        assert_eq!(payload["state"], "pending");

        let without_preparation: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) \
             VALUES ('prepare-get-readonly', 'https://clips.twitch.tv/Readonly', 'nani') \
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let readonly_get = clip_preparation_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(without_preparation.to_string()),
        )
        .await;
        assert_eq!(readonly_get.status(), StatusCode::NOT_FOUND);
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(without_preparation)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 0, "GET darf keine Preparation-Zeile anlegen");

        let post_response = clip_preparation_post_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip_db_id.to_string()),
        )
        .await;
        assert_eq!(post_response.status(), StatusCode::ACCEPTED);
        assert_eq!(body_json(post_response).await["state"], "pending");

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let rendered_dir = FsPath::new(DEFAULT_CLIPS_DIR).join("rendered");
        tokio::fs::create_dir_all(&rendered_dir).await.unwrap();
        let media_path = rendered_dir.join(format!("{clip_db_id}-{nonce}.mp4"));
        tokio::fs::write(&media_path, b"0123456789").await.unwrap();
        sqlx::query(
            "UPDATE social_media_clip_preparation SET state = 'preview_ready', \
             source_fingerprint = 'source', render_fingerprint = 'render', render_path = $2, \
             completed_at = NOW(), updated_at = NOW() WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .bind(media_path.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();

        let unauthenticated = clip_preparation_media_handler(
            DashboardAuthLevel::None,
            State(pool.clone()),
            Path(clip_db_id.to_string()),
            Query(PreparationMediaQuery::default()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        sqlx::query(
            "INSERT INTO social_media_partner_access (streamer_login, granted) \
             VALUES ('other', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let out_of_scope = clip_preparation_media_handler(
            sm_partner("other"),
            State(pool.clone()),
            Path(clip_db_id.to_string()),
            Query(PreparationMediaQuery::default()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(out_of_scope.status(), StatusCode::FORBIDDEN);

        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=2-5"));
        let response = clip_preparation_media_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip_db_id.to_string()),
            Query(PreparationMediaQuery::default()),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 2-5/10"
        );
        assert_eq!(
            response.headers().get(header::ACCEPT_RANGES).unwrap(),
            "bytes"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "private, no-store"
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 16)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"2345");

        let mut invalid_headers = HeaderMap::new();
        invalid_headers.insert(header::RANGE, HeaderValue::from_static("bytes=99-"));
        let invalid = clip_preparation_media_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip_db_id.to_string()),
            Query(PreparationMediaQuery::default()),
            invalid_headers,
        )
        .await;
        assert_eq!(invalid.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            invalid.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes */10"
        );
        assert_eq!(
            invalid.headers().get(header::CACHE_CONTROL).unwrap(),
            "private, no-store"
        );

        #[cfg(unix)]
        {
            let outside = std::env::temp_dir().join(format!("tb_sm_outside_{nonce}.mp4"));
            tokio::fs::write(&outside, b"outside").await.unwrap();
            let link = rendered_dir.join(format!("{clip_db_id}-{nonce}-link.mp4"));
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            sqlx::query(
                "UPDATE social_media_clip_preparation SET render_path = $2 WHERE clip_db_id = $1",
            )
            .bind(clip_db_id)
            .bind(link.to_string_lossy().as_ref())
            .execute(&pool)
            .await
            .unwrap();
            let symlink_response = clip_preparation_media_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path(clip_db_id.to_string()),
                Query(PreparationMediaQuery::default()),
                HeaderMap::new(),
            )
            .await;
            assert_eq!(symlink_response.status(), StatusCode::NOT_FOUND);
            let _ = std::fs::remove_file(link);
            let _ = std::fs::remove_file(outside);
        }
        let _ = std::fs::remove_file(media_path);
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn sm_partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    #[tokio::test]
    async fn partner_ohne_freigabe_bekommt_403() {
        let Some(pool) = make_pool("t_dash_sm_access_deny").await else {
            return;
        };
        let err = require_sm_access(&sm_partner("earlysalty"), &pool, None)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);

        // Auch ein explizit auf false gesetzter Eintrag bleibt gesperrt.
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('earlysalty', FALSE)")
            .execute(&pool)
            .await
            .unwrap();
        let err = require_sm_access(&sm_partner("earlysalty"), &pool, None)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn partner_mit_freigabe_bekommt_eigenen_scope_und_kein_fremdes_konto() {
        let Some(pool) = make_pool("t_dash_sm_access_grant").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('earlysalty', TRUE)")
            .execute(&pool)
            .await
            .unwrap();

        let scope = require_sm_access(&sm_partner("EarlySalty"), &pool, None)
            .await
            .unwrap();
        assert_eq!(scope, Some("earlysalty".to_string()));

        let err = require_sm_access(&sm_partner("earlysalty"), &pool, Some("ismile_e"))
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_bleibt_ungebremst_und_none_ist_401() {
        let Some(pool) = make_pool("t_dash_sm_access_admin").await else {
            return;
        };
        let scope = require_sm_access(&DashboardAuthLevel::admin(), &pool, Some("ismile_e"))
            .await
            .unwrap();
        assert_eq!(scope, Some("ismile_e".to_string()));

        let err = require_sm_access(&DashboardAuthLevel::None, &pool, None)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    /// Die Pipeline-Seite baut sich aus drei Abfragen auf. Reißt eine davon mit 403
    /// ab, zeigt das Dashboard dem Partner trotz Freigabe wieder die Admin-Wand.
    #[tokio::test]
    async fn freigegebener_partner_kommt_durch_alle_drei_einstiegsabfragen() {
        let Some(pool) = make_pool("t_dash_sm_access_entry").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('earlysalty', TRUE)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('earlysalty', '1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let resp = streamer_layout_get_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("earlysalty".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "Layout-Abfrage");

        let resp = admin_clips_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Query(AdminClipsQuery {
                page: None,
                page_size: None,
                status: None,
                streamer: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "Clip-Liste");

        let resp = posting_plan_get_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("earlysalty".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "Zeitplan des eigenen Kanals");

        // Ohne Freigabe bleibt dieselbe Abfrage zu.
        let resp = posting_plan_get_handler(
            sm_partner("ismile_e"),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("ismile_e".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Der Zeitplan gehoert dem Kanal: ein freigegebener Partner darf den
        // eigenen setzen, den eines fremden Kanals nicht.
        let resp = posting_plan_put_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("earlysalty".into()),
            }),
            "{\"approval_mode\":\"full_auto\"}".to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "eigener Zeitplan");

        let resp = posting_plan_put_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("ismile_e".into()),
            }),
            "{\"approval_mode\":\"full_auto\"}".to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "fremder Zeitplan");
    }

    #[tokio::test]
    async fn admin_clips_partner_sieht_nur_eigene_clips() {
        let Some(pool) = make_pool("t_dash_sm_access_clips").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('earlysalty', TRUE)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status) VALUES ('eigen', 'earlysalty', 'pending'), ('fremd', 'ismile_e', 'pending')")
            .execute(&pool)
            .await
            .unwrap();

        // Partner fragt ohne Filter: der Scope schneidet fremde Clips weg.
        let resp = admin_clips_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Query(AdminClipsQuery {
                page: None,
                page_size: None,
                status: None,
                streamer: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["clip_id"], "eigen");

        // Der fremde Clip ist auch einzeln nicht erreichbar.
        let fremd_id: i64 =
            sqlx::query_scalar("SELECT id FROM twitch_clips_social_media WHERE clip_id = 'fremd'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let resp = admin_clip_detail_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Path(fremd_id.to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn clips_handler_liste_und_limit() {
        let Some(pool) = make_pool("t_dash_sm_clips").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status, created_at) VALUES ('c1', 'nani', 'pending', '2026-06-10')").execute(&pool).await.unwrap();

        // Happy-Path: Admin sieht den Clip.
        let resp = clips_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ClipsQuery {
                streamer: None,
                status: None,
                limit: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["clip_id"], "c1");
        assert_eq!(v[0]["pending_uploads"], 0);

        // Ungültiges Limit → 400.
        let resp = clips_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ClipsQuery {
                streamer: None,
                status: None,
                limit: Some("999".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Partner mit fremdem streamer → 403.
        let resp = clips_handler(
            partner("nani"),
            State(pool.clone()),
            Query(ClipsQuery {
                streamer: Some("other".into()),
                status: None,
                limit: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_template_handler_validierung() {
        let Some(pool) = make_pool("t_dash_sm_tpl_create").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('nani', TRUE)")
            .execute(&pool)
            .await
            .unwrap();
        // Partner legt eigenes Template an.
        let resp = create_template_handler(
            partner("nani"),
            State(pool.clone()),
            Json(CreateTemplateBody {
                streamer: None,
                template_name: Some("Default".into()),
                description: Some("Desc {{title}}".into()),
                hashtags: vec!["deadlock".into()],
                is_default: true,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["success"], true);
        let tid = v["template_id"].as_i64().unwrap();
        let row: (String, String, bool) = sqlx::query_as("SELECT streamer_login, template_name, is_default FROM clip_templates_streamer WHERE id = $1").bind(tid).fetch_one(&pool).await.unwrap();
        assert_eq!(row, ("nani".to_string(), "Default".to_string(), true));

        // Fehlende description → 400.
        let resp = create_template_handler(
            partner("nani"),
            State(pool.clone()),
            Json(CreateTemplateBody {
                streamer: None,
                template_name: Some("X".into()),
                description: None,
                hashtags: vec![],
                is_default: false,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Admin ohne streamer (required) → 400.
        let resp = create_template_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(CreateTemplateBody {
                streamer: None,
                template_name: Some("X".into()),
                description: Some("Y".into()),
                hashtags: vec![],
                is_default: false,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_template_handler_ownership() {
        let Some(pool) = make_pool("t_dash_sm_tpl_apply").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, clip_title, game_name) VALUES ('c1', 'nani', 'Titel', 'Deadlock') RETURNING id").fetch_one(&pool).await.unwrap();
        let tpl: i64 = sqlx::query_scalar("INSERT INTO clip_templates_streamer (streamer_login, template_name, description_template, hashtags) VALUES ('nani', 'T', 'Beschr {{title}}', '[\"a\"]') RETURNING id").fetch_one(&pool).await.unwrap();

        // Ohne Freigabe kommt der Partner gar nicht erst durch.
        let resp = apply_template_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(ApplyTemplateBody {
                clip_id: Some(json!(clip)),
                template_id: Some(json!(tpl)),
                is_global: false,
                streamer: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Beide Partner freigeben, damit hier wirklich die Clip-Zugehörigkeit greift.
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('nani', TRUE), ('other', TRUE)")
            .execute(&pool)
            .await
            .unwrap();

        // Partner wendet eigenes Template auf eigenen Clip an → success.
        let resp = apply_template_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(ApplyTemplateBody {
                clip_id: Some(json!(clip)),
                template_id: Some(json!(tpl)),
                is_global: false,
                streamer: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["success"], true);

        // Fehlende IDs → 400.
        let resp = apply_template_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(ApplyTemplateBody {
                clip_id: None,
                template_id: Some(json!(tpl)),
                is_global: false,
                streamer: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Fremder Clip (Partner other besitzt clip nicht) → 403.
        let resp = apply_template_handler(
            partner("other"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(ApplyTemplateBody {
                clip_id: Some(json!(clip)),
                template_id: Some(json!(tpl)),
                is_global: false,
                streamer: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn templates_get_handler_shapes() {
        let Some(pool) = make_pool("t_dash_sm_tpl_get").await else {
            return;
        };
        // Globale Templates (eines mit Kategorie).
        sqlx::query("INSERT INTO clip_templates_global (template_name, description_template, hashtags, category, usage_count) VALUES ('G1', 'd', '[\"a\",\"b\"]', 'gaming', 5), ('G2', 'd', '[]', NULL, 1)").execute(&pool).await.unwrap();
        // Streamer-Templates für nani (eines default).
        sqlx::query("INSERT INTO clip_templates_streamer (streamer_login, template_name, description_template, hashtags, is_default) VALUES ('nani', 'S1', 'd', '[\"x\"]', TRUE), ('nani', 'S2', 'd', '[]', FALSE)").execute(&pool).await.unwrap();

        // global: alle 2, hashtags als Array.
        let resp = templates_global_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(CategoryQuery { category: None }),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["templates"].as_array().unwrap().len(), 2);
        let g1 = &v["templates"][0]; // usage_count DESC → G1 (5) zuerst
        assert_eq!(g1["template_name"], "G1");
        assert_eq!(g1["hashtags"], json!(["a", "b"]));
        // Kategorie-Filter.
        let resp = templates_global_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(CategoryQuery {
                category: Some("gaming".into()),
            }),
        )
        .await;
        assert_eq!(
            body_json(resp).await["templates"].as_array().unwrap().len(),
            1
        );

        // Ohne Freigabe sieht ein Partner seine Templates nicht mehr.
        let resp = templates_streamer_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('nani', TRUE)")
            .execute(&pool)
            .await
            .unwrap();

        // streamer: Partner nani sieht 2, is_default als int (1/0), default zuerst.
        let resp = templates_streamer_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["templates"].as_array().unwrap().len(), 2);
        assert_eq!(v["templates"][0]["template_name"], "S1"); // is_default DESC
        assert_eq!(v["templates"][0]["is_default"], 1); // int, nicht true
        assert_eq!(v["templates"][1]["is_default"], 0);

        // None-Auth → 401.
        let resp = templates_global_handler(
            DashboardAuthLevel::None,
            State(pool.clone()),
            Query(CategoryQuery { category: None }),
        )
        .await;
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
        let Some(pool) = make_pool("t_dash_sm_layout").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('nani', '1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // GET ohne gespeichertes Layout → Default + is_default true.
        let resp = streamer_layout_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("nani".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["is_default"], true);
        assert_eq!(v["mode"], "pip");

        // PUT setzt ein Layout (mode stacked).
        let resp = streamer_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(json!({ "streamer_login": "nani", "layout": valid_layout(), "mode": "stacked", "cam_enabled": false })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["mode"], "stacked");

        // GET jetzt → is_default false, mode stacked.
        let resp = streamer_layout_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("nani".into()),
            }),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["is_default"], false);
        assert_eq!(v["mode"], "stacked");
        assert_eq!(v["cam_enabled"], false);

        // Unbekannter Streamer → 404.
        let resp = streamer_layout_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("ghost".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // Partner → 403.
        let resp = streamer_layout_get_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("nani".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        // PUT ohne layout-Key → 400 invalid_layout.
        let resp = streamer_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(json!({ "streamer_login": "nani" })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Neue Eingaben werden streng gegen den Zielframe 1080x1920 geprueft:
        // die Kulanz beim Lesen alter Layouts darf hier nicht durchschlagen.
        let mut zu_breit = valid_layout();
        zu_breit["cam_position"] = json!({"x": 126, "y": 0, "w": 1080, "h": 540});
        let resp = streamer_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(json!({ "streamer_login": "nani", "layout": zu_breit })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(body_json(resp).await["message"]
            .as_str()
            .unwrap_or_default()
            .contains("cam_position"));

        // Ein hoher Cam-Streifen passt dagegen: 1600 < 1920 (Zielframe), auch
        // wenn die Quelle nur 1080 hoch ist.
        let mut hoch = valid_layout();
        hoch["cam_position"] = json!({"x": 0, "y": 0, "w": 1080, "h": 1600});
        let resp = streamer_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(json!({ "streamer_login": "nani", "layout": hoch, "mode": "stacked" })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn clip_layout_put_set_und_clear() {
        let Some(pool) = make_pool("t_dash_sm_clip_layout").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('c1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // Override setzen.
        let resp = clip_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            Json(json!({ "layout": valid_layout() })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert!(!v["layout_override"].is_null());
        assert!(v["effective_layout"]["version"] == 1);

        // Override löschen (layout null).
        let resp = clip_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            Json(json!({ "layout": Value::Null })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_json(resp).await["layout_override"].is_null());

        // Ungültige clip_db_id (Pfad) → 400.
        let resp = clip_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("abc".into()),
            Json(json!({})),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Nicht existierender Clip → 404.
        let resp = clip_layout_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("99999".into()),
            Json(json!({})),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn vocab_crud_und_seed() {
        let Some(pool) = make_pool("t_dash_sm_vocab").await else {
            return;
        };

        // Upsert: gültiger Eintrag (Kategorie hero).
        let resp = vocab_upsert_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(json!({ "term": "Haze", "canonical": "Haze", "category": "hero", "aliases": ["hayz"], "weight": 3 })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["term"], "haze"); // normalisiert
        assert_eq!(v["weight"], 3);

        // Ungültige Kategorie → 400 invalid_vocab.
        let resp = vocab_upsert_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(json!({ "term": "x", "canonical": "X", "category": "bogus" })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Nicht-Objekt-Body → 400 invalid_payload.
        let resp = vocab_upsert_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(json!([1, 2])),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // List: findet den Eintrag.
        let resp = vocab_list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(VocabListQuery {
                page: None,
                page_size: None,
                category: None,
                q: None,
            }),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["items"][0]["term"], "haze");
        // Ungültige Pagination → 400.
        let resp = vocab_list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(VocabListQuery {
                page: Some("abc".into()),
                page_size: None,
                category: None,
                q: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Delete: vorhanden → 204, dann → 404.
        let resp = vocab_delete_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("haze".into()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = vocab_delete_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("haze".into()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Seed nur Slang (include_api=false → kein Netzwerk) → 25 geschrieben.
        let resp = vocab_seed_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"include_api\": false}".to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["written"], 25);
        assert_eq!(v["inserted"], 25);

        // Partner → 403 (admin-only).
        let resp = vocab_list_handler(
            partner("nani"),
            State(pool.clone()),
            Query(VocabListQuery {
                page: None,
                page_size: None,
                category: None,
                q: None,
            }),
        )
        .await;
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
            provider_calls_enabled: false,
        };
        // Scope + globaler Fallback → maskiert.
        let v = platform_status_json(&status, true);
        assert!(v["username"].is_null());
        assert!(v["user_id"].is_null());
        assert_eq!(v["uses_global_fallback"], true);
        assert_eq!(v["provider_calls_enabled"], false);
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
        let Some(pool) = make_pool("t_dash_sm_platforms").await else {
            return;
        };
        // None-Auth → 401 vor dem Cipher-Aufbau (kein Secret nötig).
        let resp = platforms_status_handler(
            DashboardAuthLevel::None,
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    fn queue_body(clip_id: i64, platforms: Value) -> QueueUploadBody {
        QueueUploadBody {
            clip_id: Some(json!(clip_id)),
            platforms,
            title: None,
            description: None,
            hashtags: None,
            priority: 0,
            streamer: None,
            schedule: None,
            forms: Vec::new(),
        }
    }

    #[test]
    fn scheduled_at_value_maps_each_schedule_mode() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-16T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let taken = [chrono::DateTime::parse_from_rfc3339("2026-07-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)];

        assert_eq!(
            scheduled_at_value(
                Some("auto"),
                now,
                &taken,
                &tb_social_media::settings::PostingSchedule::default(),
            )
            .unwrap(),
            Some("2026-07-16T16:00:00+00:00".to_string())
        );
        assert_eq!(
            scheduled_at_value(
                Some("now"),
                now,
                &taken,
                &tb_social_media::settings::PostingSchedule::default(),
            )
            .unwrap(),
            Some("2026-07-16T10:00:00+00:00".to_string())
        );
        assert_eq!(
            scheduled_at_value(
                None,
                now,
                &taken,
                &tb_social_media::settings::PostingSchedule::default(),
            )
            .unwrap(),
            None
        );
        assert_eq!(
            scheduled_at_value(
                Some("2026-07-20T08:15:00Z"),
                now,
                &taken,
                &tb_social_media::settings::PostingSchedule::default(),
            )
            .unwrap(),
            Some("2026-07-20T08:15:00Z".to_string())
        );
        assert!(scheduled_at_value(
            Some("later"),
            now,
            &taken,
            &tb_social_media::settings::PostingSchedule::default(),
        )
        .is_err());
    }

    #[tokio::test]
    async fn queue_upload_handler_paths() {
        let Some(pool) = make_pool("t_dash_sm_queue").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('c1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // Admin, eine Plattform → ein queue_id.
        let resp = queue_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(queue_body(clip, json!(["tiktok"]))),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["queued"].as_array().unwrap().len(), 1);
        assert_eq!(v["queued"][0]["platform"], "tiktok");
        assert!(v["queued"][0]["queue_id"].is_number());

        // "all" → 3 Plattformen.
        let resp = queue_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(queue_body(clip, json!("all"))),
        )
        .await;
        assert_eq!(body_json(resp).await["queued"].as_array().unwrap().len(), 3);

        // Ungültige Plattform → error queue_failed (kein Crash).
        let resp = queue_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(queue_body(clip, json!(["snapchat"]))),
        )
        .await;
        assert_eq!(body_json(resp).await["queued"][0]["error"], "queue_failed");

        // Fehlende clip_id → 400.
        let resp = queue_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(QueueUploadBody {
                clip_id: None,
                platforms: json!(["tiktok"]),
                title: None,
                description: None,
                hashtags: None,
                priority: 0,
                streamer: None,
                schedule: None,
                forms: Vec::new(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Partner mit fremdem Clip → 403.
        let resp = queue_upload_handler(
            partner("other"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(queue_body(clip, json!(["tiktok"]))),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn queue_upload_handler_reports_disabled_form_without_reserving_it() {
        let Some(pool) = make_pool("t_dash_sm_queue_forms").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, clip_title) VALUES ('forms-c1', 'https://clips.twitch.tv/forms-c1', 'nani', 'Resolved title') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mut body = queue_body(clip, json!([]));
        body.forms = vec![tb_social_media::forms::FormKey::DeadlockHigh];

        let resp = queue_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(body),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert!(body["queued"].as_array().unwrap().is_empty());
        assert_eq!(
            body["forms"],
            json!([{
                "form": "deadlock_high",
                "status": "skipped_disabled",
                "http_status": null,
            }])
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clip_form_submissions WHERE clip_id = $1 AND form_key = 'deadlock_high'",
        )
        .bind(clip as i32)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    fn clips_query(status: Option<&str>) -> AdminClipsQuery {
        AdminClipsQuery {
            page: None,
            page_size: None,
            status: status.map(String::from),
            streamer: None,
        }
    }

    #[tokio::test]
    async fn admin_clips_list_detail_discard() {
        let Some(pool) = make_pool("t_dash_sm_admin_clips").await else {
            return;
        };
        // Clip A: tiktok hochgeladen, mit Enrichment + Approval.
        let a: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, clip_title, status, created_at, uploaded_tiktok) VALUES ('a', 'nani', 'Clip A', 'ready', '2026-06-10', TRUE) RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, status, llm_provider, hashtags_youtube, hashtags_tiktok) VALUES ($1, 'done', 'ollama', '[\"a\",\"b\"]'::jsonb, '[\"b\",\"c\"]'::jsonb)").bind(a as i32).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_approval (clip_db_id, state) VALUES ($1, 'approved')",
        )
        .bind(a as i32)
        .execute(&pool)
        .await
        .unwrap();
        // Clip B: verworfen.
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status, created_at, discarded_at) VALUES ('b', 'nani', 'discarded', '2026-06-09', NOW())").execute(&pool).await.unwrap();
        assert_ne!(a, 0);

        // Liste: total 2, neuester (A) zuerst.
        let resp = admin_clips_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(clips_query(None)),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["total"], 2);
        assert_eq!(v["items"][0]["clip_db_id"], a);
        assert_ne!(v["items"][0]["clip_db_id"], 0);

        // Status-Filter "discarded" → nur B.
        let resp = admin_clips_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(clips_query(Some("discarded"))),
        )
        .await;
        assert_eq!(body_json(resp).await["total"], 1);

        // Detail von A: Merge aus Enrichment + Approval + platform_status + effective_layout.
        let resp = admin_clip_detail_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(a.to_string()),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], a);
        assert_eq!(v["enrichment_status"], "done");
        assert_eq!(
            v["enrichment_summary"]["top_hashtags"],
            json!(["a", "b", "c"])
        ); // dedup
        assert_eq!(v["enrichment_summary"]["provider"], "ollama");
        assert_eq!(v["platform_status"]["tiktok"], true);
        assert_eq!(v["platform_status"]["youtube"], false);
        assert_eq!(v["platform_status"]["instagram"], false);
        assert_eq!(v["approval"]["state"], "approved");
        assert!(v["effective_layout"]["version"] == 1);

        // Discard von A → discarded_at gesetzt im zurückgegebenen Record.
        let resp = admin_clip_discard_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(a.to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], a);
        assert!(!v["discarded_at"].is_null());

        // Fehlerpfade.
        assert_eq!(
            admin_clip_detail_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("abc".into())
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            admin_clip_detail_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("99999".into())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            admin_clip_discard_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("99999".into())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        // Partner → 403.
        assert_eq!(
            admin_clips_handler(
                partner("nani"),
                State(pool.clone()),
                Query(clips_query(None))
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn approval_und_zeitplan() {
        let Some(pool) = make_pool("t_dash_sm_approval").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ('nani')")
            .execute(&pool)
            .await
            .unwrap();
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status) VALUES ('a', 'nani', 'awaiting_approval') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_enrichment (clip_db_id, title_tiktok) VALUES ($1, 'TT')",
        )
        .bind(clip as i32)
        .execute(&pool)
        .await
        .unwrap();

        // approval-get vor Entscheidung → approval null.
        let resp = approval_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
        )
        .await;
        assert!(body_json(resp).await["approval"].is_null());

        // decision approve mit tiktok → state approved + Queue.
        let resp = approval_decision_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            "{\"decision\":\"approve\",\"platforms\":[\"tiktok\"]}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["approval"]["state"], "approved");
        assert_eq!(v["approval"]["approved_platforms"], json!(["tiktok"]));
        assert!(!v["clip"].is_null());

        // decision approve ohne Plattform + ohne Auto-Approve → 400 invalid_decision.
        let resp = approval_decision_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            "{\"decision\":\"approve\",\"platforms\":[]}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // platforms kein Array → 400.
        let resp = approval_decision_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            "{\"decision\":\"approve\",\"platforms\":\"tiktok\"}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Zeitplan lesen: Defaults aus der Kadenz-Recherche, Freigabe manuell.
        let nani = Query(StreamerLoginQuery {
            streamer_login: Some("nani".into()),
        });
        let resp = posting_plan_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            nani.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["approval_mode"], "manual");
        assert_eq!(v["timezone"], "Europe/Berlin");
        assert_eq!(v["platforms"].as_array().unwrap().len(), 3);
        assert_eq!(v["platforms"][0]["posts_per_week"], 4);
        assert_eq!(v["platforms"][0]["max_posts_per_day"], 1);
        assert_eq!(v["platforms"][0]["auto_post"], false);
        // Deadlock ist voreingestellt an und darf angereichert werden.
        let deadlock = v["categories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["category_key"] == "deadlock")
            .cloned()
            .unwrap();
        assert_eq!(deadlock["enrichment_enabled"], true);
        assert_eq!(deadlock["auto_post"], true);

        // Freigabe-Modus setzen.
        let resp = posting_plan_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            nani.clone(),
            "{\"approval_mode\":\"veto_window\"}".into(),
        )
        .await;
        assert_eq!(body_json(resp).await["approval_mode"], "veto_window");

        // Unbekannter Modus und unbekannte Zeitzone werden abgelehnt.
        for kaputt in [
            "{\"approval_mode\":\"sofort\"}",
            "{\"timezone\":\"Nirgendwo/Erfunden\"}",
        ] {
            let resp = posting_plan_put_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                nani.clone(),
                kaputt.into(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{kaputt}");
        }

        // Kadenz einer Plattform setzen.
        let resp = posting_plan_platform_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("tiktok".into()),
            nani.clone(),
            "{\"auto_post\":true,\"posts_per_week\":3,\"post_times\":[\"20:00\",\"09:30\"]}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        let tiktok = v["platforms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["platform"] == "tiktok")
            .cloned()
            .unwrap();
        assert_eq!(tiktok["auto_post"], true);
        assert_eq!(tiktok["posts_per_week"], 3);
        assert_eq!(tiktok["post_times"], json!(["09:30", "20:00"]));
        assert!(
            tiktok["next_slot"].is_string(),
            "Termin wird vorausberechnet"
        );

        // Unsinnige Kadenz und kaputte Zeiten werden abgelehnt.
        for kaputt in [
            "{\"posts_per_week\":999}",
            "{\"max_posts_per_day\":-1}",
            "{\"post_times\":[\"25:00\"]}",
            "{\"post_times\":[]}",
        ] {
            let resp = posting_plan_platform_put_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("tiktok".into()),
                nani.clone(),
                kaputt.into(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{kaputt}");
        }
        let resp = posting_plan_platform_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("mastodon".into()),
            nani.clone(),
            "{\"auto_post\":true}".into(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "unbekannte Plattform"
        );

        // Kategorie-Schalter.
        let resp = posting_plan_category_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("deadlock".into()),
            nani.clone(),
            "{\"auto_post\":false}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(
            v["categories"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["category_key"] == "deadlock")
                .unwrap()["auto_post"],
            false
        );
        let resp = posting_plan_category_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("solitaer".into()),
            nani.clone(),
            "{\"auto_post\":true}".into(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "unbekannte Kategorie"
        );

        // Partner ohne Social-Media-Freigabe → 403, invalid clip → 400.
        assert_eq!(
            posting_plan_get_handler(partner("nani"), State(pool.clone()), nani.clone())
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            approval_get_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("abc".into())
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn vod_archiv_einstellung() {
        let Some(pool) = make_pool("t_dash_sm_vod").await else {
            return;
        };
        for login in ["earlysalty", "nani"] {
            sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ($1)")
                .bind(login)
                .execute(&pool)
                .await
                .unwrap();
        }
        let frage = |login: &str| StreamerLoginQuery {
            streamer_login: Some(login.to_string()),
        };

        // Default: aus und privat, weil das Google-Projekt nicht auditiert ist.
        let resp = vod_archive_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(frage("earlysalty")),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["enabled"], false);
        assert_eq!(v["privacy"], "private");
        assert_eq!(v["privacy_forced"], privacy_forced());
        assert_eq!(v["streamer_login"], "earlysalty");

        // Einschalten mit Sichtbarkeit.
        let resp = vod_archive_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"streamer_login\":\"earlysalty\",\"enabled\":true,\"privacy\":\"unlisted\"}".into(),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["enabled"], true);
        assert_eq!(v["privacy"], "unlisted");

        // Der zweite Kanal bleibt davon unberuehrt.
        let resp = vod_archive_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(frage("nani")),
        )
        .await;
        assert_eq!(body_json(resp).await["enabled"], false);

        // Toggle ohne privacy behaelt die bisherige Sichtbarkeit.
        let resp = vod_archive_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"streamer_login\":\"earlysalty\",\"enabled\":false}".into(),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["enabled"], false);
        assert_eq!(v["privacy"], "unlisted");

        // Unbekannte Sichtbarkeit wird abgewiesen, nicht still ersetzt.
        let resp = vod_archive_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"streamer_login\":\"earlysalty\",\"enabled\":true,\"privacy\":\"weltweit\"}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Der abgewiesene Aufruf darf nichts geaendert haben.
        let resp = vod_archive_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(frage("earlysalty")),
        )
        .await;
        assert_eq!(body_json(resp).await["enabled"], false);

        // Kaputtes JSON → 400, fehlender Kanal → 400, unbekannter Kanal → 404.
        assert_eq!(
            vod_archive_put_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                "kein json".into()
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            vod_archive_put_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                "{\"enabled\":true}".into()
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            vod_archive_get_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(frage("gibtsnicht"))
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }

    /// Der sicherheitsrelevante Kern: ein freigeschalteter Partner darf sein
    /// eigenes Archiv schalten, aber unter keinen Umstaenden ein fremdes.
    #[tokio::test]
    async fn partner_schaltet_nur_den_eigenen_kanal() {
        let Some(pool) = make_pool("t_dash_sm_vod_partner").await else {
            return;
        };
        for login in ["earlysalty", "nani"] {
            sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ($1)")
                .bind(login)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('nani', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Eigener Kanal: geht.
        let resp = vod_archive_put_handler(
            sm_partner("nani"),
            State(pool.clone()),
            "{\"streamer_login\":\"nani\",\"enabled\":true}".into(),
        )
        .await;
        assert_eq!(body_json(resp).await["enabled"], true);

        // Fremder Kanal: 403, und der fremde Schalter bleibt aus.
        let resp = vod_archive_put_handler(
            sm_partner("nani"),
            State(pool.clone()),
            "{\"streamer_login\":\"earlysalty\",\"enabled\":true}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let resp = vod_archive_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: Some("earlysalty".to_string()),
            }),
        )
        .await;
        assert_eq!(body_json(resp).await["enabled"], false);

        // Ohne Angabe faellt der Partner auf den eigenen Kanal zurueck, nie auf
        // einen fremden.
        let resp = vod_archive_get_handler(
            sm_partner("nani"),
            State(pool.clone()),
            Query(StreamerLoginQuery {
                streamer_login: None,
            }),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["streamer_login"], "nani");
        assert_eq!(v["enabled"], true);

        // Ein nicht freigeschalteter Partner kommt gar nicht erst durch.
        assert_eq!(
            vod_archive_put_handler(
                sm_partner("earlysalty"),
                State(pool.clone()),
                "{\"streamer_login\":\"earlysalty\",\"enabled\":true}".into()
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn enrichment_analytics_reports_get() {
        let Some(pool) = make_pool("t_dash_sm_reads").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('a', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // enrichment-get: ensure_enrichment_row legt pending an.
        let resp = enrichment_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], clip);
        assert_eq!(v["status"], "pending");
        assert_eq!(v["hashtags_youtube"], json!([])); // leere Liste, nicht null

        // analytics-get: zwei Snapshots.
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, views, likes, engagement_rate, provider, synced_at) VALUES ($1, 'tiktok', '24h', 100, 10, 12.5, 'tiktok_open_api_v2', NOW())").bind(clip).execute(&pool).await.unwrap();
        let resp = clip_analytics_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
        )
        .await;
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], clip);
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["views"], 100);
        assert_eq!(v["items"][0]["engagement_rate"], 12.5);

        // reports-list: insert + Filter.
        sqlx::query("INSERT INTO social_media_reports (kind, streamer_login, period_start, period_end, content_md) VALUES ('streamer', 'nani', NOW()-INTERVAL '7 days', NOW(), '# R')").execute(&pool).await.unwrap();
        let resp = reports_list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ReportsQuery {
                kind: None,
                streamer: None,
                limit: None,
            }),
        )
        .await;
        assert_eq!(body_json(resp).await["items"].as_array().unwrap().len(), 1);
        // invalid_kind → 400.
        let resp = reports_list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ReportsQuery {
                kind: Some("bogus".into()),
                streamer: None,
                limit: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // invalid_limit → 400.
        let resp = reports_list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ReportsQuery {
                kind: None,
                streamer: None,
                limit: Some("x".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Fehlerpfade: nicht existierender Clip → 404, Partner → 403.
        assert_eq!(
            enrichment_get_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("99999".into())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            reports_list_handler(
                partner("nani"),
                State(pool.clone()),
                Query(ReportsQuery {
                    kind: None,
                    streamer: None,
                    limit: None
                })
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );

        // enrichment-PUT: Titel setzen + hashtags (#-Präfix/dedup).
        let resp = enrichment_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            "{\"title_youtube\":\"YT\",\"hashtags_youtube\":[\"a\",\"#a\",\"b\"]}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["title_youtube"], "YT");
        assert_eq!(v["hashtags_youtube"], json!(["#a", "#b"])); // #-Präfix + dedup
                                                                // ungültiges Feld (Zahl) → 400 invalid_field.
        let resp = enrichment_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            "{\"title_youtube\":5}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Plattformgrenzen gelten im echten Schreibhandler und verändern den
        // zuvor gespeicherten Datensatz bei einer Ablehnung nicht.
        let resp = enrichment_put_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            json!({ "title_youtube": "x".repeat(101) }).to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let error = body_json(resp).await;
        assert_eq!(error["error"], "enrichment_limits_exceeded");
        let saved = get_enrichment(&pool, i32::try_from(clip).unwrap())
            .await
            .unwrap();
        assert_eq!(saved.title_youtube.as_deref(), Some("YT"));

        // nicht existierender Clip → 404.
        assert_eq!(
            enrichment_put_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("99999".into()),
                "{}".into()
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn enrichment_run_skips_without_transcriber_and_llm() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let Some(pool) = make_pool("t_dash_sm_enrich_run").await else {
            return;
        };
        std::env::set_var("OLLAMA_HOST", "127.0.0.1:59999"); // LLM → Fallback (schnell, deterministisch)

        // Clip ohne Video-Pfad → Transkription übersprungen; LLM scheitert →
        // Pipeline endet bei skipped_no_key.
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('r', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        let resp = enrichment_run_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            String::new(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["clip_db_id"], clip);
        assert_eq!(v["outcome"]["status"], "skipped_no_key");
        assert_eq!(v["enrichment"]["clip_db_id"], clip);

        // force im Body wird akzeptiert (kein Parse-Fehler) → weiterhin OK.
        let resp = enrichment_run_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path(clip.to_string()),
            "{\"force\":true}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Fehlerpfade: ungültige ID → 400, fehlender Clip → 404, Partner → 403.
        assert_eq!(
            enrichment_run_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("x".into()),
                String::new()
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            enrichment_run_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("99999".into()),
                String::new()
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            enrichment_run_handler(
                partner("nani"),
                State(pool.clone()),
                Path(clip.to_string()),
                String::new()
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn reports_run_kinds() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let Some(pool) = make_pool("t_dash_sm_reports_run").await else {
            return;
        };
        std::env::set_var("OLLAMA_HOST", "127.0.0.1:59999"); // LLM → Fallback (schnell)

        // kind=streamer → erzeugt einen Streamer-Report (No-Data-Fallback).
        let resp = reports_run_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"kind\":\"streamer\",\"streamer\":\"nani\"}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["kind"], "streamer");
        assert_eq!(v["streamer_login"], "nani");
        assert!(v["id"].is_number());

        // kind=cross → Cross-Report.
        let resp = reports_run_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"kind\":\"cross\"}".into(),
        )
        .await;
        assert_eq!(body_json(resp).await["kind"], "cross");

        // ungültiger kind → 400 invalid_kind.
        let resp = reports_run_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"kind\":\"admin\"}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // streamer ohne streamer-Feld → 400 streamer_required.
        let resp = reports_run_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            "{\"kind\":\"streamer\"}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Partner → 403.
        assert_eq!(
            reports_run_handler(
                partner("nani"),
                State(pool.clone()),
                "{\"kind\":\"cross\"}".into()
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }

    fn location(resp: &Response) -> String {
        resp.headers()
            .get(axum::http::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }

    #[tokio::test]
    async fn oauth_start_callback_disconnect() {
        let Some(pool) = make_pool("t_dash_sm_oauth").await else {
            return;
        };

        // start: ungültige Plattform → 400.
        assert_eq!(
            oauth_start_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("snapchat".into()),
                Query(StreamerQuery { streamer: None })
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        // start: gültige Plattform → 302 (Auth-URL oder oauth_error-Redirect, je nach Env).
        assert_eq!(
            oauth_start_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("tiktok".into()),
                Query(StreamerQuery { streamer: None })
            )
            .await
            .status(),
            StatusCode::FOUND
        );
        // start: None-Auth → 401, Partner-Cross-Account → 403.
        assert_eq!(
            oauth_start_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Path("tiktok".into()),
                Query(StreamerQuery { streamer: None })
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            oauth_start_handler(
                partner("nani"),
                State(pool.clone()),
                Path("tiktok".into()),
                Query(StreamerQuery {
                    streamer: Some("other".into())
                })
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );

        // callback: error-Param → 302 provider_error.
        let resp = oauth_callback_handler(
            State(pool.clone()),
            None,
            Query(OAuthCallbackQuery {
                code: None,
                state: None,
                error: Some("access_denied".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FOUND);
        assert!(location(&resp).contains("oauth_error=provider_error"));
        // callback: fehlender code/state → 400.
        assert_eq!(
            oauth_callback_handler(
                State(pool.clone()),
                None,
                Query(OAuthCallbackQuery {
                    code: None,
                    state: None,
                    error: None
                })
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );

        // disconnect: setzt enabled=0 (kein Cipher nötig).
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login, enabled) VALUES ('tiktok', 'nani', 1)").execute(&pool).await.unwrap();
        let resp = oauth_disconnect_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("tiktok".into()),
            Query(StreamerQuery {
                streamer: Some("nani".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["success"], true);
        let enabled: i32 = sqlx::query_scalar("SELECT enabled FROM social_media_platform_auth WHERE platform='tiktok' AND streamer_login='nani'").fetch_one(&pool).await.unwrap();
        assert_eq!(enabled, 0);
        // disconnect: ungültige Plattform → 400.
        assert_eq!(
            oauth_disconnect_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("snap".into()),
                Query(StreamerQuery { streamer: None })
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }

    fn mark_body(
        clip_id: Option<i64>,
        reconciliations: Vec<MarkUploadedReconciliation>,
    ) -> MarkUploadedBody {
        MarkUploadedBody {
            clip_id: clip_id.map(|c| json!(c)),
            reconciliations,
            streamer: None,
        }
    }

    fn reconciliation(
        queue_id: i64,
        provider_started_at: DateTime<Utc>,
        external_id: Option<&str>,
    ) -> MarkUploadedReconciliation {
        MarkUploadedReconciliation {
            reconciliation_id: queue_id.to_string(),
            platform: "tiktok".into(),
            provider_started_at,
            provider_external_id: external_id.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn mark_uploaded_handler_paths() {
        let Some(pool) = make_pool("t_dash_sm_mark").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok', 'nani')").execute(&pool).await.unwrap();
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('c1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        let (queue_id, provider_started_at): (i64, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO twitch_clips_upload_queue \
             (clip_id, platform, status, provider_started_at, provider_lease_token, provider_external_id) \
             VALUES ($1, 'tiktok', 'reconciliation_required', NOW(), 'lease-a', 'video-42') \
             RETURNING id, provider_started_at",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        // Erfolg: exakt die im Dashboard angezeigte Reconciliation-ID.
        let resp = mark_uploaded_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(mark_body(
                Some(clip),
                vec![reconciliation(
                    queue_id,
                    provider_started_at,
                    Some("video-42"),
                )],
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["ok"], true);
        let up: bool = sqlx::query_scalar(
            "SELECT uploaded_tiktok FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(up);

        // Fehlende clip_id → 400, leere Reconciliation-Liste → 400.
        assert_eq!(
            mark_uploaded_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(StreamerQuery { streamer: None }),
                Json(mark_body(
                    None,
                    vec![reconciliation(
                        queue_id,
                        provider_started_at,
                        Some("video-42")
                    )]
                ))
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            mark_uploaded_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(StreamerQuery { streamer: None }),
                Json(mark_body(Some(clip), vec![]))
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        // Partner mit fremdem Clip → 403.
        assert_eq!(
            mark_uploaded_handler(
                partner("other"),
                State(pool.clone()),
                Query(StreamerQuery { streamer: None }),
                Json(mark_body(
                    Some(clip),
                    vec![reconciliation(
                        queue_id,
                        provider_started_at,
                        Some("video-42")
                    )]
                ))
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }

    /// Erzeugt eine winzige gültige MP4 via ffmpeg; None wenn ffmpeg fehlt.
    async fn tiny_mp4() -> Option<Vec<u8>> {
        let path = std::env::temp_dir().join("tb_upload_test_src.mp4");
        let out = tokio::process::Command::new("ffmpeg")
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=128x128:rate=10",
                "-pix_fmt",
                "yuv420p",
                "-y",
                &path.to_string_lossy(),
            ])
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
        let Some(pool) = make_pool("t_dash_sm_upload").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('nani', '1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let base = std::env::temp_dir()
            .join("tb_upload_test_dash")
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_dir_all(&base);

        // Ungültiger Streamer-Slug → 400.
        let resp = process_uploaded_clip(&pool, &base, Some("bad slug!"), None, None, b"xxxx")
            .await
            .unwrap_err();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Unbekannter Streamer → 404.
        let resp = process_uploaded_clip(&pool, &base, Some("ghost"), None, None, b"xxxxftypisom")
            .await
            .unwrap_err();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // Bekannter Streamer, aber keine MP4 (kein ftyp) → 415.
        let resp = process_uploaded_clip(
            &pool,
            &base,
            Some("nani"),
            Some("c1"),
            None,
            b"not a video at all",
        )
        .await
        .unwrap_err();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        // Happy-Path mit echter MP4 (best-effort, nur wenn ffmpeg da).
        if let Some(mp4) = tiny_mp4().await {
            let payload = process_uploaded_clip(
                &pool,
                &base,
                Some("nani"),
                Some("good1"),
                Some("Mein Clip"),
                &mp4,
            )
            .await
            .unwrap();
            assert!(payload["clip_db_id"].as_i64().unwrap() > 0);
            let first_id = payload["clip_id"].as_str().unwrap();
            assert!(first_id.starts_with("manual:"));
            assert!(std::path::Path::new(&format!("{base}/nani/{first_id}.mp4")).exists());

            // Auch dieselbe frei gewählte Client-ID reserviert keinen globalen
            // Twitch-Namen; jeder Request erhält einen neuen Server-Namensraum.
            let second =
                process_uploaded_clip(&pool, &base, Some("nani"), Some("good1"), None, &mp4)
                    .await
                    .unwrap();
            let second_id = second["clip_id"].as_str().unwrap();
            assert!(second_id.starts_with("manual:"));
            assert_ne!(first_id, second_id);

            // Der echte Multipart-Handler verarbeitet die UI-Reihenfolge
            // (Datei vor Metadaten) streamend und ignoriert die globale
            // Client-Clip-ID auch auf dem vollständigen DB-Pfad.
            let boundary = "tb-real-upload-boundary";
            let payload = multipart_payload(
                &[
                    ("file", Some("clip.mp4"), &mp4),
                    ("streamer_login", None, b"nani"),
                    ("clip_id", None, b"fremde-globale-twitch-id"),
                    ("title", None, b"Mein Handler-Clip"),
                ],
                boundary,
            );
            let route_semaphore = Semaphore::new(1);
            let response = upload_clip_handler_inner(
                DashboardAuthLevel::admin(),
                pool.clone(),
                extract_multipart(Body::from(payload), boundary).await,
                &base,
                &route_semaphore,
                UploadTiming::production(),
                UploadLimits::production(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::CREATED);
            let route_payload = body_json(response).await;
            let route_id = route_payload["clip_id"].as_str().unwrap();
            assert!(route_id.starts_with("manual:"));
            assert_ne!(route_id, "fremde-globale-twitch-id");
            assert!(std::path::Path::new(&format!("{base}/nani/{route_id}.mp4")).exists());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn batch_upload_handler_paths() {
        let Some(pool) = make_pool("t_dash_sm_batch").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, clip_title, created_at) VALUES ('a', 'nani', 'A', '2026-06-10')").execute(&pool).await.unwrap();

        // Admin mit streamer + tiktok → 1 eingereiht.
        let resp = batch_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(BatchUploadBody {
                streamer: Some("nani".into()),
                platforms: json!(["tiktok"]),
                apply_default_template: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["success"], true);
        assert_eq!(v["stats"]["queued"], 1);

        // platforms leer → 400.
        let resp = batch_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(BatchUploadBody {
                streamer: Some("nani".into()),
                platforms: json!([]),
                apply_default_template: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Admin ohne streamer (required) → 400.
        let resp = batch_upload_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(BatchUploadBody {
                streamer: None,
                platforms: json!(["tiktok"]),
                apply_default_template: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Partner cross-account → 403.
        let resp = batch_upload_handler(
            partner("nani"),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
            Json(BatchUploadBody {
                streamer: Some("other".into()),
                platforms: json!(["tiktok"]),
                apply_default_template: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn fetch_clips_auth_pfade() {
        let Some(pool) = make_pool("t_dash_sm_fetch").await else {
            return;
        };
        // None-Auth → 401 (vor Scope/Helix).
        assert_eq!(
            fetch_clips_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Json(FetchClipsBody {
                    streamer: Some("nani".into()),
                    limit: None,
                    days: None
                })
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        // Admin ohne streamer (required) → 400.
        assert_eq!(
            fetch_clips_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Json(FetchClipsBody {
                    streamer: None,
                    limit: None,
                    days: None
                })
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        // Partner cross-account → 403.
        assert_eq!(
            fetch_clips_handler(
                partner("nani"),
                State(pool.clone()),
                Json(FetchClipsBody {
                    streamer: Some("other".into()),
                    limit: None,
                    days: None
                })
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }

    /// Der zerstoerende Knopf: „Trennen" ohne `streamer` zeigte fuer
    /// Admin-Sessions still auf die Sammelverbindung und kappte damit alle
    /// Kanaele auf einmal.
    #[tokio::test]
    async fn trennen_braucht_kanal_und_trifft_nur_dessen_zeile() {
        let Some(pool) = make_pool("t_dash_sm_disconnect_scope").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO social_media_platform_auth (platform, streamer_login, enabled) \
             VALUES ('youtube', NULL, 1), ('youtube', 'earlysalty', 1), ('youtube', 'ismile_e', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('earlysalty', TRUE)")
            .execute(&pool)
            .await
            .unwrap();

        // Ohne Parameter: 400 statt stillem Griff zur Sammelverbindung.
        let resp = oauth_disconnect_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("youtube".to_string()),
            Query(StreamerQuery { streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Fremder Kanal: 403.
        let resp = oauth_disconnect_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Path("youtube".to_string()),
            Query(StreamerQuery {
                streamer: Some("ismile_e".to_string()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Beide Abweisungen duerfen nichts angefasst haben.
        let aktiv: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM social_media_platform_auth WHERE enabled = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(aktiv, 3, "abgewiesene Anfragen duerfen nicht wirken");

        // Eigener Kanal: trifft genau die kanalgebundene Zeile.
        let resp = oauth_disconnect_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Path("youtube".to_string()),
            Query(StreamerQuery {
                streamer: Some("EarlySalty".to_string()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let rows: Vec<(Option<String>, i32)> = sqlx::query_as(
            "SELECT streamer_login, enabled FROM social_media_platform_auth ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (None, 1),
                (Some("earlysalty".to_string()), 0),
                (Some("ismile_e".to_string()), 1),
            ],
            "die Sammelverbindung und der fremde Kanal bleiben unberuehrt"
        );

        // Die Sammelverbindung nur noch ueber den ausdruecklichen Marker.
        let resp = oauth_disconnect_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("youtube".to_string()),
            Query(StreamerQuery {
                streamer: Some(GLOBAL_SCOPE_MARKER.to_string()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let global_enabled: i32 = sqlx::query_scalar(
            "SELECT enabled FROM social_media_platform_auth WHERE streamer_login IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(global_enabled, 0);

        // Ein Partner kommt an den Marker nicht heran.
        let resp = oauth_disconnect_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Path("youtube".to_string()),
            Query(StreamerQuery {
                streamer: Some(GLOBAL_SCOPE_MARKER.to_string()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn plattform_status_braucht_ebenfalls_einen_kanal() {
        let Some(pool) = make_pool("t_dash_sm_status_scope").await else {
            return;
        };
        let resp = platforms_status_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = platforms_status_handler(
            sm_partner("earlysalty"),
            State(pool.clone()),
            Query(StreamerQuery {
                streamer: Some("ismile_e".to_string()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let resp = platforms_status_handler(
            DashboardAuthLevel::None,
            State(pool.clone()),
            Query(StreamerQuery { streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Im Dashboard stand vorher nur ein nacktes „Fehler" ohne Grund und in den
    /// Zeitplan-Modi kein Termin.
    #[tokio::test]
    async fn clip_json_liefert_termin_und_fehlergrund_je_plattform() {
        let Some(pool) = make_pool("t_dash_sm_queue_info").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('q1', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, scheduled_at, last_error) \
             VALUES ($1, 'youtube', 'pending', '2026-08-24T18:00:00Z'::timestamptz, NULL), \
                    ($1, 'tiktok', 'failed', NULL, 'quota exceeded: daily limit')",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();

        let detail = body_json(
            admin_clip_detail_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path(clip.to_string()),
            )
            .await,
        )
        .await;
        assert_eq!(
            detail["scheduled_at"]["youtube"],
            "2026-08-24T18:00:00+00:00"
        );
        assert!(detail["scheduled_at"]["tiktok"].is_null());
        assert!(detail["scheduled_at"]["instagram"].is_null());
        assert!(detail["upload_errors"]["youtube"].is_null());
        assert_eq!(
            detail["upload_errors"]["tiktok"],
            "quota exceeded: daily limit"
        );
        assert!(detail["upload_errors"]["instagram"].is_null());

        // Die Liste liefert dieselben Felder (eine Abfrage fuer die ganze Seite).
        let liste = body_json(
            admin_clips_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(AdminClipsQuery {
                    page: None,
                    page_size: None,
                    status: None,
                    streamer: None,
                }),
            )
            .await,
        )
        .await;
        let item = &liste["items"][0];
        assert_eq!(item["scheduled_at"]["youtube"], "2026-08-24T18:00:00+00:00");
        assert_eq!(
            item["upload_errors"]["tiktok"],
            "quota exceeded: daily limit"
        );

        // Ein Clip ganz ohne Queue-Zeilen bekommt drei ausdrueckliche Nullwerte.
        let leer: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('q2', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let detail = body_json(
            admin_clip_detail_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path(leer.to_string()),
            )
            .await,
        )
        .await;
        assert_eq!(
            detail["scheduled_at"],
            json!({ "youtube": null, "tiktok": null, "instagram": null })
        );
        assert_eq!(
            detail["upload_errors"],
            json!({ "youtube": null, "tiktok": null, "instagram": null })
        );
    }

    #[tokio::test]
    async fn abbruch_raeumt_wartende_uploads_ab_und_meldet_laufende() {
        let Some(pool) = make_pool("t_dash_sm_approval_cancel").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status) VALUES ('c1', 'nani', 'approved') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) VALUES ($1, 'approved', '[\"youtube\", \"tiktok\"]'::jsonb)")
            .bind(clip as i32)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'youtube', 'pending'), ($1, 'tiktok', 'pending')",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();

        let out = body_json(
            approval_cancel_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path(clip.to_string()),
            )
            .await,
        )
        .await;
        assert_eq!(out, json!({ "cancelled": 2, "already_running": 0 }));

        let rest: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rest, 0);
        let (state, status): (String, String) = sqlx::query_as(
            "SELECT a.state, c.status FROM social_media_clip_approval a \
             JOIN twitch_clips_social_media c ON c.id = a.clip_db_id WHERE a.clip_db_id = $1",
        )
        .bind(clip as i32)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, "awaiting_approval");
        assert_eq!(status, "awaiting_approval");
        let plattformen: String = sqlx::query_scalar(
            "SELECT approved_platforms::text FROM social_media_clip_approval WHERE clip_db_id = $1",
        )
        .bind(clip as i32)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(plattformen, "[]");

        // Zweiter Clip: eine Plattform ist schon unterwegs und bleibt stehen.
        let laeuft: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, streamer_login, status) VALUES ('c2', 'nani', 'approved') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'youtube', 'processing'), ($1, 'tiktok', 'pending')",
        )
        .bind(laeuft)
        .execute(&pool)
        .await
        .unwrap();
        let out = body_json(
            approval_cancel_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path(laeuft.to_string()),
            )
            .await,
        )
        .await;
        assert_eq!(out, json!({ "cancelled": 1, "already_running": 1 }));
        let rest: Vec<(String, String)> = sqlx::query_as(
            "SELECT platform, status FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(laeuft)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rest,
            vec![("youtube".to_string(), "processing".to_string())]
        );
    }

    #[tokio::test]
    async fn abbruch_fremder_clip_ist_403_und_unbekannter_clip_404() {
        let Some(pool) = make_pool("t_dash_sm_approval_cancel_scope").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('nani', TRUE)")
            .execute(&pool)
            .await
            .unwrap();
        let fremd: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('fremd', 'ismile_e') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, 'youtube', 'pending')")
            .bind(fremd)
            .execute(&pool)
            .await
            .unwrap();

        let resp = approval_cancel_handler(
            sm_partner("nani"),
            State(pool.clone()),
            Path(fremd.to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let rest: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(fremd)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rest, 1, "der fremde Clip bleibt eingeplant");

        let resp = approval_cancel_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Path("999999".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let resp = approval_cancel_handler(
            DashboardAuthLevel::None,
            State(pool.clone()),
            Path(fremd.to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

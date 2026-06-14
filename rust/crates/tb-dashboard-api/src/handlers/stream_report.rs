//! Handler für `GET /twitch/api/v2/stream-report`.
//!
//! Port von `bot/analytics/api_post_stream.py:_api_v2_stream_report` +
//! `_serialize_report_payload`. Liest die KI-Post-Stream-A/B-Reports
//! (`twitch_stream_ai_reports`) für die Dashboard-Anzeige, inkl. eingebettetem
//! Rating der eigenen Session/Variante.
//!
//! Auth: Partner → nur eigener Login; Admin/Localhost → beliebiger Streamer.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

const VARIANT_COMPACT: &str = "compact";
const VARIANT_FULL: &str = "full";

/// Spalten-Select (JSONB/Zeit als `::text`, parsen/`str()` wie Python).
const REPORT_SELECT: &str = "SELECT session_id, model, generated_at::text AS generated_at, status, \
     schema_version, report_variant, prompt_version, started_at::text AS started_at, \
     finished_at::text AS finished_at, report_json::text AS report_json, \
     word_groups_json::text AS word_groups_json, error \
     FROM twitch_stream_ai_reports";

#[derive(Deserialize)]
pub struct StreamReportParams {
    pub streamer: Option<String>,
    pub session_id: Option<String>,
    pub variant: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ReportRow {
    session_id: Option<i64>,
    model: Option<String>,
    generated_at: Option<String>,
    status: Option<String>,
    schema_version: Option<String>,
    report_variant: Option<String>,
    prompt_version: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    report_json: Option<String>,
    word_groups_json: Option<String>,
    error: Option<String>,
}

fn owner_login(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => Some(twitch_login.to_lowercase()),
        _ => None,
    }
}

/// Baut die serialisierte Report-Payload (Python `_serialize_report_payload`):
/// JSONB-Text → Value (Fehler/leer → `{}`/`[]`), Zeitfelder als String.
fn serialize_report_row(r: &ReportRow) -> serde_json::Value {
    let report = r
        .report_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or_else(|| json!({}));
    let word_groups = r
        .word_groups_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or_else(|| json!([]));
    json!({
        "session_id": r.session_id,
        "model": r.model,
        "generated_at": r.generated_at.clone().unwrap_or_default(),
        "status": r.status,
        "schema_version": r.schema_version,
        "report_variant": r.report_variant.clone().unwrap_or_else(|| VARIANT_COMPACT.to_string()),
        "prompt_version": r.prompt_version,
        "started_at": r.started_at.clone().unwrap_or_default(),
        "finished_at": r.finished_at.clone().unwrap_or_default(),
        "report": report,
        "word_groups": word_groups,
        "error": r.error,
    })
}

/// `GET /twitch/api/v2/stream-report`
pub async fn stream_report_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<StreamReportParams>,
) -> impl IntoResponse {
    let streamer = params.streamer.unwrap_or_default().trim().to_lowercase();
    let session_id: Option<i64> = params
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .and_then(|s| s.parse().ok());
    let mut variant = params.variant.unwrap_or_default().trim().to_lowercase();
    if variant.is_empty() {
        variant = VARIANT_COMPACT.to_string();
    }
    if !matches!(variant.as_str(), VARIANT_COMPACT | VARIANT_FULL | "ab" | "all") {
        variant = VARIANT_COMPACT.to_string();
    }
    if streamer.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "streamer required"}))).into_response();
    }

    // Auth: None → 401; Partner → eigener Login Pflicht; Admin/Localhost → frei.
    match &auth {
        DashboardAuthLevel::None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
        }
        DashboardAuthLevel::Localhost | DashboardAuthLevel::Admin => {}
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            if streamer != twitch_login.to_lowercase() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "forbidden"}))).into_response();
            }
        }
    }

    // A/B-Spalten best-effort sicherstellen (Python `_ensure_report_ab_columns`).
    let _ = tb_analytics::post_stream::ensure_report_ab_columns(&pool).await;

    // ── A/B / all: beide Varianten sammeln (neueste je Variante gewinnt) ──────
    if variant == "ab" || variant == "all" {
        let rows: Vec<ReportRow> = match session_id {
            Some(sid) => sqlx::query_as::<_, ReportRow>(&format!(
                "{REPORT_SELECT} WHERE session_id = $1 AND streamer_login = $2 \
                 AND COALESCE(report_variant, 'compact') IN ('compact', 'full') \
                 ORDER BY generated_at DESC"
            ))
            .bind(sid)
            .bind(&streamer)
            .fetch_all(&pool)
            .await,
            None => sqlx::query_as::<_, ReportRow>(&format!(
                "{REPORT_SELECT} WHERE streamer_login = $1 \
                 AND COALESCE(report_variant, 'compact') IN ('compact', 'full') \
                 ORDER BY generated_at DESC"
            ))
            .bind(&streamer)
            .fetch_all(&pool)
            .await,
        }
        .unwrap_or_default();

        // setdefault: erste (neueste) Zeile je Variante gewinnt.
        let mut reports = serde_json::Map::new();
        for r in &rows {
            let key = r.report_variant.clone().unwrap_or_else(|| VARIANT_COMPACT.to_string());
            reports.entry(key).or_insert_with(|| serialize_report_row(r));
        }
        if reports.is_empty() {
            return Json(json!({"empty": true, "streamer": streamer, "variant": variant})).into_response();
        }
        return Json(json!({"streamer": streamer, "variant": "ab", "reports": reports})).into_response();
    }

    // ── Einzel-Variante (compact|full): neuester Report ───────────────────────
    let row: Option<ReportRow> = match session_id {
        Some(sid) => sqlx::query_as::<_, ReportRow>(&format!(
            "{REPORT_SELECT} WHERE session_id = $1 AND streamer_login = $2 \
             AND COALESCE(report_variant, 'compact') = $3 \
             ORDER BY generated_at DESC LIMIT 1"
        ))
        .bind(sid)
        .bind(&streamer)
        .bind(&variant)
        .fetch_optional(&pool)
        .await,
        None => sqlx::query_as::<_, ReportRow>(&format!(
            "{REPORT_SELECT} WHERE streamer_login = $1 \
             AND COALESCE(report_variant, 'compact') = $2 \
             ORDER BY generated_at DESC LIMIT 1"
        ))
        .bind(&streamer)
        .bind(&variant)
        .fetch_optional(&pool)
        .await,
    }
    .ok()
    .flatten();

    let Some(row) = row else {
        return Json(json!({"empty": true, "streamer": streamer, "variant": variant})).into_response();
    };

    // Rating der eigenen Session/Variante einbetten (nur Partner hat einen Login).
    let mut rating = serde_json::Value::Null;
    if let (Some(sid), Some(rater)) = (row.session_id, owner_login(&auth)) {
        let variant_for_rating = row.report_variant.clone().unwrap_or_else(|| VARIANT_COMPACT.to_string());
        if let Ok(Some((rt, comment, rated_by, updated_at))) =
            sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>, Option<String>)>(
                "SELECT rating, comment, rated_by, updated_at::text \
                 FROM twitch_stream_report_ratings \
                 WHERE session_id = $1 AND report_variant = $2 AND rated_by = $3 LIMIT 1",
            )
            .bind(sid)
            .bind(&variant_for_rating)
            .bind(&rater)
            .fetch_optional(&pool)
            .await
        {
            rating = json!({
                "rating": rt,
                "comment": comment,
                "rated_by": rated_by,
                "updated_at": updated_at.unwrap_or_default(),
            });
        }
    }

    let mut payload = serialize_report_row(&row);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("rating".into(), rating);
    }
    Json(payload).into_response()
}

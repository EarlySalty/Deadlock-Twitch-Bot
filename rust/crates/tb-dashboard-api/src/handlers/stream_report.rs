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

#[derive(Deserialize)]
pub struct RateBody {
    pub session_id: Option<serde_json::Value>,
    pub streamer: Option<String>,
    pub variant: Option<String>,
    pub rating: Option<String>,
    pub comment: Option<String>,
}

/// UPSERT einer Report-Bewertung (Python INSERT … ON CONFLICT DO UPDATE).
async fn upsert_rating(
    pool: &PgPool,
    session_id: i64,
    streamer: &str,
    variant: &str,
    rating: &str,
    comment: Option<&str>,
    rated_by: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_stream_report_ratings \
         (session_id, streamer_login, report_variant, rating, comment, rated_by, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW()) \
         ON CONFLICT (session_id, report_variant, rated_by) \
         DO UPDATE SET rating = EXCLUDED.rating, comment = EXCLUDED.comment, updated_at = NOW()",
    )
    .bind(session_id)
    .bind(streamer)
    .bind(variant)
    .bind(rating)
    .bind(comment)
    .bind(rated_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// `POST /twitch/api/v2/stream-report/rate` (Python `_api_v2_stream_report_rate`).
pub async fn stream_report_rate_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let parsed: RateBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid JSON"}))).into_response();
        }
    };

    // session_id: Zahl oder Ziffern-String (Python body.get + str().isdigit()).
    let session_id: Option<i64> = match &parsed.session_id {
        Some(serde_json::Value::Number(n)) => n.as_i64(),
        Some(serde_json::Value::String(s)) => {
            let t = s.trim();
            if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
                t.parse().ok()
            } else {
                None
            }
        }
        _ => None,
    };
    let Some(session_id) = session_id else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "session_id required"}))).into_response();
    };

    let streamer = parsed.streamer.unwrap_or_default().trim().to_lowercase();
    if streamer.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "streamer required"}))).into_response();
    }

    let mut variant = parsed.variant.unwrap_or_default().trim().to_lowercase();
    if !matches!(variant.as_str(), VARIANT_COMPACT | VARIANT_FULL) {
        variant = VARIANT_COMPACT.to_string();
    }

    let rating = parsed.rating.unwrap_or_default().trim().to_lowercase();
    if !matches!(rating.as_str(), "gut" | "schlecht" | "neutral") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "rating must be 'gut', 'schlecht' or 'neutral'"})),
        )
            .into_response();
    }

    // comment: trim + auf 1000 Zeichen kürzen (Python `.strip()[:1000]`).
    let comment: String = parsed
        .comment
        .unwrap_or_default()
        .trim()
        .chars()
        .take(1000)
        .collect();

    // rated_by: Partner-Login, sonst Auth-Level-Name (Python
    // `twitch_login or auth_level or "unknown"`); None → 401.
    let rated_by = match &auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => twitch_login.trim().to_lowercase(),
        DashboardAuthLevel::Admin => "admin".to_string(),
        DashboardAuthLevel::Localhost => "localhost".to_string(),
        DashboardAuthLevel::None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
        }
    };

    let _ = tb_analytics::post_stream::ensure_report_ab_columns(&pool).await;
    let comment_opt = if comment.is_empty() { None } else { Some(comment.as_str()) };
    match upsert_rating(&pool, session_id, &streamer, &variant, &rating, comment_opt, &rated_by).await
    {
        Ok(()) => Json(json!({"ok": true, "rating": rating, "comment": comment})).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "PostStream Rating: Speichern fehlgeschlagen");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Bewertung konnte nicht gespeichert werden"})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn upsert_rating_on_conflict() {
        let Some(pool) = pool_or_skip("t7b_post_stream_rating").await else { return };
        tb_analytics::post_stream::ensure_report_ab_columns(&pool).await.unwrap();

        upsert_rating(&pool, 1, "streamer", "compact", "gut", Some("top"), "rater").await.unwrap();
        let (rating, comment): (String, Option<String>) = sqlx::query_as(
            "SELECT rating, comment FROM twitch_stream_report_ratings \
             WHERE session_id=1 AND report_variant='compact' AND rated_by='rater'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(rating, "gut");
        assert_eq!(comment.as_deref(), Some("top"));

        // Erneut (anderer Wert, comment None) → ON CONFLICT UPDATE, weiterhin 1 Zeile.
        upsert_rating(&pool, 1, "streamer", "compact", "schlecht", None, "rater").await.unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::int8 FROM twitch_stream_report_ratings WHERE session_id=1",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
        let (rating2, comment2): (String, Option<String>) = sqlx::query_as(
            "SELECT rating, comment FROM twitch_stream_report_ratings WHERE session_id=1 AND rated_by='rater'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(rating2, "schlecht"); // aktualisiert
        assert_eq!(comment2, None); // None überschreibt vorhandenen Kommentar

        // Anderer rated_by → eigene Zeile (UNIQUE pro session/variant/rated_by).
        upsert_rating(&pool, 1, "streamer", "compact", "neutral", None, "anderer").await.unwrap();
        let count2: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::int8 FROM twitch_stream_report_ratings WHERE session_id=1",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(count2, 2);
    }
}

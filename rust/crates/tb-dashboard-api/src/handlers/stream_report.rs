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
const REPORT_SELECT: &str =
    "SELECT session_id, model, generated_at::text AS generated_at, status, \
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

/// Login, unter dem Bewertungen/Stimmen dieses Auth-Levels gespeichert UND
/// gelesen werden (Schlüssel für eigenes Rating / `own_vote`).
///
/// P2.71: Ein per Twitch-OAuth eingeloggter Admin (z. B. `earlysalty`) trägt
/// seine Session-Identität im `AdminActor` (auth/level.rs) — Python liest
/// `twitch_login` IMMER aus der Session, auch bei `auth_level='admin'`
/// (api_post_stream.py:1271/1310/1364). Daher liefern wir hier den
/// Actor-Login statt `None`, damit GET das eigene Rating / `own_vote` einbettet
/// und der UPSERT-Schlüssel mit Python übereinstimmt.
/// Discord-Admin (`actor=None`) und Localhost haben keine Twitch-Identität → `None`.
fn owner_login(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => Some(twitch_login.to_lowercase()),
        DashboardAuthLevel::Admin { actor: Some(actor) } => Some(actor.twitch_login.to_lowercase()),
        _ => None,
    }
}

/// Schreib-Schlüssel (`rated_by`/`voted_by`) für ein Auth-Level.
///
/// P2.71: Spiegelt Python `twitch_login OR auth_level OR 'unknown'`
/// (api_post_stream.py:1271/1364). Twitch-OAuth-Admin → sein Login; Discord-Admin
/// ohne Twitch-Identität → `"admin"`; None → kein Schlüssel (Caller liefert 401).
fn writer_key(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            Some(twitch_login.trim().to_lowercase())
        }
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            Some(actor.twitch_login.trim().to_lowercase())
        }
        DashboardAuthLevel::Admin { actor: None } => Some("admin".to_string()),
        DashboardAuthLevel::None => None,
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
    if !matches!(
        variant.as_str(),
        VARIANT_COMPACT | VARIANT_FULL | "ab" | "all"
    ) {
        variant = VARIANT_COMPACT.to_string();
    }
    if streamer.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "streamer required"})),
        )
            .into_response();
    }

    // Auth: None → 401; Partner → eigener Login Pflicht; Admin/Localhost → frei.
    match &auth {
        DashboardAuthLevel::None => {
            return crate::auth::unauthorized_v2_response();
        }
        DashboardAuthLevel::Admin { .. } => {}
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            if streamer != twitch_login.to_lowercase() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "forbidden"})))
                    .into_response();
            }
        }
    }

    // ── A/B / all: beide Varianten sammeln (neueste je Variante gewinnt) ──────
    if variant == "ab" || variant == "all" {
        let rows_result = match session_id {
            Some(sid) => {
                sqlx::query_as::<_, ReportRow>(&format!(
                    "{REPORT_SELECT} WHERE session_id = $1 AND streamer_login = $2 \
                 AND COALESCE(report_variant, 'compact') IN ('compact', 'full') \
                 ORDER BY generated_at DESC"
                ))
                .bind(sid)
                .bind(&streamer)
                .fetch_all(&pool)
                .await
            }
            None => {
                sqlx::query_as::<_, ReportRow>(&format!(
                    "{REPORT_SELECT} WHERE streamer_login = $1 \
                 AND COALESCE(report_variant, 'compact') IN ('compact', 'full') \
                 ORDER BY generated_at DESC"
                ))
                .bind(&streamer)
                .fetch_all(&pool)
                .await
            }
        };
        let rows: Vec<ReportRow> = match rows_result {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!(error = %e, streamer = %streamer, "PostStream API: Report-Lookup fehlgeschlagen");
                return crate::auth::analytics_request_failed_json().into_response();
            }
        };

        // setdefault: erste (neueste) Zeile je Variante gewinnt.
        let mut reports = serde_json::Map::new();
        for r in &rows {
            let key = r
                .report_variant
                .clone()
                .unwrap_or_else(|| VARIANT_COMPACT.to_string());
            reports
                .entry(key)
                .or_insert_with(|| serialize_report_row(r));
        }
        if reports.is_empty() {
            return Json(json!({"empty": true, "streamer": streamer, "variant": variant}))
                .into_response();
        }
        return Json(json!({"streamer": streamer, "variant": "ab", "reports": reports}))
            .into_response();
    }

    // ── Einzel-Variante (compact|full): neuester Report ───────────────────────
    let row_result = match session_id {
        Some(sid) => {
            sqlx::query_as::<_, ReportRow>(&format!(
                "{REPORT_SELECT} WHERE session_id = $1 AND streamer_login = $2 \
             AND COALESCE(report_variant, 'compact') = $3 \
             ORDER BY generated_at DESC LIMIT 1"
            ))
            .bind(sid)
            .bind(&streamer)
            .bind(&variant)
            .fetch_optional(&pool)
            .await
        }
        None => {
            sqlx::query_as::<_, ReportRow>(&format!(
                "{REPORT_SELECT} WHERE streamer_login = $1 \
             AND COALESCE(report_variant, 'compact') = $2 \
             ORDER BY generated_at DESC LIMIT 1"
            ))
            .bind(&streamer)
            .bind(&variant)
            .fetch_optional(&pool)
            .await
        }
    };
    let row: Option<ReportRow> = match row_result {
        Ok(row) => row,
        Err(e) => {
            tracing::error!(error = %e, streamer = %streamer, "PostStream API: Report-Lookup fehlgeschlagen");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };

    let Some(row) = row else {
        return Json(json!({"empty": true, "streamer": streamer, "variant": variant}))
            .into_response();
    };

    // Rating der eigenen Session/Variante einbetten (nur Partner hat einen Login).
    let mut rating = serde_json::Value::Null;
    if let (Some(sid), Some(rater)) = (row.session_id, owner_login(&auth)) {
        let variant_for_rating = row
            .report_variant
            .clone()
            .unwrap_or_else(|| VARIANT_COMPACT.to_string());
        if let Ok(Some((rt, comment, rated_by, updated_at))) = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(
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
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }

    let parsed: RateBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid JSON"})),
            )
                .into_response();
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
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "session_id required"})),
        )
            .into_response();
    };

    // IDOR-Klemme: Partner darf nur für den eigenen Login bewerten; Admin frei.
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, parsed.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "streamer required"})),
                )
                    .into_response()
            }
            Err(resp) => return resp,
        };

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

    // rated_by: Partner-Login bzw. Twitch-OAuth-Admin-Login, sonst Auth-Level-Name
    // (Python `twitch_login or auth_level or "unknown"`, P2.71); None → 401.
    let Some(rated_by) = writer_key(&auth) else {
        return crate::auth::unauthorized_v2_response();
    };

    let comment_opt = if comment.is_empty() {
        None
    } else {
        Some(comment.as_str())
    };
    match upsert_rating(
        &pool,
        session_id,
        &streamer,
        &variant,
        &rating,
        comment_opt,
        &rated_by,
    )
    .await
    {
        Ok(()) => Json(json!({"ok": true, "rating": rating, "comment": comment})).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "PostStream Rating: Speichern fehlgeschlagen");
            crate::auth::analytics_request_failed_json().into_response()
        }
    }
}

// ─── A/B-Vote (GET own+totals, POST upsert) ─────────────────────────────────

#[derive(Deserialize)]
pub struct AbVoteQuery {
    pub streamer: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct AbVoteBody {
    pub session_id: Option<serde_json::Value>,
    pub streamer: Option<String>,
    pub winner: Option<String>,
    pub comment: Option<String>,
}

/// Stimmen-Aggregat einer Session als `{compact, full, gleich}` (Python `agg`),
/// fehlende Sieger bleiben 0.
async fn ab_vote_totals(pool: &PgPool, session_id: i64) -> Result<serde_json::Value, sqlx::Error> {
    let mut agg = serde_json::Map::new();
    agg.insert("compact".into(), json!(0));
    agg.insert("full".into(), json!(0));
    agg.insert("gleich".into(), json!(0));
    let rows = sqlx::query_as::<_, (Option<String>, i64)>(
        "SELECT winner, COUNT(*)::int8 AS n FROM twitch_stream_report_ab_votes \
         WHERE session_id = $1 GROUP BY winner",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    for (winner, n) in rows {
        if let Some(w) = winner {
            agg.insert(w, json!(n));
        }
    }
    Ok(serde_json::Value::Object(agg))
}

/// UPSERT einer A/B-Stimme (Python INSERT … ON CONFLICT (session_id, voted_by)).
async fn upsert_ab_vote(
    pool: &PgPool,
    session_id: i64,
    streamer: &str,
    winner: &str,
    comment: Option<&str>,
    voted_by: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_stream_report_ab_votes \
         (session_id, streamer_login, winner, comment, voted_by, updated_at) \
         VALUES ($1, $2, $3, $4, $5, NOW()) \
         ON CONFLICT (session_id, voted_by) \
         DO UPDATE SET winner = EXCLUDED.winner, comment = EXCLUDED.comment, updated_at = NOW()",
    )
    .bind(session_id)
    .bind(streamer)
    .bind(winner)
    .bind(comment)
    .bind(voted_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// `GET /twitch/api/v2/stream-report/ab-vote` — eigene Stimme + Aggregat.
/// `voted_by` ist hier NUR der Partner-Login (Python: kein auth_level-Fallback),
/// Admin/Localhost sehen daher kein `own_vote`.
pub async fn stream_report_ab_vote_get(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<AbVoteQuery>,
) -> impl IntoResponse {
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }

    let session_id: Option<i64> = q
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .and_then(|s| s.parse().ok());
    // IDOR-Klemme: Partner nur eigener Login; Admin braucht streamer (required).
    // None-Auth → 401 (vom Helfer). Der geklemmte Login wird hier nur zur
    // Ownership-Prüfung gebraucht (die Query filtert über session_id + voted_by).
    let _streamer = match crate::auth::resolve_streamer_scope(&auth, q.streamer.as_deref(), true) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "streamer und session_id erforderlich"})),
            )
                .into_response();
        }
        Err(resp) => return resp,
    };
    if session_id.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "streamer und session_id erforderlich"})),
        )
            .into_response();
    }
    let session_id = session_id.unwrap();
    let own_json = if let Some(vb) = owner_login(&auth) {
        match sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
            "SELECT winner, comment, updated_at::text FROM twitch_stream_report_ab_votes \
             WHERE session_id = $1 AND voted_by = $2 LIMIT 1",
        )
        .bind(session_id)
        .bind(&vb)
        .fetch_optional(&pool)
        .await
        {
            Ok(Some((winner, comment, updated_at))) => json!({
                "winner": winner,
                "comment": comment,
                "updated_at": updated_at.unwrap_or_default(),
            }),
            Ok(None) => serde_json::Value::Null,
            Err(e) => {
                tracing::error!(error = %e, "PostStream AB-Vote GET: Own-Vote-Lookup fehlgeschlagen");
                return crate::auth::analytics_request_failed_json().into_response();
            }
        }
    } else {
        serde_json::Value::Null
    };

    let totals = match ab_vote_totals(&pool, session_id).await {
        Ok(totals) => totals,
        Err(e) => {
            tracing::error!(error = %e, "PostStream AB-Vote GET: Aggregat-Lookup fehlgeschlagen");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };
    Json(json!({"session_id": session_id, "own_vote": own_json, "totals": totals})).into_response()
}

/// `POST /twitch/api/v2/stream-report/ab-vote` — Stimme abgeben/ändern.
pub async fn stream_report_ab_vote_post(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }

    let parsed: AbVoteBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid JSON"})),
            )
                .into_response();
        }
    };

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
    // IDOR-Klemme: Partner darf nur für den eigenen Login abstimmen; Admin frei.
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, parsed.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "session_id und streamer erforderlich"})),
                )
                    .into_response();
            }
            Err(resp) => return resp,
        };
    let Some(session_id) = session_id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "session_id und streamer erforderlich"})),
        )
            .into_response();
    };

    let winner = parsed.winner.unwrap_or_default().trim().to_lowercase();
    if !matches!(winner.as_str(), "compact" | "full" | "gleich") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "winner muss 'compact', 'full' oder 'gleich' sein"})),
        )
            .into_response();
    }

    // comment: trim + auf 500 Zeichen (Python `.strip()[:500]`).
    let comment: String = parsed
        .comment
        .unwrap_or_default()
        .trim()
        .chars()
        .take(500)
        .collect();

    // voted_by: Partner-Login bzw. Twitch-OAuth-Admin-Login, sonst Auth-Level-Name
    // (P2.71); None → 401.
    let Some(voted_by) = writer_key(&auth) else {
        return crate::auth::unauthorized_v2_response();
    };

    let comment_opt = if comment.is_empty() {
        None
    } else {
        Some(comment.as_str())
    };
    if let Err(e) = upsert_ab_vote(
        &pool,
        session_id,
        &streamer,
        &winner,
        comment_opt,
        &voted_by,
    )
    .await
    {
        tracing::error!(error = %e, "PostStream AB-Vote: Speichern fehlgeschlagen");
        return crate::auth::analytics_request_failed_json().into_response();
    }
    let totals = match ab_vote_totals(&pool, session_id).await {
        Ok(totals) => totals,
        Err(e) => {
            tracing::error!(error = %e, "PostStream AB-Vote POST: Aggregat-Lookup fehlgeschlagen");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };
    Json(json!({"ok": true, "winner": winner, "totals": totals})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    async fn create_stream_report_fixtures(pool: &PgPool) {
        tb_analytics::post_stream::ensure_report_ab_columns(pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_report_ratings (\
                id BIGSERIAL PRIMARY KEY, session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL, \
                report_variant TEXT NOT NULL DEFAULT 'compact', \
                rating TEXT NOT NULL CHECK (rating IN ('gut', 'schlecht', 'neutral')), \
                comment TEXT, rated_by TEXT NOT NULL, created_at TIMESTAMPTZ DEFAULT NOW(), \
                updated_at TIMESTAMPTZ DEFAULT NOW(), UNIQUE (session_id, report_variant, rated_by))",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_report_ab_votes (\
                id BIGSERIAL PRIMARY KEY, session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL, \
                winner TEXT NOT NULL CHECK (winner IN ('compact', 'full', 'gleich')), \
                comment TEXT, voted_by TEXT NOT NULL, created_at TIMESTAMPTZ DEFAULT NOW(), \
                updated_at TIMESTAMPTZ DEFAULT NOW(), UNIQUE (session_id, voted_by))",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("CREATE INDEX idx_ab_votes_session ON twitch_stream_report_ab_votes (session_id)")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("CREATE INDEX idx_ab_votes_streamer ON twitch_stream_report_ab_votes (streamer_login)")
            .execute(pool)
            .await
            .unwrap();
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: "1".into(),
            display_name: login.into(),
        }
    }

    /// IDOR: Ein Partner darf nicht für einen fremden Streamer ein Report-Rating
    /// abgeben → 403. Die Klemme greift vor jedem DB-Zugriff (Dummy-Pool genügt).
    #[tokio::test]
    async fn partner_fremder_streamer_rate_ist_403() {
        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:5432/none")
        {
            Ok(p) => p,
            Err(_) => return,
        };
        let rate_body = serde_json::to_vec(&json!({
            "session_id": 1, "streamer": "fremd", "variant": "compact", "rating": "gut"
        }))
        .unwrap();
        let resp = stream_report_rate_handler(partner("someone"), State(pool), rate_body.into())
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    fn admin_earlysalty() -> DashboardAuthLevel {
        DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "42".into(),
                twitch_login: "earlysalty".into(),
            }),
        }
    }

    /// P2.71: writer_key/owner_login lösen für den Twitch-OAuth-Admin seinen
    /// Login auf (nicht das hartkodierte "admin"); Discord-Admin bleibt "admin".
    #[test]
    fn writer_key_und_owner_login_twitch_admin() {
        assert_eq!(
            writer_key(&admin_earlysalty()).as_deref(),
            Some("earlysalty")
        );
        assert_eq!(
            owner_login(&admin_earlysalty()).as_deref(),
            Some("earlysalty")
        );
        // Discord-Admin (kein Actor) → "admin" / kein own-key.
        assert_eq!(
            writer_key(&DashboardAuthLevel::admin()).as_deref(),
            Some("admin")
        );
        assert_eq!(owner_login(&DashboardAuthLevel::admin()), None);
        assert_eq!(writer_key(&DashboardAuthLevel::None), None);
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// P2.71: Ein per Twitch-OAuth eingeloggter Admin (earlysalty) bewertet einen
    /// Report; die Zeile wird unter 'earlysalty' gespeichert und GET stream-report
    /// bettet das eigene Rating ein (vorher: 'admin' + kein eingebettetes Rating).
    #[tokio::test]
    async fn p2_71_twitch_admin_rating_unter_login_und_eingebettet() {
        let Some(pool) = pool_or_skip("t_p2_71_rating").await else {
            return;
        };
        create_stream_report_fixtures(&pool).await;
        // Ein Report existiert für die Session.
        sqlx::query(
            "INSERT INTO twitch_stream_ai_reports \
             (session_id, streamer_login, model, status, report_variant, report_json, generated_at) \
             VALUES (1, 'streamerx', 'm', 'ready', 'compact', '{}'::jsonb, NOW())",
        )
        .execute(&pool).await.unwrap();

        // Admin bewertet.
        let rate_body = serde_json::to_vec(&json!({
            "session_id": 1, "streamer": "streamerx", "variant": "compact", "rating": "gut"
        }))
        .unwrap();
        let resp =
            stream_report_rate_handler(admin_earlysalty(), State(pool.clone()), rate_body.into())
                .await
                .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        // Zeile steht unter 'earlysalty', nicht 'admin'.
        let rated_by: String = sqlx::query_scalar(
            "SELECT rated_by FROM twitch_stream_report_ratings WHERE session_id=1 AND report_variant='compact'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(rated_by, "earlysalty");

        // GET stream-report bettet das eigene Rating ein.
        let resp = stream_report_handler(
            admin_earlysalty(),
            State(pool.clone()),
            Query(StreamReportParams {
                streamer: Some("streamerx".into()),
                session_id: Some("1".into()),
                variant: Some("compact".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(
            v["rating"]["rating"], "gut",
            "eigenes Rating muss eingebettet sein"
        );
        assert_eq!(v["rating"]["rated_by"], "earlysalty");
    }

    /// P2.71: Admin-A/B-Stimme wird unter 'earlysalty' gespeichert und GET ab-vote
    /// liefert own_vote != null (vorher: 'admin' + own_vote=null).
    #[tokio::test]
    async fn p2_71_twitch_admin_abvote_unter_login_und_own_vote() {
        let Some(pool) = pool_or_skip("t_p2_71_abvote").await else {
            return;
        };
        create_stream_report_fixtures(&pool).await;

        let vote_body = serde_json::to_vec(&json!({
            "session_id": 5, "streamer": "streamerx", "winner": "compact"
        }))
        .unwrap();
        let resp =
            stream_report_ab_vote_post(admin_earlysalty(), State(pool.clone()), vote_body.into())
                .await
                .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        let voted_by: String = sqlx::query_scalar(
            "SELECT voted_by FROM twitch_stream_report_ab_votes WHERE session_id=5",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(voted_by, "earlysalty");

        let resp = stream_report_ab_vote_get(
            admin_earlysalty(),
            State(pool.clone()),
            Query(AbVoteQuery {
                streamer: Some("streamerx".into()),
                session_id: Some("5".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(
            v["own_vote"]["winner"], "compact",
            "own_vote muss gesetzt sein"
        );
    }

    #[tokio::test]
    async fn upsert_rating_on_conflict() {
        let Some(pool) = pool_or_skip("t7b_post_stream_rating").await else {
            return;
        };
        create_stream_report_fixtures(&pool).await;

        upsert_rating(&pool, 1, "streamer", "compact", "gut", Some("top"), "rater")
            .await
            .unwrap();
        let (rating, comment): (String, Option<String>) = sqlx::query_as(
            "SELECT rating, comment FROM twitch_stream_report_ratings \
             WHERE session_id=1 AND report_variant='compact' AND rated_by='rater'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rating, "gut");
        assert_eq!(comment.as_deref(), Some("top"));

        // Erneut (anderer Wert, comment None) → ON CONFLICT UPDATE, weiterhin 1 Zeile.
        upsert_rating(&pool, 1, "streamer", "compact", "schlecht", None, "rater")
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::int8 FROM twitch_stream_report_ratings WHERE session_id=1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
        let (rating2, comment2): (String, Option<String>) = sqlx::query_as(
            "SELECT rating, comment FROM twitch_stream_report_ratings WHERE session_id=1 AND rated_by='rater'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(rating2, "schlecht"); // aktualisiert
        assert_eq!(comment2, None); // None überschreibt vorhandenen Kommentar

        // Anderer rated_by → eigene Zeile (UNIQUE pro session/variant/rated_by).
        upsert_rating(&pool, 1, "streamer", "compact", "neutral", None, "anderer")
            .await
            .unwrap();
        let count2: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::int8 FROM twitch_stream_report_ratings WHERE session_id=1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count2, 2);
    }

    #[tokio::test]
    async fn ab_vote_upsert_und_totals() {
        let Some(pool) = pool_or_skip("t7c_post_stream_abvote").await else {
            return;
        };
        create_stream_report_fixtures(&pool).await;

        upsert_ab_vote(&pool, 1, "streamer", "compact", Some("a"), "voter1")
            .await
            .unwrap();
        upsert_ab_vote(&pool, 1, "streamer", "full", None, "voter2")
            .await
            .unwrap();
        upsert_ab_vote(&pool, 1, "streamer", "compact", None, "voter3")
            .await
            .unwrap();

        let totals = ab_vote_totals(&pool, 1).await.unwrap();
        assert_eq!(totals["compact"], 2);
        assert_eq!(totals["full"], 1);
        assert_eq!(totals["gleich"], 0); // Default-Key bleibt 0

        // voter1 ändert die Stimme → ON CONFLICT UPDATE (kein Duplikat).
        upsert_ab_vote(&pool, 1, "streamer", "gleich", None, "voter1")
            .await
            .unwrap();
        let totals2 = ab_vote_totals(&pool, 1).await.unwrap();
        assert_eq!(totals2["compact"], 1); // voter1 weg von compact
        assert_eq!(totals2["gleich"], 1);
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::int8 FROM twitch_stream_report_ab_votes WHERE session_id=1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 3); // 3 Voter, keine Duplikate

        // Fremde Session bleibt leer → alle 0.
        let empty = ab_vote_totals(&pool, 999).await.unwrap();
        assert_eq!(empty["compact"], 0);
        assert_eq!(empty["full"], 0);
        assert_eq!(empty["gleich"], 0);
    }
}

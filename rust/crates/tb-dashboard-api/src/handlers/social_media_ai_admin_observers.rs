use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{header::CACHE_CONTROL, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_http_core::ApiError;

use crate::auth::{
    level::{AuthenticatedAdminSessionId, AuthenticatedPartnerSessionId, DashboardAuthLevel},
    session::DashboardAuthState,
};
use crate::social_media_ai_admin_usage::{actor, failure, parse};

const CLIP_LIMIT: i64 = 60;
const COMPARISON_LIMIT: i64 = 20;
const OPEN_LIMIT: i64 = 5;
const MAX_NOTE_CHARS: usize = 1000;
macro_rules! clip_filter {
    () => {
        "c.source_kind = 'twitch' AND c.category_key = 'deadlock' \
         AND c.discarded_at IS NULL AND c.local_file_path IS NOT NULL AND c.local_file_path <> '' \
         AND c.twitch_user_id ~ '^[1-9][0-9]{0,18}$'"
    };
}

pub fn routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/twitch/api/admin/social-ai/observer-compare",
            get(overview).post(start),
        )
        .route(
            "/twitch/api/admin/social-ai/observer-compare/trials/{trial_id}/rating",
            post(rate),
        )
        .layer(DefaultBodyLimit::max(8192))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct Clip {
    clip_db_id: i32,
    login: String,
    title: String,
    clip_url: String,
    compared: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct Comparison {
    id: i64,
    clip_db_id: i64,
    login: String,
    title: String,
    clip_url: String,
    status: String,
    failure_code: Option<String>,
    requested_by: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct Trial {
    id: i64,
    comparison_id: i64,
    observer: String,
    model_item: String,
    thinking_item: String,
    status: String,
    failure_code: Option<String>,
    answer: Option<String>,
    elapsed_ms: Option<i64>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    rating: Option<i16>,
    rating_note: Option<String>,
    rated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct Score {
    observer: String,
    model_item: String,
    thinking_item: String,
    trials: i64,
    answered: i64,
    failed: i64,
    pending: i64,
    rated: i64,
    avg_rating: Option<f64>,
    avg_elapsed_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ComparisonView {
    #[serde(flatten)]
    comparison: Comparison,
    trials: Vec<Trial>,
}

#[derive(Debug, Serialize)]
struct Report {
    clips: Vec<Clip>,
    comparisons: Vec<ComparisonView>,
    scoreboard: Vec<Score>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRequest {
    clip_db_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RatingRequest {
    rating: i16,
    #[serde(default)]
    note: String,
}

fn unavailable(_: sqlx::Error) -> ApiError {
    failure(
        StatusCode::SERVICE_UNAVAILABLE,
        "Der Modellvergleich ist gerade nicht verfügbar. Bitte lade den Stand neu.",
    )
}

async fn clips(pool: &PgPool) -> Result<Vec<Clip>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT c.id AS clip_db_id, c.streamer_login AS login, COALESCE(c.clip_title, '') AS title, \
         c.clip_url, EXISTS (SELECT 1 FROM public.social_ai_observer_comparisons x WHERE x.clip_db_id = c.id) AS compared \
         FROM public.twitch_clips_social_media c WHERE ",
        clip_filter!(),
        " ORDER BY c.id DESC LIMIT $1"
    ))
    .bind(CLIP_LIMIT)
    .fetch_all(pool)
    .await
}

async fn comparisons(pool: &PgPool) -> Result<Vec<ComparisonView>, sqlx::Error> {
    let comparisons: Vec<Comparison> = sqlx::query_as(
        "SELECT x.id, x.clip_db_id, COALESCE(c.streamer_login, '') AS login, \
         COALESCE(c.clip_title, '') AS title, COALESCE(c.clip_url, '') AS clip_url, \
         x.status, x.failure_code, x.requested_by, x.created_at \
         FROM public.social_ai_observer_comparisons x \
         LEFT JOIN public.twitch_clips_social_media c ON c.id = x.clip_db_id \
         ORDER BY x.id DESC LIMIT $1",
    )
    .bind(COMPARISON_LIMIT)
    .fetch_all(pool)
    .await?;
    let ids: Vec<i64> = comparisons.iter().map(|comparison| comparison.id).collect();
    let mut trials: Vec<Trial> = sqlx::query_as(
        "SELECT id, comparison_id, observer, model_item, thinking_item, status, failure_code, answer, \
         elapsed_ms, started_at, finished_at, rating, rating_note, rated_at \
         FROM public.social_ai_observer_trials WHERE comparison_id = ANY($1) \
         ORDER BY comparison_id, position",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    Ok(comparisons
        .into_iter()
        .map(|comparison| {
            let (own, rest): (Vec<Trial>, Vec<Trial>) = std::mem::take(&mut trials)
                .into_iter()
                .partition(|trial| trial.comparison_id == comparison.id);
            trials = rest;
            ComparisonView {
                comparison,
                trials: own,
            }
        })
        .collect())
}

async fn scoreboard(pool: &PgPool) -> Result<Vec<Score>, sqlx::Error> {
    sqlx::query_as(
        "SELECT observer, MIN(model_item) AS model_item, MIN(thinking_item) AS thinking_item, \
         COUNT(*) AS trials, \
         COUNT(*) FILTER (WHERE status = 'answered') AS answered, \
         COUNT(*) FILTER (WHERE status = 'failed') AS failed, \
         COUNT(*) FILTER (WHERE status IN ('queued', 'running')) AS pending, \
         COUNT(rating) AS rated, \
         AVG(rating)::float8 AS avg_rating, \
         (AVG(elapsed_ms) FILTER (WHERE status = 'answered'))::float8 AS avg_elapsed_ms \
         FROM public.social_ai_observer_trials GROUP BY observer \
         ORDER BY AVG(rating) DESC NULLS LAST, observer",
    )
    .fetch_all(pool)
    .await
}

async fn report(pool: &PgPool) -> Result<Response, ApiError> {
    let data = Report {
        clips: clips(pool).await.map_err(unavailable)?,
        comparisons: comparisons(pool).await.map_err(unavailable)?,
        scoreboard: scoreboard(pool).await.map_err(unavailable)?,
    };
    Ok(([(CACHE_CONTROL, "no-store")], Json(data)).into_response())
}

pub async fn overview(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    report(&pool).await
}

async fn open_comparisons(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM public.social_ai_observer_comparisons x WHERE x.status = 'queued' \
         OR EXISTS (SELECT 1 FROM public.social_ai_observer_trials t \
                    WHERE t.comparison_id = x.id AND t.status IN ('queued', 'running'))",
    )
    .fetch_one(pool)
    .await
}

async fn clip_owner(pool: &PgPool, clip_db_id: i64) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT c.twitch_user_id::bigint FROM public.twitch_clips_social_media c WHERE c.id = $1 AND ",
        clip_filter!()
    ))
    .bind(clip_db_id)
    .fetch_optional(pool)
    .await
}

pub async fn start(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let request: StartRequest = parse(&body)?;
    let actor = actor(&auth, state, session, partner).await?;
    let owner = clip_owner(&pool, request.clip_db_id)
        .await
        .map_err(unavailable)?
        .ok_or_else(|| {
            failure(
                StatusCode::NOT_FOUND,
                "Dieser Clip liegt nicht lokal vor oder ist kein Deadlock-Clip.",
            )
        })?;
    if open_comparisons(&pool).await.map_err(unavailable)? >= OPEN_LIMIT {
        return Err(failure(
            StatusCode::CONFLICT,
            "Es laufen schon fünf Vergleiche. Warte, bis einer fertig ist.",
        ));
    }
    sqlx::query(
        "INSERT INTO public.social_ai_observer_comparisons (clip_db_id, twitch_user_id, requested_by) \
         VALUES ($1, $2, $3)",
    )
    .bind(request.clip_db_id)
    .bind(owner)
    .bind(&actor)
    .execute(&pool)
    .await
    .map_err(unavailable)?;
    report(&pool).await
}

fn valid_rating(request: &RatingRequest) -> bool {
    (1..=5).contains(&request.rating) && request.note.chars().count() <= MAX_NOTE_CHARS
}

pub async fn rate(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(trial_id): Path<i64>,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let request: RatingRequest = parse(&body)?;
    if !valid_rating(&request) {
        return Err(ApiError::bad_request(
            "Die Bewertung muss zwischen 1 und 5 liegen, die Notiz höchstens 1000 Zeichen.",
        ));
    }
    let actor = actor(&auth, state, session, partner).await?;
    let note = request.note.trim();
    let updated = sqlx::query(
        "UPDATE public.social_ai_observer_trials SET rating = $2, rating_note = $3, rated_by = $4, \
         rated_at = NOW() WHERE id = $1 AND status = 'answered'",
    )
    .bind(trial_id)
    .bind(request.rating)
    .bind((!note.is_empty()).then_some(note))
    .bind(&actor)
    .execute(&pool)
    .await
    .map_err(unavailable)?;
    if updated.rows_affected() == 0 {
        return Err(failure(
            StatusCode::NOT_FOUND,
            "Diese Antwort gibt es nicht oder sie ist noch nicht fertig.",
        ));
    }
    report(&pool).await
}

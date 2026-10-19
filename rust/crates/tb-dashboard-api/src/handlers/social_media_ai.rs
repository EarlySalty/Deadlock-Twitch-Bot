use std::{
    path::{Path as FilePath, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_social_media::{
    ai_editor::{self, store, AiEditorError, EditorRuntime},
    clip_prep_worker::ClipDownloader,
    produced_video,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::auth::level::DashboardAuthLevel;

#[derive(Clone)]
pub struct AiRuntime(pub Option<Arc<EditorRuntime>>);

impl AiRuntime {
    pub fn packaged() -> Self {
        match EditorRuntime::load_packaged() {
            Ok(runtime) => Self(Some(Arc::new(runtime))),
            Err(_) => {
                tracing::warn!("Packaged social AI configuration is unavailable");
                Self(None)
            }
        }
    }

    fn required(&self) -> Result<&EditorRuntime, ApiError> {
        self.0
            .as_deref()
            .ok_or_else(|| ApiError::from(AiEditorError::Configuration))
    }
}

#[derive(Clone)]
pub struct AiSourceDownloader(pub Arc<dyn ClipDownloader>);

pub fn routes(runtime: AiRuntime) -> Router<PgPool> {
    Router::new()
        .route(
            "/social-media/api/ai/consent",
            get(consent_get).put(consent_put),
        )
        .route(
            "/social-media/api/ai/options",
            get(options_get).put(options_put),
        )
        .route(
            "/social-media/api/ai/jobs",
            get(jobs_list).post(jobs_create),
        )
        .route("/social-media/api/ai/jobs/{id}", get(job_get))
        .route(
            "/social-media/api/ai/jobs/{id}/retry",
            axum::routing::post(job_retry),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/original",
            get(original_file),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/versions/{version}/file",
            get(version_file),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/proposal",
            axum::routing::post(proposal_decide),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/versions/{version}/revision",
            axum::routing::post(version_revise),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/versions/{version}/select",
            axum::routing::post(version_select),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/versions/{version}/decision",
            axum::routing::post(version_decide),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/versions/{version}/approve",
            axum::routing::post(version_approve),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/versions/{version}/schedule",
            axum::routing::post(version_schedule),
        )
        .route(
            "/social-media/api/ai/jobs/{id}/versions/{version}/original",
            get(version_original),
        )
        .route(
            "/social-media/api/ai/videos/{video}/metadata",
            get(metadata_get).put(metadata_put),
        )
        .route(
            "/social-media/api/ai/videos/{video}/editor",
            axum::routing::post(video_editor),
        )
        .layer(Extension(runtime))
}

pub struct ApiError(Box<Response>);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        *self.0
    }
}

impl From<AiEditorError> for ApiError {
    fn from(error: AiEditorError) -> Self {
        if matches!(error, AiEditorError::PipelineBusy) {
            return response_error(StatusCode::CONFLICT, "pipeline_busy");
        }
        let status = match error {
            AiEditorError::Input => StatusCode::BAD_REQUEST,
            AiEditorError::Publishing(ref error) => match error {
                produced_video::ProducedVideoError::NotFound
                | produced_video::ProducedVideoError::Ownership => StatusCode::NOT_FOUND,
                produced_video::ProducedVideoError::Database(_) => StatusCode::SERVICE_UNAVAILABLE,
                produced_video::ProducedVideoError::Invalid
                | produced_video::ProducedVideoError::NoPlatforms => StatusCode::BAD_REQUEST,
                _ => StatusCode::CONFLICT,
            },
            AiEditorError::NotFound
            | AiEditorError::Source
            | AiEditorError::SourceMissing
            | AiEditorError::SourceUnreadable => StatusCode::NOT_FOUND,
            AiEditorError::Consent => StatusCode::FORBIDDEN,
            AiEditorError::Conflict | AiEditorError::Attempts => StatusCode::CONFLICT,
            AiEditorError::Configuration | AiEditorError::Storage => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            _ => StatusCode::BAD_GATEWAY,
        };
        response_error(status, &error.to_string())
    }
}

impl From<store::AiStoreError> for ApiError {
    fn from(error: store::AiStoreError) -> Self {
        AiEditorError::from(error).into()
    }
}

fn response_error(status: StatusCode, text: &str) -> ApiError {
    ApiError(Box::new(
        (status, Json(json!({ "error": text }))).into_response(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelQuery {
    twitch_user_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentInput {
    twitch_user_id: String,
    external_ai_consent: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionsInput {
    twitch_user_id: String,
    multi_output_enabled: bool,
    experimental_ai_layout: bool,
    #[serde(default)]
    publish_language: Option<tb_social_media::autopilot_settings::PublishLanguage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    twitch_user_id: String,
    clip_db_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryInput {
    twitch_user_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionInput {
    twitch_user_id: String,
    feedback: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInput {
    twitch_user_id: String,
    accept: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataInput {
    twitch_user_id: String,
    platforms: std::collections::BTreeMap<String, ai_editor::metadata::PlatformCopy>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformsInput {
    twitch_user_id: String,
    platforms: Vec<String>,
}

async fn channel(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    requested: Option<&str>,
) -> Result<String, ApiError> {
    if !auth.is_authenticated() {
        return Err(response_error(
            StatusCode::UNAUTHORIZED,
            "Bitte melde dich an.",
        ));
    }
    let requested = requested
        .filter(|id| {
            !id.starts_with('0')
                && !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && id.parse::<i64>().is_ok_and(|id| id > 0)
        })
        .ok_or_else(|| {
            response_error(
                StatusCode::BAD_REQUEST,
                "Bitte wähle einen Twitch-Kanal aus.",
            )
        })?;
    if matches!(auth, DashboardAuthLevel::Partner { twitch_user_id, .. } if twitch_user_id != requested)
    {
        return Err(response_error(
            StatusCode::FORBIDDEN,
            "Du kannst nur auf deinen eigenen Twitch-Kanal zugreifen.",
        ));
    }
    super::social_media::required_streamer_scope(auth, pool, Some(requested))
        .await
        .map_err(|response| ApiError(Box::new(response)))
}

async fn consent_view(pool: &PgPool, owner: &str) -> Result<Value, ApiError> {
    let external_ai_consent = store::get_user_consent(pool, owner).await?;
    let global_allowed = tb_llm::social_editor::global_consent(pool)
        .await
        .map_err(AiEditorError::from)?;
    Ok(json!({ "external_ai_consent": external_ai_consent, "global_allowed": global_allowed }))
}

pub async fn consent_get(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Query(query): Query<ChannelQuery>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    Ok(Json(consent_view(&pool, &owner).await?))
}

pub async fn consent_put(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Json(input): Json<ConsentInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    let actor = match &auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => twitch_user_id.as_str(),
        DashboardAuthLevel::Admin { actor: Some(actor) } => actor.twitch_user_id.as_str(),
        DashboardAuthLevel::Admin { actor: None } => "dashboard-admin",
        DashboardAuthLevel::None => {
            return Err(response_error(
                StatusCode::UNAUTHORIZED,
                "Bitte melde dich an.",
            ))
        }
    };
    store::upsert_user_consent(&pool, &owner, input.external_ai_consent, actor).await?;
    Ok(Json(consent_view(&pool, &owner).await?))
}

pub async fn options_get(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Query(query): Query<ChannelQuery>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    Ok(Json(json!(ai_editor::options::load(&pool, &owner).await?)))
}

pub async fn options_put(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Json(input): Json<OptionsInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    let publish_language = match input.publish_language {
        Some(language) => language,
        None => {
            ai_editor::options::load(&pool, &owner)
                .await?
                .publish_language
        }
    };
    let options = ai_editor::EditorOptions {
        multi_output_enabled: input.multi_output_enabled,
        experimental_ai_layout: input.experimental_ai_layout,
        publish_language,
    };
    Ok(Json(json!(
        ai_editor::options::save(&pool, &owner, options).await?
    )))
}

pub async fn jobs_list(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Query(query): Query<ChannelQuery>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    Ok(Json(
        json!({ "items": ai_editor::list_manual_jobs(&pool, runtime.required()?, &owner).await? }),
    ))
}

pub async fn jobs_create(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Extension(downloader): Extension<AiSourceDownloader>,
    Json(input): Json<CreateInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    let job = ai_editor::create_manual_job(
        &pool,
        runtime.required()?,
        &owner,
        input.clip_db_id,
        downloader.0.as_ref(),
    )
    .await?;
    Ok(Json(json!({ "job": job })))
}

pub async fn job_get(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path(id): Path<i64>,
    Query(query): Query<ChannelQuery>,
) -> Result<Json<store::AiJobView>, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    Ok(Json(
        ai_editor::get_manual_job(&pool, runtime.required()?, &owner, id).await?,
    ))
}

pub async fn job_retry(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path(id): Path<i64>,
    Json(input): Json<RetryInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    Ok(Json(
        json!({ "job": ai_editor::retry_manual_job(&pool, runtime.required()?, &owner, id).await? }),
    ))
}

pub async fn version_revise(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path((id, version)): Path<(i64, i64)>,
    Json(input): Json<RevisionInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    Ok(Json(
        json!({"job":ai_editor::revision::create_revision(&pool,runtime.required()?,&owner,id,version,&input.feedback).await?}),
    ))
}

pub async fn proposal_decide(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path(id): Path<i64>,
    Json(input): Json<DecisionInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    Ok(Json(
        json!({"job":ai_editor::revision::decide_proposal(&pool,runtime.required()?,&owner,id,input.accept).await?}),
    ))
}

pub async fn version_select(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path((id, version)): Path<(i64, i64)>,
    Json(input): Json<RetryInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    Ok(Json(
        json!({"job":ai_editor::revision::select_version(&pool,runtime.required()?,&owner,id,version).await?}),
    ))
}

pub async fn version_decide(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path((id, version)): Path<(i64, i64)>,
    Json(input): Json<DecisionInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    ai_editor::revision::record_version_decision(&pool, &owner, id, version, input.accept).await?;
    Ok(Json(
        json!({"job":ai_editor::get_manual_job(&pool,runtime.required()?,&owner,id).await?}),
    ))
}

pub async fn version_approve(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path((id, version)): Path<(i64, i64)>,
    Json(input): Json<PlatformsInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    ai_editor::revision::approve_version(&pool, &owner, id, version, &input.platforms).await?;
    Ok(Json(
        json!({"job":ai_editor::get_manual_job(&pool,runtime.required()?,&owner,id).await?}),
    ))
}

pub async fn version_schedule(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path((id, version)): Path<(i64, i64)>,
    Json(input): Json<PlatformsInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    let reservation =
        ai_editor::revision::publish_version(&pool, &owner, id, version, &input.platforms).await?;
    Ok(Json(
        json!({"reservation":reservation,"job":ai_editor::get_manual_job(&pool,runtime.required()?,&owner,id).await?}),
    ))
}

pub async fn video_editor(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path(video): Path<i64>,
    Json(input): Json<RetryInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    Ok(Json(
        json!({"job": ai_editor::autopilot_revision::open_editor(&pool, runtime.required()?, &owner, video).await?}),
    ))
}

pub async fn metadata_get(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Path(video): Path<i64>,
    Query(query): Query<ChannelQuery>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    Ok(Json(json!(
        ai_editor::metadata::load(&pool, &owner, video).await?
    )))
}

pub async fn metadata_put(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Path(video): Path<i64>,
    Json(input): Json<MetadataInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = channel(&pool, &auth, Some(&input.twitch_user_id)).await?;
    match ai_editor::metadata::save(&pool, &owner, video, input.platforms).await {
        Ok(view) => Ok(Json(json!(view))),
        Err(ai_editor::metadata::MetadataError::Editor(AiEditorError::Conflict)) => {
            Err(response_error(
                StatusCode::CONFLICT,
                "Diese Fassung ist nicht mehr zur Sichtung offen. Titel und Beschreibung lassen sich nicht mehr ändern.",
            ))
        }
        Err(ai_editor::metadata::MetadataError::Editor(error)) => Err(error.into()),
        Err(ai_editor::metadata::MetadataError::Invalid(issues)) => Err(ApiError(Box::new(
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": ai_editor::metadata::summary(&issues),
                    "issues": issues
                })),
            )
                .into_response(),
        ))),
    }
}

pub async fn version_original(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path((id, version)): Path<(i64, i64)>,
    Query(query): Query<ChannelQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    let runtime = runtime.required()?;
    let path = ai_editor::manual_version_original_file(&pool, runtime, &owner, id, version).await?;
    stream_file(
        &runtime.limits.output_root,
        &path,
        runtime.limits.original_preview.max_output_bytes,
        &headers,
    )
    .await
}

pub async fn original_file(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path(id): Path<i64>,
    Query(query): Query<ChannelQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    let runtime = runtime.required()?;
    let path = ai_editor::manual_original_file(&pool, runtime, &owner, id).await?;
    stream_file(
        &runtime.limits.output_root,
        &path,
        runtime.limits.original_preview.max_output_bytes,
        &headers,
    )
    .await
}

pub async fn version_file(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path((id, version)): Path<(i64, i64)>,
    Query(query): Query<ChannelQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    let runtime = runtime.required()?;
    let video = ai_editor::revision::owned_version(&pool, &owner, id, version).await?;
    if !ai_editor::revision::usable(&video) {
        return Err(AiEditorError::NotFound.into());
    }
    let path = PathBuf::from(video.file_path.ok_or(AiEditorError::NotFound)?);
    stream_file(
        &runtime.limits.output_root,
        &path,
        runtime.limits.resources.max_output_bytes,
        &headers,
    )
    .await
}

pub async fn produced_file(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Extension(runtime): Extension<AiRuntime>,
    Path(id): Path<i64>,
    Query(query): Query<ChannelQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let owner = channel(&pool, &auth, query.twitch_user_id.as_deref()).await?;
    let runtime = runtime.required()?;
    let video = produced_video::get(&pool, &owner, id)
        .await
        .map_err(AiEditorError::Publishing)?
        .ok_or(AiEditorError::NotFound)?;
    if video.status == "expired" {
        return Err(AiEditorError::NotFound.into());
    }
    let path = PathBuf::from(video.file_path.ok_or(AiEditorError::NotFound)?);
    stream_file(
        &runtime.limits.output_root,
        &path,
        runtime.limits.resources.max_output_bytes,
        &headers,
    )
    .await
}

fn byte_range(range: Option<&str>, size: u64) -> Result<(u64, u64, bool), ApiError> {
    let invalid = || {
        response_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "Dieser Videoausschnitt ist nicht verfügbar.",
        )
    };
    if size == 0 {
        return Err(invalid());
    }
    let Some(range) = range else {
        return Ok((0, size - 1, false));
    };
    let range = range.strip_prefix("bytes=").ok_or_else(invalid)?;
    let (start, end) = range.split_once('-').ok_or_else(invalid)?;
    if range.contains(',') {
        return Err(invalid());
    }
    let (start, end) = if start.is_empty() {
        let count = end.parse::<u64>().map_err(|_| invalid())?;
        if count == 0 {
            return Err(invalid());
        }
        (size.saturating_sub(count), size - 1)
    } else {
        let start = start.parse::<u64>().map_err(|_| invalid())?;
        let end = if end.is_empty() {
            size - 1
        } else {
            end.parse::<u64>().map_err(|_| invalid())?.min(size - 1)
        };
        (start, end)
    };
    if start >= size || end < start {
        return Err(invalid());
    }
    Ok((start, end, true))
}

async fn stream_file(
    root: &FilePath,
    path: &FilePath,
    max_bytes: u64,
    headers: &HeaderMap,
) -> Result<Response, ApiError> {
    let path = ai_editor::authorized_path(root, path, max_bytes)?;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| AiEditorError::NotFound)?;
    let metadata = file.metadata().await.map_err(|_| AiEditorError::Source)?;
    let size = metadata.len();
    if !metadata.is_file() || size == 0 || size > max_bytes {
        return Err(AiEditorError::Source.into());
    }
    if headers.get_all(header::RANGE).iter().count() > 1 {
        return Err(response_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "Dieser Videoausschnitt ist nicht verfügbar.",
        ));
    }
    let range = headers
        .get(header::RANGE)
        .map(|value| {
            value.to_str().map_err(|_| {
                response_error(
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "Dieser Videoausschnitt ist nicht verfügbar.",
                )
            })
        })
        .transpose()?;
    let (start, end, partial) = byte_range(range, size)?;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|_| AiEditorError::Source)?;
    let length = end - start + 1;
    let stream =
        futures_util::stream::try_unfold((file, length), |(mut file, remaining)| async move {
            if remaining == 0 {
                return Ok::<_, std::io::Error>(None);
            }
            let mut buffer = vec![0_u8; remaining.min(65_536) as usize];
            let count = file.read(&mut buffer).await?;
            if count == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            buffer.truncate(count);
            Ok(Some((buffer, (file, remaining - count as u64))))
        });
    let mut builder = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store")
        .header("x-content-type-options", "nosniff");
    if partial {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|_| AiEditorError::Storage.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_ai_request_requires_explicit_channel_and_partner_cannot_choose_foreign_id() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unused@localhost/unused")
            .expect("no database connection");
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "fixture".into(),
            twitch_user_id: "101".into(),
            display_name: "Fixture".into(),
        };
        for auth in [&partner, &DashboardAuthLevel::admin()] {
            for requested in [
                None,
                Some(""),
                Some("0101"),
                Some("fixture"),
                Some("9223372036854775808"),
            ] {
                let error = channel(&pool, auth, requested)
                    .await
                    .expect_err("explicit numeric channel");
                assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
            }
        }
        assert_eq!(
            channel(&pool, &partner, Some("202"))
                .await
                .expect_err("foreign channel")
                .into_response()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            channel(&pool, &DashboardAuthLevel::None, Some("101"))
                .await
                .expect_err("authentication")
                .into_response()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn media_stream_is_bounded_and_uses_unchanged_source_bytes() {
        let directory = tempfile::tempdir().expect("media root");
        let original = directory.path().join("original.mp4");
        tokio::fs::write(&original, b"original bytes")
            .await
            .expect("synthetic original bytes");
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, "bytes=2-6".parse().expect("range"));
        let response = stream_file(directory.path(), &original, 32, &headers)
            .await
            .map_err(|_| ())
            .expect("bounded source stream");
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-6/14");
        let bytes = axum::body::to_bytes(response.into_body(), 32)
            .await
            .expect("streamed bytes");
        assert_eq!(&bytes[..], b"igina");
        assert!(
            stream_file(directory.path(), &original, 1, &HeaderMap::new())
                .await
                .is_err()
        );
        let foreign = tempfile::NamedTempFile::new().expect("foreign root");
        assert!(
            stream_file(directory.path(), foreign.path(), 32, &HeaderMap::new())
                .await
                .is_err()
        );
    }

    #[test]
    fn bounded_single_ranges() {
        assert_eq!(byte_range(None, 10).map_err(|_| ()).unwrap(), (0, 9, false));
        assert_eq!(
            byte_range(Some("bytes=2-5"), 10).map_err(|_| ()).unwrap(),
            (2, 5, true)
        );
        assert_eq!(
            byte_range(Some("bytes=-3"), 10).map_err(|_| ()).unwrap(),
            (7, 9, true)
        );
        assert_eq!(
            byte_range(Some("bytes=2-999"), 10).map_err(|_| ()).unwrap(),
            (2, 9, true)
        );
        for range in [
            "bytes=10-",
            "bytes=2-1",
            "bytes=-0",
            "bytes=0-1,3-4",
            "bad",
            "bytes=0-18446744073709551616",
        ] {
            assert!(byte_range(Some(range), 10).is_err(), "{range}");
        }
    }
}

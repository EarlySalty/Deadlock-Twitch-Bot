use super::*;
use tb_social_media::credentials::SocialMediaCredentials;
use tb_social_media::uploaders::tiktok::{
    build_caption, video_digest, CreatorInfo, TikTokPostOptions, TikTokUploader,
};

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"error": "tiktok_posting", "message": message})),
    )
        .into_response()
}

async fn credentials(pool: &PgPool, clip_id: i64) -> Result<SocialMediaCredentials, Response> {
    let user: Option<String> =
        sqlx::query_scalar("SELECT twitch_user_id FROM twitch_clips_social_media WHERE id = $1")
            .bind(clip_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| {
                error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Das TikTok-Konto konnte nicht geladen werden.",
                )
            })?
            .flatten();
    let user = user.ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "Dieser Clip ist keinem Kanal zugeordnet.",
        )
    })?;
    let manager = build_credential_manager(pool.clone()).ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die TikTok-Verbindung ist gerade nicht verfügbar.",
        )
    })?;
    let credentials = manager
        .get_channel_credentials_for_id("tiktok", &user)
        .await
        .ok_or_else(|| {
            error(
                StatusCode::BAD_REQUEST,
                "Bitte verbinde dein TikTok-Konto im Dashboard erneut.",
            )
        })?;
    if !credentials
        .scopes
        .as_deref()
        .unwrap_or("")
        .split([',', ' '])
        .any(|scope| scope == "video.publish")
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Bitte verbinde TikTok erneut, damit Clips direkt veröffentlicht werden können.",
        ));
    }
    if credentials
        .platform_user_id
        .as_deref()
        .is_none_or(str::is_empty)
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Das TikTok-Konto konnte nicht eindeutig erkannt werden. Bitte verbinde es erneut.",
        ));
    }
    Ok(credentials)
}

async fn creator(credentials: &SocialMediaCredentials) -> Result<CreatorInfo, Response> {
    TikTokUploader::new(credentials.access_token.clone())
        .query_creator_info()
        .await
        .map_err(|_| {
            error(
        StatusCode::BAD_GATEWAY,
        "TikTok kann die Veröffentlichung gerade nicht freigeben. Bitte versuche es später erneut.",
    )
        })
}

async fn duration(pool: &PgPool, clip_id: i64) -> Result<(String, f64, String), Response> {
    let preview = get_preview(pool, clip_id).await.ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "Bitte erstelle zuerst die Videovorschau für diesen Clip.",
        )
    })?;
    match preview.status.as_deref() {
        Some(tb_social_media::preview::PREVIEW_PENDING) => {
            return Err(error(
                StatusCode::CONFLICT,
                "Die Videovorschau ist angefordert. Bitte warte, bis sie fertig ist.",
            ));
        }
        Some(tb_social_media::preview::PREVIEW_RENDERING) => {
            return Err(error(
                StatusCode::CONFLICT,
                "Die Videovorschau wird gerade erstellt. Bitte warte, bis sie fertig ist.",
            ));
        }
        Some(tb_social_media::preview::PREVIEW_ERROR) => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Die Videovorschau konnte nicht erstellt werden. Bitte fordere sie erneut an.",
            ));
        }
        Some(PREVIEW_READY) => {}
        _ => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Für diesen Clip fehlt eine gültige Videovorschau. Bitte erstelle sie erneut.",
            ));
        }
    }
    let path = preview.path.ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "Bitte erstelle die Videovorschau erneut.",
        )
    })?;
    let duration = VideoProcessor::default()
        .get_video_info(&path)
        .await
        .map(|info| info.duration)
        .map_err(|_| {
            error(
                StatusCode::BAD_REQUEST,
                "Die Videovorschau konnte nicht geprüft werden. Bitte erstelle sie erneut.",
            )
        })?;
    let digest = video_digest(&path).await.map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "Bitte erstelle die Videovorschau erneut.",
        )
    })?;
    Ok((path, duration, digest))
}

#[derive(Debug, Default, Deserialize)]
pub struct CreatorInfoQuery {
    #[serde(default)]
    produced_video_id: Option<i64>,
}

fn subject_error(failure: tb_social_media::autopilot_publish::PublishError) -> Response {
    use tb_social_media::autopilot_publish::PublishError;
    match failure {
        PublishError::NotFound => version_missing(),
        PublishError::State | PublishError::Io(_) => error(
            StatusCode::CONFLICT,
            "Diese Version ist nicht mehr verfügbar. Bitte wähle eine aktuelle Version.",
        ),
        _ => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Die TikTok-Freigabe konnte nicht geladen werden.",
        ),
    }
}

fn version_missing() -> Response {
    error(StatusCode::NOT_FOUND, "Diese Version wurde nicht gefunden.")
}

async fn version_info(pool: &PgPool, clip_id: i64, video_id: i64) -> Response {
    let user: Option<String> = match sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_clips_social_media WHERE id = $1",
    )
    .bind(clip_id)
    .fetch_optional(pool)
    .await
    {
        Ok(user) => user.flatten(),
        Err(_) => return clip_load_failed(),
    };
    let Some(user) = user else {
        return invalid_clip_db_id();
    };
    let subject =
        match tb_social_media::autopilot_publish::tiktok_subject(pool, &user, video_id).await {
            Ok(subject) if subject.clip_id == clip_id => subject,
            Ok(_) => return version_missing(),
            Err(failure) => return subject_error(failure),
        };
    let credentials = match credentials(pool, clip_id).await {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
    if Some(credentials.id) != subject.credential_id
        || credentials.platform_user_id != subject.platform_user_id
    {
        return error(
            StatusCode::CONFLICT,
            "Das TikTok-Konto hat sich geändert. Bitte öffne die Freigabe erneut.",
        );
    }
    let info = match creator(&credentials).await {
        Ok(info) => info,
        Err(response) => return response,
    };
    let duration = subject.duration_ms.unwrap_or_default() as f64 / 1000.0;
    if duration <= 0.0 || duration > info.max_video_post_duration_sec as f64 {
        return error(
            StatusCode::BAD_REQUEST,
            "Die Videodauer passt nicht zu diesem TikTok-Konto. Bitte kürze den Clip.",
        );
    }
    let (draft, defaults) = match tiktok_editor::saved_choices(pool, clip_id, &user).await {
        Ok(choices) => choices,
        Err(response) => return response,
    };
    let bound = subject.binding().ok().cloned().map(|mut options| {
        options.video_path.clear();
        options
    });
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "creator": info,
            "caption": subject.caption,
            "duration_seconds": duration,
            "approved_video_sha256": subject.approved_video_sha256,
            "credential_id": credentials.id,
            "platform_user_id": credentials.platform_user_id,
            "produced_video_id": subject.video_id,
            "music_consent": subject.music_consent,
            "draft": draft,
            "defaults": defaults,
            "bound_options": bound,
        })),
    )
        .into_response()
}

pub async fn creator_info_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    Query(query): Query<CreatorInfoQuery>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let Some(clip_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(response) = require_clip_in_scope(&pool, &auth, clip_id, scope.as_deref()).await {
        return response;
    }
    if let Some(response) = guard_partner_access_for_clip(&pool, &auth, clip_id).await {
        return response;
    }
    if let Some(video_id) = query.produced_video_id {
        return version_info(&pool, clip_id, video_id).await;
    }
    let credentials = match credentials(&pool, clip_id).await {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
    let info = match creator(&credentials).await {
        Ok(info) => info,
        Err(response) => return response,
    };
    let (_, duration, digest) = match duration(&pool, clip_id).await {
        Ok(duration) => duration,
        Err(response) => return response,
    };
    if !duration.is_finite()
        || duration <= 0.0
        || duration > info.max_video_post_duration_sec as f64
    {
        return error(
            StatusCode::BAD_REQUEST,
            "Die Videodauer passt nicht zu diesem TikTok-Konto. Bitte kürze den Clip.",
        );
    }
    let enrichment = match i32::try_from(clip_id) {
        Ok(id) => get_enrichment(&pool, id).await,
        Err(_) => None,
    };
    let clip: Option<(Option<String>, Option<String>, Option<String>)> = match sqlx::query_as(
        "SELECT clip_title, custom_description, hashtags FROM twitch_clips_social_media WHERE id = $1",
    ).bind(clip_id).fetch_optional(&pool).await {
        Ok(clip) => clip,
        Err(_) => return clip_load_failed(),
    };
    let Some((title, description, tags)) = clip else {
        return invalid_clip_db_id();
    };
    let fallback_tags: Vec<String> = tags
        .as_deref()
        .and_then(|tags| serde_json::from_str(tags).ok())
        .unwrap_or_default();
    let caption = match enrichment {
        Some(enrichment) => build_caption(
            enrichment
                .title_tiktok
                .as_deref()
                .or(title.as_deref())
                .unwrap_or(""),
            enrichment
                .description_tiktok
                .as_deref()
                .or(description.as_deref())
                .unwrap_or(""),
            &enrichment.hashtags_tiktok,
        ),
        None => build_caption(
            title.as_deref().unwrap_or(""),
            description.as_deref().unwrap_or(""),
            &fallback_tags,
        ),
    };
    Json(json!({
        "creator": info,
        "caption": caption,
        "duration_seconds": duration,
        "approved_video_sha256": digest,
        "credential_id": credentials.id,
        "platform_user_id": credentials.platform_user_id,
    }))
    .into_response()
}

pub(super) async fn save_choice(
    pool: &PgPool,
    clip_id: i64,
    raw: Option<&Value>,
) -> Result<(), Response> {
    let mut options: TikTokPostOptions = raw.cloned().and_then(|raw| serde_json::from_value(raw).ok()).ok_or_else(|| error(
        StatusCode::BAD_REQUEST, "Bitte öffne die TikTok-Freigabe am Clip und wähle die Veröffentlichungseinstellungen.",
    ))?;
    let credentials = credentials(pool, clip_id).await?;
    if options.credential_id != credentials.id
        || Some(options.platform_user_id.as_str()) != credentials.platform_user_id.as_deref()
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Das TikTok-Konto hat sich geändert. Bitte öffne die Freigabe erneut.",
        ));
    }
    let user: String =
        sqlx::query_scalar("SELECT twitch_user_id FROM twitch_clips_social_media WHERE id = $1")
            .bind(clip_id)
            .fetch_one(pool)
            .await
            .map_err(|_| clip_load_failed())?;
    if !tiktok_editor::valid_music_consent(pool, &user, credentials.platform_user_id.as_deref())
        .await?
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Bitte bestätige zuerst einmalig die Musiknutzung für dieses TikTok-Konto.",
        ));
    }
    let info = creator(&credentials).await?;
    let (path, duration, digest) = duration(pool, clip_id).await?;
    if options.approved_video_sha256 != digest {
        return Err(error(
            StatusCode::CONFLICT,
            "Die Videovorschau hat sich geändert. Bitte öffne die TikTok-Freigabe erneut.",
        ));
    }
    options.video_path = path;
    options.validate(&info, duration).map_err(|error_value| {
        if let tb_social_media::uploaders::UploadError::Validation(message) = error_value {
            error(StatusCode::BAD_REQUEST, &message)
        } else {
            error(
                StatusCode::BAD_REQUEST,
                "Bitte prüfe die TikTok-Freigabe erneut.",
            )
        }
    })?;
    let options = serde_json::to_value(options).map_err(|_| invalid_payload())?;
    persist_choice(pool, clip_id, options).await
}

pub(super) async fn persist_choice(
    pool: &PgPool,
    clip_id: i64,
    options: Value,
) -> Result<(), Response> {
    let mut tx = pool.begin().await.map_err(|_| clip_load_failed())?;
    sqlx::query("SELECT id FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE")
        .bind(clip_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| clip_load_failed())?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM twitch_clips_upload_queue WHERE clip_id = $1 AND platform = 'tiktok' AND (status IN ('processing', 'inbox', 'inbox_pending', 'completed') OR (tiktok_publish_id IS NOT NULL AND NOT (status = 'failed' AND COALESCE(tiktok_publish_status = 'FAILED', FALSE)))))",
    ).bind(clip_id).fetch_one(&mut *tx).await.map_err(|_| clip_load_failed())?;
    if active {
        return Err(error(StatusCode::CONFLICT, "Für diesen Clip hat die TikTok-Übertragung bereits begonnen. Ein weiterer Upload bleibt gesperrt."));
    }
    sqlx::query("UPDATE twitch_clips_social_media SET tiktok_post_options = $1 WHERE id = $2")
        .bind(&options)
        .bind(clip_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| clip_load_failed())?;
    tb_social_media::clip_queue::apply_tiktok_choice(&mut tx, clip_id, &options)
        .await
        .map_err(|_| clip_load_failed())?;
    tx.commit().await.map_err(|_| clip_load_failed())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Method, Request, StatusCode};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    #[tokio::test]
    async fn cross_origin_tiktok_approval_is_rejected_before_binding() {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .expect("lazy pool");
        let response = crate::build_router(pool, "tok".into())
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/social-media/api/autopilot/publish/videos/1/approval")
                    .header(header::HOST, "dashboard.example.com")
                    .header(header::ORIGIN, "https://evil.example")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"destinations":[{"platform":"tiktok","tiktok_options":{"consent":true}}]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(String::from_utf8_lossy(&body).contains("invalid_csrf"));
    }
}

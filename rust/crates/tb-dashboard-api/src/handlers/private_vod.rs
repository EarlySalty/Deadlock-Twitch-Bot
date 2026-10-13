use std::sync::Arc;

use axum::{
    body::Body,
    extract::{FromRequestParts, Path, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_vod_archive::private_range::{PrivateRangePaths, RangeJob};
use tokio::{io::AsyncReadExt, sync::Semaphore};

use crate::auth::level::DashboardAuthLevel;

#[derive(Clone)]
pub struct PrivateVodRuntime {
    pub paths: PrivateRangePaths,
    pub downloads: Arc<Semaphore>,
}

fn owner_id(auth: &DashboardAuthLevel) -> Result<&str, (StatusCode, &'static str)> {
    let id = match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => Some(twitch_user_id.as_str()),
        DashboardAuthLevel::Admin { actor: Some(actor) } => Some(actor.twitch_user_id.as_str()),
        DashboardAuthLevel::None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Bitte melde dich mit deinem Twitch-Konto an.",
            ))
        }
        _ => None,
    };
    id.filter(|id| {
        !id.is_empty() && !id.starts_with('0') && id.bytes().all(|byte| byte.is_ascii_digit())
    })
    .ok_or((
        StatusCode::FORBIDDEN,
        "Für diesen Download ist die Anmeldung des Kanalinhabers erforderlich.",
    ))
}

pub struct PrivateVodOwner(String);

impl<S: Send + Sync> FromRequestParts<S> for PrivateVodOwner {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = match DashboardAuthLevel::from_request_parts(parts, state).await {
            Ok(auth) => auth,
            Err(never) => match never {},
        };
        let owner = owner_id(&auth)
            .map_err(|(status, text)| error(status, text))?
            .to_string();
        let denied = || {
            error(
                StatusCode::FORBIDDEN,
                "Für diesen Download ist die Anmeldung des Kanalinhabers erforderlich.",
            )
        };
        let auth_state = parts
            .extensions
            .get::<crate::auth::session::DashboardAuthState>()
            .cloned()
            .ok_or_else(denied)?;
        let session_id =
            crate::auth::level::extract_cookie(parts, crate::auth::session::PARTNER_COOKIE_NAME);
        let partner = if let Some(session_id) = session_id {
            auth_state
                .load_partner_session(session_id)
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        if partner.is_some_and(|partner| partner.twitch_user_id == owner) {
            Ok(Self(owner))
        } else {
            Err(denied())
        }
    }
}

fn error(status: StatusCode, text: &str) -> Response {
    no_store((status, Json(json!({"error":text}))).into_response())
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response
}

pub async fn list_handler(
    PrivateVodOwner(owner): PrivateVodOwner,
    State(pool): State<PgPool>,
) -> Response {
    let result = async {
        let enabled: bool = sqlx::query_scalar("SELECT COALESCE((SELECT enabled FROM twitch_private_vod_settings WHERE owner_id=$1),false)").bind(&owner).fetch_one(&pool).await?;
        let jobs = sqlx::query_as::<_,RangeJob>("SELECT id,owner_id,video_id,start_seconds,end_seconds,status,created_at,expires_at,file_bytes,error_code FROM twitch_private_vod_jobs WHERE owner_id=$1 AND expires_at>now() ORDER BY created_at DESC LIMIT 20").bind(&owner).fetch_all(&pool).await?;
        Ok::<_,sqlx::Error>((enabled,jobs))
    }.await;
    match result {
        Ok((enabled, jobs)) => {
            no_store(Json(json!({"enabled":enabled,"jobs":jobs})).into_response())
        }
        Err(error_value) => {
            tracing::error!(database_code=?error_value.as_database_error().and_then(|error| error.code()),"Private VOD list failed");
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Die VOD-Ausschnitte sind gerade nicht abrufbar.",
            )
        }
    }
}

#[derive(Deserialize)]
pub struct Settings {
    pub enabled: bool,
}

pub async fn settings_handler(
    PrivateVodOwner(owner): PrivateVodOwner,
    State(pool): State<PgPool>,
    Json(settings): Json<Settings>,
) -> Response {
    match sqlx::query("INSERT INTO twitch_private_vod_settings (owner_id,enabled) VALUES ($1,$2) ON CONFLICT (owner_id) DO UPDATE SET enabled=EXCLUDED.enabled").bind(&owner).bind(settings.enabled).execute(&pool).await {
        Ok(_)=>no_store(Json(json!({"enabled":settings.enabled})).into_response()),
        Err(error_value)=>{ tracing::error!(database_code=?error_value.as_database_error().and_then(|error| error.code()),"Private VOD settings failed"); error(StatusCode::SERVICE_UNAVAILABLE,"Die Einstellung konnte nicht gespeichert werden.") }
    }
}

pub async fn download_handler(
    PrivateVodOwner(owner): PrivateVodOwner,
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    runtime: Option<Extension<PrivateVodRuntime>>,
) -> Response {
    let Some(Extension(runtime)) = runtime else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Der Download ist gerade nicht verfügbar.",
        );
    };
    let Some(path) = runtime.paths.file(&id) else {
        return error(
            StatusCode::NOT_FOUND,
            "Dieser VOD-Ausschnitt ist nicht verfügbar.",
        );
    };
    let job = sqlx::query_as::<_,RangeJob>("SELECT id,owner_id,video_id,start_seconds,end_seconds,status,created_at,expires_at,file_bytes,error_code FROM twitch_private_vod_jobs WHERE id=$1 AND owner_id=$2 AND status='ready' AND expires_at>now()")
        .bind(&id).bind(&owner).fetch_optional(&pool).await;
    let job = match job {
        Ok(Some(job)) => job,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "Dieser VOD-Ausschnitt ist nicht verfügbar oder bereits abgelaufen.",
            )
        }
        Err(error_value) => {
            tracing::error!(database_code=?error_value.as_database_error().and_then(|error| error.code()),"Private VOD download lookup failed");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Der Download ist gerade nicht verfügbar.",
            );
        }
    };
    let permit = match runtime.downloads.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "Es laufen bereits Downloads. Versuch es gleich nochmal.",
            )
        }
    };
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(_) => {
            return error(
                StatusCode::NOT_FOUND,
                "Die Videodatei ist nicht mehr verfügbar.",
            )
        }
    };
    let size = match file.metadata().await {
        Ok(meta)
            if meta.is_file()
                && meta.len() > 0
                && meta.len() <= tb_domain::vod_range::MAX_FILE_BYTES
                && Some(meta.len() as i64) == job.file_bytes =>
        {
            meta.len()
        }
        _ => return error(StatusCode::NOT_FOUND, "Die Videodatei ist nicht verfügbar."),
    };
    let body = download_body(file, size, job.expires_at, permit);
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("video/mp4"),
    );
    if let Ok(value) = size.to_string().parse() {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    if let Ok(value) = format!(
        "attachment; filename=\"vodclip-{}-{}.mp4\"",
        job.start_seconds, job.end_seconds
    )
    .parse()
    {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    no_store(response)
}

fn download_body(
    mut file: tokio::fs::File,
    mut remaining: u64,
    expires: chrono::DateTime<chrono::Utc>,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Body {
    let until_expiry = (expires - chrono::Utc::now()).to_std().unwrap_or_default();
    let deadline =
        tokio::time::Instant::now() + until_expiry.min(std::time::Duration::from_secs(300));
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(2);
    let (finished, completion) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _permit = permit;
        let transfer = async {
            while remaining > 0 {
                let mut chunk = vec![0u8; remaining.min(65536) as usize];
                let count = file.read(&mut chunk).await?;
                if count == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "VOD download incomplete",
                    ));
                }
                chunk.truncate(count);
                remaining -= count as u64;
                if sender.send(Ok(chunk)).await.is_err() {
                    return Ok(());
                }
            }
            Ok::<_, std::io::Error>(())
        };
        match tokio::time::timeout_at(deadline, transfer).await {
            Ok(Err(error)) => {
                let _ = sender.try_send(Err(error));
            }
            Err(_) => {
                let _ = sender.try_send(Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "VOD download expired",
                )));
            }
            Ok(Ok(())) => {}
        }
        drop(file);
        drop(sender);
        let _ = tokio::time::timeout_at(deadline, completion).await;
    });
    let stream = futures_util::stream::try_unfold(
        (receiver, finished),
        move |(mut receiver, finished)| async move {
            let next = match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(next) => next,
                Err(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "VOD download expired",
                    ));
                }
            };
            if tokio::time::Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "VOD download expired",
                ));
            }
            match next {
                Some(Ok(chunk)) => Ok(Some((chunk, (receiver, finished)))),
                Some(Err(error)) => Err(error),
                None => Ok(None),
            }
        },
    );
    Body::from_stream(stream)
}

#[cfg(test)]
#[path = "private_vod_tests.rs"]
mod tests;

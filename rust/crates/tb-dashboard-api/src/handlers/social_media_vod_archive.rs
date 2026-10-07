#![allow(clippy::result_large_err)]

use super::*;
use sqlx::Row;

#[cfg(test)]
#[path = "social_media_vod_archive_tests.rs"]
mod tests;

#[derive(Deserialize)]
pub struct ArchiveQuery {
    pub twitch_user_id: Option<String>,
    pub page: Option<i64>,
}

pub async fn list_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<ArchiveQuery>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, q.twitch_user_id.as_deref()).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let page = q.page.unwrap_or(1).clamp(1, 100_000);
    let rows = sqlx::query(
        "WITH visible AS (SELECT * FROM twitch_vod_archive_vods \
            WHERE hidden_at IS NULL AND ($1::text IS NULL OR twitch_user_id = $1)), \
         paged AS (SELECT v.id, v.twitch_id, v.streamer_login, v.twitch_user_id, v.title, v.duration_sec, \
         v.recorded_at, v.discovered_at, v.status, v.last_error, v.drive_url, v.drive_requested, \
         v.last_attempt_at, v.uploaded_at, \
         CASE WHEN c.vod_id IS NOT NULL THEN jsonb_build_object('state',c.state,'complete',c.complete,'observations',c.observations,'last_attempt_at',c.last_attempt_at,'last_success_at',c.last_success_at,'error',c.last_error,'pending',c.requested_at IS NOT NULL,'can_request',c.requested_at IS NULL AND (c.last_attempt_at IS NULL OR c.last_attempt_at<NOW()-INTERVAL '10 minutes') AND (c.last_error IS DISTINCT FROM 'quota' OR c.next_check_at<=NOW())) END AS youtube_check, \
         a.id IS NOT NULL AS can_check_youtube, \
         COALESCE((SELECT jsonb_agg(jsonb_build_object('index', p.part_index, 'status', p.status, \
            'youtube_video_id', p.youtube_video_id) ORDER BY p.part_index) \
            FROM twitch_vod_archive_parts p WHERE p.vod_id = v.id), '[]'::jsonb) AS parts, \
         COALESCE(NOT a.needs_reauth AND a.access_token_enc IS NOT NULL \
            AND string_to_array(COALESCE(a.scopes, ''), ' ') && ARRAY['https://www.googleapis.com/auth/youtube.upload', 'https://www.googleapis.com/auth/youtube', 'https://www.googleapis.com/auth/youtube.force-ssl'], FALSE) AS youtube_connected, \
         CASE WHEN a.refresh_token_enc IS NOT NULL THEN a.refresh_expires_at::text ELSE a.token_expires_at END AS youtube_expires_at \
         FROM visible v \
         LEFT JOIN LATERAL (SELECT a.* FROM social_media_platform_auth a \
            WHERE a.twitch_user_id=v.twitch_user_id AND a.platform='youtube' AND a.enabled=1 \
            ORDER BY a.authorized_at DESC, a.id DESC LIMIT 1) a ON TRUE \
         LEFT JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id AND c.auth_id=a.id \
           AND c.auth_revision=md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')) \
           AND c.upload_snapshot=(SELECT COALESCE(jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index),'[]'::jsonb) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id) \
         ORDER BY v.discovered_at DESC, v.id DESC LIMIT 50 OFFSET $2) \
         SELECT paged.*, totals.total FROM (SELECT COUNT(*) AS total FROM visible) totals \
         LEFT JOIN paged ON TRUE ORDER BY paged.discovered_at DESC, paged.id DESC",
    )
    .bind(scope)
    .bind((page - 1) * 50)
    .fetch_all(&pool)
    .await;
    match rows {
        Ok(rows) => {
            let total = rows
                .first()
                .map(|row| row.get::<i64, _>("total"))
                .unwrap_or(0);
            let items: Vec<Value> = rows.into_iter().filter(|row| row.get::<Option<i64>, _>("id").is_some()).map(|row| {
                let status: String = row.get("status");
                let parts: Value = row.get("parts");
                let expires: Option<String> = row.get("youtube_expires_at");
                let connected: bool = row.get::<bool, _>("youtube_connected") && expires.as_deref().is_none_or(|value| {
                    chrono::DateTime::parse_from_rfc3339(value).or_else(|_| chrono::DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f%#z"))
                        .is_ok_and(|expires| expires > chrono::Utc::now())
                });
                let last_error: Option<String> = row.get("last_error");
                let drive_requested: bool = row.get("drive_requested");
                let drive_url: Option<String> = row.get("drive_url");
                let uploaded_at: Option<chrono::DateTime<chrono::Utc>> = row.get("uploaded_at");
                let progress = archive_progress(&status, &parts, drive_url.as_deref(), uploaded_at.is_some());
                let check: Option<Value> = row.get("youtube_check");
                let check_state = check.as_ref().and_then(|c| c["state"].as_str());
                let checked_status = youtube_check_label(check_state);
                json!({
                    "youtube_check": check,
                    "can_check_youtube": row.get::<bool, _>("can_check_youtube"),
                    "youtube_verified_complete": check.as_ref().is_some_and(|c| c["complete"] == true && c["state"] == "confirmed"),
                    "id": row.get::<i64, _>("id"),
                    "twitch_id": row.get::<String, _>("twitch_id"),
                    "channel": row.get::<String, _>("streamer_login"),
                    "twitch_user_id": row.get::<Option<String>, _>("twitch_user_id"),
                    "title": row.get::<String, _>("title"),
                    "duration_sec": row.get::<i64, _>("duration_sec"),
                    "recorded_at": row.get::<Option<chrono::NaiveDate>, _>("recorded_at"),
                    "discovered_at": row.get::<chrono::DateTime<chrono::Utc>, _>("discovered_at"),
                    "status": status, "status_label": checked_status.map_or(progress.label, |(_,label)| label),
                    "display_status": checked_status.map_or(progress.state, |(state,_)| state),
                    "youtube_complete": progress.youtube_complete,
                    "drive_complete": progress.drive_complete,
                    "confirmed_parts": progress.confirmed_parts,
                    "total_parts": progress.total_parts,
                    "can_retry": progress.can_retry,
                    "reason": if check.is_some() { None } else if parts.as_array().is_some_and(|parts| parts.iter().any(|part| part["status"] == "rejected")) && !progress.drive_complete { Some("YouTube hat einen Upload abgelehnt oder entfernt. Prüfe das Ziel und die YouTube-Verbindung.") } else if progress.state == "unknown" { Some("Für diesen früheren Upload fehlt ein vollständiger Nachweis. Der YouTube-Abgleich kann vorhandene Videos zuordnen.") } else { error_label(&status, last_error.as_deref(), drive_requested) },
                    "drive_url": drive_url,
                    "drive_requested": drive_requested,
                    "last_attempt_at": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_attempt_at"),
                    "uploaded_at": uploaded_at,
                    "parts": parts,
                    "needs_connection": !connected,
                })
            }).collect();
            Json(json!({"items": items, "total": total, "page": page})).into_response()
        }
        Err(error) => {
            tracing::error!(%error, "VOD-Liste nicht lesbar");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Die VOD-Liste konnte nicht geladen werden."})),
            )
                .into_response()
        }
    }
}

fn youtube_check_label(state: Option<&str>) -> Option<(&'static str, &'static str)> {
    match state? {
        "confirmed" => Some(("youtube_confirmed", "Auf YouTube bestätigt")),
        "processing" => Some(("youtube_processing", "YouTube verarbeitet das Video")),
        "rejected" => Some(("failed", "YouTube hat das Video abgelehnt")),
        "unavailable" => Some(("youtube_unavailable", "Bei YouTube nicht abrufbar")),
        "error" => Some(("youtube_error", "Prüfung gerade nicht möglich")),
        "partial" => Some(("partial", "YouTube-Zuordnung noch unvollständig")),
        "unresolved" => Some(("unknown", "Bei YouTube nicht eindeutig zugeordnet")),
        "searching" => Some(("waiting", "YouTube-Abgleich läuft")),
        _ => None,
    }
}

struct ArchiveProgress {
    state: &'static str,
    label: &'static str,
    youtube_complete: bool,
    drive_complete: bool,
    confirmed_parts: usize,
    total_parts: usize,
    can_retry: bool,
}

fn archive_progress(
    status: &str,
    parts: &Value,
    drive_url: Option<&str>,
    has_uploaded_at: bool,
) -> ArchiveProgress {
    let parts = parts.as_array().map(Vec::as_slice).unwrap_or_default();
    let confirmed_parts = parts
        .iter()
        .filter(|part| {
            part["status"] == "done"
                && part["youtube_video_id"]
                    .as_str()
                    .is_some_and(|id| !id.trim().is_empty())
        })
        .count();
    let youtube_complete = matches!(status, "uploaded" | "archived")
        && !parts.is_empty()
        && confirmed_parts == parts.len();
    let drive_complete = status == "drive_uploaded"
        && has_uploaded_at
        && drive_url.is_some_and(|url| {
            url.starts_with("https://drive.google.com/") && !url.chars().any(char::is_control)
        });
    let terminal = matches!(status, "uploaded" | "archived" | "drive_uploaded");
    let (state, label) = if youtube_complete {
        ("youtube_uploaded", "YouTube-Upload abgeschlossen")
    } else if drive_complete {
        ("drive_uploaded", "Auf Drive gesichert")
    } else if terminal && parts.iter().any(|part| part["status"] == "rejected") {
        ("failed", "YouTube-Upload abgelehnt")
    } else if terminal {
        ("unknown", "Abschluss unklar")
    } else if status == "downloading" {
        ("downloading", "Lädt herunter")
    } else if status == "uploading" {
        ("uploading", "Lädt hoch")
    } else if confirmed_parts > 0 {
        ("partial", "Teilweise auf YouTube")
    } else if matches!(status, "download_failed" | "upload_failed" | "unavailable")
        || parts
            .iter()
            .any(|part| matches!(part["status"].as_str(), Some("failed" | "rejected")))
    {
        ("failed", "Fehlgeschlagen")
    } else if matches!(status, "new" | "downloaded") {
        ("waiting", "Wartet")
    } else {
        ("unknown", "Status unklar")
    };
    ArchiveProgress {
        state,
        label,
        youtube_complete,
        drive_complete,
        confirmed_parts,
        total_parts: parts.len(),
        can_retry: !terminal
            && matches!(state, "waiting" | "partial" | "failed")
            && status != "unavailable",
    }
}

fn error_label(
    status: &str,
    last_error: Option<&str>,
    drive_requested: bool,
) -> Option<&'static str> {
    match status {
        "upload_failed" if drive_requested => {
            Some("Die Sicherung auf Drive ist fehlgeschlagen. Du kannst sie erneut versuchen.")
        }
        "upload_failed" if last_error.is_some_and(|error| error.contains("quotaExceeded")) => Some(
            "YouTube nimmt heute keine weiteren Uploads an. Das Archiv versucht es bei einem späteren Lauf erneut.",
        ),
        "download_failed" => {
            Some("Der Download ist fehlgeschlagen. Du kannst ihn erneut versuchen.")
        }
        "upload_failed" => Some(
            "Der Upload ist fehlgeschlagen. Prüfe die YouTube-Verbindung und versuche es erneut.",
        ),
        "unavailable" => Some("Dieses VOD ist auf Twitch nicht mehr verfügbar."),
        _ => None,
    }
}

#[derive(Deserialize)]
pub struct ArchiveAction {
    pub id: i64,
    pub action: String,
    pub twitch_user_id: Option<String>,
}

pub async fn action_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<ArchiveAction>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, body.twitch_user_id.as_deref()).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    if body.id <= 0 || !matches!(body.action.as_str(), "retry" | "drive" | "hide" | "check") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Diese Aktion ist nicht verfügbar."})),
        )
            .into_response();
    }
    match apply_action(&pool, body.id, scope.as_deref(), &body.action).await {
        Ok(Some(true)) => Json(json!({"ok": true})).into_response(),
        Ok(Some(false)) => (
            StatusCode::CONFLICT,
            Json(json!({"error": "Dieses VOD wird gerade bearbeitet oder ist bereits fertig."})),
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(%error, "VOD-Aktion fehlgeschlagen");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Die Änderung konnte nicht gespeichert werden."})),
            )
                .into_response()
        }
    }
}

async fn apply_action(
    pool: &PgPool,
    id: i64,
    scope: Option<&str>,
    action: &str,
) -> Result<Option<bool>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query("SELECT status, local_path FROM twitch_vod_archive_vods WHERE id=$1 AND ($2::text IS NULL OR twitch_user_id=$2) FOR UPDATE")
        .bind(id).bind(scope).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(186976768, $1::int)")
        .bind(
            i32::try_from(id)
                .map_err(|_| sqlx::Error::Protocol("VOD-ID außerhalb des Bereichs".into()))?,
        )
        .fetch_one(&mut *tx)
        .await?;
    if !locked {
        return Ok(Some(false));
    }
    let status: String = row.get("status");
    if action == "check" {
        let queued = sqlx::query("INSERT INTO twitch_vod_youtube_checks (vod_id,auth_id,auth_revision,channel_id,requested_at,upload_snapshot) SELECT v.id,a.id,md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')),a.platform_user_id,NOW(),(SELECT COALESCE(jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index),'[]'::jsonb) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id) FROM twitch_vod_archive_vods v JOIN LATERAL (SELECT a.* FROM social_media_platform_auth a WHERE a.twitch_user_id=v.twitch_user_id AND a.platform='youtube' AND a.enabled=1 ORDER BY a.authorized_at DESC,a.id DESC LIMIT 1) a ON TRUE WHERE v.id=$1 AND (v.status IN ('uploaded','archived') OR EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND p.youtube_video_id IS NOT NULL)) ON CONFLICT (vod_id) DO UPDATE SET requested_at=COALESCE(twitch_vod_youtube_checks.requested_at,NOW()),next_check_at=NOW() WHERE twitch_vod_youtube_checks.requested_at IS NULL AND (twitch_vod_youtube_checks.last_attempt_at IS NULL OR twitch_vod_youtube_checks.last_attempt_at<NOW()-INTERVAL '10 minutes') AND (twitch_vod_youtube_checks.last_error IS DISTINCT FROM 'quota' OR twitch_vod_youtube_checks.next_check_at<=NOW())")
            .bind(id).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        return Ok(Some(queued == 1));
    }
    if action != "hide" && matches!(status.as_str(), "uploaded" | "archived" | "drive_uploaded") {
        return Ok(Some(false));
    }
    if action == "hide" {
        sqlx::query("UPDATE twitch_vod_archive_vods SET hidden_at=NOW() WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("UPDATE twitch_vod_archive_vods SET status='new', drive_requested=$2, last_error=NULL, updated_at=NOW() WHERE id=$1")
            .bind(id).bind(action == "drive").execute(&mut *tx).await?;
        sqlx::query("UPDATE twitch_vod_archive_parts SET status='pending', last_error=NULL, updated_at=NOW() WHERE vod_id=$1 AND status IN ('failed','rejected')")
            .bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Some(true))
}

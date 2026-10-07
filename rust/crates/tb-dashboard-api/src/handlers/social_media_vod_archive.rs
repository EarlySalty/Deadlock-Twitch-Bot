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
        "SELECT v.id, v.twitch_id, v.streamer_login, v.twitch_user_id, v.title, v.duration_sec, \
         v.recorded_at, v.discovered_at, v.status, v.last_error, v.drive_url, v.drive_requested, \
         v.last_attempt_at, v.updated_at, \
         COALESCE((SELECT jsonb_agg(jsonb_build_object('index', p.part_index, 'status', p.status, \
            'youtube_video_id', p.youtube_video_id) ORDER BY p.part_index) \
            FROM twitch_vod_archive_parts p WHERE p.vod_id = v.id), '[]'::jsonb) AS parts, \
         COALESCE(NOT a.needs_reauth AND a.access_token_enc IS NOT NULL \
            AND string_to_array(COALESCE(a.scopes, ''), ' ') && ARRAY['https://www.googleapis.com/auth/youtube.upload', 'https://www.googleapis.com/auth/youtube', 'https://www.googleapis.com/auth/youtube.force-ssl'], FALSE) AS youtube_connected, \
         CASE WHEN a.refresh_token_enc IS NOT NULL THEN a.refresh_expires_at::text ELSE a.token_expires_at END AS youtube_expires_at, \
         COUNT(*) OVER() AS total \
         FROM twitch_vod_archive_vods v \
         LEFT JOIN LATERAL (SELECT a.* FROM social_media_platform_auth a \
            WHERE a.twitch_user_id=v.twitch_user_id AND a.platform='youtube' AND a.enabled=1 \
            ORDER BY a.authorized_at DESC, a.id DESC LIMIT 1) a ON TRUE \
         WHERE v.hidden_at IS NULL AND ($1::text IS NULL OR v.twitch_user_id = $1) \
         ORDER BY v.discovered_at DESC, v.id DESC LIMIT 50 OFFSET $2",
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
            let items: Vec<Value> = rows.into_iter().map(|row| {
                let status: String = row.get("status");
                let parts: Value = row.get("parts");
                let expires: Option<String> = row.get("youtube_expires_at");
                let connected: bool = row.get::<bool, _>("youtube_connected") && expires.as_deref().is_none_or(|value| {
                    chrono::DateTime::parse_from_rfc3339(value).or_else(|_| chrono::DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f%#z"))
                        .is_ok_and(|expires| expires > chrono::Utc::now())
                });
                let label = status_label(&status, &parts);
                let last_error: Option<String> = row.get("last_error");
                let drive_requested: bool = row.get("drive_requested");
                json!({
                    "id": row.get::<i64, _>("id"),
                    "twitch_id": row.get::<String, _>("twitch_id"),
                    "channel": row.get::<String, _>("streamer_login"),
                    "twitch_user_id": row.get::<Option<String>, _>("twitch_user_id"),
                    "title": row.get::<String, _>("title"),
                    "duration_sec": row.get::<i64, _>("duration_sec"),
                    "recorded_at": row.get::<Option<chrono::NaiveDate>, _>("recorded_at"),
                    "discovered_at": row.get::<chrono::DateTime<chrono::Utc>, _>("discovered_at"),
                    "status": status, "status_label": label,
                    "reason": if label == "Fehlgeschlagen" && matches!(status.as_str(), "uploaded" | "archived") { Some("Für diese Sicherung fehlt ein bestätigter Upload. Bitte prüfe das Ziel, bevor du sie erneut startest.") } else { error_label(&status, last_error.as_deref(), drive_requested) },
                    "drive_url": row.get::<Option<String>, _>("drive_url"),
                    "drive_requested": row.get::<bool, _>("drive_requested"),
                    "last_attempt_at": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_attempt_at"),
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

fn status_label(status: &str, parts: &Value) -> &'static str {
    match status {
        "uploaded" | "archived"
            if parts.as_array().is_none_or(|parts| {
                parts.is_empty()
                    || parts
                        .iter()
                        .any(|part| part["status"] != "done" || part["youtube_video_id"].is_null())
            }) =>
        {
            "Fehlgeschlagen"
        }
        "uploaded" | "archived" => "Fertig",
        "uploading" => "Lädt hoch",
        "drive_uploaded" => "Ausweichweg Drive",
        "downloading" => "Lädt herunter",
        "download_failed" | "upload_failed" | "unavailable" => "Fehlgeschlagen",
        _ if parts
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| part["status"] == "uploading")) =>
        {
            "Lädt hoch"
        }
        _ => "Wartet",
    }
}

fn error_label(
    status: &str,
    last_error: Option<&str>,
    drive_requested: bool,
) -> Option<&'static str> {
    match status {
        "upload_failed" if drive_requested => Some("Die Sicherung auf Drive ist fehlgeschlagen. Du kannst sie erneut versuchen."),
        "upload_failed" if last_error.is_some_and(|error| error.contains("quotaExceeded")) => Some("YouTube nimmt heute keine weiteren Uploads an. Das Archiv versucht es bei einem späteren Lauf erneut."),
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
    if body.id <= 0 || !matches!(body.action.as_str(), "retry" | "drive" | "hide") {
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

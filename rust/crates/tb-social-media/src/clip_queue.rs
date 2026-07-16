//! Upload-Queue (Port der Queue-Methoden aus `clip_manager.py`).
//!
//! `twitch_clips_upload_queue` ist eine Lease-Queue je (Clip, Plattform):
//! `queue_upload` dedupliziert (pending wiederverwenden, stale processing
//! re-queuen), `get_upload_queue` reclaimt verwaiste processing-Jobs + lädt die
//! Queue mit Clip-Daten, `update_upload_status` verschiebt Status + pflegt bei
//! Erfolg die Clip-Upload-Spalten + den Publication-Status.

use chrono::{Duration, Utc};
use sqlx::{PgPool, Row};

use crate::retention::refresh_clip_publication_status;

/// Stale-Schwelle für processing-Jobs (Python: 30 min).
const PROCESSING_STALE_MINUTES: i64 = 30;
const PLATFORMS: [&str; 3] = ["tiktok", "youtube", "instagram"];

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Invalid platform: {0}")]
    InvalidPlatform(String),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

/// Ein Queue-Eintrag samt Clip-Daten (für den Upload-Worker).
#[derive(Debug, Clone)]
pub struct UploadQueueItem {
    pub id: i64,
    pub clip_db_id: i64,
    pub platform: String,
    pub status: String,
    pub priority: i32,
    pub title: Option<String>,
    pub description: Option<String>,
    pub hashtags: Option<String>,
    pub scheduled_at: Option<String>,
    pub attempts: i32,
    pub twitch_clip_id: Option<String>,
    pub clip_url: Option<String>,
    pub clip_title: Option<String>,
    pub streamer_login: Option<String>,
    pub local_file_path: Option<String>,
    pub converted_file_path: Option<String>,
}

fn hashtags_json(hashtags: Option<&[String]>) -> Option<String> {
    hashtags
        .filter(|h| !h.is_empty())
        .map(|h| serde_json::to_string(h).unwrap_or_else(|_| "[]".to_string()))
}

/// Fügt einen Upload zur Queue hinzu (oder gibt einen vorhandenen wieder).
#[allow(clippy::too_many_arguments)]
pub async fn queue_upload<C>(
    pool: &PgPool,
    clip_db_id: C,
    platform: &str,
    title: Option<&str>,
    description: Option<&str>,
    hashtags: Option<&[String]>,
    scheduled_at: Option<&str>,
    priority: i32,
) -> Result<i64, QueueError>
where
    C: Into<i64>,
{
    let clip_db_id = clip_db_id.into();
    if !PLATFORMS.contains(&platform) {
        return Err(QueueError::InvalidPlatform(platform.to_string()));
    }
    let tags = hashtags_json(hashtags);

    // 1) Pending wiederverwenden.
    if let Some(id) = sqlx::query_scalar!(
        "SELECT id AS \"id!\" FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND platform = $2 AND status = 'pending' \
         ORDER BY priority DESC, created_at ASC, id ASC LIMIT 1",
        clip_db_id,
        platform
    )
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }

    // 2) Processing: frisch → wiederverwenden; stale → re-queuen.
    let stale_cutoff = (Utc::now() - Duration::minutes(PROCESSING_STALE_MINUTES)).to_rfc3339();
    if let Some(row) = sqlx::query!(
        "SELECT id AS \"id!\", \
                (COALESCE(last_attempt_at, created_at) IS NOT NULL \
                 AND COALESCE(last_attempt_at, created_at) >= $3::text::timestamptz) AS \"is_fresh!\" \
         FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND platform = $2 AND status = 'processing' \
         ORDER BY COALESCE(last_attempt_at, created_at) DESC, id DESC LIMIT 1",
        clip_db_id,
        platform,
        stale_cutoff
    )
    .fetch_optional(pool)
    .await?
    {
        let id = row.id;
        if row.is_fresh {
            return Ok(id); // wird noch verarbeitet
        }
        sqlx::query!(
            "UPDATE twitch_clips_upload_queue SET status = 'pending', title = $1, description = $2, \
             hashtags = $3, scheduled_at = $4::text::timestamptz, priority = $5, last_error = NULL, \
             last_attempt_at = NULL, completed_at = NULL WHERE id = $6",
            title,
            description,
            tags.as_deref(),
            scheduled_at,
            priority,
            id
        )
        .execute(pool)
        .await?;
        return Ok(id);
    }

    // 3) Neu einfügen.
    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO twitch_clips_upload_queue \
            (clip_id, platform, title, description, hashtags, scheduled_at, priority, status, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6::text::timestamptz, $7, 'pending', $8::text::timestamptz) \
         RETURNING id AS \"id!\"",
        clip_db_id,
        platform,
        title,
        description,
        tags.as_deref(),
        scheduled_at,
        priority,
        Utc::now().to_rfc3339()
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Lädt die Upload-Queue (+ Clip-Daten). Optional vorab verwaiste processing-Jobs
/// (älter als `reclaim_stale_before`) auf pending zurücksetzen.
pub async fn get_upload_queue(
    pool: &PgPool,
    platform: Option<&str>,
    status: &str,
    limit: i64,
    reclaim_stale_before: Option<&str>,
) -> Vec<UploadQueueItem> {
    if status == "pending" {
        if let Some(cutoff) = reclaim_stale_before {
            let _ = sqlx::query!(
                "UPDATE twitch_clips_upload_queue SET status = 'pending', last_error = NULL \
                 WHERE status = 'processing' AND COALESCE(last_attempt_at, created_at) < $1::text::timestamptz",
                cutoff
            )
            .execute(pool)
            .await;
        }
    }

    let mut sql = String::from(
        "SELECT q.id, q.clip_id, q.platform, q.status, q.priority, q.title, q.description, \
                q.hashtags, q.scheduled_at, q.attempts, c.clip_id AS twitch_clip_id, c.clip_url, \
                c.clip_title, c.streamer_login, c.local_file_path, c.converted_file_path \
         FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id WHERE q.status = $1",
    );
    if platform.is_some() {
        sql.push_str(" AND q.platform = $2");
    }
    if status == "pending" {
        sql.push_str(" AND (q.scheduled_at IS NULL OR q.scheduled_at <= now())");
    }
    sql.push_str(" ORDER BY q.priority DESC, q.created_at ASC LIMIT ");
    sql.push_str(&limit.max(0).to_string());

    let mut query = sqlx::query(&sql).bind(status);
    if let Some(p) = platform {
        query = query.bind(p);
    }
    let rows = match query.fetch_all(pool).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(
                %error,
                status,
                platform = platform.unwrap_or(""),
                "Social-Media-Upload-Queue: Jobs nicht ladbar"
            );
            Vec::new()
        }
    };
    rows.iter()
        .map(|r| UploadQueueItem {
            id: r.try_get("id").unwrap_or(0),
            clip_db_id: r.try_get("clip_id").unwrap_or(0),
            platform: r.try_get("platform").unwrap_or_default(),
            status: r.try_get("status").unwrap_or_default(),
            priority: r.try_get("priority").unwrap_or(0),
            title: r.try_get("title").unwrap_or(None),
            description: r.try_get("description").unwrap_or(None),
            hashtags: r.try_get("hashtags").unwrap_or(None),
            scheduled_at: r.try_get("scheduled_at").unwrap_or(None),
            attempts: r.try_get("attempts").unwrap_or(0),
            twitch_clip_id: r.try_get("twitch_clip_id").unwrap_or(None),
            clip_url: r.try_get("clip_url").unwrap_or(None),
            clip_title: r.try_get("clip_title").unwrap_or(None),
            streamer_login: r.try_get("streamer_login").unwrap_or(None),
            local_file_path: r.try_get("local_file_path").unwrap_or(None),
            converted_file_path: r.try_get("converted_file_path").unwrap_or(None),
        })
        .collect()
}

/// Aktualisiert den Queue-Status. Bei `completed` werden zusätzlich die
/// Clip-Upload-Spalten gesetzt + der Publication-Status aufgefrischt.
pub async fn update_upload_status(
    pool: &PgPool,
    queue_id: i64,
    status: &str,
    external_video_id: Option<&str>,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    match status {
        "completed" => {
            let mut tx = pool.begin().await?;
            let queue_row = sqlx::query!(
                "SELECT clip_id AS \"clip_id!\", platform AS \"platform!\" \
                 FROM twitch_clips_upload_queue WHERE id = $1",
                queue_id
            )
            .fetch_optional(&mut *tx)
            .await?;
            sqlx::query!(
                "UPDATE twitch_clips_upload_queue SET status = 'completed', completed_at = $1::text::timestamptz WHERE id = $2",
                &now,
                queue_id
            )
                .execute(&mut *tx)
                .await?;
            let mut clip_for_refresh = None;
            if let Some(row) = queue_row {
                let clip_id = row.clip_id;
                let platform = row.platform;
                let clip_sql = match platform.as_str() {
                    "tiktok" => "UPDATE twitch_clips_social_media SET uploaded_tiktok = TRUE, tiktok_video_id = $1, tiktok_uploaded_at = $2::text::timestamptz WHERE id = $3",
                    "youtube" => "UPDATE twitch_clips_social_media SET uploaded_youtube = TRUE, youtube_video_id = $1, youtube_uploaded_at = $2::text::timestamptz WHERE id = $3",
                    "instagram" => "UPDATE twitch_clips_social_media SET uploaded_instagram = TRUE, instagram_media_id = $1, instagram_uploaded_at = $2::text::timestamptz WHERE id = $3",
                    _ => {
                        tx.commit().await?;
                        return Ok(());
                    }
                };
                sqlx::query(clip_sql)
                    .bind(external_video_id)
                    .bind(&now)
                    .bind(clip_id)
                    .execute(&mut *tx)
                    .await?;
                clip_for_refresh = Some(clip_id);
            }
            tx.commit().await?;
            if let Some(clip_id) = clip_for_refresh {
                refresh_clip_publication_status(pool, clip_id).await;
            }
        }
        "failed" => {
            sqlx::query!(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', attempts = attempts + 1, \
                 last_error = $1, last_attempt_at = $2::text::timestamptz WHERE id = $3",
                error,
                &now,
                queue_id
            )
            .execute(pool)
            .await?;
        }
        other => {
            sqlx::query!(
                "UPDATE twitch_clips_upload_queue SET status = $1, last_attempt_at = $2::text::timestamptz WHERE id = $3",
                other,
                &now,
                queue_id
            )
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
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
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, clip_title TEXT, streamer_login TEXT NOT NULL, local_file_path TEXT, converted_file_path TEXT, status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT, tiktok_uploaded_at TIMESTAMPTZ, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        Some(pool)
    }

    async fn seed_clip(pool: &PgPool) -> i64 {
        sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, clip_title) VALUES ('c1', 'https://clips.test/c1', 'nani', 'T') RETURNING id").fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn queue_dedup_und_invalid_platform() {
        let Some(pool) = make_pool("t_sm_queue").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        assert!(matches!(
            queue_upload(&pool, clip, "twitter", None, None, None, None, 0).await,
            Err(QueueError::InvalidPlatform(_))
        ));

        let tags = vec!["#deadlock".to_string()];
        let id1 = queue_upload(&pool, clip, "tiktok", Some("T"), None, Some(&tags), None, 5)
            .await
            .unwrap();
        // Zweiter Aufruf für denselben pending → gleiche ID.
        let id2 = queue_upload(&pool, clip, "tiktok", None, None, None, None, 0)
            .await
            .unwrap();
        assert_eq!(id1, id2);
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        // hashtags als JSON gespeichert.
        let tags_raw: Option<String> =
            sqlx::query_scalar("SELECT hashtags FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(id1)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tags_raw.as_deref(), Some("[\"#deadlock\"]"));
    }

    #[tokio::test]
    async fn get_queue_und_update_completed() {
        let Some(pool) = make_pool("t_sm_queue_flow").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        // Aktive Plattform tiktok für 'nani' → completed soll published_all geben.
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok','nani')").execute(&pool).await.unwrap();
        let qid = queue_upload(&pool, clip, "tiktok", None, None, None, None, 0)
            .await
            .unwrap();

        let items = get_upload_queue(&pool, Some("tiktok"), "pending", 10, None).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, qid);
        assert_eq!(items[0].clip_db_id, clip);
        assert_eq!(items[0].twitch_clip_id.as_deref(), Some("c1"));
        assert_eq!(items[0].streamer_login.as_deref(), Some("nani"));

        // completed → Queue completed, Clip uploaded_tiktok=true, status=published_all.
        update_upload_status(&pool, qid, "completed", Some("vid123"), None)
            .await
            .unwrap();
        let qstatus: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(qid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(qstatus, "completed");
        let (up, vidid, cstatus): (bool, Option<String>, String) = sqlx::query_as("SELECT uploaded_tiktok, tiktok_video_id, status FROM twitch_clips_social_media WHERE id = $1").bind(clip).fetch_one(&pool).await.unwrap();
        assert!(up);
        assert_eq!(vidid.as_deref(), Some("vid123"));
        assert_eq!(cstatus, "published_all"); // einzige aktive Plattform hochgeladen

        // Pending-Queue jetzt leer.
        assert!(get_upload_queue(&pool, None, "pending", 10, None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn get_queue_skips_future_scheduled_jobs() {
        let Some(pool) = make_pool("t_sm_queue_schedule_gate").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        queue_upload(
            &pool,
            clip,
            "tiktok",
            None,
            None,
            None,
            Some("2999-01-01T00:00:00Z"),
            0,
        )
        .await
        .unwrap();

        assert!(get_upload_queue(&pool, None, "pending", 10, None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn update_failed_zaehlt_attempts() {
        let Some(pool) = make_pool("t_sm_queue_fail").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        let qid = queue_upload(&pool, clip, "youtube", None, None, None, None, 0)
            .await
            .unwrap();
        update_upload_status(&pool, qid, "failed", None, Some("boom"))
            .await
            .unwrap();
        let (status, attempts, err): (String, i32, Option<String>) = sqlx::query_as(
            "SELECT status, attempts, last_error FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(qid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(attempts, 1);
        assert_eq!(err.as_deref(), Some("boom"));
    }
}

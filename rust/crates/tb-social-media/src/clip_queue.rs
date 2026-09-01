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
    /// Vertagungen wegen vollem Tageskontingent. Eigenes Konto neben
    /// `attempts`, damit ein volles Kontingent die Versuchsgrenze nicht
    /// aufzehrt und sich trotzdem nicht endlos vertagen kann.
    pub quota_deferrals: i32,
    pub twitch_clip_id: Option<String>,
    pub clip_url: Option<String>,
    pub clip_title: Option<String>,
    pub streamer_login: Option<String>,
    pub local_file_path: Option<String>,
    pub converted_file_path: Option<String>,
}

/// Auf welches Konto eine Vertagung geht.
///
/// Die beiden Zaehler sind bewusst getrennt: ein voller Tag beim Anbieter ist
/// Alltag, ein kaputter Upload nicht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertagungsKonto {
    /// Echter Fehlversuch, zaehlt gegen die normale Versuchsgrenze.
    Versuch,
    /// Volles Tageskontingent, zaehlt gegen die eigene Kontingent-Grenze.
    Kontingent,
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
    let mut transaction = pool.begin().await?;
    let queue_lock = format!("tb-social-media-queue:{clip_db_id}:{platform}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(queue_lock)
        .execute(&mut *transaction)
        .await?;

    // 1) Pending wiederverwenden.
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND platform = $2 AND status = 'pending' \
           AND provider_started_at IS NULL \
         ORDER BY priority DESC, created_at ASC, id ASC LIMIT 1",
    )
    .bind(clip_db_id)
    .bind(platform)
    .fetch_optional(&mut *transaction)
    .await?
    {
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET title = COALESCE($1, title), \
             description = COALESCE($2, description), hashtags = COALESCE($3, hashtags), \
             scheduled_at = COALESCE($4::text::timestamptz, scheduled_at), \
             priority = GREATEST(twitch_clips_upload_queue.priority, $5), last_error = NULL \
             WHERE id = $6",
        )
        .bind(title)
        .bind(description)
        .bind(tags.as_deref())
        .bind(scheduled_at)
        .bind(priority)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(id);
    }

    // 2) Processing: frisch → wiederverwenden; stale → re-queuen.
    let stale_cutoff = (Utc::now() - Duration::minutes(PROCESSING_STALE_MINUTES)).to_rfc3339();
    if let Some((id, is_fresh, provider_started)) = sqlx::query_as::<_, (i64, bool, bool)>(
        "SELECT id, \
                (COALESCE(last_attempt_at, created_at) IS NOT NULL \
                 AND COALESCE(last_attempt_at, created_at) >= $3::text::timestamptz) AS is_fresh, \
                provider_started_at IS NOT NULL AS provider_started \
         FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND platform = $2 \
           AND status IN ('processing', 'reconciliation_required') \
         ORDER BY provider_started_at DESC NULLS LAST, \
                  COALESCE(last_attempt_at, created_at) DESC, id DESC LIMIT 1",
    )
    .bind(clip_db_id)
    .bind(platform)
    .bind(&stale_cutoff)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if provider_started || is_fresh {
            transaction.commit().await?;
            return Ok(id); // wird noch verarbeitet
        }
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = 'pending', title = $1, description = $2, \
             hashtags = $3, scheduled_at = $4::text::timestamptz, priority = $5, last_error = NULL, \
             last_attempt_at = NULL, completed_at = NULL \
             WHERE id = $6 AND provider_started_at IS NULL",
        )
        .bind(title)
        .bind(description)
        .bind(tags.as_deref())
        .bind(scheduled_at)
        .bind(priority)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
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
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
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
) -> Result<Vec<UploadQueueItem>, sqlx::Error> {
    if status == "pending" {
        return claim_pending_uploads(pool, platform, limit, reclaim_stale_before).await;
    }

    let mut sql = String::from(
        "SELECT q.id, q.clip_id, q.platform, q.status, q.priority, q.title, q.description, \
                q.hashtags, q.scheduled_at, q.attempts, q.quota_deferrals, \
                c.clip_id AS twitch_clip_id, c.clip_url, \
                c.clip_title, c.streamer_login, c.local_file_path, c.converted_file_path \
         FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id \
         LEFT JOIN social_media_streamer_settings s \
           ON LOWER(s.streamer_login) = LOWER(c.streamer_login) \
         WHERE q.status = $1 AND c.discarded_at IS NULL",
    );
    if platform.is_some() {
        sql.push_str(" AND q.platform = $2");
    }
    if status == "pending" {
        sql.push_str(
            " AND (q.scheduled_at IS NULL OR q.scheduled_at <= now()) \
              AND COALESCE(s.release_mode, 'prepare_only') = 'live'",
        );
    }
    sql.push_str(" ORDER BY q.priority DESC, q.created_at ASC LIMIT ");
    sql.push_str(&limit.max(0).to_string());

    let mut query = sqlx::query(&sql).bind(status);
    if let Some(p) = platform {
        query = query.bind(p);
    }
    let rows = query.fetch_all(pool).await?;
    rows.iter().map(row_to_item).collect()
}

/// Atomarer Lease-Claim für fällige Jobs. Ein bloßes SELECT ließ zwei
/// überlappende Worker denselben Clip sehen; `SKIP LOCKED` plus Statuswechsel in
/// derselben Transaktion gibt jeden Queue-Eintrag höchstens einmal aus.
async fn claim_pending_uploads(
    pool: &PgPool,
    platform: Option<&str>,
    limit: i64,
    reclaim_stale_before: Option<&str>,
) -> Result<Vec<UploadQueueItem>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if let Some(cutoff) = reclaim_stale_before {
        sqlx::query(
            "UPDATE twitch_clips_upload_queue \
             SET status = 'reconciliation_required', \
                 last_error = 'provider_result_unknown_after_restart' \
             WHERE status = 'processing' AND provider_started_at IS NOT NULL \
               AND COALESCE(last_attempt_at, provider_started_at) < $1::text::timestamptz",
        )
        .bind(cutoff)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = 'pending', last_error = NULL \
             WHERE status = 'processing' AND provider_started_at IS NULL \
               AND COALESCE(last_attempt_at, created_at) < $1::text::timestamptz",
        )
        .bind(cutoff)
        .execute(&mut *tx)
        .await?;
    }

    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT q.id FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id \
         LEFT JOIN social_media_streamer_settings s \
           ON LOWER(s.streamer_login) = LOWER(c.streamer_login) \
         WHERE q.status = 'pending' AND q.provider_started_at IS NULL \
           AND c.discarded_at IS NULL \
           AND q.platform <> 'tiktok' \
           AND ($1::text IS NULL OR q.platform = $1) \
           AND (q.scheduled_at IS NULL OR q.scheduled_at <= NOW()) \
           AND COALESCE(s.release_mode, 'prepare_only') = 'live' \
           AND COALESCE((SELECT a.provider_calls_enabled \
                 FROM social_media_platform_auth a \
                WHERE a.platform = q.platform AND a.enabled = 1 \
                  AND (LOWER(a.streamer_login) = LOWER(c.streamer_login) \
                       OR a.streamer_login IS NULL) \
                ORDER BY CASE WHEN LOWER(a.streamer_login) = LOWER(c.streamer_login) \
                              THEN 1 ELSE 0 END DESC, a.id DESC LIMIT 1), FALSE) \
           AND COALESCE((SELECT p.auto_post AND p.posts_per_week > 0 \
                                AND p.max_posts_per_day > 0 \
                 FROM social_media_platform_schedule p \
                WHERE LOWER(p.streamer_login) = LOWER(c.streamer_login) \
                  AND p.platform = q.platform), FALSE) \
         ORDER BY q.priority DESC, q.created_at ASC, q.id ASC \
         LIMIT $2 FOR UPDATE OF q SKIP LOCKED",
    )
    .bind(platform)
    .bind(limit.max(0))
    .fetch_all(&mut *tx)
    .await?;
    if ids.is_empty() {
        tx.commit().await?;
        return Ok(Vec::new());
    }

    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = 'processing', \
         last_attempt_at = CURRENT_TIMESTAMP, last_error = NULL \
         WHERE id = ANY($1::bigint[]) AND provider_started_at IS NULL",
    )
    .bind(&ids)
    .execute(&mut *tx)
    .await?;

    let rows = sqlx::query(
        "SELECT q.id, q.clip_id, q.platform, q.status, q.priority, q.title, q.description, \
                q.hashtags, q.scheduled_at, q.attempts, q.quota_deferrals, \
                c.clip_id AS twitch_clip_id, c.clip_url, c.clip_title, c.streamer_login, \
                c.local_file_path, c.converted_file_path \
           FROM twitch_clips_upload_queue q \
           JOIN twitch_clips_social_media c ON c.id = q.clip_id \
          WHERE q.id = ANY($1::bigint[]) \
          ORDER BY q.priority DESC, q.created_at ASC, q.id ASC",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    let items = rows
        .iter()
        .map(row_to_item)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await?;
    Ok(items)
}

fn row_to_item(row: &sqlx::postgres::PgRow) -> Result<UploadQueueItem, sqlx::Error> {
    Ok(UploadQueueItem {
        id: row.try_get("id")?,
        clip_db_id: row.try_get("clip_id")?,
        platform: row.try_get("platform")?,
        status: row.try_get("status")?,
        priority: row.try_get("priority")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        hashtags: row.try_get("hashtags")?,
        scheduled_at: row.try_get("scheduled_at")?,
        attempts: row.try_get("attempts")?,
        quota_deferrals: row.try_get("quota_deferrals")?,
        twitch_clip_id: row.try_get("twitch_clip_id")?,
        clip_url: row.try_get("clip_url")?,
        clip_title: row.try_get("clip_title")?,
        streamer_login: row.try_get("streamer_login")?,
        local_file_path: row.try_get("local_file_path")?,
        converted_file_path: row.try_get("converted_file_path")?,
    })
}

/// Aktualisiert den Queue-Status. Bei `completed` werden zusätzlich die
/// Clip-Upload-Spalten gesetzt + der Publication-Status aufgefrischt.
/// Legt einen Job zurueck in die Warteschlange, statt ihn endgueltig zu
/// verbrennen. Ein erschoepftes Tageskontingent und ein Netzwerkzucken sind
/// keine kaputten Clips: frueher landeten beide auf `failed`, und `failed` wird
/// nie wieder abgeholt. `last_error` bleibt sichtbar.
///
/// `konto` entscheidet, welcher Zaehler hochlaeuft. Bei einem vollen
/// Tageskontingent ist weder der Clip noch der Zugang kaputt, es geht
/// ausschliesslich um den Termin; zaehlte das gegen `attempts`, waere die
/// Versuchsgrenze nach vier Kontingent-Vertagungen aufgebraucht und der erste
/// harmlose Netzfehler danach wuerde den Clip verwerfen. Umgekehrt darf sich
/// eine Kontingent-Vertagung nicht endlos wiederholen, deshalb bekommt sie mit
/// `quota_deferrals` ein eigenes Konto mit eigener, hoeherer Grenze.
pub async fn reschedule_upload(
    pool: &PgPool,
    queue_id: i64,
    naechster_versuch: &str,
    error: Option<&str>,
    konto: VertagungsKonto,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE twitch_clips_upload_queue \
            SET status = 'pending', \
                attempts = attempts + CASE WHEN $5 THEN 1 ELSE 0 END, \
                quota_deferrals = quota_deferrals + CASE WHEN $6 THEN 1 ELSE 0 END, \
                last_error = $1, \
                last_attempt_at = $2::text::timestamptz, \
                scheduled_at = $3::text::timestamptz \
          WHERE id = $4 AND provider_started_at IS NULL",
    )
    .bind(error)
    .bind(&now)
    .bind(naechster_versuch)
    .bind(queue_id)
    .bind(konto == VertagungsKonto::Versuch)
    .bind(konto == VertagungsKonto::Kontingent)
    .execute(pool)
    .await?;
    Ok(())
}

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
            let hint = sqlx::query_as::<_, (i64, String)>(
                "SELECT clip_id, platform FROM twitch_clips_upload_queue WHERE id = $1",
            )
            .bind(queue_id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some((clip_id, platform)) = hint else {
                tx.commit().await?;
                return Ok(());
            };
            sqlx::query(
                "SELECT clip_db_id FROM social_media_clip_preparation \
                 WHERE clip_db_id = $1 FOR UPDATE",
            )
            .bind(clip_id)
            .fetch_all(&mut *tx)
            .await?;
            sqlx::query("SELECT id FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE")
                .bind(clip_id)
                .fetch_optional(&mut *tx)
                .await?;
            let queue_row: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM twitch_clips_upload_queue \
                 WHERE id = $1 AND clip_id = $2 AND provider_started_at IS NULL FOR UPDATE",
            )
            .bind(queue_id)
            .bind(clip_id)
            .fetch_optional(&mut *tx)
            .await?;
            if queue_row.is_none() {
                tx.commit().await?;
                return Ok(());
            }
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'completed', \
                 completed_at = $1::text::timestamptz \
                 WHERE id = $2 AND provider_started_at IS NULL",
            )
            .bind(&now)
            .bind(queue_id)
            .execute(&mut *tx)
            .await?;
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
            tx.commit().await?;
            if let Err(error) = refresh_clip_publication_status(pool, clip_id).await {
                tracing::error!(
                    clip_db_id = clip_id,
                    code = "publication_status_refresh_failed",
                    database_code = ?error
                        .as_database_error()
                        .and_then(|database| database.code()),
                    "Clip-Publikationsstatus konnte nach Queue-Abschluss nicht aktualisiert werden"
                );
            }
        }
        "failed" => {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', attempts = attempts + 1, \
                 last_error = $1, last_attempt_at = $2::text::timestamptz \
                 WHERE id = $3 AND provider_started_at IS NULL",
            )
            .bind(error)
            .bind(&now)
            .bind(queue_id)
            .execute(pool)
            .await?;
        }
        other => {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = $1, \
                 last_attempt_at = $2::text::timestamptz \
                 WHERE id = $3 AND provider_started_at IS NULL",
            )
            .bind(other)
            .bind(&now)
            .bind(queue_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// Verbucht einen Provider-Erfolg ausschließlich für genau den gestarteten
/// Provider-Versuch. Ein alter Worker darf nach Crash/Reclaim weder eine neue
/// Lease finalisieren noch Clip-Flags setzen.
pub async fn complete_provider_upload(
    pool: &PgPool,
    queue_id: i64,
    provider_lease_token: &str,
    external_video_id: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let hint = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT q.clip_id, q.platform, c.streamer_login \
         FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id \
         WHERE q.id = $1",
    )
    .bind(queue_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((clip_id, platform, streamer_login)) = hint else {
        tx.commit().await?;
        return Ok(false);
    };
    crate::posting_plan::acquire_release_lock(tx.as_mut(), &streamer_login).await?;
    sqlx::query(
        "SELECT clip_db_id FROM social_media_clip_preparation \
         WHERE clip_db_id = $1 FOR UPDATE",
    )
    .bind(clip_id)
    .fetch_all(&mut *tx)
    .await?;
    let clip_locked: Option<i64> =
        sqlx::query_scalar("SELECT id FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE")
            .bind(clip_id)
            .fetch_optional(&mut *tx)
            .await?;
    if clip_locked.is_none() {
        tx.commit().await?;
        return Ok(false);
    }
    let queue_row: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM twitch_clips_upload_queue \
         WHERE id = $1 AND clip_id = $2 AND status = 'processing' \
           AND provider_started_at IS NOT NULL AND provider_lease_token = $3 \
         FOR UPDATE",
    )
    .bind(queue_id)
    .bind(clip_id)
    .bind(provider_lease_token)
    .fetch_optional(&mut *tx)
    .await?;
    if queue_row.is_none() {
        tx.commit().await?;
        return Ok(false);
    }
    // Erst nach der gemeinsamen Release-Sperre stempeln. Ein Abschluss, der
    // beim Abschalten bereits auf diese Sperre wartete, muss für die ältere
    // Settings-Transaktion als neues Provider-Ergebnis sichtbar sein.
    let now = Utc::now().to_rfc3339();

    let updated = sqlx::query(
        "UPDATE twitch_clips_upload_queue \
         SET status = 'completed', completed_at = $3::text::timestamptz, last_error = NULL, \
             provider_external_id = COALESCE($4, provider_external_id), \
             provider_accepted_at = COALESCE(provider_accepted_at, $3::text::timestamptz) \
         WHERE id = $1 AND provider_lease_token = $2 \
           AND provider_started_at IS NOT NULL AND status = 'processing'",
    )
    .bind(queue_id)
    .bind(provider_lease_token)
    .bind(&now)
    .bind(external_video_id)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        tx.commit().await?;
        return Ok(false);
    }

    let clip_sql = match platform.as_str() {
        "tiktok" => "UPDATE twitch_clips_social_media SET uploaded_tiktok = TRUE, tiktok_video_id = $1, tiktok_uploaded_at = $2::text::timestamptz WHERE id = $3",
        "youtube" => "UPDATE twitch_clips_social_media SET uploaded_youtube = TRUE, youtube_video_id = $1, youtube_uploaded_at = $2::text::timestamptz WHERE id = $3",
        "instagram" => "UPDATE twitch_clips_social_media SET uploaded_instagram = TRUE, instagram_media_id = $1, instagram_uploaded_at = $2::text::timestamptz WHERE id = $3",
        _ => {
            tx.rollback().await?;
            return Ok(false);
        }
    };
    sqlx::query(clip_sql)
        .bind(external_video_id)
        .bind(&now)
        .bind(clip_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if let Err(error) = refresh_clip_publication_status(pool, clip_id).await {
        tracing::error!(
            clip_db_id = clip_id,
            code = "publication_status_refresh_failed",
            database_code = ?error
                .as_database_error()
                .and_then(|database| database.code()),
            "Clip-Publikationsstatus konnte nach Provider-Abschluss nicht aktualisiert werden"
        );
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        // Gemeinsame Notbremse statt einer Kopie je Testmodul: ohne DSN
        // meldet ein uebersprungener DB-Test sonst gruen, ohne etwas
        // geprueft zu haben.
        let dsn = crate::test_support::test_dsn()?;
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
        sqlx::query("CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1, provider_calls_enabled BOOLEAN NOT NULL DEFAULT TRUE)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, clip_title TEXT, streamer_login TEXT NOT NULL, local_file_path TEXT, converted_file_path TEXT, status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT, tiktok_uploaded_at TIMESTAMPTZ, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, release_mode TEXT NOT NULL DEFAULT 'live')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_streamer_settings (streamer_login) VALUES ('nani')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE social_media_platform_schedule (streamer_login TEXT NOT NULL, platform TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT TRUE, posts_per_week INTEGER NOT NULL DEFAULT 4, max_posts_per_day INTEGER NOT NULL DEFAULT 1, PRIMARY KEY (streamer_login, platform))").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_platform_schedule (streamer_login, platform) VALUES ('nani','tiktok'),('nani','youtube'),('nani','instagram')").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
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

        let old_tags = vec!["#old".to_string()];
        let id1 = queue_upload(
            &pool,
            clip,
            "tiktok",
            Some("Alt"),
            Some("Altbeschreibung"),
            Some(&old_tags),
            Some("2030-01-01T00:00:00Z"),
            5,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE twitch_clips_upload_queue SET last_error = 'alt' WHERE id = $1")
            .bind(id1)
            .execute(&pool)
            .await
            .unwrap();
        // Zweiter Aufruf für denselben pending → gleiche ID mit aktualisierten Werten.
        let new_tags = vec!["#deadlock".to_string()];
        let id2 = queue_upload(
            &pool,
            clip,
            "tiktok",
            Some("Neu"),
            Some("Neubeschreibung"),
            Some(&new_tags),
            Some("2031-02-03T04:05:06Z"),
            9,
        )
        .await
        .unwrap();
        assert_eq!(id1, id2);
        let id3 = queue_upload(&pool, clip, "tiktok", None, None, None, None, 0)
            .await
            .unwrap();
        assert_eq!(id1, id3);
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        let row = sqlx::query(
            "SELECT title, description, hashtags, \
             scheduled_at = '2031-02-03T04:05:06Z'::timestamptz AS scheduled_matches, \
             priority, last_error \
             FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(id1)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.try_get::<Option<String>, _>("title")
                .unwrap()
                .as_deref(),
            Some("Neu")
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("description")
                .unwrap()
                .as_deref(),
            Some("Neubeschreibung")
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("hashtags")
                .unwrap()
                .as_deref(),
            Some("[\"#deadlock\"]")
        );
        assert_eq!(
            row.try_get::<Option<bool>, _>("scheduled_matches").unwrap(),
            Some(true)
        );
        assert_eq!(row.try_get::<i32, _>("priority").unwrap(), 9);
        assert_eq!(
            row.try_get::<Option<String>, _>("last_error").unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn get_queue_und_update_completed() {
        let Some(pool) = make_pool("t_sm_queue_flow").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        // Aktive Plattform YouTube für 'nani' → completed soll published_all geben.
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('youtube','nani')").execute(&pool).await.unwrap();
        let qid = queue_upload(&pool, clip, "youtube", None, None, None, None, 0)
            .await
            .unwrap();

        let items = get_upload_queue(&pool, Some("youtube"), "pending", 10, None)
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, qid);
        assert_eq!(items[0].clip_db_id, clip);
        assert_eq!(items[0].twitch_clip_id.as_deref(), Some("c1"));
        assert_eq!(items[0].streamer_login.as_deref(), Some("nani"));

        // completed → Queue completed, Clip uploaded_youtube=true, status=published_all.
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
        let (up, vidid, cstatus): (bool, Option<String>, String) = sqlx::query_as("SELECT uploaded_youtube, youtube_video_id, status FROM twitch_clips_social_media WHERE id = $1").bind(clip).fetch_one(&pool).await.unwrap();
        assert!(up);
        assert_eq!(vidid.as_deref(), Some("vid123"));
        assert_eq!(cstatus, "published_all"); // einzige aktive Plattform hochgeladen

        // Pending-Queue jetzt leer.
        assert!(get_upload_queue(&pool, None, "pending", 10, None)
            .await
            .unwrap()
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
            "youtube",
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
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn pending_queue_filtert_prepare_only_und_verworfene_clips() {
        let Some(pool) = make_pool("t_sm_queue_release_gate").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        queue_upload(&pool, clip, "youtube", None, None, None, None, 0)
            .await
            .unwrap();

        sqlx::query("UPDATE social_media_streamer_settings SET release_mode = 'prepare_only'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(get_upload_queue(&pool, None, "pending", 10, None)
            .await
            .unwrap()
            .is_empty());

        sqlx::query("UPDATE social_media_streamer_settings SET release_mode = 'live'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE twitch_clips_social_media SET discarded_at = NOW() WHERE id = $1")
            .bind(clip)
            .execute(&pool)
            .await
            .unwrap();
        assert!(get_upload_queue(&pool, None, "pending", 10, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn parallele_worker_claimen_einen_job_hoechstens_einmal() {
        let Some(pool) = make_pool("t_sm_queue_atomic_claim").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        queue_upload(&pool, clip, "youtube", None, None, None, None, 0)
            .await
            .unwrap();

        let (left, right) = tokio::join!(
            get_upload_queue(&pool, None, "pending", 1, None),
            get_upload_queue(&pool, None, "pending", 1, None),
        );
        let left = left.unwrap();
        let right = right.unwrap();
        assert_eq!(left.len() + right.len(), 1);
        let status: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "processing");
    }

    #[tokio::test]
    async fn tiktok_wird_ohne_per_clip_consent_nicht_geclaimt() {
        let Some(pool) = make_pool("t_sm_queue_tiktok_consent_gate").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        sqlx::query(
            "INSERT INTO social_media_platform_auth \
             (platform, streamer_login, provider_calls_enabled) \
             VALUES ('tiktok', 'nani', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let queue_id = queue_upload(&pool, clip, "tiktok", None, None, None, None, 0)
            .await
            .unwrap();

        assert!(get_upload_queue(&pool, Some("tiktok"), "pending", 10, None)
            .await
            .unwrap()
            .is_empty());
        let status: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(queue_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            status, "pending",
            "das Consent-Gate darf keinen Claim erzeugen"
        );
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

    #[tokio::test]
    async fn gestartete_provider_lease_wird_nie_requeued() {
        let Some(pool) = make_pool("t_sm_queue_provider_crash").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        let queue_id = queue_upload(&pool, clip, "tiktok", None, None, None, None, 0)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_clips_upload_queue \
             SET status = 'processing', provider_started_at = NOW() - INTERVAL '2 hours', \
                 provider_lease_token = 'lease-a', last_attempt_at = NOW() - INTERVAL '2 hours' \
             WHERE id = $1",
        )
        .bind(queue_id)
        .execute(&pool)
        .await
        .unwrap();

        let cutoff = (Utc::now() - Duration::minutes(30)).to_rfc3339();
        assert!(get_upload_queue(&pool, None, "pending", 10, Some(&cutoff))
            .await
            .unwrap()
            .is_empty());
        let (status, error): (String, Option<String>) = sqlx::query_as(
            "SELECT status, last_error FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "reconciliation_required");
        assert_eq!(
            error.as_deref(),
            Some("provider_result_unknown_after_restart")
        );

        reschedule_upload(
            &pool,
            queue_id,
            "2099-01-01T00:00:00Z",
            Some("retry"),
            VertagungsKonto::Versuch,
        )
        .await
        .unwrap();
        update_upload_status(&pool, queue_id, "failed", None, Some("foreign"))
            .await
            .unwrap();
        let (status, attempts): (String, i32) =
            sqlx::query_as("SELECT status, attempts FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(queue_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "reconciliation_required");
        assert_eq!(attempts, 0);
    }

    #[tokio::test]
    async fn doppelqueue_gibt_gestarteten_provider_versuch_zurueck() {
        let Some(pool) = make_pool("t_sm_queue_provider_dedup").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        let queue_id = queue_upload(&pool, clip, "youtube", None, None, None, None, 0)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = 'reconciliation_required', \
             provider_started_at = NOW(), provider_lease_token = 'lease-a' WHERE id = $1",
        )
        .bind(queue_id)
        .execute(&pool)
        .await
        .unwrap();

        let duplicate = queue_upload(
            &pool,
            clip,
            "youtube",
            Some("neuer Titel"),
            None,
            None,
            None,
            0,
        )
        .await
        .unwrap();
        assert_eq!(duplicate, queue_id);
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_upload_queue \
             WHERE clip_id = $1 AND platform = 'youtube'",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn provider_finalize_ist_an_lease_token_gebunden() {
        let Some(pool) = make_pool("t_sm_queue_provider_finalize").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        let queue_id = queue_upload(&pool, clip, "tiktok", None, None, None, None, 0)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = 'processing', \
             provider_started_at = NOW(), provider_lease_token = 'lease-new', \
             provider_external_id = 'provider-42' WHERE id = $1",
        )
        .bind(queue_id)
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            !complete_provider_upload(&pool, queue_id, "lease-old", Some("wrong"))
                .await
                .unwrap()
        );
        assert!(
            complete_provider_upload(&pool, queue_id, "lease-new", Some("provider-42"))
                .await
                .unwrap()
        );
        let (status, uploaded, video_id): (String, bool, Option<String>) = sqlx::query_as(
            "SELECT q.status, c.uploaded_tiktok, c.tiktok_video_id \
             FROM twitch_clips_upload_queue q \
             JOIN twitch_clips_social_media c ON c.id = q.clip_id WHERE q.id = $1",
        )
        .bind(queue_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "completed");
        assert!(uploaded);
        assert_eq!(video_id.as_deref(), Some("provider-42"));
    }

    #[tokio::test]
    async fn queue_db_und_row_fehler_werden_nie_als_leere_queue_getarnt() {
        let Some(pool) = make_pool("t_sm_queue_fail_closed_errors").await else {
            return;
        };
        let malformed = sqlx::query(
            "SELECT 'keine-id'::text AS id, 1::bigint AS clip_id, \
                    'youtube'::text AS platform, 'pending'::text AS status, \
                    0::integer AS priority, NULL::text AS title, \
                    NULL::text AS description, NULL::text AS hashtags, \
                    NULL::text AS scheduled_at, 0::integer AS attempts, \
                    0::integer AS quota_deferrals, NULL::text AS twitch_clip_id, \
                    NULL::text AS clip_url, NULL::text AS clip_title, \
                    NULL::text AS streamer_login, NULL::text AS local_file_path, \
                    NULL::text AS converted_file_path",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row_to_item(&malformed).is_err());

        sqlx::query("DROP TABLE twitch_clips_upload_queue")
            .execute(&pool)
            .await
            .unwrap();
        assert!(get_upload_queue(&pool, None, "pending", 10, None)
            .await
            .is_err());
    }
}

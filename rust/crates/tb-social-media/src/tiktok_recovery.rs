use async_trait::async_trait;
use sqlx::PgPool;

use crate::uploaders::{UploadCheckpoint, UploadError};

pub(crate) async fn reserve(pool: &PgPool, queue_id: i64) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let clip: Option<(i64, bool)> = sqlx::query_as(
        "SELECT c.id, COALESCE(c.uploaded_tiktok, FALSE) \
         FROM twitch_clips_social_media c \
         JOIN twitch_clips_upload_queue q ON q.clip_id = c.id \
         WHERE q.id = $1 AND q.platform = 'tiktok' FOR UPDATE OF c",
    )
    .bind(queue_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((clip_id, false)) = clip else {
        return Ok(false);
    };
    let active: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND platform = 'tiktok' AND status IN ('inbox', 'inbox_pending') \
         ORDER BY id LIMIT 1",
    )
    .bind(clip_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(active) = active {
        if active != queue_id {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', \
                 last_error = 'Für diesen Clip läuft bereits eine TikTok-Übertragung.' \
                 WHERE id = $1 AND status IN ('pending', 'processing')",
            )
            .bind(queue_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        return Ok(false);
    }
    let changed = sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = 'inbox_pending', \
         tiktok_publish_id = NULL, last_attempt_at = NOW(), last_error = NULL \
         WHERE id = $1 AND status IN ('pending', 'processing')",
    )
    .bind(queue_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(changed == 1)
}

pub(crate) struct Checkpoint<'a> {
    pub pool: &'a PgPool,
    pub queue_id: i64,
}

#[async_trait]
impl UploadCheckpoint for Checkpoint<'_> {
    async fn record_publish_id(&self, publish_id: &str) -> Result<(), UploadError> {
        if publish_id.trim().is_empty() {
            return Err(UploadError::Validation(
                "TikTok-Vorgangsnummer fehlt".into(),
            ));
        }
        let changed = sqlx::query(
            "UPDATE twitch_clips_upload_queue SET tiktok_publish_id = $1, last_attempt_at = NOW() \
             WHERE id = $2 AND platform = 'tiktok' AND status = 'inbox_pending' \
             AND tiktok_publish_id IS NULL",
        )
        .bind(publish_id)
        .bind(self.queue_id)
        .execute(self.pool)
        .await
        .map_err(|_| UploadError::Api("TikTok-Vorgang konnte nicht gesichert werden".into()))?
        .rows_affected();
        if changed != 1 {
            return Err(UploadError::Api(
                "TikTok-Vorgang ist bereits vergeben".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) async fn rejected(
    pool: &PgPool,
    queue_id: i64,
    publish_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = 'failed', last_attempt_at = NOW(), \
         last_error = 'TikTok hat diese Übertragung endgültig abgelehnt.' \
         WHERE id = $1 AND platform = 'tiktok' AND tiktok_publish_id = $2 \
         AND status IN ('inbox', 'inbox_pending')",
    )
    .bind(queue_id)
    .bind(publish_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn uncertain(pool: &PgPool, queue_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET last_error = \
         'Die TikTok-Übertragung ist noch nicht bestätigt. Wir prüfen denselben Vorgang weiter; ein zweiter Upload bleibt gesperrt.' \
         WHERE id = $1 AND platform = 'tiktok' AND status = 'inbox_pending'",
    )
    .bind(queue_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn release_not_started(pool: &PgPool, queue_id: i64) -> Result<bool, sqlx::Error> {
    let changed = sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = 'processing' \
         WHERE id = $1 AND platform = 'tiktok' AND status = 'inbox_pending' \
         AND tiktok_publish_id IS NULL",
    )
    .bind(queue_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(
                PgConnectOptions::from_str(&dsn)
                    .unwrap()
                    .options([("search_path", schema)]),
            )
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE twitch_clips_social_media (id BIGINT PRIMARY KEY, uploaded_tiktok BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT); \
             CREATE TABLE twitch_clips_upload_queue (id BIGINT PRIMARY KEY, clip_id BIGINT, platform TEXT DEFAULT 'tiktok', status TEXT DEFAULT 'pending', last_attempt_at TIMESTAMPTZ, last_error TEXT); \
             INSERT INTO twitch_clips_social_media (id) VALUES (1); \
             INSERT INTO twitch_clips_upload_queue (id, clip_id) VALUES (1,1),(2,1);"
        ).execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260930030000_social_media_tiktok_recovery.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn only_proven_not_started_operation_without_id_can_be_released() {
        let Some(pool) = pool("t_sm_tiktok_release_local").await else {
            return;
        };
        assert!(reserve(&pool, 1).await.unwrap());
        assert!(release_not_started(&pool, 1).await.unwrap());
        assert!(reserve(&pool, 1).await.unwrap());
        Checkpoint {
            pool: &pool,
            queue_id: 1,
        }
        .record_publish_id("begun")
        .await
        .unwrap();
        assert!(!release_not_started(&pool, 1).await.unwrap());
        assert!(!reserve(&pool, 2).await.unwrap());
    }

    #[tokio::test]
    async fn competing_jobs_reserve_only_one_transfer() {
        let Some(pool) = pool("t_sm_tiktok_reserve").await else {
            return;
        };
        let (a, b) = tokio::join!(reserve(&pool, 1), reserve(&pool, 2));
        assert_ne!(a.unwrap(), b.unwrap());
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE status = 'inbox_pending'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn restart_and_unknown_result_keep_the_same_reserved_operation() {
        let Some(pool) = pool("t_sm_tiktok_restart").await else {
            return;
        };
        assert!(reserve(&pool, 1).await.unwrap());
        let checkpoint = Checkpoint {
            pool: &pool,
            queue_id: 1,
        };
        checkpoint.record_publish_id("operation-1").await.unwrap();
        uncertain(&pool, 1).await.unwrap();
        assert!(!reserve(&pool, 1).await.unwrap());
        assert!(!reserve(&pool, 2).await.unwrap());
        let id: String = sqlx::query_scalar(
            "SELECT tiktok_publish_id FROM twitch_clips_upload_queue WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(id, "operation-1");
        assert!(checkpoint.record_publish_id("operation-2").await.is_err());
    }

    #[tokio::test]
    async fn init_without_id_stays_locked_until_matching_terminal_failure() {
        let Some(pool) = pool("t_sm_tiktok_terminal").await else {
            return;
        };
        assert!(reserve(&pool, 1).await.unwrap());
        uncertain(&pool, 1).await.unwrap();
        assert!(!reserve(&pool, 1).await.unwrap());
        rejected(&pool, 1, "not-our-operation").await.unwrap();
        assert!(!reserve(&pool, 1).await.unwrap());
        Checkpoint {
            pool: &pool,
            queue_id: 1,
        }
        .record_publish_id("operation-1")
        .await
        .unwrap();
        rejected(&pool, 1, "not-our-operation").await.unwrap();
        assert!(!reserve(&pool, 1).await.unwrap());
        rejected(&pool, 1, "operation-1").await.unwrap();
        assert!(reserve(&pool, 2).await.unwrap());
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

#[async_trait]
pub trait SocialReauthNotifier: Send + Sync {
    async fn notify(
        &self,
        twitch_user_id: &str,
        platform: &str,
        needs_reauth: bool,
        incident_at: DateTime<Utc>,
    ) -> bool;
}

pub struct ReauthState {
    pool: PgPool,
    notifier: Option<Arc<dyn SocialReauthNotifier>>,
}

#[derive(sqlx::FromRow)]
struct Connection {
    id: i32,
    platform: String,
    access_token_enc: Vec<u8>,
    token_expires_at: Option<String>,
    refresh_expires_at: Option<DateTime<Utc>>,
    needs_reauth: bool,
}

impl ReauthState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            notifier: None,
        }
    }

    pub fn with_notifier(mut self, notifier: Arc<dyn SocialReauthNotifier>) -> Self {
        self.notifier = Some(notifier);
        self
    }

    pub async fn mark_required(&self, auth_id: i32, access_enc: &[u8]) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE social_media_platform_auth SET needs_reauth = TRUE, \
             reauth_required_at = COALESCE(reauth_required_at, CURRENT_TIMESTAMP) \
             WHERE id = $1 AND access_token_enc = $2 AND enabled = 1",
        )
        .bind(auth_id)
        .bind(access_enc)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn sweep(&self, now: DateTime<Utc>) -> Result<(), sqlx::Error> {
        let connections = sqlx::query_as::<_, Connection>(
            "SELECT id, platform, access_token_enc, token_expires_at, refresh_expires_at, needs_reauth \
             FROM social_media_platform_auth WHERE enabled = 1",
        )
        .fetch_all(&self.pool)
        .await?;
        for connection in connections {
            let expiry = if connection.platform == "instagram" {
                connection.token_expires_at.as_deref().and_then(|raw| {
                    DateTime::parse_from_rfc3339(raw)
                        .ok()
                        .map(|date| date.with_timezone(&Utc))
                })
            } else {
                connection.refresh_expires_at
            };
            let expired = expiry.is_some_and(|expiry| expiry <= now);
            let pending = connection.needs_reauth
                || expiry.is_some_and(|expiry| expiry < now + Duration::days(7));
            sqlx::query(
                "UPDATE social_media_platform_auth \
                 SET needs_reauth = needs_reauth OR $3, \
                     reauth_required_at = CASE WHEN $4 OR needs_reauth THEN COALESCE(reauth_required_at, $5) ELSE NULL END, \
                     reauth_notified_at = CASE WHEN $4 OR needs_reauth THEN reauth_notified_at ELSE NULL END \
                 WHERE id = $1 AND access_token_enc = $2 AND enabled = 1",
            )
            .bind(connection.id)
            .bind(&connection.access_token_enc)
            .bind(expired)
            .bind(pending)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
        self.notify_pending().await
    }

    async fn notify_pending(&self) -> Result<(), sqlx::Error> {
        let Some(notifier) = &self.notifier else {
            return Ok(());
        };
        let ids = sqlx::query_scalar::<_, i32>(
            "SELECT id FROM social_media_platform_auth \
             WHERE enabled = 1 AND twitch_user_id IS NOT NULL \
               AND reauth_required_at IS NOT NULL AND reauth_notified_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        for id in ids {
            let mut tx = self.pool.begin().await?;
            let connection = sqlx::query_as::<_, (String, String, bool, DateTime<Utc>)>(
                "SELECT twitch_user_id, platform, needs_reauth, reauth_required_at FROM social_media_platform_auth \
                 WHERE id = $1 AND enabled = 1 AND twitch_user_id IS NOT NULL \
                   AND reauth_required_at IS NOT NULL AND reauth_notified_at IS NULL \
                 FOR UPDATE SKIP LOCKED",
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
            let mut delivered = false;
            if let Some((twitch_user_id, platform, needs_reauth, incident_at)) = connection {
                if !twitch_user_id.is_empty()
                    && twitch_user_id.bytes().all(|byte| byte.is_ascii_digit())
                    && notifier
                        .notify(&twitch_user_id, &platform, needs_reauth, incident_at)
                        .await
                {
                    sqlx::query("UPDATE social_media_platform_auth SET reauth_notified_at = CURRENT_TIMESTAMP WHERE id = $1")
                        .bind(id).execute(&mut *tx).await?;
                    delivered = true;
                }
            }
            tx.commit().await?;
            if delivered {
                tracing::info!(auth_id = id, "Social reauth DM delivered");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct Notifications(Mutex<Vec<(String, String, bool)>>);

    #[async_trait]
    impl SocialReauthNotifier for Notifications {
        async fn notify(
            &self,
            id: &str,
            platform: &str,
            required: bool,
            _incident_at: DateTime<Utc>,
        ) -> bool {
            self.0
                .lock()
                .await
                .push((id.into(), platform.into(), required));
            true
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn pool(schema: &str) -> PgPool {
        let dsn = crate::test_support::test_dsn().expect("Test database required");
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
        let options = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT NOT NULL, \
             twitch_user_id TEXT, access_token_enc BYTEA NOT NULL, enabled INTEGER DEFAULT 1, \
             token_expires_at TEXT, refresh_expires_at TIMESTAMPTZ, needs_reauth BOOLEAN NOT NULL DEFAULT FALSE, \
             reauth_required_at TIMESTAMPTZ, reauth_notified_at TIMESTAMPTZ)",
        ).execute(&pool).await.unwrap();
        pool
    }

    async fn seed(pool: &PgPool, platform: &str, expiry: Option<DateTime<Utc>>) -> i32 {
        sqlx::query_scalar(
            "INSERT INTO social_media_platform_auth (platform, twitch_user_id, access_token_enc, refresh_expires_at) \
             VALUES ($1, '42', decode('01','hex'), $2) RETURNING id",
        ).bind(platform).bind(expiry).fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn warning_survives_restart_and_escalation_without_duplicate_dm() {
        let pool = pool("t_sm_reauth_once").await;
        let id = seed(&pool, "tiktok", Some(now() + Duration::days(6))).await;
        let sent = Arc::new(Notifications::default());
        ReauthState::new(pool.clone())
            .with_notifier(sent.clone())
            .sweep(now())
            .await
            .unwrap();
        let restarted = ReauthState::new(pool.clone()).with_notifier(sent.clone());
        restarted.sweep(now()).await.unwrap();
        restarted.mark_required(id, &[1]).await.unwrap();
        restarted.sweep(now()).await.unwrap();
        assert_eq!(
            *sent.0.lock().await,
            vec![("42".into(), "tiktok".into(), false)]
        );
        let required: bool =
            sqlx::query_scalar("SELECT needs_reauth FROM social_media_platform_auth WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(required);
    }

    #[tokio::test]
    async fn concurrent_sweeps_send_one_dm() {
        let pool = pool("t_sm_reauth_concurrent").await;
        seed(&pool, "tiktok", Some(now() - Duration::days(1))).await;
        let sent = Arc::new(Notifications::default());
        let first = ReauthState::new(pool.clone()).with_notifier(sent.clone());
        let second = ReauthState::new(pool.clone()).with_notifier(sent.clone());
        let (a, b) = tokio::join!(first.sweep(now()), second.sweep(now()));
        a.unwrap();
        b.unwrap();
        assert_eq!(sent.0.lock().await.len(), 1);
        assert!(sent.0.lock().await[0].2);
    }

    #[tokio::test]
    async fn indefinite_and_boundary_connections_do_not_warn() {
        let pool = pool("t_sm_reauth_boundary").await;
        seed(&pool, "youtube", None).await;
        seed(&pool, "tiktok", Some(now() + Duration::days(7))).await;
        let sent = Arc::new(Notifications::default());
        ReauthState::new(pool)
            .with_notifier(sent.clone())
            .sweep(now())
            .await
            .unwrap();
        assert!(sent.0.lock().await.is_empty());
    }

    #[tokio::test]
    async fn instagram_expiry_is_its_own_deadline_and_stale_errors_cannot_invalidate_reconnect() {
        let pool = pool("t_sm_reauth_instagram").await;
        let id = seed(&pool, "instagram", None).await;
        sqlx::query("UPDATE social_media_platform_auth SET token_expires_at = $1, access_token_enc = decode('02','hex') WHERE id = $2")
            .bind((now() - Duration::hours(1)).to_rfc3339()).bind(id).execute(&pool).await.unwrap();
        let state = ReauthState::new(pool.clone());
        state.mark_required(id, &[1]).await.unwrap();
        let required: bool =
            sqlx::query_scalar("SELECT needs_reauth FROM social_media_platform_auth WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!required);
        let sent = Arc::new(Notifications::default());
        state
            .with_notifier(sent.clone())
            .sweep(now())
            .await
            .unwrap();
        assert_eq!(
            *sent.0.lock().await,
            vec![("42".into(), "instagram".into(), true)]
        );
    }

    #[tokio::test]
    async fn renewed_connection_resolves_warning_and_allows_new_incident() {
        let pool = pool("t_sm_reauth_resolved").await;
        let id = seed(&pool, "tiktok", Some(now() + Duration::days(6))).await;
        let sent = Arc::new(Notifications::default());
        let state = ReauthState::new(pool.clone()).with_notifier(sent.clone());
        state.sweep(now()).await.unwrap();
        sqlx::query("UPDATE social_media_platform_auth SET refresh_expires_at = $1 WHERE id = $2")
            .bind(now() + Duration::days(365))
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        state.sweep(now()).await.unwrap();
        let cleared: bool = sqlx::query_scalar("SELECT reauth_required_at IS NULL AND reauth_notified_at IS NULL FROM social_media_platform_auth WHERE id = $1")
            .bind(id).fetch_one(&pool).await.unwrap();
        assert!(cleared);
        state.mark_required(id, &[1]).await.unwrap();
        state.sweep(now()).await.unwrap();
        assert_eq!(sent.0.lock().await.len(), 2);
    }
}

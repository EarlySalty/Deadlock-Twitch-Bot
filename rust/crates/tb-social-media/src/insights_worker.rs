//! Insights-Worker (Port von
//! `bot/social_media/analytics/insights_worker.py`).
//!
//! Pollt für veröffentlichte Clips periodisch die Plattform-Statistiken und
//! persistiert 24h/7d/30d-Snapshots. Fällige Ziele = Clip × Plattform × Bucket,
//! deren `next_pull_at` leer oder erreicht ist. Erfolg → nächster Pull nach
//! bucket-spezifischer Frist; Fehler → Retry in 1h. An/Aus 1:1: dauerhaft an,
//! Intervall 30min, Batch 18.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use crate::analytics::{upsert_clip_analytics, ClipAnalyticsUpsert, BUCKETS, PLATFORMS};
use crate::credentials::{CredentialManager, SocialMediaCredentials};
use crate::uploaders::instagram::InstagramUploader;
use crate::uploaders::tiktok::TikTokUploader;
use crate::uploaders::PlatformUploader;

const INTERVAL_SECS: u64 = 30 * 60;
const INITIAL_DELAY_SECS: u64 = 75;
const BATCH_SIZE: i64 = 18;

/// Ein fälliges Analytics-Ziel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsTarget {
    pub clip_db_id: i64,
    pub streamer_login: String,
    pub platform: String,
    pub platform_video_id: String,
    pub bucket: String,
}

/// Wartezeit bis zum nächsten Pull bei Erfolg (Python `SUCCESS_POLL_DELAYS`).
fn success_delay(bucket: &str) -> chrono::Duration {
    match bucket {
        "24h" => chrono::Duration::hours(6),
        "7d" => chrono::Duration::hours(24),
        "30d" => chrono::Duration::days(3),
        _ => chrono::Duration::days(1),
    }
}

/// Baut den Analytics-Client (mirror `_resolve_client`): YouTube/TikTok immer
/// bauen, Instagram nur mit `platform_user_id`. Token kommt fertig vom
/// credential_manager.
fn resolve_insights_client(
    platform: &str,
    creds: &SocialMediaCredentials,
) -> Option<Arc<dyn PlatformUploader>> {
    match platform {
        "youtube" => Some(Arc::new(crate::upload_worker::youtube_uploader(creds))),
        "tiktok" => Some(Arc::new(TikTokUploader::new(creds.access_token.clone()))),
        "instagram" => {
            let uid = creds
                .platform_user_id
                .as_deref()
                .filter(|s| !s.is_empty())?;
            Some(Arc::new(InstagramUploader::new(
                creds.access_token.clone(),
                uid.to_string(),
            )))
        }
        _ => None,
    }
}

/// Sammelt fällige (Clip × Plattform × Bucket)-Ziele. `not_due` (next_pull_at in
/// der Zukunft) wird per SQL-Zeitvergleich bestimmt — robuster als Pythons
/// String-Vergleich, gleiche Absicht.
pub async fn collect_due_targets(pool: &PgPool, limit: i64) -> Vec<AnalyticsTarget> {
    let limit = limit.max(1);
    let clip_limit = (limit * 4).max(limit);
    let clip_rows = sqlx::query!(
        "SELECT id AS \"id!\", streamer_login AS \"streamer_login!\", \
                COALESCE(uploaded_tiktok, false) AS \"uploaded_tiktok!\", \
                COALESCE(uploaded_youtube, false) AS \"uploaded_youtube!\", \
                COALESCE(uploaded_instagram, false) AS \"uploaded_instagram!\", \
                tiktok_video_id, youtube_video_id, instagram_media_id \
           FROM twitch_clips_social_media \
          WHERE discarded_at IS NULL AND ( \
                (uploaded_tiktok AND tiktok_video_id IS NOT NULL) \
             OR (uploaded_youtube AND youtube_video_id IS NOT NULL) \
             OR (uploaded_instagram AND instagram_media_id IS NOT NULL)) \
          ORDER BY created_at DESC, id DESC LIMIT $1",
        clip_limit
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let analytics_rows = sqlx::query!(
        "SELECT clip_id AS \"clip_id!\", platform AS \"platform!\", COALESCE(bucket, '') AS \"bucket!\", \
                (next_pull_at IS NOT NULL AND next_pull_at > now()) AS \"not_due!\" \
           FROM twitch_clips_social_analytics",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut not_due: HashMap<(i64, String, String), bool> = HashMap::new();
    for r in &analytics_rows {
        let key = (r.clip_id, r.platform.clone(), r.bucket.clone());
        not_due.insert(key, r.not_due);
    }

    let mut due: Vec<AnalyticsTarget> = Vec::new();
    for row in &clip_rows {
        let id = row.id;
        let streamer_login = row.streamer_login.clone();
        let video_id_for = |platform: &str| -> Option<String> {
            let (uploaded, external_id) = match platform {
                "tiktok" => (row.uploaded_tiktok, row.tiktok_video_id.as_ref()),
                "youtube" => (row.uploaded_youtube, row.youtube_video_id.as_ref()),
                _ => (row.uploaded_instagram, row.instagram_media_id.as_ref()),
            };
            if !uploaded {
                return None;
            }
            external_id
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
        };

        for platform in PLATFORMS {
            let Some(video_id) = video_id_for(platform) else {
                continue;
            };
            for bucket in BUCKETS {
                if *not_due
                    .get(&(id, platform.to_string(), bucket.to_string()))
                    .unwrap_or(&false)
                {
                    continue;
                }
                due.push(AnalyticsTarget {
                    clip_db_id: id,
                    streamer_login: streamer_login.clone(),
                    platform: platform.to_string(),
                    platform_video_id: video_id.clone(),
                    bucket: bucket.to_string(),
                });
                if due.len() as i64 >= limit {
                    return due;
                }
            }
        }
    }
    due
}

/// Insights-Worker.
pub struct InsightsWorker {
    pool: PgPool,
    credentials: CredentialManager,
    batch_size: i64,
    interval: Duration,
}

impl InsightsWorker {
    pub fn new(pool: PgPool, credentials: CredentialManager) -> Self {
        Self {
            pool,
            credentials,
            batch_size: BATCH_SIZE,
            interval: Duration::from_secs(INTERVAL_SECS),
        }
    }

    async fn resolve_client(
        &self,
        platform: &str,
        streamer_login: &str,
    ) -> Option<Arc<dyn PlatformUploader>> {
        let creds = self
            .credentials
            .get_credentials(platform, Some(streamer_login))
            .await?;
        // Kein Rueckfall auf die Sammelverbindung. Der VOD-Worker sperrt ihn
        // bewusst, hier fehlte er: ein privates oder ungelistetes Partner-Video
        // wurde mit dem Betreiber-Token abgefragt, lieferte eine leere Trefferliste
        // und wurde als "0 Views" verbucht.
        if creds.streamer_login.as_deref() != Some(streamer_login) {
            tracing::debug!(
                platform = %platform,
                streamer = %streamer_login,
                "Insights uebersprungen: keine eigene Plattform-Verbindung"
            );
            return None;
        }
        resolve_insights_client(platform, &creds)
    }

    async fn schedule_retry(&self, target: &AnalyticsTarget, provider: &str) {
        let next_pull = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let _ = crate::analytics::schedule_clip_analytics_retry(
            &self.pool,
            target.clip_db_id,
            &target.platform,
            &target.bucket,
            Some(provider),
            &next_pull,
        )
        .await;
    }

    /// Ein Durchlauf (Python `_process_due_targets`).
    pub async fn run_once(&self) {
        let targets = collect_due_targets(&self.pool, self.batch_size).await;
        if targets.is_empty() {
            return;
        }
        let mut client_cache: HashMap<(String, String), Option<Arc<dyn PlatformUploader>>> =
            HashMap::new();
        for target in targets {
            let key = (target.platform.clone(), target.streamer_login.clone());
            let client = match client_cache.get(&key) {
                Some(c) => c.clone(),
                None => {
                    let c = self
                        .resolve_client(&target.platform, &target.streamer_login)
                        .await;
                    client_cache.insert(key, c.clone());
                    c
                }
            };
            let Some(client) = client else {
                self.schedule_retry(
                    &target,
                    &format!("error:{}:missing_client", target.platform),
                )
                .await;
                continue;
            };
            let metrics = match client
                .fetch_video_analytics(&target.platform_video_id, &target.bucket)
                .await
            {
                Ok(m) => m,
                Err(_) => {
                    self.schedule_retry(&target, &format!("error:{}:api", target.platform))
                        .await;
                    continue;
                }
            };
            let provider = if metrics.provider.trim().is_empty() {
                target.platform.clone()
            } else {
                metrics.provider.trim().to_string()
            };
            let next_pull = (Utc::now() + success_delay(&target.bucket)).to_rfc3339();
            let _ = upsert_clip_analytics(
                &self.pool,
                &ClipAnalyticsUpsert {
                    clip_db_id: target.clip_db_id,
                    platform: target.platform.clone(),
                    bucket: target.bucket.clone(),
                    views: metrics.views,
                    likes: metrics.likes,
                    comments: metrics.comments,
                    shares: metrics.shares,
                    watch_time_seconds: metrics.watch_time_seconds,
                    ctr_percent: metrics.ctr_percent,
                    engagement_rate: metrics.engagement_rate,
                    provider: Some(provider),
                    synced_at: None,
                    next_pull_at: Some(next_pull),
                },
            )
            .await;
        }
    }

    /// Hintergrund-Loop (75s Initial-Delay + 30min-Intervall). Noch nicht in
    /// tb-bot gespawnt (Wiring = Cutover-Slice).
    pub async fn run(&self) {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            self.run_once().await;
            tokio::time::sleep(self.interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
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
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, streamer_login TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), source_kind TEXT NOT NULL DEFAULT 'twitch', discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT)",
            "CREATE TABLE twitch_clips_social_analytics (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, bucket TEXT, synced_at TIMESTAMPTZ NOT NULL, next_pull_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn due_targets_selektion() {
        let Some(pool) = make_pool("t_sm_insights").await else {
            return;
        };
        // A: tiktok veröffentlicht, keine Analytics → 3 Buckets fällig.
        let a: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_tiktok, tiktok_video_id) VALUES ('a', 'https://clips.test/a', 'nani', TRUE, 'tt1') RETURNING id").fetch_one(&pool).await.unwrap();
        // B: youtube uploaded aber video_id NULL → gar kein Kandidat.
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_youtube) VALUES ('b', 'https://clips.test/b', 'nani', TRUE)").execute(&pool).await.unwrap();
        // C: tiktok veröffentlicht, 24h hat next_pull in der Zukunft → nur 7d/30d fällig.
        let c: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_tiktok, tiktok_video_id) VALUES ('c', 'https://clips.test/c', 'nani', TRUE, 'tt3') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, synced_at, next_pull_at) VALUES ($1, 'tiktok', '24h', NOW(), NOW() + INTERVAL '1 day')").bind(c).execute(&pool).await.unwrap();
        // D: verworfen → kein Kandidat.
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_tiktok, tiktok_video_id, discarded_at) VALUES ('d', 'https://clips.test/d', 'nani', TRUE, 'tt4', NOW())").execute(&pool).await.unwrap();

        let targets = collect_due_targets(&pool, 100).await;
        let keys: Vec<(i64, String)> = targets
            .iter()
            .map(|t| (t.clip_db_id, t.bucket.clone()))
            .collect();
        // A: alle 3 Buckets.
        assert!(
            keys.contains(&(a, "24h".into()))
                && keys.contains(&(a, "7d".into()))
                && keys.contains(&(a, "30d".into()))
        );
        // C: 7d/30d fällig, 24h NICHT.
        assert!(keys.contains(&(c, "7d".into())) && keys.contains(&(c, "30d".into())));
        assert!(!keys.contains(&(c, "24h".into())));
        // Gesamt genau 5 (A×3 + C×2), kein B/D.
        assert_eq!(targets.len(), 5);
        assert!(targets
            .iter()
            .all(|t| t.platform == "tiktok" && t.platform_video_id.starts_with("tt")));

        // Limit greift.
        assert_eq!(collect_due_targets(&pool, 2).await.len(), 2);
    }
}

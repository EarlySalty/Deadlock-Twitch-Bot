use std::time::{Duration, Instant};

use dashmap::DashMap;
use sqlx::PgPool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModerationSettings {
    pub global_ban_enabled: bool,
    pub scam_pitch_enabled: bool,
    pub spam_autoban_enabled: bool,
    pub sus_invite_enabled: bool,
}

impl Default for ModerationSettings {
    fn default() -> Self {
        Self {
            global_ban_enabled: true,
            scam_pitch_enabled: true,
            spam_autoban_enabled: true,
            sus_invite_enabled: true,
        }
    }
}

const CACHE_TTL: Duration = Duration::from_secs(60);

struct CacheEntry {
    settings: ModerationSettings,
    loaded_at: Instant,
}

pub struct ModerationSettingsCache {
    pool: PgPool,
    cache: DashMap<String, CacheEntry>,
    ttl: Duration,
}

impl ModerationSettingsCache {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache: DashMap::new(),
            ttl: CACHE_TTL,
        }
    }

    pub async fn settings(&self, channel_user_id: &str) -> ModerationSettings {
        if let Some(entry) = self.cache.get(channel_user_id) {
            if entry.loaded_at.elapsed() < self.ttl {
                return entry.settings;
            }
        }
        let settings = self.load(channel_user_id).await;
        self.cache.insert(
            channel_user_id.to_string(),
            CacheEntry {
                settings,
                loaded_at: Instant::now(),
            },
        );
        settings
    }

    async fn load(&self, channel_user_id: &str) -> ModerationSettings {
        let row: Result<Option<(bool, bool, bool, bool)>, sqlx::Error> = sqlx::query_as(
            "SELECT global_ban_enabled, scam_pitch_enabled, spam_autoban_enabled, sus_invite_enabled \
               FROM twitch_moderation_settings \
              WHERE channel_user_id = $1",
        )
        .bind(channel_user_id)
        .fetch_optional(&self.pool)
        .await;

        match row {
            Ok(Some((global_ban_enabled, scam_pitch_enabled, spam_autoban_enabled, sus_invite_enabled))) => {
                ModerationSettings {
                    global_ban_enabled,
                    scam_pitch_enabled,
                    spam_autoban_enabled,
                    sus_invite_enabled,
                }
            }
            Ok(None) => ModerationSettings::default(),
            Err(error) => {
                tracing::warn!(
                    %error,
                    channel_user_id,
                    "moderation-settings load failed, defaulting to all enabled"
                );
                ModerationSettings::default()
            }
        }
    }
}

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
        match self.load(channel_user_id).await {
            Ok(settings) => {
                self.cache.insert(
                    channel_user_id.to_string(),
                    CacheEntry {
                        settings,
                        loaded_at: Instant::now(),
                    },
                );
                settings
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    channel_user_id,
                    "moderation-settings load failed, defaulting to all enabled (nicht gecacht)"
                );
                ModerationSettings::default()
            }
        }
    }

    async fn load(&self, channel_user_id: &str) -> Result<ModerationSettings, sqlx::Error> {
        let row: Option<(bool, bool, bool, bool)> = sqlx::query_as(
            "SELECT global_ban_enabled, scam_pitch_enabled, spam_autoban_enabled, sus_invite_enabled \
               FROM twitch_moderation_settings \
              WHERE channel_user_id = $1",
        )
        .bind(channel_user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match row {
            Some((
                global_ban_enabled,
                scam_pitch_enabled,
                spam_autoban_enabled,
                sus_invite_enabled,
            )) => ModerationSettings {
                global_ban_enabled,
                scam_pitch_enabled,
                spam_autoban_enabled,
                sus_invite_enabled,
            },
            None => ModerationSettings::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn pool_in_schema(schema: &str) -> Option<PgPool> {
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return None;
        };
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
        Some(
            PgPoolOptions::new()
                .max_connections(4)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn db_fehler_vergiftet_den_cache_nicht() {
        let Some(pool) = pool_in_schema("modset_err_cache").await else {
            return;
        };
        let cache = ModerationSettingsCache::new(pool.clone());

        let first = cache.settings("kanal1").await;
        assert!(
            first.global_ban_enabled && first.spam_autoban_enabled,
            "fehlende Tabelle → Default alles an"
        );

        sqlx::query(
            "CREATE TABLE twitch_moderation_settings (
                channel_user_id      TEXT PRIMARY KEY,
                global_ban_enabled   BOOLEAN NOT NULL DEFAULT TRUE,
                scam_pitch_enabled   BOOLEAN NOT NULL DEFAULT TRUE,
                spam_autoban_enabled BOOLEAN NOT NULL DEFAULT TRUE,
                sus_invite_enabled   BOOLEAN NOT NULL DEFAULT TRUE
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_moderation_settings \
             (channel_user_id, global_ban_enabled, scam_pitch_enabled, spam_autoban_enabled, sus_invite_enabled) \
             VALUES ('kanal1', TRUE, TRUE, FALSE, TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let second = cache.settings("kanal1").await;
        assert!(
            !second.spam_autoban_enabled,
            "DB-Fehler wurde faelschlich gecacht und ueberdeckt die echte Zeile"
        );
    }

    #[tokio::test]
    async fn fehlende_zeile_wird_gecacht() {
        let Some(pool) = pool_in_schema("modset_none_cache").await else {
            return;
        };
        sqlx::query(
            "CREATE TABLE twitch_moderation_settings (
                channel_user_id      TEXT PRIMARY KEY,
                global_ban_enabled   BOOLEAN NOT NULL DEFAULT TRUE,
                scam_pitch_enabled   BOOLEAN NOT NULL DEFAULT TRUE,
                spam_autoban_enabled BOOLEAN NOT NULL DEFAULT TRUE,
                sus_invite_enabled   BOOLEAN NOT NULL DEFAULT TRUE
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let cache = ModerationSettingsCache::new(pool.clone());
        let first = cache.settings("kanal2").await;
        assert!(first.global_ban_enabled, "fehlende Zeile → alles an");

        sqlx::query(
            "INSERT INTO twitch_moderation_settings \
             (channel_user_id, global_ban_enabled, scam_pitch_enabled, spam_autoban_enabled, sus_invite_enabled) \
             VALUES ('kanal2', FALSE, TRUE, TRUE, TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let second = cache.settings("kanal2").await;
        assert!(
            second.global_ban_enabled,
            "Ok(None) ist ein echtes alles-an und bleibt 60s gecacht"
        );
    }
}

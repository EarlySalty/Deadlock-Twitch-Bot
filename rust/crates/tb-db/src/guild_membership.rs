use sqlx::PgPool;
use tb_domain::language::TextLanguage;

pub async fn home_guild_id(
    pool: &PgPool,
    twitch_user_id: &str,
    community_guild_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT guild_id FROM streamer_guilds WHERE twitch_user_id = $1 AND active ORDER BY (guild_id = $2) DESC, joined_at ASC, guild_id ASC LIMIT 1",
    )
    .bind(twitch_user_id)
    .bind(community_guild_id)
    .fetch_optional(pool)
    .await
}

pub fn community_guild_id() -> Option<i64> {
    tb_config::runtime::active()
        .and_then(|config| i64::try_from(config.settings().discord.oauth_followup.guild_id).ok())
}

pub async fn twitch_language(pool: &PgPool, broadcaster_id: &str) -> TextLanguage {
    let result = sqlx::query_scalar::<_, Option<String>>(
        "SELECT broadcaster_language FROM twitch_streamer_identities WHERE twitch_user_id = $1",
    )
    .bind(broadcaster_id)
    .fetch_optional(pool)
    .await;
    language_result(result)
}

pub async fn guild_language(pool: &PgPool, guild_id: i64) -> TextLanguage {
    let result = sqlx::query_scalar::<_, Option<String>>(
        "SELECT language FROM discord_guilds WHERE guild_id = $1",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await;
    language_result(result)
}

pub async fn dm_language(pool: &PgPool, twitch_user_id: &str) -> TextLanguage {
    let Some(community_guild_id) = community_guild_id() else {
        tracing::warn!("DM-Sprache nicht konfigurierbar; Deutsch wird verwendet");
        return TextLanguage::German;
    };
    dm_language_for_community(pool, twitch_user_id, community_guild_id).await
}

async fn dm_language_for_community(
    pool: &PgPool,
    twitch_user_id: &str,
    community_guild_id: i64,
) -> TextLanguage {
    match home_guild_id(pool, twitch_user_id, community_guild_id).await {
        Ok(Some(guild_id)) => guild_language(pool, guild_id).await,
        Ok(None) => TextLanguage::German,
        Err(error) => {
            tracing::warn!(%error, "Heimatserver nicht abrufbar; Deutsch wird verwendet");
            TextLanguage::German
        }
    }
}

fn language_result(result: Result<Option<Option<String>>, sqlx::Error>) -> TextLanguage {
    match result {
        Ok(code) => TextLanguage::from_code(code.flatten().as_deref()),
        Err(error) => {
            tracing::warn!(%error, "Textsprache nicht abrufbar; Deutsch wird verwendet");
            TextLanguage::German
        }
    }
}

pub async fn channel_language(pool: &PgPool, channel_id: i64) -> TextLanguage {
    let result = sqlx::query_scalar::<_, Option<String>>(
        "SELECT language FROM discord_guilds WHERE announce_channel_id = $1 OR alert_channel_id = $1 ORDER BY guild_id LIMIT 1",
    ).bind(channel_id).fetch_optional(pool).await;
    language_result(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn destination_language_and_home_membership_selection() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = &database.pool;
        sqlx::raw_sql("CREATE TABLE streamer_guilds (twitch_user_id text, guild_id bigint, active boolean, joined_at timestamptz); CREATE TABLE discord_guilds (guild_id bigint, language text, announce_channel_id bigint, alert_channel_id bigint); CREATE TABLE twitch_streamer_identities (twitch_user_id text, broadcaster_language text); INSERT INTO discord_guilds VALUES (1, 'de', 10, 11), (2, 'en', 20, 21), (3, 'fr', 30, 31); INSERT INTO twitch_streamer_identities VALUES ('42', 'en-GB'), ('43', NULL), ('44', 'fr'); INSERT INTO streamer_guilds VALUES ('42', 1, true, '2026-02-01'), ('42', 2, true, '2026-01-01'), ('42', 3, true, '2026-01-01')")
            .execute(pool).await.expect("language fixture");
        assert_eq!(home_guild_id(pool, "42", 1).await.expect("home"), Some(1));
        assert_eq!(twitch_language(pool, "42").await, TextLanguage::English);
        assert_eq!(guild_language(pool, 1).await, TextLanguage::German);
        assert_eq!(
            dm_language_for_community(pool, "42", 1).await,
            TextLanguage::German
        );
        assert_eq!(channel_language(pool, 20).await, TextLanguage::English);
        assert_eq!(channel_language(pool, 21).await, TextLanguage::English);
        sqlx::query("UPDATE streamer_guilds SET active = false WHERE guild_id = 1")
            .execute(pool)
            .await
            .expect("deactivate community membership");
        assert_eq!(
            home_guild_id(pool, "42", 1)
                .await
                .expect("oldest membership"),
            Some(2)
        );
        assert_eq!(
            dm_language_for_community(pool, "42", 1).await,
            TextLanguage::English
        );
        sqlx::query("UPDATE streamer_guilds SET active = false")
            .execute(pool)
            .await
            .expect("deactivate all");
        assert_eq!(
            home_guild_id(pool, "42", 1).await.expect("no membership"),
            None
        );
        for user_id in ["43", "44", "missing"] {
            assert_eq!(twitch_language(pool, user_id).await, TextLanguage::German);
        }
        for guild_id in [3, 999] {
            assert_eq!(guild_language(pool, guild_id).await, TextLanguage::German);
        }
        assert_eq!(
            home_guild_id(pool, "missing", 1)
                .await
                .expect("unknown recipient"),
            None
        );
        assert_eq!(
            dm_language_for_community(pool, "42", 1).await,
            TextLanguage::German
        );
        pool.close().await;
        assert_eq!(twitch_language(pool, "42").await, TextLanguage::German);
        assert_eq!(guild_language(pool, 2).await, TextLanguage::German);
        assert_eq!(
            dm_language_for_community(pool, "42", 1).await,
            TextLanguage::German
        );
    }
}

use sqlx::PgPool;

pub async fn home_guild_id(
    pool: &PgPool,
    twitch_user_id: &str,
    community_guild_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT sg.guild_id FROM streamer_guilds sg JOIN discord_guilds g USING (guild_id)
         WHERE sg.twitch_user_id = $1 AND sg.active AND g.status = 'active' AND g.removed_at IS NULL
         ORDER BY (sg.guild_id = $2) DESC, sg.joined_at, sg.guild_id LIMIT 1",
    )
    .bind(twitch_user_id)
    .bind(community_guild_id)
    .fetch_optional(pool)
    .await
}

pub fn community_guild_id() -> Result<i64, sqlx::Error> {
    let config =
        tb_config::runtime::settings().map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
    i64::try_from(config.discord.streamer_link.guild_id)
        .map_err(|error| sqlx::Error::Protocol(error.to_string()))
}

pub async fn scoped_invite_for_id(
    pool: &PgPool,
    twitch_user_id: &str,
) -> Result<Option<(String, bool)>, sqlx::Error> {
    let user_id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_streamer_identities WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await?;
    match user_id {
        Some(user_id) => scoped_invite_for_user(pool, user_id).await,
        None => Ok(None),
    }
}

pub async fn scoped_invite_for_login(
    pool: &PgPool,
    twitch_login: &str,
) -> Result<Option<(String, bool)>, sqlx::Error> {
    let user_id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_streamer_identities
         WHERE lower(twitch_login) = lower($1) ORDER BY twitch_user_id LIMIT 1",
    )
    .bind(twitch_login)
    .fetch_optional(pool)
    .await?;
    match user_id {
        Some(user_id) => scoped_invite_for_user(pool, user_id).await,
        None => Ok(None),
    }
}

async fn scoped_invite_for_user(
    pool: &PgPool,
    user_id: String,
) -> Result<Option<(String, bool)>, sqlx::Error> {
    let Some(home) = home_guild_id(pool, &user_id, community_guild_id()?).await? else {
        return Ok(None);
    };
    sqlx::query_as(
        "SELECT invite_url, true AS specific FROM twitch_streamer_invites
         WHERE guild_id = $1 AND (twitch_user_id = $2 OR (twitch_user_id IS NULL AND lower(streamer_login) IN
           (SELECT lower(twitch_login) FROM twitch_streamer_identities WHERE twitch_user_id = $2)))
           AND btrim(invite_url) <> ''
         UNION ALL
         SELECT promo_invite_url, false FROM discord_guilds
         WHERE guild_id = $1 AND btrim(promo_invite_url) <> '' ORDER BY specific DESC LIMIT 1",
    )
    .bind(home)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub fn language_group(language: &str) -> Option<String> {
    let normalized = language.trim().to_ascii_lowercase().replace('_', "-");
    let group = normalized.split('-').next()?;
    if group.len() < 2
        || group.len() > 8
        || !group.bytes().all(|byte| byte.is_ascii_lowercase())
        || matches!(group, "und" | "other")
    {
        return None;
    }
    Some(group.to_string())
}

pub fn raid_scope_allows(
    source_guilds: &[(i64, bool)],
    home: i64,
    target_guilds: &[(i64, bool)],
    same_language: bool,
) -> bool {
    source_guilds
        .iter()
        .any(|(source, _)| target_guilds.iter().any(|(target, _)| source == target))
        || (same_language
            && source_guilds
                .iter()
                .any(|(guild, enabled)| *guild == home && *enabled)
            && target_guilds.iter().any(|(_, enabled)| *enabled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_groups_normalize_regions_without_unknown_fallback() {
        for language in ["de", "de-de", "DE_at", "de-ch"] {
            assert_eq!(language_group(language).as_deref(), Some("de"));
        }
        for language in ["en", "en-gb", "EN_US"] {
            assert_eq!(language_group(language).as_deref(), Some("en"));
        }
        assert_eq!(language_group("fr-ca").as_deref(), Some("fr"));
        assert_eq!(language_group("und"), None);
        assert_eq!(language_group(""), None);
    }

    #[test]
    fn shared_server_ignores_language_and_cross_server_needs_language_and_both_switches() {
        assert!(raid_scope_allows(&[(1, false)], 1, &[(1, false)], true));
        assert!(raid_scope_allows(&[(1, false)], 1, &[(1, false)], false));
        assert!(!raid_scope_allows(&[(1, false)], 1, &[(2, true)], true));
        assert!(!raid_scope_allows(&[(1, true)], 1, &[(2, false)], true));
        assert!(raid_scope_allows(&[(1, true)], 1, &[(2, true)], true));
        assert!(!raid_scope_allows(&[(1, true)], 1, &[(2, true)], false));
        assert!(raid_scope_allows(
            &[(1, false), (2, false)],
            1,
            &[(2, false)],
            false
        ));
        assert!(!raid_scope_allows(
            &[(1, false), (2, true)],
            1,
            &[(3, true)],
            true
        ));
        assert!(!raid_scope_allows(&[], 1, &[(2, true)], true));
    }
}

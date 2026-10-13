use sqlx::PgPool;

use crate::GuildEventSink;

#[derive(Clone)]
pub struct PgSink {
    pool: PgPool,
}

impl PgSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupPersist {
    pub language: Option<String>,
    pub announce_channel_id: Option<i64>,
    pub alert_channel_id: Option<i64>,
    pub streamer_role_id: Option<i64>,
    pub ping_role_id: Option<i64>,
    pub promo_invite_url: Option<String>,
    pub auto_partner_default: Option<bool>,
}

impl SetupPersist {
    pub fn has_updates(&self) -> bool {
        self.language.is_some()
            || self.announce_channel_id.is_some()
            || self.alert_channel_id.is_some()
            || self.streamer_role_id.is_some()
            || self.ping_role_id.is_some()
            || self.promo_invite_url.is_some()
            || self.auto_partner_default.is_some()
    }
}

#[async_trait::async_trait]
impl GuildEventSink for PgSink {
    async fn guild_added(&self, guild_id: u64, name: &str) {
        if let Err(error) = upsert_guild(&self.pool, guild_id, name).await {
            tracing::error!(error = %error, guild_id, "Guild-Upsert fehlgeschlagen");
        }
    }

    async fn guild_removed(&self, guild_id: u64) {
        if let Err(error) = mark_guild_removed(&self.pool, guild_id).await {
            tracing::error!(error = %error, guild_id, "Guild-Entfernen fehlgeschlagen");
        }
    }

    async fn member_left(&self, guild_id: u64, user_id: u64) {
        if let Err(error) = deactivate_member(&self.pool, guild_id, user_id).await {
            tracing::error!(error = %error, guild_id, user_id, "Mitglied-Deaktivierung fehlgeschlagen");
        }
    }
}

pub async fn upsert_guild(pool: &PgPool, guild_id: u64, name: &str) -> Result<(), sqlx::Error> {
    let Some(guild_id) = to_guild_id(guild_id) else {
        return Ok(());
    };
    let name: String = name.chars().take(200).collect();
    sqlx::query(
        r#"
        INSERT INTO discord_guilds (guild_id, name, status, removed_at, updated_at)
        VALUES ($1, $2, 'active', NULL, NOW())
        ON CONFLICT (guild_id) DO UPDATE SET
            name = CASE WHEN EXCLUDED.name <> '' THEN EXCLUDED.name ELSE discord_guilds.name END,
            status = 'active',
            removed_at = NULL,
            updated_at = NOW()
        "#,
    )
    .bind(guild_id)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_guild_removed(pool: &PgPool, guild_id: u64) -> Result<(), sqlx::Error> {
    let Some(guild_id) = to_guild_id(guild_id) else {
        return Ok(());
    };
    sqlx::query(
        r#"
        UPDATE discord_guilds
           SET status = 'removed',
               removed_at = COALESCE(removed_at, NOW()),
               updated_at = NOW()
         WHERE guild_id = $1
        "#,
    )
    .bind(guild_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn deactivate_member(
    pool: &PgPool,
    guild_id: u64,
    discord_user_id: u64,
) -> Result<(), sqlx::Error> {
    let Some(guild_id) = to_guild_id(guild_id) else {
        return Ok(());
    };
    let discord_user_id = discord_user_id.to_string();
    sqlx::query(
        r#"
        UPDATE streamer_guilds sg
           SET active = FALSE,
               deactivated_by = 'discord_leave',
               deactivated_at = COALESCE(sg.deactivated_at, NOW())
          FROM twitch_streamer_identities i
         WHERE sg.twitch_user_id = i.twitch_user_id
           AND sg.guild_id = $1
           AND sg.active
           AND i.discord_user_id = $2
        "#,
    )
    .bind(guild_id)
    .bind(discord_user_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn persist_setup(
    pool: &PgPool,
    guild_id: u64,
    name: &str,
    persist: &SetupPersist,
) -> Result<(), sqlx::Error> {
    let Some(id) = to_guild_id(guild_id) else {
        return Ok(());
    };
    upsert_guild(pool, guild_id, name).await?;
    if !persist.has_updates() {
        return Ok(());
    }
    sqlx::query(
        r#"
        UPDATE discord_guilds
           SET language = COALESCE($2, language),
               announce_channel_id = COALESCE($3, announce_channel_id),
               alert_channel_id = COALESCE($4, alert_channel_id),
               streamer_role_id = COALESCE($5, streamer_role_id),
               ping_role_id = COALESCE($6, ping_role_id),
               promo_invite_url = COALESCE($7, promo_invite_url),
               auto_partner_default = COALESCE($8, auto_partner_default),
               status = 'active',
               removed_at = NULL,
               updated_at = NOW(),
               updated_by = 'setup'
         WHERE guild_id = $1
        "#,
    )
    .bind(id)
    .bind(persist.language.as_deref())
    .bind(persist.announce_channel_id)
    .bind(persist.alert_channel_id)
    .bind(persist.streamer_role_id)
    .bind(persist.ping_role_id)
    .bind(persist.promo_invite_url.as_deref())
    .bind(persist.auto_partner_default)
    .execute(pool)
    .await?;
    Ok(())
}

fn to_guild_id(guild_id: u64) -> Option<i64> {
    i64::try_from(guild_id).ok().filter(|id| *id > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_persist_erkennt_leere_und_gesetzte_felder() {
        assert!(!SetupPersist::default().has_updates());
        assert!(SetupPersist {
            language: Some("en".into()),
            ..SetupPersist::default()
        }
        .has_updates());
    }

    #[test]
    fn guild_id_null_und_overflow_werden_verworfen() {
        assert_eq!(to_guild_id(0), None);
        assert!(to_guild_id(1).is_some());
        assert_eq!(to_guild_id(u64::MAX), None);
    }
}

//! Bestehende Discord-Betriebsziele. Gleichnamige frühere ENV-Overrides hatten
//! je Verbraucher verschiedene Fallbacks; diese bleiben ausdrücklich getrennt.
use crate::{
    file::FileError,
    global::{public_url, range},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const COMMUNITY: u64 = 1_289_721_245_281_292_288;
const STREAMER_ROLE: u64 = 1_313_624_729_466_441_769;

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DiscordOperations {
    pub chat: DiscordChat,
    pub internal: DiscordInternal,
    pub streamer_link: StreamerLink,
    pub oauth_followup: OAuthFollowup,
    pub token_lifecycle: TokenLifecycle,
    pub raid_oauth: RaidOAuth,
    pub shadow_review_channel_id: Option<i64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscordChat {
    pub moderation_alert_channel_id: u64,
    pub promo_invite: Option<String>,
}
impl Default for DiscordChat {
    fn default() -> Self { Self { moderation_alert_channel_id: 1374364800817303632, promo_invite: None } }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscordInternal { pub owner_id: String }
impl Default for DiscordInternal {
    fn default() -> Self { Self { owner_id: "662995601738170389".into() } }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StreamerLink {
    pub enabled: bool,
    pub notify_channel_id: u64,
    pub streamer_role_id: u64,
    pub guild_id: u64,
    pub state_path: PathBuf,
}
impl Default for StreamerLink {
    fn default() -> Self {
        Self {
            enabled: true,
            notify_channel_id: 1_374_364_800_817_303_632,
            streamer_role_id: STREAMER_ROLE,
            guild_id: COMMUNITY,
            state_path: "/home/naniadm/Documents/Deadlock-Bots/data/streamer_link_state.json"
                .into(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct OAuthFollowup {
    pub guild_id: u64,
    pub streamer_role_id: u64,
}
impl Default for OAuthFollowup {
    fn default() -> Self {
        Self {
            guild_id: COMMUNITY,
            streamer_role_id: STREAMER_ROLE,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TokenLifecycle {
    pub guild_id: Option<u64>,
    pub streamer_role_id: u64,
}
impl Default for TokenLifecycle {
    fn default() -> Self {
        Self {
            guild_id: None,
            streamer_role_id: STREAMER_ROLE,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RaidOAuth {
    /// None: bisher kein Guard. Some([]): Guard aktiv, niemand erlaubt.
    pub allowed_guild_ids: Option<Vec<i64>>,
    pub allowed_channel_ids: Option<Vec<i64>>,
    pub allowed_role_ids: Option<Vec<i64>>,
    pub success_redirect_url: String,
}
impl Default for RaidOAuth {
    fn default() -> Self {
        Self {
            allowed_guild_ids: None,
            allowed_channel_ids: None,
            allowed_role_ids: None,
            success_redirect_url: "https://deutsche-deadlock-community.de/twitch/dashboard".into(),
        }
    }
}

impl DiscordOperations {
    pub fn validate(&self) -> Result<(), FileError> {
        crate::global::positive_id(&self.internal.owner_id, "discord.internal.owner_id")?;
        range(self.chat.moderation_alert_channel_id, 1, u64::MAX, "discord.chat.moderation_alert_channel_id")?;
        if let Some(url) = &self.chat.promo_invite { public_url(url, "discord.chat.promo_invite", false)?; }
        for (id, field) in [
            (
                self.streamer_link.notify_channel_id,
                "discord.streamer_link.notify_channel_id",
            ),
            (
                self.streamer_link.streamer_role_id,
                "discord.streamer_link.streamer_role_id",
            ),
            (
                self.streamer_link.guild_id,
                "discord.streamer_link.guild_id",
            ),
            (
                self.oauth_followup.guild_id,
                "discord.oauth_followup.guild_id",
            ),
            (
                self.oauth_followup.streamer_role_id,
                "discord.oauth_followup.streamer_role_id",
            ),
            (
                self.token_lifecycle.streamer_role_id,
                "discord.token_lifecycle.streamer_role_id",
            ),
        ] {
            range(id, 1, u64::MAX, field)?;
        }
        if let Some(id) = self.token_lifecycle.guild_id {
            range(id, 1, u64::MAX, "discord.token_lifecycle.guild_id")?;
        }
        if self.shadow_review_channel_id.is_some_and(|id| id <= 0) {
            return Err(FileError::invalid("discord.shadow_review_channel_id"));
        }
        let path = self
            .streamer_link
            .state_path
            .to_str()
            .ok_or_else(|| FileError::invalid("discord.streamer_link.state_path"))?;
        if path.is_empty() || path.chars().any(char::is_control) {
            return Err(FileError::invalid("discord.streamer_link.state_path"));
        }
        for (ids, field) in [
            (
                &self.raid_oauth.allowed_guild_ids,
                "discord.raid_oauth.allowed_guild_ids",
            ),
            (
                &self.raid_oauth.allowed_channel_ids,
                "discord.raid_oauth.allowed_channel_ids",
            ),
            (
                &self.raid_oauth.allowed_role_ids,
                "discord.raid_oauth.allowed_role_ids",
            ),
        ] {
            if ids
                .as_ref()
                .is_some_and(|ids| ids.iter().any(|id| *id <= 0))
            {
                return Err(FileError::invalid(field));
            }
        }
        public_url(
            &self.raid_oauth.success_redirect_url,
            "discord.raid_oauth.success_redirect_url",
            false,
        )
    }
}

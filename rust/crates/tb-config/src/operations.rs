//! Bestehende Bot-Betriebsoptionen. Defaults stammen aus der bisherigen
//! Composition-Root; keine Aktivierung neuer Aufgaben durch die Migration.
use crate::{file::FileError, global::range};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BotOperations {
    pub runtime_role: String,
    pub runtime_enforce: bool,
    pub legacy_internal_api_port: Option<u16>,
    pub run_database_migrations: bool,
    pub highlight_clipper_enabled: bool,
    pub monitoring_poll_enabled: bool,
    pub clip_fetcher_enabled: bool,
    pub eventsub_receiver_port: u16,
    pub scam_guard_discord_channel_id: u64,
    pub live_ping_guild_id: u64,
    pub alert_mention: Option<String>,
    pub discord_ref_code: Option<String>,
    pub chat_enabled: bool,
    pub chat_persist_all_games: bool,
    pub lfg_pitch_enabled: bool,
    pub golive_tips_enabled: bool,
    pub irc_lurker_enabled: bool,
    pub outreach_shadow_enabled: bool,
    pub smalltalk_loop_enabled: bool,
    pub smalltalk_loop_live_send: bool,
    pub auto_raid_offline_grace_seconds: u64,
    pub auto_unraid_window_seconds: f64,
    pub flip_repeat_window_seconds: f64,
    pub flip_pause_seconds: f64,
    pub mcp_host: std::net::IpAddr,
    pub mcp_port: u16,
    pub raid_redirect_uri: String,
    pub clip_raid_redirect_uri: String,
    pub legacy_proxy_base_url: Option<String>,
    pub legacy_greeter_base_url: String,
    pub chat_review_log_directory: PathBuf,
    pub service_warning_log_directory: PathBuf,
    pub yt_dlp_binary: Option<PathBuf>,
    pub outreach_yt_dlp_binary: Option<PathBuf>,
    pub vod_export_remote_base: String,
}

impl Default for BotOperations {
    fn default() -> Self {
        Self {
            runtime_role: String::new(),
            runtime_enforce: true,
            legacy_internal_api_port: None,
            run_database_migrations: true,
            highlight_clipper_enabled: false,
            monitoring_poll_enabled: false,
            clip_fetcher_enabled: false,
            eventsub_receiver_port: 8786,
            scam_guard_discord_channel_id: 1_374_364_800_817_303_632,
            live_ping_guild_id: 1_289_721_245_281_292_288,
            alert_mention: None,
            discord_ref_code: None,
            chat_enabled: false,
            chat_persist_all_games: true,
            lfg_pitch_enabled: true,
            golive_tips_enabled: false,
            irc_lurker_enabled: false,
            outreach_shadow_enabled: false,
            smalltalk_loop_enabled: false,
            smalltalk_loop_live_send: false,
            auto_raid_offline_grace_seconds: 5,
            auto_unraid_window_seconds: 90.0,
            flip_repeat_window_seconds: 600.0,
            flip_pause_seconds: 43200.0,
            mcp_host: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            mcp_port: 8892,
            raid_redirect_uri: "https://deutsche-deadlock-community.de/callback/twitch".into(),
            clip_raid_redirect_uri: String::new(),
            legacy_proxy_base_url: None,
            legacy_greeter_base_url: "http://127.0.0.1:8779".into(),
            chat_review_log_directory: "logs".into(),
            service_warning_log_directory: "logs".into(),
            yt_dlp_binary: None,
            outreach_yt_dlp_binary: None,
            vod_export_remote_base: "gdrive:Deadlock/Twitch-VODs".into(),
        }
    }
}

impl BotOperations {
    pub fn validate(&self) -> Result<(), FileError> {
        use crate::global::public_url;
        if self.runtime_role.len() > 64 || self.runtime_role.chars().any(char::is_control) {
            return Err(FileError::invalid("bot.runtime_role"));
        }
        if self.legacy_internal_api_port.is_some_and(|port| port == 0 || port == 8766) {
            return Err(FileError::invalid("bot.legacy_internal_api_port"));
        }
        public_url(&self.raid_redirect_uri, "bot.raid_redirect_uri", false)?;
        if !self.clip_raid_redirect_uri.is_empty() {
            public_url(
                &self.clip_raid_redirect_uri,
                "bot.clip_raid_redirect_uri",
                false,
            )?;
        }
        if let Some(url) = &self.legacy_proxy_base_url {
            public_url(url, "bot.legacy_proxy_base_url", false)?;
        }
        public_url(
            &self.legacy_greeter_base_url,
            "bot.legacy_greeter_base_url",
            false,
        )?;
        for (path, field) in [
            (
                Some(&self.chat_review_log_directory),
                "bot.chat_review_log_directory",
            ),
            (
                Some(&self.service_warning_log_directory),
                "bot.service_warning_log_directory",
            ),
            (self.yt_dlp_binary.as_ref(), "bot.yt_dlp_binary"),
            (self.outreach_yt_dlp_binary.as_ref(), "bot.outreach_yt_dlp_binary"),
        ] {
            if let Some(path) = path {
                let value = path.to_str().ok_or_else(|| FileError::invalid(field))?;
                if value.trim().is_empty() || value.chars().any(char::is_control) {
                    return Err(FileError::invalid(field));
                }
            }
        }
        if self.vod_export_remote_base.trim().is_empty()
            || self.vod_export_remote_base.chars().any(char::is_control)
        {
            return Err(FileError::invalid("bot.vod_export_remote_base"));
        }
        if !self.mcp_host.is_loopback() {
            return Err(FileError::invalid("bot.mcp_host"));
        }
        range(u64::from(self.mcp_port), 1, 65_535, "bot.mcp_port")?;
        for (seconds, field) in [
            (
                self.auto_unraid_window_seconds,
                "bot.auto_unraid_window_seconds",
            ),
            (
                self.flip_repeat_window_seconds,
                "bot.flip_repeat_window_seconds",
            ),
            (self.flip_pause_seconds, "bot.flip_pause_seconds"),
        ] {
            if !seconds.is_finite() || seconds < 0.0 {
                return Err(FileError::invalid(field));
            }
        }
        range(
            u64::from(self.eventsub_receiver_port),
            1,
            65_535,
            "bot.eventsub_receiver_port",
        )?;
        range(
            self.scam_guard_discord_channel_id,
            1,
            i64::MAX as u64,
            "bot.scam_guard_discord_channel_id",
        )?;
        range(
            self.live_ping_guild_id,
            1,
            u64::MAX,
            "bot.live_ping_guild_id",
        )?;
        for (value, field) in [
            (&self.alert_mention, "bot.alert_mention"),
            (&self.discord_ref_code, "bot.discord_ref_code"),
        ] {
            if value
                .as_ref()
                .is_some_and(|s| s.len() > 4096 || s.contains('\0'))
            {
                return Err(FileError::invalid(field));
            }
        }
        Ok(())
    }
}

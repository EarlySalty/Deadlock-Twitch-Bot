//! Bestehende Bot-Betriebsoptionen. Defaults stammen aus der bisherigen
//! Composition-Root; keine Aktivierung neuer Aufgaben durch die Migration.
use crate::{file::FileError, global::range};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BotOperations {
    pub run_database_migrations: bool,
    pub highlight_clipper_enabled: bool,
    pub monitoring_poll_enabled: bool,
    pub clip_fetcher_enabled: bool,
    pub eventsub_receiver_port: u16,
    pub scam_guard_discord_channel_id: u64,
    pub live_ping_guild_id: u64,
    pub alert_mention: Option<String>,
    pub discord_ref_code: Option<String>,
}

impl Default for BotOperations {
    fn default() -> Self {
        Self {
            run_database_migrations: true,
            highlight_clipper_enabled: false,
            monitoring_poll_enabled: false,
            clip_fetcher_enabled: false,
            eventsub_receiver_port: 8786,
            scam_guard_discord_channel_id: 1_374_364_800_817_303_632,
            live_ping_guild_id: 1_289_721_245_281_292_288,
            alert_mention: None,
            discord_ref_code: None,
        }
    }
}

impl BotOperations {
    pub fn validate(&self) -> Result<(), FileError> {
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

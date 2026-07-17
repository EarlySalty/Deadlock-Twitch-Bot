//! Composition-Root für die OAuth-Callback-Followups — verdrahtet die
//! `tb_raid::partner_setup`-Ports mit den echten Transporten:
//!
//! - **Discord** (Display-Name + Streamer-Rolle) → Master-Broker 8770
//!   (`resolve-user` / `member/add-role`). Python machte beides in-process
//!   über den lokalen Discord-Bot (`bot/discord_role_sync.py`).
//! - **Moderator-Einsetzung** → Helix `POST /moderation/moderators` mit dem
//!   Streamer-Token (`tb_transport_twitch::moderation`).
//! - **Chat-Begrüßung** → Delegation an den Python-Chat-Prozess via
//!   `POST /internal/twitch/v1/streamers/{login}/chat-action` auf dem
//!   Legacy-Seitenport 8779 — bis zum Chat-Cutover (Welle B).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use tb_raid::partner_setup::{
    ChatGreeterPort, DiscordDirectoryPort, ModeratorInstallPort, PartnerSetupService,
};
use tb_transport_discord::BrokerRelay;
use tb_transport_twitch::{AddModeratorOutcome, HelixClient};

/// Discord-Streamer-Rolle (Python `_DEFAULT_STREAMER_ROLE_ID`,
/// `bot/discord_role_sync.py:14`; Env `STREAMER_ROLE_ID` überschreibt).
const DEFAULT_STREAMER_ROLE_ID: u64 = 1313624729466441769;

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
}

// ---------------------------------------------------------------------------
// Discord via Master-Broker
// ---------------------------------------------------------------------------

/// Rollen-Sync + User-Auflösung über den Master-Broker.
///
/// Guild-Kandidaten wie Python `iter_role_guild_candidates`:
/// `STREAMER_GUILD_ID` → `MAIN_GUILD_ID` → alle vom Broker gemeldeten Guilds.
pub struct BrokerDiscordDirectory {
    relay: Option<BrokerRelay>,
    guild_id: Option<u64>,
    role_id: u64,
}

impl BrokerDiscordDirectory {
    pub fn from_env(relay: Option<BrokerRelay>) -> Self {
        Self {
            relay,
            guild_id: env_u64("STREAMER_GUILD_ID").or_else(|| env_u64("MAIN_GUILD_ID")),
            role_id: env_u64("STREAMER_ROLE_ID").unwrap_or(DEFAULT_STREAMER_ROLE_ID),
        }
    }

    async fn role_guild_ids(&self, relay: &BrokerRelay) -> Vec<u64> {
        if let Some(guild_id) = self.guild_id {
            return vec![guild_id];
        }
        let members = match relay.list_members().await {
            Ok(members) => members,
            Err(error) => {
                tracing::warn!(
                    "Streamer-Rollen-Sync: Guild-Fallback via Broker-Mitglieder fehlgeschlagen: {error}"
                );
                return Vec::new();
            }
        };
        let mut ids = std::collections::BTreeSet::new();
        for member in members {
            if let Some(guild_id) = member.guild_id.filter(|id| *id > 0) {
                ids.insert(guild_id);
            }
        }
        ids.into_iter().collect()
    }
}

#[async_trait]
impl DiscordDirectoryPort for BrokerDiscordDirectory {
    async fn resolve_display_name(&self, discord_user_id: &str) -> Option<String> {
        let relay = self.relay.as_ref()?;
        let user_id: u64 = discord_user_id.parse().ok()?;
        match relay.resolve_user(user_id).await {
            Ok(Some(user)) => user.preferred_display_name(),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("Discord-Display-Name-Auflösung via Broker fehlgeschlagen: {e}");
                None
            }
        }
    }

    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str) {
        let Some(ref relay) = self.relay else {
            tracing::warn!("Streamer-Rollen-Sync übersprungen: kein BrokerRelay konfiguriert");
            return;
        };
        let Ok(user_id) = discord_user_id.parse::<u64>() else {
            tracing::warn!(
                "Streamer-Rollen-Sync übersprungen: ungültige Discord-User-ID {discord_user_id}"
            );
            return;
        };
        let guild_ids = self.role_guild_ids(relay).await;
        if guild_ids.is_empty() {
            tracing::warn!("Streamer-Rollen-Sync übersprungen: keine Guild-Kandidaten verfügbar");
            return;
        }
        for guild_id in guild_ids {
            match relay
                .add_member_role(guild_id, user_id, self.role_id, reason)
                .await
            {
                Ok(()) => tracing::info!(
                    "Streamer role granted to {discord_user_id} in guild {guild_id}"
                ),
                Err(e) => tracing::warn!(
                    "Streamer-Rollen-Sync für {discord_user_id} in Guild {guild_id} fehlgeschlagen: {e}"
                ),
            }
        }
    }

    async fn revoke_streamer_role(&self, discord_user_id: &str, reason: &str) {
        let Some(ref relay) = self.relay else {
            tracing::warn!("Streamer-Rollen-Entzug übersprungen: kein BrokerRelay konfiguriert");
            return;
        };
        let Ok(user_id) = discord_user_id.parse::<u64>() else {
            tracing::warn!(
                "Streamer-Rollen-Entzug übersprungen: ungültige Discord-User-ID {discord_user_id}"
            );
            return;
        };
        let guild_ids = self.role_guild_ids(relay).await;
        if guild_ids.is_empty() {
            tracing::warn!("Streamer-Rollen-Entzug übersprungen: keine Guild-Kandidaten verfügbar");
            return;
        }
        // B10: Fehler NUR loggen, kein Hard-Fail.
        for guild_id in guild_ids {
            match relay
                .remove_member_role(guild_id, user_id, self.role_id, reason)
                .await
            {
                Ok(()) => tracing::info!(
                    "Streamer role removed from {discord_user_id} in guild {guild_id}"
                ),
                Err(e) => tracing::warn!(
                    "Streamer-Rollen-Entzug für {discord_user_id} in Guild {guild_id} fehlgeschlagen: {e}"
                ),
            }
        }
    }
}

/// Gleiche Broker-Mechanik für den internen-API-Pfad (`POST …/discord-profile`):
/// leitet auf die bestehende [`DiscordDirectoryPort`]-Impl weiter.
#[async_trait]
impl tb_internal_api::DiscordRolePort for BrokerDiscordDirectory {
    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str) {
        <Self as DiscordDirectoryPort>::grant_streamer_role(self, discord_user_id, reason).await
    }

    async fn revoke_streamer_role(&self, discord_user_id: &str, reason: &str) {
        <Self as DiscordDirectoryPort>::revoke_streamer_role(self, discord_user_id, reason).await
    }
}

// ---------------------------------------------------------------------------
// Moderator via Helix
// ---------------------------------------------------------------------------

pub struct HelixModeratorInstaller {
    helix: HelixClient,
}

impl HelixModeratorInstaller {
    pub fn new(helix: HelixClient) -> Self {
        Self { helix }
    }
}

#[async_trait]
impl ModeratorInstallPort for HelixModeratorInstaller {
    async fn add_channel_moderator(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        streamer_access_token: &str,
    ) -> Result<(), String> {
        match self
            .helix
            .add_channel_moderator(broadcaster_id, bot_user_id, streamer_access_token)
            .await
        {
            Ok(AddModeratorOutcome::Added) => {
                tracing::info!(
                    "Bot (ID: {bot_user_id}) is now moderator in channel {broadcaster_id}"
                );
                Ok(())
            }
            Ok(AddModeratorOutcome::AlreadyModerator) => {
                tracing::info!(
                    "Bot (ID: {bot_user_id}) is already moderator in channel {broadcaster_id}"
                );
                Ok(())
            }
            Ok(AddModeratorOutcome::BotBanned) => {
                let error = format!(
                    "Bot (ID: {bot_user_id}) is banned in channel {broadcaster_id}; moderator setup skipped"
                );
                tracing::warn!("{error}");
                Err(error)
            }
            Ok(AddModeratorOutcome::Failed { status, body }) => {
                let error = format!(
                    "Failed to add bot as moderator in channel {broadcaster_id}: HTTP {status}: {body}"
                );
                tracing::warn!("{error}");
                Err(error)
            }
            Err(e) => {
                let error =
                    format!("Error adding bot as moderator in channel {broadcaster_id}: {e}");
                tracing::error!("{error}");
                Err(error)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Chat-Begrüßung via Legacy-Python (Interim bis Chat-Cutover)
// ---------------------------------------------------------------------------

pub struct LegacyChatGreeter {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

/// No-Op Greeter für Umgebungen ohne `TWITCH_INTERNAL_API_TOKEN`.
/// Der Follow-up-Flow (Partner-Sync, Rollen, Mod-Setup) bleibt aktiv; nur
/// Chat-Nachrichten werden nicht zugestellt.
pub struct NoopChatGreeter;

#[async_trait]
impl ChatGreeterPort for NoopChatGreeter {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        _message: &str,
    ) -> Result<bool, String> {
        tracing::warn!(
            "Chat-Begrüßung übersprungen: Kein Chat-Greeter verfügbar für {twitch_login}"
        );
        Ok(false)
    }
}

impl LegacyChatGreeter {
    /// `base_url` = `TB_INTERNAL_API_LEGACY_FALLBACK_URL` (Python-Seitenport
    /// 8779), `token` = `TWITCH_INTERNAL_API_TOKEN` (gleicher Token wie die
    /// interne API selbst).
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("TWITCH_INTERNAL_API_TOKEN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())?;
        let base_url = std::env::var("TB_INTERNAL_API_LEGACY_FALLBACK_URL")
            .ok()
            .map(|v| v.trim().trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:8779".to_string());
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self {
            client,
            base_url,
            token,
        })
    }
}

/// Nativer Greeter (nach Chat-Cutover): sendet direkt über die tb-chat-API
/// mit dem Bot-Token — der Python-Umweg über 8779 entfällt.
pub struct NativeChatGreeter {
    api: Arc<dyn tb_chat::ChatApi>,
}

impl NativeChatGreeter {
    pub fn new(api: Arc<dyn tb_chat::ChatApi>) -> Self {
        Self { api }
    }
}

#[async_trait]
impl ChatGreeterPort for NativeChatGreeter {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        message: &str,
    ) -> Result<bool, String> {
        let Some(broadcaster_id) = self.api.resolve_user_id(twitch_login).await? else {
            return Err(format!("Login {twitch_login} nicht auflösbar"));
        };
        match self.api.send_message(&broadcaster_id, message).await? {
            tb_chat::SendOutcome::Sent => Ok(true),
            other => {
                tracing::warn!(login = %twitch_login, ?other, "Begrüßung nicht zugestellt");
                Ok(false)
            }
        }
    }
}

#[async_trait]
impl ChatGreeterPort for LegacyChatGreeter {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        message: &str,
    ) -> Result<bool, String> {
        let url = format!(
            "{}/internal/twitch/v1/streamers/{}/chat-action",
            self.base_url, twitch_login
        );
        let resp = self
            .client
            .post(&url)
            .header("X-Internal-Token", &self.token)
            .json(&serde_json::json!({ "message": message }))
            .send()
            .await
            .map_err(|e| format!("chat-action request failed: {e}"))?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = match resp.text().await {
                Ok(body) => body,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        status,
                        login = %twitch_login,
                        "Chat-Action-Fehlerbody nicht lesbar"
                    );
                    String::new()
                }
            };
            let snippet: String = body.chars().take(200).collect();
            return Err(format!("chat-action HTTP {status}: {snippet}"));
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("chat-action response invalid: {e}"))?;
        Ok(body
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false))
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Baut den `PartnerSetupService` aus Env + Transporten.
/// Der Service selbst wird selbst ohne Chat-Greeter erstellt; dann läuft die
/// Folge-Logik mit einem No-Op-Greeter weiter.
pub fn build_partner_setup_service(
    pool: PgPool,
    helix: HelixClient,
    relay: Option<BrokerRelay>,
    native_chat: Option<Arc<dyn tb_chat::ChatApi>>,
) -> Option<Arc<PartnerSetupService>> {
    // Nach dem Chat-Cutover begrüßt der native Bot direkt; ohne aktiven
    // Chat (TB_CHAT_ENABLED=0) bleibt der Legacy-Weg über Python 8779.
    let greeter: Arc<dyn ChatGreeterPort> = match native_chat {
        Some(api) => Arc::new(NativeChatGreeter::new(api)),
        None => LegacyChatGreeter::from_env()
            .map(|g| Arc::new(g) as Arc<dyn ChatGreeterPort>)
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Kein Chat-Greeter konfiguriert (TWITCH_INTERNAL_API_TOKEN fehlt); \
                     Chat-Begrüßung wird übersprungen"
                );
                Arc::new(NoopChatGreeter) as Arc<dyn ChatGreeterPort>
            }),
    };
    let bot_user_id = std::env::var("TWITCH_BOT_USER_ID")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if bot_user_id.is_none() {
        tracing::warn!(
            "TWITCH_BOT_USER_ID nicht gesetzt — OAuth-Followups laufen ohne Moderator-Setup/Begrüßung"
        );
    }
    Some(Arc::new(PartnerSetupService::new(
        pool,
        Arc::new(BrokerDiscordDirectory::from_env(relay)),
        Arc::new(HelixModeratorInstaller::new(helix)),
        greeter,
        bot_user_id,
    )))
}

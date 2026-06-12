//! `ChatApi` — der Port für alle ausgehenden Twitch-Aktionen des Chat-Bots
//! (Senden, Announcements, Bans, Unbans, Message-Delete, User-Lookups).
//!
//! Implementierung: `HelixChatClient` (Helix-Endpoints aus
//! `tb-transport-twitch` + `BotTokenManager` mit dem Python-2-Attempt-Muster:
//! 401 → `force_refresh()` → einmal wiederholen). Die Module (Moderation,
//! Promos, Commands, Scam-Warnung) hängen nur am Trait — testbar mit Mocks.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::types::SendOutcome;

/// Ban-/Timeout-Ergebnis: kanonisch im Transport definiert.
pub use tb_transport_twitch::BanOutcome;

/// Port für ausgehende Chat-/Moderations-Aktionen mit dem Bot-Token.
#[async_trait]
pub trait ChatApi: Send + Sync {
    /// `POST /helix/chat/messages` — HTTP 200 kann trotzdem Drop sein
    /// (`is_sent=false` + `drop_reason`), siehe `SendOutcome`.
    async fn send_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String>;

    /// `POST /helix/chat/announcements` (Farbe: blue/green/orange/purple/primary).
    /// Python fällt bei Fehlern auf `send_message` zurück — das macht der
    /// Aufrufer, nicht diese Methode.
    async fn send_announcement(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<bool, String>;

    /// `POST /helix/moderation/bans` ohne Dauer (permanenter Ban).
    async fn ban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        reason: &str,
    ) -> Result<BanOutcome, String>;

    /// `POST /helix/moderation/bans` mit `duration` (Timeout).
    async fn timeout_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        duration_secs: u32,
        reason: &str,
    ) -> Result<BanOutcome, String>;

    /// `DELETE /helix/moderation/bans`.
    async fn unban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
    ) -> Result<bool, String>;

    /// `DELETE /helix/moderation/chat` — einzelne Nachricht löschen.
    async fn delete_message(
        &self,
        broadcaster_id: &str,
        message_id: &str,
    ) -> Result<bool, String>;

    /// `GET /helix/users?id=` → `created_at` (Account-Alter für Spam-/
    /// Scam-Eskalatoren). None = User nicht gefunden.
    async fn user_created_at(
        &self,
        user_id: &str,
    ) -> Result<Option<DateTime<Utc>>, String>;

    /// `GET /helix/users?login=` → user_id.
    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String>;

    /// Bot-User-ID (sender_id/moderator_id für alle Aktionen).
    async fn bot_user_id(&self) -> String;
}

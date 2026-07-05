//! DiscordBackend-Trait und Payload-Typen für Discord-Rich-Messages über den Master-Broker.

use serde::{Deserialize, Serialize};

/// Payload für `/internal/master/v1/discord/send-rich-message`.
#[derive(Debug, Clone, Serialize)]
pub struct SendRichMessage {
    pub channel_id: i64,
    pub content: Option<String>,
    pub embed: serde_json::Value,
    /// Fertiger Discord-Components-V2-Array (top-level), ohne Button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<serde_json::Value>,
    pub allowed_role_ids: Vec<i64>,
    pub view_spec: Option<serde_json::Value>,
}

/// Payload für `/internal/master/v1/discord/edit-rich-message`.
#[derive(Debug, Clone, Serialize)]
pub struct EditRichMessage {
    pub channel_id: i64,
    pub message_id: String,
    pub content: Option<String>,
    pub embed: serde_json::Value,
    /// Fertiger Discord-Components-V2-Array (top-level), ohne Button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<serde_json::Value>,
    pub view_spec: Option<serde_json::Value>,
}

/// Payload für `/internal/master/v1/discord/send-dm` (Token-Lifecycle-DMs).
/// Der Broker-Endpunkt nimmt ausschließlich `user_id` + Text-`content`
/// entgegen (kein Embed) und öffnet selbst den DM-Channel.
#[derive(Debug, Clone, Serialize)]
pub struct SendUserDm {
    pub user_id: u64,
    pub content: String,
}

/// Payload für eine Embed-Nachricht in den Alert-Channel. Nutzt denselben
/// Broker-Endpunkt wie [`SendRichMessage`] (`send-rich-message`), wird aber als
/// eigene, abgegrenzte Aktion modelliert (kein `view_spec`, keine
/// Rollen-Mentions außer einer optionalen Allowlist).
#[derive(Debug, Clone, Serialize)]
pub struct SendAlertEmbed {
    pub channel_id: i64,
    pub content: Option<String>,
    pub embed: serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_role_ids: Vec<i64>,
}

/// Antwort des Brokers auf `send-rich-message`.
#[derive(Debug, Deserialize)]
pub struct SendResult {
    pub ok: bool,
    pub result: SendResultInner,
}

/// Inneres Ergebnis-Objekt der Broker-Antwort.
#[derive(Debug, Deserialize)]
pub struct SendResultInner {
    /// Discord-message_id. Der Broker liefert sie je nach Implementierung als
    /// String (Python-master_broker) oder als u64-Zahl (Rust-dl-broker); beide
    /// Formen werden akzeptiert, damit ein Cross-Repo-Drift die Live-Pings nicht
    /// still bricht (sonst: Decode-Fehler → kein gespeichertes message_id →
    /// Doppel-Ping bei jedem Poll).
    #[serde(deserialize_with = "deserialize_id_as_string")]
    pub message_id: String,
}

/// Decodiert eine Discord-Snowflake-ID als String, egal ob das JSON sie als
/// String oder als (vorzeichenlose) Ganzzahl liefert.
fn deserialize_id_as_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "message_id muss String oder Zahl sein, war {other}"
        ))),
    }
}

/// Einheitlicher Fehlertyp für Discord-Transport.
#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Broker antwortete mit Status {status}: {body}")]
    BrokerError { status: u16, body: String },
    #[error("Antwort-Deserialisierung fehlgeschlagen: {0}")]
    Deserialize(#[from] serde_json::Error),
}

/// Abstraktes Discord-Backend — ermöglicht Test-Stubs ohne Netz.
#[async_trait::async_trait]
pub trait DiscordBackend: Send + Sync {
    /// Sendet eine Rich-Message in einen Discord-Kanal.
    async fn send_rich_message(&self, payload: SendRichMessage)
        -> Result<SendResult, DiscordError>;

    /// Bearbeitet eine bestehende Rich-Message.
    async fn edit_rich_message(&self, payload: EditRichMessage) -> Result<(), DiscordError>;

    /// Sendet dem Streamer eine Direktnachricht (Token-Lifecycle-DMs). Der
    /// Broker öffnet den DM-Channel selbst und liefert die `message_id`.
    async fn send_user_dm(&self, payload: SendUserDm) -> Result<SendResult, DiscordError>;

    /// Sendet ein Embed in den Alert-Channel und liefert die `message_id`.
    async fn send_alert_embed(&self, payload: SendAlertEmbed) -> Result<SendResult, DiscordError>;

    /// Entfernt eine Rolle von einem Guild-Mitglied
    /// (`POST /discord/member/remove-role`).
    async fn remove_member_role(
        &self,
        guild_id: u64,
        user_id: u64,
        role_id: u64,
        reason: &str,
    ) -> Result<(), DiscordError>;
}

//! DiscordBackend-Trait und Payload-Typen für Discord-Rich-Messages über den Master-Broker.

use serde::{Deserialize, Serialize};

/// Payload für `/internal/master/v1/discord/send-rich-message`.
#[derive(Debug, Clone, Serialize)]
pub struct SendRichMessage {
    pub channel_id: i64,
    pub content: Option<String>,
    pub embed: serde_json::Value,
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
    pub view_spec: Option<serde_json::Value>,
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
    pub message_id: String,
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
}

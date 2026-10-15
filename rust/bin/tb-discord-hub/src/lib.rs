pub mod config;
pub mod forward;
pub mod rest;
pub mod server;

use axum::body::Bytes;
use axum::http::Method;

pub const GUILD_HEADER: &str = "X-Discord-Guild-Id";
pub const TOKEN_HEADER: &str = "X-Internal-Token";
pub const IDEMPOTENCY_HEADER: &str = "X-Idempotency-Key";
pub const INTERNAL_PREFIX: &str = "/internal/master/v1/";

#[derive(Debug, Clone)]
pub struct InboundRequest {
    pub method: Method,
    pub path: String,
    pub query: Option<String>,
    pub idempotency_key: Option<String>,
    pub content_type: Option<String>,
    pub body: Bytes,
}

#[derive(Debug, Clone)]
pub struct OutboundResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Bytes,
}

impl OutboundResponse {
    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        Self {
            status,
            content_type: Some("application/json".to_string()),
            body: Bytes::from(value.to_string()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("ungültige Anfrage: {0}")]
    BadRequest(String),
    #[error("nicht gefunden: {0}")]
    NotFound(String),
    #[error("für fremde Server nicht unterstützt: {0}")]
    Unsupported(String),
    #[error("Upstream-Fehler {status}: {body}")]
    Upstream { status: u16, body: String },
    #[error("Upstream nicht erreichbar: {0}")]
    Unreachable(String),
    #[error("interner Fehler: {0}")]
    Internal(String),
}

impl HubError {
    pub fn status(&self) -> u16 {
        match self {
            HubError::BadRequest(_) => 400,
            HubError::NotFound(_) => 404,
            HubError::Unsupported(_) => 501,
            HubError::Upstream { status, .. } => *status,
            HubError::Unreachable(_) => 502,
            HubError::Internal(_) => 500,
        }
    }
}

#[async_trait::async_trait]
pub trait BrokerForward: Send + Sync {
    async fn forward(&self, req: InboundRequest) -> Result<OutboundResponse, HubError>;
}

#[async_trait::async_trait]
pub trait DirectDiscord: Send + Sync {
    async fn handle(
        &self,
        guild_id: u64,
        req: InboundRequest,
    ) -> Result<OutboundResponse, HubError>;
}

pub mod gateway;
pub mod guilds;

#[async_trait::async_trait]
pub trait GuildEventSink: Send + Sync {
    async fn guild_added(&self, guild_id: u64, name: &str);
    async fn guild_removed(&self, guild_id: u64);
    async fn member_left(&self, guild_id: u64, user_id: u64);
}

//! Discord-Hub: einziger interner Discord-Integrationspunkt des Twitch-Bots.
//!
//! Bietet dieselben internen Endpunkte wie der Master-Broker
//! (`/internal/master/v1/...`). Jede Anfrage trägt zusätzlich die Guild-ID im
//! Header [`GUILD_HEADER`]. Weiche:
//! - Guild = deutscher Community-Server oder kein Header → unverändert an den
//!   Broker des Haupt-Bots weiterreichen ([`BrokerForward`]).
//! - Jede andere Guild → Discord-REST direkt mit dem eigenen Bot-Token
//!   ([`DirectDiscord`]).
//!
//! Zusätzlich hält der Hub eine Gateway-Verbindung für `/setup`,
//! Bot hinzugefügt/entfernt und Mitglied verlassen ([`gateway`]).

pub mod config;
pub mod forward;
pub mod gateway;
pub mod rest;
pub mod server;

use axum::body::Bytes;
use axum::http::Method;

/// Header, in dem der Twitch-Bot die Ziel-Guild mitschickt.
pub const GUILD_HEADER: &str = "X-Discord-Guild-Id";
/// Header mit dem internen Token (identisch zum Broker).
pub const TOKEN_HEADER: &str = "X-Internal-Token";
/// Header mit dem Idempotenz-Schlüssel (identisch zum Broker).
pub const IDEMPOTENCY_HEADER: &str = "X-Idempotency-Key";
/// Gemeinsamer Pfad-Präfix aller internen Endpunkte.
pub const INTERNAL_PREFIX: &str = "/internal/master/v1/";

/// Eine bereits authentifizierte, eingehende Anfrage an den Hub.
#[derive(Debug, Clone)]
pub struct InboundRequest {
    pub method: Method,
    /// Pfad ohne Query, beginnt mit [`INTERNAL_PREFIX`].
    pub path: String,
    /// Roh-Query ohne führendes `?`.
    pub query: Option<String>,
    pub idempotency_key: Option<String>,
    pub content_type: Option<String>,
    pub body: Bytes,
}

/// Antwort, die der Hub an den Twitch-Bot zurückgibt.
#[derive(Debug, Clone)]
pub struct OutboundResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Bytes,
}

impl OutboundResponse {
    /// JSON-Antwort mit Status.
    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        Self {
            status,
            content_type: Some("application/json".to_string()),
            body: Bytes::from(value.to_string()),
        }
    }
}

/// Fehler innerhalb des Hubs. `server` bildet sie auf HTTP-Status ab.
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    /// 400
    #[error("ungültige Anfrage: {0}")]
    BadRequest(String),
    /// 404
    #[error("nicht gefunden: {0}")]
    NotFound(String),
    /// 501 – Endpunkt für fremde Guilds nicht vorgesehen.
    #[error("für fremde Server nicht unterstützt: {0}")]
    Unsupported(String),
    /// Upstream (Broker oder Discord) hat mit einem Fehlerstatus geantwortet.
    #[error("Upstream-Fehler {status}: {body}")]
    Upstream { status: u16, body: String },
    /// Netzwerk/Timeout zum Upstream → 502
    #[error("Upstream nicht erreichbar: {0}")]
    Unreachable(String),
    /// 500
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

/// Weiterleitung an den Broker des Haupt-Bots (deutscher Server).
/// Muss Methode, Pfad, Query, Body, Content-Type und Idempotenz-Schlüssel
/// unverändert durchreichen und die Broker-Antwort (Status + Body) 1:1
/// zurückgeben.
#[async_trait::async_trait]
pub trait BrokerForward: Send + Sync {
    async fn forward(&self, req: InboundRequest) -> Result<OutboundResponse, HubError>;
}

/// Direkte Ausführung über Discord-REST für fremde Guilds. Request- und
/// Response-Formate entsprechen den Broker-Endpunkten.
#[async_trait::async_trait]
pub trait DirectDiscord: Send + Sync {
    async fn handle(
        &self,
        guild_id: u64,
        req: InboundRequest,
    ) -> Result<OutboundResponse, HubError>;
}

/// Ereignisse aus dem Gateway. Persistenz folgt mit Paket G (`discord_guilds`,
/// `streamer_guilds`); bis dahin protokolliert die Standard-Senke nur.
#[async_trait::async_trait]
pub trait GuildEventSink: Send + Sync {
    async fn guild_added(&self, guild_id: u64, name: &str);
    async fn guild_removed(&self, guild_id: u64);
    async fn member_left(&self, guild_id: u64, user_id: u64);
}

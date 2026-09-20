//! Gemeinsames Schema der nicht geheimen Betriebskonfiguration.
//! Zugangsdaten sind absichtlich kein Bestandteil dieses Schemas.

use crate::{
    file::{FileError, Schema, Snapshot},
    BrokerConfig, DbConfig, InternalApiConfig, Settings,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

pub const SCHEMA_VERSION: u32 = 1;
pub type BotConfigSnapshot = Snapshot<BotConfig>;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BotConfig {
    pub schema_version: u32,
    pub twitch: Twitch,
    #[serde(default)]
    pub database: Database,
    #[serde(default)]
    pub internal_api: InternalApi,
    #[serde(default)]
    pub dashboard: Dashboard,
    #[serde(default)]
    pub broker: Broker,
    #[serde(default)]
    pub logging: Logging,
}

impl fmt::Debug for BotConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BotConfig([Werte nicht ausgegeben])")
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Twitch {
    /// Öffentliche Bot-ID, kein Token. Bewusst ohne erfundenen Ersatzwert.
    pub bot_user_id: String,
    /// Öffentliche Discord-Ziel-ID für bestehende Stream-Ankündigungen.
    pub notify_channel_id: String,
    pub eventsub_callback_url: String,
    #[serde(default = "default_game")]
    pub target_game: String,
    #[serde(default = "default_languages")]
    pub language_filters: Vec<String>,
}
fn default_game() -> String {
    "Deadlock".to_string()
}
fn default_languages() -> Vec<String> {
    ["de", "de-de", "de-at", "de-ch"]
        .map(str::to_string)
        .to_vec()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Database {
    pub pool_max: u32,
    /// Millisekunden, einschließlich der bisher erlaubten Bruchteile einer Sekunde.
    pub acquire_timeout_ms: u64,
    pub connect_timeout_seconds: u64,
}
impl Default for Database {
    fn default() -> Self {
        Self {
            pool_max: 10,
            acquire_timeout_ms: 5_000,
            connect_timeout_seconds: 5,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct InternalApi {
    pub host: IpAddr,
    pub port: u16,
}
impl Default for InternalApi {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8776,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dashboard {
    pub host: IpAddr,
    pub port: u16,
}
impl Default for Dashboard {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8769,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Broker {
    pub base_url: String,
}
impl Default for Broker {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8770".into(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Logging {
    /// Feste Stufen statt eines ungeprüften ENV-Filterausdrucks.
    pub level: LogLevel,
}
impl Default for Logging {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}
impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

pub(crate) fn range(
    value: u64,
    minimum: u64,
    maximum: u64,
    field: &'static str,
) -> Result<(), FileError> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(FileError::invalid(field))
    }
}

pub(crate) fn positive_id(value: &str, field: &'static str) -> Result<(), FileError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FileError::invalid(field));
    }
    let number = value
        .parse::<u64>()
        .map_err(|_| FileError::invalid(field))?;
    // Discord-Verbraucher verwenden BIGINT; kein Überlauf beim Weiterreichen.
    range(number, 1, i64::MAX as u64, field)
}

pub(crate) fn public_url(
    value: &str,
    field: &'static str,
    local_only: bool,
) -> Result<(), FileError> {
    let url = url::Url::parse(value).map_err(|_| FileError::invalid(field))?;
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host().is_none()
        || url.port() == Some(0)
        || !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || (local_only && !local)
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(FileError::invalid(field));
    }
    Ok(())
}

impl Schema for BotConfig {
    fn validate(&self) -> Result<(), FileError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(FileError::invalid("schema_version"));
        }
        positive_id(&self.twitch.bot_user_id, "twitch.bot_user_id")?;
        positive_id(&self.twitch.notify_channel_id, "twitch.notify_channel_id")?;
        public_url(
            &self.twitch.eventsub_callback_url,
            "twitch.eventsub_callback_url",
            false,
        )?;
        if self.twitch.target_game.trim().is_empty()
            || self.twitch.target_game.len() > 128
            || self.twitch.target_game.chars().any(char::is_control)
        {
            return Err(FileError::invalid("twitch.target_game"));
        }
        if self.twitch.language_filters.is_empty()
            || self.twitch.language_filters.len() > 16
            || self.twitch.language_filters.iter().any(|s| {
                s.is_empty()
                    || s.len() > 16
                    || !s.bytes().all(|c| c.is_ascii_lowercase() || c == b'-')
            })
        {
            return Err(FileError::invalid("twitch.language_filters"));
        }
        range(
            u64::from(self.database.pool_max),
            1,
            1_000,
            "database.pool_max",
        )?;
        range(
            self.database.acquire_timeout_ms,
            100,
            300_000,
            "database.acquire_timeout_ms",
        )?;
        range(
            self.database.connect_timeout_seconds,
            1,
            300,
            "database.connect_timeout_seconds",
        )?;
        if !self.internal_api.host.is_loopback() {
            return Err(FileError::invalid("internal_api.host"));
        }
        range(
            u64::from(self.internal_api.port),
            1,
            65_535,
            "internal_api.port",
        )?;
        if !self.dashboard.host.is_loopback() {
            return Err(FileError::invalid("dashboard.host"));
        }
        range(u64::from(self.dashboard.port), 1, 65_535, "dashboard.port")?;
        if self.dashboard.port == self.internal_api.port
            && self.dashboard.host == self.internal_api.host
        {
            return Err(FileError::invalid("dashboard.port"));
        }
        public_url(&self.broker.base_url, "broker.base_url", true)?;
        Ok(())
    }
}

impl BotConfigSnapshot {
    /// Bestehende Verbraucher-Typen bleiben erhalten. Der Getter wird NUR nach
    /// bekannten Zugangsdaten gefragt, nie nach einem Betriebswert.
    pub fn runtime_settings(
        &self,
        secret: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Settings, tb_error::ConfigError> {
        fn nonempty(value: Option<String>) -> Option<String> {
            value
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        }
        let required = |key: &str| {
            nonempty(secret(key))
                .ok_or_else(|| tb_error::ConfigError::MissingRequired(key.to_string()))
        };
        let internal_token = required("TWITCH_INTERNAL_API_TOKEN")?;
        let broker_token = nonempty(secret("MASTER_BROKER_TOKEN"))
            .or_else(|| nonempty(secret("MAIN_BOT_INTERNAL_TOKEN")))
            .unwrap_or_else(|| internal_token.clone());
        let config = self.settings();
        Ok(Settings {
            db: DbConfig {
                dsn: required("TWITCH_ANALYTICS_DSN")?,
                pool_max: config.database.pool_max,
                acquire_timeout: Duration::from_millis(config.database.acquire_timeout_ms),
                connect_timeout: Duration::from_secs(config.database.connect_timeout_seconds),
            },
            internal_api: InternalApiConfig {
                token: internal_token,
                host: config.internal_api.host.to_string(),
                port: config.internal_api.port,
            },
            broker: BrokerConfig {
                base_url: config.broker.base_url.clone(),
                token: broker_token,
            },
        })
    }
}

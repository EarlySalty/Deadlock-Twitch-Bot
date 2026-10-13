//! Typisierte Settings aus einer Env-Quelle. Kein globaler Mutable-State.
//!
//! Der Loader nimmt eine Quelle `Fn(&str) -> Option<String>` entgegen, damit er
//! ohne Prozess-Env testbar ist. `from_env()` nutzt `std::env::var`.

pub mod affiliate_options;
pub mod category_archive;
pub mod challenges;
pub mod dashboard_options;
pub mod discord;
pub mod editor;
pub mod file;
pub mod global;
pub mod operations;
pub mod reliability;
pub mod runtime;
pub mod shared_options;
pub mod stt;
pub mod vod_archive;

pub use global::{BotConfig, BotConfigSnapshot};

use std::time::Duration;

use tb_error::ConfigError;

type Get<'a> = dyn Fn(&str) -> Option<String> + 'a;

fn non_empty(v: String) -> Option<String> {
    let t = v.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn required(get: &Get, name: &str) -> Result<String, ConfigError> {
    get(name)
        .and_then(non_empty)
        .ok_or_else(|| ConfigError::MissingRequired(name.to_string()))
}

fn or_default(get: &Get, name: &str, default: &str) -> String {
    get(name)
        .and_then(non_empty)
        .unwrap_or_else(|| default.to_string())
}

fn parse_or<T>(get: &Get, name: &str, default: T) -> T
where
    T: std::str::FromStr + Copy + std::fmt::Display,
{
    match get(name).and_then(non_empty) {
        Some(v) => match v.parse::<T>() {
            Ok(parsed) => parsed,
            Err(_) => {
                tracing::warn!(
                    setting = name,
                    value = %v,
                    default = %default,
                    "Ungültiger optionaler Config-Wert; Default wird verwendet"
                );
                default
            }
        },
        None => default,
    }
}

fn parse_or_min<T>(get: &Get, name: &str, default: T, min: T) -> T
where
    T: std::str::FromStr + Copy + PartialOrd + std::fmt::Display,
{
    let parsed = parse_or(get, name, default);
    if parsed < min {
        tracing::warn!(
            setting = name,
            value = %parsed,
            minimum = %min,
            "Optionaler Config-Wert unter Minimum; Minimum wird verwendet"
        );
        min
    } else {
        parsed
    }
}

fn parse_f64_or_min(get: &Get, name: &str, default: f64, min: f64) -> f64 {
    let parsed = parse_or(get, name, default);
    if !parsed.is_finite() {
        tracing::warn!(
            setting = name,
            value = %parsed,
            default = %default,
            "Ungültiger optionaler Config-Wert; Default wird verwendet"
        );
        return default;
    }
    if parsed < min {
        tracing::warn!(
            setting = name,
            value = %parsed,
            minimum = %min,
            "Optionaler Config-Wert unter Minimum; Minimum wird verwendet"
        );
        min
    } else {
        parsed
    }
}

/// PostgreSQL/TimescaleDB-Verbindung. Defaults wie der Python-Pool.
#[derive(Clone)]
pub struct DbConfig {
    pub dsn: String,
    pub pool_max: u32,
    pub acquire_timeout: Duration,
    pub connect_timeout: Duration,
}

impl DbConfig {
    fn load(get: &Get) -> Result<Self, ConfigError> {
        Ok(Self {
            dsn: required(get, "TWITCH_ANALYTICS_DSN")?,
            pool_max: parse_or_min(get, "TWITCH_ANALYTICS_POOL_MAXSIZE", 10u32, 1u32),
            acquire_timeout: Duration::from_secs_f64(parse_f64_or_min(
                get,
                "TWITCH_ANALYTICS_POOL_TIMEOUT_SECONDS",
                5.0f64,
                0.1f64,
            )),
            connect_timeout: Duration::from_secs(parse_or_min(
                get,
                "TWITCH_ANALYTICS_CONNECT_TIMEOUT_SECONDS",
                5u64,
                1u64,
            )),
        })
    }
}

/// Interne API (Loopback, Port 8776). Token ist Pflicht (fail-closed).
#[derive(Clone)]
pub struct InternalApiConfig {
    pub token: String,
    pub host: String,
    pub port: u16,
}

impl InternalApiConfig {
    fn load(get: &Get) -> Result<Self, ConfigError> {
        Ok(Self {
            token: required(get, "TWITCH_INTERNAL_API_TOKEN")?,
            host: or_default(get, "TWITCH_INTERNAL_API_HOST", "127.0.0.1"),
            port: parse_or_min(get, "TWITCH_INTERNAL_API_PORT", 8776u16, 1u16),
        })
    }
}

/// Master-Broker (Discord-Bridge, Loopback, Port 8770). Token-Fallback auf das interne API-Token.
#[derive(Clone)]
pub struct BrokerConfig {
    pub base_url: String,
    pub token: String,
}

impl BrokerConfig {
    fn load(get: &Get, internal_token: &str) -> Result<Self, ConfigError> {
        let base_url = match get("MASTER_BROKER_BASE_URL").and_then(non_empty) {
            Some(u) => u,
            None => {
                let host = or_default(get, "MASTER_BROKER_HOST", "127.0.0.1");
                let port = parse_or_min(get, "MASTER_BROKER_PORT", 8770u16, 1u16);
                format!("http://{host}:{port}")
            }
        };
        // Fallback-Kette: MASTER_BROKER_TOKEN → MAIN_BOT_INTERNAL_TOKEN → TWITCH_INTERNAL_API_TOKEN
        let token = get("MASTER_BROKER_TOKEN")
            .and_then(non_empty)
            .or_else(|| get("MAIN_BOT_INTERNAL_TOKEN").and_then(non_empty))
            .unwrap_or_else(|| internal_token.to_string());
        let broker = Self { base_url, token };
        // Optionale Umschaltung auf den Discord-Hub (Paket H). Ohne
        // DISCORD_HUB_BASE_URL bleibt alles beim Broker wie bisher.
        let hub_base_url = get(DISCORD_HUB_BASE_URL).and_then(non_empty);
        let hub_token = || get(DISCORD_HUB_INTERNAL_TOKEN).and_then(non_empty);
        Ok(broker.with_discord_hub(hub_base_url, hub_token))
    }

    /// Lenkt die Konfiguration auf den Discord-Hub um, falls eine Hub-Adresse
    /// gesetzt ist. Der Hub bietet dieselben Endpunkte wie der Broker
    /// (`/internal/master/v1/...`) und reicht Anfragen ohne Guild bzw. für die
    /// Community-Guild unverändert an den Broker weiter.
    ///
    /// Token: eigenes Hub-Token, sonst das bereits aufgelöste Broker-Token.
    /// Das Hub-Token wird nur abgefragt, wenn eine Hub-Adresse gesetzt ist.
    /// Ohne Hub-Adresse bleibt die Konfiguration unverändert.
    pub fn with_discord_hub(
        self,
        hub_base_url: Option<String>,
        hub_token: impl FnOnce() -> Option<String>,
    ) -> Self {
        match hub_base_url {
            Some(base_url) => Self {
                base_url,
                token: hub_token().unwrap_or(self.token),
            },
            None => self,
        }
    }
}

/// Basis-URL des Discord-Hubs; gesetzt ⇒ Twitch-Bot spricht den Hub statt des Brokers an.
pub const DISCORD_HUB_BASE_URL: &str = "DISCORD_HUB_BASE_URL";
/// Optionales eigenes Token für den Hub; Fallback ist das Broker-Token.
pub const DISCORD_HUB_INTERNAL_TOKEN: &str = "DISCORD_HUB_INTERNAL_TOKEN";

/// Gesamte Settings des Bot-Prozesses (für Phase 0b: DB + interne API + Broker).
#[derive(Debug, Clone)]
pub struct Settings {
    pub db: DbConfig,
    pub internal_api: InternalApiConfig,
    pub broker: BrokerConfig,
}

// Auch alte Aufrufer dürfen Zugangsdaten nicht über abgeleitetes Debug ausgeben.
impl std::fmt::Debug for DbConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DbConfig([geschützt])")
    }
}
impl std::fmt::Debug for InternalApiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InternalApiConfig([geschützt])")
    }
}
impl std::fmt::Debug for BrokerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrokerConfig([geschützt])")
    }
}

impl Settings {
    /// Aus der Prozess-Umgebung (Infisical-Wrapper hat sie zuvor injiziert).
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(&|k| std::env::var(k).ok())
    }

    /// Aus einer beliebigen Quelle (Tests).
    pub fn load(get: &Get) -> Result<Self, ConfigError> {
        let internal_api = InternalApiConfig::load(get)?;
        let broker = BrokerConfig::load(get, &internal_api.token)?;
        let db = DbConfig::load(get)?;
        Ok(Self {
            db,
            internal_api,
            broker,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn src(map: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| map.get(k).map(|v| v.to_string())
    }

    fn minimal() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("TWITCH_ANALYTICS_DSN", "postgres://u:p@127.0.0.1:5433/db"),
            ("TWITCH_INTERNAL_API_TOKEN", "tok-123"),
        ])
    }

    #[test]
    fn defaults_apply_when_optional_absent() {
        let s = Settings::load(&src(minimal())).unwrap();
        assert_eq!(s.db.pool_max, 10);
        assert_eq!(s.db.connect_timeout.as_secs(), 5);
        assert_eq!(s.internal_api.host, "127.0.0.1");
        assert_eq!(s.internal_api.port, 8776);
        assert_eq!(s.broker.base_url, "http://127.0.0.1:8770");
        // Broker-Token fällt auf das interne Token zurück
        assert_eq!(s.broker.token, "tok-123");
    }

    #[test]
    fn overrides_are_parsed() {
        let mut m = minimal();
        m.insert("TWITCH_ANALYTICS_POOL_MAXSIZE", "25");
        m.insert("TWITCH_ANALYTICS_CONNECT_TIMEOUT_SECONDS", "9");
        m.insert("MASTER_BROKER_BASE_URL", "http://127.0.0.1:9999");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.db.pool_max, 25);
        assert_eq!(s.db.connect_timeout.as_secs(), 9);
        assert_eq!(s.broker.base_url, "http://127.0.0.1:9999");
    }

    #[test]
    fn missing_required_dsn_errors() {
        let m = HashMap::from([("TWITCH_INTERNAL_API_TOKEN", "t")]);
        let err = Settings::load(&src(m)).unwrap_err();
        assert!(matches!(err, ConfigError::MissingRequired(n) if n == "TWITCH_ANALYTICS_DSN"));
    }

    #[test]
    fn invalid_optional_int_defaults() {
        let mut m = minimal();
        m.insert("TWITCH_ANALYTICS_POOL_MAXSIZE", "abc");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.db.pool_max, 10);
    }

    #[test]
    fn optional_minimums_are_clamped() {
        let mut m = minimal();
        m.insert("TWITCH_ANALYTICS_POOL_MAXSIZE", "0");
        m.insert("TWITCH_ANALYTICS_POOL_TIMEOUT_SECONDS", "-1");
        m.insert("TWITCH_INTERNAL_API_PORT", "0");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.db.pool_max, 1);
        assert_eq!(s.db.acquire_timeout, Duration::from_millis(100));
        assert_eq!(s.internal_api.port, 1);
    }

    #[test]
    fn broker_token_first_priority_master_broker_token() {
        let mut m = minimal();
        m.insert("MASTER_BROKER_TOKEN", "master-tok");
        m.insert("MAIN_BOT_INTERNAL_TOKEN", "main-tok");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.token, "master-tok");
    }

    #[test]
    fn broker_token_second_priority_main_bot_internal_token() {
        let mut m = minimal();
        m.insert("MAIN_BOT_INTERNAL_TOKEN", "main-tok");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.token, "main-tok");
    }

    #[test]
    fn broker_token_fallback_to_internal_api_token() {
        // Kein MASTER_BROKER_TOKEN, kein MAIN_BOT_INTERNAL_TOKEN
        // → fällt auf TWITCH_INTERNAL_API_TOKEN zurück
        let s = Settings::load(&src(minimal())).unwrap();
        assert_eq!(s.broker.token, "tok-123");
    }

    #[test]
    fn ohne_hub_variable_bleibt_der_broker_ziel() {
        let mut m = minimal();
        m.insert("MASTER_BROKER_TOKEN", "master-tok");
        // Ein Hub-Token allein schaltet nicht um.
        m.insert("DISCORD_HUB_INTERNAL_TOKEN", "hub-tok");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.base_url, "http://127.0.0.1:8770");
        assert_eq!(s.broker.token, "master-tok");
    }

    #[test]
    fn leere_hub_variable_schaltet_nicht_um() {
        let mut m = minimal();
        m.insert("DISCORD_HUB_BASE_URL", "   ");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.base_url, "http://127.0.0.1:8770");
        assert_eq!(s.broker.token, "tok-123");
    }

    #[test]
    fn hub_variable_ersetzt_broker_adresse_und_nutzt_hub_token() {
        let mut m = minimal();
        m.insert("MASTER_BROKER_BASE_URL", "http://127.0.0.1:9999");
        m.insert("MASTER_BROKER_TOKEN", "master-tok");
        m.insert("DISCORD_HUB_BASE_URL", "http://127.0.0.1:8790");
        m.insert("DISCORD_HUB_INTERNAL_TOKEN", "hub-tok");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.base_url, "http://127.0.0.1:8790");
        assert_eq!(s.broker.token, "hub-tok");
    }

    #[test]
    fn hub_ohne_eigenes_token_faellt_auf_broker_token_kette_zurueck() {
        let mut m = minimal();
        m.insert("DISCORD_HUB_BASE_URL", "http://127.0.0.1:8790");
        m.insert("MAIN_BOT_INTERNAL_TOKEN", "main-tok");
        m.insert("DISCORD_HUB_INTERNAL_TOKEN", "");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.base_url, "http://127.0.0.1:8790");
        assert_eq!(s.broker.token, "main-tok");

        let mut m = minimal();
        m.insert("DISCORD_HUB_BASE_URL", "http://127.0.0.1:8790");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.token, "tok-123");
    }
}

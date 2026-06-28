//! Typisierte Settings aus einer Env-Quelle. Kein globaler Mutable-State.
//!
//! Der Loader nimmt eine Quelle `Fn(&str) -> Option<String>` entgegen, damit er
//! ohne Prozess-Env testbar ist. `from_env()` nutzt `std::env::var`.

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

fn parse_or<T: std::str::FromStr>(get: &Get, name: &str, default: T) -> Result<T, ConfigError> {
    match get(name).and_then(non_empty) {
        Some(v) => v
            .parse::<T>()
            .map_err(|_| ConfigError::Invalid(name.to_string())),
        None => Ok(default),
    }
}

/// PostgreSQL/TimescaleDB-Verbindung. Defaults wie der Python-Pool.
#[derive(Debug, Clone)]
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
            pool_max: parse_or(get, "TWITCH_ANALYTICS_POOL_MAXSIZE", 10u32)?,
            acquire_timeout: Duration::from_secs_f64(parse_or(
                get,
                "TWITCH_ANALYTICS_POOL_TIMEOUT_SECONDS",
                5.0f64,
            )?),
            connect_timeout: Duration::from_secs(parse_or(
                get,
                "TWITCH_ANALYTICS_CONNECT_TIMEOUT_SECONDS",
                5u64,
            )?),
        })
    }
}

/// Interne API (Loopback, Port 8776). Token ist Pflicht (fail-closed).
#[derive(Debug, Clone)]
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
            port: parse_or(get, "TWITCH_INTERNAL_API_PORT", 8776u16)?,
        })
    }
}

/// Master-Broker (Discord-Bridge, Loopback, Port 8770). Token-Fallback auf das interne API-Token.
#[derive(Debug, Clone)]
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
                let port = parse_or(get, "MASTER_BROKER_PORT", 8770u16)?;
                format!("http://{host}:{port}")
            }
        };
        // Fallback-Kette: MASTER_BROKER_TOKEN → MAIN_BOT_INTERNAL_TOKEN → TWITCH_INTERNAL_API_TOKEN
        let token = get("MASTER_BROKER_TOKEN")
            .and_then(non_empty)
            .or_else(|| get("MAIN_BOT_INTERNAL_TOKEN").and_then(non_empty))
            .unwrap_or_else(|| internal_token.to_string());
        Ok(Self { base_url, token })
    }
}

/// Gesamte Settings des Bot-Prozesses (für Phase 0b: DB + interne API + Broker).
#[derive(Debug, Clone)]
pub struct Settings {
    pub db: DbConfig,
    pub internal_api: InternalApiConfig,
    pub broker: BrokerConfig,
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
    fn invalid_int_errors() {
        let mut m = minimal();
        m.insert("TWITCH_ANALYTICS_POOL_MAXSIZE", "abc");
        let err = Settings::load(&src(m)).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(n) if n == "TWITCH_ANALYTICS_POOL_MAXSIZE"));
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
}

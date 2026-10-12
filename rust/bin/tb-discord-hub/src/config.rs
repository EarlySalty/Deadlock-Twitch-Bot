//! Konfiguration des Discord-Hubs aus Umgebungsvariablen (Infisical injiziert sie).
//!
//! Variablen:
//! - `DISCORD_HUB_HOST` / `DISCORD_HUB_PORT` – Bind-Adresse (Standard 127.0.0.1:8771)
//! - `DISCORD_HUB_INTERNAL_TOKEN` → Broker-Token-Kette – Token eingehender Anfragen
//! - `MASTER_BROKER_BASE_URL` bzw. `MASTER_BROKER_HOST`/`MASTER_BROKER_PORT` – Broker
//! - `MASTER_BROKER_TOKEN` → `MAIN_BOT_INTERNAL_TOKEN` → `TWITCH_INTERNAL_API_TOKEN` – Broker-Token
//! - `DISCORD_HUB_BOT_TOKEN` – optional; fehlt es, läuft nur die Weiterleitung
//! - `DISCORD_HUB_APPLICATION_ID` – optional
//! - `DISCORD_HUB_COMMUNITY_GUILD_ID` – deutscher Community-Server

/// Standard-Guild des deutschen Community-Servers.
pub const DEFAULT_COMMUNITY_GUILD_ID: u64 = 1_289_721_245_281_292_288;
/// Standard-Port des Hubs (Broker liegt auf 8770).
pub const DEFAULT_PORT: u16 = 8771;
/// Standard-Bind-Host.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Fehler beim Laden der Konfiguration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Pflicht-Variable fehlt: {0}")]
    MissingRequired(String),
    #[error("ungültiger Wert für {name}: {reason}")]
    Invalid { name: String, reason: String },
}

/// Gesamte Konfiguration des Hubs.
#[derive(Clone)]
pub struct HubConfig {
    pub host: String,
    pub port: u16,
    /// Token, das eingehende Anfragen im Header `X-Internal-Token` tragen müssen.
    pub internal_token: String,
    pub broker_base_url: String,
    pub broker_token: String,
    /// Eigenes Bot-Token des Hubs. `None` → nur Weiterleitung, kein Gateway/REST.
    pub bot_token: Option<String>,
    pub application_id: Option<u64>,
    pub community_guild_id: u64,
}

impl std::fmt::Debug for HubConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Tokens niemals ausgeben.
        f.debug_struct("HubConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("internal_token", &"[geschützt]")
            .field("broker_base_url", &self.broker_base_url)
            .field("broker_token", &"[geschützt]")
            .field(
                "bot_token",
                &self.bot_token.as_ref().map(|_| "[geschützt]"),
            )
            .field("application_id", &self.application_id)
            .field("community_guild_id", &self.community_guild_id)
            .finish()
    }
}

fn non_empty(v: String) -> Option<String> {
    let t = v.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

type Get<'a> = dyn Fn(&str) -> Option<String> + 'a;

fn value(get: &Get, name: &str) -> Option<String> {
    get(name).and_then(non_empty)
}

fn parse_opt<T: std::str::FromStr>(get: &Get, name: &str) -> Result<Option<T>, ConfigError>
where
    T::Err: std::fmt::Display,
{
    match value(get, name) {
        None => Ok(None),
        Some(v) => v.parse::<T>().map(Some).map_err(|e| ConfigError::Invalid {
            name: name.to_string(),
            reason: e.to_string(),
        }),
    }
}

impl HubConfig {
    /// Lädt die Konfiguration aus der Prozessumgebung.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(&|name| std::env::var(name).ok())
    }

    /// Lädt die Konfiguration über eine beliebige Quelle (für Tests).
    pub fn load(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let host = value(get, "DISCORD_HUB_HOST").unwrap_or_else(|| DEFAULT_HOST.to_string());
        let port = parse_opt::<u16>(get, "DISCORD_HUB_PORT")?.unwrap_or(DEFAULT_PORT);
        if port == 0 {
            return Err(ConfigError::Invalid {
                name: "DISCORD_HUB_PORT".to_string(),
                reason: "Port 0 nicht erlaubt".to_string(),
            });
        }

        let twitch_internal = value(get, "TWITCH_INTERNAL_API_TOKEN");
        let broker_base_url = match value(get, "MASTER_BROKER_BASE_URL") {
            Some(u) => u,
            None => {
                let host = value(get, "MASTER_BROKER_HOST")
                    .unwrap_or_else(|| "127.0.0.1".to_string());
                let port = parse_opt::<u16>(get, "MASTER_BROKER_PORT")?
                    .filter(|p| *p >= 1)
                    .unwrap_or(8770);
                format!("http://{host}:{port}")
            }
        };
        // Fallback-Kette wie BrokerConfig in tb-config:
        // MASTER_BROKER_TOKEN → MAIN_BOT_INTERNAL_TOKEN → TWITCH_INTERNAL_API_TOKEN
        let broker_token = value(get, "MASTER_BROKER_TOKEN")
            .or_else(|| value(get, "MAIN_BOT_INTERNAL_TOKEN"))
            .or(twitch_internal)
            .ok_or_else(|| ConfigError::MissingRequired("MASTER_BROKER_TOKEN".into()))?;
        // Gleiche Kette wie der Twitch-Bot (`BrokerConfig::with_discord_hub`):
        // eigenes Hub-Token, sonst das aufgelöste Broker-Token.
        let internal_token =
            value(get, "DISCORD_HUB_INTERNAL_TOKEN").unwrap_or_else(|| broker_token.clone());

        let bot_token = value(get, "DISCORD_HUB_BOT_TOKEN");
        let application_id = parse_opt::<u64>(get, "DISCORD_HUB_APPLICATION_ID")?;
        let community_guild_id = parse_opt::<u64>(get, "DISCORD_HUB_COMMUNITY_GUILD_ID")?
            .unwrap_or(DEFAULT_COMMUNITY_GUILD_ID);

        Ok(Self {
            host,
            port,
            internal_token,
            broker_base_url,
            broker_token,
            bot_token,
            application_id,
            community_guild_id,
        })
    }

    /// Bind-Adresse als `host:port`.
    pub fn bind_addr(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
            // IPv6-Literal
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(vars: &[(&str, &str)]) -> Result<HubConfig, ConfigError> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        HubConfig::load(&|k| map.get(k).cloned())
    }

    #[test]
    fn standardwerte_mit_minimalem_token() {
        let c = load(&[("TWITCH_INTERNAL_API_TOKEN", "t")]).unwrap();
        assert_eq!(c.host, "127.0.0.1");
        assert_eq!(c.port, 8771);
        assert_eq!(c.internal_token, "t");
        assert_eq!(c.broker_base_url, "http://127.0.0.1:8770");
        assert_eq!(c.broker_token, "t");
        assert!(c.bot_token.is_none());
        assert!(c.application_id.is_none());
        assert_eq!(c.community_guild_id, DEFAULT_COMMUNITY_GUILD_ID);
        assert_eq!(c.bind_addr(), "127.0.0.1:8771");
    }

    #[test]
    fn explizite_werte_haben_vorrang() {
        let c = load(&[
            ("TWITCH_INTERNAL_API_TOKEN", "t"),
            ("DISCORD_HUB_INTERNAL_TOKEN", "hub"),
            ("DISCORD_HUB_HOST", "0.0.0.0"),
            ("DISCORD_HUB_PORT", "9000"),
            ("MASTER_BROKER_BASE_URL", "http://broker:1"),
            ("MASTER_BROKER_HOST", "ignoriert"),
            ("MAIN_BOT_INTERNAL_TOKEN", "main"),
            ("DISCORD_HUB_BOT_TOKEN", "bot"),
            ("DISCORD_HUB_APPLICATION_ID", "42"),
            ("DISCORD_HUB_COMMUNITY_GUILD_ID", "7"),
        ])
        .unwrap();
        assert_eq!(c.internal_token, "hub");
        assert_eq!(c.bind_addr(), "0.0.0.0:9000");
        assert_eq!(c.broker_base_url, "http://broker:1");
        assert_eq!(c.broker_token, "main");
        assert_eq!(c.bot_token.as_deref(), Some("bot"));
        assert_eq!(c.application_id, Some(42));
        assert_eq!(c.community_guild_id, 7);
    }

    #[test]
    fn broker_host_port_und_token_kette() {
        let c = load(&[
            ("DISCORD_HUB_INTERNAL_TOKEN", "hub"),
            ("MASTER_BROKER_HOST", "10.0.0.1"),
            ("MASTER_BROKER_PORT", "9999"),
            ("MASTER_BROKER_TOKEN", "broker"),
            ("MAIN_BOT_INTERNAL_TOKEN", "main"),
        ])
        .unwrap();
        assert_eq!(c.broker_base_url, "http://10.0.0.1:9999");
        assert_eq!(c.broker_token, "broker");
    }

    #[test]
    fn fehlende_tokens_sind_fehler() {
        assert!(matches!(load(&[]), Err(ConfigError::MissingRequired(_))));
        // Hub-Token vorhanden, aber kein Broker-Token.
        assert!(matches!(
            load(&[("DISCORD_HUB_INTERNAL_TOKEN", "hub")]),
            Err(ConfigError::MissingRequired(_))
        ));
        // Leere Werte zählen als fehlend.
        assert!(load(&[("TWITCH_INTERNAL_API_TOKEN", "  ")]).is_err());
    }

    #[test]
    fn ungueltige_zahlen_sind_fehler() {
        for (k, v) in [
            ("DISCORD_HUB_PORT", "abc"),
            ("DISCORD_HUB_PORT", "0"),
            ("DISCORD_HUB_APPLICATION_ID", "x"),
            ("DISCORD_HUB_COMMUNITY_GUILD_ID", "-1"),
        ] {
            assert!(
                matches!(
                    load(&[("TWITCH_INTERNAL_API_TOKEN", "t"), (k, v)]),
                    Err(ConfigError::Invalid { .. })
                ),
                "{k}={v}"
            );
        }
    }

    #[test]
    fn debug_zeigt_keine_tokens() {
        let c = load(&[
            ("TWITCH_INTERNAL_API_TOKEN", "geheim-intern"),
            ("MASTER_BROKER_TOKEN", "geheim-broker"),
            ("DISCORD_HUB_BOT_TOKEN", "geheim-bot"),
        ])
        .unwrap();
        let s = format!("{c:?}");
        assert!(!s.contains("geheim"), "{s}");
        assert!(s.contains("[geschützt]"));
    }
}

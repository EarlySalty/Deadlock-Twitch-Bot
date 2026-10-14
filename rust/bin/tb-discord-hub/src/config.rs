pub const DEFAULT_COMMUNITY_GUILD_ID: u64 = 1_289_721_245_281_292_288;
pub const DEFAULT_PORT: u16 = 8771;
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const APPLICATION_ID: u64 = 1_537_305_565_292_924_998;
pub const OAUTH_REDIRECT_URI: &str =
    "https://deutsche-deadlock-community.de/twitch/auth/discord/hub/callback";
pub const REQUIRED_PERMISSIONS: u64 = (1 << 10) | (1 << 11) | (1 << 14) | (1 << 28);

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Pflicht-Variable fehlt: {0}")]
    MissingRequired(String),
    #[error("ungültiger Wert für {name}: {reason}")]
    Invalid { name: String, reason: String },
}

#[derive(Clone)]
pub struct HubConfig {
    pub host: String,
    pub port: u16,
    pub internal_token: String,
    pub broker_base_url: String,
    pub broker_token: String,
    pub bot_token: Option<String>,
    pub community_guild_id: u64,
}

impl std::fmt::Debug for HubConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("internal_token", &"[geschützt]")
            .field("broker_token", &"[geschützt]")
            .field("bot_token", &self.bot_token.as_ref().map(|_| "[geschützt]"))
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
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(&|name| std::env::var(name).ok())
    }

    pub fn load(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let host = value(get, "DISCORD_DEADLOCK_TWITCH_BOT_HOST")
            .unwrap_or_else(|| DEFAULT_HOST.to_string());
        let port =
            parse_opt::<u16>(get, "DISCORD_DEADLOCK_TWITCH_BOT_PORT")?.unwrap_or(DEFAULT_PORT);
        if port == 0 {
            return Err(ConfigError::Invalid {
                name: "DISCORD_DEADLOCK_TWITCH_BOT_PORT".to_string(),
                reason: "Port 0 nicht erlaubt".to_string(),
            });
        }

        let twitch_internal = value(get, "TWITCH_INTERNAL_API_TOKEN");
        let broker_base_url = match value(get, "MASTER_BROKER_BASE_URL") {
            Some(u) => u,
            None => {
                let host =
                    value(get, "MASTER_BROKER_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
                let port = parse_opt::<u16>(get, "MASTER_BROKER_PORT")?
                    .filter(|p| *p >= 1)
                    .unwrap_or(8770);
                if host.contains(':') && !host.starts_with('[') {
                    format!("http://[{host}]:{port}")
                } else {
                    format!("http://{host}:{port}")
                }
            }
        };
        let broker_token = value(get, "MASTER_BROKER_TOKEN")
            .or_else(|| value(get, "MAIN_BOT_INTERNAL_TOKEN"))
            .or(twitch_internal)
            .ok_or_else(|| ConfigError::MissingRequired("MASTER_BROKER_TOKEN".into()))?;
        let internal_token = value(get, "DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN")
            .unwrap_or_else(|| broker_token.clone());

        let bot_token = value(get, "DISCORD_DEADLOCK_TWITCH_BOT");
        let community_guild_id =
            parse_opt::<u64>(get, "DISCORD_DEADLOCK_TWITCH_BOT_COMMUNITY_GUILD_ID")?
                .unwrap_or(DEFAULT_COMMUNITY_GUILD_ID);

        if community_guild_id == 0 {
            return Err(ConfigError::Invalid {
                name: "DISCORD_DEADLOCK_TWITCH_BOT_COMMUNITY_GUILD_ID".into(),
                reason: "Guild-ID 0 nicht erlaubt".into(),
            });
        }

        Ok(Self {
            host,
            port,
            internal_token,
            broker_base_url,
            broker_token,
            bot_token,
            community_guild_id,
        })
    }

    pub fn bind_addr(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
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
        assert_eq!(c.community_guild_id, DEFAULT_COMMUNITY_GUILD_ID);
        assert_eq!(c.bind_addr(), "127.0.0.1:8771");
    }

    #[test]
    fn explizite_werte_haben_vorrang() {
        let c = load(&[
            ("TWITCH_INTERNAL_API_TOKEN", "t"),
            ("DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN", "hub"),
            ("DISCORD_DEADLOCK_TWITCH_BOT_HOST", "0.0.0.0"),
            ("DISCORD_DEADLOCK_TWITCH_BOT_PORT", "9000"),
            ("MASTER_BROKER_BASE_URL", "http://broker:1"),
            ("MASTER_BROKER_HOST", "ignoriert"),
            ("MAIN_BOT_INTERNAL_TOKEN", "main"),
            ("DISCORD_DEADLOCK_TWITCH_BOT", "bot"),
            ("DISCORD_DEADLOCK_TWITCH_BOT_COMMUNITY_GUILD_ID", "7"),
        ])
        .unwrap();
        assert_eq!(c.internal_token, "hub");
        assert_eq!(c.bind_addr(), "0.0.0.0:9000");
        assert_eq!(c.broker_base_url, "http://broker:1");
        assert_eq!(c.broker_token, "main");
        assert_eq!(c.bot_token.as_deref(), Some("bot"));
        assert_eq!(c.community_guild_id, 7);
    }

    #[test]
    fn broker_host_port_und_token_kette() {
        let c = load(&[
            ("DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN", "hub"),
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
    fn broker_ipv6_host_is_bracketed() {
        for host in ["::1", "[::1]"] {
            let config = load(&[
                ("MASTER_BROKER_TOKEN", "synthetic"),
                ("MASTER_BROKER_HOST", host),
            ])
            .expect("config");
            assert_eq!(config.broker_base_url, "http://[::1]:8770");
            assert!(reqwest::Url::parse(&config.broker_base_url).is_ok());
        }
    }

    #[test]
    fn fehlende_tokens_sind_fehler() {
        assert!(matches!(load(&[]), Err(ConfigError::MissingRequired(_))));
        assert!(matches!(
            load(&[("DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN", "hub")]),
            Err(ConfigError::MissingRequired(_))
        ));
        assert!(load(&[("TWITCH_INTERNAL_API_TOKEN", "  ")]).is_err());
    }

    #[test]
    fn ungueltige_zahlen_sind_fehler() {
        for (k, v) in [
            ("DISCORD_DEADLOCK_TWITCH_BOT_PORT", "abc"),
            ("DISCORD_DEADLOCK_TWITCH_BOT_PORT", "0"),
            ("DISCORD_DEADLOCK_TWITCH_BOT_COMMUNITY_GUILD_ID", "-1"),
            ("DISCORD_DEADLOCK_TWITCH_BOT_COMMUNITY_GUILD_ID", "0"),
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
    fn existing_application_and_minimal_permissions() {
        assert_eq!(APPLICATION_ID, 1537305565292924998);
        assert_eq!(REQUIRED_PERMISSIONS, 268454912);
        assert_eq!(REQUIRED_PERMISSIONS & ((1 << 3) | (1 << 17)), 0);
        assert_eq!(
            OAUTH_REDIRECT_URI,
            "https://deutsche-deadlock-community.de/twitch/auth/discord/hub/callback"
        );
        let c = load(&[
            ("MASTER_BROKER_TOKEN", "internal"),
            ("DISCORD_HUB_BOT_TOKEN", "old-app"),
        ])
        .expect("config");
        assert!(c.bot_token.is_none());
    }

    #[test]
    fn debug_zeigt_keine_tokens() {
        let c = load(&[
            ("TWITCH_INTERNAL_API_TOKEN", "geheim-intern"),
            ("MASTER_BROKER_TOKEN", "geheim-broker"),
            ("DISCORD_DEADLOCK_TWITCH_BOT", "geheim-bot"),
        ])
        .unwrap();
        let s = format!("{c:?}");
        assert!(!s.contains("geheim"), "{s}");
        assert!(s.contains("[geschützt]"));
    }
}

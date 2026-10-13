//! Abbildung von Dispatch-Ereignissen auf Hub-Ereignisse. Reine Logik.

use serde_json::Value;

use super::protocol::snowflake;

/// Für den Hub relevante Gateway-Ereignisse.
#[derive(Debug, Clone, PartialEq)]
pub enum HubEvent {
    /// READY: eigene Bot-User-ID und (falls mitgeliefert) Application-ID.
    Ready {
        bot_user_id: u64,
        application_id: Option<u64>,
        guild_count: usize,
    },
    /// Guild verfügbar/hinzugefügt (beim Start für jede Guild erneut).
    GuildAdded { guild_id: u64, name: String },
    /// Bot wurde aus der Guild entfernt (nicht: Guild nur vorübergehend weg).
    GuildRemoved { guild_id: u64 },
    /// Mitglied hat die Guild verlassen (oder wurde entfernt).
    MemberLeft { guild_id: u64, user_id: u64 },
    /// Slash-Befehl `/setup`.
    SetupCommand(SetupInteraction),
}

/// Die für `/setup` nötigen Felder einer Interaction.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupInteraction {
    pub interaction_id: u64,
    pub token: String,
    pub application_id: u64,
    /// `None`, wenn der Befehl außerhalb einer Guild ausgeführt wurde.
    pub guild_id: Option<u64>,
    /// `app_permissions` der Interaction (Rechte des Bots im Kanal), als Rückfall.
    pub app_permissions: Option<u64>,
    pub options: SetupOptions,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SetupOptions {
    pub language: Option<String>,
    pub announce_channel_id: Option<u64>,
    pub alert_channel_id: Option<u64>,
    pub streamer_role_id: Option<u64>,
    pub ping_role_id: Option<u64>,
    pub invite: Option<String>,
    pub auto_partner: Option<bool>,
}

/// Interaction-Typ APPLICATION_COMMAND.
const INTERACTION_APPLICATION_COMMAND: u64 = 2;

/// Bildet ein Dispatch-Ereignis ab. `None` = für den Hub uninteressant.
pub fn map_dispatch(name: &str, d: &Value) -> Option<HubEvent> {
    match name {
        "READY" => Some(HubEvent::Ready {
            bot_user_id: snowflake(d.get("user").and_then(|u| u.get("id")))?,
            application_id: snowflake(d.get("application").and_then(|a| a.get("id"))),
            guild_count: d
                .get("guilds")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        }),
        "GUILD_CREATE" => {
            if d.get("unavailable").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            Some(HubEvent::GuildAdded {
                guild_id: snowflake(d.get("id"))?,
                name: d
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        }
        "GUILD_DELETE" => {
            // `unavailable: true` = Ausfall bei Discord, Bot ist weiter Mitglied.
            if d.get("unavailable").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            Some(HubEvent::GuildRemoved {
                guild_id: snowflake(d.get("id"))?,
            })
        }
        "GUILD_MEMBER_REMOVE" => Some(HubEvent::MemberLeft {
            guild_id: snowflake(d.get("guild_id"))?,
            user_id: snowflake(d.get("user").and_then(|u| u.get("id")))?,
        }),
        "INTERACTION_CREATE" => {
            if d.get("type").and_then(Value::as_u64) != Some(INTERACTION_APPLICATION_COMMAND) {
                return None;
            }
            let cmd = d.get("data")?.get("name")?.as_str()?;
            if cmd != "setup" {
                return None;
            }
            Some(HubEvent::SetupCommand(SetupInteraction {
                interaction_id: snowflake(d.get("id"))?,
                token: d.get("token")?.as_str()?.to_string(),
                application_id: snowflake(d.get("application_id"))?,
                guild_id: snowflake(d.get("guild_id")),
                app_permissions: snowflake(d.get("app_permissions")),
                options: parse_setup_options(d.get("data").and_then(|data| data.get("options"))),
            }))
        }
        _ => None,
    }
}

fn parse_setup_options(options: Option<&Value>) -> SetupOptions {
    let mut parsed = SetupOptions::default();
    let Some(options) = options.and_then(Value::as_array) else {
        return parsed;
    };
    for option in options {
        let Some(name) = option.get("name").and_then(Value::as_str) else {
            continue;
        };
        let value = option.get("value");
        match name {
            "language" => {
                if let Some(lang) = value.and_then(Value::as_str) {
                    if lang == "de" || lang == "en" {
                        parsed.language = Some(lang.to_string());
                    }
                }
            }
            "announce_channel" => parsed.announce_channel_id = snowflake(value),
            "alert_channel" => parsed.alert_channel_id = snowflake(value),
            "streamer_role" => parsed.streamer_role_id = snowflake(value),
            "ping_role" => parsed.ping_role_id = snowflake(value),
            "invite" => {
                parsed.invite = value
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && s.len() <= 200)
                    .map(str::to_string);
            }
            "auto_partner" => parsed.auto_partner = value.and_then(Value::as_bool),
            _ => {}
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ready() {
        let d = json!({
            "user": {"id": "99"},
            "application": {"id": "77"},
            "guilds": [{"id":"1","unavailable":true},{"id":"2","unavailable":true}],
            "session_id": "s"
        });
        assert_eq!(
            map_dispatch("READY", &d),
            Some(HubEvent::Ready {
                bot_user_id: 99,
                application_id: Some(77),
                guild_count: 2
            })
        );
    }

    #[test]
    fn guild_create() {
        let d = json!({"id": "123", "name": "Test"});
        assert_eq!(
            map_dispatch("GUILD_CREATE", &d),
            Some(HubEvent::GuildAdded {
                guild_id: 123,
                name: "Test".into()
            })
        );
        let d = json!({"id": "123", "unavailable": true});
        assert_eq!(map_dispatch("GUILD_CREATE", &d), None);
    }

    #[test]
    fn guild_delete() {
        assert_eq!(
            map_dispatch("GUILD_DELETE", &json!({"id": "5"})),
            Some(HubEvent::GuildRemoved { guild_id: 5 })
        );
        assert_eq!(
            map_dispatch("GUILD_DELETE", &json!({"id": "5", "unavailable": false})),
            Some(HubEvent::GuildRemoved { guild_id: 5 })
        );
        assert_eq!(
            map_dispatch("GUILD_DELETE", &json!({"id": "5", "unavailable": true})),
            None
        );
    }

    #[test]
    fn member_remove() {
        let d = json!({"guild_id": "5", "user": {"id": "8", "username": "x"}});
        assert_eq!(
            map_dispatch("GUILD_MEMBER_REMOVE", &d),
            Some(HubEvent::MemberLeft {
                guild_id: 5,
                user_id: 8
            })
        );
        assert_eq!(map_dispatch("GUILD_MEMBER_REMOVE", &json!({})), None);
    }

    #[test]
    fn setup_interaction() {
        let d = json!({
            "id": "11", "type": 2, "token": "tok", "application_id": "77",
            "guild_id": "5", "app_permissions": "268454912",
            "data": {"name": "setup", "type": 1}
        });
        assert_eq!(
            map_dispatch("INTERACTION_CREATE", &d),
            Some(HubEvent::SetupCommand(SetupInteraction {
                interaction_id: 11,
                token: "tok".into(),
                application_id: 77,
                guild_id: Some(5),
                app_permissions: Some(268454912),
                options: SetupOptions::default(),
            }))
        );
    }

    #[test]
    fn andere_interactions_ignoriert() {
        let other_cmd = json!({
            "id": "11", "type": 2, "token": "tok", "application_id": "77",
            "data": {"name": "other"}
        });
        assert_eq!(map_dispatch("INTERACTION_CREATE", &other_cmd), None);
        let component = json!({
            "id": "11", "type": 3, "token": "tok", "application_id": "77",
            "data": {"name": "setup"}
        });
        assert_eq!(map_dispatch("INTERACTION_CREATE", &component), None);
    }

    #[test]
    fn unbekannt_ignoriert() {
        assert_eq!(map_dispatch("MESSAGE_CREATE", &json!({})), None);
    }

    #[test]
    fn setup_optionen() {
        let d = json!({
            "id": "11", "type": 2, "token": "tok", "application_id": "77",
            "guild_id": "5",
            "data": {
                "name": "setup",
                "options": [
                    {"name": "language", "type": 3, "value": "en"},
                    {"name": "announce_channel", "type": 7, "value": "9"},
                    {"name": "auto_partner", "type": 5, "value": false},
                    {"name": "invite", "type": 3, "value": "https://discord.gg/abc"}
                ]
            }
        });
        let Some(HubEvent::SetupCommand(inter)) = map_dispatch("INTERACTION_CREATE", &d) else {
            panic!("setup erwartet");
        };
        assert_eq!(inter.options.language.as_deref(), Some("en"));
        assert_eq!(inter.options.announce_channel_id, Some(9));
        assert_eq!(inter.options.auto_partner, Some(false));
        assert_eq!(
            inter.options.invite.as_deref(),
            Some("https://discord.gg/abc")
        );
    }
}

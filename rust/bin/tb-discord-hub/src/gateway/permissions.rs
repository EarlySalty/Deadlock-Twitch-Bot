//! Rechte des Hubs: Einladungslink, Berechnung der Guild-Rechte aus Rollen
//! und Text der `/setup`-Antwort. Reine Logik.

use serde::Deserialize;

use super::protocol::snowflake;

pub const ADMINISTRATOR: u64 = 1 << 3;
pub const MANAGE_GUILD: u64 = 1 << 5;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const SEND_MESSAGES: u64 = 1 << 11;
pub const EMBED_LINKS: u64 = 1 << 14;
pub const MENTION_EVERYONE: u64 = 1 << 17;
pub const MANAGE_ROLES: u64 = 1 << 28;

/// Rechte, die der Hub anfordert (Einladungslink) und bei `/setup` prüft.
pub const REQUIRED_PERMISSIONS: u64 = VIEW_CHANNEL | SEND_MESSAGES | EMBED_LINKS | MANAGE_ROLES;

/// Benötigte Rechte mit englischem Anzeigenamen.
const REQUIRED: [(u64, &str); 4] = [
    (VIEW_CHANNEL, "View Channels"),
    (SEND_MESSAGES, "Send Messages"),
    (EMBED_LINKS, "Embed Links"),
    (MANAGE_ROLES, "Manage Roles"),
];

/// Einladungslink für den Hub mit Scopes `bot applications.commands` und
/// genau den benötigten Rechten (kein Admin, kein @everyone).
pub fn invite_url(application_id: u64) -> String {
    format!(
        "https://discord.com/oauth2/authorize?client_id={application_id}\
         &scope=bot%20applications.commands&permissions={REQUIRED_PERMISSIONS}"
    )
}

/// Rolle aus `GET /guilds/{id}` bzw. `/guilds/{id}/roles`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Role {
    #[serde(deserialize_with = "de_snowflake")]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    /// Bitmaske als String (Discord-Format).
    #[serde(default, deserialize_with = "de_perm")]
    pub permissions: u64,
    #[serde(default)]
    pub position: i64,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub mentionable: bool,
}

fn de_snowflake<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    snowflake(Some(&v)).ok_or_else(|| serde::de::Error::custom("ungültige Snowflake"))
}

fn de_perm<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(snowflake(Some(&v)).unwrap_or(0))
}

/// Ergebnis der Rechteprüfung für `/setup`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupReport {
    /// Effektive Guild-Rechte des Hubs (ohne Kanal-Overrides).
    pub permissions: u64,
    /// Name und Position der höchsten Rolle des Hubs.
    pub top_role: Option<(String, i64)>,
    /// Rollen auf oder über der Hub-Rolle (kann der Hub nicht verwalten).
    pub roles_above: Vec<String>,
    /// Anzahl Rollen unterhalb der Hub-Rolle (verwaltbar).
    pub manageable_roles: usize,
}

impl SetupReport {
    pub fn has(&self, perm: u64) -> bool {
        self.permissions & ADMINISTRATOR != 0 || self.permissions & perm == perm
    }

    pub fn missing(&self) -> Vec<&'static str> {
        REQUIRED
            .iter()
            .filter(|(bit, _)| !self.has(*bit))
            .map(|(_, name)| *name)
            .collect()
    }
}

/// Berechnet die Guild-Rechte des Bots aus @everyone + eigenen Rollen und
/// die Rollen-Hierarchie relativ zur höchsten Hub-Rolle.
pub fn compute_report(
    guild_id: u64,
    owner_id: Option<u64>,
    bot_user_id: u64,
    roles: &[Role],
    member_role_ids: &[u64],
) -> SetupReport {
    let everyone = roles.iter().find(|r| r.id == guild_id);
    let mut perms = everyone.map_or(0, |r| r.permissions);
    let mut top: Option<&Role> = None;
    for role in roles.iter().filter(|r| member_role_ids.contains(&r.id)) {
        perms |= role.permissions;
        if top.is_none_or(|t| (role.position, role.id) > (t.position, t.id)) {
            top = Some(role);
        }
    }
    if owner_id == Some(bot_user_id) || perms & ADMINISTRATOR != 0 {
        perms = u64::MAX;
    }
    let top_pos = top.map_or(0, |t| t.position);
    let mut above: Vec<&Role> = roles
        .iter()
        .filter(|r| r.id != guild_id && !member_role_ids.contains(&r.id))
        .filter(|r| r.position >= top_pos)
        .collect();
    above.sort_by_key(|r| std::cmp::Reverse(r.position));
    let manageable_roles = roles
        .iter()
        .filter(|r| r.id != guild_id && !r.managed && r.position < top_pos)
        .count();
    SetupReport {
        permissions: perms,
        top_role: top.map(|t| (t.name.clone(), t.position)),
        roles_above: above.into_iter().map(|r| r.name.clone()).collect(),
        manageable_roles,
    }
}

/// Maximal so viele Rollen über der Hub-Rolle werden namentlich genannt.
const MAX_LISTED_ROLES: usize = 10;

/// Text der `/setup`-Antwort für fremde Server (Englisch, für Nutzer sichtbar).
pub fn render_setup_report(report: &SetupReport) -> String {
    let mut out = String::from("**Deadlock Hub – setup check**\n\n**Permissions**\n");
    for (bit, name) in REQUIRED {
        let mark = if report.has(bit) { "✅" } else { "❌" };
        out.push_str(&format!("{mark} {name}\n"));
    }
    let missing = report.missing();
    if missing.is_empty() {
        out.push_str("All required permissions are granted.\n");
    } else {
        out.push_str(&format!(
            "Missing: {}. Grant them to the Hub's role in Server Settings → Roles.\n",
            missing.join(", ")
        ));
    }

    out.push_str("\n**Roles**\n");
    match &report.top_role {
        Some((name, _)) => out.push_str(&format!(
            "The Hub can only manage roles **below** its highest role (`{}`). \
             Roles it can manage: {}.\n",
            sanitize(name),
            report.manageable_roles
        )),
        None => out.push_str(
            "The Hub has no own role yet, so it cannot manage any roles. \
             Re-invite it with the invite link.\n",
        ),
    }
    if !report.roles_above.is_empty() {
        let shown: Vec<String> = report
            .roles_above
            .iter()
            .take(MAX_LISTED_ROLES)
            .map(|n| format!("`{}`", sanitize(n)))
            .collect();
        let more = report.roles_above.len().saturating_sub(MAX_LISTED_ROLES);
        let suffix = if more > 0 {
            format!(" (+{more} more)")
        } else {
            String::new()
        };
        out.push_str(&format!(
            "Roles at or above the Hub role (not manageable): {}{suffix}. \
             Drag the Hub role above the streamer/ping roles you want it to assign.\n",
            shown.join(", ")
        ));
    }
    out.push_str(
        "Ping roles must be set to **\"Allow anyone to @mention this role\"**. \
         The Hub needs neither Administrator nor \"Mention @everyone\".\n",
    );
    out.push_str(
        "\nPass channel, role, language, invite and auto-partner options to `/setup` \
         to save them for this server. Channel-specific overrides are checked then.",
    );
    out
}

/// Antwort von `/setup` auf dem deutschen Community-Server.
pub fn render_community_notice() -> String {
    "This server is managed by the main Deadlock bot – no setup needed here.\n\
     Dieser Server wird vom Haupt-Bot betreut – hier ist kein Setup nötig."
        .to_string()
}

/// Antwort, wenn die Rechte nicht ermittelt werden konnten.
pub fn render_check_failed(app_permissions: Option<u64>) -> String {
    let mut out = String::from(
        "**Deadlock Hub – setup check**\n\nCould not read the server roles right now. ",
    );
    if let Some(p) = app_permissions {
        let report = SetupReport {
            permissions: p,
            top_role: None,
            roles_above: Vec::new(),
            manageable_roles: 0,
        };
        let missing = report.missing();
        if missing.is_empty() {
            out.push_str("In this channel the Hub has all required permissions. ");
        } else {
            out.push_str(&format!(
                "In this channel the Hub is missing: {}. ",
                missing.join(", ")
            ));
        }
    }
    out.push_str("Please try `/setup` again in a moment.");
    out
}

/// Entfernt Backticks und Markdown-Störer aus Rollennamen.
fn sanitize(name: &str) -> String {
    name.replace('`', "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn role(id: u64, name: &str, perms: u64, pos: i64) -> Role {
        Role {
            id,
            name: name.into(),
            permissions: perms,
            position: pos,
            managed: false,
            mentionable: false,
        }
    }

    #[test]
    fn einladungslink_ohne_admin_und_everyone() {
        let url = invite_url(123);
        assert!(url.contains("client_id=123"));
        assert!(url.contains("scope=bot%20applications.commands"));
        let perms: u64 = url
            .split("permissions=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(perms, REQUIRED_PERMISSIONS);
        assert_eq!(perms, 268_454_912);
        assert_eq!(perms & ADMINISTRATOR, 0);
        assert_eq!(perms & MENTION_EVERYONE, 0);
    }

    #[test]
    fn rolle_aus_json() {
        let r: Role = serde_json::from_value(json!({
            "id": "5", "name": "Hub", "permissions": "268454912",
            "position": 3, "managed": true, "mentionable": false
        }))
        .unwrap();
        assert_eq!(r.id, 5);
        assert_eq!(r.permissions, REQUIRED_PERMISSIONS);
        assert!(r.managed);
    }

    #[test]
    fn rechte_aus_everyone_und_rollen() {
        let guild = 1;
        let roles = vec![
            role(guild, "@everyone", VIEW_CHANNEL | SEND_MESSAGES, 0),
            role(10, "Hub", EMBED_LINKS | MANAGE_ROLES, 2),
            role(11, "Streamer", 0, 1),
            role(12, "Mods", MANAGE_GUILD, 5),
        ];
        let r = compute_report(guild, Some(999), 50, &roles, &[10]);
        assert!(r.missing().is_empty());
        assert_eq!(r.top_role, Some(("Hub".into(), 2)));
        assert_eq!(r.roles_above, vec!["Mods".to_string()]);
        assert_eq!(r.manageable_roles, 1);
    }

    #[test]
    fn fehlende_rechte() {
        let guild = 1;
        let roles = vec![
            role(guild, "@everyone", VIEW_CHANNEL, 0),
            role(10, "Hub", 0, 1),
        ];
        let r = compute_report(guild, None, 50, &roles, &[10]);
        assert_eq!(
            r.missing(),
            vec!["Send Messages", "Embed Links", "Manage Roles"]
        );
        let text = render_setup_report(&r);
        assert!(text.contains("❌ Manage Roles"));
        assert!(text.contains("✅ View Channels"));
        assert!(text.contains("Missing: Send Messages, Embed Links, Manage Roles"));
    }

    #[test]
    fn administrator_hat_alles() {
        let guild = 1;
        let roles = vec![
            role(guild, "@everyone", 0, 0),
            role(10, "Hub", ADMINISTRATOR, 1),
        ];
        let r = compute_report(guild, None, 50, &roles, &[10]);
        assert!(r.missing().is_empty());
    }

    #[test]
    fn ohne_eigene_rolle() {
        let guild = 1;
        let roles = vec![role(guild, "@everyone", REQUIRED_PERMISSIONS, 0)];
        let r = compute_report(guild, None, 50, &roles, &[]);
        assert_eq!(r.top_role, None);
        assert!(render_setup_report(&r).contains("no own role"));
    }

    #[test]
    fn text_enthaelt_hinweise() {
        let guild = 1;
        let roles = vec![
            role(guild, "@everyone", REQUIRED_PERMISSIONS, 0),
            role(10, "Hub", 0, 1),
        ];
        let text = render_setup_report(&compute_report(guild, None, 50, &roles, &[10]));
        assert!(text.contains("All required permissions"));
        assert!(text.contains("below"));
        assert!(text.contains("@mention this role"));
        assert!(text.contains("neither Administrator"));
        assert!(text.contains("Pass channel, role, language"));
        assert!(text.len() < 2000);
    }

    #[test]
    fn viele_rollen_ueber_hub_werden_gekuerzt() {
        let guild = 1;
        let mut roles = vec![role(guild, "@everyone", 0, 0), role(10, "Hub", 0, 1)];
        for i in 0..15 {
            roles.push(role(100 + i, &format!("R{i}"), 0, 2 + i as i64));
        }
        let text = render_setup_report(&compute_report(guild, None, 50, &roles, &[10]));
        assert!(text.contains("(+5 more)"));
    }

    #[test]
    fn rueckfall_text() {
        assert!(render_check_failed(Some(REQUIRED_PERMISSIONS)).contains("all required"));
        assert!(render_check_failed(Some(VIEW_CHANNEL)).contains("missing: Send Messages"));
        assert!(render_check_failed(None).contains("try `/setup` again"));
        assert!(render_community_notice().contains("main Deadlock bot"));
    }
}

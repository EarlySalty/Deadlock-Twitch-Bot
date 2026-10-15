use serde::Deserialize;

use super::protocol::snowflake;

pub const ADMINISTRATOR: u64 = 1 << 3;
pub const MANAGE_GUILD: u64 = 1 << 5;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const SEND_MESSAGES: u64 = 1 << 11;
pub const EMBED_LINKS: u64 = 1 << 14;
pub const MENTION_EVERYONE: u64 = 1 << 17;
pub const MANAGE_ROLES: u64 = 1 << 28;

pub const REQUIRED_PERMISSIONS: u64 = VIEW_CHANNEL | SEND_MESSAGES | EMBED_LINKS | MANAGE_ROLES;

const REQUIRED: [(u64, &str); 4] = [
    (VIEW_CHANNEL, "View Channels"),
    (SEND_MESSAGES, "Send Messages"),
    (EMBED_LINKS, "Embed Links"),
    (MANAGE_ROLES, "Manage Roles"),
];

pub fn invite_url(application_id: u64) -> String {
    format!(
        "https://discord.com/oauth2/authorize?client_id={application_id}\
         &scope=bot%20applications.commands&permissions={REQUIRED_PERMISSIONS}"
    )
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Role {
    #[serde(deserialize_with = "de_snowflake")]
    pub id: u64,
    #[serde(default)]
    pub name: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct SetupReport {
    pub permissions: u64,
    pub top_role: Option<(String, i64)>,
    pub roles_above: Vec<String>,
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

pub fn channel_allowed(
    channel: &serde_json::Value,
    guild: u64,
    bot: u64,
    roles: &[u64],
    mut permissions: u64,
) -> bool {
    if snowflake(channel.get("guild_id")) != Some(guild)
        || channel.get("type").and_then(|value| value.as_u64()) != Some(0)
    {
        return false;
    }
    if permissions & ADMINISTRATOR != 0 {
        return true;
    }
    let Some(overwrites) = channel
        .get("permission_overwrites")
        .and_then(|value| value.as_array())
    else {
        return false;
    };
    for (kind, target) in [(0, Some(guild)), (0, None), (1, Some(bot))] {
        let mut allow = 0;
        let mut deny = 0;
        for overwrite in overwrites {
            let Some(id) = snowflake(overwrite.get("id")) else {
                return false;
            };
            if overwrite.get("type").and_then(|value| value.as_u64()) != Some(kind)
                || !target.map_or_else(|| id != guild && roles.contains(&id), |target| id == target)
            {
                continue;
            }
            let (Some(a), Some(d)) = (
                snowflake(overwrite.get("allow")),
                snowflake(overwrite.get("deny")),
            ) else {
                return false;
            };
            allow |= a;
            deny |= d;
        }
        permissions = (permissions & !deny) | allow;
    }
    let required = VIEW_CHANNEL | SEND_MESSAGES | EMBED_LINKS;
    permissions & required == required
}

const MAX_LISTED_ROLES: usize = 10;

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

pub fn render_setup_report_de(report: &SetupReport) -> String {
    let mut text = String::from("**Deadlock Hub: Einrichtung**\n\n");
    for (permission, label) in [
        (VIEW_CHANNEL, "Kanäle ansehen"),
        (SEND_MESSAGES, "Nachrichten senden"),
        (EMBED_LINKS, "Links einbetten"),
        (MANAGE_ROLES, "Rollen verwalten"),
    ] {
        text.push_str(&format!(
            "{}: {}\n",
            label,
            if report.has(permission) {
                "vorhanden"
            } else {
                "fehlt"
            }
        ));
    }
    text.push_str("\nDie Bot-Rolle muss über den Streamer- und Ping-Rollen stehen. Die Ping-Rolle muss für alle erwähnbar sein. Der Bot braucht weder Administratorrechte noch die Berechtigung, @everyone zu erwähnen.\n\nMit den Optionen von `/setup` kannst du Kanäle, Rollen, Sprache, Einladung und die automatische Partneraufnahme speichern.");
    text
}

pub fn render_community_notice() -> String {
    "This server is managed by the main Deadlock bot – no setup needed here.\n\
     Dieser Server wird vom Haupt-Bot betreut – hier ist kein Setup nötig."
        .to_string()
}

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
    fn channel_ownership_and_overwrite_precedence() {
        let mut channel = json!({"guild_id":"1", "type":0, "permission_overwrites":[
            {"id":"1","type":0,"allow":"0","deny":SEND_MESSAGES.to_string()},
            {"id":"10","type":0,"allow":SEND_MESSAGES.to_string(),"deny":"0"}
        ]});
        assert!(channel_allowed(
            &channel,
            1,
            50,
            &[10],
            REQUIRED_PERMISSIONS
        ));
        assert!(!channel_allowed(&channel, 2, 50, &[10], ADMINISTRATOR));
        channel["permission_overwrites"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id":"50","type":1,"allow":"0","deny":SEND_MESSAGES.to_string()
            }));
        assert!(!channel_allowed(
            &channel,
            1,
            50,
            &[10],
            REQUIRED_PERMISSIONS
        ));
        channel["type"] = json!(2);
        assert!(!channel_allowed(&channel, 1, 50, &[10], ADMINISTRATOR));
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

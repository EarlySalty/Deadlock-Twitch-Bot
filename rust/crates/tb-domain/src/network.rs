//! Discord-Netzwerk eines Streamers (Multi-Server-Plan, Paket R): Sprachgruppen,
//! Server-Mitgliedschaft, Heimat-Server und die Raid-Regel über Servergrenzen.
//!
//! Reine Logik ohne I/O. Die Daten kommen aus `discord_guilds` und
//! `streamer_guilds` (geladen in `tb_db::network`).
//!
//! # Regeln
//!
//! - Ein Streamer ohne jede `streamer_guilds`-Zeile gehört fest zum deutschen
//!   Community-Server (Plan E1). Damit bleibt der heutige Betrieb gleich,
//!   auch für Partner, die nach der Migration ohne Zeile hinzukommen.
//! - Mitglied ist, wer eine aktive Zeile auf einem Server hat, der den Bot
//!   nicht entfernt hat. Heimat-Server ist der früheste davon.
//! - Sprache eines Streamers: eigene Kanalsprache, sonst die Sprache des
//!   Heimat-Servers. Verglichen wird die Sprachgruppe (`de-AT` → `de`).
//! - Raids: Partner desselben Servers sind immer erlaubt, unabhängig von der
//!   Sprache. Über Servergrenzen nur, wenn beide Seiten einen Server mit
//!   eingeschaltetem Netzwerk-Schalter haben und dieselbe Sprachgruppe sprechen.

/// Discord-Server der Deutschen Deadlock Community.
pub const COMMUNITY_GUILD_ID: i64 = 1_289_721_245_281_292_288;

/// Sprachgruppe, wenn für einen Streamer gar keine Sprache bekannt ist. Das
/// ist der bisher feste Wert des Kategorie-Ersatzes.
pub const DEFAULT_LANGUAGE_GROUP: &str = "de";

/// Sprachgruppe zu einem Twitch- oder BCP-47-Sprachcode: der Primär-Subtag in
/// Kleinbuchstaben. `de`, `de-DE`, `de_at` ergeben `de`; `en-GB` ergibt `en`.
/// Leere Werte, `other` und `und` (unbestimmt) ergeben `None`.
///
/// Der Rückgabewert ist zugleich ein gültiger Helix-Sprachparameter, weil
/// Twitch nur zweistellige ISO-639-1-Codes (oder `other`) vergibt.
pub fn language_group(code: &str) -> Option<String> {
    let primary = code.trim().split(['-', '_']).next()?.trim();
    if primary.is_empty() || primary.len() > 8 || !primary.chars().all(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    let primary = primary.to_ascii_lowercase();
    if primary == "other" || primary == "und" {
        return None;
    }
    Some(primary)
}

/// Einstellungen eines Discord-Servers, soweit Raids und Werbung sie brauchen
/// (Ausschnitt einer `discord_guilds`-Zeile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildSettings {
    pub guild_id: i64,
    /// Sprache des Servers (Sprachcode, z. B. `de` oder `en`).
    pub language: String,
    /// Netzwerk-Schalter: Raids über Servergrenzen erlaubt.
    pub cross_server_raids: bool,
    /// Discord-Einladung, mit der der Server im Twitch-Chat wirbt.
    pub promo_invite_url: Option<String>,
    /// `false`, wenn der Server den Bot entfernt hat.
    pub installed: bool,
}

impl GuildSettings {
    /// Werte des deutschen Servers, falls seine Zeile in `discord_guilds`
    /// fehlt (die Migration legt sie an; das ist nur die Rückfallebene).
    pub fn community_default() -> Self {
        Self {
            guild_id: COMMUNITY_GUILD_ID,
            language: DEFAULT_LANGUAGE_GROUP.to_string(),
            cross_server_raids: true,
            promo_invite_url: None,
            installed: true,
        }
    }

    pub fn is_community(&self) -> bool {
        self.guild_id == COMMUNITY_GUILD_ID
    }

    /// Werbe-Einladung ohne Leerraum; leere Werte zählen als nicht gesetzt.
    pub fn promo_invite(&self) -> Option<&str> {
        self.promo_invite_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
    }
}

/// Zugehörigkeit eines Streamers zu einem Server (`streamer_guilds`-Zeile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildMembership {
    pub guild: GuildSettings,
    pub active: bool,
}

/// Netzwerkdaten eines Streamers: Server-Zugehörigkeiten in Beitrittsreihenfolge
/// und die gespeicherte Kanalsprache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamerNetwork {
    memberships: Vec<GuildMembership>,
    broadcaster_language: Option<String>,
}

impl StreamerNetwork {
    /// Baut das Netzwerk aus den `streamer_guilds`-Zeilen (sortiert nach
    /// Beitritt). Ohne jede Zeile gehört der Streamer zum deutschen Server,
    /// dessen Einstellungen `community` liefert.
    pub fn new(
        memberships: Vec<GuildMembership>,
        broadcaster_language: Option<String>,
        community: &GuildSettings,
    ) -> Self {
        let memberships = if memberships.is_empty() {
            vec![GuildMembership {
                guild: community.clone(),
                active: true,
            }]
        } else {
            memberships
        };
        Self {
            memberships,
            broadcaster_language: broadcaster_language
                .map(|language| language.trim().to_string())
                .filter(|language| !language.is_empty()),
        }
    }

    /// Netzwerk eines Streamers ohne Zeilen, also Mitglied des deutschen Servers.
    pub fn community(community: &GuildSettings) -> Self {
        Self::new(Vec::new(), None, community)
    }

    /// Server, auf denen der Streamer aktiv Mitglied ist, in Beitrittsreihenfolge.
    pub fn active_guilds(&self) -> impl Iterator<Item = &GuildSettings> {
        self.memberships
            .iter()
            .filter(|membership| membership.active && membership.guild.installed)
            .map(|membership| &membership.guild)
    }

    /// Heimat-Server: der früheste Server mit aktiver Mitgliedschaft.
    pub fn home_guild(&self) -> Option<&GuildSettings> {
        self.active_guilds().next()
    }

    /// Gespeicherte Kanalsprache (`broadcaster_language`), roh.
    pub fn broadcaster_language(&self) -> Option<&str> {
        self.broadcaster_language.as_deref()
    }

    /// Sprachgruppe des Streamers: Sprache des laufenden Streams, sonst die
    /// gespeicherte Kanalsprache, sonst die Sprache des Heimat-Servers.
    pub fn language_group(&self, live_language: Option<&str>) -> Option<String> {
        live_language
            .and_then(language_group)
            .or_else(|| self.broadcaster_language().and_then(language_group))
            .or_else(|| {
                self.home_guild()
                    .and_then(|guild| language_group(&guild.language))
            })
    }

    /// `true`, wenn mindestens ein aktiver Server Raids über Servergrenzen erlaubt.
    pub fn allows_cross_server_raids(&self) -> bool {
        self.active_guilds().any(|guild| guild.cross_server_raids)
    }

    /// `true`, wenn beide Streamer aktiv auf einem gemeinsamen Server sind.
    pub fn shares_guild_with(&self, other: &StreamerNetwork) -> bool {
        self.active_guilds().any(|mine| {
            other
                .active_guilds()
                .any(|theirs| theirs.guild_id == mine.guild_id)
        })
    }
}

/// Weshalb ein Partner als Raid-Ziel erlaubt ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaidReach {
    /// Gemeinsamer Server; die Sprache spielt keine Rolle.
    SameServer,
    /// Anderer Server, beide mit Netzwerk-Schalter, gleiche Sprachgruppe.
    CrossServer,
}

/// Weshalb ein Partner als Raid-Ziel ausscheidet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaidBlock {
    /// Kein gemeinsamer Server und mindestens eine Seite hat den
    /// Netzwerk-Schalter aus (oder keinen aktiven Server).
    NetworkSwitchOff,
    /// Beide Sprachen bekannt, aber verschiedene Sprachgruppen.
    LanguageMismatch,
    /// Mindestens eine Sprache ist unbekannt.
    LanguageUnknown,
}

impl RaidBlock {
    /// Kurzer Grund für Logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NetworkSwitchOff => "netzwerk_schalter_aus",
            Self::LanguageMismatch => "andere_sprache",
            Self::LanguageUnknown => "sprache_unbekannt",
        }
    }
}

/// Darf `raider` den Partner `target` raiden? Die Sprachgruppen kommen
/// bereits aufgelöst herein (siehe [`StreamerNetwork::language_group`]).
pub fn raid_reach(
    raider: &StreamerNetwork,
    raider_language: Option<&str>,
    target: &StreamerNetwork,
    target_language: Option<&str>,
) -> Result<RaidReach, RaidBlock> {
    if raider.shares_guild_with(target) {
        return Ok(RaidReach::SameServer);
    }
    if !raider.allows_cross_server_raids() || !target.allows_cross_server_raids() {
        return Err(RaidBlock::NetworkSwitchOff);
    }
    match (raider_language, target_language) {
        (Some(mine), Some(theirs)) if mine == theirs => Ok(RaidReach::CrossServer),
        (Some(_), Some(_)) => Err(RaidBlock::LanguageMismatch),
        _ => Err(RaidBlock::LanguageUnknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN_GUILD: i64 = 42;
    const EN_GUILD_2: i64 = 43;

    fn guild(guild_id: i64, language: &str, cross: bool) -> GuildSettings {
        GuildSettings {
            guild_id,
            language: language.to_string(),
            cross_server_raids: cross,
            promo_invite_url: None,
            installed: true,
        }
    }

    fn member(guild: GuildSettings) -> GuildMembership {
        GuildMembership {
            guild,
            active: true,
        }
    }

    fn community() -> GuildSettings {
        GuildSettings::community_default()
    }

    fn network(memberships: Vec<GuildMembership>, language: Option<&str>) -> StreamerNetwork {
        StreamerNetwork::new(memberships, language.map(str::to_string), &community())
    }

    #[test]
    fn sprachgruppe_nimmt_den_primaer_subtag() {
        assert_eq!(language_group("de").as_deref(), Some("de"));
        assert_eq!(language_group(" DE-at ").as_deref(), Some("de"));
        assert_eq!(language_group("de_CH").as_deref(), Some("de"));
        assert_eq!(language_group("en-GB").as_deref(), Some("en"));
        assert_eq!(language_group("en-us").as_deref(), Some("en"));
        assert_eq!(language_group("zh-hk").as_deref(), Some("zh"));
    }

    #[test]
    fn sprachgruppe_verwirft_leere_und_unbestimmte_werte() {
        assert_eq!(language_group(""), None);
        assert_eq!(language_group("   "), None);
        assert_eq!(language_group("other"), None);
        assert_eq!(language_group("und"), None);
        assert_eq!(language_group("-de"), None);
        assert_eq!(language_group("d3"), None);
    }

    #[test]
    fn ohne_zeile_gehoert_der_streamer_zum_deutschen_server() {
        let net = network(Vec::new(), None);
        let home = net.home_guild().expect("Heimat-Server");
        assert_eq!(home.guild_id, COMMUNITY_GUILD_ID);
        assert!(net.allows_cross_server_raids());
        assert_eq!(net.language_group(None).as_deref(), Some("de"));
    }

    #[test]
    fn heimat_server_ist_der_frueheste_aktive() {
        let net = network(
            vec![
                GuildMembership {
                    guild: guild(EN_GUILD, "en", true),
                    active: false,
                },
                member(guild(EN_GUILD_2, "en", true)),
                member(community()),
            ],
            None,
        );
        assert_eq!(net.home_guild().map(|g| g.guild_id), Some(EN_GUILD_2));
    }

    #[test]
    fn entfernter_server_zaehlt_nicht_als_mitgliedschaft() {
        let mut removed = guild(EN_GUILD, "en", true);
        removed.installed = false;
        let net = network(vec![member(removed)], None);
        assert!(net.home_guild().is_none());
        assert!(!net.allows_cross_server_raids());
    }

    #[test]
    fn nur_inaktive_zeilen_ergeben_keinen_heimat_server() {
        let net = network(
            vec![GuildMembership {
                guild: community(),
                active: false,
            }],
            Some("de"),
        );
        assert!(net.home_guild().is_none());
        assert_eq!(net.language_group(None).as_deref(), Some("de"));
    }

    #[test]
    fn sprache_kommt_aus_stream_dann_kanal_dann_server() {
        let net = network(vec![member(guild(EN_GUILD, "en", true))], Some("fr"));
        assert_eq!(net.language_group(Some("es")).as_deref(), Some("es"));
        assert_eq!(net.language_group(Some("other")).as_deref(), Some("fr"));
        assert_eq!(net.language_group(None).as_deref(), Some("fr"));

        let ohne_kanal = network(vec![member(guild(EN_GUILD, "en-GB", true))], Some(" "));
        assert_eq!(ohne_kanal.language_group(None).as_deref(), Some("en"));
    }

    #[test]
    fn gleicher_server_erlaubt_raid_unabhaengig_von_sprache() {
        let raider = network(Vec::new(), Some("de"));
        let target = network(Vec::new(), Some("en"));
        assert_eq!(
            raid_reach(&raider, Some("de"), &target, Some("en")),
            Ok(RaidReach::SameServer)
        );
    }

    #[test]
    fn gleicher_server_gilt_auch_bei_ausgeschaltetem_netzwerk() {
        let raider = network(vec![member(guild(EN_GUILD, "en", false))], None);
        let target = network(vec![member(guild(EN_GUILD, "en", false))], None);
        assert_eq!(
            raid_reach(&raider, None, &target, None),
            Ok(RaidReach::SameServer)
        );
    }

    #[test]
    fn ueber_servergrenzen_nur_mit_gleicher_sprachgruppe() {
        let raider = network(vec![member(guild(EN_GUILD, "en", true))], None);
        let en_target = network(vec![member(guild(EN_GUILD_2, "en", true))], None);
        let de_target = network(Vec::new(), None);
        assert_eq!(
            raid_reach(&raider, Some("en"), &en_target, Some("en")),
            Ok(RaidReach::CrossServer)
        );
        assert_eq!(
            raid_reach(&raider, Some("en"), &de_target, Some("de")),
            Err(RaidBlock::LanguageMismatch)
        );
        assert_eq!(
            raid_reach(&raider, None, &en_target, Some("en")),
            Err(RaidBlock::LanguageUnknown)
        );
    }

    #[test]
    fn netzwerk_schalter_muss_auf_beiden_seiten_an_sein() {
        let an = network(vec![member(guild(EN_GUILD, "en", true))], None);
        let aus = network(vec![member(guild(EN_GUILD_2, "en", false))], None);
        assert_eq!(
            raid_reach(&an, Some("en"), &aus, Some("en")),
            Err(RaidBlock::NetworkSwitchOff)
        );
        assert_eq!(
            raid_reach(&aus, Some("en"), &an, Some("en")),
            Err(RaidBlock::NetworkSwitchOff)
        );
    }

    #[test]
    fn ein_server_mit_schalter_genuegt_bei_mehreren_mitgliedschaften() {
        let raider = network(
            vec![
                member(guild(EN_GUILD, "en", false)),
                member(guild(EN_GUILD_2, "en", true)),
            ],
            None,
        );
        let target = network(vec![member(guild(77, "en", true))], None);
        assert_eq!(
            raid_reach(&raider, Some("en"), &target, Some("en")),
            Ok(RaidReach::CrossServer)
        );
    }

    #[test]
    fn werbe_einladung_ignoriert_leere_werte() {
        let mut settings = community();
        settings.promo_invite_url = Some("  ".to_string());
        assert_eq!(settings.promo_invite(), None);
        settings.promo_invite_url = Some(" https://discord.gg/abc ".to_string());
        assert_eq!(settings.promo_invite(), Some("https://discord.gg/abc"));
    }
}

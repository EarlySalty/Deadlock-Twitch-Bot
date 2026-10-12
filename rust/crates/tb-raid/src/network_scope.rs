//! Raid-Ziele nach Server-Netzwerk und Sprache filtern (Multi-Server-Plan,
//! Paket R). Reine Funktion; die Netzwerkdaten lädt der Aufrufer
//! (`tb_db::network`), die Regel steht in [`tb_domain::network::raid_reach`].
//!
//! Partner desselben Servers bleiben immer im Pool. Über Servergrenzen nur,
//! wenn beide Seiten den Netzwerk-Schalter an haben und dieselbe Sprachgruppe
//! sprechen. Für den deutschen Server allein ändert sich dadurch nichts.

use std::collections::HashMap;

use tb_domain::network::{
    raid_reach, RaidBlock, RaidReach, StreamerNetwork, DEFAULT_LANGUAGE_GROUP,
};

use crate::partner_roster::OnlineCandidate;

/// Ein ausgeschiedener Kandidat samt Grund (für Logs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkDrop {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub reason: RaidBlock,
}

/// Ergebnis des Netzwerkfilters.
#[derive(Debug, Clone, Default)]
pub struct NetworkFilterOutcome {
    /// Erlaubte Kandidaten in Eingabereihenfolge.
    pub kept: Vec<OnlineCandidate>,
    /// Davon über Servergrenzen erreicht.
    pub cross_server: usize,
    /// Ausgeschiedene Kandidaten.
    pub dropped: Vec<NetworkDrop>,
}

/// Filtert die Online-Partner für einen Raider. `raider_language` ist die
/// bereits aufgelöste Sprachgruppe des Raiders. Kandidaten ohne Eintrag in
/// `networks` bleiben wie bisher im Pool (der Lader liefert jede ID; das ist
/// nur die Rückfallebene).
pub fn filter_candidates_by_network(
    raider: &StreamerNetwork,
    raider_language: Option<&str>,
    candidates: Vec<OnlineCandidate>,
    networks: &HashMap<String, StreamerNetwork>,
) -> NetworkFilterOutcome {
    let mut outcome = NetworkFilterOutcome::default();
    for candidate in candidates {
        let Some(target) = networks.get(candidate.twitch_user_id.trim()) else {
            outcome.kept.push(candidate);
            continue;
        };
        let target_language = target.language_group(candidate.stream.language.as_deref());
        match raid_reach(raider, raider_language, target, target_language.as_deref()) {
            Ok(RaidReach::SameServer) => outcome.kept.push(candidate),
            Ok(RaidReach::CrossServer) => {
                outcome.cross_server += 1;
                outcome.kept.push(candidate);
            }
            Err(reason) => outcome.dropped.push(NetworkDrop {
                twitch_user_id: candidate.twitch_user_id,
                twitch_login: candidate.twitch_login,
                reason,
            }),
        }
    }
    outcome
}

/// Sprache des Kategorie-Ersatzes: die Sprachgruppe des Raiders, sonst der
/// bisher feste Wert `de`.
pub fn fallback_language(raider_language: Option<&str>) -> String {
    raider_language
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .unwrap_or(DEFAULT_LANGUAGE_GROUP)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partner_roster::StreamData;
    use tb_domain::network::{GuildMembership, GuildSettings};

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

    fn net(guilds: Vec<GuildSettings>, language: Option<&str>) -> StreamerNetwork {
        StreamerNetwork::new(
            guilds
                .into_iter()
                .map(|guild| GuildMembership {
                    guild,
                    active: true,
                })
                .collect(),
            language.map(str::to_string),
            &GuildSettings::community_default(),
        )
    }

    fn candidate(id: &str, live_language: Option<&str>) -> OnlineCandidate {
        OnlineCandidate {
            twitch_user_id: id.to_string(),
            twitch_login: format!("login_{id}"),
            raid_enabled: true,
            stream: StreamData {
                language: live_language.map(str::to_string),
                ..StreamData::default()
            },
        }
    }

    fn logins(outcome: &NetworkFilterOutcome) -> Vec<&str> {
        outcome
            .kept
            .iter()
            .map(|c| c.twitch_login.as_str())
            .collect()
    }

    #[test]
    fn deutscher_server_behaelt_alle_partner_unabhaengig_von_sprache() {
        let raider = net(Vec::new(), Some("de"));
        let networks = HashMap::from([
            ("1".to_string(), net(Vec::new(), Some("de"))),
            ("2".to_string(), net(Vec::new(), Some("en"))),
            ("3".to_string(), net(Vec::new(), None)),
        ]);
        let outcome = filter_candidates_by_network(
            &raider,
            Some("de"),
            vec![
                candidate("1", Some("de")),
                candidate("2", Some("en")),
                candidate("3", Some("other")),
            ],
            &networks,
        );
        assert_eq!(logins(&outcome), vec!["login_1", "login_2", "login_3"]);
        assert_eq!(outcome.cross_server, 0);
        assert!(outcome.dropped.is_empty());
    }

    #[test]
    fn ueber_servergrenzen_nur_gleiche_sprachgruppe_mit_schalter() {
        let raider = net(vec![guild(EN_GUILD, "en", true)], Some("en"));
        let networks = HashMap::from([
            // Anderer englischer Server mit Netzwerk: erlaubt.
            (
                "1".to_string(),
                net(vec![guild(EN_GUILD_2, "en", true)], None),
            ),
            // Deutscher Server: andere Sprache.
            ("2".to_string(), net(Vec::new(), Some("de"))),
            // Englischer Server ohne Netzwerk-Schalter.
            ("3".to_string(), net(vec![guild(77, "en", false)], None)),
            // Gleicher Server, andere Sprache: erlaubt.
            (
                "4".to_string(),
                net(vec![guild(EN_GUILD, "en", true)], Some("fr")),
            ),
        ]);
        let outcome = filter_candidates_by_network(
            &raider,
            Some("en"),
            vec![
                candidate("1", Some("en-GB")),
                candidate("2", Some("de")),
                candidate("3", Some("en")),
                candidate("4", Some("fr")),
            ],
            &networks,
        );
        assert_eq!(logins(&outcome), vec!["login_1", "login_4"]);
        assert_eq!(outcome.cross_server, 1);
        assert_eq!(
            outcome.dropped,
            vec![
                NetworkDrop {
                    twitch_user_id: "2".to_string(),
                    twitch_login: "login_2".to_string(),
                    reason: RaidBlock::LanguageMismatch,
                },
                NetworkDrop {
                    twitch_user_id: "3".to_string(),
                    twitch_login: "login_3".to_string(),
                    reason: RaidBlock::NetworkSwitchOff,
                },
            ]
        );
    }

    #[test]
    fn live_sprache_des_kandidaten_hat_vorrang() {
        let raider = net(vec![guild(EN_GUILD, "en", true)], Some("en"));
        // Gespeichert de, aber der laufende Stream ist englisch.
        let networks = HashMap::from([("1".to_string(), net(Vec::new(), Some("de")))]);
        let outcome = filter_candidates_by_network(
            &raider,
            Some("en"),
            vec![candidate("1", Some("en"))],
            &networks,
        );
        assert_eq!(logins(&outcome), vec!["login_1"]);
        assert_eq!(outcome.cross_server, 1);
    }

    #[test]
    fn kandidat_ohne_netzwerkdaten_bleibt_im_pool() {
        let raider = net(vec![guild(EN_GUILD, "en", true)], Some("en"));
        let outcome = filter_candidates_by_network(
            &raider,
            Some("en"),
            vec![candidate("1", Some("de"))],
            &HashMap::new(),
        );
        assert_eq!(logins(&outcome), vec!["login_1"]);
    }

    #[test]
    fn ersatz_sprache_ist_die_sprachgruppe_sonst_de() {
        assert_eq!(fallback_language(Some("en")), "en");
        assert_eq!(fallback_language(Some(" ")), "de");
        assert_eq!(fallback_language(None), "de");
    }
}

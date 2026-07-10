//! Kuratierter, user-sichtbarer Befehls-Katalog — einzige Quelle für die
//! `!commands`-Chat-Antwort, die /streamer/commands-Seite und das Deadlock-Gate.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGroup {
    Stats,
    Match,
    Mod,
    Fun,
}

impl CommandGroup {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommandGroup::Stats => "stats",
            CommandGroup::Match => "match",
            CommandGroup::Mod => "mod",
            CommandGroup::Fun => "fun",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CommandGroup::Stats => "Statistik",
            CommandGroup::Match => "Match",
            CommandGroup::Mod => "Moderation",
            CommandGroup::Fun => "Sonstiges",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CommandInfo {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub group: CommandGroup,
    pub deadlock_only: bool,
    pub summary: &'static str,
}

pub fn catalog() -> &'static [CommandInfo] {
    use CommandGroup::*;
    &[
        CommandInfo {
            name: "!rank",
            aliases: &[],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt deinen aktuellen Deadlock-Rang im Chat.",
        },
        CommandInfo {
            name: "!wins",
            aliases: &[],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt deine Deadlock-Karriere-Siege im Chat.",
        },
        CommandInfo {
            name: "!winrate",
            aliases: &[],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt deine Deadlock-Winrate der letzten Spiele.",
        },
        CommandInfo {
            name: "!mmr",
            aliases: &["!climb"],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt deinen aktuellen Rang und Trend der letzten Tage.",
        },
        CommandInfo {
            name: "!live",
            aliases: &[],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt, ob du gerade live in einem Deadlock-Match bist.",
        },
        CommandInfo {
            name: "!lastmatch",
            aliases: &["!last"],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt dein letztes Deadlock-Spiel (Ergebnis, Hero, KDA).",
        },
        CommandInfo {
            name: "!streak",
            aliases: &[],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt deine aktuelle Sieges- oder Pechsträhne.",
        },
        CommandInfo {
            name: "!mostplayed",
            aliases: &["!main"],
            group: Stats,
            deadlock_only: true,
            summary: "Zeigt deinen meistgespielten Hero der letzten Spiele.",
        },
        CommandInfo {
            name: "!clip",
            aliases: &["!createclip"],
            group: Fun,
            deadlock_only: true,
            summary: "Erstellt einen Clip vom aktuellen Stream.",
        },
        CommandInfo {
            name: "!invite",
            aliases: &[],
            group: Fun,
            deadlock_only: true,
            summary: "Postet den Einladungslink zur Community.",
        },
        CommandInfo {
            name: "!discord",
            aliases: &["!dldc", "!dlde"],
            group: Fun,
            deadlock_only: true,
            summary: "Postet den Einladungslink zur Deutschen Deadlock Community.",
        },
        CommandInfo {
            name: "!commands",
            aliases: &[],
            group: Fun,
            deadlock_only: false,
            summary: "Zeigt die Befehle im Chat, plus Link zur vollen Übersicht.",
        },
        CommandInfo {
            name: "!help",
            aliases: &[],
            group: Fun,
            deadlock_only: false,
            summary: "Kurzerklärung zu einem Feature: !help <thema>.",
        },
        CommandInfo {
            name: "!ping",
            aliases: &["!health", "!status", "!bot"],
            group: Fun,
            deadlock_only: false,
            summary: "Prüft, ob der Bot gerade antwortet.",
        },
        CommandInfo {
            name: "!engagement_ignore_me",
            aliases: &[],
            group: Fun,
            deadlock_only: false,
            summary: "Nimmt dich aus dem Engagement-Tracking raus.",
        },
        CommandInfo {
            name: "!engagement_remember_me",
            aliases: &[],
            group: Fun,
            deadlock_only: false,
            summary: "Nimmt dich wieder ins Engagement-Tracking auf.",
        },
        CommandInfo {
            name: "!raid",
            aliases: &["!traid"],
            group: Mod,
            deadlock_only: true,
            summary: "Startet einen Raid zu einem Deadlock-Streamer (Mods/Broadcaster).",
        },
        CommandInfo {
            name: "!title",
            aliases: &["!titel"],
            group: Mod,
            deadlock_only: true,
            summary: "Schlägt einen Stream-Titel vor: !title <stichworte>.",
        },
        CommandInfo {
            name: "!raid_status",
            aliases: &["!raidbot_status"],
            group: Mod,
            deadlock_only: false,
            summary: "Zeigt Auto-Raid-Status und grobe Raid-Statistik.",
        },
        CommandInfo {
            name: "!raid_history",
            aliases: &["!raidbot_history"],
            group: Mod,
            deadlock_only: false,
            summary: "Zeigt die letzten Raids.",
        },
        CommandInfo {
            name: "!uban",
            aliases: &["!unban"],
            group: Mod,
            deadlock_only: false,
            summary: "Nimmt den letzten Auto-Ban zurück.",
        },
        CommandInfo {
            name: "!explain",
            aliases: &[],
            group: Mod,
            deadlock_only: false,
            summary: "Erklärt, warum der Bot jemanden als Scam eingestuft hat.",
        },
        CommandInfo {
            name: "!silentban",
            aliases: &[],
            group: Mod,
            deadlock_only: false,
            summary: "Schaltet Chat-Hinweise zu Auto-Bans um.",
        },
        CommandInfo {
            name: "!silentraid",
            aliases: &[],
            group: Mod,
            deadlock_only: false,
            summary: "Schaltet Chat-Hinweise zu Raids um.",
        },
        CommandInfo {
            name: "!lurkersteuer_off",
            aliases: &["!lurkersteuer_aus", "!lurker_tax_off"],
            group: Mod,
            deadlock_only: false,
            summary: "Deaktiviert die Lurker-Erinnerung (nur Broadcaster).",
        },
        CommandInfo {
            name: "!engagement_status",
            aliases: &[],
            group: Mod,
            deadlock_only: false,
            summary: "Zeigt, ob Engagement-Tracking im Kanal aktiv ist.",
        },
        CommandInfo {
            name: "!engagement_on",
            aliases: &[],
            group: Mod,
            deadlock_only: false,
            summary: "Schaltet Engagement-Tracking für den Kanal ein.",
        },
        CommandInfo {
            name: "!engagement_off",
            aliases: &[],
            group: Mod,
            deadlock_only: false,
            summary: "Schaltet Engagement-Tracking für den Kanal aus.",
        },
    ]
}

pub fn deadlock_only(cmd: &str) -> bool {
    let cmd = cmd.trim();
    cmd.starts_with('!')
        && catalog().iter().any(|c| {
            c.deadlock_only
                && (c.name.eq_ignore_ascii_case(cmd)
                    || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(cmd)))
        })
}

pub fn grouped() -> Vec<(CommandGroup, Vec<&'static CommandInfo>)> {
    use CommandGroup::*;
    [Stats, Match, Mod, Fun]
        .iter()
        .map(|g| {
            let items: Vec<&'static CommandInfo> =
                catalog().iter().filter(|c| c.group == *g).collect();
            (*g, items)
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn katalog_eindeutig_und_valide() {
        let cat = catalog();
        assert!(!cat.is_empty());
        for c in cat {
            assert!(c.name.starts_with('!'));
            assert!(!c.summary.trim().is_empty());
            for alias in c.aliases {
                assert!(alias.starts_with('!'));
            }
        }
    }

    #[test]
    fn kein_name_oder_alias_doppelt() {
        let mut commands: Vec<&str> = catalog()
            .iter()
            .flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied()))
            .collect();
        commands.sort_unstable();

        let duplicates: Vec<&str> = commands
            .windows(2)
            .filter_map(|w| (w[0] == w[1]).then_some(w[0]))
            .collect();

        assert!(
            duplicates.is_empty(),
            "Name/Alias mehrfach im Katalog registriert: {duplicates:?}"
        );
    }

    #[test]
    fn alle_dispatch_commands_sind_im_katalog_registriert() {
        let registered: Vec<&str> = catalog()
            .iter()
            .flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied()))
            .collect();
        let mut missing: Vec<&str> = include_str!("commands.rs")
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("\"!") && line.contains("=>"))
            .flat_map(|line| {
                line.split('"')
                    .enumerate()
                    .filter_map(|(i, part)| (i % 2 == 1 && part.starts_with('!')).then_some(part))
            })
            .filter(|cmd| !registered.contains(cmd))
            .collect();
        missing.sort_unstable();
        missing.dedup();

        assert!(
            missing.is_empty(),
            "Dispatch-Commands fehlen im Katalog: {missing:?}"
        );
    }

    #[test]
    fn deadlock_only_findet_name_und_alias() {
        assert!(deadlock_only("!rank"));
        assert!(deadlock_only("!climb"));
        assert!(deadlock_only("!discord"));
        assert!(!deadlock_only("!commands"));
        assert!(!deadlock_only("!uban"));
        assert!(!deadlock_only("!unbekannt"));
    }

    #[test]
    fn grouped_summiert_auf_alle() {
        assert_eq!(
            grouped().iter().map(|(_, v)| v.len()).sum::<usize>(),
            catalog().len()
        );
    }

    #[test]
    fn help_und_commands_im_katalog() {
        let n: Vec<_> = catalog().iter().map(|c| c.name).collect();
        assert!(n.contains(&"!help") && n.contains(&"!commands"));
    }
}

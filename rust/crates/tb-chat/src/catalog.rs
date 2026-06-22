//! Kuratierter, user-sichtbarer Befehls-Katalog — einzige Quelle für die
//! `!commands`-Chat-Antwort und die /streamer/commands-Seite. NICHT jeder
//! interne Befehl steht hier.

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
    pub group: CommandGroup,
    pub summary: &'static str,
}

pub fn catalog() -> &'static [CommandInfo] {
    use CommandGroup::*;
    &[
        CommandInfo {
            name: "!rank",
            group: Stats,
            summary: "Zeigt deinen aktuellen Deadlock-Rang im Chat.",
        },
        CommandInfo {
            name: "!wins",
            group: Stats,
            summary: "Zeigt deine Deadlock-Karriere-Siege im Chat.",
        },
        CommandInfo {
            name: "!winrate",
            group: Stats,
            summary: "Zeigt deine Deadlock-Winrate der letzten Spiele.",
        },
        CommandInfo {
            name: "!mmr",
            group: Stats,
            summary: "Zeigt deinen aktuellen Rang und Trend der letzten Tage.",
        },
        CommandInfo {
            name: "!live",
            group: Stats,
            summary: "Zeigt, ob du gerade live in einem Deadlock-Match bist.",
        },
        CommandInfo {
            name: "!lastmatch",
            group: Stats,
            summary: "Zeigt dein letztes Deadlock-Spiel (Ergebnis, Hero, KDA).",
        },
        CommandInfo {
            name: "!streak",
            group: Stats,
            summary: "Zeigt deine aktuelle Sieges- oder Pechsträhne.",
        },
        CommandInfo {
            name: "!mostplayed",
            group: Stats,
            summary: "Zeigt deinen meistgespielten Hero der letzten Spiele.",
        },
        CommandInfo {
            name: "!commands",
            group: Fun,
            summary: "Liste aller Bot-Befehle (Link zur Übersicht).",
        },
        CommandInfo {
            name: "!help",
            group: Fun,
            summary: "Kurzerklärung zu einem Feature: !help <thema>.",
        },
        CommandInfo {
            name: "!clip",
            group: Fun,
            summary: "Erstellt einen Clip vom aktuellen Stream.",
        },
        CommandInfo {
            name: "!raid",
            group: Mod,
            summary: "Startet einen Raid zu einem Deadlock-Streamer (Mods/Broadcaster).",
        },
        CommandInfo {
            name: "!invite",
            group: Fun,
            summary: "Postet den Einladungslink zur Community.",
        },
    ]
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
        let mut n: Vec<_> = cat.iter().map(|c| c.name).collect();
        n.sort();
        let before = n.len();
        n.dedup();
        assert_eq!(before, n.len(), "Namen eindeutig");
        for c in cat {
            assert!(c.name.starts_with('!'));
            assert!(!c.summary.trim().is_empty());
        }
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

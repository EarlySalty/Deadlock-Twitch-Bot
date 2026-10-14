#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DashboardLanguage {
    #[default]
    De,
    En,
}

impl DashboardLanguage {
    pub const HEADER: &'static str = "x-dashboard-language";

    pub fn from_header_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some(raw) if raw.eq_ignore_ascii_case("en") => Self::En,
            _ => Self::De,
        }
    }

    pub fn text(self, text: &str) -> &str {
        if self == Self::De {
            return text;
        }
        match text {
            "Abo-Link für diesen Kanal. Freiwillige Erinnerung: !sub erinnerung an/aus/status." => "Subscription link for this channel. Optional reminder: !sub erinnerung an/aus/status.",
            "Adresse und Stream-Key gehören zusammen: entweder beide oder keins von beidem." => "The address and stream key go together: provide both or neither.",
            "Aktive Community" => "Active community",
            "Aktuelle Sieges- oder Pechsträhne. Optional: !streak @user." => "Current winning or losing streak. Optional: !streak @user.",
            "Alles aus Netzwerk Plus" => "Everything in Netzwerk Plus",
            "Alles aus Netzwerk Plus, dazu Vorrang bei Support und neuen Funktionen." => "Everything in Netzwerk Plus, plus priority for support and new features.",
            "Auto-Raid in beide Richtungen" => "Auto raid in both directions",
            "Bitte einen Befehlsnamen eingeben." => "Please enter a command name.",
            "Chat-Daten unzureichend" => "Insufficient chat data",
            "Dauerhaft kostenlos: Auto-Raid, Chat-Schutz und die Tagesform deines letzten Streams." => "Free for good: auto raid, chat protection and a snapshot of your last stream.",
            "Deadlock-Karriere-Siege des Streamers oder mit !wins @user einer anderen Person." => "The streamer’s career wins in Deadlock, or !wins @user for someone else.",
            "Deadlock-Rang: !rank me für dich, !rank @user für andere; alternativ !rank steam:<ID>." => "Deadlock rank: !rank me for yourself, !rank @user for others, or !rank steam:<ID>.",
            "Deadlock-Winrate der letzten Spiele. Optional: !winrate @user." => "Deadlock win rate over recent games. Optional: !winrate @user.",
            "Deaktiviert die Lurker-Erinnerung (nur Broadcaster)." => "Disables the lurker reminder (broadcaster only).",
            "Dein Channel profitiert stark vom Community-Oekoystem. Viele Zuschauer kennst du aus anderen Channels." => "Your channel benefits greatly from the community ecosystem. Many viewers also watch other channels.",
            "Dein voller Verlauf, Zeitraumvergleiche und die komplette KI-Auswertung." => "Your full history, period comparisons and the complete AI analysis.",
            "Deine Community ist stark eigenstaendig - die meisten Chatter sind nur in deinem Channel aktiv." => "Your community is largely independent. Most chatters are only active in your channel.",
            "Der Befehlsname ist zu lang." => "The command name is too long.",
            "Entfernt deine eigene direkte Steam-Zuordnung und deaktiviert automatische Zuordnung." => "Removes your own direct Steam link and disables automatic linking.",
            "Entweder eine Stufe oder eigene Werte, nicht beides." => "Choose a preset or custom values, not both.",
            "Erfasste Zuschauerzeit hier im Kanal. Mit !watchtime @user für eine andere Person." => "Recorded watch time in this channel. Use !watchtime @user for someone else.",
            "Erinnere alle 20-30 Minuten an Follow mit konkretem Grund." => "Give viewers a specific reason to follow every 20 to 30 minutes.",
            "Erklärt, warum der Bot jemanden als Scam eingestuft hat." => "Explains why the bot classified someone as a scammer.",
            "Erlaubt sind nur Buchstaben und Zahlen. Das ! steht bereits davor." => "Only letters and numbers are allowed. The ! is already included.",
            "Erstellt einen Clip vom aktuellen Stream." => "Creates a clip of the current stream.",
            "Follower-Verlust" => "Follower loss",
            "Follower-Verlust! Prufe ob Content-Wechsel oder lange Pausen Unfollows verursachen." => "Losing followers! Check whether content changes or long breaks cause unfollows.",
            "Go-Live-Post im Community-Discord" => "Go-live post in the community Discord",
            "Gute Mischung: Ein Teil deiner Zuschauer kommt aus der Deadlock-Community, viele sind aber deine eigenen." => "A good mix: some viewers come from the Deadlock community, while many are your own.",
            "KI-Analyse, KI-Chat, Coaching und KI-Wochenreport" => "AI analysis, AI chat, coaching and weekly AI report",
            "Kein Ziel angegeben." => "No destination provided.",
            "Keine Chatter-Daten verfuegbar." => "No chatter data available.",
            "Keine Änderung angegeben." => "No changes provided.",
            "Kompletter Chat-Schutz und alle Chat-Befehle" => "Full chat protection and all chat commands",
            "Kurzerklärung zu einem Feature: !help <thema>." => "Brief explanation of a feature: !help <thema>.",
            "Langsames Follower-Wachstum" => "Slow follower growth",
            "Laufendes Deadlock-Match des Streamers oder mit !live @user einer anderen Person." => "The streamer’s current Deadlock match, or !live @user for someone else.",
            "Letztes Deadlock-Spiel mit Ergebnis, Hero und KDA. Optional: !lastmatch @user." => "Last Deadlock game with result, hero and KDA. Optional: !lastmatch @user.",
            "Meistgespielter Hero der letzten Spiele. Optional: !mostplayed @user." => "Most played hero over recent games. Optional: !mostplayed @user.",
            "Niedrige Chat-Aktivitat" => "Low chat activity",
            "Niedrige Retention" => "Low retention",
            "Nimmt den letzten Auto-Ban zurück." => "Reverses the last automatic ban.",
            "Nimmt dich aus den Engagement-Antworten des Bots heraus." => "Opts you out of the bot’s engagement replies.",
            "Nimmt dich wieder in die Engagement-Antworten des Bots auf." => "Opts you back into the bot’s engagement replies.",
            "Overlay-Builder und Sendeplanung" => "Overlay builder and stream scheduling",
            "Plattform fehlt" => "Missing platform",
            "Postet den Einladungslink zur Community." => "Posts the community invite link.",
            "Postet den Einladungslink zur Deutschen Deadlock Community." => "Posts the Deutsche Deadlock Community invite link.",
            "Privater VOD-Ausschnitt für den Kanalinhaber: !vodclip 55 58. Mods und Kanalinhaber; im Dashboard einschaltbar." => "Private VOD excerpt for the channel owner: !vodclip 55 58. For moderators and the channel owner; enable it in the dashboard.",
            "Prüft, ob der Bot gerade antwortet." => "Checks whether the bot is responding.",
            "Rang und Trend der letzten Tage. Optional: !mmr @user." => "Rank and trend over recent days. Optional: !mmr @user.",
            "Reicht einen Clip aus diesem Kanal für den Wochen-Contest im Discord ein: !clipcontest <Clip-Link>, ohne Link den neuesten Clip des laufenden Streams (Broadcaster/Mods)." => "Submits a clip from this channel to the weekly Discord contest: !clipcontest <Clip-Link>. Without a link, uses the newest clip from the current stream (broadcaster/moderators).",
            "Retention-Daten unzureichend" => "Insufficient retention data",
            "Sagt dem Chat, dass du still weiter zuschaust." => "Tells chat that you are still watching quietly.",
            "Schaltet Chat-Hinweise zu Auto-Bans um." => "Toggles chat notices about automatic bans.",
            "Schaltet Chat-Hinweise zu Raids um." => "Toggles chat notices about raids.",
            "Schaltet Engagement-Tracking für den Kanal aus." => "Disables engagement tracking for the channel.",
            "Schaltet Engagement-Tracking für den Kanal ein." => "Enables engagement tracking for the channel.",
            "Schlägt einen Stream-Titel vor: !title <stichworte>." => "Suggests a stream title: !title <stichworte>.",
            "Sonstiges" => "Other",
            "Starke Retention" => "Strong retention",
            "Starkes Wachstum" => "Strong growth",
            "Starte mit einem starken Hook in den ersten 2 Minuten." => "Start with a strong hook in the first 2 minutes.",
            "Startet einen Raid zu einem Deadlock-Streamer (Mods/Broadcaster)." => "Starts a raid to a Deadlock streamer (moderators/broadcaster).",
            "Statistik" => "Statistics",
            "Stelle alle 5-10 Minuten eine direkte Frage an den Chat." => "Ask chat a direct question every 5 to 10 minutes.",
            "Tagesform deines letzten Streams" => "Snapshot of your last stream",
            "Twitch-Audiowahl ist ungültig." => "Invalid Twitch audio option.",
            "Twitch-Betriebsart ist ungültig." => "Invalid Twitch output mode.",
            "Verbindet dein eigenes Twitch- und Steam-Konto über unsere Website, ohne Discord." => "Links your own Twitch and Steam accounts through our website, without Discord.",
            "Verlinkt dein Twitch-Dashboard." => "Links to your Twitch dashboard.",
            "Verlinkt die Bewertung der Deutschen Deadlock Community auf Trustpilot." => "Links to the Deutsche Deadlock Community review page on Trustpilot.",
            "Verlinkt die Übersicht aller Befehle." => "Links to the list of all commands.",
            "Voller Verlauf statt nur letztem Stream" => "Full history instead of just your last stream",
            "Vorrang bei Support und neuen Funktionen" => "Priority for support and new features",
            "Werbefreier Chat, Raid-Vorrang und Lurker-Erinnerung" => "Ad-free chat, raid priority and lurker reminder",
            "Zeigt Auto-Raid-Status und grobe Raid-Statistik." => "Shows auto raid status and basic raid statistics.",
            "Zeigt die letzten Raids." => "Shows recent raids.",
            "Zeigt, ob Engagement-Tracking im Kanal aktiv ist." => "Shows whether engagement tracking is active in this channel.",
            "Zeitraumvergleiche und Wachstumskurven" => "Period comparisons and growth curves",
            "Zu wenige Sessions mit >=3 Viewern fur aussagekraftige Chat-Metriken." => "Too few sessions with at least 3 viewers for meaningful chat metrics.",
            "Zu wenige Sessions mit >=3 Viewern fur aussagekraftige Retention-Werte." => "Too few sessions with at least 3 viewers for meaningful retention figures.",
            "unbekannte Plattform" => "Unknown platform",
            "unbekanntes Profil" => "Unknown profile",
            _ => text,
        }
    }

    pub fn pick<T>(self, de: T, en: T) -> T {
        match self {
            Self::De => de,
            Self::En => en,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DashboardLanguage;

    #[test]
    fn header_value_defaults_to_german_and_en_selects_english() {
        assert_eq!(
            DashboardLanguage::from_header_value(None),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("de")),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("fr")),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("")),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("en")),
            DashboardLanguage::En
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some(" EN ")),
            DashboardLanguage::En
        );
    }

    #[test]
    fn pick_returns_the_variant_for_the_language() {
        assert_eq!(DashboardLanguage::De.pick(1, 2), 1);
        assert_eq!(DashboardLanguage::En.pick(1, 2), 2);
    }
}

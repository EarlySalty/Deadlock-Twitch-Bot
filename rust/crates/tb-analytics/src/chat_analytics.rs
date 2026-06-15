//! Chat-Analytics (`/twitch/api/v2/chat-analytics`).
//!
//! Port von `bot/analytics/api_insights.py:_api_v2_chat_analytics` (größte
//! Analytics-Einheit). **Teil 2: der Nachrichten-Klassifikator
//! `_classify_message`** (pure). Snapshot-Loader + Handler-Aggregation folgen.
//!
//! Die Keyword-Listen sind exakt aus der Python-Quelle generiert
//! ([`crate::chat_analytics_lexicon`]).

use crate::chat_analytics_lexicon::*;

/// Klassifiziert eine Chat-Nachricht (Python `_classify_message`).
/// Reihenfolge der Prüfungen ist relevant (erste Übereinstimmung gewinnt).
pub fn classify_message(content: &str) -> &'static str {
    if content.is_empty() {
        return "Other";
    }
    let cl = content.to_lowercase();
    if content.starts_with('!') {
        return "Command";
    }
    if HYPE.iter().any(|w| cl.contains(w)) {
        return "Hype";
    }
    if GREETING.iter().any(|w| cl.contains(w)) {
        return "Greeting";
    }
    // "?" wird im Original-Content geprüft (Python `"?" in content`).
    if content.contains('?') || QUESTION.iter().any(|w| cl.contains(w)) {
        return "Question";
    }
    if FEEDBACK.iter().any(|w| cl.contains(w)) {
        return "Feedback";
    }
    if TECHNICAL.iter().any(|w| cl.contains(w)) {
        return "Technical";
    }
    if SOCIAL.iter().any(|w| cl.contains(w)) {
        return "Social";
    }
    if REACTION.iter().any(|w| cl.contains(w)) {
        return "Reaction";
    }
    if GAME.iter().any(|w| cl.contains(w)) {
        return "Game-Related";
    }
    "Other"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        assert_eq!(classify_message(""), "Other");
        assert_eq!(classify_message("!uptime"), "Command");
        assert_eq!(classify_message("POG das war insane"), "Hype");
        assert_eq!(classify_message("moin"), "Greeting");
        assert_eq!(classify_message("warum lagt das?"), "Question"); // ? → Question
        assert_eq!(classify_message("wie geht es dir"), "Question"); // "wie" ohne ?
        assert_eq!(classify_message("nice play"), "Feedback");
        assert_eq!(classify_message("lag und fps drops"), "Technical");
        assert_eq!(classify_message("danke fuers following"), "Social");
        assert_eq!(classify_message("lol haha"), "Reaction");
        assert_eq!(classify_message("haze build ist gut"), "Game-Related");
        assert_eq!(classify_message("zzz"), "Other");
        // Reihenfolge: Command schlaegt alles.
        assert_eq!(classify_message("!pog"), "Command");
    }
}

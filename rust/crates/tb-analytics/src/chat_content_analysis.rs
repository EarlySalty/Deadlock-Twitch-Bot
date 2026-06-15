//! Chat-Content-Analyse (`/twitch/api/v2/chat-content-analysis`).
//!
//! Port von `bot/analytics/api_chat_deep.py:_load_chat_content_analysis_payload_sync`
//! + die Keyword-Heuristiken. Hero-/Topic-Erkennung, Sentiment-Scoring,
//! Nachrichten-Klassifikation (reaction/greeting/social/smalltalk/community).
//!
//! Die Keyword-Daten liegen exakt aus der Python-Quelle generiert in
//! [`crate::chat_content_lexicon`]. Diese Datei enthält die (selbst geschriebene)
//! Logik. **Teil 1: die pure Detection-Schicht.** Loader + Handler folgen als Teil 2.

use std::sync::LazyLock;

use regex::Regex;

use crate::chat_content_lexicon::*;

/// Python `_WORD_RE = r"[a-z0-9äöüß_+#']+"` (content ist bereits kleingeschrieben).
static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9äöüß_+#']+").unwrap());

/// Tokenisiert kleingeschriebenen Chat-Text (Python `_tokenize_words`).
pub fn tokenize_words(content_lower: &str) -> Vec<&str> {
    WORD_RE.find_iter(content_lower).map(|m| m.as_str()).collect()
}

/// Erwähnte Hero-Keys, dedupliziert, in ALIAS_TO_HERO-Reihenfolge (Python `_detect_heroes`).
pub fn detect_heroes(content_lower: &str) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for (alias, hero) in ALIAS_TO_HERO {
        if content_lower.contains(alias) && !found.contains(hero) {
            found.push(hero);
        }
    }
    found
}

/// Getroffene Topic-Kategorien (Python `_detect_topics`).
pub fn detect_topics(content_lower: &str) -> Vec<&'static str> {
    let mut topics: Vec<&'static str> = Vec::new();
    for (topic, keywords) in TOPIC_KEYWORDS {
        if keywords.iter().any(|kw| content_lower.contains(kw)) {
            topics.push(topic);
        }
    }
    topics
}

fn any_contains(haystacks: &[&str], needle: &str) -> bool {
    haystacks.iter().any(|h| needle.contains(h))
}

fn is_alpha_word(token: &str) -> bool {
    token.chars().any(|ch| ch.is_ascii_lowercase() || matches!(ch, 'ä' | 'ö' | 'ü' | 'ß'))
}

fn count_alpha_words(words: &[&str]) -> usize {
    words.iter().filter(|t| is_alpha_word(t)).count()
}

/// Kurze Emote-/Hype-Nachricht (Python `_is_reaction_message`).
pub fn is_reaction_message(content_lower: &str, words: &[&str]) -> bool {
    let stripped = content_lower.trim();
    if matches!(stripped, "?" | "??" | "!" | "!!") {
        return true;
    }
    if REACTION_PHRASES.iter().any(|p| content_lower.contains(p)) {
        return true;
    }
    // Reine Emoji-/Symbol-Nachrichten (keine alnum-Tokens) sind pure Reaction.
    if words.is_empty() && !stripped.is_empty() {
        return true;
    }
    words.iter().any(|&t| {
        REACTION_TOKENS.contains(&t)
            || EMOTE_PREFIXES.iter().any(|p| t.starts_with(p))
            || EMOTE_SUFFIXES.iter().any(|s| t.ends_with(s))
            || t.starts_with("xd")
            || t.starts_with("haha")
    })
}

/// Bot-/Chat-Command (Python `_is_command_message`).
pub fn is_command_message(content_lower: &str) -> bool {
    content_lower.trim_start().starts_with('!')
}

/// Begrüßung/Verabschiedung (Python `_is_greeting_message`).
pub fn is_greeting_message(content_lower: &str, words: &[&str]) -> bool {
    if GREETING_PHRASES.iter().any(|p| content_lower.contains(p)) {
        return true;
    }
    words.iter().any(|&t| GREETING_TOKENS.contains(&t))
}

/// Social-/Channel-/Meta-Chat (Python `_is_social_message`).
pub fn is_social_message(content_lower: &str) -> bool {
    any_contains(SOCIAL_MARKERS, content_lower)
}

/// Kurze Bestätigungen / leichtes Geplänkel (Python `_is_smalltalk_message`).
pub fn is_smalltalk_message(_content_lower: &str, words: &[&str]) -> bool {
    if words.len() <= 4 && words.iter().any(|&t| SMALLTALK_TOKENS.contains(&t)) {
        return true;
    }
    let alpha = count_alpha_words(words);
    (1..=2).contains(&alpha)
}

/// Community-/Stream-Chat ohne Game-Topic (Python `_looks_like_community_message`).
pub fn looks_like_community_message(content_lower: &str, words: &[&str]) -> bool {
    let alpha = count_alpha_words(words);
    if alpha >= 4 {
        return true;
    }
    if content_lower.contains('?') && alpha >= 2 {
        return true;
    }
    false
}

/// Sentiment: +1 positiv, -1 negativ, 0 neutral (Python `_score_sentiment`).
pub fn score_sentiment(content_lower: &str) -> i32 {
    if content_lower.trim().is_empty() {
        return 0;
    }
    let mut pos = 0i32;
    let mut neg = 0i32;

    // 1) Multi-Wort-Phrasen (Substring).
    for phrase in POSITIVE_PHRASES {
        if content_lower.contains(phrase) {
            pos += 1;
        }
    }
    for phrase in NEGATIVE_PHRASES {
        if content_lower.contains(phrase) {
            neg += 1;
        }
    }

    // 2) Tokenisieren via Whitespace (Python str.split()).
    for token in content_lower.split_whitespace() {
        // 3) Kurze, mehrdeutige Tokens nur als isoliertes Wort.
        if SHORT_POSITIVE.contains(&token) {
            pos += 1;
            continue;
        }
        if SHORT_NEGATIVE.contains(&token) {
            neg += 1;
            continue;
        }
        // 4) Reguläre Wörter (sehr kurze überspringen).
        if token.chars().count() < 2 {
            continue;
        }
        if POSITIVE_WORDS.contains(&token) {
            pos += 1;
        } else if NEGATIVE_WORDS.contains(&token) {
            neg += 1;
        }
    }

    // 5) Mehrheit entscheidet.
    if pos > neg {
        1
    } else if neg > pos {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize() {
        assert_eq!(tokenize_words("hey! <3 lol c++"), vec!["hey", "3", "lol", "c++"]);
        assert_eq!(tokenize_words("schön übel ärgerlich"), vec!["schön", "übel", "ärgerlich"]);
    }

    #[test]
    fn heroes_und_topics() {
        // "talon" → grey_talon (steht in ALIAS-Reihenfolge vor haze).
        assert_eq!(detect_heroes("nice talon und haze play"), vec!["grey_talon", "haze"]);
        assert_eq!(detect_heroes("kein hero hier"), Vec::<&str>::new());
        assert_eq!(detect_topics("the meta is broken, pls nerf"), vec!["meta"]);
        assert_eq!(detect_topics("guter build mit item"), vec!["builds"]);
    }

    #[test]
    fn sentiment() {
        assert_eq!(score_sentiment("gg nice clutch"), 1); // gg(short)+nice+clutch
        assert_eq!(score_sentiment("trash garbage"), -1);
        assert_eq!(score_sentiment("hello world"), 0);
        assert_eq!(score_sentiment("lets go that was so good"), 1); // 2 Phrasen
        assert_eq!(score_sentiment(""), 0);
        assert_eq!(score_sentiment("w"), 1); // SHORT_POSITIVE isoliert
        assert_eq!(score_sentiment("ff"), -1); // SHORT_NEGATIVE isoliert
    }

    #[test]
    fn klassifikation() {
        assert!(is_reaction_message("kekw", &tokenize_words("kekw")));
        assert!(is_reaction_message("??", &tokenize_words("??")));
        assert!(is_reaction_message("xddd", &tokenize_words("xddd"))); // startswith xd
        assert!(is_command_message("!uptime"));
        assert!(!is_command_message("kein command"));
        assert!(is_greeting_message("moin zusammen", &tokenize_words("moin zusammen")));
        assert!(is_social_message("schau auf meinem discord"));
        assert!(is_smalltalk_message("ja", &tokenize_words("ja")));
        assert!(looks_like_community_message("warum macht ihr das alle", &tokenize_words("warum macht ihr das alle")));
        assert!(looks_like_community_message("was geht?", &tokenize_words("was geht?")));
    }
}

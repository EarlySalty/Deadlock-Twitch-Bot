//! Mention-/Marken-Scoring — Erkennung unbekannter @mentions als Spam-Signal.
//!
//! Port von `_score_mention_patterns` (bot/chat/moderation.py Z. 429–478).
//!
//! # Vertrag
//!
//! Freie Funktion [`score_mention_patterns`] — kein Struct-State nötig (alle
//! teureren Lookups kommen über den `resolver`-Trait herein).
//!
//! Ablauf exakt nach moderation.py Z. 429–478:
//! 1. Mentions mit `@([A-Za-z0-9_]{3,25})` extrahieren (moderation.py Z. 271).
//! 2. Host-Mention-Bonus: wenn `allow_host_bonus` UND host in mentions → +1,
//!    reason "Muster: @host mention" (moderation.py Z. 449–451).
//! 3. Kandidaten = alle anderen Mentions (ohne host), dedupliziert, sortiert
//!    (moderation.py Z. 453).
//! 4. Bekannte Channel-Chatter (DB-Rollup-Check via `resolver`) herausfiltern
//!    (moderation.py Z. 458–461).
//! 5. Helix-User-Lookup via `resolver.resolve_existing`: existierende User → kein Signal.
//!    - lookup_ok=true → unaufgelöste Mentions: +1, reason "Muster: @unknown mention"
//!    - lookup_ok=false → Fallback-Heuristik: >= 8 Zeichen, nur a-z0-9 (kein _),
//!      enthält Zahl ODER gemischte Groß/Kleinschreibung → +1,
//!      reason "Muster: @ + random chars (fallback)" (moderation.py Z. 474–476).
//!
//! # Kompatibilität mit SpamContext
//!
//! Das Ergebnis (score, reasons) wird vom Orchestrator in `SpamContext.mention_score`
//! eingetragen (bot.py Z. 1608–1615). Die Funktion selbst bleibt rein — kein
//! SpamContext als Parameter.
//!
//! # WHITELISTED_BOTS
//!
//! Exportierte Konstante aus `bot/core/chat_bots.py` + `bot/chat/constants.py Z. 34`.
//! Wird von der Pipeline (Schritt 2) genutzt um Bot-Nachrichten zu überspringen.

use std::collections::HashSet;

use std::sync::OnceLock;

use async_trait::async_trait;
use regex::Regex;

// ---------------------------------------------------------------------------
// WHITELISTED_BOTS — bot/core/chat_bots.py Z. 8–19 (KNOWN_CHAT_BOTS),
// importiert als WHITELISTED_BOTS via constants.py Z. 34.
// ---------------------------------------------------------------------------

/// Bekannte Service-/Chat-Bot-Accounts — werden von der Pipeline übersprungen.
///
/// Wörtlich aus `bot/core/chat_bots.py` Z. 8–19 (`KNOWN_CHAT_BOTS`), der als
/// `WHITELISTED_BOTS = set(KNOWN_CHAT_BOTS)` in `bot/chat/constants.py Z. 34`
/// re-exportiert wird.
pub const WHITELISTED_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

// ---------------------------------------------------------------------------
// Mention-Extraktion — moderation.py Z. 269–271
// ---------------------------------------------------------------------------

/// Regex für @mentions.
///
/// Port von `_extract_mentions` (moderation.py Z. 271):
/// `r"(?<!\w)@([A-Za-z0-9_]{3,25})\b"` — die `regex`-Crate unterstützt kein
/// Lookbehind. Äquivalent: Match auf `(^|[^\w])@([A-Za-z0-9_]{3,25})\b` und
/// Capture-Gruppe 2 nehmen. Das schließt `user@example.com` aus (hat \w davor).
fn mention_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Gruppe 1: optionaler Nicht-Wort-Char vor @; Gruppe 2: der eigentliche Login
    RE.get_or_init(|| {
        Regex::new(r"(?:^|[^\w])@([A-Za-z0-9_]{3,25})\b").expect("MENTION_RE ist konstant")
    })
}

/// Extrahiert @mentions aus einer Nachricht.
///
/// Port von `_extract_mentions` (moderation.py Z. 269–271).
fn extract_mentions(content: &str) -> Vec<String> {
    mention_re()
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Fallback-Heuristik — moderation.py Z. 274–289
// ---------------------------------------------------------------------------

/// Fallback-Heuristik für Offline-Erkennung zufälliger Mention-Tokens.
///
/// Port von `_looks_like_random_mention_token` (moderation.py Z. 274–289).
/// Bedingungen:
/// - mindestens 8 Zeichen
/// - nur a-zA-Z0-9 (kein `_`)
/// - enthält Zahl ODER gemischte Groß-/Kleinschreibung
fn looks_like_random_mention_token(token: &str) -> bool {
    let normalized = token.trim();
    if normalized.len() < 8 {
        return false;
    }
    // Nur alphanumerisch, kein Underscore (moderation.py Z. 284: `r"[A-Za-z0-9]+"`)
    if !normalized.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let has_digit = normalized.chars().any(|c| c.is_ascii_digit());
    let has_lower = normalized.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = normalized.chars().any(|c| c.is_ascii_uppercase());
    has_digit || (has_lower && has_upper)
}

// ---------------------------------------------------------------------------
// Resolver-Trait — ersetzt _is_known_channel_chatter + _resolve_existing_twitch_users
// ---------------------------------------------------------------------------

/// Port-Trait für die zwei async-Operationen, die `_score_mention_patterns`
/// aus der Bot-Instanz zieht (moderation.py Z. 459, 466).
#[async_trait]
pub trait MentionResolver: Send + Sync {
    /// Prüft ob ein Login als Chatter im Streamer-Kontext bekannt ist (Rollup- oder
    /// Session-Tabelle). Port von `_is_known_channel_chatter` (moderation.py Z. 291–356).
    async fn is_known_chatter(&self, channel_login: &str, mention_login: &str) -> bool;

    /// Löst Logins via Helix auf. Rückgabe: (gefundene Logins, lookup_ok).
    /// Port von `_resolve_existing_twitch_users` (moderation.py Z. 358–427).
    async fn resolve_existing(&self, logins: &[&str]) -> (HashSet<String>, bool);
}

// ---------------------------------------------------------------------------
// score_mention_patterns — die eigentliche freie Funktion
// ---------------------------------------------------------------------------

/// Bewertet Mentions in einer Chat-Nachricht auf verdächtige Muster.
///
/// Port von `_score_mention_patterns` (bot/chat/moderation.py Z. 429–478).
///
/// # Parameter
/// - `text` — Nachrichten-Rohtext.
/// - `host_login` — Login des Kanal-Hosts (ohne `#`/`@`), lowercased.
/// - `allow_host_bonus` — true wenn bereits ein Phrase- oder Fragment-Signal
///   vorliegt (bot.py Z. 1611: `allow_host_bonus=has_phrase_or_fragment_signal`).
/// - `resolver` — Trait-Objekt für Chatter- und Helix-Lookups.
///
/// # Rückgabe
/// `(reasons, score)` — reasons: alle Signale als Strings; score: Summe.
/// (Reihenfolge reasons zuerst orientiert sich an Python `return hits, reasons`,
///  Rust-Seite kehrt das für ergonomischere Destructuring um.)
pub async fn score_mention_patterns(
    text: &str,
    host_login: &str,
    allow_host_bonus: bool,
    resolver: &dyn MentionResolver,
) -> (Vec<String>, i32) {
    let raw = text.trim();
    if raw.is_empty() {
        return (vec![], 0);
    }

    // Schritt 1: Mentions extrahieren (moderation.py Z. 441)
    let mentions: Vec<String> = extract_mentions(raw)
        .into_iter()
        .map(|m| m.to_lowercase())
        .collect();

    if mentions.is_empty() {
        return (vec![], 0);
    }

    let mut hits: i32 = 0;
    let mut reasons: Vec<String> = Vec::new();

    // Host-Login normalisieren (moderation.py Z. 447)
    let normalized_host = host_login.trim().to_lowercase();
    let normalized_host = normalized_host.trim_start_matches(['#', '@']);

    // Schritt 2: Host-Mention-Bonus (moderation.py Z. 449–451)
    if allow_host_bonus
        && !normalized_host.is_empty()
        && mentions.contains(&normalized_host.to_string())
    {
        hits += 1;
        reasons.push("Muster: @host mention".to_string());
    }

    // Schritt 3: Kandidaten = alle anderen Mentions, dedupliziert, sortiert
    // (moderation.py Z. 453: `sorted({m for m in mentions if m != normalized_host})`)
    let candidates: Vec<String> = {
        let mut set: HashSet<&str> = HashSet::new();
        let mut v: Vec<String> = mentions
            .iter()
            .filter(|m| m.as_str() != normalized_host)
            .filter(|m| set.insert(m.as_str()))
            .cloned()
            .collect();
        v.sort();
        v
    };

    if candidates.is_empty() {
        return (reasons, hits);
    }

    // Schritt 4: Bekannte Chatter herausfiltern (moderation.py Z. 458–461)
    let mut maybe_random: Vec<String> = Vec::new();
    for mention in &candidates {
        if resolver.is_known_chatter(normalized_host, mention).await {
            continue;
        }
        maybe_random.push(mention.clone());
    }

    if maybe_random.is_empty() {
        return (reasons, hits);
    }

    // Schritt 5: Helix-User-Lookup (moderation.py Z. 466–476)
    let logins_ref: Vec<&str> = maybe_random.iter().map(|s| s.as_str()).collect();
    let (existing_users, lookup_ok) = resolver.resolve_existing(&logins_ref).await;

    let unresolved: Vec<&str> = maybe_random
        .iter()
        .filter(|m| !existing_users.contains(m.as_str()))
        .map(|s| s.as_str())
        .collect();

    if unresolved.is_empty() {
        return (reasons, hits);
    }

    if lookup_ok {
        // Lookup klappte, Mentions existieren nicht auf Twitch → verdächtig
        hits += 1;
        reasons.push("Muster: @unknown mention".to_string());
    } else if unresolved
        .iter()
        .any(|m| looks_like_random_mention_token(m))
    {
        // Fallback-Heuristik (moderation.py Z. 474–476)
        hits += 1;
        reasons.push("Muster: @ + random chars (fallback)".to_string());
    }

    (reasons, hits)
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // --- Test-Resolver ---

    /// Konfigurierbarer Test-Resolver: bekannte Chatter + bekannte Twitch-User.
    struct TestResolver {
        known_chatters: Vec<(String, String)>, // (channel, login)
        existing_users: HashSet<String>,
        lookup_ok: bool,
    }

    impl TestResolver {
        fn new(
            known_chatters: Vec<(&str, &str)>,
            existing_users: Vec<&str>,
            lookup_ok: bool,
        ) -> Self {
            Self {
                known_chatters: known_chatters
                    .into_iter()
                    .map(|(c, l)| (c.to_string(), l.to_string()))
                    .collect(),
                existing_users: existing_users.iter().map(|s| s.to_string()).collect(),
                lookup_ok,
            }
        }
    }

    #[async_trait]
    impl MentionResolver for TestResolver {
        async fn is_known_chatter(&self, channel_login: &str, mention_login: &str) -> bool {
            self.known_chatters
                .iter()
                .any(|(c, l)| c == channel_login && l == mention_login)
        }

        async fn resolve_existing(&self, logins: &[&str]) -> (HashSet<String>, bool) {
            let found: HashSet<String> = logins
                .iter()
                .filter(|l| self.existing_users.contains(**l))
                .map(|l| l.to_string())
                .collect();
            (found, self.lookup_ok)
        }
    }

    // --- extract_mentions ---

    #[test]
    fn extrahiert_einfachen_mention() {
        let ms = extract_mentions("hey @testuser schau mal");
        assert_eq!(ms, vec!["testuser"]);
    }

    #[test]
    fn extrahiert_mehrere_mentions() {
        let ms = extract_mentions("@alice und @bob");
        assert!(ms.contains(&"alice".to_string()));
        assert!(ms.contains(&"bob".to_string()));
    }

    #[test]
    fn kein_mention_in_url() {
        // Kein vorausgehendes Nicht-Wort-Zeichen: word@example.com → kein Match
        let ms = extract_mentions("user@example.com");
        assert!(ms.is_empty());
    }

    #[test]
    fn zu_kurzer_mention_kein_match() {
        // < 3 Zeichen
        let ms = extract_mentions("@ab schau");
        assert!(ms.is_empty());
    }

    // --- looks_like_random_mention_token ---

    #[test]
    fn zufalls_token_mit_zahl() {
        assert!(looks_like_random_mention_token("abc12345"));
    }

    #[test]
    fn zufalls_token_gemischt() {
        assert!(looks_like_random_mention_token("AbcDefGhi"));
    }

    #[test]
    fn kurzer_token_kein_zufalls_treffer() {
        assert!(!looks_like_random_mention_token("abc123"));
    }

    #[test]
    fn token_mit_underscore_kein_zufalls_treffer() {
        // Underscore → kein reiner alphanumerischer String
        assert!(!looks_like_random_mention_token("abc_12345"));
    }

    #[test]
    fn token_nur_kleinbuchstaben_kein_treffer() {
        assert!(!looks_like_random_mention_token("abcdefghi"));
    }

    // --- score_mention_patterns ---

    #[tokio::test]
    async fn leer_gibt_null_score() {
        let r = TestResolver::new(vec![], vec![], true);
        let (reasons, score) = score_mention_patterns("", "host", false, &r).await;
        assert_eq!(score, 0);
        assert!(reasons.is_empty());
    }

    #[tokio::test]
    async fn kein_mention_gibt_null_score() {
        let r = TestResolver::new(vec![], vec![], true);
        let (_, score) = score_mention_patterns("hallo wie gehts", "host", false, &r).await;
        assert_eq!(score, 0);
    }

    #[tokio::test]
    async fn bekannter_twitch_user_kein_signal() {
        // @existinguser ist auf Twitch bekannt → kein Mention-Score
        let r = TestResolver::new(vec![], vec!["existinguser"], true);
        let (_, score) = score_mention_patterns("hey @existinguser", "host", false, &r).await;
        assert_eq!(score, 0);
    }

    #[tokio::test]
    async fn unbekannter_mention_bei_lookup_ok_plus_1() {
        // User nicht auf Twitch → lookup_ok=true → +1, reason "Muster: @unknown mention"
        let r = TestResolver::new(vec![], vec![], true);
        let (reasons, score) =
            score_mention_patterns("komm auf @xyz99abc12", "host", false, &r).await;
        assert_eq!(score, 1);
        assert!(reasons.iter().any(|r| r == "Muster: @unknown mention"));
    }

    #[tokio::test]
    async fn fallback_heuristik_bei_lookup_fehler() {
        // lookup_ok=false, Token sieht zufällig aus → +1, reason "Muster: @ + random chars (fallback)"
        let r = TestResolver::new(vec![], vec![], false);
        let (reasons, score) =
            score_mention_patterns("komm auf @Abc123Xyz", "host", false, &r).await;
        assert_eq!(score, 1);
        assert!(reasons
            .iter()
            .any(|r| r == "Muster: @ + random chars (fallback)"));
    }

    #[tokio::test]
    async fn host_bonus_nur_wenn_allow_host_bonus() {
        // allow_host_bonus=false → host mention gibt kein Signal
        let r = TestResolver::new(vec![], vec![], true);
        let (reasons, score) =
            score_mention_patterns("hey @streamer1 schau", "streamer1", false, &r).await;
        assert!(
            !reasons.iter().any(|r| r == "Muster: @host mention"),
            "Score: {score}"
        );
    }

    #[tokio::test]
    async fn host_bonus_wenn_allow_host_bonus_true() {
        // allow_host_bonus=true + host in mentions → +1
        let r = TestResolver::new(vec![], vec!["streamer1"], true);
        let (reasons, score) =
            score_mention_patterns("hey @streamer1 schau", "streamer1", true, &r).await;
        assert_eq!(score, 1, "Host-Bonus muss +1 geben");
        assert!(reasons.iter().any(|r| r == "Muster: @host mention"));
    }

    #[tokio::test]
    async fn bekannter_chatter_wird_herausgefiltert() {
        // mention ist bekannter Chatter → kein Signal
        let r = TestResolver::new(vec![("host1", "friendlyuser")], vec![], true);
        let (_, score) = score_mention_patterns("hey @friendlyuser", "host1", false, &r).await;
        assert_eq!(score, 0);
    }

    #[tokio::test]
    async fn kein_fallback_bei_kurzem_token() {
        // Token < 8 Zeichen → Heuristik gibt false → kein Signal bei lookup_ok=false
        let r = TestResolver::new(vec![], vec![], false);
        let (_, score) = score_mention_patterns("hey @abc1234", "host", false, &r).await;
        // abc1234 = 7 Zeichen → kein Heuristik-Treffer
        assert_eq!(score, 0);
    }

    // --- WHITELISTED_BOTS ---

    #[test]
    fn whitelisted_bots_enthält_nightbot() {
        assert!(WHITELISTED_BOTS.contains(&"nightbot"));
    }

    #[test]
    fn whitelisted_bots_enthält_alle_aus_python() {
        // Wörtlich aus bot/core/chat_bots.py Z. 8–19
        let expected = &[
            "botrix",
            "deutschedeadlockcommunity",
            "fossabot",
            "moobot",
            "nightbot",
            "pretzelrocks",
            "soundalerts",
            "streamlabs",
            "streamelements",
            "wizebot",
        ];
        for bot in expected {
            assert!(
                WHITELISTED_BOTS.contains(bot),
                "'{bot}' fehlt in WHITELISTED_BOTS"
            );
        }
        assert_eq!(WHITELISTED_BOTS.len(), expected.len());
    }
}

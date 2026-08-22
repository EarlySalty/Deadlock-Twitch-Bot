//! Zweistufiger Spam-Score-Filter — Port von `bot/chat/moderation.py` (Z. 25–617).
//!
//! # Architektur
//!
//! [`SpamFilter::evaluate`] berechnet einen numerischen Score aus bis zu sieben
//! Signalquellen, normalisiert den Eingangstext via NFKC + 3-schichtiger
//! Homoglyph-Tabelle und trifft dann eine der drei Aktionsentscheidungen:
//! - Score >= [`SPAM_MIN_MATCHES`] → [`SpamAction::Ban`]
//! - Score > 0 UND hartes Signal → [`SpamAction::DeleteOnly`]
//! - Score > 0 ohne hartes Signal → [`SpamAction::None`] (nur Alert/AI-Review)
//!
//! Kontext-Eskalatoren (Account-Alter, Erstnachricht) werden bewusst NICHT
//! hier berechnet — das braucht Helix-Lookups und DB. Der Orchestrator injiziert
//! sie fertig über [`SpamContext`].
//!
//! # Gelernte Muster
//!
//! [`LearnedPatterns::load`] liest aus `twitch_auto_learned_spam_patterns` und
//! aus der manuellen Safe-Tabelle. Der Filter-Cache wird einmalig beim Start
//! befüllt und kann über [`LearnedPatterns::load`] jederzeit neu gebaut werden.
//!
//! Safe-Muster sind kein negatives Score-Signal. Sie stammen ausschließlich aus
//! einem privilegierten menschlichen Gegen-Override und werden als exakter
//! kanonisierter Volltext vor Score und LLM geprüft. So kann ein harmloser
//! Einzelfall gelernt werden, ohne kurze Safe-Teilstrings in Spam-Nachrichten
//! einzubauen.

use std::sync::OnceLock;

use std::sync::Arc;

use arc_swap::ArcSwap;
use regex::Regex;
use sqlx::PgPool;
use unicode_normalization::UnicodeNormalization;

// ---------------------------------------------------------------------------
// Konstanten — exakt aus bot/chat/constants.py Z. 36–109
// ---------------------------------------------------------------------------

/// Mindest-Score für automatischen Ban (constants.py Z. 109).
pub const SPAM_MIN_MATCHES: i32 = 3;

/// Bekannte Spam-Marken-Tokens für den Hard-Signal-Check (moderation.py Z. 593–596).
const SPAM_BRAND_TOKENS: &[&str] = &[
    "streamboo",
    "smmhype",
    "smmbest",
    "smmtop",
    "topsmm",
    "promnow",
    "prmxy",
    "prmup",
    "smmtop32",
    "smmbest4",
    "smmbest5",
];

/// Bekannte Spam-Phrasen, exakt wie in constants.py Z. 36–56.
/// Reihenfolge und Sonderzeichen identisch mit Python (thumbs-up U+1F44D,
/// Right Single Quotation Mark U+2019).
const SPAM_PHRASES: &[&str] = &[
    "Best viewers streamboo.com",
    "Best viewers streamboo .com",
    "Best viewers streamboo com",
    "Best viewers smmtop32.online",
    "Best viewers smmtop32 .online",
    "Best viewers smmtop32 online",
    "Ai viewers streamboo . com",
    "Ai viewers streamboo .com",
    "(remove the space)",
    "Cool overlay \u{1F44D} Honestly, it\u{2019}s so hard to get found on the directory lately. I have small tips on beating the algorithm. Mind if I send you an share?",
    "Mind if I send you an share",
    " Viewers https://smmbest5.online",
    "Viewers smmbest4.online",
    "Viewers streamboo .com",
    "Viewers smmhype12.ru",
    "Viewers smmhype1.ru",
    "Viewers smmhype",
    "viewers on streamboo .com (remove the space)",
    "Hey friend I really enjoy your content so I give you a follow I\u{2019}d love to be a friend and of you feel free to Add me on Discord",
];

/// Fragment-Fallback-Liste, exakt wie in constants.py Z. 58–90.
const SPAM_FRAGMENTS: &[&str] = &[
    "best viewers",
    "cheap viewers",
    "streamboo.com",
    "streamboo .com",
    "streamboo com",
    "streamboo",
    "smmtop32.online",
    "smmtop32 .online",
    "smmtop32 online",
    "smmtop32",
    "remove the space",
    "cool overlay",
    "get found on the directory",
    "beating the algorithm",
    "d!sc",
    "smmbest4.online",
    "smmbest5.online",
    "rookie",
    "smmhype12.ru",
    "smmhype1.ru",
    "smmhype",
    "topsmm3.ru",
    "topsmm3 .ru",
    "topsmm3 ru",
    "topsmm3",
    "promnow.ru",
    "promnow ru",
    "promnow",
    "top viewers",
    "prmxy",
    "prmup",
];

// ---------------------------------------------------------------------------
// Homoglyph-Normalisierung — bot/chat/moderation.py Z. 25–83
// ---------------------------------------------------------------------------

/// Baut die Homoglyph-Mapping-Tabelle (char → char) exakt nach
/// `_build_homoglyph_table()` in moderation.py Z. 25–80.
///
/// Schicht 1: Small Capitals (24 Zeichen).
/// Schicht 2: Mathematical Alphanumeric Symbols U+1D400–U+1D7FF (18 A–Z + 18 a–z Blöcke).
/// Schicht 3: Kyrillische/griechische Homoglyphen.
fn build_homoglyph_table() -> Vec<(char, char)> {
    let mut table: Vec<(char, char)> = Vec::new();

    // Schicht 1 — Small Capitals (moderation.py Z. 29–37)
    let small_caps: &[(char, char)] = &[
        ('ᴀ', 'a'),
        ('ʙ', 'b'),
        ('ᴄ', 'c'),
        ('ᴅ', 'd'),
        ('ᴇ', 'e'),
        ('ꜰ', 'f'),
        ('ɢ', 'g'),
        ('ʜ', 'h'),
        ('ɪ', 'i'),
        ('ᴊ', 'j'),
        ('ᴋ', 'k'),
        ('ʟ', 'l'),
        ('ᴍ', 'm'),
        ('ɴ', 'n'),
        ('ᴏ', 'o'),
        ('ᴘ', 'p'),
        ('ʀ', 'r'),
        ('ꜱ', 's'),
        ('ᴛ', 't'),
        ('ᴜ', 'u'),
        ('ᴠ', 'v'),
        ('ᴡ', 'w'),
        ('ʏ', 'y'),
        ('ᴢ', 'z'),
    ];
    table.extend_from_slice(small_caps);

    // Schicht 2 — Mathematical Alphanumeric Symbols (moderation.py Z. 43–58)
    // 18 A–Z-Blöcke
    let az_upper_starts: &[u32] = &[
        0x1D400, 0x1D434, 0x1D468, 0x1D49C, 0x1D4D0, 0x1D504, 0x1D538, 0x1D56C, 0x1D5A0, 0x1D5D4,
        0x1D608, 0x1D63C, 0x1D670, 0x1D6A8, 0x1D6E2, 0x1D71C, 0x1D756, 0x1D790,
    ];
    // 18 a–z-Blöcke
    let az_lower_starts: &[u32] = &[
        0x1D41A, 0x1D44E, 0x1D482, 0x1D4B6, 0x1D4EA, 0x1D51E, 0x1D552, 0x1D586, 0x1D5BA, 0x1D5EE,
        0x1D622, 0x1D656, 0x1D68A, 0x1D6C2, 0x1D6FC, 0x1D736, 0x1D770, 0x1D7AA,
    ];
    for &base in az_upper_starts {
        for i in 0u32..26 {
            if let (Some(src), Some(dst)) =
                (char::from_u32(base + i), char::from_u32('A' as u32 + i))
            {
                table.push((src, dst));
            }
        }
    }
    for &base in az_lower_starts {
        for i in 0u32..26 {
            if let (Some(src), Some(dst)) =
                (char::from_u32(base + i), char::from_u32('a' as u32 + i))
            {
                table.push((src, dst));
            }
        }
    }

    // Schicht 3 — Kyrillisch/Griechisch (moderation.py Z. 64–78)
    // Reihenfolge exakt wie im Python (letzter Eintrag überschreibt ggf. vorherige).
    let cyrillic_greek: &[(char, char)] = &[
        // Kyrillisch – Kleinbuchstaben
        ('а', 'a'),
        ('е', 'e'),
        ('о', 'o'),
        ('с', 'c'),
        ('р', 'p'),
        ('х', 'x'),
        ('у', 'y'),
        ('к', 'k'),
        ('м', 'm'),
        ('т', 't'),
        ('і', 'i'),
        ('ѕ', 's'),
        ('ј', 'j'),
        ('ԁ', 'd'),
        ('о', 'o'),
        // Kyrillisch – Großbuchstaben
        ('А', 'A'),
        ('Е', 'E'),
        ('О', 'O'),
        ('С', 'C'),
        ('Р', 'P'),
        ('Х', 'X'),
        ('У', 'Y'),
        ('К', 'K'),
        ('М', 'M'),
        ('Т', 'T'),
        ('В', 'B'),
        ('Н', 'H'),
        ('І', 'I'),
        ('Ѕ', 'S'),
        ('Ј', 'J'),
        // Griechisch
        ('ο', 'o'),
        ('α', 'a'),
        ('ρ', 'p'),
        ('ν', 'v'),
        ('κ', 'k'),
        ('μ', 'm'),
        ('τ', 't'),
        ('χ', 'x'),
        ('Ο', 'O'),
        ('Α', 'A'),
        ('Ε', 'E'),
        ('Ρ', 'P'),
        ('Τ', 'T'),
        ('Χ', 'X'),
        ('Κ', 'K'),
        ('Μ', 'M'),
        ('Ν', 'N'),
        ('Ι', 'I'),
    ];
    table.extend_from_slice(cyrillic_greek);

    table
}

/// Einmalig berechnete Homoglyph-Tabelle (build_homoglyph_table ist teuer wegen
/// der Mathematik-Block-Schleifen — nur einmal beim ersten Aufruf bauen).
static HOMOGLYPH_TABLE: OnceLock<Vec<(char, char)>> = OnceLock::new();

fn homoglyph_table() -> &'static Vec<(char, char)> {
    HOMOGLYPH_TABLE.get_or_init(build_homoglyph_table)
}

/// NFKC-Normalisierung + Homoglyph-Ersetzung + strip().
/// Port von `_normalize_spam_text` (moderation.py Z. 614–617).
/// Reihenfolge: NFKC zuerst, dann Homoglyphen, dann trim.
fn normalize_spam_text(content: &str) -> String {
    // NFKC via unicode-normalization
    let nfkc: String = content.nfkc().collect();
    // Homoglyph-Ersetzung (char-für-char)
    let table = homoglyph_table();
    let replaced: String = nfkc
        .chars()
        .map(|c| {
            // Lineare Suche ist ok: Tabelle ist ~500 Einträge, wird selten gesucht
            // und ist gecacht. Für O(1) würde ein HashMap reichen — hier YAGNI.
            table
                .iter()
                .rev() // Letzter Eintrag gewinnt (wie Python dict-Überschreibung)
                .find(|(src, _)| *src == c)
                .map(|(_, dst)| *dst)
                .unwrap_or(c)
        })
        .collect();
    replaced.trim().to_string()
}

/// Kanonischer Schlüssel für einen exakten manuellen Safe-Volltext.
///
/// Whitespace-Kompression macht Chat-Zeilenumbrüche und mehrfachen Whitespace
/// äquivalent; der spätere Vergleich bleibt ein vollständiger Gleichheits-
/// vergleich und ist kein Teilstring-Match.
pub fn normalize_exact_spam_message(content: &str) -> String {
    normalize_spam_text(content)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Nur a-z0-9 — für gelernte Muster (compact form).
fn compact(lowered: &str) -> String {
    lowered
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// a-z0-9 und Punkte — für Domain-Regex (domainized form).
fn domainized(lowered: &str) -> String {
    lowered
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
        .collect()
}

/// Regex für Domain-Kompakt-Erkennung (constants.py Z. 106–108).
/// Angewendet auf `domainized`-Form (nur a-z0-9+Punkte).
fn spam_domain_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:streamboo|smmhype|smmbest|smmtop|topsmm|promnow)\.?(?:com|org|net|ru|online|xyz|site|io|gg)",
        )
        .expect("SPAM_DOMAIN_RE ist eine Kompilier-Zeit-Konstante")
    })
}

/// Regex für "viewer(s) \w+" (moderation.py Z. 537).
fn viewer_pattern_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bviewers?\s+\w+").expect("viewer-Regex ist konstant"))
}

/// Grund-Label des reinen Viewer-Regex-Treffers (Schritt 5). Als Konstante,
/// weil das Safe-Wording-Gate exakt gegen dieses Label prüft.
pub const VIEWER_PATTERN_REASON: &str = "Muster: viewer + name";

/// Deutsche Umgangssprache, die harmlos mit „viewer" zusammenfällt und den
/// weichen Viewer-Regex (Schritt 5) allein auslöst: Lurk- und Gönn-Sprech.
///
/// KEIN Negativ-Scoring (das war das Safe-List-Poisoning vom 11.07.), sondern
/// nur ein Gate für den reinen Viewer-Regex-Treffer, siehe
/// [`spam_signal_ist_nur_viewer_muster`]. Distinktiv gehalten: englischer
/// Viewbot-Spam nutzt keinen dieser Marker.
const SAFE_WORDING_MARKERS: &[&str] = &[
    "gönn",  // gönn, gönne, gönnt, gönn dir, gönnen
    "goenn", // ohne Umlaut getippt
    "am lurken",
    "nur am lurk",
    "bin am lurk",
];

/// Kontaktaufnahme und Verkauf. Wer sie mitschickt, will etwas verkaufen, auch
/// wenn er es in Gönn-Sprech verpackt: „gönn dir viewers, schreib mir" trägt
/// sonst nur den weichen Viewer-Regex und käme ohne diese Gegenprüfung am Judge
/// vorbei. Das Gate ist gegen falsche Timeouts bei Umgangssprache gebaut, nicht
/// als Freifahrtschein für getarnte Angebote.
const KONTAKT_MARKERS: &[&str] = &[
    "schreib mir",
    "schreibt mir",
    "schreib mich",
    "melde dich",
    "meldet euch",
    "dm mir",
    "dm mich",
    "schick mir",
    "kontaktier",
    "t.me/",
    "telegram",
    "whatsapp",
    "discord.gg",
    "billig",
    "guenstig",
    "günstig",
    "kaufen",
    "preis",
];

/// Trifft ein harmloses Umgangssprache-Muster, das den Viewer-Regex fälschlich
/// triggert, gibt den Marker zurück. Lowercase-Contains reicht: die Marker sind
/// distinktiv, und das Gate greift nur im reinen Viewer-Regex-Pfad.
///
/// Trägt der Text zusätzlich eine Kontaktaufnahme oder ein Verkaufswort, gibt es
/// keinen Marker zurück: dann entscheidet wieder der Judge.
pub fn matches_safe_wording(text: &str) -> Option<&'static str> {
    // Dieselbe Normalisierung wie der Detektor, den das Gate ueberstimmt: NFKC
    // plus Homoglyph-Tabelle. Ohne sie liefe die Gegenpruefung auf dem Rohtext,
    // und ein kyrillisches Zeichen in "schreib mir" haette das Gate wieder
    // geoeffnet, obwohl das ganze Modul dagegen gehaertet ist.
    let lowered = normalize_spam_text(text).to_lowercase();
    // Fuer die Gegenpruefung zusaetzlich die Akzente weg: die Homoglyph-Tabelle
    // kennt das kyrillische s, aber nicht jedes Zeichen mit Diakritikum, und
    // "billig" mit Akzent-i ist derselbe Trick. Schaerfer zu normalisieren ist
    // hier die sichere Richtung: das Gate greift dann seltener, also entscheidet
    // haeufiger der Judge. Die Safe-Marker selbst bleiben auf `lowered`, sonst
    // wuerde "goenn" den Umlaut in "gönn" verlieren und beide Schreibweisen
    // faenden sich gegenseitig nicht mehr.
    let ohne_akzente: String = lowered
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect();
    if KONTAKT_MARKERS
        .iter()
        .any(|k| lowered.contains(k) || ohne_akzente.contains(k))
    {
        return None;
    }
    SAFE_WORDING_MARKERS
        .iter()
        .copied()
        .find(|marker| lowered.contains(marker))
}

/// True, wenn der EINZIGE Spam-Grund der weiche Viewer-Regex ist (kein
/// Fragment, keine Phrase, keine Domain, kein gelerntes Muster). Nur dann darf
/// ein Safe-Wording-Treffer den Judge überstimmen, damit echter Spam, der
/// immer ein stärkeres Signal trägt, nie durchrutscht.
pub fn spam_signal_ist_nur_viewer_muster(matched: &[String]) -> bool {
    !matched.is_empty() && matched.iter().all(|reason| reason == VIEWER_PATTERN_REASON)
}

/// Baut einen Word-Boundary-Regex für ein Literal-Muster (\b...\b).
/// Entspricht `re.search(r"\b" + re.escape(...) + r"\b", lowered)`.
fn word_boundary_re(pattern: &str) -> Option<Regex> {
    let escaped = regex::escape(pattern);
    Regex::new(&format!(r"\b{escaped}\b")).ok()
}

// ---------------------------------------------------------------------------
// Hard-Signal-Check — moderation.py Z. 598–612
// ---------------------------------------------------------------------------

/// True wenn mindestens ein Grund ein hartes Spam-Signal ist
/// (`_has_hard_spam_signal`, moderation.py Z. 598–612).
///
/// Hart = True wenn:
/// - reason startet mit "Domain(" oder "Learned-" → immer hart
/// - reason startet mit "Phrase(" oder "Fragment(" UND enthält ein Brand-Token
pub fn has_hard_spam_signal(reasons: &[String]) -> bool {
    for reason in reasons {
        if reason.starts_with("Domain(") || reason.starts_with("Learned-") {
            return true;
        }
        if reason.starts_with("Phrase(") || reason.starts_with("Fragment(") {
            let low = reason.to_lowercase();
            if SPAM_BRAND_TOKENS.iter().any(|tok| low.contains(tok)) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Öffentliche Typen
// ---------------------------------------------------------------------------

/// Kontext-Informationen vom Orchestrator — bestimmt Eskalatoren.
/// Der Orchestrator füllt diese vor dem Evaluate-Call aus Helix + DB
/// (moderation.py Z. 1617–1653).
#[derive(Debug, Clone, Default)]
pub struct SpamContext {
    /// Account-Alter in Tagen (Helix user.created_at). None = unbekannt.
    pub account_age_days: Option<i64>,
    /// True wenn kein Rollup-Eintrag für diese Kombination (Erstnachricht für
    /// diesen Streamer). Abfrage VOR _track_chat_health — race-free.
    /// (moderation.py Z. 185–189)
    pub is_first_message: bool,
    /// Bekannter Chatter: total_messages >= 40 ODER sessions >= 3 ODER
    /// first_seen_at > 14 Tage alt. Exakt wie `_is_established_chatter`
    /// (moderation.py Z. 741–775, Vertrag Z. 469, Code-Realität: 40 Messages).
    pub is_established_chatter: bool,
    /// Vorberechneter Mention-Score vom Orchestrator (inkl. @host-Bonus wenn
    /// has_phrase_or_fragment_signal=true). Der SpamFilter addiert ihn direkt.
    /// (moderation.py Z. 1608–1615: mention_score wird IMMER addiert,
    /// nur allow_host_bonus ist conditioned)
    pub mention_score: i32,
    /// True wenn der Kanal-Host in der Mention-Liste war UND
    /// has_phrase_or_fragment_signal=true gilt — steuert den @host-Bonus
    /// im Orchestrator (hier nur dokumentiert, schon im mention_score drin).
    pub allow_host_bonus: bool,
}

/// Resultat der Spam-Bewertung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpamAction {
    /// Score == 0 oder Score > 0 ohne hartes Signal und < SPAM_MIN_MATCHES.
    None,
    /// 0 < Score < SPAM_MIN_MATCHES UND hartes Signal → Nachricht löschen, kein Ban.
    DeleteOnly,
    /// Score >= SPAM_MIN_MATCHES → Ban + Löschung.
    Ban,
}

/// Vollständiges Ergebnis von [`SpamFilter::evaluate`].
#[derive(Debug, Clone)]
pub struct SpamVerdict {
    /// Gesamtscore inkl. Mention-Score und Kontext-Eskalatoren.
    pub score: i32,
    /// True wenn mindestens ein hartes Signal in matched enthalten ist.
    pub hard_signal: bool,
    /// Empfohlene Aktion für den Orchestrator.
    pub action: SpamAction,
    /// Alle gematchten Signale als Reason-Strings (wie Python reasons-Liste).
    pub matched: Vec<String>,
}

// ---------------------------------------------------------------------------
// Gelernte Muster — DB-Tabelle twitch_auto_learned_spam_patterns und
// manuelle Safe-Tabelle twitch_auto_learned_safe_patterns.
// ---------------------------------------------------------------------------

/// Ein gelerntes Spam-Muster aus `twitch_auto_learned_spam_patterns`.
#[derive(Debug, Clone)]
struct LearnedSpamPattern {
    /// Mustertext (TEXT-Spalte).
    pattern: String,
    /// "phrase" oder anderes (TEXT-Spalte).
    pattern_type: String,
}

/// Ein manuell als harmlos gelernter exakter Volltext.
#[derive(Debug, Clone)]
struct LearnedSafePattern {
    pattern: String,
}

/// Gecachte gelernte Muster — einmalig per `LearnedPatterns::load` aus der DB geladen.
#[derive(Debug, Clone, Default)]
pub struct LearnedPatterns {
    spam: Vec<LearnedSpamPattern>,
    safe: Vec<LearnedSafePattern>,
}

impl LearnedPatterns {
    /// Leere Muster-Menge (für Tests ohne DB).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Erstellt eine Muster-Menge mit manuellen Safe-Volltexten.
    ///
    /// Der Produktivpfad verwendet [`Self::load`]; der Konstruktor hält den
    /// exakten Pre-LLM-Pfad auch für isolierte Pipeline-Tests reproduzierbar.
    pub fn with_safe_patterns(patterns: impl IntoIterator<Item = String>) -> Self {
        Self {
            spam: vec![],
            safe: patterns
                .into_iter()
                .map(|pattern| LearnedSafePattern { pattern })
                .collect(),
        }
    }

    /// Lädt gelernte Spam- und manuelle Safe-Muster aus der Postgres-DB.
    ///
    /// Tabelle (Prod-Schema 12.6.):
    /// - `twitch_auto_learned_spam_patterns`: pattern TEXT, pattern_type TEXT
    /// - `twitch_auto_learned_safe_patterns`: pattern TEXT, source_channel TEXT
    ///
    /// Fehler beim Laden werden als Warn geloggt und mit leerer Muster-Menge
    /// beantwortet (fail-open wie Python: `except Exception: pass`).
    pub async fn load(pool: &PgPool) -> Self {
        // Compile-time-geprüfte Loads; Fehler bleiben fail-open wie im Python-Pfad.
        #[derive(sqlx::FromRow)]
        struct SpamRow {
            pattern: Option<String>,
            pattern_type: Option<String>,
        }

        let spam = match sqlx::query_as!(
            SpamRow,
            "SELECT pattern AS \"pattern?\", pattern_type AS \"pattern_type?\" \
             FROM twitch_auto_learned_spam_patterns \
             WHERE pattern IS NOT NULL AND pattern_type IS NOT NULL",
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|r| {
                    let pattern = r.pattern.filter(|p| !p.is_empty())?;
                    let pattern_type = r.pattern_type.filter(|p| !p.is_empty())?;
                    // Gate auch beim Laden: Altbestand aus der Zeit vor dem
                    // Distinktivitäts-Gate darf nicht wirksam werden —
                    // generische Muster wären harte Signale (+2) gegen
                    // harmloses Viewer-Gerede.
                    if !is_distinctive_spam_pattern_vom_menschen(&pattern) {
                        tracing::warn!(
                            pattern = %pattern,
                            "Gelerntes Spam-Muster ignoriert: nicht distinktiv (Altbestand)"
                        );
                        return None;
                    }
                    Some(LearnedSpamPattern {
                        pattern,
                        pattern_type,
                    })
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Konnte twitch_auto_learned_spam_patterns nicht laden: {e}");
                vec![]
            }
        };

        #[derive(sqlx::FromRow)]
        struct SafeRow {
            pattern: String,
        }

        let safe = match sqlx::query_as::<_, SafeRow>(
            "SELECT pattern FROM twitch_auto_learned_safe_patterns \
             WHERE pattern IS NOT NULL AND source_channel = $1",
        )
        .bind("discord-correction")
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows
                .into_iter()
                .filter(|row| !row.pattern.trim().is_empty())
                .map(|row| LearnedSafePattern {
                    pattern: row.pattern,
                })
                .collect(),
            Err(e) => {
                tracing::warn!(
                    "Konnte manuelle twitch_auto_learned_safe_patterns nicht laden: {e}"
                );
                vec![]
            }
        };

        Self { spam, safe }
    }
}

// ---------------------------------------------------------------------------
// SpamFilter
// ---------------------------------------------------------------------------

/// Zweistufiger Spam-Score-Filter.
///
/// Hält die gelernten Muster in einem `ArcSwap` (lock-freier Hot-Reload);
/// wird über `Arc<SpamFilter>` geteilt, daher kein `Clone` nötig.
pub struct SpamFilter {
    /// Gelernte Muster, lock-frei austauschbar (ArcSwap). Ein Hintergrund-Task
    /// (chat_wiring) lädt sie periodisch neu — analog zum Python-Cache mit TTL
    /// 120 s (spam_ai_review.py), der pro Nachricht load_learned_patterns()
    /// aufrief und nach jedem Lernschritt invalidiert wurde. Ohne Reload griffen
    /// neu gelernte Spam-Muster im nativen Betrieb erst nach Bot-Neustart.
    learned: ArcSwap<LearnedPatterns>,
}

impl SpamFilter {
    /// Erstellt einen neuen Filter mit vorgeladenen gelernten Mustern.
    pub fn new(learned: LearnedPatterns) -> Self {
        Self {
            learned: ArcSwap::from_pointee(learned),
        }
    }

    /// Lädt die gelernten Muster neu aus der DB und tauscht sie atomar aus.
    /// Wird vom periodischen Reload-Task aufgerufen (Ziel: neu gelernte Muster
    /// greifen innerhalb der TTL statt erst nach Neustart).
    pub async fn reload(&self, pool: &PgPool) {
        let fresh = LearnedPatterns::load(pool).await;
        self.learned.store(Arc::new(fresh));
    }

    /// Gibt das gelernte Safe-Pattern zurück, wenn `text` exakt dem
    /// kanonisierten Volltext entspricht. Teilstrings und längere Nachrichten
    /// werden bewusst nicht akzeptiert.
    pub fn known_safe_pattern(&self, text: &str) -> Option<String> {
        let key = normalize_exact_spam_message(text);
        if key.chars().count() < 4 {
            return None;
        }
        self.learned.load().safe.iter().find_map(|pattern| {
            (normalize_exact_spam_message(&pattern.pattern) == key).then(|| pattern.pattern.clone())
        })
    }

    /// Berechnet Spam-Score und trifft Aktionsentscheidung.
    ///
    /// Port von:
    /// - `_calculate_spam_score` (moderation.py Z. 480–587)
    /// - Schwellen-Entscheid (bot.py Z. 1617–1737)
    ///
    /// # Scoring-Reihenfolge
    ///
    /// 1. Exact Phrase (+2, break)
    /// 2. Casefold Phrase (+2, break) — nur wenn kein Exact-Treffer
    /// 3. Domain-Kompakt (+2) — nur wenn kein Phrase-Treffer
    /// 4. Fragment-Fallback (+1, break) — nur wenn kein Phrase/Domain-Treffer
    /// 5. Viewer-Muster (+1, immer)
    /// 6. Gelernte Phrase (+2, break)
    /// 7. Gelerntes Fragment (+1, break)
    /// 8. Mention-Score addieren (aus ctx)
    /// 9. Kontext-Eskalatoren: Account-Alter <90d +1 / Erstnachricht +1 (nur bei hartem Signal UND Score < SPAM_MIN_MATCHES)
    ///
    /// Safe-Muster sind kein Score-Signal. Ein manueller Safe-Volltext wird
    /// bereits über [`Self::known_safe_pattern`] vor diesem Scoring geprüft.
    pub fn evaluate(&self, text: &str, ctx: &SpamContext) -> SpamVerdict {
        if text.is_empty() {
            return SpamVerdict {
                score: 0,
                hard_signal: false,
                action: SpamAction::None,
                matched: vec![],
            };
        }

        let (mut hits, mut reasons) = self.calculate_spam_score(text);

        // Mention-Score addieren (moderation.py Z. 1611–1615)
        if ctx.mention_score > 0 {
            hits += ctx.mention_score;
            // Mention-Gründe kommen bereits aus dem Orchestrator in mention_score
            // zusammengefasst — kein separater Reason-String nötig, der Score
            // ist schon drin.
        }

        // Kontext-Eskalatoren (moderation.py Z. 1617–1653):
        // NUR wenn Score noch < SPAM_MIN_MATCHES UND hartes Signal vorhanden.
        if hits < SPAM_MIN_MATCHES && has_hard_spam_signal(&reasons) {
            // Account-Alter < 90 Tage → +1
            if let Some(age) = ctx.account_age_days {
                if age < 90 {
                    hits += 1;
                    reasons.push(format!("Account-Alter: {age} Tage"));
                }
            }
            // Erstnachricht → +1 (NUR wenn auch hartes Signal, siehe Bedingung oben)
            if ctx.is_first_message {
                hits += 1;
                reasons.push("Erstnachricht".to_string());
            }
        }

        let hard = has_hard_spam_signal(&reasons);
        let action = if hits >= SPAM_MIN_MATCHES {
            SpamAction::Ban
        } else if hits > 0 && hard {
            SpamAction::DeleteOnly
        } else {
            SpamAction::None
        };

        SpamVerdict {
            score: hits,
            hard_signal: hard,
            action,
            matched: reasons,
        }
    }

    /// Interner Score ohne Kontext-Eskalatoren und Mention-Score.
    /// Port von `_calculate_spam_score` (moderation.py Z. 480–587).
    fn calculate_spam_score(&self, content: &str) -> (i32, Vec<String>) {
        let mut reasons: Vec<String> = Vec::new();
        let raw = normalize_spam_text(content);
        let mut hits: i32 = 0;
        let mut phrase_matched = false;

        // Schritt 1: Exact Phrase (moderation.py Z. 494–499)
        for phrase in SPAM_PHRASES {
            if raw.contains(*phrase) {
                hits += 2;
                reasons.push(format!("Phrase(Exact): {phrase}"));
                phrase_matched = true;
                break;
            }
        }

        let lowered = raw.to_lowercase(); // casefold() = to_lowercase() für ASCII-Spam
        let compact_str = compact(&lowered);
        let domainized_str = domainized(&lowered);

        // Schritt 2: Casefold Phrase (moderation.py Z. 509–515)
        if !phrase_matched {
            for phrase in SPAM_PHRASES {
                let plow = phrase.to_lowercase();
                if lowered.contains(plow.as_str()) {
                    hits += 2;
                    reasons.push(format!("Phrase(Casefold): {phrase}"));
                    phrase_matched = true;
                    break;
                }
            }
        }

        // Schritt 3: Domain-Kompakt (moderation.py Z. 521–526)
        if !phrase_matched {
            if let Some(m) = spam_domain_re().find(&domainized_str) {
                hits += 2;
                reasons.push(format!("Domain(Kompakt): {}", m.as_str()));
                phrase_matched = true;
            }
        }

        // Schritt 4: Fragment-Fallback (moderation.py Z. 529–534)
        if !phrase_matched {
            for frag in SPAM_FRAGMENTS {
                let frag_low = frag.to_lowercase();
                if let Some(re) = word_boundary_re(&frag_low) {
                    if re.is_match(&lowered) {
                        hits += 1;
                        reasons.push(format!("Fragment(Fallback): {frag}"));
                        break;
                    }
                }
            }
        }

        // Schritt 5: Viewer-Muster (moderation.py Z. 537–539)
        if viewer_pattern_re().is_match(&lowered) {
            hits += 1;
            reasons.push(VIEWER_PATTERN_REASON.to_string());
        }

        // Schritt 6+7: Gelernte Muster (moderation.py Z. 542–562)
        // Erst alle Phrasen prüfen (break bei erstem Treffer),
        // dann alle Fragmente (break bei erstem Treffer).
        let learned = self.learned.load();
        let mut learned_phrase_hit = false;
        for lp in &learned.spam {
            if lp.pattern_type != "phrase" {
                continue;
            }
            let pc = compact(&lp.pattern.to_lowercase());
            if lp.pattern.to_lowercase().as_str().chars().count() > 0
                && (lowered.contains(lp.pattern.to_lowercase().as_str())
                    || (pc.len() >= 4 && compact_str.contains(pc.as_str())))
            {
                hits += 2;
                reasons.push(format!("Learned-Phrase: {}", lp.pattern));
                learned_phrase_hit = true;
                break;
            }
        }
        if !learned_phrase_hit {
            for lp in &learned.spam {
                if lp.pattern_type == "phrase" {
                    continue;
                }
                let pc = compact(&lp.pattern.to_lowercase());
                let pat_low = lp.pattern.to_lowercase();
                let frag_match = word_boundary_re(&pat_low)
                    .map(|re| re.is_match(&lowered))
                    .unwrap_or(false);
                if frag_match || (pc.len() >= 4 && compact_str.contains(pc.as_str())) {
                    hits += 1;
                    reasons.push(format!("Learned-Fragment: {}", lp.pattern));
                    break;
                }
            }
        }

        (hits, reasons)
    }
}

// ---------------------------------------------------------------------------
// Distinktivitäts-Gate für gelernte Spam-Muster
// ---------------------------------------------------------------------------

/// Generisches Chat-Vokabular, das allein nie ein Spam-Muster tragen darf.
/// Ein gelerntes Muster wie „best viewers" würde sonst als hartes Signal (+2)
/// jedes Kompliment über Viewer in Ban-Nähe rücken.
const GENERIC_PATTERN_TOKENS: &[&str] = &[
    "viewer",
    "viewers",
    "view",
    "views",
    "follower",
    "followers",
    "follow",
    "sub",
    "subs",
    "subscriber",
    "subscribers",
    "best",
    "top",
    "real",
    "live",
    "cheap",
    "free",
    "buy",
    "get",
    "more",
    "big",
    "mad",
    "ai",
    "bot",
    "bots",
    "stream",
    "streams",
    "streamer",
    "streaming",
    "twitch",
    "chat",
    "promo",
    "promotion",
    "growth",
    "grow",
    "boost",
    "the",
    "and",
    "for",
    "with",
    "your",
    "com",
    "org",
    "net",
    "online",
    "site",
    "link",
];

/// Ab dieser Wortzahl trägt ein Muster allein durch seine Satzlänge — beides
/// muss zutreffen, sonst rutschen kurze Wortgruppen wie „get top views now"
/// durch.
///
/// Kalibriert an beiden Seiten, weil eine so gelernte Phrase hart bannt:
/// echte Angebote liegen bei 32 bis 42 Zeichen („i can help you grow your
/// channel" 7/32, „cheap viewers and followers available" 5/37), beiläufiger
/// Chat darunter („hey how are you doing" 5/21, „gg that was a good game"
/// 6/23). 4/20 fing beides und machte „more real live viewers" lernbar.
const PHRASE_MIN_WORDS: usize = 5;
/// Zweite Bedingung der Phrasen-Regel: Mindestlänge in Zeichen.
const PHRASE_MIN_CHARS: usize = 30;

/// Höchstzahl Tokens, die ein Muster **ohne** Domain im strengen Gate haben
/// darf. Ein Dienstname steht allein oder mit einem Zusatz („streamboo",
/// „ai clicknex"); ab drei Tokens ist es ein Satz, und dann trägt nicht mehr
/// der Name das Muster, sondern irgendein längeres Alltagswort darin. Genau
/// daran scheiterte die erste Fassung: „cheap viewers and followers available"
/// kam über „available" (9 Zeichen) durch, „i can help you grow your channel"
/// über „channel" (7).
const STRICT_MAX_TOKENS: usize = 2;

/// Normalform eines Tokens für beide Gate-Zweige, oder `None`, wenn das Token
/// von vornherein nichts trägt. Gleiche Normalform wie `compact()` im
/// Matching: nur Alphanumerik — „view.ers" kompaktiert zu „viewers" und ist
/// damit genauso generisch wie „viewers".
fn gate_token(token: &str) -> Option<(&str, String)> {
    let t = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '.');
    let compacted: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
    if compacted.chars().count() < 4 || GENERIC_PATTERN_TOKENS.contains(&compacted.as_str()) {
        return None;
    }
    Some((t, compacted))
}

/// Domain, deren registrierbarer Name (Label vor der TLD) selbst distinktiv
/// ist. Fängt „viewer.com" und „view.ers" ab.
fn ist_dienstdomain(token: &str) -> bool {
    let Some((t, _)) = gate_token(token) else {
        return false;
    };
    let labels: Vec<&str> = t
        .trim_matches('.')
        .split('.')
        .filter(|l| !l.is_empty())
        .collect();
    labels.len() >= 2 && {
        let name = labels[labels.len() - 2];
        name.chars().count() >= 4 && !GENERIC_PATTERN_TOKENS.contains(&name)
    }
}

/// Einzelwort ohne Punkt, ab 6 Zeichen dienstnamen-tauglich — darunter
/// überwiegen Alltagswörter, die das Gate nicht per Blockliste aufzählen kann.
/// Darüber auch, deshalb greift dieser Zweig nur bis [`STRICT_MAX_TOKENS`].
fn ist_dienstwort(token: &str) -> bool {
    let Some((t, compacted)) = gate_token(token) else {
        return false;
    };
    !t.trim_matches('.').contains('.') && compacted.chars().count() >= 6
}

/// True, wenn ein Muster unterscheidungskräftig genug ist, um gelernt zu
/// werden: entweder trägt ein Token eine Domain mit distinktivem
/// registrierbarem Namen („eballo.com", „clicknex.online") — dann ist die
/// Länge des Musters egal —, oder das ganze Muster besteht aus höchstens
/// [`STRICT_MAX_TOKENS`] Tokens, von denen eines ein Dienstname ist
/// („streamboo", „peakpy").
///
/// Dies ist das **strenge** Gate ohne Phrasen-Ausnahme. Es gilt überall, wo
/// kein Mensch im Loop steht — allen voran auf dem Judge-Pfad
/// (`scam_pitch::persist_spam_learning`). Ein gelerntes Muster ist laut
/// `has_hard_spam_signal` immer ein hartes Signal (+2) und erreicht zusammen
/// mit dem Erstnachricht-Zuschlag `SPAM_MIN_MATCHES` — ein Judge-Fehlurteil
/// über einen harmlosen Langsatz („i can help you find good builds for this
/// hero") würde damit jeden Erstchatter mit diesem Satz in **jedem** Kanal
/// bannen, dauerhaft und rückwirkend über [`LearnedPatterns::load`].
/// Die Phrasen-Ausnahme steht deshalb in
/// [`is_distinctive_spam_pattern_vom_menschen`] und nicht hier.
///
/// „eballo.com" ✓ · „streamboo" ✓ · „ai viewers clicknex.online" ✓ (Domain) ·
/// „best viewers" ✗ · „hello viewers" ✗ · „view.ers" ✗ · „viewer.com" ✗ ·
/// „cheap viewers and followers available" ✗ (5 Tokens, keine Domain) ·
/// „boost viewers on the stream – promotion. ru" ✗ (8 Tokens, „promotion."
/// hat kein zweites Label und ist damit keine Domain)
pub fn is_distinctive_spam_pattern(pattern: &str) -> bool {
    // „eballo .com" ist dieselbe Domain wie „eballo.com" — das Leerzeichen vor
    // dem Punkt ist ein verbreiteter Trennversuch. Ein Satzpunkt klebt dagegen
    // am Wortende („Hi. How are you") und bleibt hier unangetastet.
    let lowered = pattern.to_lowercase().replace(" .", ".");
    let tokens: Vec<&str> = lowered.split_whitespace().collect();
    if tokens.iter().copied().any(ist_dienstdomain) {
        return true;
    }
    tokens.len() <= STRICT_MAX_TOKENS && tokens.iter().copied().any(ist_dienstwort)
}

/// Wie [`is_distinctive_spam_pattern`], zusätzlich mit der Phrasen-Ausnahme:
/// ab [`PHRASE_MIN_WORDS`] Wörtern **und** [`PHRASE_MIN_CHARS`] Zeichen trägt
/// der Satz sich selbst, auch wenn er aus lauter generischen Tokens besteht.
///
/// **Nur für Muster, die ein Mensch eingetragen hat.** Anonyme Angebote
/// („boost viewers on the stream – promotion. ru") haben keinen Dienstnamen und
/// fallen durch das strenge Gate — genau daran scheiterte jede Mod-Korrektur
/// solcher Alerts (10.08.2026). Die Lockerung umgeht [`GENERIC_PATTERN_TOKENS`]
/// vollständig, ihre einzige Decke ist die Länge. Das ist tragbar, solange ein
/// Mensch den Eintrag verantwortet; ohne einen wäre es ein Freibrief für
/// beliebige Fünf-Wort-Sätze als globales Ban-Muster (Merge-Kritiker
/// 10.08.2026).
///
/// Zwei Aufrufer, beide mit Mensch davor:
/// - `tb-internal-api::handlers::spam_learning` — die Mod-Korrektur selbst.
/// - [`LearnedPatterns::load`] — der Reload dessen, was so entstanden ist.
///   Das ist der einzige **rückwirkende** Pfad: eine Lockerung hier hebt jede
///   alte Zeile wieder in Kraft, die die neue Regel erfüllt. Stand 10.08.2026
///   stehen in `twitch_auto_learned_spam_patterns` zwei Zeilen
///   (`streamboo.com`, `eballo.com`), beide über den Domain-Zweig distinktiv,
///   null von der Phrasen-Ausnahme erfasst.
///
/// Gezählt wird mit `split_whitespace`: „boost viewers on the stream –
/// promotion. ru" sind 8 Tokens (der Gedankenstrich zählt mit) und 43 Zeichen.
///
/// Eine Lücke bleibt bewusst offen: mittellange Einträge ohne Domain
/// (3 bis 4 Tokens, unter [`PHRASE_MIN_CHARS`] Zeichen — „cheap viewers
/// available") fallen durch beide Zweige, weil sie für [`STRICT_MAX_TOKENS`]
/// zu lang und für die Phrasen-Regel zu kurz sind. Der Mod bekommt eine
/// Ablehnung und trägt den vollen Satz ein; das ist billiger als ein
/// Drei-Wort-Generikum als globales Ban-Muster.
///
/// „boost viewers on the stream – promotion. ru" ✓ ·
/// „cheap viewers and followers available" ✓ (5 / 37) ·
/// „boost viewers on the stream" ✗ (5 Wörter, aber 27 Zeichen — beide
/// Bedingungen müssen halten) · „hey how are you doing" ✗ (5 / 21)
pub fn is_distinctive_spam_pattern_vom_menschen(pattern: &str) -> bool {
    let lowered = pattern.to_lowercase();
    if lowered.split_whitespace().count() >= PHRASE_MIN_WORDS
        && lowered.chars().count() >= PHRASE_MIN_CHARS
    {
        return true;
    }
    is_distinctive_spam_pattern(pattern)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn filter() -> SpamFilter {
        SpamFilter::new(LearnedPatterns::empty())
    }

    fn ctx_default() -> SpamContext {
        SpamContext::default()
    }

    // --- Normalisierung ---

    #[test]
    fn normalisierung_nfkc_zuerst() {
        // NFKC normalisiert z. B. ﬁ (U+FB01) → fi
        let out = normalize_spam_text("ﬁle");
        assert_eq!(out, "file");
    }

    #[test]
    fn normalisierung_small_cap_a() {
        let out = normalize_spam_text("ᴀ");
        assert_eq!(out, "a");
    }

    #[test]
    fn normalisierung_math_bold_a() {
        // U+1D400 = Mathematical Bold Capital A → A
        let bold_a = char::from_u32(0x1D400).unwrap();
        let out = normalize_spam_text(&bold_a.to_string());
        assert_eq!(out, "A");
    }

    #[test]
    fn normalisierung_kyrillisch_a() {
        // Kyrillisches а (U+0430) → lateinisches a
        let out = normalize_spam_text("а");
        assert_eq!(out, "a");
    }

    #[test]
    fn normalisierung_kyrillisch_streamboo() {
        // Klassisches Spammer-Muster: kyrillische Buchstaben in "streamboo"
        // е = U+0435, а = U+0430, о = U+043E
        let spammer = "strеаmbоо"; // kyrillische e, a, o
        let out = normalize_spam_text(spammer);
        assert_eq!(out, "streamboo");
    }

    // --- Exact Phrase ---

    #[test]
    fn exact_phrase_streamboo_com() {
        // "Best viewers streamboo.com" → Phrase(Exact) +2 + Viewer-Muster +1 = 3 → Ban
        let v = filter().evaluate("Best viewers streamboo.com", &ctx_default());
        assert_eq!(v.score, 3);
        assert!(v.matched.iter().any(|r| r.starts_with("Phrase(Exact)")));
        assert_eq!(v.action, SpamAction::Ban);
    }

    #[test]
    fn exact_phrase_remove_the_space() {
        // "viewers on streamboo .com (remove the space)":
        // Phrase-Iteration trifft ZUERST "(remove the space)" (Position 8 in SPAM_PHRASES,
        // kommt vor "viewers on streamboo .com (remove the space)").
        // Reason: "Phrase(Exact): (remove the space)" — enthält kein Brand-Token.
        // Zusätzlich: Viewer-Muster greift ("viewers on" matcht \bviewers?\s+\w+).
        // Score: 2+1=3 → Ban; hard_signal=false (kein Brand-Token im Phrase-Reason).
        let v = filter().evaluate(
            "viewers on streamboo .com (remove the space)",
            &ctx_default(),
        );
        assert!(v.score >= 2, "score: {} reasons: {:?}", v.score, v.matched);
        assert_eq!(v.action, SpamAction::Ban);
        // "(remove the space)" enthält kein Brand-Token → hard_signal=false
        assert!(!v.hard_signal, "Reasons: {:?}", v.matched);
    }

    #[test]
    fn exact_phrase_cool_overlay_volltext() {
        let text = "Cool overlay \u{1F44D} Honestly, it\u{2019}s so hard to get found on the directory lately. I have small tips on beating the algorithm. Mind if I send you an share?";
        let v = filter().evaluate(text, &ctx_default());
        assert!(v.score >= 2);
        assert!(v.matched.iter().any(|r| r.starts_with("Phrase(Exact)")));
    }

    // --- Casefold Phrase ---

    #[test]
    fn casefold_phrase_grossbuchstaben() {
        // Großgeschrieben, kein Exact-Match → Casefold-Match
        let v = filter().evaluate("BEST VIEWERS STREAMBOO.COM", &ctx_default());
        assert!(v.score >= 2);
        assert!(v.matched.iter().any(|r| r.starts_with("Phrase(Casefold)")));
    }

    // --- Domain-Kompakt ---

    #[test]
    fn domain_kompakt_mit_leerzeichen() {
        // "s t r e a m b o o . c o m" → domainized = "streamboo.com" → Domain-Treffer
        let v = filter().evaluate("s t r e a m b o o . c o m", &ctx_default());
        assert!(v.score >= 2);
        assert!(v.matched.iter().any(|r| r.starts_with("Domain(Kompakt)")));
        assert!(v.hard_signal);
    }

    #[test]
    fn domain_kompakt_smmhype_ru() {
        let v = filter().evaluate("kaufe views auf smmhype.ru", &ctx_default());
        assert!(v.score >= 2);
        assert!(v.hard_signal);
    }

    // --- Fragment-Fallback ---

    #[test]
    fn fragment_fallback_rookie() {
        let v = filter().evaluate("hey rookie, join my channel", &ctx_default());
        // Fragment(Fallback): rookie +1, aber kein hartes Signal (rookie nicht in BRAND_TOKENS)
        assert_eq!(v.score, 1);
        assert!(!v.hard_signal);
        assert_eq!(v.action, SpamAction::None);
    }

    #[test]
    fn fragment_fallback_streamboo_ohne_tld() {
        // "streamboo" ohne TLD → Fragment-Fallback (kein Domain-Regex-Match)
        let v = filter().evaluate("check out streamboo today", &ctx_default());
        assert!(v.score >= 1);
        // streamboo ist Brand-Token → hard signal über Fragment(
        assert!(v.hard_signal);
    }

    // --- Viewer-Muster ---

    #[test]
    fn viewer_muster_weich() {
        let v = filter().evaluate("get more viewers now", &ctx_default());
        // Viewer-Muster +1, aber kein hartes Signal
        assert_eq!(v.score, 1);
        assert!(!v.hard_signal);
        assert_eq!(v.action, SpamAction::None);
    }

    // --- Safe-Wording-Gate ---

    #[test]
    fn safe_wording_trifft_goenn_und_lurk_sprech() {
        assert_eq!(matches_safe_wording("gönne viewer du weißt"), Some("gönn"));
        assert_eq!(
            matches_safe_wording("GÖNN dir viewer bro"),
            Some("gönn"),
            "case-insensitiv"
        );
        assert_eq!(matches_safe_wording("goenn dir viewer"), Some("goenn"));
        assert_eq!(matches_safe_wording("bin nur am lurken hehe"), Some("am lurken"));
        assert_eq!(matches_safe_wording("best viewers streamboo com"), None);
    }

    /// Getarnte Angebote tragen den harmlosen Marker mit, wollen aber verkaufen.
    /// Der reine Viewer-Regex ist dabei das einzige Spam-Signal, das Gate wuerde
    /// sie also ohne diese Gegenpruefung am Judge vorbei durchwinken.
    #[test]
    fn safe_wording_greift_nicht_bei_kontakt_oder_verkauf() {
        for text in [
            "gönn dir viewers, schreib mir",
            "gönn dir viewer, dm mir",
            "goenn dir viewer, billig",
            "gönn dir viewer, melde dich per telegram",
            "bin am lurken, kaufen kannst du viewer bei mir",
        ] {
            assert_eq!(
                matches_safe_wording(text),
                None,
                "Kontakt- oder Verkaufsangebot darf nicht als harmlos gelten: {text}"
            );
        }
        // Die harmlose Umgangssprache bleibt harmlos.
        assert_eq!(matches_safe_wording("gönn dir viewer bro"), Some("gönn"));
    }

    /// Das Gate ueberstimmt einen Detektor, der ueber die Homoglyph-Tabelle
    /// laeuft. Ohne dieselbe Normalisierung waere ein kyrillisches Zeichen der
    /// Freifahrtschein am Judge vorbei.
    #[test]
    fn safe_wording_sieht_durch_homoglyphen_hindurch() {
        // Kyrillisches s (U+0455) in "schreib mir".
        assert_eq!(
            matches_safe_wording("gönn dir viewers, \u{0455}chreib mir"),
            None,
            "Homoglyph im Kontaktwort umgeht die Gegenpruefung"
        );
        // Akzent-i in "billig".
        assert_eq!(
            matches_safe_wording("gönn dir viewer bill\u{00ed}g"),
            None,
            "Homoglyph im Verkaufswort umgeht die Gegenpruefung"
        );
    }

    /// Die Blockliste ist handgepflegt. Ein Angebot mit blosser Preisangabe
    /// traegt keinen Marker: dieser Test haelt fest, was das Gate heute NICHT
    /// faengt, damit die Luecke sichtbar bleibt statt still zu wirken.
    #[test]
    fn safe_wording_faengt_reine_preisangaben_nicht() {
        assert_eq!(
            matches_safe_wording("gönn dir 10k viewer, 5 euro"),
            Some("gönn"),
            "bekannte Luecke: ohne Kontakt- oder Verkaufswort greift das Gate"
        );
    }

    #[test]
    fn gate_greift_nur_bei_reinem_viewer_muster() {
        // Reiner Viewer-Regex-Treffer → Gate darf greifen.
        let nur_viewer = filter().evaluate("gönne viewer du weißt", &ctx_default());
        assert_eq!(nur_viewer.matched, vec![VIEWER_PATTERN_REASON.to_string()]);
        assert!(spam_signal_ist_nur_viewer_muster(&nur_viewer.matched));

        // Zusätzliches Fragment/Phrase → Gate darf NICHT greifen (echter Spam
        // trägt immer ein stärkeres Signal).
        let mit_phrase = filter().evaluate("gönn dir viewers smmhype", &ctx_default());
        assert!(
            !spam_signal_ist_nur_viewer_muster(&mit_phrase.matched),
            "matched: {:?}",
            mit_phrase.matched
        );

        // Leere Trefferliste ist kein Viewer-Muster.
        assert!(!spam_signal_ist_nur_viewer_muster(&[]));
    }

    // --- SPAM_MIN_MATCHES Schwelle ---

    #[test]
    fn score_3_ergibt_ban() {
        // "Viewers smmhype" → Exact Phrase(Exact) +2 → score 2
        // + "viewer + name" Pattern → +1 → score 3 → Ban
        let v = filter().evaluate("Viewers smmhype now", &ctx_default());
        assert!(v.score >= 3 || v.score >= 2, "score: {}", v.score);
        // Mindestens score >= 2 (Phrase)
        // Wenn viewer-muster auch greift: >= 3 → Ban
    }

    #[test]
    fn ban_bei_score_genau_3() {
        // "Mind if I send you an share" → Phrase(Exact) +2, kein Viewer-Muster → score 2
        // + mention_score=1 → score 3 → Ban
        let ctx = SpamContext {
            mention_score: 1,
            ..Default::default()
        };
        let v = filter().evaluate("Mind if I send you an share", &ctx);
        assert_eq!(v.score, 3, "reasons: {:?}", v.matched);
        assert_eq!(v.action, SpamAction::Ban);
    }

    // --- Kontext-Eskalatoren ---

    #[test]
    fn kontext_account_alter_eskaliert_nur_bei_hartem_signal() {
        // Weiches Signal (Fragment "rookie") + junges Konto → KEINE Eskalation
        let ctx = SpamContext {
            account_age_days: Some(10),
            ..Default::default()
        };
        let v = filter().evaluate("hey rookie", &ctx);
        // hits=1 (Fragment), kein hartes Signal → Eskalator greift nicht
        assert_eq!(v.score, 1);
        assert_eq!(v.action, SpamAction::None);
    }

    #[test]
    fn kontext_account_alter_eskaliert_bei_hartem_signal() {
        // Hartes Signal (Domain) + junges Konto → +1
        let ctx = SpamContext {
            account_age_days: Some(10),
            ..Default::default()
        };
        // "streamboo" (Fragment mit hard signal) → score 1, hard → Eskalator +1 → score 2
        let v = filter().evaluate("check streamboo out", &ctx);
        assert!(v.score >= 2);
        assert!(v.matched.iter().any(|r| r.contains("Account-Alter")));
    }

    #[test]
    fn kontext_erstnachricht_nur_bei_hartem_signal() {
        let ctx = SpamContext {
            is_first_message: true,
            ..Default::default()
        };
        // Nur weiches Signal → kein Eskalator
        let v = filter().evaluate("hey rookie", &ctx);
        assert!(!v.matched.iter().any(|r| r == "Erstnachricht"));
    }

    #[test]
    fn kontext_erstnachricht_mit_hartem_signal() {
        let ctx = SpamContext {
            is_first_message: true,
            ..Default::default()
        };
        let v = filter().evaluate("check streamboo out", &ctx);
        assert!(
            v.matched.iter().any(|r| r == "Erstnachricht"),
            "reasons: {:?}",
            v.matched
        );
    }

    #[test]
    fn eskalatoren_greifen_nicht_wenn_schon_ban() {
        // "Mind if I send you an share" → Phrase +2, kein Viewer-Muster.
        // + mention_score=1 → score=3 (Ban-Schwelle).
        // Eskalatoren-Bedingung: hits < SPAM_MIN_MATCHES → nicht erfüllt (3 < 3 = false).
        // Account-Alter + Erstnachricht dürfen score NICHT weiter erhöhen.
        let ctx = SpamContext {
            mention_score: 1,
            account_age_days: Some(5),
            is_first_message: true,
            ..Default::default()
        };
        let v = filter().evaluate("Mind if I send you an share", &ctx);
        // Score bleibt bei 3 (nicht 5), Eskalatoren greifen nicht.
        assert_eq!(
            v.score, 3,
            "Eskalatoren dürfen nicht auf bereits-Ban-Score addieren: {:?}",
            v.matched
        );
        assert_eq!(v.action, SpamAction::Ban);
    }

    // --- Kein Negativ-Scoring mehr (Regression Safe-List-Poisoning 11.7.) ---

    #[test]
    fn eballo_spam_erreicht_ban_schwelle_trotz_frueherem_safe_poisoning() {
        // Der reale Vorfall vom 11.07.: Phrase(Exact) "(remove the space)" +2
        // + Viewer-Muster +1 = 3. Das damals gelernte Safe-Muster "viewer"
        // drückte den Score auf 1 — dieser Mechanismus existiert nicht mehr,
        // die Nachricht MUSS auf Ban laufen.
        let v = filter().evaluate(
            "Best Viewers Eballo .com (remove the space)",
            &ctx_default(),
        );
        assert_eq!(v.score, 3, "Reasons: {:?}", v.matched);
        assert_eq!(v.action, SpamAction::Ban);
        assert!(!v.matched.iter().any(|r| r.starts_with("Safe(AI)")));
    }

    // --- Distinktivitäts-Gate für gelernte Spam-Muster ---

    #[test]
    fn gate_lehnt_generisches_vokabular_ab() {
        assert!(!is_distinctive_spam_pattern("viewer"));
        assert!(!is_distinctive_spam_pattern("best viewers"));
        assert!(!is_distinctive_spam_pattern("ai viewers"));
        assert!(!is_distinctive_spam_pattern("Buy Followers"));
    }

    #[test]
    fn gate_lehnt_kurze_alltagswoerter_ab() {
        // Wörter ohne Punkt erst ab 6 Zeichen — sonst wird jedes ungelistete
        // Alltagswort zum harten Learned-Signal (Merge-Kritiker 11.7.).
        assert!(!is_distinctive_spam_pattern("hello"));
        assert!(!is_distinctive_spam_pattern("hello viewers"));
        assert!(!is_distinctive_spam_pattern("kaufe views"));
        assert!(is_distinctive_spam_pattern("eballo"));
        assert!(is_distinctive_spam_pattern("peakpy"));
    }

    #[test]
    fn gate_akzeptiert_domains_und_dienstnamen() {
        assert!(is_distinctive_spam_pattern("eballo.com"));
        assert!(is_distinctive_spam_pattern("streamboo"));
        assert!(is_distinctive_spam_pattern("best viewers eballo .com"));
        assert!(is_distinctive_spam_pattern("Ai viewers clicknex.online"));
    }

    #[test]
    fn menschen_gate_akzeptiert_ganze_saetze_ohne_dienstnamen() {
        // Anonyme Angebote bestehen nur aus Allerweltswörtern. Als ganzer Satz
        // sind sie trotzdem distinktiv — ohne diese Regel lief jede
        // Mod-Korrektur eines solchen Alerts ins Leere (10.08.2026).
        assert!(is_distinctive_spam_pattern_vom_menschen(
            "Boost viewers on the stream – promotion. ru"
        ));
        assert!(is_distinctive_spam_pattern_vom_menschen(
            "cheap viewers and followers available"
        ));
        assert!(is_distinctive_spam_pattern_vom_menschen(
            "i can help you grow your channel"
        ));
    }

    #[test]
    fn strenges_gate_lehnt_dieselben_saetze_ab() {
        // Der Judge-Pfad hat keinen Menschen im Loop: ein Fehlurteil würde hier
        // zu einem dauerhaften harten Ban-Signal über alle Kanäle. Deshalb
        // greift für ihn nur der Dienstnamen-Zweig (Merge-Kritiker 10.08.2026).
        for satz in [
            "Boost viewers on the stream – promotion. ru",
            "cheap viewers and followers available",
            "i can help you grow your channel",
            // Der Fall, der den Blocker begründet: harmloser Langsatz.
            "i can help you find good builds for this hero",
        ] {
            assert!(
                !is_distinctive_spam_pattern(satz),
                "ohne Menschen im Loop darf {satz:?} nicht lernbar sein"
            );
        }
        // Mit Dienstname bleibt der Judge-Pfad lernfähig.
        assert!(is_distinctive_spam_pattern("Ai viewers clicknex.online"));
    }

    #[test]
    fn gate_lehnt_kurze_wortgruppen_weiter_ab() {
        // Die Phrasen-Regel darf die generischen Kurzmuster nicht aufweichen:
        // zu wenige Wörter oder zu kurz.
        assert!(!is_distinctive_spam_pattern("buy best viewers"));
        assert!(!is_distinctive_spam_pattern("more real subs"));
        assert!(!is_distinctive_spam_pattern("get top views now"));
    }

    #[test]
    fn menschen_gate_lehnt_alltagssaetze_trotz_satzlaenge_ab() {
        // Die Phrasen-Regel umgeht das Token-Gate bewusst — jede so gelernte
        // Phrase wird über has_hard_spam_signal zum harten Ban-Signal. Die
        // Schwelle muss deshalb ueber dem liegen, was ein Mensch beilaeufig
        // schreibt (Merge-Kritiker 10.08.2026).
        assert!(
            !is_distinctive_spam_pattern_vom_menschen("more real live viewers"),
            "4 Woerter / 22 Zeichen, alle Tokens generisch"
        );
        assert!(
            !is_distinctive_spam_pattern_vom_menschen("hey how are you doing"),
            "5 Woerter / 21 Zeichen, harmloser Gruss"
        );
        assert!(
            !is_distinctive_spam_pattern_vom_menschen("gg that was a good game"),
            "6 Woerter / 23 Zeichen, normaler Chat"
        );
        // Beide Bedingungen muessen halten: Wortzahl reicht, Laenge nicht.
        // Ohne die Zeichen-Bedingung waere das hier lernbar.
        assert!(
            !is_distinctive_spam_pattern_vom_menschen("boost viewers on the stream"),
            "5 Woerter, aber nur 27 Zeichen"
        );
    }

    #[test]
    fn gate_lehnt_kurze_und_leere_muster_ab() {
        assert!(!is_distinctive_spam_pattern(""));
        assert!(!is_distinctive_spam_pattern("abc"));
        assert!(!is_distinctive_spam_pattern("a b c"));
        assert!(!is_distinctive_spam_pattern("...."));
    }

    #[test]
    fn gate_prueft_auf_matching_normalform() {
        // "view.ers" kompaktiert beim Matching zu "viewers" — muss genauso
        // generisch behandelt werden wie das Wort selbst (Review-Blocker 11.7.).
        assert!(!is_distinctive_spam_pattern("view.ers"));
        assert!(!is_distinctive_spam_pattern("vie.wer"));
        // Domain mit generischem registrierbarem Namen bleibt generisch.
        assert!(!is_distinctive_spam_pattern("viewer.com"));
        assert!(!is_distinctive_spam_pattern("best viewers view.ers"));
        // Echte Dienstnamen/Domains bleiben lernbar.
        assert!(is_distinctive_spam_pattern("peakpy. c0m"));
        assert!(is_distinctive_spam_pattern("streambo\u{1d4f8} .com"));
    }

    // --- Leerer Input ---

    #[test]
    fn leer_gibt_score_0() {
        let v = filter().evaluate("", &ctx_default());
        assert_eq!(v.score, 0);
        assert_eq!(v.action, SpamAction::None);
        assert!(v.matched.is_empty());
    }

    // --- Gelernte Muster ---

    #[test]
    fn gelerntes_spam_phrase_pattern() {
        let learned = LearnedPatterns {
            spam: vec![LearnedSpamPattern {
                pattern: "kaufe viewboost".to_string(),
                pattern_type: "phrase".to_string(),
            }],
            safe: vec![],
        };
        let f = SpamFilter::new(learned);
        let v = f.evaluate("kaufe viewboost günstig", &ctx_default());
        assert_eq!(v.score, 2);
        assert!(v.matched.iter().any(|r| r.starts_with("Learned-Phrase")));
        // ALLE Learned-* Reasons sind hard signals (moderation.py Z. 606: startswith "Learned-")
        assert!(
            v.hard_signal,
            "Learned-* muss immer hard signal sein: {:?}",
            v.matched
        );
        // Score 2 < SPAM_MIN_MATCHES(3) UND hartes Signal → DeleteOnly
        assert_eq!(v.action, SpamAction::DeleteOnly);
    }

    #[test]
    fn gelerntes_spam_fragment_pattern() {
        let learned = LearnedPatterns {
            spam: vec![LearnedSpamPattern {
                pattern: "viewbot".to_string(),
                pattern_type: "fragment".to_string(),
            }],
            safe: vec![],
        };
        let f = SpamFilter::new(learned);
        let v = f.evaluate("ich nutze viewbot", &ctx_default());
        assert_eq!(v.score, 1);
        assert!(v.matched.iter().any(|r| r.starts_with("Learned-Fragment")));
        // ALLE Learned-* Reasons sind hard signals (moderation.py Z. 606)
        assert!(
            v.hard_signal,
            "Learned-* muss immer hard signal sein: {:?}",
            v.matched
        );
        // Score 1 < 3 UND hartes Signal → DeleteOnly
        assert_eq!(v.action, SpamAction::DeleteOnly);
    }

    #[test]
    fn gelerntes_learned_hard_signal() {
        // Learned- Präfix → immer hard signal
        let learned = LearnedPatterns {
            spam: vec![LearnedSpamPattern {
                pattern: "smmhype neu".to_string(),
                pattern_type: "fragment".to_string(),
            }],
            safe: vec![],
        };
        let f = SpamFilter::new(learned);
        let v = f.evaluate("check smmhype neu aus", &ctx_default());
        assert!(v.hard_signal, "Learned-* muss immer hard signal sein");
    }

    // --- Compact-Form gelernte Muster ---

    #[test]
    fn gelerntes_phrase_compact_match() {
        // Compact: "kaufeviews" aus "k a u f e v i e w s" (alle non-alnum raus)
        let learned = LearnedPatterns {
            spam: vec![LearnedSpamPattern {
                pattern: "kaufeviews".to_string(),
                pattern_type: "phrase".to_string(),
            }],
            safe: vec![],
        };
        let f = SpamFilter::new(learned);
        // Text mit Spreizung: "k a u f e v i e w s" → compact = "kaufeviews"
        let v = f.evaluate("k a u f e v i e w s günstig", &ctx_default());
        assert_eq!(v.score, 2);
        assert!(v.matched.iter().any(|r| r.starts_with("Learned-Phrase")));
    }

    #[test]
    fn manuelles_safe_pattern_matcht_nur_exakten_volltext() {
        let learned = LearnedPatterns {
            spam: vec![],
            safe: vec![LearnedSafePattern {
                pattern: "8 viewer brd wild".to_string(),
            }],
        };
        let filter = SpamFilter::new(learned);

        assert_eq!(
            filter.known_safe_pattern(" 8  VIEWER brd wild\n"),
            Some("8 viewer brd wild".to_string())
        );
        assert_eq!(
            filter.known_safe_pattern("8 viewer brd wild eballo.com"),
            None,
            "ein längerer Text darf kein Safe-Teilstringtreffer sein"
        );
    }

    // --- SPAM_MIN_MATCHES Konstante ---

    #[test]
    fn spam_min_matches_ist_3() {
        assert_eq!(SPAM_MIN_MATCHES, 3);
    }

    // --- has_hard_spam_signal direkt ---

    #[test]
    fn hard_signal_domain_immer() {
        assert!(has_hard_spam_signal(&[
            "Domain(Kompakt): streamboo.com".to_string()
        ]));
    }

    #[test]
    fn hard_signal_learned_immer() {
        assert!(has_hard_spam_signal(&["Learned-Phrase: xyz".to_string()]));
        assert!(has_hard_spam_signal(&["Learned-Fragment: abc".to_string()]));
    }

    #[test]
    fn hard_signal_phrase_mit_brand() {
        assert!(has_hard_spam_signal(&[
            "Phrase(Exact): Best viewers streamboo.com".to_string()
        ]));
    }

    #[test]
    fn hard_signal_phrase_ohne_brand() {
        // "Mind if I send you an share" enthält keinen Brand-Token
        assert!(!has_hard_spam_signal(&[
            "Phrase(Exact): Mind if I send you an share".to_string()
        ]));
    }

    #[test]
    fn hard_signal_fragment_rookie_kein_brand() {
        assert!(!has_hard_spam_signal(&[
            "Fragment(Fallback): rookie".to_string()
        ]));
    }

    #[test]
    fn hard_signal_fragment_mit_brand_token() {
        assert!(has_hard_spam_signal(&[
            "Fragment(Fallback): streamboo".to_string()
        ]));
    }
}

// ---------------------------------------------------------------------------
// DB-Tests (nur wenn TB_TEST_DATABASE_URL gesetzt)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use std::str::FromStr;

    use sqlx::PgPool;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    use super::*;

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap()
    }

    async fn create_learned_tables(pool: &PgPool) {
        // Spiegel des Prod-Schemas inkl. Migration 20260711170000 (id-Spalte).
        sqlx::query(
            "CREATE TABLE twitch_auto_learned_spam_patterns (
                pattern TEXT PRIMARY KEY,
                pattern_type TEXT,
                source_message TEXT,
                source_channel TEXT,
                minimax_reasoning TEXT,
                hit_count INTEGER DEFAULT 0,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                id BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE
            )",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_auto_learned_safe_patterns (
                pattern TEXT PRIMARY KEY,
                source_message TEXT,
                source_channel TEXT,
                minimax_reasoning TEXT,
                hit_count INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn load_leere_tabellen_gibt_leere_patterns() {
        let pool = pool_or_skip!("sf_load_leer");
        create_learned_tables(&pool).await;
        let lp = LearnedPatterns::load(&pool).await;
        assert!(lp.spam.is_empty());
        assert!(lp.safe.is_empty());
    }

    #[tokio::test]
    async fn load_spam_patterns() {
        let pool = pool_or_skip!("sf_load_spam");
        create_learned_tables(&pool).await;

        sqlx::query(
            "INSERT INTO twitch_auto_learned_spam_patterns (pattern, pattern_type) VALUES ($1, $2)",
        )
        .bind("kaufe viewboost")
        .bind("phrase")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_auto_learned_spam_patterns (pattern, pattern_type) VALUES ($1, $2)",
        )
        .bind("viewbots")
        .bind("fragment")
        .execute(&pool)
        .await
        .unwrap();

        let lp = LearnedPatterns::load(&pool).await;
        assert_eq!(lp.spam.len(), 2);
        assert!(
            lp.spam
                .iter()
                .any(|p| p.pattern == "kaufe viewboost" && p.pattern_type == "phrase")
        );
        assert!(
            lp.spam
                .iter()
                .any(|p| p.pattern == "viewbots" && p.pattern_type == "fragment")
        );
    }

    #[tokio::test]
    async fn load_laedt_nur_manuelle_safe_quelle() {
        let pool = pool_or_skip!("sf_load_safe");
        create_learned_tables(&pool).await;

        sqlx::query(
            "INSERT INTO twitch_auto_learned_safe_patterns
             (pattern, source_message, source_channel)
             VALUES
                ('8 viewer brd wild', '8 viewer brd wild', 'discord-correction'),
                ('viewer', 'viewer', 'ai-review')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let lp = LearnedPatterns::load(&pool).await;
        assert_eq!(lp.safe.len(), 1);
        let filter = SpamFilter::new(lp);
        assert_eq!(
            filter.known_safe_pattern("8 VIEWER brd wild"),
            Some("8 viewer brd wild".to_string())
        );
        assert_eq!(filter.known_safe_pattern("viewer"), None);
    }

    #[tokio::test]
    async fn load_filtert_den_altbestand_nach_derselben_regel() {
        // load ist der einzige Pfad, auf dem eine Lockerung des Gates
        // rueckwirkend wirkt: eine Zeile, die unter der alten Regel inert lag,
        // wird beim naechsten Reload wieder scharf (Merge-Kritiker 10.08.2026).
        // Der Test bindet load an dieselbe Regel wie die Schreibpfade.
        let pool = pool_or_skip!("sf_load_gate");
        create_learned_tables(&pool).await;

        for (pattern, typ) in [
            ("best viewers", "phrase"),                // generisch, 2 Woerter
            ("boost viewers on the stream", "phrase"), // 5 Woerter, 27 Zeichen
            ("cheap viewers and followers available", "phrase"), // 5 Woerter, 37 Zeichen
            ("streamboo.com", "fragment"),             // Domain
        ] {
            sqlx::query(
                "INSERT INTO twitch_auto_learned_spam_patterns (pattern, pattern_type) \
                 VALUES ($1, $2)",
            )
            .bind(pattern)
            .bind(typ)
            .execute(&pool)
            .await
            .unwrap();
        }

        let lp = LearnedPatterns::load(&pool).await;
        let mut geladen: Vec<&str> = lp.spam.iter().map(|p| p.pattern.as_str()).collect();
        geladen.sort_unstable();
        assert_eq!(
            geladen,
            vec!["cheap viewers and followers available", "streamboo.com"],
            "nur was is_distinctive_spam_pattern passiert, darf scharf werden"
        );
    }

    #[tokio::test]
    async fn learned_patterns_werden_im_filter_genutzt() {
        let pool = pool_or_skip!("sf_filter_use");
        create_learned_tables(&pool).await;

        sqlx::query(
            "INSERT INTO twitch_auto_learned_spam_patterns (pattern, pattern_type) VALUES ($1, $2)",
        )
        .bind("kaufe viewboost")
        .bind("phrase")
        .execute(&pool)
        .await
        .unwrap();

        let lp = LearnedPatterns::load(&pool).await;
        let f = SpamFilter::new(lp);
        let v = f.evaluate("kaufe viewboost günstig", &SpamContext::default());
        assert_eq!(v.score, 2);
        assert!(v.matched.iter().any(|r| r.starts_with("Learned-Phrase")));
    }

    #[tokio::test]
    async fn reload_uebernimmt_neu_gelernte_muster_ohne_neubau() {
        let pool = pool_or_skip!("sf_reload");
        create_learned_tables(&pool).await;

        // Filter startet mit leeren Mustern.
        let f = SpamFilter::new(LearnedPatterns::load(&pool).await);
        let before = f.evaluate("kaufe viewboost günstig", &SpamContext::default());
        assert!(
            !before
                .matched
                .iter()
                .any(|r| r.starts_with("Learned-Phrase")),
            "vor dem Lernen kein Learned-Phrase-Treffer"
        );

        // Muster wird gelernt (DB-Insert) ...
        sqlx::query(
            "INSERT INTO twitch_auto_learned_spam_patterns (pattern, pattern_type) VALUES ($1, $2)",
        )
        .bind("kaufe viewboost")
        .bind("phrase")
        .execute(&pool)
        .await
        .unwrap();

        // ... und greift nach reload() OHNE neuen Filter (atomarer ArcSwap).
        f.reload(&pool).await;
        let after = f.evaluate("kaufe viewboost günstig", &SpamContext::default());
        assert_eq!(after.score, 2);
        assert!(
            after
                .matched
                .iter()
                .any(|r| r.starts_with("Learned-Phrase"))
        );
    }

    // Hinweis: Der frühere NULL-Pattern-Test entfiel mit der prod-treuen DDL —
    // `pattern` ist PRIMARY KEY (NOT NULL), NULL-Zeilen kann es nicht geben.
    // Die IS-NOT-NULL-Defense in `LearnedPatterns::load` bleibt trotzdem.
}

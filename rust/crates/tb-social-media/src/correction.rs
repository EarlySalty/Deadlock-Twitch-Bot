//! Fuzzy-Korrektur eines Whisper-Transkripts gegen das Deadlock-Vokabular
//! (Port von `bot/social_media/transcription/correction.py`).
//!
//! 1. Index aus (term, alias) → canonical bauen.
//! 2. Multi-Word-Aliase (Bigrams/Trigrams) als Phrasen im Volltext ersetzen.
//! 3. Token-für-Token: exakter Lookup, sonst Levenshtein ≤ adaptiver Schwelle.
//! Liefert korrigierten Text + erkannte canonical-Begriffe (dedupe, in
//! Vorkommens-Reihenfolge) für den späteren LLM-Domain-Hinweis. Reine
//! Textverarbeitung (Vokabular lädt der Aufrufer via [`crate::vocab`]).

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::{Captures, NoExpand, Regex};

use crate::vocab::VocabEntry;

/// Ergebnis der Korrektur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionResult {
    pub corrected: String,
    pub detected_terms: Vec<String>,
    pub replacements: Vec<(String, String)>,
}

/// Token-Regex (Python `_TOKEN_RE`): Buchstaben (inkl. Umlaute/ß) + Apostroph.
fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Za-zÄÖÜäöüß']+").unwrap())
}

/// Iterative Levenshtein-Distanz, O(min(|a|,|b|)) Speicher.
fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let (a, b): (Vec<char>, Vec<char>) = if a.chars().count() < b.chars().count() {
        (b.chars().collect(), a.chars().collect())
    } else {
        (a.chars().collect(), b.chars().collect())
    };
    if b.is_empty() {
        return a.len();
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![0usize; b.len() + 1];
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let insert = current[j] + 1;
            let delete = previous[j + 1] + 1;
            let replace = previous[j] + usize::from(ca != cb);
            current[j + 1] = insert.min(delete).min(replace);
        }
        previous = current;
    }
    previous[b.len()]
}

/// Toleranzschwelle abhängig von Token-Länge (Python `_adaptive_threshold`).
fn adaptive_threshold(token_length: usize) -> usize {
    match token_length {
        0..=3 => 0,
        4..=5 => 1,
        _ => 2,
    }
}

/// Vokabular-Index für die Korrektur.
struct VocabIndex {
    exact: HashMap<String, String>,
    multi: HashMap<String, String>,
    singles: Vec<(String, String, i32)>,
}

/// Tokenisiert kleingeschrieben (für den Index-Aufbau).
fn tokens_lower(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    token_re().find_iter(&lower).map(|m| m.as_str().to_string()).collect()
}

fn build_index(entries: &[VocabEntry]) -> VocabIndex {
    let mut exact: HashMap<String, String> = HashMap::new();
    let mut multi: HashMap<String, String> = HashMap::new();
    let mut singles: Vec<(String, String, i32)> = Vec::new();

    for entry in entries {
        let canonical = entry.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        let mut candidates: Vec<&str> = vec![entry.term.as_str(), canonical];
        candidates.extend(entry.aliases.iter().map(String::as_str));
        for cand in candidates {
            let cand_text = cand.trim();
            if cand_text.is_empty() {
                continue;
            }
            let tokens = tokens_lower(cand_text);
            if tokens.is_empty() {
                continue;
            }
            if tokens.len() == 1 {
                let key = tokens[0].clone();
                // exact: first-wins.
                exact.entry(key.clone()).or_insert_with(|| canonical.to_string());
                singles.push((key, canonical.to_string(), entry.weight.max(1)));
            } else {
                multi.insert(tokens.join(" "), canonical.to_string());
            }
        }
    }

    // singles dedup: höchstes Gewicht behalten.
    let mut dedup: HashMap<String, (String, i32)> = HashMap::new();
    for (token, canon, weight) in singles {
        match dedup.get(&token) {
            Some((_, w)) if *w >= weight => {}
            _ => {
                dedup.insert(token, (canon, weight));
            }
        }
    }
    let singles = dedup.into_iter().map(|(t, (c, w))| (t, c, w)).collect();

    VocabIndex { exact, multi, singles }
}

/// Ersetzt Multi-Word-Phrasen (längste zuerst, Word-Boundary, Whitespace-tolerant).
fn replace_multi_word(text: &str, index: &VocabIndex) -> (String, Vec<String>) {
    if index.multi.is_empty() {
        return (text.to_string(), Vec::new());
    }
    let mut patterns: Vec<(&String, &String)> = index.multi.iter().collect();
    patterns.sort_by_key(|(phrase, _)| std::cmp::Reverse(phrase.len()));

    let mut text = text.to_string();
    let mut detected = Vec::new();
    for (phrase, canonical) in patterns {
        let escaped: Vec<String> = phrase.split_whitespace().map(regex::escape).collect();
        let pat = format!(r"(?i)\b{}\b", escaped.join(r"\s+"));
        let Ok(re) = Regex::new(&pat) else { continue };
        let count = re.find_iter(&text).count();
        if count == 0 {
            continue;
        }
        text = re.replace_all(&text, NoExpand(canonical)).into_owned();
        for _ in 0..count {
            detected.push(canonical.clone());
        }
    }
    (text, detected)
}

/// Korrigiert einen einzelnen Token → (Ersetzung, canonical | None).
fn correct_token(token: &str, index: &VocabIndex) -> (String, Option<String>) {
    if token.is_empty() || !token.chars().all(char::is_alphabetic) {
        return (token.to_string(), None);
    }
    let lower = token.to_lowercase();
    if let Some(canonical) = index.exact.get(&lower) {
        return (canonical.clone(), Some(canonical.clone()));
    }
    let threshold = adaptive_threshold(lower.chars().count());
    if threshold == 0 {
        return (token.to_string(), None);
    }
    let lower_len = lower.chars().count();
    let mut best_distance = threshold + 1;
    let mut best_canonical: Option<&str> = None;
    let mut best_weight = -1;
    for (term, canon, weight) in &index.singles {
        let term_len = term.chars().count();
        if term_len.abs_diff(lower_len) > threshold {
            continue;
        }
        let dist = levenshtein(&lower, term);
        if dist < best_distance || (dist == best_distance && *weight > best_weight) {
            best_distance = dist;
            best_canonical = Some(canon);
            best_weight = *weight;
        }
    }
    match best_canonical {
        Some(c) if best_distance <= threshold => (c.to_string(), Some(c.to_string())),
        _ => (token.to_string(), None),
    }
}

/// Korrigiert das Transkript + sammelt erkannte Domain-Begriffe (Python
/// `correct_transcript`). Leere/vokabularlose Eingabe → unverändert.
pub fn correct_transcript(transcript: &str, vocab: &[VocabEntry]) -> CorrectionResult {
    if transcript.trim().is_empty() || vocab.is_empty() {
        return CorrectionResult {
            corrected: transcript.to_string(),
            detected_terms: Vec::new(),
            replacements: Vec::new(),
        };
    }
    let index = build_index(vocab);
    let (text, multi_detected) = replace_multi_word(transcript, &index);

    let mut detected = multi_detected;
    let mut replacements: Vec<(String, String)> = Vec::new();
    let corrected = token_re()
        .replace_all(&text, |caps: &Captures| {
            let original = &caps[0];
            let (replacement, canonical) = correct_token(original, &index);
            if let Some(c) = canonical {
                if replacement.to_lowercase() != original.to_lowercase() {
                    replacements.push((original.to_string(), replacement.clone()));
                }
                detected.push(c);
            }
            replacement
        })
        .into_owned();

    // detected dedupe, Reihenfolge erhalten.
    let mut seen = std::collections::HashSet::new();
    let detected_terms: Vec<String> = detected.into_iter().filter(|t| seen.insert(t.clone())).collect();

    CorrectionResult { corrected, detected_terms, replacements }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, canonical: &str, aliases: &[&str], weight: i32) -> VocabEntry {
        VocabEntry {
            term: term.to_string(),
            canonical: canonical.to_string(),
            category: "hero".to_string(),
            source: "manual".to_string(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            weight,
            updated_at: None,
        }
    }

    #[test]
    fn levenshtein_und_threshold() {
        assert_eq!(levenshtein("haze", "haze"), 0);
        assert_eq!(levenshtein("haze", "hase"), 1);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(adaptive_threshold(3), 0);
        assert_eq!(adaptive_threshold(5), 1);
        assert_eq!(adaptive_threshold(9), 2);
    }

    #[test]
    fn exakte_und_fuzzy_korrektur() {
        let vocab = vec![entry("haze", "Haze", &[], 5), entry("bebop", "Bebop", &[], 5)];
        let r = correct_transcript("ich spiele haze und bebob heute", &vocab);
        // "haze" exakt → Haze; "bebob" fuzzy (dist 1, len 5 → thresh 1) → Bebop.
        assert!(r.corrected.contains("Haze"));
        assert!(r.corrected.contains("Bebop"));
        assert!(r.detected_terms.contains(&"Haze".to_string()));
        assert!(r.detected_terms.contains(&"Bebop".to_string()));
        // Replacements erfasst (Original→Ersetzung), nur bei echter Änderung.
        assert!(r.replacements.iter().any(|(o, n)| o == "bebob" && n == "Bebop"));
    }

    #[test]
    fn multi_word_alias() {
        let mut e = entry("soul_orb", "Soul Orb", &["soul orb"], 5);
        e.term = "soul_orb".to_string();
        let vocab = vec![e];
        let r = correct_transcript("hol dir die soul   orb schnell", &vocab);
        assert!(r.corrected.contains("Soul Orb"));
        assert!(r.detected_terms.contains(&"Soul Orb".to_string()));
    }

    #[test]
    fn kurzer_token_nicht_fuzzy_korrigiert() {
        // "ich" (len 3 → thresh 0) wird nicht gegen "ice" gematcht.
        let vocab = vec![entry("ice", "Ice", &[], 5)];
        let r = correct_transcript("ich bin hier", &vocab);
        assert_eq!(r.corrected, "ich bin hier");
        assert!(r.detected_terms.is_empty());
    }

    #[test]
    fn leeres_oder_kein_vokabular() {
        assert_eq!(correct_transcript("   ", &[]).corrected, "   ");
        let r = correct_transcript("text", &[]);
        assert_eq!(r.corrected, "text");
        assert!(r.detected_terms.is_empty());
    }
}

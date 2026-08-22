//! Lädt und hält die Wissensdokumente; selektiert sie pro Frage.

use std::fs;
use std::path::Path;

use crate::doc::{ist_oeffentlich, parse_doc, KnowledgeDoc, KnowledgeError, Namespace};

#[derive(Debug, Clone, Default)]
pub struct KnowledgeBase {
    docs: Vec<KnowledgeDoc>,
}

impl KnowledgeBase {
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn docs(&self) -> &[KnowledgeDoc] {
        &self.docs
    }

    /// Alle als Tipp ausspielbaren Dokumente (tip_eligible + nicht-leerer tip_text).
    pub fn eligible_tips(&self) -> Vec<&KnowledgeDoc> {
        self.docs
            .iter()
            .filter(|d| d.tip_eligible && !d.tip_text.trim().is_empty())
            .collect()
    }

    /// Lädt `root/bot/*.md` + `root/deadlock/*.md`. Fehlt `root` oder ein
    /// Namespace-Unterordner, wird er übersprungen (kein Fehler). Ein
    /// **Parse-Fehler** in einer vorhandenen `.md` ist strikt ein `Err`
    /// (docs-as-code: kaputte Doku fällt im Test/CI auf).
    pub fn load_from_dir(root: &Path) -> Result<KnowledgeBase, KnowledgeError> {
        let mut docs = Vec::new();
        for ns in [Namespace::Bot, Namespace::Deadlock] {
            let dir = root.join(ns.as_str());
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let mut paths: Vec<_> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
                .collect();
            paths.sort();
            for path in paths {
                let slug = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let raw = fs::read_to_string(&path).map_err(|e| KnowledgeError::Io {
                    path: path.display().to_string(),
                    msg: e.to_string(),
                })?;
                docs.push(parse_doc(&raw, &slug)?);
            }
        }
        Ok(KnowledgeBase { docs })
    }

    /// Deterministische lexikalische Selektion (kein RAG). Score je Doc =
    /// gewichtete Treffer der Frage-Tokens in Titel/Kategorie/tip_flags/Body.
    ///
    /// `audience: None` heisst **oeffentlich**, nicht "alles". Die Aufrufer
    /// sitzen ueberwiegend an ungeschuetzten Oberflaechen (Hilfeseite,
    /// Self-Explainer, `!help` im Twitch-Chat), und ein Doc mit eigener
    /// Zielgruppe traegt Anweisungen oder Interna: `uplink-concierge.md`
    /// zaehlt zum Beispiel auf, welche Admin-Funktionen es gibt. Wer wirklich
    /// alles braucht, nennt seine Zielgruppe ausdruecklich.
    pub fn select(
        &self,
        query: &str,
        namespace: Namespace,
        audience: Option<&str>,
        k: usize,
    ) -> Vec<&KnowledgeDoc> {
        let tokens = tokenize(query);
        if tokens.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(i64, &KnowledgeDoc)> = self
            .docs
            .iter()
            .filter(|d| d.namespace == namespace)
            .filter(|d| match audience {
                Some(a) => d.audience.is_empty() || d.audience == a,
                None => ist_oeffentlich(&d.audience),
            })
            .map(|d| (score_doc(d, &tokens), d))
            .filter(|(s, _)| *s > 0)
            .collect();

        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(a.1.time_to_value.cmp(&b.1.time_to_value))
                .then(a.1.slug.cmp(&b.1.slug))
        });
        scored.into_iter().take(k).map(|(_, d)| d).collect()
    }
}

/// Zerlegt Text in lowercase-Tokens (≥ 2 Zeichen), ohne triviale Stoppwörter.
fn tokenize(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "der", "die", "das", "ein", "eine", "und", "oder", "wie", "was", "wer", "ist", "den",
        "dem", "ich", "wir", "ihr", "mit", "für", "von", "in", "the", "and", "for", "how", "what",
        "does", "the",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 2)
        .filter(|t| !STOP.contains(t))
        .map(|t| t.to_string())
        .collect()
}

/// Gewichtetes Scoring: Titel > Kategorie/Flags > Body (Body gedeckelt).
fn score_doc(doc: &KnowledgeDoc, tokens: &[String]) -> i64 {
    let title = doc.title.to_lowercase();
    let category = doc.category.to_lowercase();
    let flags = doc.tip_flags.join(" ").to_lowercase();
    let body = doc.body.to_lowercase();
    let mut score = 0i64;
    for t in tokens {
        if title.contains(t.as_str()) {
            score += 5;
        }
        if category.contains(t.as_str()) || flags.contains(t.as_str()) {
            score += 2;
        }
        let body_hits = body.matches(t.as_str()).count().min(3) as i64;
        score += body_hits;
    }
    score
}

#[cfg(test)]
mod select_tests {
    use super::*;
    use crate::doc::parse_doc;

    fn kb() -> KnowledgeBase {
        let raids = parse_doc(
            "---\ntitle: Auto-Raid\nnamespace: bot\ncategory: feature\ntime_to_value: 2\n---\nGeht ein Streamer offline, raidet der Bot dessen Zuschauer automatisch weiter.",
            "auto-raid",
        )
        .unwrap();
        let setup = parse_doc(
            "---\ntitle: Einrichtung\nnamespace: bot\ncategory: setup\ntime_to_value: 1\n---\nMit dem Twitch-Konto verbinden und im Dashboard speichern.",
            "einrichtung",
        )
        .unwrap();
        let dl = parse_doc(
            "---\ntitle: Held\nnamespace: deadlock\n---\nEin Deadlock-Thema.",
            "held",
        )
        .unwrap();
        KnowledgeBase {
            docs: vec![raids, setup, dl],
        }
    }

    #[test]
    fn findet_relevantes_dokument() {
        let kb = kb();
        let hits = kb.select("Wie raidet der Bot?", Namespace::Bot, None, 4);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].slug, "auto-raid");
    }

    #[test]
    fn respektiert_namespace() {
        let kb = kb();
        let hits = kb.select("raidet", Namespace::Deadlock, None, 4);
        assert!(hits.is_empty());
    }

    #[test]
    fn unbekannte_frage_liefert_nichts() {
        let kb = kb();
        let hits = kb.select(
            "völlig fremdes thema quantenphysik",
            Namespace::Bot,
            None,
            4,
        );
        assert!(
            hits.is_empty(),
            "ohne lexikalischen Treffer keine Doku → Refusal"
        );
    }

    /// `None` ist der Default aller ungeschuetzten Aufrufer (Hilfeseite,
    /// Self-Explainer, `!help` im Twitch-Chat). Ein Doc mit eigener Zielgruppe
    /// darf dort nie auftauchen, auch nicht als Titel in einer Quellenliste.
    #[test]
    fn ohne_zielgruppe_kommen_nur_oeffentliche_docs() {
        let mut kb = kb();
        kb.docs.push(
            parse_doc(
                "---\ntitle: Uplink Concierge\nnamespace: bot\naudience: concierge\n---\nWie der Bot raidet, steht hier intern.",
                "uplink-concierge",
            )
            .unwrap(),
        );
        let hits = kb.select("raidet", Namespace::Bot, None, 8);
        assert!(
            hits.iter().all(|d| d.slug != "uplink-concierge"),
            "Concierge-Doc an einer ungeschuetzten Oberflaeche: {:?}",
            hits.iter().map(|d| &d.slug).collect::<Vec<_>>()
        );
        assert!(!hits.is_empty(), "die oeffentlichen Docs fehlen dafuer");

        // Wer die Zielgruppe ausdruecklich nennt, bekommt sie weiterhin.
        let intern = kb.select("raidet", Namespace::Bot, Some("concierge"), 8);
        assert!(intern.iter().any(|d| d.slug == "uplink-concierge"));
    }

    #[test]
    fn respektiert_top_k() {
        let kb = kb();
        let hits = kb.select("bot verbinden dashboard raidet", Namespace::Bot, None, 1);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn eligible_tips_filtert_korrekt() {
        let a = parse_doc(
            "---\ntitle: A\nnamespace: bot\ntip_eligible: true\ntip_text: Tipp A\n---\nx",
            "a",
        )
        .unwrap();
        let b = parse_doc(
            "---\ntitle: B\nnamespace: bot\ntip_eligible: false\n---\nx",
            "b",
        )
        .unwrap();
        let c = parse_doc(
            "---\ntitle: C\nnamespace: bot\ntip_eligible: true\n---\nx",
            "c",
        )
        .unwrap();
        let kb = KnowledgeBase {
            docs: vec![a, b, c],
        };
        let tips = kb.eligible_tips();
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0].slug, "a");
    }
}

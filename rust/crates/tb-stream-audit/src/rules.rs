//! Regelbasierte Erkennung und Redaktion, portiert aus
//! `bot/stream_coaching_audit/service.py` (entfernt beim Abriss der
//! Python-Laufzeit am 21.07.2026).
//!
//! Zwei Eigenschaften sind hier absichtlich so und nicht anders:
//!
//! Die Muster erlauben zwischen den Buchstaben beliebige Nicht-Wort-Zeichen
//! (`n i-g_g e r`). Wer sie auf reine Wortsuche vereinfacht, verliert genau die
//! Verschleierungen, wegen derer die Regeln existieren.
//!
//! [`redact_text`] laeuft ueber *dieselben* Muster wie die Erkennung. Damit ist
//! ausgeschlossen, dass ein Treffer gemeldet, aber sein Wortlaut nicht
//! geschwaerzt wird. Eine zweite, eigene Redaktionsliste waere genau die stille
//! Divergenz, die man erst bemerkt, wenn ein Slur im Bericht steht.

use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};

/// Kategorie, Schwere und Begruendung einer Regel.
pub struct Rule {
    pub muster: &'static str,
    pub kategorie: &'static str,
    pub schwere: &'static str,
    pub begruendung: &'static str,
}

pub const REGELN: &[Rule] = &[
    Rule {
        muster: r"(?i)\bn[\W_]*[i1!|][\W_]*g[\W_]*g[\W_]*(?:e[\W_]*r|a)(?:[\W_]*s)?\b",
        kategorie: "hate_speech_slur",
        schwere: "high",
        begruendung: "Moegliche rassistische Beleidigung. Kontext und Transkript manuell pruefen.",
    },
    Rule {
        muster: r"(?i)\b(?:f[\W_]*a[\W_]*g(?:[\W_]*g[\W_]*o[\W_]*t)?|schwuchtel)\w*\b",
        kategorie: "hate_speech_slur",
        schwere: "high",
        begruendung:
            "Moegliche diskriminierende Beleidigung. Kontext und Transkript manuell pruefen.",
    },
    Rule {
        muster: r"(?i)\b(?:ich\s+(?:bring|mach)\s+dich\s+um|kill\s+yourself|kys)\b",
        kategorie: "threat_or_self_harm",
        schwere: "medium",
        begruendung:
            "Moegliche Drohung oder Aufforderung zur Selbstverletzung. Kontext manuell pruefen.",
    },
];

fn kompiliert() -> &'static [Regex] {
    static CACHE: OnceLock<Vec<Regex>> = OnceLock::new();
    CACHE.get_or_init(|| {
        REGELN
            .iter()
            .map(|r| Regex::new(r.muster).expect("Regel-Muster ist konstant und getestet"))
            .collect()
    })
}

fn leerraum_normalisieren(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Schwaerzt bekannte Slurs, bevor ein Zitat das fluechtige Transkript verlaesst.
pub fn redact_text(text: &str) -> String {
    let mut aktuell = text.to_owned();
    for regex in kompiliert() {
        aktuell = regex.replace_all(&aktuell, "[REDACTED]").into_owned();
    }
    leerraum_normalisieren(&aktuell)
}

/// SHA-256 ueber den Rohtext. Der Bericht traegt nur den Hash, damit sich zwei
/// Funde vergleichen lassen, ohne den Wortlaut zu speichern.
pub fn evidence_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Ein Regeltreffer in einem Segment.
pub struct Treffer {
    pub kategorie: &'static str,
    pub schwere: &'static str,
    pub begruendung: &'static str,
    /// Bereits geschwaerzter Ausschnitt rund um die Fundstelle.
    pub zitat_redigiert: String,
    pub zitat_hash: String,
}

const AUSSCHNITT_RADIUS: usize = 80;

fn ausschnitt(text: &str, start: usize, ende: usize) -> String {
    let von = text[..start]
        .char_indices()
        .rev()
        .nth(AUSSCHNITT_RADIUS)
        .map_or(0, |(i, _)| i);
    let bis = text[ende..]
        .char_indices()
        .nth(AUSSCHNITT_RADIUS)
        .map_or(text.len(), |(i, _)| ende + i);
    text[von..bis].to_owned()
}

/// Alle Regeltreffer eines Segments, in Regelreihenfolge.
pub fn treffer_im_text(text: &str) -> Vec<Treffer> {
    let mut raus = Vec::new();
    for (regel, regex) in REGELN.iter().zip(kompiliert()) {
        for fund in regex.find_iter(text) {
            let roh = ausschnitt(text, fund.start(), fund.end());
            raus.push(Treffer {
                kategorie: regel.kategorie,
                schwere: regel.schwere,
                begruendung: regel.begruendung,
                zitat_redigiert: redact_text(&roh),
                zitat_hash: evidence_hash(&roh),
            });
        }
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn muster_kompilieren_alle() {
        assert_eq!(kompiliert().len(), REGELN.len());
    }

    #[test]
    fn verschleierte_schreibweise_wird_erkannt() {
        // Genau dafuer sind die [\W_]*-Luecken da.
        for probe in ["n i g g e r", "n-i-g-g-a", "N_I_G_G_E_R"] {
            assert!(!treffer_im_text(probe).is_empty(), "nicht erkannt: {probe}");
        }
    }

    #[test]
    fn harmloser_text_erzeugt_keinen_treffer() {
        let probe = "Guten Abend zusammen, heute spielen wir eine Runde Deadlock.";
        assert!(treffer_im_text(probe).is_empty());
    }

    #[test]
    fn drohung_wird_mit_mittlerer_schwere_erkannt() {
        let treffer = treffer_im_text("und dann sagt er ich bring dich um, echt jetzt");
        assert_eq!(treffer.len(), 1);
        assert_eq!(treffer[0].kategorie, "threat_or_self_harm");
        assert_eq!(treffer[0].schwere, "medium");
    }

    #[test]
    fn redaktion_deckt_jede_erkennungsregel_ab() {
        // Der eigentliche Sicherheitsvertrag: was erkannt wird, wird geschwaerzt.
        for probe in ["so ein n i g g e r", "du schwuchtel", "kill yourself"] {
            let redigiert = redact_text(probe);
            assert!(
                redigiert.contains("[REDACTED]"),
                "nicht geschwaerzt: {probe}"
            );
            assert!(
                treffer_im_text(&redigiert).is_empty(),
                "nach Redaktion noch auffindbar: {probe}"
            );
        }
    }

    #[test]
    fn zitat_im_treffer_ist_bereits_geschwaerzt() {
        let treffer = treffer_im_text("er hat woertlich schwuchtel gesagt");
        assert_eq!(treffer.len(), 1);
        assert!(treffer[0].zitat_redigiert.contains("[REDACTED]"));
        assert!(!treffer[0]
            .zitat_redigiert
            .to_lowercase()
            .contains("schwuchtel"));
    }

    #[test]
    fn hash_kommt_vom_rohtext_nicht_vom_geschwaerzten() {
        let treffer = treffer_im_text("du schwuchtel");
        assert_ne!(
            treffer[0].zitat_hash,
            evidence_hash(&treffer[0].zitat_redigiert)
        );
    }

    #[test]
    fn leerraum_wird_normalisiert() {
        assert_eq!(redact_text("  viel   Raum \n hier "), "viel Raum hier");
    }

    #[test]
    fn ausschnitt_bricht_nicht_in_der_mitte_eines_zeichens() {
        // Umlaute sind mehrere Bytes; ein Byte-Slice wuerde hier panicken.
        let text = format!("{} schwuchtel {}", "ä".repeat(200), "ö".repeat(200));
        let treffer = treffer_im_text(&text);
        assert_eq!(treffer.len(), 1);
    }
}

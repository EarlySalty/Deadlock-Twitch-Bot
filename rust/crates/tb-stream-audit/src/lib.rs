//! Privates Coaching-Audit autorisierter Twitch-Streams.
//!
//! Portiert aus `bot/stream_coaching_audit/` und `scripts/audit_stream_tos.py`,
//! die beim Abriss der Python-Laufzeit am 21.07.2026 entfielen. Der zugehoerige
//! Dienst `deadlock-twitch-stream-coaching-watch.service` zeigte danach auf ein
//! geloeschtes Startskript und scheiterte mit `203/EXEC`.
//!
//! # Ablauf
//!
//! Aufgenommen wird parallel, je sendendem Kanal ein eigener Task, und in
//! Bloecke geschnitten. Ausgewertet wird seriell, ein Block nach dem anderen.
//! Auf einer Maschine ohne GPU ist das der Unterschied zwischen "laeuft" und
//! "kommt nicht hinterher": drei gleichzeitig transkribierte Streams teilten
//! sich dieselben CPU-Kerne wie das Modell. Aufnehmen dagegen kostet fast
//! nichts, und wer seriell aufnimmt, verpasst zwei Drittel jedes Streams.
//!
//! Bloecke statt eines Mitschnitts am Stueck, weil das Modell darauf ausgelegt
//! ist und ein abgebrochener Stream so nur den letzten Block kostet.
//!
//! # Was den Rechner nicht verlaesst
//!
//! Die Transkription laeuft lokal gegen `deadlock-stt-server.service`. Zitate
//! werden vor jeder Weitergabe durch [`rules::redact_text`] geschwaerzt, und
//! zwar mit denselben Mustern, die sie gefunden haben.

pub mod config;
pub mod llm;
pub mod melden;
pub mod plan;
pub mod report;
pub mod rules;

/// Ein transkribierter Abschnitt mit Zeitbezug zum Stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    pub start_sekunden: f64,
    pub ende_sekunden: f64,
    pub text: String,
}

/// Ein Fund. `zitat_roh` bleibt bewusst aussen vor: der Bericht traegt nur die
/// geschwaerzte Fassung und den Hash des Originals.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fund {
    pub segment_id: String,
    pub start_sekunden: f64,
    pub ende_sekunden: f64,
    pub kategorie: String,
    pub schwere: String,
    pub erkenner: String,
    pub sicherheit: String,
    pub begruendung: String,
    pub zitat_redigiert: String,
    pub zitat_hash: String,
}

/// Regelbasierte Funde ueber alle Segmente.
pub fn regelfunde(segmente: &[Segment]) -> Vec<Fund> {
    let mut raus = Vec::new();
    for segment in segmente {
        for treffer in rules::treffer_im_text(&segment.text) {
            raus.push(Fund {
                segment_id: segment.id.clone(),
                start_sekunden: segment.start_sekunden,
                ende_sekunden: segment.ende_sekunden,
                kategorie: treffer.kategorie.to_owned(),
                schwere: treffer.schwere.to_owned(),
                erkenner: "regel".to_owned(),
                sicherheit: "high".to_owned(),
                begruendung: treffer.begruendung.to_owned(),
                zitat_redigiert: treffer.zitat_redigiert,
                zitat_hash: treffer.zitat_hash,
            });
        }
    }
    raus
}

/// Fasst nur wirklich gleiche Funde zusammen.
///
/// Frueher fielen Regel- und Modellfund derselben Kategorie im selben Segment
/// zusammen. Das ging nicht auf: ein Modellfund belegt immer das ganze Segment
/// und enthaelt damit jeden Regelausschnitt, egal welchen Vorfall das Modell
/// tatsaechlich meinte. Ein zweiter, eigenstaendiger Befund verschwand so
/// still. Lieber zwei Zeilen im Bericht als ein verlorener Fund - zusammen
/// kommt nur, was in Segment, Kategorie, Erkenner und Beleg uebereinstimmt.
pub fn funde_zusammenfassen(mut funde: Vec<Fund>) -> Vec<Fund> {
    funde.sort_by(|a, b| {
        a.segment_id
            .cmp(&b.segment_id)
            .then_with(|| a.kategorie.cmp(&b.kategorie))
            .then_with(|| (a.erkenner != "regel").cmp(&(b.erkenner != "regel")))
            .then_with(|| a.zitat_hash.cmp(&b.zitat_hash))
    });
    funde.dedup_by(|a, b| {
        a.segment_id == b.segment_id
            && a.kategorie == b.kategorie
            && a.erkenner == b.erkenner
            && a.zitat_hash == b.zitat_hash
    });
    funde
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(id: &str, text: &str) -> Segment {
        Segment {
            id: id.to_owned(),
            start_sekunden: 0.0,
            ende_sekunden: 30.0,
            text: text.to_owned(),
        }
    }

    #[test]
    fn sauberes_segment_erzeugt_keinen_fund() {
        let funde = regelfunde(&[segment("s00001", "Willkommen zurueck im Stream.")]);
        assert!(funde.is_empty());
    }

    #[test]
    fn fund_traegt_segment_und_zeitbezug() {
        let funde = regelfunde(&[Segment {
            id: "s00007".to_owned(),
            start_sekunden: 120.5,
            ende_sekunden: 150.0,
            text: "du schwuchtel".to_owned(),
        }]);
        assert_eq!(funde.len(), 1);
        assert_eq!(funde[0].segment_id, "s00007");
        assert_eq!(funde[0].start_sekunden, 120.5);
        assert_eq!(funde[0].erkenner, "regel");
    }

    #[test]
    fn fund_enthaelt_kein_rohzitat() {
        let funde = regelfunde(&[segment("s1", "so ein schwuchtel")]);
        assert!(!funde[0]
            .zitat_redigiert
            .to_lowercase()
            .contains("schwuchtel"));
    }

    #[test]
    fn regel_und_modellfund_bleiben_beide_stehen() {
        // Der Modellfund entsteht wie im Betrieb: er belegt das ganze Segment,
        // der Regelfund nur seinen Ausschnitt. Die Hashes sind deshalb
        // verschieden - zusammengefasst wird trotzdem.
        let mut funde = regelfunde(&[segment("s1", "du schwuchtel")]);
        let ganzes_segment = "irgendwas davor du schwuchtel und noch etwas danach";
        funde.push(Fund {
            segment_id: funde[0].segment_id.clone(),
            start_sekunden: funde[0].start_sekunden,
            ende_sekunden: funde[0].ende_sekunden,
            kategorie: funde[0].kategorie.clone(),
            schwere: funde[0].schwere.clone(),
            erkenner: "modell".to_owned(),
            sicherheit: "medium".to_owned(),
            begruendung: "Modellbegruendung".to_owned(),
            zitat_redigiert: rules::redact_text(ganzes_segment),
            zitat_hash: rules::evidence_hash(ganzes_segment),
        });
        assert_ne!(
            funde[0].zitat_hash, funde[1].zitat_hash,
            "die Belege unterscheiden sich - genau darum ging der Fehler"
        );
        let zusammen = funde_zusammenfassen(funde);
        assert_eq!(
            zusammen.len(),
            2,
            "verschiedene Belege bleiben zwei Zeilen - ein Modellfund belegt \
immer das ganze Segment und darf keinen Regelfund verdraengen"
        );
        assert_eq!(zusammen[0].erkenner, "regel", "die Regel steht oben");
    }

    #[test]
    fn modellfund_ueber_eine_andere_stelle_bleibt_erhalten() {
        // Gleiches Segment, gleiche Kategorie, aber ein anderer Vorfall: der
        // Modellfund belegt Worte, die im Regelbeleg nicht vorkommen.
        let mut funde = regelfunde(&[segment("s1", "du schwuchtel")]);
        funde.push(Fund {
            segment_id: funde[0].segment_id.clone(),
            start_sekunden: funde[0].start_sekunden,
            ende_sekunden: funde[0].ende_sekunden,
            kategorie: funde[0].kategorie.clone(),
            schwere: "medium".to_owned(),
            erkenner: "modell".to_owned(),
            sicherheit: "medium".to_owned(),
            begruendung: "andere Stelle".to_owned(),
            zitat_redigiert: "ganz andere Worte ohne Ueberschneidung".to_owned(),
            zitat_hash: rules::evidence_hash("ganz andere Worte ohne Ueberschneidung"),
        });
        assert_eq!(
            funde_zusammenfassen(funde).len(),
            2,
            "ein Modellfund ueber eine andere Stelle darf nicht verschwinden"
        );
    }

    #[test]
    fn zwei_regelfunde_derselben_kategorie_bleiben_zwei() {
        // Zwei Vorfaelle derselben Kategorie, weit genug auseinander fuer
        // eigene Ausschnitte: beide bleiben stehen.
        let weit = "harmloser Fuelltext ".repeat(20);
        let funde = regelfunde(&[segment(
            "s1",
            &format!("du schwuchtel {weit} und spaeter du faggot"),
        )]);
        assert_eq!(funde.len(), 2);
        assert_eq!(funde_zusammenfassen(funde).len(), 2);
    }

    #[test]
    fn wortgleiche_treffer_im_selben_ausschnitt_zaehlen_einmal() {
        // Derselbe Beleg, derselbe Erkenner: eine Zeile reicht.
        let funde = regelfunde(&[segment("s1", "du schwuchtel du schwuchtel")]);
        let zusammen = funde_zusammenfassen(funde);
        assert_eq!(zusammen.len(), 1);
    }

    #[test]
    fn zwei_kategorien_im_selben_segment_bleiben_getrennt() {
        let funde = regelfunde(&[segment("s1", "du schwuchtel, kill yourself")]);
        assert_eq!(
            funde_zusammenfassen(funde).len(),
            2,
            "Beleidigung und Drohung sind zwei Befunde, kein doppelter"
        );
    }

    #[test]
    fn verschiedene_segmente_bleiben_getrennt() {
        let funde = regelfunde(&[
            segment("s1", "du schwuchtel"),
            segment("s2", "du schwuchtel"),
        ]);
        assert_eq!(funde_zusammenfassen(funde).len(), 2);
    }
}

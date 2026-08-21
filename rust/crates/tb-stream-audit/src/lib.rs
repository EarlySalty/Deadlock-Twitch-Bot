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

/// Ein Fund. Die persistente Akte (.md/.json) traegt nur die geschwaerzte
/// Fassung und den Hash des Originals.
///
/// `zitat_roh` ist die einzige Ausnahme, und mit Absicht eng gehalten: es traegt
/// den unredigierten Wortlaut nur im Speicher und nur bis zur Discord-DM an den
/// Admin. `skip_serializing` haelt es aus jeder Datei heraus, `default` laesst
/// einen von Platte nachgeladenen Bericht ohne dieses Feld weiter
/// deserialisieren. Wer die Akte liest, sieht also weiter kein Rohzitat; nur der
/// Admin, der den Fund melden soll, bekommt den Klartext einmalig zugestellt.
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
    #[serde(default, skip_serializing)]
    pub zitat_roh: String,
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
                zitat_roh: treffer.zitat_roh,
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

/// Ob ein Fund den Streamer real eine Twitch-Sperre riskieren laesst.
///
/// Twitch ahndet Slurs, Herabwuerdigung geschuetzter Gruppen und Drohungen /
/// Selbstverletzung als "Hateful Conduct" mit Nulltoleranz - unabhaengig von
/// der Schwere, ein Slur bleibt ein Slur. Allgemeine Beleidigung
/// (`harassment`) und sexuelle Sprache (`sexual_content`) greift Twitch nur
/// auf, wenn sie deutlich wird; sie zaehlen darum erst ab `high`. Alles andere
/// (`sonstiges`, `sonstiges:*`) ist Coaching-Material fuers Protokoll, aber
/// kein Grund fuer eine Meldung - genau die "safe Dinger", die kein Bann-Risiko
/// tragen.
pub fn tos_meldewuerdig(fund: &Fund) -> bool {
    match fund.kategorie.as_str() {
        "hate_speech_slur" | "discriminatory_speech" | "threat_or_self_harm" => true,
        "harassment" | "sexual_content" => fund.schwere == "high",
        _ => false,
    }
}

/// Behaelt nur die Funde, die ein echtes Twitch-Bann-Risiko tragen. Der volle
/// Fundsatz bleibt im Platten-Bericht; die Admin-DM soll nur melden, wo Twitch
/// tatsaechlich eingreifen wuerde.
pub fn nur_tos_meldewuerdig(funde: &[Fund]) -> Vec<Fund> {
    funde
        .iter()
        .filter(|f| tos_meldewuerdig(f))
        .cloned()
        .collect()
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

    fn fund(kategorie: &str, schwere: &str) -> Fund {
        Fund {
            segment_id: "s1".to_owned(),
            start_sekunden: 0.0,
            ende_sekunden: 30.0,
            kategorie: kategorie.to_owned(),
            schwere: schwere.to_owned(),
            erkenner: "modell".to_owned(),
            sicherheit: "high".to_owned(),
            begruendung: String::new(),
            zitat_redigiert: String::new(),
            zitat_roh: String::new(),
            zitat_hash: String::new(),
        }
    }

    #[test]
    fn slurs_und_drohungen_sind_immer_meldewuerdig() {
        for kat in [
            "hate_speech_slur",
            "discriminatory_speech",
            "threat_or_self_harm",
        ] {
            assert!(
                tos_meldewuerdig(&fund(kat, "low")),
                "{kat} low sollte melden"
            );
            assert!(
                tos_meldewuerdig(&fund(kat, "high")),
                "{kat} high sollte melden"
            );
        }
    }

    #[test]
    fn beleidigung_und_sex_erst_ab_high() {
        for kat in ["harassment", "sexual_content"] {
            assert!(!tos_meldewuerdig(&fund(kat, "low")));
            assert!(!tos_meldewuerdig(&fund(kat, "medium")));
            assert!(tos_meldewuerdig(&fund(kat, "high")));
        }
    }

    #[test]
    fn sonstiges_ist_nie_meldewuerdig() {
        assert!(!tos_meldewuerdig(&fund("sonstiges", "high")));
        assert!(!tos_meldewuerdig(&fund("sonstiges:politik", "high")));
    }

    #[test]
    fn nur_tos_meldewuerdig_filtert_die_safe_dinger() {
        let alle = vec![
            fund("hate_speech_slur", "low"),
            fund("harassment", "low"),
            fund("sexual_content", "medium"),
            fund("harassment", "high"),
        ];
        let gemeldet = nur_tos_meldewuerdig(&alle);
        assert_eq!(gemeldet.len(), 2);
        assert_eq!(gemeldet[0].kategorie, "hate_speech_slur");
        assert_eq!(gemeldet[1].kategorie, "harassment");
        assert_eq!(gemeldet[1].schwere, "high");
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
            zitat_roh: ganzes_segment.to_owned(),
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
            zitat_roh: "ganz andere Worte ohne Ueberschneidung".to_owned(),
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

    #[test]
    fn regelfund_traegt_rohzitat_fuer_den_admin() {
        // Der Admin-Report braucht den Klartext, um einen Verstoss zu melden.
        let funde = regelfunde(&[segment("s1", "so ein schwuchtel")]);
        assert!(
            funde[0].zitat_roh.to_lowercase().contains("schwuchtel"),
            "das Rohzitat fehlt: {:?}",
            funde[0].zitat_roh
        );
    }

    #[test]
    fn akte_traegt_kein_rohzitat() {
        // Der eigentliche Vertrag: was auf die Platte geht, enthaelt den
        // Wortlaut nicht - nur die fluechtige DM zeigt ihn.
        let funde = regelfunde(&[segment("s1", "so ein schwuchtel")]);
        let json = serde_json::to_string(&funde[0]).expect("JSON");
        assert!(
            !json.to_lowercase().contains("schwuchtel"),
            "der Wortlaut steht in der Akte: {json}"
        );
        assert!(
            !json.contains("zitat_roh"),
            "das Rohzitat-Feld gehoert nicht in die Datei: {json}"
        );
    }
}

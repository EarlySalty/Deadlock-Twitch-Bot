//! Bericht als Markdown und JSON.
//!
//! Der Bericht ist das, was ein Mensch liest, und das Einzige, was den Rechner
//! verlaesst. Deshalb steht in ihm nie ein Rohzitat, sondern die geschwaerzte
//! Fassung und der Hash. Wer den Wortlaut braucht, geht ueber den Zeitstempel
//! in die Aufnahme, und die liegt lokal.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::Fund;

/// Kopfdaten eines Laufs.
#[derive(Debug, Clone, Serialize)]
pub struct Bericht {
    pub lauf_id: String,
    pub erstellt_am: String,
    pub quelle: String,
    pub kanal: String,
    pub transkription: String,
    pub modell: String,
    pub anbieter: String,
    /// Ob das Rohtranskript auf der Platte bleibt. Der Bericht sagt es
    /// ausdruecklich, damit niemand raten muss, wo Wortlaut liegt.
    pub transkript_behalten: bool,
    pub segmente: usize,
    pub funde: Vec<Fund>,
}

/// Sekunden als `hh:mm:ss`.
pub fn zeit(sekunden: f64) -> String {
    let gesamt = sekunden.max(0.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        gesamt / 3600,
        (gesamt % 3600) / 60,
        gesamt % 60
    )
}

fn schwere_rang(schwere: &str) -> u8 {
    match schwere {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    }
}

/// Funde nach Schwere, dann nach Zeit. Wer den Bericht ueberfliegt, soll das
/// Dringendste zuerst sehen.
pub fn sortiert(mut funde: Vec<Fund>) -> Vec<Fund> {
    funde.sort_by(|a, b| {
        schwere_rang(&a.schwere)
            .cmp(&schwere_rang(&b.schwere))
            .then_with(|| {
                a.start_sekunden
                    .partial_cmp(&b.start_sekunden)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    funde
}

pub fn markdown(bericht: &Bericht) -> String {
    let mut zeilen = vec![
        format!("# Coaching-Audit {}", bericht.kanal),
        String::new(),
        format!("- Lauf: `{}`", bericht.lauf_id),
        format!("- Erstellt: {}", bericht.erstellt_am),
        format!("- Quelle: {}", bericht.quelle),
        format!(
            "- Transkription: {} ({}), lokal",
            bericht.transkription, bericht.modell
        ),
        format!("- Bewertung: {}", bericht.anbieter),
        format!("- Segmente: {}", bericht.segmente),
        format!(
            "- Rohtranskript behalten: {}",
            if bericht.transkript_behalten {
                "ja"
            } else {
                "nein"
            }
        ),
        String::new(),
    ];

    if bericht.funde.is_empty() {
        zeilen.push("Keine Auffaelligkeiten.".to_owned());
        zeilen.push(String::new());
        return zeilen.join("\n");
    }

    zeilen.push(format!("## Funde ({})", bericht.funde.len()));
    zeilen.push(String::new());
    for fund in &bericht.funde {
        zeilen.push(format!(
            "### {} bis {} — {} ({})",
            zeit(fund.start_sekunden),
            zeit(fund.ende_sekunden),
            fund.kategorie,
            fund.schwere
        ));
        zeilen.push(String::new());
        zeilen.push(format!(
            "- Erkenner: {}, Sicherheit: {}",
            fund.erkenner, fund.sicherheit
        ));
        if !fund.begruendung.is_empty() {
            zeilen.push(format!("- Begruendung: {}", fund.begruendung));
        }
        zeilen.push(format!("- Zitat (geschwaerzt): {}", fund.zitat_redigiert));
        zeilen.push(format!("- Beleg-Hash: `{}`", &fund.zitat_hash[..16]));
        zeilen.push(String::new());
    }
    zeilen.join("\n")
}

/// Kurzfassung fuer die Discord-DM. Discord kappt lange Nachrichten, und eine
/// abgeschnittene Liste liest sich wie eine vollstaendige.
pub fn dm_text(bericht: &Bericht, grenze: usize) -> String {
    let kopf = if bericht.funde.is_empty() {
        format!(
            "Coaching-Audit {}: keine Auffaelligkeiten ({} Segmente).",
            bericht.kanal, bericht.segmente
        )
    } else {
        let hoch = bericht.funde.iter().filter(|f| f.schwere == "high").count();
        format!(
            "Coaching-Audit {}: {} Funde, davon {} hoch ({} Segmente).",
            bericht.kanal,
            bericht.funde.len(),
            hoch,
            bericht.segmente
        )
    };

    let mut text = kopf;
    for fund in bericht.funde.iter().take(grenze) {
        text.push_str(&format!(
            "\n• {} {} ({})",
            zeit(fund.start_sekunden),
            fund.kategorie,
            fund.schwere
        ));
    }
    if bericht.funde.len() > grenze {
        text.push_str(&format!(
            "\n… {} weitere im Bericht",
            bericht.funde.len() - grenze
        ));
    }
    text
}

pub fn lauf_id(jetzt: DateTime<Utc>, kanal: &str) -> String {
    format!("{}-{}", jetzt.format("%Y%m%dT%H%M%SZ"), kanal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fund(schwere: &str, start: f64, kategorie: &str) -> Fund {
        Fund {
            segment_id: "s1".to_owned(),
            start_sekunden: start,
            ende_sekunden: start + 30.0,
            kategorie: kategorie.to_owned(),
            schwere: schwere.to_owned(),
            erkenner: "regel".to_owned(),
            sicherheit: "high".to_owned(),
            begruendung: "Begruendung".to_owned(),
            zitat_redigiert: "so ein [REDACTED] echt".to_owned(),
            zitat_hash: "a".repeat(64),
        }
    }

    fn bericht(funde: Vec<Fund>) -> Bericht {
        Bericht {
            lauf_id: "20260813T200000Z-testkanal".to_owned(),
            erstellt_am: "2026-08-13T20:00:00Z".to_owned(),
            quelle: "live".to_owned(),
            kanal: "testkanal".to_owned(),
            transkription: "faster-whisper".to_owned(),
            modell: "large-v3-turbo".to_owned(),
            anbieter: "fireworks".to_owned(),
            transkript_behalten: true,
            segmente: 12,
            funde,
        }
    }

    #[test]
    fn zeit_formatiert_stunden_minuten_sekunden() {
        assert_eq!(zeit(0.0), "00:00:00");
        assert_eq!(zeit(3725.4), "01:02:05");
    }

    #[test]
    fn negative_zeit_wird_nicht_negativ() {
        assert_eq!(zeit(-5.0), "00:00:00");
    }

    #[test]
    fn hohe_schwere_steht_vorn() {
        let sortierte = sortiert(vec![
            fund("low", 10.0, "a"),
            fund("high", 900.0, "b"),
            fund("medium", 5.0, "c"),
        ]);
        let schweren: Vec<_> = sortierte.iter().map(|f| f.schwere.as_str()).collect();
        assert_eq!(schweren, vec!["high", "medium", "low"]);
    }

    #[test]
    fn gleiche_schwere_nach_zeit() {
        let sortierte = sortiert(vec![fund("high", 900.0, "a"), fund("high", 30.0, "b")]);
        assert_eq!(sortierte[0].start_sekunden, 30.0);
    }

    #[test]
    fn bericht_ohne_funde_sagt_das_deutlich() {
        let text = markdown(&bericht(vec![]));
        assert!(text.contains("Keine Auffaelligkeiten."));
        assert!(!text.contains("## Funde"));
    }

    #[test]
    fn bericht_nennt_zeit_kategorie_und_geschwaerztes_zitat() {
        let text = markdown(&bericht(vec![fund("high", 3725.0, "hate_speech_slur")]));
        assert!(text.contains("01:02:05"));
        assert!(text.contains("hate_speech_slur"));
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn bericht_nennt_lokale_transkription_und_aufbewahrung() {
        let text = markdown(&bericht(vec![]));
        assert!(text.contains("lokal"));
        assert!(text.contains("Rohtranskript behalten: ja"));
    }

    #[test]
    fn dm_zaehlt_hohe_funde_und_kappt_die_liste() {
        let funde: Vec<_> = (0..10)
            .map(|i| fund("high", i as f64 * 60.0, "harassment"))
            .collect();
        let text = dm_text(&bericht(funde), 3);
        assert!(text.contains("10 Funde, davon 10 hoch"));
        assert_eq!(text.matches('•').count(), 3);
        assert!(text.contains("7 weitere im Bericht"));
    }

    #[test]
    fn dm_ohne_funde_bleibt_einzeilig() {
        let text = dm_text(&bericht(vec![]), 3);
        assert!(text.contains("keine Auffaelligkeiten"));
        assert!(!text.contains('•'));
    }

    #[test]
    fn lauf_id_traegt_zeit_und_kanal() {
        let jetzt = DateTime::parse_from_rfc3339("2026-08-13T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(lauf_id(jetzt, "testkanal"), "20260813T200000Z-testkanal");
    }
}

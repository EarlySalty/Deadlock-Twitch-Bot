//! Bericht als Markdown und JSON.
//!
//! Der Bericht ist das, was ein Mensch liest. Jeder Beleg laeuft durch die
//! Schwaerzung und traegt den Hash des Originals - die Schwaerzung kennt aber
//! nur die drei Muster aus [`crate::rules`]. Was ein Modellfund sonst an
//! Wortlaut im Segment mitbringt, steht im Bericht; er ist damit kein
//! zitatfreier Text, sondern eine zugriffsbeschraenkte Akte (Modus 0600). Der Wortlaut ist danach in aller Regel weg: die
//! Aufnahme wird nach erfolgreicher Auswertung geloescht, und das Transkript
//! bleibt nur liegen, wenn `STREAM_AUDIT_KEEP_TRANSCRIPT` vorher gesetzt war.
//! Der Hash kann ein vorgelegtes Zitat bestaetigen, aber keines herstellen.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Fund;

/// Kopfdaten eines Laufs.
///
/// `Deserialize`, weil ein Bericht, dessen Meldung nicht rausging, spaeter
/// erneut gemeldet wird - aus der Datei, nicht aus einer zweiten Auswertung.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bericht {
    pub lauf_id: String,
    pub erstellt_am: String,
    pub quelle: String,
    pub kanal: String,
    pub transkription: String,
    pub modell: String,
    /// Lief die Transkription auf diesem Rechner? Frueher stand hier fest
    /// "lokal" im Bericht - eine Herkunftsangabe, die bei einem entfernten
    /// STT-Endpunkt schlicht falsch war.
    pub transkription_lokal: bool,
    pub anbieter: String,
    /// Modell des Sprachmodells. `modell` traegt das Whisper-Modell; ohne
    /// dieses Feld waere aus dem Bericht nicht mehr zu erkennen, wer eine
    /// Einschaetzung abgegeben hat.
    pub llm_modell: String,
    /// Ob das Rohtranskript auf der Platte bleibt. Der Bericht sagt es
    /// ausdruecklich, damit niemand raten muss, wo Wortlaut liegt.
    pub transkript_behalten: bool,
    pub segmente: usize,
    /// Ob der Modellschritt vollstaendig durchlief. Faellt er aus, stuende
    /// sonst "keine Auffaelligkeiten" im Bericht, obwohl nie jemand
    /// hingeschaut hat - Stille laesst sich nicht von Sauberkeit
    /// unterscheiden.
    pub modell_geprueft: bool,
    /// Grund, falls der Modellschritt ausfiel oder uebersprungen wurde.
    pub modell_hinweis: String,
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
        // "angefragt": der lokale STT-Dienst ignoriert den Modellnamen aus der
        // Anfrage und laedt, was in seiner eigenen Konfiguration steht. Nennt
        // seine Antwort kein Modell, ist dieser Name nur der gewuenschte.
        format!(
            "- Transkription: {} (Modell angefragt: {}), {}",
            bericht.transkription,
            bericht.modell,
            if bericht.transkription_lokal {
                "lokal"
            } else {
                "ENTFERNT - Audio hat den Rechner verlassen"
            }
        ),
        format!(
            "- Bewertung: {} ({}){}",
            bericht.anbieter,
            if bericht.llm_modell.is_empty() {
                "Modell unbekannt"
            } else {
                &bericht.llm_modell
            },
            if bericht.modell_geprueft {
                String::new()
            } else {
                format!(" — NICHT GELAUFEN: {}", bericht.modell_hinweis)
            }
        ),
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
        zeilen.push(if bericht.modell_geprueft {
            "Keine Auffaelligkeiten.".to_owned()
        } else {
            format!(
                "Keine Regelfunde. **Der Modellschritt lief nicht** ({}), diese Seite ist daher NICHT vollstaendig geprueft.",
                bericht.modell_hinweis
            )
        });
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
        // Zeichenweise kuerzen: `Bericht` wird auch von der Platte gelesen,
        // und ein abgeschnittener Hash duerfte den Bericht nicht sprengen.
        let hash_kurz: String = fund.zitat_hash.chars().take(16).collect();
        zeilen.push(format!("- Beleg-Hash: `{hash_kurz}`"));
        zeilen.push(String::new());
    }
    zeilen.join("\n")
}

/// Kurzfassung fuer die Discord-DM. Discord kappt lange Nachrichten, und eine
/// abgeschnittene Liste liest sich wie eine vollstaendige.
pub fn dm_text(bericht: &Bericht, grenze: usize) -> String {
    let kopf = if bericht.funde.is_empty() {
        if bericht.modell_geprueft {
            format!(
                "Coaching-Audit {}: keine Auffaelligkeiten ({} Segmente).",
                bericht.kanal, bericht.segmente
            )
        } else {
            format!(
                "Coaching-Audit {}: keine Regelfunde, aber Modellschritt lief nicht ({}). Nicht vollstaendig geprueft.",
                bericht.kanal, bericht.modell_hinweis
            )
        }
    } else {
        let hoch = bericht.funde.iter().filter(|f| f.schwere == "high").count();
        // Der Hinweis auf den ausgefallenen Modellschritt gehoert auch hierhin:
        // Funde heissen sonst "das war alles", obwohl nur die Regeln liefen.
        let unvollstaendig = if bericht.modell_geprueft {
            String::new()
        } else {
            format!(" Modellschritt lief nicht ({}).", bericht.modell_hinweis)
        };
        format!(
            "Coaching-Audit {}: {} Funde, davon {} hoch ({} Segmente).{}",
            bericht.kanal,
            bericht.funde.len(),
            hoch,
            bericht.segmente,
            unvollstaendig
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
            transkription_lokal: true,
            anbieter: "fireworks".to_owned(),
            llm_modell: "testmodell".to_owned(),
            transkript_behalten: true,
            segmente: 12,
            modell_geprueft: true,
            modell_hinweis: String::new(),
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

    fn bericht_ohne_modell(funde: Vec<Fund>) -> Bericht {
        let mut b = bericht(funde);
        b.modell_geprueft = false;
        b.modell_hinweis = "kein Schluessel".to_owned();
        b
    }

    #[test]
    fn ausgefallener_modellschritt_faellt_im_bericht_auf() {
        // Ohne diesen Hinweis liest sich ein leerer Bericht wie "sauber",
        // obwohl der halbe Pruefpfad nie gelaufen ist.
        let text = markdown(&bericht_ohne_modell(vec![]));
        assert!(text.contains("NICHT GELAUFEN"));
        assert!(text.contains("NICHT vollstaendig geprueft"));
        assert!(!text.contains("Keine Auffaelligkeiten."));
    }

    #[test]
    fn ausgefallener_modellschritt_steht_auch_in_der_dm() {
        let text = dm_text(&bericht_ohne_modell(vec![]), 3);
        assert!(text.contains("Modellschritt lief nicht"));
        assert!(!text.contains("keine Auffaelligkeiten"));
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
    fn entfernte_transkription_wird_als_solche_ausgewiesen() {
        let mut b = bericht(vec![]);
        b.transkription_lokal = false;
        let text = markdown(&b);
        assert!(text.contains("ENTFERNT"));
        assert!(!text.contains("), lokal"));
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

    #[test]
    fn dm_mit_funden_nennt_den_ausgefallenen_modellschritt() {
        // Funde plus Stille ueber den Modellschritt liest sich wie "das war
        // alles", obwohl nur die Regeln geprueft haben.
        let mut b = bericht(vec![fund("high", 12.0, "harassment")]);
        b.modell_geprueft = false;
        b.modell_hinweis = "Modellaufruf fehlgeschlagen".to_owned();
        let text = dm_text(&b, 5);
        assert!(
            text.contains("Modellschritt lief nicht"),
            "unerwartet: {text}"
        );
    }
}

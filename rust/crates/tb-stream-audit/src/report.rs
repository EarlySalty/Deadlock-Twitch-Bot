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

use chrono::{DateTime, Duration, Utc};
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
    /// Twitch-Startzeit der Sendung. Bleibt optional fuer alte Berichte und
    /// fuer Streams, deren Helix-Daten keine brauchbare Zeit enthielten.
    #[serde(default)]
    pub stream_start_utc: Option<String>,
    /// Zeitpunkt, an dem dieser Aufnahmeprozess begann. Dient als ehrliche
    /// Ersatzbasis, wenn der Stream-Start unbekannt war.
    #[serde(default)]
    pub aufnahme_beginn_utc: Option<String>,
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
    /// Ob der zweite LLM-Schritt fuer die kopierfertigen Meldegruende
    /// vollstaendig erfolgreich war.
    #[serde(default)]
    pub meldegrund_aufbereitet: bool,
    /// Hinweis, wenn die Aufbereitung fehlte oder nur teilweise gelang.
    #[serde(default)]
    pub meldegrund_hinweis: String,
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
        format!(
            "- Stream-Beginn UTC: {}",
            bericht.stream_start_utc.as_deref().unwrap_or("unbekannt")
        ),
        format!(
            "- Aufnahmebeginn UTC: {}",
            bericht
                .aufnahme_beginn_utc
                .as_deref()
                .unwrap_or("unbekannt")
        ),
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
                format!(": NICHT GELAUFEN: {}", bericht.modell_hinweis)
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
            "### {} bis {}: {} ({})",
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
        if !fund.twitch_meldegrund.trim().is_empty() {
            zeilen.push(format!("- Twitch-Meldegrund: {}", fund.twitch_meldegrund));
        }
        // Zeichenweise kuerzen: `Bericht` wird auch von der Platte gelesen,
        // und ein abgeschnittener Hash duerfte den Bericht nicht sprengen.
        let hash_kurz: String = fund.zitat_hash.chars().take(16).collect();
        zeilen.push(format!("- Beleg-Hash: `{hash_kurz}`"));
        zeilen.push(String::new());
    }
    zeilen.join("\n")
}

/// Laenge des Zitats in einer DM-Zeile. Genug, um den Wortlaut samt Umgebung zu
/// sehen, ohne dass ein einzelner Fund die ganze Nachricht fuellt.
const DM_ZITAT_MAX: usize = 140;

/// Datum aus einem ISO-Zeitstempel fuer den DM-Kopf. Ohne den Stream-Tag ist der
/// Sekunden-Offset je Fund nicht zuzuordnen: der Admin muss wissen, welchen
/// Stream er im VOD aufschlagen soll. Ist der Wert kein ISO-Datum, geht er
/// unveraendert durch, statt still gekuerzt zu werden.
fn tag_aus(erstellt_am: &str) -> &str {
    let stelle = erstellt_am.as_bytes().get(10);
    if erstellt_am.len() >= 10 && matches!(stelle, Some(b'T') | Some(b' ') | None) {
        &erstellt_am[..10]
    } else {
        erstellt_am
    }
}

/// Das Zitat, das in die Admin-DM geht.
///
/// Bevorzugt den unredigierten Wortlaut: der Admin soll den echten Ausdruck
/// sehen, um einen Twitch-Verstoss beurteilen und melden zu koennen. Faellt das
/// Rohzitat weg - etwa weil der Bericht von der Platte nachgeladen wurde, wo es
/// nie stand - tritt die geschwaerzte Fassung an seine Stelle, damit die Zeile
/// nicht leer bleibt.
fn zitat_fuer_dm(fund: &Fund) -> String {
    let quelle = if fund.zitat_roh.trim().is_empty() {
        &fund.zitat_redigiert
    } else {
        &fund.zitat_roh
    };
    let quelle = quelle.split_whitespace().collect::<Vec<_>>().join(" ");
    if quelle.chars().count() <= DM_ZITAT_MAX {
        return format!("„{quelle}“");
    }
    let gekuerzt: String = quelle.chars().take(DM_ZITAT_MAX).collect();
    format!("„{gekuerzt}…“")
}

fn basiszeit(bericht: &Bericht) -> Option<(DateTime<Utc>, bool)> {
    bericht
        .stream_start_utc
        .as_deref()
        .and_then(|wert| DateTime::parse_from_rfc3339(wert).ok())
        .map(|wert| (wert.with_timezone(&Utc), true))
        .or_else(|| {
            bericht
                .aufnahme_beginn_utc
                .as_deref()
                .and_then(|wert| DateTime::parse_from_rfc3339(wert).ok())
                .map(|wert| (wert.with_timezone(&Utc), false))
        })
}

fn fundzeitpunkt(bericht: &Bericht, fund: &Fund) -> Option<(DateTime<Utc>, bool)> {
    let (basis, ist_streambeginn) = basiszeit(bericht)?;
    let millisekunden = (fund.start_sekunden.max(0.0) * 1000.0).round() as i64;
    Some((
        basis + Duration::milliseconds(millisekunden),
        ist_streambeginn,
    ))
}

fn stream_zeitfenster(bericht: &Bericht, fund: &Fund) -> String {
    let fenster = format!(
        "ca. {} bis {}",
        zeit(fund.start_sekunden),
        zeit(fund.ende_sekunden)
    );
    match basiszeit(bericht) {
        Some((_, true)) => fenster,
        Some((_, false)) => format!("{fenster} (ab Aufnahmebeginn, Stream-Start unbekannt)"),
        None => format!("{fenster} (Stream-Start unbekannt)"),
    }
}

/// Sachlicher Rückfall, falls der zweite Modellschritt nicht antwortet. Der
/// Text bleibt bewusst vorsichtig und behauptet keine Einzelheiten, die nur
/// aus dem unredigierten Wortlaut stammen könnten.
pub fn fallback_meldegrund(fund: &Fund) -> String {
    match fund.kategorie.as_str() {
        "hate_speech_slur" => {
            "Der Streamer verwendete laut Transkript eine mögliche rassistische oder diskriminierende Beleidigung".to_owned()
        }
        "discriminatory_speech" => {
            "Der Streamer äußerte laut Transkript eine diskriminierende Aussage über eine geschützte Gruppe".to_owned()
        }
        "threat_or_self_harm" => {
            "Der Streamer äußerte laut Transkript eine mögliche Drohung oder Aufforderung zur Selbstverletzung".to_owned()
        }
        "harassment" => {
            "Der Streamer beleidigte laut Transkript eine andere Person in deutlicher Weise".to_owned()
        }
        "sexual_content" => {
            "Der Streamer verwendete laut Transkript eine deutlich sexuelle Äußerung gegen eine andere Person".to_owned()
        }
        _ => "Im Transkript wurde eine möglicherweise meldewürdige Äußerung erkannt".to_owned(),
    }
}

fn meldegrund(fund: &Fund) -> String {
    let text = if fund.twitch_meldegrund.trim().is_empty() {
        fallback_meldegrund(fund)
    } else {
        fund.twitch_meldegrund.clone()
    };
    let text = crate::rules::redact_text(&text);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    text.trim_end_matches(['.', '!', '?']).to_owned()
}

fn kopierfertiger_meldegrund(bericht: &Bericht, fund: &Fund) -> String {
    let grund = meldegrund(fund);
    let fenster = stream_zeitfenster(bericht, fund);
    match fundzeitpunkt(bericht, fund) {
        Some((zeitpunkt, true)) => format!(
            "{grund}; bitte prüft am {} um {} UTC das VOD im Zeitfenster {fenster}.",
            zeitpunkt.format("%d.%m.%Y"),
            zeitpunkt.format("%H:%M:%S")
        ),
        Some((zeitpunkt, false)) => format!(
            "{grund}; bitte prüft am {} um {} UTC ab Aufnahmebeginn das VOD im Zeitfenster {fenster}.",
            zeitpunkt.format("%d.%m.%Y"),
            zeitpunkt.format("%H:%M:%S")
        ),
        None => format!(
            "{grund}; bitte prüft das VOD im ungefähren Stream-Zeitfenster {fenster}."
        ),
    }
}

/// Kurzfassung fuer die Discord-DM. Discord kappt lange Nachrichten, und eine
/// abgeschnittene Liste liest sich wie eine vollstaendige.
pub fn dm_text(bericht: &Bericht, grenze: usize) -> String {
    let kopf = if bericht.funde.is_empty() {
        // Nur fuer den Bericht auf der Platte: ohne Funde verschickt der
        // Dienst keine DM, ein ausgefallener Modellschritt meldet sich
        // gedrosselt an anderer Stelle.
        format!(
            "Coaching-Audit {}: keine Auffaelligkeiten ({} Segmente).",
            bericht.kanal, bericht.segmente
        )
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
            "Coaching-Audit {} ({}, {}): {} Funde, davon {} hoch ({} Segmente).{}",
            bericht.kanal,
            bericht.quelle,
            tag_aus(&bericht.erstellt_am),
            bericht.funde.len(),
            hoch,
            bericht.segmente,
            unvollstaendig
        )
    };

    let mut text = kopf;
    if !bericht.meldegrund_aufbereitet && !bericht.meldegrund_hinweis.trim().is_empty() {
        let status = if bericht
            .meldegrund_hinweis
            .trim_start()
            .starts_with("unvollständig")
        {
            "unvollständig"
        } else {
            "nicht verfügbar"
        };
        text.push_str(&format!(
            "\n\nLLM-Aufbereitung {status}: {}",
            bericht.meldegrund_hinweis
        ));
    }
    for fund in bericht.funde.iter().take(grenze) {
        text.push_str(&format!(
            "\n\n• Fundstelle {} bis {} ({}, {})\nGesagt (Transkript): {}\nGesagt am: {}\nStream-Zeitfenster: {}\nKopierfertiger Twitch-Meldegrund:\n{}",
            zeit(fund.start_sekunden),
            zeit(fund.ende_sekunden),
            fund.kategorie,
            fund.schwere,
            zitat_fuer_dm(fund),
            fundzeitpunkt(bericht, fund)
                .map(|(zeitpunkt, _)| zeitpunkt.format("%d.%m.%Y %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "unbekannt (Stream-Start nicht verfügbar)".to_owned()),
            stream_zeitfenster(bericht, fund),
            kopierfertiger_meldegrund(bericht, fund)
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
            zitat_roh: "so ein nigga echt".to_owned(),
            twitch_meldegrund:
                "Der Streamer verwendete eine rassistische Beleidigung gegen eine andere Person"
                    .to_owned(),
            zitat_hash: "a".repeat(64),
        }
    }

    fn bericht(funde: Vec<Fund>) -> Bericht {
        Bericht {
            lauf_id: "20260813T200000Z-testkanal".to_owned(),
            erstellt_am: "2026-08-13T20:00:00Z".to_owned(),
            stream_start_utc: Some("2026-08-13T20:00:00Z".to_owned()),
            aufnahme_beginn_utc: Some("2026-08-13T20:00:00Z".to_owned()),
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
            meldegrund_aufbereitet: true,
            meldegrund_hinweis: String::new(),
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
    fn dm_zeigt_klartext_und_faellt_auf_geschwaerzt_zurueck() {
        // Der Admin soll den echten Wortlaut sehen, um zu melden.
        let text = dm_text(&bericht(vec![fund("high", 12.0, "hate_speech_slur")]), 5);
        assert!(text.contains("nigga"), "Klartext fehlt: {text}");
        // Ohne Rohzitat - etwa aus der Datei nachgeladen - tritt die
        // geschwaerzte Fassung ein, statt die Zeile leer zu lassen.
        let mut f = fund("high", 12.0, "hate_speech_slur");
        f.zitat_roh = String::new();
        let text = dm_text(&bericht(vec![f]), 5);
        assert!(text.contains("[REDACTED]"), "Fallback fehlt: {text}");
        assert!(
            !text.contains("nigga"),
            "Rohzitat trotz leerem Feld: {text}"
        );
    }

    #[test]
    fn dm_kopf_traegt_stream_tag_und_quelle() {
        // Ein Sekunden-Offset ohne Stream-Tag ist im VOD nicht zu finden.
        let text = dm_text(&bericht(vec![fund("high", 12.0, "harassment")]), 5);
        assert!(text.contains("2026-08-13"), "Datum fehlt: {text}");
        assert!(text.contains("live"), "Quelle fehlt: {text}");
        assert!(text.contains("00:00:12"), "Fund-Offset fehlt: {text}");
    }

    #[test]
    fn dm_zeigt_originalwortlaut_zeitfenster_und_kopiergrund() {
        let text = dm_text(&bericht(vec![fund("high", 12.0, "hate_speech_slur")]), 5);
        assert!(
            text.contains("Gesagt (Transkript):"),
            "Zitatlabel fehlt: {text}"
        );
        assert!(text.contains("nigga"), "Originalwortlaut fehlt: {text}");
        assert!(
            text.contains("Gesagt am: 13.08.2026 20:00:12 UTC"),
            "absolute Uhrzeit fehlt: {text}"
        );
        assert!(
            text.contains("Stream-Zeitfenster: ca. 00:00:12 bis 00:00:42"),
            "Zeitfenster fehlt: {text}"
        );
        assert!(
            text.contains("Kopierfertiger Twitch-Meldegrund:"),
            "Meldegrundlabel fehlt: {text}"
        );
        assert!(
            text.contains("bitte prüft am 13.08.2026 um 20:00:12 UTC"),
            "Datum und Uhrzeit fehlen im Kopiergrund: {text}"
        );
    }

    #[test]
    fn zwei_copy_paste_funde_bleiben_ungekuerzt() {
        let text = dm_text(
            &bericht(vec![
                fund("high", 12.0, "hate_speech_slur"),
                fund("high", 72.0, "discriminatory_speech"),
            ]),
            2,
        );
        let anfrage = crate::melden::anfrage(crate::melden::STANDARD_EMPFAENGER, &text);
        assert_eq!(anfrage.content, text);
        assert!(anfrage
            .content
            .contains("Kopierfertiger Twitch-Meldegrund:"));
    }

    #[test]
    fn dm_markiert_fallback_ohne_llm_aufbereitung() {
        let mut b = bericht(vec![fund("high", 12.0, "hate_speech_slur")]);
        b.meldegrund_aufbereitet = false;
        b.meldegrund_hinweis = "Modellantwort fehlt".to_owned();
        let text = dm_text(&b, 5);
        assert!(
            text.contains("LLM-Aufbereitung nicht verfügbar: Modellantwort fehlt"),
            "Fallback-Hinweis fehlt: {text}"
        );
        assert!(
            text.contains("Kopierfertiger Twitch-Meldegrund:"),
            "Fallback-Grund fehlt: {text}"
        );
    }

    #[test]
    fn tag_aus_nimmt_datum_oder_laesst_durch() {
        assert_eq!(tag_aus("2026-08-13T20:00:00Z"), "2026-08-13");
        assert_eq!(tag_aus("2026-08-13 20:00:00"), "2026-08-13");
        assert_eq!(tag_aus("2026-08-13"), "2026-08-13");
        assert_eq!(tag_aus("unbekannt"), "unbekannt");
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

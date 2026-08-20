//! Modellgestuetzte Funde ergaenzend zu den Regeln.
//!
//! Die Regeln finden nur, was als Muster formulierbar ist. Belaestigung,
//! sexuelle Inhalte oder diskriminierende Aussagen ohne Reizwort brauchen ein
//! Modell.
//!
//! Der Anbieter kommt aus `tb_llm::selection::endpoint_for` unter dem
//! Anwendungsfall `stream_audit`, nicht fest verdrahtet. Das ist genau ein
//! Anbieter, keine Kette: faellt er aus, bleibt es bei den Regelfunden, und
//! der Bericht sagt es. Damit folgt das Audit
//! derselben Konfiguration wie der Rest des Bots (`TB_LLM_PROVIDER_*`), und ein
//! Anbieterwechsel passiert an einer Stelle statt in jedem Aufrufer.
//!
//! # Was an das Modell geht
//!
//! Segmenttexte, durch die Schwaerzung geschickt, mit laufender Nummer statt
//! Segment-ID. Kein Kanalname, keine Stream-ID, keine Zuschauer, keine Steam-
//! oder Discord-IDs: die eigentliche Segment-ID enthaelt Kanal und Sendung
//! und bliebe sonst am Text kleben.
//!
//! Was die Schwaerzung **nicht** leistet: sie kennt die drei Muster aus
//! [`crate::rules`] und sonst nichts. Alles andere - Namen, Beleidigungen ohne
//! Reizwort, private Erzaehlungen - geht im Wortlaut mit. Das ist der Preis
//! dafuer, dass ein Modell ueberhaupt etwas bewerten kann, und es ist der
//! Grund, warum ein fremder Anbieter ausdruecklich erlaubt werden muss. Ohne
//! Erlaubnis bleibt es bei den Regelfunden, und der Bericht sagt das (siehe
//! `report::Bericht`). Audio verlaesst den Rechner nie.
//!
//! Die Erlaubnis steuert [`fernes_modell_erlaubt`]: ein Anbieter auf localhost
//! gilt immer als erlaubt, alles andere nur mit
//! `STREAM_AUDIT_ALLOW_REMOTE_LLM=1`.

use serde::Deserialize;

use crate::{Fund, Segment};

/// Anwendungsfall-Schluessel fuer die Anbieterwahl.
pub const USE_CASE: &str = "stream_audit";

/// So viele Segmente gehen gemeinsam in eine Anfrage. Gross genug, dass Kontext
/// ueber Segmentgrenzen erhalten bleibt, klein genug fuer eine stabile Antwort.
pub const SEGMENTE_JE_ANFRAGE: usize = 20;

pub const SYSTEM_PROMPT: &str = concat!(
    "Du pruefst autorisierte Stream-Transkripte fuer privates Coaching. ",
    "Finde nur wahrscheinliche Twitch-Sicherheitsrisiken: hate_speech_slur, ",
    "harassment, threat_or_self_harm, sexual_content oder discriminatory_speech. ",
    "Nicht automatisch flaggen: Zitate zur Kritik, Songtexte, Diskussionen ueber ",
    "Moderation oder unsichere Transkriptionsfehler. Antworte ausschliesslich als ",
    "JSON: {\"findings\":[{\"segment_id\":\"s00001\",\"category\":\"harassment\",",
    "\"severity\":\"low|medium|high\",\"confidence\":\"low|medium|high\",",
    "\"reason\":\"kurze sachliche Begruendung\"}]}. ",
    "Erfinde keine IDs und zitiere keine problematischen Begriffe."
);

/// Zweiter Modellschritt: Aus einem bereits bewerteten Fund wird ein einzelner
/// sachlicher Satz fuer das Twitch-Meldeformular. Die Zeitangaben setzt der
/// Rust-Code ein, damit das Modell weder Zeiten erfindet noch aus dem Kontext
/// falsch uebernimmt.
pub const MELDEGRUND_SYSTEM_PROMPT: &str = concat!(
    "Formuliere fuer jeden Fund genau einen kurzen, sachlichen deutschen Satz ",
    "als Grund fuer eine Twitch-Meldung. Nutze nur die gelieferten, bereits ",
    "geschwärzten Angaben. Behaupte keine neuen Tatsachen, nenne keine Namen ",
    "und wiederhole keine Schimpfwörter oder Schimpfwort-Platzhalter. Keine ",
    "Zeitangaben, keine Aufforderung zu einer Maßnahme, kein Markdown und keine ",
    "Anrede. Antworte ausschliesslich als JSON: ",
    "{\"reasons\":[{\"id\":\"f00001\",\"text\":\"ein sachlicher Satz\"}]}. ",
    "Liefere genau eine Antwort fuer jede ID und erfinde keine IDs."
);

#[derive(Debug, Deserialize)]
pub struct ModellFund {
    pub segment_id: String,
    pub category: String,
    pub severity: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub reason: String,
}

/// `findings` ist Pflicht. Frueher war das Feld optional: eine Antwort wie
/// `{}` galt dann als vollstaendige Pruefung ohne Funde, und Stille war nicht
/// von Sauberkeit zu unterscheiden.
#[derive(Debug, Deserialize)]
pub struct ModellAntwort {
    pub findings: Vec<ModellFund>,
}

#[derive(Debug, Deserialize)]
pub struct MeldegrundAntwort {
    pub reasons: Vec<Meldegrund>,
}

#[derive(Debug, Deserialize)]
pub struct Meldegrund {
    pub id: String,
    pub text: String,
}

/// Segmente in Anfragegroessen schneiden.
pub fn stapel(segmente: &[Segment]) -> Vec<&[Segment]> {
    segmente.chunks(SEGMENTE_JE_ANFRAGE).collect()
}

/// Umgebungsschalter fuer einen Anbieter ausserhalb dieses Rechners.
pub const REMOTE_ERLAUBT_ENV: &str = "STREAM_AUDIT_ALLOW_REMOTE_LLM";

/// Darf dieser Anbieter Transkriptausschnitte sehen?
///
/// Localhost immer. Alles andere nur mit ausdruecklicher Erlaubnis: es geht um
/// die Sprache fremder Menschen, und die verlaesst den Rechner nicht, weil
/// zufaellig ein API-Schluessel in der Umgebung liegt.
pub fn fernes_modell_erlaubt(base_url: &str) -> bool {
    if ist_lokal(base_url) {
        return true;
    }
    matches!(
        std::env::var(REMOTE_ERLAUBT_ENV).unwrap_or_default().trim(),
        "1" | "true" | "ja" | "yes"
    )
}

/// Zeigt die URL auf diesen Rechner?
///
/// Der Host wird herausgeschnitten und **vollstaendig** verglichen. Ein
/// Teilstring-Test wuerde `https://localhost.angreifer.example/` als lokal
/// durchwinken und damit Transkript und API-Schluessel dorthin schicken.
pub fn ist_lokal(base_url: &str) -> bool {
    let Some(host) = host_aus_url(base_url) else {
        return false;
    };
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

/// Host-Anteil einer URL, ohne Schema, Benutzerinfo und Port.
fn host_aus_url(base_url: &str) -> Option<String> {
    let ohne_schema = base_url.split("://").nth(1)?;
    // Der Rueckwaertsschraegstrich beendet die Autoritaet genauso wie der
    // normale: reqwest liest `http://evil.example\\@localhost/` als Host
    // `evil.example`. Wer ihn hier nicht trennt, haelt genau diese URL fuer
    // localhost und laesst Audio nach draussen.
    let autoritaet = ohne_schema
        .split(['/', '\\', '?', '#'])
        .next()
        .filter(|s| !s.is_empty())?;
    // Benutzerinfo abschneiden: `user@host` und der Trick `localhost@evil`.
    let host_port = autoritaet.rsplit('@').next()?;
    // IPv6 in Klammern behaelt seine Klammern, sonst am Doppelpunkt kuerzen.
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().map(|h| format!("[{h}]"))?
    } else {
        host_port.split(':').next()?.to_owned()
    };
    Some(host.to_lowercase())
}

/// Laufende Nummer eines Segments innerhalb der Anfrage.
///
/// Die echte Segment-ID traegt Kanal und Stream-ID. Sie mitzuschicken hiesse,
/// dem Anbieter zu sagen, wen wir gerade pruefen - und genau das soll er
/// nicht wissen.
pub fn anfrage_id(stelle: usize) -> String {
    format!("s{:05}", stelle + 1)
}

/// Lokale, nicht rueckverfolgbare ID fuer die zweite LLM-Anfrage.
pub fn meldegrund_id(stelle: usize) -> String {
    format!("f{:05}", stelle + 1)
}

/// Die Nutzlast einer Anfrage: laufende Nummer, Zeitfenster und
/// **geschwaerzter** Text.
///
/// Die Schwaerzung laeuft hier und nicht beim Aufrufer, damit kein zweiter
/// Aufrufer sie vergessen kann.
pub fn anfrage_json(stapel: &[Segment]) -> String {
    let eintraege: Vec<_> = stapel
        .iter()
        .enumerate()
        .map(|(i, s)| {
            serde_json::json!({
                "id": anfrage_id(i),
                "start_seconds": (s.start_sekunden * 100.0).round() / 100.0,
                "end_seconds": (s.ende_sekunden * 100.0).round() / 100.0,
                "text": crate::rules::redact_text(&s.text),
            })
        })
        .collect();
    serde_json::json!({ "segments": eintraege }).to_string()
}

/// Nur geschwaerzte Funddaten an den zweiten Modellschritt senden. Das
/// Rohzitat bleibt in der Anwendung und erreicht den entfernten Anbieter nie.
pub fn meldegrund_anfrage_json(funde: &[Fund]) -> String {
    let eintraege: Vec<_> = funde
        .iter()
        .enumerate()
        .map(|(i, fund)| {
            serde_json::json!({
                "id": meldegrund_id(i),
                "category": fund.kategorie,
                "severity": fund.schwere,
                "detector": fund.erkenner,
                "reason": crate::rules::redact_text(&fund.begruendung),
                "quote": crate::rules::redact_text(&fund.zitat_redigiert),
            })
        })
        .collect();
    serde_json::json!({ "findings": eintraege }).to_string()
}

/// Erstes JSON-Objekt aus einer Modellantwort schneiden. Modelle rahmen die
/// Antwort gern mit Fliesstext oder Code-Zaun ein, obwohl der Prompt es
/// verbietet; daran soll der Lauf nicht scheitern.
pub fn json_objekt_ausschneiden(roh: &str) -> Option<&str> {
    let start = roh.find('{')?;
    let mut tiefe = 0usize;
    let mut in_text = false;
    let mut maskiert = false;
    for (i, zeichen) in roh[start..].char_indices() {
        if in_text {
            match zeichen {
                _ if maskiert => maskiert = false,
                '\\' => maskiert = true,
                '"' => in_text = false,
                _ => {}
            }
            continue;
        }
        match zeichen {
            '"' => in_text = true,
            '{' => tiefe += 1,
            '}' => {
                tiefe -= 1;
                if tiefe == 0 {
                    return Some(&roh[start..start + i + zeichen.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Modellfunde auf die gemeinsame [`Fund`]-Form bringen.
///
/// Funde zu unbekannten Segment-IDs fallen weg: das Modell soll keine IDs
/// erfinden, und ein Fund ohne Zeitbezug ist im Bericht wertlos.
pub fn zu_funden(antwort: &ModellAntwort, segmente: &[Segment]) -> Vec<Fund> {
    zu_funden_gezaehlt(antwort, segmente).0
}

/// Wie [`zu_funden`], gibt zusaetzlich die Zahl der verworfenen Treffer
/// zurueck. Ein Modell, das IDs erfindet, hat nicht sauber gearbeitet; das
/// gehoert in den Bericht und nicht ins Nichts.
pub fn zu_funden_gezaehlt(antwort: &ModellAntwort, segmente: &[Segment]) -> (Vec<Fund>, usize) {
    let mut verworfen = 0usize;
    let mut funde = Vec::new();
    for treffer in &antwort.findings {
        let Some(segment) = stelle_aus_id(&treffer.segment_id).and_then(|i| segmente.get(i)) else {
            verworfen += 1;
            continue;
        };
        funde.push(Fund {
            segment_id: segment.id.clone(),
            start_sekunden: segment.start_sekunden,
            ende_sekunden: segment.ende_sekunden,
            kategorie: kategorie_normal(&treffer.category),
            schwere: stufe_normal(&treffer.severity, "medium"),
            erkenner: "modell".to_owned(),
            sicherheit: stufe_normal(&treffer.confidence, "medium"),
            // Die Begruendung kommt vom Modell und zitiert gern das, was sie
            // beanstandet. Ohne Schwaerzung stuende genau der Wortlaut im
            // Bericht, den der Beleg daneben verdeckt.
            begruendung: crate::rules::redact_text(&treffer.reason),
            zitat_redigiert: crate::rules::redact_text(&segment.text),
            // Nur fuer die fluechtige Admin-DM, nie in die Akte: siehe Fund.
            zitat_roh: segment.text.clone(),
            twitch_meldegrund: String::new(),
            zitat_hash: crate::rules::evidence_hash(&segment.text),
        });
    }
    (funde, verworfen)
}

/// Stelle im Stapel aus der laufenden Nummer, die wir vergeben haben.
fn stelle_aus_id(id: &str) -> Option<usize> {
    let nummer: usize = id.trim().trim_start_matches('s').parse().ok()?;
    nummer.checked_sub(1)
}

/// Bekannte Kategorien; alles andere wird als `sonstiges` gefuehrt.
///
/// Ohne Normalisierung stuende im Bericht, was das Modell gerade schreibt -
/// `HIGH`, `Harassment`, `critical` - und die Sortierung nach Schwere sowie
/// die Zaehlung hoher Funde in der DM gingen daran vorbei.
fn kategorie_normal(roh: &str) -> String {
    let k = roh.trim().to_lowercase();
    match k.as_str() {
        "hate_speech_slur"
        | "harassment"
        | "threat_or_self_harm"
        | "sexual_content"
        | "discriminatory_speech" => k,
        "" => "sonstiges".to_owned(),
        _ => format!("sonstiges:{k}"),
    }
}

/// `low|medium|high` aus einer Modellangabe; Unbekanntes faellt auf den
/// Standard zurueck. `critical` gilt als `high`, nicht als niedrig.
fn stufe_normal(roh: &str, standard: &str) -> String {
    match roh.trim().to_lowercase().as_str() {
        "low" | "niedrig" => "low".to_owned(),
        "medium" | "mittel" => "medium".to_owned(),
        "high" | "hoch" | "critical" | "kritisch" | "severe" => "high".to_owned(),
        _ => standard.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(id: &str, text: &str) -> Segment {
        Segment {
            id: id.to_owned(),
            start_sekunden: 10.0,
            ende_sekunden: 40.0,
            text: text.to_owned(),
        }
    }

    #[test]
    fn stapel_teilt_nach_groesse() {
        let segmente: Vec<_> = (0..45).map(|i| segment(&format!("s{i}"), "text")).collect();
        let teile = stapel(&segmente);
        assert_eq!(teile.len(), 3);
        assert_eq!(teile[0].len(), SEGMENTE_JE_ANFRAGE);
        assert_eq!(teile[2].len(), 5);
    }

    #[test]
    fn anfrage_enthaelt_nur_id_zeit_und_text() {
        let json: serde_json::Value =
            serde_json::from_str(&anfrage_json(&[segment("s1", "hallo")])).unwrap();
        let eintrag = &json["segments"][0];
        let felder: Vec<_> = eintrag.as_object().unwrap().keys().cloned().collect();
        assert_eq!(felder, vec!["end_seconds", "id", "start_seconds", "text"]);
    }

    #[test]
    fn anfrage_enthaelt_keinen_rohwortlaut() {
        // Der eigentliche Datenschutz-Vertrag dieses Moduls.
        let json = anfrage_json(&[segment("s1", "du schwuchtel jetzt reicht es")]);
        assert!(!json.to_lowercase().contains("schwuchtel"));
        assert!(json.contains("[REDACTED]"));
    }

    #[test]
    fn localhost_gilt_immer_als_erlaubt() {
        assert!(fernes_modell_erlaubt("http://127.0.0.1:8791/v1"));
        assert!(fernes_modell_erlaubt("http://localhost:1234/v1"));
    }

    #[test]
    fn aehnlich_aussehende_hosts_gelten_nicht_als_lokal() {
        // Genau die Faelle, die ein Teilstring-Test durchwinken wuerde.
        for url in [
            "https://localhost.angreifer.example/v1",
            "https://127.0.0.1.evil.example/v1",
            "https://evil.example/?x=//127.0.0.1",
            "https://localhost@evil.example/v1",
            "https://not-localhost/v1",
        ] {
            assert!(!ist_lokal(url), "faelschlich lokal: {url}");
        }
    }

    #[test]
    fn echte_lokale_adressen_werden_erkannt() {
        for url in [
            "http://127.0.0.1:8791/v1/audio/transcriptions",
            "http://localhost:1234/v1",
            "http://[::1]:8080/v1",
            "HTTP://LOCALHOST:9/v1",
        ] {
            assert!(ist_lokal(url), "nicht als lokal erkannt: {url}");
        }
    }

    #[test]
    fn fremder_anbieter_braucht_ausdrueckliche_erlaubnis() {
        std::env::remove_var(REMOTE_ERLAUBT_ENV);
        assert!(!fernes_modell_erlaubt(
            "https://api.fireworks.ai/inference/v1"
        ));
        std::env::set_var(REMOTE_ERLAUBT_ENV, "1");
        assert!(fernes_modell_erlaubt(
            "https://api.fireworks.ai/inference/v1"
        ));
        std::env::remove_var(REMOTE_ERLAUBT_ENV);
    }

    #[test]
    fn json_wird_aus_umgebendem_text_geschnitten() {
        let roh = "Klar, hier:\n```json\n{\"findings\":[]}\n```\nViel Erfolg!";
        assert_eq!(json_objekt_ausschneiden(roh), Some("{\"findings\":[]}"));
    }

    #[test]
    fn geschweifte_klammer_im_text_verwirrt_den_schnitt_nicht() {
        let roh = r#"{"findings":[{"reason":"sagte } wortwoertlich"}]}"#;
        assert_eq!(json_objekt_ausschneiden(roh), Some(roh));
    }

    #[test]
    fn unbekannte_segment_id_wird_verworfen() {
        let antwort: ModellAntwort = serde_json::from_str(
            r#"{"findings":[{"segment_id":"gibtsnicht","category":"harassment","severity":"low"}]}"#,
        )
        .unwrap();
        assert!(zu_funden(&antwort, &[segment("s1", "text")]).is_empty());
    }

    #[test]
    fn modellfund_wird_uebernommen_und_zitat_geschwaerzt() {
        let antwort: ModellAntwort = serde_json::from_str(
            r#"{"findings":[{"segment_id":"s1","category":"harassment","severity":"medium","confidence":"high","reason":"beleidigt Zuschauer"}]}"#,
        )
        .unwrap();
        let funde = zu_funden(&antwort, &[segment("s1", "du schwuchtel jetzt reicht es")]);
        assert_eq!(funde.len(), 1);
        assert_eq!(funde[0].erkenner, "modell");
        assert_eq!(funde[0].start_sekunden, 10.0);
        assert!(funde[0].zitat_redigiert.contains("[REDACTED]"));
    }

    #[test]
    fn fehlende_sicherheit_faellt_auf_mittel_zurueck() {
        let antwort: ModellAntwort = serde_json::from_str(
            r#"{"findings":[{"segment_id":"s1","category":"harassment","severity":"low"}]}"#,
        )
        .unwrap();
        let funde = zu_funden(&antwort, &[segment("s1", "text")]);
        assert_eq!(funde[0].sicherheit, "medium");
    }
}

#[cfg(test)]
mod tests_anonym {
    use super::*;

    fn segment(id: &str, text: &str) -> Segment {
        Segment {
            id: id.to_owned(),
            start_sekunden: 0.0,
            ende_sekunden: 10.0,
            text: text.to_owned(),
        }
    }

    #[test]
    fn anfrage_nennt_weder_kanal_noch_sendung() {
        let stapel = [segment(
            "helmbombenricky-4711-t000600-s00001",
            "harmloser Satz",
        )];
        let json = anfrage_json(&stapel);
        assert!(
            !json.contains("helmbombenricky") && !json.contains("4711"),
            "Kanal oder Sendung stehen in der Anfrage: {json}"
        );
        assert!(json.contains("\"id\":\"s00001\""), "{json}");
    }

    #[test]
    fn erfundene_ids_werden_gezaehlt_statt_verschluckt() {
        let segmente = [segment("kanal-lauf-t000000-s00001", "text")];
        let antwort = ModellAntwort {
            findings: vec![
                ModellFund {
                    segment_id: "s00001".to_owned(),
                    category: "harassment".to_owned(),
                    severity: "high".to_owned(),
                    confidence: String::new(),
                    reason: "grund".to_owned(),
                },
                ModellFund {
                    segment_id: "s09999".to_owned(),
                    category: "harassment".to_owned(),
                    severity: "high".to_owned(),
                    confidence: String::new(),
                    reason: "erfunden".to_owned(),
                },
            ],
        };
        let (funde, verworfen) = zu_funden_gezaehlt(&antwort, &segmente);
        assert_eq!(funde.len(), 1);
        assert_eq!(verworfen, 1);
        assert_eq!(
            funde[0].segment_id, "kanal-lauf-t000000-s00001",
            "im Bericht steht wieder die echte ID"
        );
    }

    #[test]
    fn antwort_ohne_findings_ist_ein_fehler() {
        // `{}` galt frueher als vollstaendige Pruefung ohne Funde.
        assert!(serde_json::from_str::<ModellAntwort>("{}").is_err());
        assert!(serde_json::from_str::<ModellAntwort>(r#"{"findings":[]}"#).is_ok());
    }

    #[test]
    fn schwere_wird_normalisiert() {
        assert_eq!(stufe_normal("HIGH", "medium"), "high");
        assert_eq!(stufe_normal("critical", "medium"), "high");
        assert_eq!(stufe_normal("", "medium"), "medium");
        assert_eq!(stufe_normal("voellig unbekannt", "medium"), "medium");
        assert_eq!(kategorie_normal("Harassment"), "harassment");
        assert_eq!(kategorie_normal("spam"), "sonstiges:spam");
    }
}

#[cfg(test)]
mod tests_host {
    use super::*;

    #[test]
    fn rueckwaertsschraegstrich_taeuscht_kein_localhost_vor() {
        // reqwest verbindet sich hier zu evil.example, nicht zu localhost.
        assert!(!ist_lokal("http://evil.example\\@localhost/v1"));
        assert!(!ist_lokal("http://evil.example\\@127.0.0.1/v1"));
        assert!(ist_lokal("http://127.0.0.1:8791/v1/audio/transcriptions"));
        assert!(ist_lokal("http://localhost:8791/v1"));
        assert!(!ist_lokal("http://user@evil.example/v1"));
    }
}

#[cfg(test)]
mod tests_meldegrund {
    use super::*;

    fn fund() -> Fund {
        Fund {
            segment_id: "kanal-lauf-t000012-s00001".to_owned(),
            start_sekunden: 12.0,
            ende_sekunden: 42.0,
            kategorie: "hate_speech_slur".to_owned(),
            schwere: "high".to_owned(),
            erkenner: "regel".to_owned(),
            sicherheit: "high".to_owned(),
            begruendung: "Mögliche rassistische Beleidigung".to_owned(),
            zitat_redigiert: "der [REDACTED] wurde beleidigt".to_owned(),
            zitat_roh: "der nigga wurde beleidigt".to_owned(),
            twitch_meldegrund: String::new(),
            zitat_hash: "a".repeat(64),
        }
    }

    #[test]
    fn meldegrund_anfrage_verwendet_redigierten_beleg() {
        let json = meldegrund_anfrage_json(&[fund()]);
        assert!(
            json.contains("[REDACTED]"),
            "geschwärzter Beleg fehlt: {json}"
        );
        assert!(!json.contains("nigga"), "Rohzitat gelangt ins LLM: {json}");
        assert!(
            json.contains("f00001"),
            "stabile lokale Fund-ID fehlt: {json}"
        );
    }

    #[test]
    fn meldegrund_antwort_erwartet_reasons() {
        let antwort: MeldegrundAntwort = serde_json::from_str(
            r#"{"reasons":[{"id":"f00001","text":"Eine rassistische Beleidigung wurde verwendet"}]}"#,
        )
        .expect("Meldegrund-Antwort");
        assert_eq!(antwort.reasons.len(), 1);
        assert_eq!(antwort.reasons[0].id, "f00001");
    }
}

//! Konfiguration des Audit-Laeufers.
//!
//! Alles kommt aus der Umgebung, damit die Unit die einzige Stelle bleibt, an
//! der Kanaele und Pfade stehen. Kein Kanal ist fest eingebaut: wer einen
//! Streamer aufnimmt, soll das in der Unit sehen und nicht im Binaerprogramm
//! suchen muessen.

use std::path::PathBuf;

/// Kanaele aus `STREAM_AUDIT_CHANNELS`, komma- oder leerzeichengetrennt.
pub const KANAELE_ENV: &str = "STREAM_AUDIT_CHANNELS";
/// Ablage fuer Berichte und Transkripte.
pub const AUSGABE_ENV: &str = "STREAM_AUDIT_OUTPUT_DIR";
/// Ob das Rohtranskript liegen bleibt. Standard: **nein**.
///
/// Die Coaching-Doku sagt zu, dass vollstaendige Transkripte nicht dauerhaft
/// gespeichert werden. Ein Standard auf "behalten" haette diese Zusage still
/// gebrochen: der Bericht traegt geschwaerzte Zitate und Hashes, das ist der
/// Nachweis. Wer den Wortlaut fuer eine Sitzung braucht, schaltet es
/// bewusst ein.
pub const TRANSKRIPT_BEHALTEN_ENV: &str = "STREAM_AUDIT_KEEP_TRANSCRIPT";

/// Aufbewahrungsdauer der Berichte in Tagen; 0 heisst unbegrenzt.
pub const AUFBEWAHRUNG_ENV: &str = "STREAM_AUDIT_RETENTION_DAYS";
pub const AUFBEWAHRUNG_STANDARD: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Konfiguration {
    pub kanaele: Vec<String>,
    pub ausgabe: PathBuf,
    pub transkript_behalten: bool,
    pub aufbewahrung_tage: u64,
    /// Obergrenze fuer alle aufbewahrten Aufnahmen zusammen, in Bytes.
    ///
    /// Die Frist allein ist keine Grenze: faellt der Modellschritt aus, gilt
    /// jeder Block als unvollstaendig geprueft und seine Aufnahme bleibt
    /// liegen. Bei drei Kanaelen sind das mehrere Gigabyte am Tag, dreissig
    /// Tage lang, auf derselben Platte wie Bot und Datenbank. `0` hebt die
    /// Grenze auf.
    pub behalten_grenze_bytes: u64,
}

/// Trennt an Komma, Semikolon und Leerraum, normalisiert auf Kleinschreibung
/// und entfernt Doppelte unter Beibehaltung der Reihenfolge.
pub fn kanaele_lesen(roh: &str) -> Vec<String> {
    let mut raus: Vec<String> = Vec::new();
    for teil in roh.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let name = teil.trim().trim_start_matches('#').to_lowercase();
        if name.is_empty() {
            continue;
        }
        // Auch eine ganze URL soll gehen, damit niemand von Hand kuerzt.
        // Query und Fragment muessen weg: `...twitch.tv/ricky?foo=1` waere
        // sonst der Login "ricky?foo=1" und die Aufnahme scheiterte still.
        let ohne_anhang = name
            .split(['?', '#'])
            .next()
            .unwrap_or(&name)
            .trim_end_matches('/');
        let name = ohne_anhang
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(ohne_anhang)
            .to_owned();
        if name.is_empty() {
            continue;
        }
        if !raus.contains(&name) {
            raus.push(name);
        }
    }
    raus
}

/// Nur bekannte Ja-Werte gelten als wahr.
///
/// Frueher war jeder unbekannte Wert wahr. Ein Tippfehler wie `flase` hiess
/// damit "Rohtranskripte fremder Menschen behalten" - genau die Richtung, in
/// die ein Vertipper nicht kippen darf.
fn wahrheit(roh: &str, standard: bool) -> bool {
    match roh.trim().to_lowercase().as_str() {
        "" => standard,
        "1" | "true" | "on" | "yes" | "ja" => true,
        "0" | "false" | "off" | "no" | "nein" => false,
        andere => {
            tracing::warn!(wert = andere, "unbekannter Schalterwert, nehme Standard");
            standard
        }
    }
}

/// Aufbewahrung in Tagen. Ein unbrauchbarer Wert faellt auf den Standard
/// zurueck - aber nicht still: sonst weicht die tatsaechliche Aufbewahrung von
/// dem ab, was jemand eingestellt zu haben glaubt.
/// Umgebungsvariable fuer die Obergrenze der aufbewahrten Aufnahmen, in
/// Gigabyte.
pub const BEHALTEN_GRENZE_ENV: &str = "STREAM_AUDIT_MAX_KEEP_GB";

/// Zwanzig Gigabyte reichen fuer rund zehn Tage Dauerausfall bei drei Kanaelen
/// und lassen auf der Platte noch Luft fuer Bot und Datenbank.
const BEHALTEN_GRENZE_STANDARD_GB: u64 = 20;

/// Liest die Obergrenze in Gigabyte und rechnet sie in Bytes um. `0` heisst
/// ausdruecklich "keine Grenze".
fn grenze_lesen(roh: &str) -> u64 {
    let roh = roh.trim();
    let gigabyte = if roh.is_empty() {
        BEHALTEN_GRENZE_STANDARD_GB
    } else {
        match roh.parse::<u64>() {
            Ok(wert) => wert,
            Err(_) => {
                tracing::warn!(
                    wert = roh,
                    standard = BEHALTEN_GRENZE_STANDARD_GB,
                    "unbrauchbarer Wert fuer die Aufnahmegrenze, nehme den Standard"
                );
                BEHALTEN_GRENZE_STANDARD_GB
            }
        }
    };
    gigabyte.saturating_mul(1024 * 1024 * 1024)
}

fn aufbewahrung_lesen(roh: &str) -> u64 {
    let roh = roh.trim();
    if roh.is_empty() {
        return AUFBEWAHRUNG_STANDARD;
    }
    match roh.parse::<u64>() {
        Ok(tage) => tage,
        Err(_) => {
            tracing::warn!(
                wert = roh,
                standard = AUFBEWAHRUNG_STANDARD,
                "unbrauchbarer Wert fuer die Aufbewahrung, nehme den Standard"
            );
            AUFBEWAHRUNG_STANDARD
        }
    }
}

impl Konfiguration {
    pub fn from_env() -> Self {
        Self {
            kanaele: kanaele_lesen(&std::env::var(KANAELE_ENV).unwrap_or_default()),
            ausgabe: std::env::var(AUSGABE_ENV)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("data/stream_coaching_audits")),
            transkript_behalten: wahrheit(
                &std::env::var(TRANSKRIPT_BEHALTEN_ENV).unwrap_or_default(),
                false,
            ),
            aufbewahrung_tage: aufbewahrung_lesen(
                &std::env::var(AUFBEWAHRUNG_ENV).unwrap_or_default(),
            ),
            behalten_grenze_bytes: grenze_lesen(
                &std::env::var(BEHALTEN_GRENZE_ENV).unwrap_or_default(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trennt_an_komma_und_leerzeichen() {
        assert_eq!(
            kanaele_lesen("helmbombenricky, skifahrertv deadlockgermany"),
            vec!["helmbombenricky", "skifahrertv", "deadlockgermany"]
        );
    }

    #[test]
    fn normalisiert_gross_klein_und_doppelte() {
        assert_eq!(
            kanaele_lesen("Ricky, ricky , RICKY"),
            vec!["ricky"],
            "derselbe Kanal darf nicht doppelt aufgenommen werden"
        );
    }

    #[test]
    fn ganze_url_wird_auf_den_login_gekuerzt() {
        assert_eq!(
            kanaele_lesen("https://www.twitch.tv/skifahrertv"),
            vec!["skifahrertv"]
        );
    }

    #[test]
    fn url_mit_anhang_und_schraegstrich_ergibt_den_reinen_login() {
        assert_eq!(
            kanaele_lesen("https://www.twitch.tv/skifahrertv/?referrer=raid"),
            vec!["skifahrertv"],
            "Query und abschliessender Schraegstrich duerfen nicht im Login landen"
        );
        assert_eq!(
            kanaele_lesen("https://www.twitch.tv/ricky#chat"),
            vec!["ricky"]
        );
    }

    #[test]
    fn leere_eingabe_ergibt_keine_kanaele() {
        assert!(kanaele_lesen("   ,  ; ").is_empty());
    }

    #[test]
    fn reihenfolge_bleibt_erhalten() {
        assert_eq!(kanaele_lesen("c,a,b"), vec!["c", "a", "b"]);
    }

    #[test]
    fn transkript_wird_standardmaessig_nicht_behalten() {
        // Die Coaching-Doku sagt zu, dass vollstaendige Transkripte nicht
        // dauerhaft liegen bleiben.
        assert!(!wahrheit("", false));
        assert!(wahrheit("1", false));
        assert!(wahrheit("ja", false));
        assert!(!wahrheit("nein", false));
    }

    #[test]
    fn unbrauchbare_aufbewahrung_faellt_auf_den_standard() {
        assert_eq!(aufbewahrung_lesen(""), AUFBEWAHRUNG_STANDARD);
        assert_eq!(aufbewahrung_lesen("dreissig"), AUFBEWAHRUNG_STANDARD);
        assert_eq!(aufbewahrung_lesen("-5"), AUFBEWAHRUNG_STANDARD);
        assert_eq!(aufbewahrung_lesen("7"), 7);
        assert_eq!(aufbewahrung_lesen(" 0 "), 0, "0 heisst unbegrenzt");
    }
}

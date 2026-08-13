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
        let name = name
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(&name)
            .to_owned();
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
            aufbewahrung_tage: std::env::var(AUFBEWAHRUNG_ENV)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(AUFBEWAHRUNG_STANDARD),
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
}

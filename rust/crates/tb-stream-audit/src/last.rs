//! Last-Gate fuer die Auswertung.
//!
//! Aufnehmen kostet fast nichts, transkribieren viel: die Maschine hat keine
//! GPU, und das lokale Whisper laeuft auf denselben Kernen wie alles andere.
//! Sind mehrere Kanaele gleichzeitig live, kann die Auswertung den Server unter
//! Dauerlast bringen.
//!
//! Deshalb dieser Waechter: **nur** die Auswertung wird zurueckgestellt, wenn die
//! Server-Auslastung eine Weile ueber der Grenze bleibt. Aufgenommen wird
//! weiter, die Bloecke bleiben auf der Platte liegen und werden nachgeholt,
//! sobald die Last faellt. So sieht das Audit spaeter trotzdem die ganze
//! Sendezeit, und der Server kollabiert nicht.
//!
//! Der Waechter ist bewusst frei von I/O: die Messung (CPU/RAM aus `/proc`)
//! liegt im Dienst, hier steht nur die Entscheidung. Das macht sie ohne echten
//! Rechner testbar.

/// Ab dieser Auslastung in Prozent gilt der Server als ueberlastet.
pub const GRENZE_ENV: &str = "STREAM_AUDIT_LOAD_LIMIT";
pub const GRENZE_STANDARD: f32 = 90.0;

/// Erst unter diesem Wert faellt das Gate wieder. Die Luecke zur Grenze ist
/// Absicht: liegt die Freigabe genauso hoch wie die Grenze, flattert das Gate
/// bei jeder Schwankung um die 90 Prozent an und aus.
pub const FREIGABE_ENV: &str = "STREAM_AUDIT_LOAD_RELEASE";
pub const FREIGABE_STANDARD: f32 = 80.0;

/// So lange muss die Auslastung ununterbrochen ueber der Grenze liegen, bevor
/// das Gate greift. Ein einzelner Ausschlag - etwa ein Transkript-Block, der
/// gerade rechnet - soll die Auswertung nicht anhalten.
pub const FENSTER_ENV: &str = "STREAM_AUDIT_LOAD_WINDOW_SECS";
pub const FENSTER_STANDARD: u64 = 240;

/// Obergrenze, wie lange das Gate am Stueck aktiv bleiben darf. RAM ist als
/// Signal tueckisch: haelt ein fremder Dienst den Speicher dauerhaft oben,
/// faellt die Last nicht dadurch, dass die Auswertung stoppt - anders als bei
/// reiner CPU. Ohne Deckel bliebe das Gate dann fuer immer zu, waehrend die
/// Aufbewahrung die aeltesten, noch ungeprueften Aufnahmen loescht. Nach dem
/// Deckel oeffnet das Gate fuer ein garantiertes Katchup-Fenster und holt nach;
/// steigt die Last wirklich, greift es danach gleich wieder. `0` schaltet den
/// Deckel ab.
pub const MAX_HALTE_ENV: &str = "STREAM_AUDIT_LOAD_MAX_HOLD_SECS";
pub const MAX_HALTE_STANDARD: u64 = 1800;

/// Mindestlaenge des Katchup-Fensters nach dem Deckel. Es muss laenger sein als
/// der Takt, in dem die Auswertungsschleife das Gate abfragt (60 s), sonst
/// koennte sie das offene Fenster verschlafen. Waehrend dieser Zeit ist das Gate
/// bewusst lastunabhaengig offen, damit wenigstens ein Schwung ausgewertet wird.
const KATCHUP_MIN_S: u64 = 120;

/// Entscheidet anhand der gemessenen Auslastung, ob die Auswertung gerade
/// zurueckgestellt werden soll.
#[derive(Debug, Clone)]
pub struct Lastwaechter {
    grenze: f32,
    freigabe: f32,
    fenster_s: u64,
    max_halte_s: u64,
    // Zeitpunkt, seit dem die Auslastung ununterbrochen ueber der Grenze liegt.
    // `None`, sobald ein Wert wieder darunter faellt.
    ueber_seit_s: Option<u64>,
    // Zeitpunkt, seit dem das Gate aktiv ist - fuer den Max-Halte-Deckel.
    aktiv_seit_s: Option<u64>,
    // Bis zu diesem Zeitpunkt bleibt das Gate nach dem Deckel offen, egal wie
    // hoch die Last ist. Sonst koennte es sich schneller wieder schliessen, als
    // die Auswertungsschleife hinsieht.
    pause_bis_s: Option<u64>,
    aktiv: bool,
}

impl Lastwaechter {
    pub fn neu(grenze: f32, freigabe: f32, fenster_s: u64, max_halte_s: u64) -> Self {
        Self {
            grenze,
            freigabe,
            fenster_s,
            max_halte_s,
            ueber_seit_s: None,
            aktiv_seit_s: None,
            pause_bis_s: None,
            aktiv: false,
        }
    }

    /// Liest die Grenzwerte aus der Umgebung; fehlt oder taugt ein Wert nicht,
    /// greift der Standard - aber nicht still, damit die tatsaechlichen Grenzen
    /// nicht von dem abweichen, was jemand gesetzt zu haben glaubt.
    pub fn aus_umgebung() -> Self {
        let grenze = prozent_aus_umgebung(GRENZE_ENV, GRENZE_STANDARD);
        let mut freigabe = prozent_aus_umgebung(FREIGABE_ENV, FREIGABE_STANDARD);
        // Eine Freigabe ueber der Grenze wuerde das Gate nie fallen lassen.
        if freigabe > grenze {
            tracing::warn!(
                freigabe,
                grenze,
                "Freigabe liegt ueber der Grenze, setze sie auf die Grenze"
            );
            freigabe = grenze;
        }
        let fenster_s = u64_aus_umgebung(FENSTER_ENV, FENSTER_STANDARD);
        let max_halte_s = u64_aus_umgebung(MAX_HALTE_ENV, MAX_HALTE_STANDARD);
        Self::neu(grenze, freigabe, fenster_s, max_halte_s)
    }

    /// Nimmt eine Messung entgegen (`auslastung` in Prozent, `jetzt_s` eine
    /// monoton steigende Sekundenzahl) und gibt zurueck, ob das Gate jetzt
    /// aktiv ist.
    pub fn beobachten(&mut self, auslastung: f32, jetzt_s: u64) -> bool {
        // Katchup-Fenster nach dem Deckel: bewusst lastunabhaengig offen, damit
        // die Auswertung sicher zum Zug kommt, bevor neu bewertet wird.
        if let Some(bis) = self.pause_bis_s {
            if jetzt_s < bis {
                return false;
            }
            self.pause_bis_s = None;
        }

        if auslastung >= self.grenze {
            let seit = *self.ueber_seit_s.get_or_insert(jetzt_s);
            // Erst wenn die Last das ganze Fenster lang oben blieb, greift das
            // Gate. `saturating_sub` faengt eine ruecklaufende Uhr ab.
            if jetzt_s.saturating_sub(seit) >= self.fenster_s && !self.aktiv {
                self.aktiv = true;
                self.aktiv_seit_s = Some(jetzt_s);
            }
        } else {
            // Jede Entlastung setzt die Uhr zurueck: das Fenster meint
            // *ununterbrochene* Ueberlast.
            self.ueber_seit_s = None;
            // Zwischen Freigabe und Grenze bleibt das Gate, wie es war
            // (Hysterese). Erst klar unter der Freigabe faellt es.
            if auslastung < self.freigabe {
                self.aktiv = false;
                self.aktiv_seit_s = None;
            }
        }

        // Max-Halte-Deckel: auch unter Dauerlast muss irgendwann ausgewertet
        // werden, sonst loescht die Aufbewahrung ungeprüfte Aufnahmen. Der
        // Deckel oeffnet das Gate fuer ein garantiertes Fenster; die Uhr faengt
        // von vorn an, sodass es bei echter Dauerlast danach wieder greift.
        if self.aktiv && self.max_halte_s > 0 {
            if let Some(seit) = self.aktiv_seit_s {
                if jetzt_s.saturating_sub(seit) >= self.max_halte_s {
                    self.aktiv = false;
                    self.aktiv_seit_s = None;
                    self.ueber_seit_s = None;
                    self.pause_bis_s = Some(jetzt_s + self.fenster_s.max(KATCHUP_MIN_S));
                }
            }
        }
        self.aktiv
    }

    /// Gibt das Gate frei und vergisst jeden Zwischenstand. Der Dienst ruft das,
    /// wenn er die Auslastung nicht mehr messen kann: ohne Beleg fuer Ueberlast
    /// die Auswertung anzuhalten waere die falsche Vorgabe (fail-open), und ein
    /// aktives Gate bliebe sonst haengen, solange keine Messung mehr kommt.
    pub fn zuruecksetzen(&mut self) {
        self.aktiv = false;
        self.ueber_seit_s = None;
        self.aktiv_seit_s = None;
        self.pause_bis_s = None;
    }

    pub fn aktiv(&self) -> bool {
        self.aktiv
    }
}

/// Liest einen Prozentwert (0 < x <= 100). Ein Tippfehler wie `900` wuerde das
/// Gate sonst still ausschalten, weil keine echte Auslastung ihn je erreicht.
fn prozent_aus_umgebung(name: &str, standard: f32) -> f32 {
    let roh = std::env::var(name).unwrap_or_default();
    let roh = roh.trim();
    if roh.is_empty() {
        return standard;
    }
    match roh.parse::<f32>() {
        Ok(wert) if wert.is_finite() && wert > 0.0 && wert <= 100.0 => wert,
        _ => {
            tracing::warn!(
                variable = name,
                wert = roh,
                standard,
                "unbrauchbarer Prozentwert (erwartet 0 < x <= 100), nehme den Standard"
            );
            standard
        }
    }
}

fn u64_aus_umgebung(name: &str, standard: u64) -> u64 {
    let roh = std::env::var(name).unwrap_or_default();
    let roh = roh.trim();
    if roh.is_empty() {
        return standard;
    }
    match roh.parse::<u64>() {
        Ok(wert) => wert,
        Err(_) => {
            tracing::warn!(
                variable = name,
                wert = roh,
                standard,
                "unbrauchbarer Wert, nehme den Standard"
            );
            standard
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waechter() -> Lastwaechter {
        // Grenze 90, Freigabe 80, Fenster 240 s, kein Max-Halte-Deckel.
        Lastwaechter::neu(90.0, 80.0, 240, 0)
    }

    #[test]
    fn einzelner_ausschlag_oeffnet_das_gate_nicht() {
        let mut w = waechter();
        // Ein Wert ueber der Grenze, aber lange nicht das ganze Fenster.
        assert!(!w.beobachten(99.0, 0));
        assert!(!w.beobachten(99.0, 120));
        assert!(!w.aktiv());
    }

    #[test]
    fn dauerlast_ueber_das_fenster_oeffnet_das_gate() {
        let mut w = waechter();
        assert!(!w.beobachten(95.0, 0));
        assert!(!w.beobachten(95.0, 239));
        // Genau am Fenster kippt es.
        assert!(w.beobachten(95.0, 240));
        assert!(w.aktiv());
    }

    #[test]
    fn entlastung_setzt_die_uhr_zurueck() {
        let mut w = waechter();
        w.beobachten(95.0, 0);
        w.beobachten(95.0, 200);
        // Kurz drunter: die Uhr faengt von vorn an.
        assert!(!w.beobachten(50.0, 220));
        // Ab hier wieder ueber der Grenze, aber die 240 zaehlen ab 260.
        assert!(!w.beobachten(95.0, 260));
        assert!(!w.beobachten(95.0, 499));
        assert!(w.beobachten(95.0, 500));
    }

    #[test]
    fn hysterese_haelt_das_gate_zwischen_freigabe_und_grenze() {
        let mut w = waechter();
        // Gate oeffnen.
        w.beobachten(95.0, 0);
        assert!(w.beobachten(95.0, 240));
        // Zwischen 80 und 90: Gate bleibt aktiv.
        assert!(w.beobachten(85.0, 260));
        // Unter die Freigabe: Gate faellt.
        assert!(!w.beobachten(79.0, 280));
    }

    #[test]
    fn freigabe_ueber_grenze_wird_gekappt() {
        // Ueber aus_umgebung nicht testbar ohne Env; hier die Invariante direkt.
        let mut w = Lastwaechter::neu(90.0, 90.0, 60, 0);
        w.beobachten(95.0, 0);
        assert!(w.beobachten(95.0, 60));
        // Bei Freigabe == Grenze faellt das Gate erst unter der Grenze.
        assert!(!w.beobachten(89.0, 80));
    }

    #[test]
    fn max_halte_oeffnet_ein_garantiertes_katchup_fenster() {
        // Deckel 300 s, Fenster 60 s. Katchup = max(60, 120) = 120 s.
        let mut w = Lastwaechter::neu(90.0, 80.0, 60, 300);
        w.beobachten(99.0, 0);
        assert!(w.beobachten(99.0, 60)); // Gate an
        assert!(w.beobachten(99.0, 359)); // kurz vor dem Deckel noch aktiv
        // Am Deckel faellt es und oeffnet die Katchup-Pause bis 360+120=480.
        assert!(!w.beobachten(99.0, 360));
        // Waehrend der Pause bleibt es offen, egal wie hoch die Last ist -
        // lang genug, dass der 60-s-Auswerter es sicher trifft.
        assert!(!w.beobachten(99.0, 420));
        assert!(!w.beobachten(99.0, 479));
        // Nach der Pause muss die Last erst wieder ein volles Fenster
        // akkumulieren, bevor das Gate erneut greift.
        assert!(!w.beobachten(99.0, 480));
        assert!(!w.beobachten(99.0, 539));
        assert!(w.beobachten(99.0, 540));
    }

    #[test]
    fn zuruecksetzen_gibt_das_gate_frei() {
        let mut w = Lastwaechter::neu(90.0, 80.0, 60, 1800);
        w.beobachten(99.0, 0);
        assert!(w.beobachten(99.0, 60));
        // Messung faellt aus: fail-open.
        w.zuruecksetzen();
        assert!(!w.aktiv());
        // Danach beginnt die Akkumulation frisch.
        assert!(!w.beobachten(99.0, 80));
        assert!(!w.beobachten(99.0, 139));
        assert!(w.beobachten(99.0, 140));
    }
}

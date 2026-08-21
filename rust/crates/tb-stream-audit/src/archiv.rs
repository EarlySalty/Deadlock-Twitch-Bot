//! Archivierung fertiger Streams nach Google Drive.
//!
//! Neben den kurzen Auswertungs-Bloecken (die fuers Voice-zu-Text zerschnitten
//! und danach geloescht werden) laeuft je Stream ein durchgehender Recorder, der
//! eine vollstaendige, saubere 1:1-Aufnahme schreibt. Ist der Stream vorbei,
//! gehoert diese Aufnahme nicht auf die lokale Platte: sie wird zusammen mit den
//! Berichten in einen eigenen Ordner je Stream auf Google Drive geschoben. Erst
//! wenn das nachweislich oben liegt, wird lokal geloescht.
//!
//! Der rclone-Remote `gdrive:` traegt das OAuth-Token, hier steht nur der Ordner
//! darunter - dasselbe Muster wie beim VOD-Export.
//!
//! Diese Datei enthaelt bewusst nur die reine Logik (Pfade, Namen,
//! Kommandozeilen); die Prozesse laufen im Dienst. So bleibt sie ohne rclone und
//! echten Stream testbar.

use std::path::Path;

/// Basis-Ordner im Drive. `gdrive:` ist der rclone-Remote mit dem Token.
pub const REMOTE_ENV: &str = "STREAM_AUDIT_DRIVE_REMOTE";
pub const REMOTE_STANDARD: &str = "gdrive:Deadlock/Coaching-Audit";

/// Schaltet die Archivierung ab, ohne den Rest anzufassen. Standard: an.
pub const AKTIV_ENV: &str = "STREAM_AUDIT_DRIVE_ARCHIVE";

/// Praefix der durchgehenden Mitschnitt-Dateien im Lauf-Ordner. Ein Zeitstempel
/// haengt hinten dran, damit ein Neustart des Dienstes mitten im Stream die
/// vorige (Teil-)Aufnahme nicht ueberschreibt - beide werden hochgeladen.
pub const MITSCHNITT_PREFIX: &str = "mitschnitt-";

/// Endung des Mitschnitts: nur der Ton. Video braucht das Coaching nicht, und
/// ohne Video ist die Datei winzig (rund 1 MB je Minute statt ~50). ADTS/`.aac`
/// ist bewusst gewaehlt: ein hart gekappter Recorder hinterlaesst eine trotzdem
/// abspielbare Datei, anders als ein nicht finalisiertes MP4.
pub const MITSCHNITT_ENDUNG: &str = ".aac";

/// So viel Platz muss auf der Platte frei bleiben, damit ein neuer Recorder
/// startet. Der Ton ist winzig, aber ohne Untergrenze koennte ein langer
/// Drive-Ausfall (nichts wird hochgeladen und geloescht) die geteilte Platte
/// irgendwann fuellen. Faellt der Platz darunter, wird nicht mehr aufgenommen -
/// bestehende Aufnahmen bleiben unangetastet.
pub const MIN_FREE_ENV: &str = "STREAM_AUDIT_DRIVE_MIN_FREE_GB";
pub const MIN_FREE_STANDARD_GB: u64 = 20;

/// Mindest-Freiplatz in Bytes aus der Umgebung oder Standard.
pub fn min_frei_bytes() -> u64 {
    let gb = match std::env::var(MIN_FREE_ENV) {
        Ok(v) => v.trim().parse::<u64>().unwrap_or(MIN_FREE_STANDARD_GB),
        Err(_) => MIN_FREE_STANDARD_GB,
    };
    gb.saturating_mul(1024 * 1024 * 1024)
}

/// Ob nach dem Stream ueberhaupt archiviert wird.
pub fn archiv_aktiv() -> bool {
    aktiv_aus_wort(std::env::var(AKTIV_ENV).ok().as_deref())
}

/// Reine Wortlogik hinter [`archiv_aktiv`], ohne Umgebungszugriff - so testbar,
/// ohne prozessweite Variablen zu setzen.
fn aktiv_aus_wort(wert: Option<&str>) -> bool {
    match wert {
        Some(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "aus" || v == "nein")
        }
        None => true,
    }
}

/// Basis-Ordner im Drive aus der Umgebung oder Standard.
pub fn remote_basis() -> String {
    match std::env::var(REMOTE_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => REMOTE_STANDARD.to_string(),
    }
}

/// Zielordner eines Streams: ein eigener Ordner je Kanal und Lauf.
pub fn remote_ordner(basis: &str, kanal: &str, lauf: &str) -> String {
    format!("{}/{}/{}", basis.trim_end_matches('/'), kanal, lauf)
}

/// Dateiname eines durchgehenden Mitschnitts mit Zeitstempel.
pub fn mitschnitt_name(zeitstempel: i64) -> String {
    format!("{MITSCHNITT_PREFIX}{zeitstempel}{MITSCHNITT_ENDUNG}")
}

/// Ob ein Dateiname eine durchgehende Mitschnitt-Datei ist.
pub fn ist_mitschnitt(dateiname: &str) -> bool {
    dateiname.starts_with(MITSCHNITT_PREFIX) && dateiname.ends_with(MITSCHNITT_ENDUNG)
}

/// rclone-Argumente, um eine einzelne Datei in den Stream-Ordner auf Drive zu
/// kopieren. `copy` legt sie unter ihrem Namen im Zielordner ab. So bleibt die
/// grosse Mitschnitt-Datei aussen vor dem Sammelordner und wird nicht doppelt
/// auf der Platte gehalten.
pub fn rclone_datei_args(datei: &Path, remote_ordner: &str) -> Vec<String> {
    vec![
        "copy".to_string(),
        datei.to_string_lossy().into_owned(),
        remote_ordner.to_string(),
    ]
}

/// rclone-Argumente, um einen lokalen Ordner (die kleinen Berichte) in den
/// Stream-Ordner auf Drive zu kopieren.
pub fn rclone_ordner_args(quelle: &Path, remote_ordner: &str) -> Vec<String> {
    vec![
        "copy".to_string(),
        quelle.to_string_lossy().into_owned(),
        remote_ordner.to_string(),
    ]
}

/// rclone-Argumente, um zu pruefen, was im Stream-Ordner liegt. Vor dem
/// Loeschen: nur wenn das Ziel die Dateien wirklich fuehrt, ist der Upload
/// belegt.
pub fn rclone_lsf_args(remote_ordner: &str) -> Vec<String> {
    vec!["lsf".to_string(), remote_ordner.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_ordner_haengt_kanal_und_lauf_an() {
        assert_eq!(
            remote_ordner(
                "gdrive:Deadlock/Coaching-Audit",
                "helmbombenricky",
                "318370678502"
            ),
            "gdrive:Deadlock/Coaching-Audit/helmbombenricky/318370678502"
        );
        // Ein Schraegstrich am Ende der Basis darf nicht doppeln.
        assert_eq!(
            remote_ordner("gdrive:Coaching/", "k", "l"),
            "gdrive:Coaching/k/l"
        );
    }

    #[test]
    fn mitschnitt_name_und_erkennung() {
        let name = mitschnitt_name(1_700_000_000);
        assert_eq!(name, "mitschnitt-1700000000.aac");
        assert!(ist_mitschnitt(&name));
        assert!(!ist_mitschnitt("mitschnitt-1700000000.txt"));
        assert!(!ist_mitschnitt("block.json"));
        assert!(!ist_mitschnitt("audio.ts"));
    }

    #[test]
    fn rclone_argumente_stimmen() {
        let d = rclone_datei_args(Path::new("/tmp/mitschnitt-1.aac"), "gdrive:X/k/l");
        assert_eq!(d, vec!["copy", "/tmp/mitschnitt-1.aac", "gdrive:X/k/l"]);
        let o = rclone_ordner_args(Path::new("/tmp/stage"), "gdrive:X/k/l");
        assert_eq!(o, vec!["copy", "/tmp/stage", "gdrive:X/k/l"]);
        let l = rclone_lsf_args("gdrive:X/k/l");
        assert_eq!(l, vec!["lsf", "gdrive:X/k/l"]);
    }

    #[test]
    fn aktiv_wortlogik() {
        // Ohne Wert: an. Nur klare Nein-Woerter schalten ab.
        assert!(aktiv_aus_wort(None));
        assert!(aktiv_aus_wort(Some("1")));
        assert!(aktiv_aus_wort(Some("an")));
        assert!(!aktiv_aus_wort(Some("0")));
        assert!(!aktiv_aus_wort(Some(" AUS ")));
        assert!(!aktiv_aus_wort(Some("false")));
        assert!(!aktiv_aus_wort(Some("nein")));
    }
}

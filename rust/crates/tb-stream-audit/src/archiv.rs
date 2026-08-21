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

/// Schaltet die Archivierung ab, ohne den Rest anzufassen.
///
/// **Vorgabe ist an**, und das ist eine bewusste Entscheidung des Betreibers,
/// keine vergessene Absicherung: der Mitschnitt ist der Zweck dieses Bausteins,
/// und ein Coaching-Audit ohne Aufnahme waere wertlos. Die Unit setzt den
/// Schalter zusaetzlich ausdruecklich, damit er dort sichtbar ist. Wer
/// widerspricht, kommt in [`AUSNAHME_ENV`]; das wirkt je Kanal und braucht
/// keinen Eingriff am globalen Schalter.
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
///
/// Ein unlesbarer Wert faellt auf den Standard zurueck, aber nicht still: wer
/// `200G` oder `100.5` schreibt, wollte die Untergrenze anheben und bekaeme
/// sonst wortlos die 20 GB der Vorgabe.
pub fn min_frei_bytes() -> u64 {
    gb_aus_wort(std::env::var(MIN_FREE_ENV).ok().as_deref())
        .saturating_mul(1024 * 1024 * 1024)
}

/// Reine Wortlogik hinter [`min_frei_bytes`], ohne Umgebungszugriff.
fn gb_aus_wort(wert: Option<&str>) -> u64 {
    let Some(wert) = wert else {
        return MIN_FREE_STANDARD_GB;
    };
    let wert = wert.trim();
    if wert.is_empty() {
        return MIN_FREE_STANDARD_GB;
    }
    match wert.parse::<u64>() {
        Ok(gb) => gb,
        Err(_) => {
            tracing::warn!(
                wert,
                standard_gb = MIN_FREE_STANDARD_GB,
                "{MIN_FREE_ENV} ist keine ganze Zahl in GB - Vorgabe greift"
            );
            MIN_FREE_STANDARD_GB
        }
    }
}

/// Kanaele, die vom durchgehenden Mitschnitt ausgenommen sind, kommagetrennt.
///
/// Der Mitschnitt ist eine 1:1-Tonaufnahme eines fremden Streams. Widerspricht
/// ein Streamer, muss sich das genau fuer seinen Kanal umsetzen lassen - ohne
/// den Dienst fuer alle anderen abzuschalten. Fuer einen Kanal auf dieser Liste
/// startet kein Recorder und es wird nichts hochgeladen.
pub const AUSNAHME_ENV: &str = "STREAM_AUDIT_DRIVE_EXCLUDE";

/// Ob nach dem Stream ueberhaupt archiviert wird.
pub fn archiv_aktiv() -> bool {
    aktiv_aus_wort(std::env::var(AKTIV_ENV).ok().as_deref())
}

/// Ob fuer diesen Kanal aufgenommen und archiviert wird. Prueft den globalen
/// Schalter und die kanalweise Ausnahmeliste.
pub fn archiv_aktiv_fuer(kanal: &str) -> bool {
    archiv_aktiv()
        && !kanal_ausgenommen(kanal, std::env::var(AUSNAHME_ENV).ok().as_deref())
}

/// Reine Wortlogik hinter [`archiv_aktiv_fuer`], ohne Umgebungszugriff.
/// Gross- und Kleinschreibung spielt keine Rolle: Twitch-Logins sind klein,
/// aber wer die Liste pflegt, tippt sie leicht anders.
fn kanal_ausgenommen(kanal: &str, liste: Option<&str>) -> bool {
    let Some(liste) = liste else {
        return false;
    };
    let kanal = kanal.trim().to_ascii_lowercase();
    liste
        .split([',', ' ', ';'])
        .map(|eintrag| eintrag.trim().to_ascii_lowercase())
        .filter(|eintrag| !eintrag.is_empty())
        .any(|eintrag| eintrag == kanal)
}

/// Reine Wortlogik hinter [`archiv_aktiv`], ohne Umgebungszugriff - so testbar,
/// ohne prozessweite Variablen zu setzen. Ohne Wert gilt "an", siehe
/// [`AKTIV_ENV`].
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
    fn mindestplatz_faellt_nur_bei_unsinn_zurueck() {
        assert_eq!(gb_aus_wort(None), MIN_FREE_STANDARD_GB);
        assert_eq!(gb_aus_wort(Some("  ")), MIN_FREE_STANDARD_GB);
        assert_eq!(gb_aus_wort(Some(" 200 ")), 200);
        // Einheit oder Komma sind kein gueltiger Wert und ergeben die Vorgabe -
        // sichtbar im Protokoll, nicht stillschweigend.
        assert_eq!(gb_aus_wort(Some("200G")), MIN_FREE_STANDARD_GB);
        assert_eq!(gb_aus_wort(Some("100.5")), MIN_FREE_STANDARD_GB);
    }

    #[test]
    fn ein_widersprechender_kanal_wird_nicht_aufgenommen() {
        assert!(kanal_ausgenommen("skifahrertv", Some("skifahrertv")));
        assert!(kanal_ausgenommen(
            "skifahrertv",
            Some("helmbombenricky, SkifahrerTV")
        ));
        assert!(kanal_ausgenommen("skifahrertv", Some("a b skifahrertv")));
        // Andere Kanaele bleiben unberuehrt, und ein Teiltreffer zaehlt nicht.
        assert!(!kanal_ausgenommen("skifahrertv", Some("helmbombenricky")));
        assert!(!kanal_ausgenommen("ski", Some("skifahrertv")));
        assert!(!kanal_ausgenommen("skifahrertv", Some("")));
        assert!(!kanal_ausgenommen("skifahrertv", None));
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

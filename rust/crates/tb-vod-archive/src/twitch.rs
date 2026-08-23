//! Die Twitch-Seite des Archivs: VODs finden, laden, messen, schneiden.
//!
//! Alles laeuft ueber yt-dlp und ffmpeg statt ueber die Twitch-API. Das spart
//! einen zweiten Token-Pfad und liefert nebenbei die `info.json` mit dem echten
//! Aufnahmedatum, das die API so nicht hergibt.
//!
//! Jeder Unterprozess bekommt eine harte Zeitgrenze. Ein haengendes yt-dlp
//! wuerde sonst den Worker fuer immer blockieren, und weil der Worker nur
//! zweimal taeglich laeuft, faellt das erst Tage spaeter auf.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::config::{VodArchiveConfig, MAX_PART_SECONDS};
use crate::error::VodArchiveError;

/// Ergebnis eines Unterprozesses.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Startet Unterprozesse mit Zeitgrenze. Als Trait, damit die Ablauflogik ohne
/// echtes yt-dlp pruefbar bleibt.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(
        &self,
        program: &Path,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, VodArchiveError>;
}

pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(
        &self,
        program: &Path,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, VodArchiveError> {
        let kind = tokio::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Ohne kill_on_drop bliebe ein abgelaufener Prozess als Waise
            // laufen und wuerde weiter Bandbreite und Platte fressen.
            .kill_on_drop(true)
            .spawn()?;

        match tokio::time::timeout(timeout, kind.wait_with_output()).await {
            Ok(output) => {
                let output = output?;
                Ok(CommandOutput {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                })
            }
            Err(_) => Err(VodArchiveError::Zeitgrenze {
                programm: program.display().to_string(),
                sekunden: timeout.as_secs(),
            }),
        }
    }
}

/// Ein VOD, wie es in der Uebersichtsliste steht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VodEintrag {
    pub twitch_id: String,
    pub title: String,
    pub duration_sec: i64,
}

#[derive(Deserialize)]
struct Playlist {
    entries: Option<Vec<PlaylistEintrag>>,
}

#[derive(Deserialize)]
struct PlaylistEintrag {
    id: Option<String>,
    title: Option<String>,
    duration: Option<f64>,
}

#[derive(Deserialize)]
struct InfoJson {
    timestamp: Option<i64>,
    upload_date: Option<String>,
}

/// Twitch fuehrt VOD-IDs mal mit, mal ohne fuehrendes `v`.
pub fn vod_url(twitch_id: &str) -> String {
    format!(
        "https://www.twitch.tv/videos/{}",
        twitch_id.trim_start_matches('v')
    )
}

/// Sagt die Fehlerausgabe von yt-dlp eindeutig, dass der Kanal gerade nicht
/// sendet? Ein Exit-Code ungleich null allein sagt das nicht: ein Netzfehler,
/// ein 403 oder eine Sperre enden genauso. Nur diese Meldungen sind eine
/// Aussage ueber den Sendebetrieb.
///
/// Nachgemessen mit yt-dlp 2026.07.04:
/// offline liefert "ERROR: [twitch:stream] kanal: The channel is not currently
/// live", ein unerreichbarer Twitch-Endpunkt dagegen "Unable to download JSON
/// metadata: ...". Aeltere yt-dlp-Fassungen schreiben "<kanal> is offline".
pub fn meldet_offline(stderr: &str) -> bool {
    let text = stderr.to_lowercase();
    text.contains("not currently live")
        || text.contains("is offline")
        || text.contains("does not exist")
}

/// Laeuft gerade ein Stream? Dann ist das neueste VOD noch nicht vollstaendig
/// und wird ausgelassen, sonst landet ein halber Stream im Archiv.
///
/// Im Zweifel gilt der Kanal als live: ein spaeter geladenes VOD ist besser
/// als ein abgeschnittenes. "Im Zweifel" heisst hier alles ausser einer
/// eindeutigen Offline-Meldung, also auch ein gescheitertes yt-dlp.
pub async fn ist_live(runner: &dyn CommandRunner, cfg: &VodArchiveConfig, kanal: &str) -> bool {
    let args = vec![
        "--no-warnings".to_string(),
        "--simulate".to_string(),
        "--quiet".to_string(),
        format!("https://www.twitch.tv/{kanal}"),
    ];
    match runner.run(&cfg.yt_dlp, &args, Duration::from_secs(120)).await {
        Ok(output) if output.success => true,
        Ok(output) if meldet_offline(&output.stderr) => false,
        Ok(output) => {
            tracing::warn!(
                kanal = %kanal,
                meldung = %kurzfassung(&output.stderr),
                "Live-Pruefung nicht eindeutig, der Kanal gilt als live"
            );
            true
        }
        Err(fehler) => {
            tracing::warn!(
                kanal = %kanal,
                %fehler,
                "Live-Pruefung nicht ausfuehrbar, der Kanal gilt als live"
            );
            true
        }
    }
}

/// Zerlegt die yt-dlp-Playlist. Eintraege ohne ID sind unbrauchbar und fallen
/// still raus, fehlende Titel und Laengen dagegen nicht.
pub fn parse_vod_liste(json: &str) -> Result<Vec<VodEintrag>, VodArchiveError> {
    let playlist: Playlist = serde_json::from_str(json)?;
    Ok(playlist
        .entries
        .unwrap_or_default()
        .into_iter()
        .filter_map(|eintrag| {
            let twitch_id = eintrag.id.map(|id| id.trim().to_string())?;
            if twitch_id.is_empty() {
                return None;
            }
            Some(VodEintrag {
                twitch_id,
                title: eintrag.title.unwrap_or_else(|| "Twitch VOD".to_string()),
                duration_sec: eintrag.duration.unwrap_or(0.0) as i64,
            })
        })
        .collect())
}

/// Holt die Archiv-VODs des Kanals, neueste zuerst.
pub async fn liste_vods(
    runner: &dyn CommandRunner,
    cfg: &VodArchiveConfig,
    kanal: &str,
) -> Result<Vec<VodEintrag>, VodArchiveError> {
    let args = vec![
        "--no-warnings".to_string(),
        "--flat-playlist".to_string(),
        "-J".to_string(),
        format!("https://www.twitch.tv/{kanal}/videos?filter=archives"),
    ];
    let output = runner
        .run(&cfg.yt_dlp, &args, Duration::from_secs(300))
        .await?;
    if !output.success {
        return Err(VodArchiveError::Werkzeug {
            schritt: "VOD-Liste".to_string(),
            meldung: kurzfassung(&output.stderr),
        });
    }
    parse_vod_liste(&output.stdout)
}

/// Eine fertig geladene Aufzeichnung.
#[derive(Debug, Clone)]
pub struct Download {
    pub pfad: PathBuf,
    pub aufgenommen_am: Option<chrono::NaiveDate>,
}

/// yt-dlp-Argumente fuer den Download. Ausgelagert, damit die Argumentliste
/// ohne laufenden Prozess pruefbar ist.
pub fn download_args(cfg: &VodArchiveConfig, ziel: &Path, twitch_id: &str) -> Vec<String> {
    let mut args = vec![
        "--no-warnings".to_string(),
        "--no-progress".to_string(),
        // Twitch liefert bei langen VODs regelmaessig einzelne Fragmente
        // fehlerhaft; ohne die Wiederholungen bricht der halbe Lauf ab.
        "--retries".to_string(),
        "10".to_string(),
        "--fragment-retries".to_string(),
        "20".to_string(),
        "--concurrent-fragments".to_string(),
        "4".to_string(),
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        // Traegt das echte Aufnahmedatum, das die Playlist nicht hergibt.
        "--write-info-json".to_string(),
    ];
    if let Some(limit) = &cfg.rate_limit {
        args.push("--limit-rate".to_string());
        args.push(limit.clone());
    }
    args.push("-o".to_string());
    args.push(
        ziel.join(format!("{twitch_id}.%(ext)s"))
            .display()
            .to_string(),
    );
    args.push(vod_url(twitch_id));
    args
}

/// Laedt ein VOD in das Verzeichnis des Streamers.
pub async fn lade_vod(
    runner: &dyn CommandRunner,
    cfg: &VodArchiveConfig,
    verzeichnis: &Path,
    twitch_id: &str,
) -> Result<Download, VodArchiveError> {
    tokio::fs::create_dir_all(verzeichnis).await?;
    let args = download_args(cfg, verzeichnis, twitch_id);
    let output = runner.run(&cfg.yt_dlp, &args, cfg.download_timeout).await?;
    if !output.success {
        return Err(VodArchiveError::Werkzeug {
            schritt: "Download".to_string(),
            meldung: kurzfassung(&output.stderr),
        });
    }
    Ok(Download {
        pfad: finde_mediendatei(verzeichnis, twitch_id)?,
        aufgenommen_am: lies_aufnahmedatum(verzeichnis, twitch_id),
    })
}

/// Sucht die fertige Mediendatei. yt-dlp haelt sich nicht immer an `.mp4`,
/// deshalb faellt die Suche auf jede Datei mit passendem Stamm zurueck.
fn finde_mediendatei(verzeichnis: &Path, twitch_id: &str) -> Result<PathBuf, VodArchiveError> {
    let erwartet = verzeichnis.join(format!("{twitch_id}.mp4"));
    if erwartet.is_file() {
        return Ok(erwartet);
    }
    let mut kandidaten: Vec<PathBuf> = std::fs::read_dir(verzeichnis)?
        .filter_map(|eintrag| eintrag.ok().map(|e| e.path()))
        .filter(|pfad| {
            pfad.file_stem().and_then(|s| s.to_str()) == Some(twitch_id)
                && !matches!(
                    pfad.extension().and_then(|e| e.to_str()),
                    Some("json") | Some("part")
                )
        })
        .collect();
    kandidaten.sort();
    kandidaten
        .into_iter()
        .next()
        .ok_or_else(|| VodArchiveError::Werkzeug {
            schritt: "Download".to_string(),
            meldung: "keine Mediendatei entstanden".to_string(),
        })
}

/// Aufnahmedatum aus der `info.json`.
pub fn parse_aufnahmedatum(json: &str) -> Option<chrono::NaiveDate> {
    let info: InfoJson = serde_json::from_str(json).ok()?;
    if let Some(stempel) = info.timestamp {
        if let Some(zeit) = chrono::DateTime::from_timestamp(stempel, 0) {
            return Some(zeit.date_naive());
        }
    }
    // Rueckfall auf das Textdatum im Format YYYYMMDD.
    let roh = info.upload_date?;
    chrono::NaiveDate::parse_from_str(roh.trim(), "%Y%m%d").ok()
}

fn lies_aufnahmedatum(verzeichnis: &Path, twitch_id: &str) -> Option<chrono::NaiveDate> {
    let roh = std::fs::read_to_string(verzeichnis.join(format!("{twitch_id}.info.json"))).ok()?;
    parse_aufnahmedatum(&roh)
}

/// Misst die Laenge der geladenen Datei. Die Laenge aus der Playlist ist nur
/// ein Schaetzwert, geschnitten wird aber nach der echten Laenge.
pub async fn miss_laenge(runner: &dyn CommandRunner, cfg: &VodArchiveConfig, pfad: &Path) -> i64 {
    let args = vec![
        "-v".to_string(),
        "error".to_string(),
        "-show_entries".to_string(),
        "format=duration".to_string(),
        "-of".to_string(),
        "default=nw=1:nk=1".to_string(),
        pfad.display().to_string(),
    ];
    runner
        .run(&cfg.ffprobe, &args, Duration::from_secs(300))
        .await
        .ok()
        .filter(|output| output.success)
        .and_then(|output| output.stdout.trim().parse::<f64>().ok())
        .map(|sekunden| sekunden as i64)
        .unwrap_or(0)
}

/// ffmpeg-Argumente fuer den verlustfreien Schnitt.
pub fn schnitt_args(pfad: &Path, muster: &Path) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-i".to_string(),
        pfad.display().to_string(),
        // Nur umkopieren, nicht neu kodieren: sonst laeuft der Schnitt eines
        // achtstuendigen VODs laenger als der Download.
        "-c".to_string(),
        "copy".to_string(),
        "-map".to_string(),
        "0".to_string(),
        "-f".to_string(),
        "segment".to_string(),
        "-segment_time".to_string(),
        MAX_PART_SECONDS.to_string(),
        "-reset_timestamps".to_string(),
        "1".to_string(),
        muster.display().to_string(),
    ]
}

/// Baut das Namensmuster der Teile: `<stamm>.part%03d.<endung>`.
pub fn teil_muster(pfad: &Path) -> Option<PathBuf> {
    let stamm = pfad.file_stem().and_then(|s| s.to_str())?;
    let endung = pfad.extension().and_then(|s| s.to_str()).unwrap_or("mp4");
    Some(pfad.parent()?.join(format!("{stamm}.part%03d.{endung}")))
}

/// Schneidet ueberlange VODs, weil YouTube bei 12 Stunden dichtmacht. Kurze
/// Aufzeichnungen bleiben unangetastet und gelten als einteilig.
pub async fn schneide_bei_bedarf(
    runner: &dyn CommandRunner,
    cfg: &VodArchiveConfig,
    pfad: &Path,
    laenge_sec: i64,
) -> Result<Vec<PathBuf>, VodArchiveError> {
    if laenge_sec <= MAX_PART_SECONDS {
        return Ok(vec![pfad.to_path_buf()]);
    }
    let muster = teil_muster(pfad).ok_or_else(|| VodArchiveError::Werkzeug {
        schritt: "Schneiden".to_string(),
        meldung: format!("unbrauchbarer Dateiname: {}", pfad.display()),
    })?;
    tracing::info!(laenge_sec, datei = %pfad.display(), "VOD ist zu lang, wird geschnitten");

    let output = runner
        .run(
            &cfg.ffmpeg,
            &schnitt_args(pfad, &muster),
            Duration::from_secs(14_400),
        )
        .await?;
    if !output.success {
        return Err(VodArchiveError::Werkzeug {
            schritt: "Schneiden".to_string(),
            meldung: kurzfassung(&output.stderr),
        });
    }

    let teile = sammle_teile(pfad)?;
    if teile.is_empty() {
        return Err(VodArchiveError::Werkzeug {
            schritt: "Schneiden".to_string(),
            meldung: "keine Teile entstanden".to_string(),
        });
    }
    Ok(teile)
}

fn sammle_teile(pfad: &Path) -> Result<Vec<PathBuf>, VodArchiveError> {
    let stamm = pfad
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let endung = pfad.extension().and_then(|s| s.to_str()).unwrap_or("mp4");
    let praefix = format!("{stamm}.part");
    let verzeichnis = pfad.parent().unwrap_or_else(|| Path::new("."));
    let mut teile: Vec<PathBuf> = std::fs::read_dir(verzeichnis)?
        .filter_map(|eintrag| eintrag.ok().map(|e| e.path()))
        .filter(|kandidat| {
            kandidat
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with(&praefix) && name.ends_with(endung))
                .unwrap_or(false)
        })
        .collect();
    teile.sort();
    Ok(teile)
}

/// Das Ende einer yt-dlp-Fehlerausgabe traegt die eigentliche Ursache, der
/// Anfang nur Fortschrittsrauschen. Ungekuerzt sind das zehntausende Zeichen.
fn kurzfassung(stderr: &str) -> String {
    let getrimmt = stderr.trim();
    let zeichen: Vec<char> = getrimmt.chars().collect();
    if zeichen.len() <= 500 {
        return getrimmt.to_string();
    }
    zeichen[zeichen.len() - 500..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Runner, der feste Antworten liefert und die Aufrufe mitschreibt.
    struct TestRunner {
        antwort: CommandOutput,
        aufrufe: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl TestRunner {
        fn neu(success: bool, stdout: &str, stderr: &str) -> Self {
            Self {
                antwort: CommandOutput {
                    success,
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
                },
                aufrufe: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl CommandRunner for TestRunner {
        async fn run(
            &self,
            program: &Path,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, VodArchiveError> {
            self.aufrufe
                .lock()
                .unwrap()
                .push((program.display().to_string(), args.to_vec()));
            Ok(self.antwort.clone())
        }
    }

    #[test]
    fn vod_url_entfernt_das_v() {
        assert_eq!(
            vod_url("v2841862636"),
            "https://www.twitch.tv/videos/2841862636"
        );
        assert_eq!(
            vod_url("2841862636"),
            "https://www.twitch.tv/videos/2841862636"
        );
    }

    #[test]
    fn playlist_ueberlebt_fehlende_felder() {
        let roh = r#"{"entries":[{"id":"v1"},{"title":"ohne id"},{"id":"v2","title":"da","duration":12.5}]}"#;
        let eintraege = parse_vod_liste(roh).unwrap();
        assert_eq!(eintraege.len(), 2);
        assert_eq!(eintraege[0].title, "Twitch VOD");
        assert_eq!(eintraege[1].duration_sec, 12);
    }

    #[test]
    fn leere_playlist_ist_kein_fehler() {
        assert!(parse_vod_liste("{}").unwrap().is_empty());
        assert!(parse_vod_liste("{\"entries\":null}").unwrap().is_empty());
    }

    #[test]
    fn aufnahmedatum_aus_zeitstempel_und_textdatum() {
        assert_eq!(
            parse_aufnahmedatum(r#"{"timestamp":1700000000}"#),
            chrono::NaiveDate::from_ymd_opt(2023, 11, 14)
        );
        assert_eq!(
            parse_aufnahmedatum(r#"{"upload_date":"20260813"}"#),
            chrono::NaiveDate::from_ymd_opt(2026, 8, 13)
        );
        assert!(parse_aufnahmedatum(r#"{"upload_date":"kaputt"}"#).is_none());
        assert!(parse_aufnahmedatum("kein json").is_none());
    }

    #[test]
    fn kurze_vods_werden_nicht_geschnitten() {
        let cfg = VodArchiveConfig::default();
        let runner = TestRunner::neu(true, "", "");
        let pfad = Path::new("/tmp/beispiel.mp4");
        let teile =
            tokio_test_block(schneide_bei_bedarf(&runner, &cfg, pfad, MAX_PART_SECONDS)).unwrap();
        assert_eq!(teile, vec![pfad.to_path_buf()]);
        // ffmpeg wurde gar nicht erst gestartet.
        assert!(runner.aufrufe.lock().unwrap().is_empty());
    }

    #[test]
    fn teil_muster_haengt_am_dateinamen() {
        assert_eq!(
            teil_muster(Path::new("/archiv/v123.mp4")),
            Some(PathBuf::from("/archiv/v123.part%03d.mp4"))
        );
        assert_eq!(
            teil_muster(Path::new("/archiv/v123.mkv")),
            Some(PathBuf::from("/archiv/v123.part%03d.mkv"))
        );
    }

    #[test]
    fn download_args_tragen_datum_und_bremse() {
        let cfg = VodArchiveConfig {
            rate_limit: Some("5M".to_string()),
            ..VodArchiveConfig::default()
        };
        let args = download_args(&cfg, Path::new("/archiv"), "v9");
        assert!(args.contains(&"--write-info-json".to_string()));
        assert!(args.contains(&"5M".to_string()));
        assert!(args.contains(&"/archiv/v9.%(ext)s".to_string()));
        assert_eq!(args.last().unwrap(), "https://www.twitch.tv/videos/9");
    }

    #[test]
    fn liste_meldet_fehlerausgabe_gekuerzt() {
        let cfg = VodArchiveConfig::default();
        let runner = TestRunner::neu(false, "", &"x".repeat(2000));
        let fehler = tokio_test_block(liste_vods(&runner, &cfg, "earlysalty")).unwrap_err();
        let text = fehler.to_string();
        assert!(text.contains("VOD-Liste"));
        assert!(text.len() < 700);
    }

    #[test]
    fn live_pruefung_ist_im_zweifel_vorsichtig() {
        let cfg = VodArchiveConfig::default();
        // Ein Runner, der nur Fehler liefert, darf nicht "nicht live" melden.
        struct Kaputt;
        #[async_trait]
        impl CommandRunner for Kaputt {
            async fn run(
                &self,
                _program: &Path,
                _args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, VodArchiveError> {
                Err(VodArchiveError::Werkzeug {
                    schritt: "test".to_string(),
                    meldung: "kaputt".to_string(),
                })
            }
        }
        assert!(tokio_test_block(ist_live(&Kaputt, &cfg, "earlysalty")));
    }

    #[test]
    fn nur_eine_eindeutige_offline_meldung_zaehlt_als_nicht_live() {
        let cfg = VodArchiveConfig::default();
        // Echte yt-dlp-Ausgaben, siehe Kommentar an `meldet_offline`.
        let offline = TestRunner::neu(
            false,
            "",
            "ERROR: [twitch:stream] earlysalty: The channel is not currently live",
        );
        assert!(!tokio_test_block(ist_live(&offline, &cfg, "earlysalty")));

        // Ein Netzfehler sagt nichts ueber den Sendebetrieb. Frueher galt der
        // Kanal hier als offline, das laufende VOD wurde angeschnitten
        // archiviert und hochgeladen.
        let netzfehler = TestRunner::neu(
            false,
            "",
            "ERROR: [twitch:stream] earlysalty: Unable to download JSON metadata: \
             [Errno 111] Connection refused",
        );
        assert!(tokio_test_block(ist_live(&netzfehler, &cfg, "earlysalty")));

        // Erfolg heisst: es laeuft etwas.
        let laeuft = TestRunner::neu(true, "", "");
        assert!(tokio_test_block(ist_live(&laeuft, &cfg, "earlysalty")));
    }

    #[test]
    fn offline_meldungen_werden_erkannt() {
        assert!(meldet_offline(
            "ERROR: [twitch:stream] nani: The channel is not currently live"
        ));
        // Aeltere yt-dlp-Fassung.
        assert!(meldet_offline("ERROR: [twitch:stream] nani: nani is offline"));
        assert!(meldet_offline(
            "ERROR: [twitch:stream] nani: nani does not exist"
        ));
        assert!(!meldet_offline(
            "ERROR: HTTP Error 403: Forbidden (caused by TransportError)"
        ));
        assert!(!meldet_offline(""));
    }

    /// Kleiner Runtime-Helfer, damit die Tests ohne tokio-Attribut auskommen.
    fn tokio_test_block<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(fut)
    }
}

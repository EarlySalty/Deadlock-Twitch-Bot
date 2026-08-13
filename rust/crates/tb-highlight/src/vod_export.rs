use std::{path::Path, process::Stdio};

use async_trait::async_trait;
use chrono::DateTime;
use chrono_tz::Europe::Berlin;
use thiserror::Error;

use crate::{
    config::FFMPEG_PATH,
    twitch_vod::{parse_duration_seconds, parse_twitch_datetime, TwitchVodApi},
};

pub const TARGET_LOGIN: &str = "dach_lock";
/// Standardziel im Google Drive. Storj war auf Dauer zu teuer; der rclone-Remote
/// `gdrive:` traegt das OAuth-Token, hier steht nur der Ordner darunter.
pub const DEFAULT_REMOTE_BASE: &str = "gdrive:Deadlock/Twitch-VODs";
/// Rueckfallordner, wenn der Zeitstempel des VOD nicht in ein Datum aufloest.
/// Der Export soll dann trotzdem laufen — eine Datei im falschen Ordner ist
/// besser als ein Abbruch nach einem mehrstuendigen Download.
const UNKNOWN_DATE_FOLDER: &str = "ohne-datum";
/// Ein Archiv-VOD gilt nur als "der gerade beendete Stream", wenn sein
/// geschätztes Ende (`created_at` + `duration`) höchstens so weit vom
/// `stream.offline`-Zeitpunkt abweicht. Deckt Twitchs eigene
/// Verarbeitungsverzögerung ab, grenzt aber zuverlässig gegen ein älteres
/// VOD desselben Kanals ab (z. B. ein zweiter Stream am selben Tag), das
/// sonst als "das gerade beendete" durchgehen könnte, während das echte
/// neue VOD in der API noch nicht sichtbar ist.
const MAX_VOD_END_DRIFT_SECONDS: i64 = 30 * 60;

pub fn should_export(login: Option<&str>) -> bool {
    login == Some(TARGET_LOGIN)
}

/// Ergebnis eines erfolgreichen Exports. Traegt neben dem Freigabelink die
/// Kennzahlen, die der Log-Channel braucht — die lokale Datei ist zum
/// Meldezeitpunkt schon geloescht, `size_bytes` wird deshalb vor dem Aufraeumen
/// gemessen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VodExportReport {
    pub vod_id: String,
    pub link: String,
    pub duration_seconds: i64,
    pub size_bytes: u64,
}

pub fn export_log_title(success: bool) -> &'static str {
    if success {
        "VOD-Export erfolgreich"
    } else {
        "VOD-Export fehlgeschlagen"
    }
}

/// Beschreibungstext fuer den Discord-Log-Channel — **jeder** Ausgang wird
/// gemeldet, Erfolg wie Abbruch. Der Freigabelink bleibt in diesem Text bewusst
/// draussen: er oeffnet dauerhaft ein privates VOD. Er wird nur dort ausgegeben,
/// wo alle Mitlesenden das VOD ohnehin sehen duerfen — in der DM und im
/// Caster-Chat, beide Male als eigener Nachrichtentext neben diesem Embed.
pub fn export_log_description(
    result: &Result<VodExportReport, VodExportError>,
    elapsed_seconds: i64,
    dm_delivered: bool,
) -> String {
    match result {
        Ok(report) => {
            let dm_status = if dm_delivered {
                "DM zugestellt"
            } else {
                "DM fehlgeschlagen"
            };
            format!(
                "Kanal: {TARGET_LOGIN}\nVOD: {vod_id}\nStreamlaenge: {stream}\nGroesse: {size}\nExportdauer: {elapsed}\nFreigabe: Drive-Link, {dm_status}",
                vod_id = report.vod_id,
                stream = format_duration(report.duration_seconds),
                size = format_bytes(report.size_bytes),
                elapsed = format_duration(elapsed_seconds),
            )
        }
        // Ein fehlgeschlagener yt-dlp-Lauf ueber ein mehrstuendiges VOD kann
        // zehntausende Zeichen stderr liefern. Ungekuerzt lehnt Discord das
        // Embed ab (description-Limit) — der Log-Channel schwiege dann
        // ausgerechnet im lautesten Fehlerfall.
        Err(error) => format!(
            "Kanal: {TARGET_LOGIN}\nGrund: {grund}\nAbbruch nach: {elapsed}",
            grund = truncate_chars(&error.to_string(), MAX_REASON_CHARS),
            elapsed = format_duration(elapsed_seconds),
        ),
    }
}

/// Obergrenze fuer den Fehlergrund im Embed. Deutlich unter Discords
/// description-Limit (4096), damit Rahmenzeilen und Mehrbyte-Zeichen nicht
/// darueber hinauslaufen.
const MAX_REASON_CHARS: usize = 1500;

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let gekuerzt: String = text.chars().take(limit).collect();
    format!("{gekuerzt}… [gekuerzt]")
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let bytes_f = bytes as f64;
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    for (limit, unit) in [
        (KB * KB, "KB"),
        (KB * KB * KB, "MB"),
        (KB * KB * KB * KB, "GB"),
    ] {
        if bytes_f < limit {
            return format!("{:.2} {unit}", bytes_f / (limit / KB));
        }
    }
    format!("{:.2} TB", bytes_f / (KB * KB * KB * KB))
}

pub fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let rest = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {rest}s")
    } else {
        format!("{rest}s")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, program: &Path, args: &[String]) -> std::io::Result<CommandOutput>;
}

pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(&self, program: &Path, args: &[String]) -> std::io::Result<CommandOutput> {
        let output = tokio::process::Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Debug, Error)]
pub enum VodExportError {
    #[error("kein Archiv-VOD gefunden")]
    NoVod,
    #[error("kein neues Archiv-VOD seit Stream-Start gefunden")]
    NoNewVod,
    #[error("ungueltige Twitch-VOD-ID: {0}")]
    InvalidVodId(String),
    #[error("lokales Export-Verzeichnis konnte nicht erstellt werden: {0}")]
    CreateDirectory(#[source] std::io::Error),
    #[error("{program} konnte nicht gestartet werden: {source}")]
    StartCommand {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} fehlgeschlagen: {stderr}")]
    CommandFailed { program: String, stderr: String },
    #[error("yt-dlp meldete Erfolg, aber die Ausgabedatei fehlt")]
    MissingDownload,
    #[error("lokale VOD-Datei konnte nicht geloescht werden: {0}")]
    Cleanup(#[source] std::io::Error),
    #[error("rclone lieferte keinen Freigabelink")]
    MissingLink,
}

/// Statische Ziele/Pfade für einen Export-Lauf (yt-dlp/rclone-Binaries,
/// Archiv-Ordner, lokales Zwischenverzeichnis) — gebündelt, damit
/// `export_latest_vod` nicht auf eine unübersichtliche Parameterliste wächst.
pub struct ExportTargets<'a> {
    pub yt_dlp_path: &'a Path,
    pub rclone_path: &'a Path,
    /// rclone-Ziel inklusive Remote, z. B. `gdrive:Deadlock/Twitch-VODs`.
    pub remote_base: &'a str,
    pub temp_dir: &'a Path,
}

pub async fn export_latest_vod(
    api: &dyn TwitchVodApi,
    runner: &dyn CommandRunner,
    targets: &ExportTargets<'_>,
    channel_id: &str,
    stream_offline_unix: i64,
) -> Result<VodExportReport, VodExportError> {
    let vods = api.get_archive_videos(channel_id, 1).await;
    let vod = vods.first().ok_or(VodExportError::NoVod)?;
    let vod_id = vod
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or(VodExportError::NoVod)?;
    if !vod_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(VodExportError::InvalidVodId(vod_id.to_string()));
    }
    let created_at = vod
        .get("created_at")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let started_at = parse_twitch_datetime(created_at).ok_or(VodExportError::NoNewVod)?;
    let duration = vod
        .get("duration")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let duration_seconds = parse_duration_seconds(duration);
    if duration_seconds <= 0 {
        return Err(VodExportError::NoNewVod);
    }
    let estimated_end = started_at + duration_seconds;
    if (stream_offline_unix - estimated_end).abs() > MAX_VOD_END_DRIFT_SECONDS {
        return Err(VodExportError::NoNewVod);
    }

    let temp_dir = targets.temp_dir;
    std::fs::create_dir_all(temp_dir).map_err(VodExportError::CreateDirectory)?;
    let local_path = temp_dir.join(format!("{vod_id}.mp4"));
    cleanup_download_files(temp_dir, vod_id);

    if let Err(error) = run_checked(
        runner,
        targets.yt_dlp_path,
        &yt_dlp_args(vod_id, &local_path),
    )
    .await
    {
        cleanup_download_files(temp_dir, vod_id);
        return Err(error);
    }
    if !local_path.is_file() {
        cleanup_download_files(temp_dir, vod_id);
        return Err(VodExportError::MissingDownload);
    }

    // Groesse vor dem Upload messen — danach ist die lokale Datei geloescht und
    // die Kennzahl fuer den Log-Channel waere nicht mehr zu bekommen.
    let size_bytes = std::fs::metadata(&local_path)
        .map(|meta| meta.len())
        .unwrap_or(0);

    let upload_result = run_checked(
        runner,
        targets.rclone_path,
        &rclone_copy_args(&local_path, targets.remote_base, vod_id, started_at),
    )
    .await;
    let cleanup_result = std::fs::remove_file(&local_path);
    if let Err(error) = &cleanup_result {
        tracing::warn!(
            %error,
            path = %local_path.display(),
            "VOD-Export: lokale Datei konnte nicht geloescht werden"
        );
    }
    upload_result?;
    cleanup_result.map_err(VodExportError::Cleanup)?;

    let link_output = run_checked(
        runner,
        targets.rclone_path,
        &rclone_link_args(targets.remote_base, vod_id, started_at),
    )
    .await?;
    let link = link_output
        .stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
        .ok_or(VodExportError::MissingLink)?;

    Ok(VodExportReport {
        vod_id: vod_id.to_string(),
        link,
        duration_seconds,
        size_bytes,
    })
}

fn yt_dlp_args(vod_id: &str, output_path: &Path) -> Vec<String> {
    vec![
        "--ffmpeg-location".to_string(),
        FFMPEG_PATH.to_string(),
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        "-f".to_string(),
        "bestvideo+bestaudio/best".to_string(),
        "-o".to_string(),
        output_path.to_string_lossy().into_owned(),
        format!("https://www.twitch.tv/videos/{vod_id}"),
    ]
}

/// Ordnername fuer einen Streamtag. Twitch liefert `created_at` in UTC — ein
/// Stream, der um 01:30 Berliner Zeit anfaengt, gehoert in den Ordner dieses
/// Tages und nicht in den des UTC-Vortags.
fn stream_date_folder(started_at_unix: i64) -> String {
    DateTime::from_timestamp(started_at_unix, 0)
        .map(|dt| dt.with_timezone(&Berlin).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| UNKNOWN_DATE_FOLDER.to_string())
}

fn remote_object_path(remote_base: &str, vod_id: &str, started_at_unix: i64) -> String {
    format!(
        "{}/{ordner}/{vod_id}.mp4",
        remote_base.trim_end_matches('/'),
        ordner = stream_date_folder(started_at_unix)
    )
}

fn rclone_copy_args(
    local_path: &Path,
    remote_base: &str,
    vod_id: &str,
    started_at_unix: i64,
) -> Vec<String> {
    vec![
        "copyto".to_string(),
        local_path.to_string_lossy().into_owned(),
        remote_object_path(remote_base, vod_id, started_at_unix),
    ]
}

/// Kein `--expire`: Google Drive kennt keine ablaufenden Freigabelinks. Der Link bleibt
/// gueltig, bis die Datei geloescht oder die Freigabe entzogen wird.
fn rclone_link_args(remote_base: &str, vod_id: &str, started_at_unix: i64) -> Vec<String> {
    vec![
        "link".to_string(),
        remote_object_path(remote_base, vod_id, started_at_unix),
    ]
}

async fn run_checked(
    runner: &dyn CommandRunner,
    program: &Path,
    args: &[String],
) -> Result<CommandOutput, VodExportError> {
    let program_name = program.display().to_string();
    let output =
        runner
            .run(program, args)
            .await
            .map_err(|source| VodExportError::StartCommand {
                program: program_name.clone(),
                source,
            })?;
    if output.success {
        Ok(output)
    } else {
        Err(VodExportError::CommandFailed {
            program: program_name,
            stderr: output.stderr.trim().to_string(),
        })
    }
}

fn cleanup_download_files(temp_dir: &Path, vod_id: &str) {
    let prefix = format!("{vod_id}.");
    let Ok(entries) = std::fs::read_dir(temp_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::twitch_vod::TwitchVodApi;

    fn beispiel_report() -> VodExportReport {
        VodExportReport {
            vod_id: "987".to_string(),
            link: "https://share.example/987".to_string(),
            duration_seconds: 3 * 60 * 60 + 25 * 60,
            size_bytes: 9_942_383_597,
        }
    }

    #[test]
    fn erfolgs_log_nennt_kennzahlen_aber_nie_den_link() {
        let text = export_log_description(&Ok(beispiel_report()), 780, true);

        assert!(text.contains("987"), "VOD-ID fehlt: {text}");
        assert!(text.contains("3h 25m"), "Streamdauer fehlt: {text}");
        assert!(text.contains("9.26 GB"), "Groesse fehlt: {text}");
        assert!(text.contains("13m 0s"), "Exportdauer fehlt: {text}");
        assert!(text.contains("DM zugestellt"), "DM-Status fehlt: {text}");
        // Der Freigabelink ist eine private Drive-Freigabe auf ein
        // nicht-oeffentliches VOD — er darf nie im Log-Channel landen.
        assert!(
            !text.contains("share.example"),
            "Freigabelink im Log-Channel: {text}"
        );
    }

    #[test]
    fn erfolgs_log_meldet_auch_fehlgeschlagene_dm() {
        let text = export_log_description(&Ok(beispiel_report()), 780, false);

        assert!(
            text.contains("DM fehlgeschlagen"),
            "DM-Status fehlt: {text}"
        );
    }

    #[test]
    fn jeder_fehlerausgang_wird_gemeldet() {
        for error in [
            VodExportError::NoVod,
            VodExportError::NoNewVod,
            VodExportError::MissingDownload,
            VodExportError::MissingLink,
            VodExportError::InvalidVodId("abc".to_string()),
            VodExportError::CommandFailed {
                program: "rclone".to_string(),
                stderr: "quota exceeded".to_string(),
            },
        ] {
            let erwartet = error.to_string();
            let text = export_log_description(&Err(error), 42, false);
            assert!(
                text.contains(&erwartet),
                "Fehlergrund fehlt im Log: {text} (erwartet: {erwartet})"
            );
            assert!(text.contains("42s"), "Laufzeit fehlt im Log: {text}");
        }
    }

    #[test]
    fn langer_subprozess_stderr_wird_gekuerzt() {
        // yt-dlp haengt bei einem mehrstuendigen VOD pro Fragment-Retry eine
        // Zeile an — 20k Zeichen sind realistisch, Discord nimmt aber nur
        // 4096 Zeichen Description. Ungekuerzt bliebe der Log-Channel im
        // Fehlerfall stumm.
        let stderr = "ERROR: fragment 1 retry\n".repeat(1000);
        assert!(stderr.chars().count() > 20_000);

        let text = export_log_description(
            &Err(VodExportError::CommandFailed {
                program: "/opt/yt-dlp".to_string(),
                stderr,
            }),
            42,
            false,
        );

        assert!(
            text.chars().count() < 4096,
            "Description ueber Discord-Limit: {} Zeichen",
            text.chars().count()
        );
        assert!(text.contains("[gekuerzt]"), "Kuerzungsmarke fehlt: {text}");
        assert!(text.contains("/opt/yt-dlp"), "Programm fehlt: {text}");
        assert!(text.contains("42s"), "Laufzeit fehlt: {text}");
    }

    #[test]
    fn kurzer_grund_bleibt_unveraendert() {
        assert_eq!(truncate_chars("kurz", 1500), "kurz");
        // Mehrbyte-Zeichen zaehlen als ein Zeichen, nicht als Bytes — sonst
        // schnitte die Kuerzung mitten in ein UTF-8-Zeichen.
        assert_eq!(truncate_chars("äöü", 2), "äö… [gekuerzt]");
    }

    #[test]
    fn log_titel_trennt_erfolg_und_fehlschlag() {
        assert_eq!(export_log_title(true), "VOD-Export erfolgreich");
        assert_eq!(export_log_title(false), "VOD-Export fehlgeschlagen");
    }

    #[test]
    fn groessen_und_dauerformat_bleiben_lesbar() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.00 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MB");
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(600), "10m 0s");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn trigger_nur_fuer_dach_lock() {
        assert!(should_export(Some("dach_lock")));
        assert!(!should_export(Some("DACH_LOCK")));
        assert!(!should_export(Some("anderer_kanal")));
        assert!(!should_export(None));
    }

    #[test]
    fn drive_pfad_und_rclone_befehle_sind_korrekt() {
        let local = Path::new("/tmp/987.mp4");

        assert_eq!(
            remote_object_path(DEFAULT_REMOTE_BASE, "987", FRESH_CREATED_AT_UNIX),
            "gdrive:Deadlock/Twitch-VODs/2024-01-01/987.mp4"
        );
        // Ein abschliessender Schraegstrich in der Konfiguration darf den Pfad nicht doppeln.
        assert_eq!(
            remote_object_path("gdrive:Deadlock/Twitch-VODs/", "987", FRESH_CREATED_AT_UNIX),
            "gdrive:Deadlock/Twitch-VODs/2024-01-01/987.mp4"
        );
        assert_eq!(
            rclone_copy_args(local, DEFAULT_REMOTE_BASE, "987", FRESH_CREATED_AT_UNIX),
            vec![
                "copyto",
                "/tmp/987.mp4",
                "gdrive:Deadlock/Twitch-VODs/2024-01-01/987.mp4",
            ]
        );
        // Drive kennt kein Ablaufdatum — mit --expire bricht rclone den Aufruf ab.
        assert_eq!(
            rclone_link_args(DEFAULT_REMOTE_BASE, "987", FRESH_CREATED_AT_UNIX),
            vec!["link", "gdrive:Deadlock/Twitch-VODs/2024-01-01/987.mp4"]
        );
    }

    #[test]
    fn nachtstream_landet_im_ordner_des_berliner_tages() {
        // 2026-08-09T23:30:00Z ist in Berlin bereits der 10.08. um 01:30 —
        // ein Nachtstream gehoert in den Ordner des Tages, an dem er
        // tatsaechlich lief, nicht in den des UTC-Vortags.
        assert_eq!(stream_date_folder(1_786_318_200), "2026-08-10");
        // Winterzeit (+1): 21:30Z ist noch derselbe Tag.
        assert_eq!(stream_date_folder(1_766_957_400), "2025-12-28");
    }

    #[test]
    fn unbrauchbarer_zeitstempel_bricht_den_export_nicht_ab() {
        assert_eq!(stream_date_folder(i64::MAX), UNKNOWN_DATE_FOLDER);
        assert_eq!(
            remote_object_path(DEFAULT_REMOTE_BASE, "987", i64::MAX),
            "gdrive:Deadlock/Twitch-VODs/ohne-datum/987.mp4"
        );
    }

    const FRESH_CREATED_AT: &str = "2024-01-01T12:00:00Z";
    const FRESH_CREATED_AT_UNIX: i64 = 1_704_110_400;
    const FRESH_DURATION: &str = "2h";
    const FRESH_DURATION_SECONDS: i64 = 2 * 60 * 60;
    const FRESH_VOD_END_UNIX: i64 = FRESH_CREATED_AT_UNIX + FRESH_DURATION_SECONDS;

    struct MockApi {
        created_at: &'static str,
        duration: &'static str,
    }

    impl Default for MockApi {
        fn default() -> Self {
            Self {
                created_at: FRESH_CREATED_AT,
                duration: FRESH_DURATION,
            }
        }
    }

    #[async_trait]
    impl TwitchVodApi for MockApi {
        async fn get_user_info(&self, _login: &str) -> Option<serde_json::Value> {
            None
        }

        async fn get_archive_videos(
            &self,
            _channel_id: &str,
            first: u32,
        ) -> Vec<serde_json::Value> {
            assert_eq!(first, 1);
            vec![json!({
                "id": "987",
                "created_at": self.created_at,
                "duration": self.duration,
            })]
        }
    }

    #[derive(Default)]
    struct MockRunner {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        fail_download: bool,
    }

    #[async_trait]
    impl CommandRunner for MockRunner {
        async fn run(&self, program: &Path, args: &[String]) -> std::io::Result<CommandOutput> {
            self.calls
                .lock()
                .expect("call lock")
                .push((program.display().to_string(), args.to_vec()));
            if program == Path::new("/opt/yt-dlp") {
                let output_index = args.iter().position(|arg| arg == "-o").expect("-o");
                let output_path = &args[output_index + 1];
                if self.fail_download {
                    std::fs::write(format!("{output_path}.part"), b"partial")?;
                } else {
                    std::fs::write(output_path, b"vod")?;
                }
            }
            Ok(CommandOutput {
                success: !(self.fail_download && program == Path::new("/opt/yt-dlp")),
                stdout: if args.first().map(String::as_str) == Some("link") {
                    "notice\nhttps://share.example/987\n".to_string()
                } else {
                    String::new()
                },
                stderr: String::new(),
            })
        }
    }

    #[tokio::test]
    async fn export_nutzt_gemockte_subprozesse_und_loescht_lokale_datei() {
        let runner = Arc::new(MockRunner::default());
        let temp_dir = std::env::temp_dir().join(format!(
            "tb-vod-export-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        std::fs::create_dir_all(&temp_dir).expect("temp dir");

        let targets = ExportTargets {
            yt_dlp_path: Path::new("/opt/yt-dlp"),
            rclone_path: Path::new("rclone"),
            remote_base: "gdrive:Deadlock/Twitch-VODs",
            temp_dir: &temp_dir,
        };
        let report = export_latest_vod(
            &MockApi::default(),
            runner.as_ref(),
            &targets,
            "channel-1",
            FRESH_VOD_END_UNIX + 100,
        )
        .await
        .expect("export succeeds");

        assert_eq!(report.link, "https://share.example/987");
        assert_eq!(report.vod_id, "987");
        assert_eq!(report.duration_seconds, FRESH_DURATION_SECONDS);
        // Groesse wird vor dem Loeschen der lokalen Datei gemessen (MockRunner
        // schreibt b"vod"); nach dem Upload ist sie sonst nicht mehr ermittelbar.
        assert_eq!(report.size_bytes, 3);
        assert!(!temp_dir.join("987.mp4").exists());
        let calls = runner.calls.lock().expect("call lock");
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "/opt/yt-dlp");
        assert!(!calls[0].1.iter().any(|arg| arg == "--download-sections"));
        assert_eq!(
            calls[1].1,
            rclone_copy_args(
                &temp_dir.join("987.mp4"),
                DEFAULT_REMOTE_BASE,
                "987",
                FRESH_CREATED_AT_UNIX
            )
        );
        assert_eq!(
            calls[2].1,
            rclone_link_args(DEFAULT_REMOTE_BASE, "987", FRESH_CREATED_AT_UNIX)
        );
        // Der Pfad kommt aus dem Streamdatum, nicht aus einem Kanalordner.
        assert!(
            calls[1].1[2].ends_with("/2024-01-01/987.mp4"),
            "Datumsordner fehlt: {}",
            calls[1].1[2]
        );

        std::fs::remove_dir_all(temp_dir).expect("temp cleanup");
    }

    #[tokio::test]
    async fn fehlgeschlagener_download_loescht_part_datei() {
        let runner = MockRunner {
            fail_download: true,
            ..MockRunner::default()
        };
        let temp_dir =
            std::env::temp_dir().join(format!("tb-vod-export-failure-test-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).expect("temp dir");

        let targets = ExportTargets {
            yt_dlp_path: Path::new("/opt/yt-dlp"),
            rclone_path: Path::new("rclone"),
            remote_base: "gdrive:Deadlock/Twitch-VODs",
            temp_dir: &temp_dir,
        };
        let result = export_latest_vod(
            &MockApi::default(),
            &runner,
            &targets,
            "channel-1",
            FRESH_VOD_END_UNIX + 100,
        )
        .await;

        assert!(matches!(result, Err(VodExportError::CommandFailed { .. })));
        assert!(!temp_dir.join("987.mp4.part").exists());
        std::fs::remove_dir_all(temp_dir).expect("temp cleanup");
    }

    #[tokio::test]
    async fn frueheres_vod_desselben_tages_liefert_kein_neues_vod() {
        let runner = MockRunner::default();
        let temp_dir = std::env::temp_dir().join(format!(
            "tb-vod-export-earlier-same-day-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_dir).expect("temp dir");

        let targets = ExportTargets {
            yt_dlp_path: Path::new("/opt/yt-dlp"),
            rclone_path: Path::new("rclone"),
            remote_base: "gdrive:Deadlock/Twitch-VODs",
            temp_dir: &temp_dir,
        };
        // stream.offline liegt 3h nach dem VOD-Ende — z. B. ein zweiter Stream
        // desselben Tages ist gerade offline gegangen, aber sein VOD ist in
        // der API noch nicht sichtbar; das ältere VOD (< 24h alt) darf hier
        // NICHT als "der gerade beendete Stream" durchgehen.
        let result = export_latest_vod(
            &MockApi::default(),
            &runner,
            &targets,
            "channel-1",
            FRESH_VOD_END_UNIX + 3 * 60 * 60,
        )
        .await;

        assert!(matches!(result, Err(VodExportError::NoNewVod)));
        assert!(runner.calls.lock().expect("call lock").is_empty());
        std::fs::remove_dir_all(temp_dir).expect("temp cleanup");
    }

    #[tokio::test]
    async fn fehlendes_created_at_liefert_kein_neues_vod() {
        let runner = MockRunner::default();
        let temp_dir = std::env::temp_dir().join(format!(
            "tb-vod-export-missing-created-at-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_dir).expect("temp dir");

        let targets = ExportTargets {
            yt_dlp_path: Path::new("/opt/yt-dlp"),
            rclone_path: Path::new("rclone"),
            remote_base: "gdrive:Deadlock/Twitch-VODs",
            temp_dir: &temp_dir,
        };
        let result = export_latest_vod(
            &MockApi { created_at: "", duration: FRESH_DURATION },
            &runner,
            &targets,
            "channel-1",
            FRESH_CREATED_AT_UNIX,
        )
        .await;

        assert!(matches!(result, Err(VodExportError::NoNewVod)));
        std::fs::remove_dir_all(temp_dir).expect("temp cleanup");
    }

    #[tokio::test]
    async fn fehlende_duration_liefert_kein_neues_vod() {
        let runner = MockRunner::default();
        let temp_dir = std::env::temp_dir().join(format!(
            "tb-vod-export-missing-duration-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_dir).expect("temp dir");

        let targets = ExportTargets {
            yt_dlp_path: Path::new("/opt/yt-dlp"),
            rclone_path: Path::new("rclone"),
            remote_base: "gdrive:Deadlock/Twitch-VODs",
            temp_dir: &temp_dir,
        };
        let result = export_latest_vod(
            &MockApi { created_at: FRESH_CREATED_AT, duration: "" },
            &runner,
            &targets,
            "channel-1",
            FRESH_CREATED_AT_UNIX,
        )
        .await;

        assert!(matches!(result, Err(VodExportError::NoNewVod)));
        std::fs::remove_dir_all(temp_dir).expect("temp cleanup");
    }
}

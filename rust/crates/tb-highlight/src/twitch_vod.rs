//! Twitch-VOD-Auswahl + Clip-Extraktion via yt-dlp und ffmpeg.
//!
//! Port von `bot/highlight_clipper/twitch_vod.py`. Die API-gekoppelten Fetches
//! (`get_channel_id`, `get_archive_videos`) liegen im Worker-Slice; hier die
//! reine VOD-Auswahl ([`select_vod_for_match`]), die Parsing-Helfer und die
//! Clip-Pipeline ([`download_clip`], yt-dlp Download-Section → ffmpeg-Reencode →
//! Größencheck). Subprocess-Fehler → `false` (Python-Parität).

use std::path::{Path, PathBuf};

use chrono::DateTime;
use regex::Regex;

use crate::config::{FFMPEG_PATH, MAX_DISCORD_FILE_MB};

/// Treffer der VOD-Suche: VOD-ID + Startzeitpunkt (Unix-Sekunden).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VodMatch {
    pub vod_id: String,
    pub vod_started_at: i64,
}

/// Wählt das erste Archiv-VOD, das das Match-Zeitfenster vollständig abdeckt.
/// Reiner Port der Schleife aus `find_vod_for_match` (ohne den API-Fetch).
pub fn select_vod_for_match(
    vods: &[serde_json::Value],
    match_start_unix: i64,
    match_duration_s: i64,
) -> Option<VodMatch> {
    for vod in vods {
        let created_at = vod.get("created_at").and_then(serde_json::Value::as_str).unwrap_or("");
        let duration = vod.get("duration").and_then(serde_json::Value::as_str).unwrap_or("");
        let Some(started_at) = parse_twitch_datetime(created_at) else {
            continue;
        };
        let duration_s = parse_duration_seconds(duration);
        if duration_s <= 0 {
            continue;
        }
        if started_at <= match_start_unix
            && started_at + duration_s >= match_start_unix + match_duration_s
        {
            let vod_id = vod
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            return Some(VodMatch { vod_id, vod_started_at: started_at });
        }
    }
    None
}

/// Lädt den Clip-Abschnitt des VODs (yt-dlp) und reencodet ihn (ffmpeg, 720p,
/// libx264 crf28). `true` nur, wenn die finale Datei existiert und unter
/// `MAX_DISCORD_FILE_MB` bleibt. yt-dlp- und ffmpeg-Pfad sind injizierbar.
pub async fn download_clip(
    yt_dlp_path: &Path,
    vod_id: &str,
    clip_start_s: i64,
    clip_end_s: i64,
    output_path: &Path,
) -> bool {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let clip_start_s = clip_start_s.max(0);
    let clip_end_s = clip_end_s.max(clip_start_s + 1);

    let stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("clip");
    let raw_prefix_name = format!("{stem}.raw");
    let raw_template = parent.join(format!("{raw_prefix_name}.%(ext)s"));

    cleanup_paths(&glob_prefix(parent, &raw_prefix_name));
    let _ = std::fs::remove_file(output_path);

    let yt_cmd = build_yt_dlp_cmd(
        yt_dlp_path,
        vod_id,
        clip_start_s,
        clip_end_s,
        &raw_template.to_string_lossy(),
    );
    if !run_process(&yt_cmd).await {
        return false;
    }

    let mut raw_candidates = glob_prefix(parent, &raw_prefix_name);
    raw_candidates.sort();
    if raw_candidates.is_empty() {
        return false;
    }
    let raw_path = match pick_downloaded_video(&raw_candidates) {
        Some(p) => p,
        None => {
            cleanup_paths(&raw_candidates);
            return false;
        }
    };

    let compressed_path = parent.join(format!("{stem}.compressed.mp4"));
    let _ = std::fs::remove_file(&compressed_path);

    let ff_cmd = build_ffmpeg_cmd(&raw_path, &compressed_path);
    if !run_process(&ff_cmd).await {
        cleanup_paths(&raw_candidates);
        return false;
    }

    cleanup_paths(&raw_candidates);
    if !compressed_path.exists() {
        return false;
    }
    let _ = std::fs::remove_file(output_path);
    if std::fs::rename(&compressed_path, output_path).is_err() {
        return false;
    }

    let max_bytes = (MAX_DISCORD_FILE_MB * 1024 * 1024) as u64;
    match std::fs::metadata(output_path) {
        Ok(m) if m.len() < max_bytes => true,
        _ => {
            let _ = std::fs::remove_file(output_path);
            false
        }
    }
}

/// Baut das yt-dlp-Kommando für den Download-Section-Schnitt.
fn build_yt_dlp_cmd(
    yt_dlp_path: &Path,
    vod_id: &str,
    clip_start_s: i64,
    clip_end_s: i64,
    raw_template: &str,
) -> Vec<String> {
    vec![
        yt_dlp_path.to_string_lossy().into_owned(),
        "--ffmpeg-location".to_string(),
        FFMPEG_PATH.to_string(),
        "--download-sections".to_string(),
        format!(
            "*{}-{}",
            format_hhmmss(clip_start_s),
            format_hhmmss(clip_end_s)
        ),
        "-o".to_string(),
        raw_template.to_string(),
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        "-f".to_string(),
        "bestvideo[height<=720]+bestaudio/bestvideo+bestaudio/best".to_string(),
        format!("https://www.twitch.tv/videos/{vod_id}"),
    ]
}

/// Baut das ffmpeg-Reencode-Kommando (720p, libx264 crf28, aac 96k).
fn build_ffmpeg_cmd(raw_path: &Path, compressed_path: &Path) -> Vec<String> {
    vec![
        FFMPEG_PATH.to_string(),
        "-y".to_string(),
        "-i".to_string(),
        raw_path.to_string_lossy().into_owned(),
        "-vf".to_string(),
        "scale=-2:720".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-crf".to_string(),
        "28".to_string(),
        "-preset".to_string(),
        "fast".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "96k".to_string(),
        compressed_path.to_string_lossy().into_owned(),
    ]
}

/// Führt ein Kommando aus (stdout verworfen, stderr geloggt bei Fehler).
async fn run_process(cmd: &[String]) -> bool {
    let Some((program, args)) = cmd.split_first() else {
        return false;
    };
    let output = tokio::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            tracing::warn!(cmd = %cmd.join(" "), "HighlightClipper subprocess failed");
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.trim().is_empty() {
                tracing::warn!("{}", stderr.trim());
            }
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, cmd = %cmd.join(" "), "HighlightClipper subprocess error");
            false
        }
    }
}

/// Alle Dateien `<parent>/<prefix>.*` (entspricht Pythons glob).
fn glob_prefix(parent: &Path, prefix_name: &str) -> Vec<PathBuf> {
    let needle = format!("{prefix_name}.");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&needle))
        })
        .map(|e| e.path())
        .collect()
}

fn cleanup_paths(paths: &[PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

/// Wählt die heruntergeladene Videodatei nach Endungs-Priorität.
fn pick_downloaded_video(paths: &[PathBuf]) -> Option<PathBuf> {
    for suffix in ["mp4", "mkv", "webm", "ts"] {
        for path in paths {
            if path.extension().and_then(|e| e.to_str()) == Some(suffix) {
                return Some(path.clone());
            }
        }
    }
    paths.first().cloned()
}

/// Parst einen Twitch-ISO-Zeitstempel zu Unix-Sekunden; leer/ungültig → None.
fn parse_twitch_datetime(value: &str) -> Option<i64> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(text).ok().map(|dt| dt.timestamp())
}

/// Parst eine Twitch-Dauer wie „1h2m3s" zu Sekunden; kein Match → 0.
fn parse_duration_seconds(value: &str) -> i64 {
    let text = value.trim();
    let re = Regex::new(r"^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?$").expect("static regex");
    let Some(caps) = re.captures(text) else {
        return 0;
    };
    let group = |i: usize| -> i64 {
        caps.get(i)
            .and_then(|m| m.as_str().parse::<i64>().ok())
            .unwrap_or(0)
    };
    group(1) * 3600 + group(2) * 60 + group(3)
}

/// Formatiert Sekunden als `HH:MM:SS` (zweistellig, Stunden ohne Cap).
fn format_hhmmss(total_seconds: i64) -> String {
    let total = total_seconds.max(0);
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_duration_varianten() {
        assert_eq!(parse_duration_seconds("1h2m3s"), 3723);
        assert_eq!(parse_duration_seconds("47m"), 2820);
        assert_eq!(parse_duration_seconds("30s"), 30);
        assert_eq!(parse_duration_seconds("2h"), 7200);
        assert_eq!(parse_duration_seconds(""), 0); // all-optional → leer matcht
        assert_eq!(parse_duration_seconds("garbage"), 0); // fullmatch schlägt fehl
    }

    #[test]
    fn parse_datetime_iso_und_leer() {
        // 2021-05-01T00:00:00Z = 1619827200
        assert_eq!(parse_twitch_datetime("2021-05-01T00:00:00Z"), Some(1619827200));
        assert_eq!(parse_twitch_datetime("  "), None);
        assert_eq!(parse_twitch_datetime("kein-datum"), None);
    }

    #[test]
    fn format_hhmmss_padding() {
        assert_eq!(format_hhmmss(3661), "01:01:01");
        assert_eq!(format_hhmmss(0), "00:00:00");
        assert_eq!(format_hhmmss(90061), "25:01:01"); // Stunden ohne Cap
    }

    #[test]
    fn select_vod_deckt_match_ab() {
        let vods = vec![
            json!({"id": "111", "created_at": "2021-05-01T00:00:00Z", "duration": "10m"}),
            // dieses deckt das Fenster ab: Start 1000s vor Match, 2h Dauer
            json!({"id": "222", "created_at": "2021-05-01T00:00:00Z", "duration": "2h"}),
        ];
        // Match startet 1h nach VOD-Start, dauert 10min.
        let m = select_vod_for_match(&vods, 1619827200 + 3600, 600);
        assert_eq!(m, Some(VodMatch { vod_id: "222".into(), vod_started_at: 1619827200 }));
    }

    #[test]
    fn select_vod_keine_abdeckung() {
        let vods = vec![json!({"id": "1", "created_at": "2021-05-01T00:00:00Z", "duration": "5m"})];
        // Match weit nach VOD-Ende.
        assert_eq!(select_vod_for_match(&vods, 1619827200 + 100000, 600), None);
    }

    #[test]
    fn pick_video_endungs_prioritaet() {
        let paths = vec![
            PathBuf::from("/x/clip.raw.webm"),
            PathBuf::from("/x/clip.raw.mp4"),
            PathBuf::from("/x/clip.raw.ts"),
        ];
        assert_eq!(pick_downloaded_video(&paths), Some(PathBuf::from("/x/clip.raw.mp4")));
        // Ohne bevorzugte Endung → erste Datei.
        let other = vec![PathBuf::from("/x/clip.raw.flv")];
        assert_eq!(pick_downloaded_video(&other), Some(PathBuf::from("/x/clip.raw.flv")));
        assert_eq!(pick_downloaded_video(&[]), None);
    }

    #[test]
    fn yt_dlp_cmd_enthaelt_section_und_url() {
        let cmd = build_yt_dlp_cmd(
            Path::new("/venv/bin/yt-dlp"),
            "98765",
            65,
            125,
            "/clips/c.raw.%(ext)s",
        );
        assert_eq!(cmd[0], "/venv/bin/yt-dlp");
        assert!(cmd.contains(&"*00:01:05-00:02:05".to_string()));
        assert!(cmd.contains(&"https://www.twitch.tv/videos/98765".to_string()));
        assert!(cmd.contains(&"/clips/c.raw.%(ext)s".to_string()));
    }

    #[test]
    fn ffmpeg_cmd_reencode_args() {
        let cmd = build_ffmpeg_cmd(Path::new("/clips/c.raw.mp4"), Path::new("/clips/c.compressed.mp4"));
        assert_eq!(cmd[0], FFMPEG_PATH);
        assert!(cmd.contains(&"scale=-2:720".to_string()));
        assert!(cmd.contains(&"libx264".to_string()));
        assert!(cmd.contains(&"/clips/c.compressed.mp4".to_string()));
    }
}

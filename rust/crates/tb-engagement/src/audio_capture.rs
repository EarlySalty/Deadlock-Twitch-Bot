//! Audio-Capture eines kurzen Twitch-Stream-Ausschnitts via `streamlink`
//! (Port von `bot/community/voice_reaction/audio_capture.py`, soweit der
//! Engagement-Transkript-Loop es nutzt).
//!
//! `streamlink` zieht `duration` Sekunden in eine Temp-`.ts`-Datei; der Aufrufer
//! transkribiert sie und ruft danach [`CaptureResult::cleanup`].

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;

/// Default-streamlink-Quality (Python `DEFAULT_QUALITY`).
pub const DEFAULT_QUALITY: &str = "worst";
/// Präfix der Capture-Temp-Verzeichnisse (Python `CAPTURE_TMP_PREFIX`).
const CAPTURE_TMP_PREFIX: &str = "voice-reaction-";
/// < 32 KB → wahrscheinlich Connect-Failure (Python `_MIN_USEFUL_BYTES`).
const MIN_USEFUL_BYTES: u64 = 32 * 1024;

/// Erfolgreiches Capture-Ergebnis. Der Aufrufer muss [`Self::cleanup`] rufen.
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub media_path: PathBuf,
    pub workdir: PathBuf,
    pub quality: String,
    pub requested_duration_seconds: u64,
    pub actual_duration_seconds: f64,
    pub bytes: u64,
}

impl CaptureResult {
    /// Löscht das Capture-Verzeichnis (nur wenn es dem Präfix-Schema entspricht).
    pub async fn cleanup(&self) {
        cleanup_workdir(&self.workdir).await;
    }
}

/// Capture-Fehler (Python `AudioCaptureError`).
#[derive(Debug)]
pub struct CaptureError(pub String);

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Zieht Stream-Audio via streamlink.
pub struct AudioCapturer {
    streamlink_bin: String,
}

impl Default for AudioCapturer {
    fn default() -> Self {
        Self::from_env()
    }
}

impl AudioCapturer {
    /// `VOICE_REACTION_STREAMLINK_BIN` oder `streamlink`.
    pub fn from_env() -> Self {
        Self {
            streamlink_bin: std::env::var("VOICE_REACTION_STREAMLINK_BIN")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "streamlink".to_string()),
        }
    }

    /// Setzt das streamlink-Binary (Tests).
    pub fn with_bin(bin: impl Into<String>) -> Self {
        Self { streamlink_bin: bin.into() }
    }

    /// Lädt `duration_seconds` Sekunden Audio des Streams `login`. `workdir_root`
    /// = Eltern-Verzeichnis der Temp-Files (Default System-Temp).
    pub async fn capture(
        &self,
        login: &str,
        duration_seconds: u64,
        quality: &str,
        workdir_root: Option<&Path>,
    ) -> Result<CaptureResult, CaptureError> {
        let normalized = login.trim().to_lowercase();
        if normalized.is_empty() {
            return Err(CaptureError("login leer".to_string()));
        }
        if duration_seconds < 5 {
            return Err(CaptureError(format!("duration_seconds zu klein: {duration_seconds}")));
        }

        let workdir = make_workdir(workdir_root).await.map_err(CaptureError)?;
        let media_path = workdir.join("audio.ts");
        let target_url = format!("https://twitch.tv/{normalized}");
        let args: Vec<String> = vec![
            "--hls-duration".to_string(),
            format_hls_duration(duration_seconds),
            "--twitch-disable-ads".to_string(),
            "--quiet".to_string(),
            "-o".to_string(),
            media_path.to_string_lossy().to_string(),
            target_url,
            quality.to_string(),
        ];

        let started = Instant::now();
        let run = self.run_streamlink(&args, duration_seconds).await;
        let elapsed = started.elapsed().as_secs_f64();
        let (returncode, stderr) = match run {
            Ok(v) => v,
            Err(e) => {
                cleanup_workdir(&workdir).await;
                return Err(CaptureError(e));
            }
        };

        let size_bytes = match tokio::fs::metadata(&media_path).await {
            Ok(m) => m.len(),
            Err(_) => {
                cleanup_workdir(&workdir).await;
                let tail = truncate(&String::from_utf8_lossy(&stderr), 300);
                return Err(CaptureError(format!(
                    "streamlink lieferte keine Datei (rc={returncode}): {tail}"
                )));
            }
        };
        if size_bytes < MIN_USEFUL_BYTES {
            cleanup_workdir(&workdir).await;
            let tail = truncate(&String::from_utf8_lossy(&stderr), 300);
            return Err(CaptureError(format!(
                "Capture zu klein ({size_bytes} bytes, rc={returncode}): {tail}"
            )));
        }
        if returncode != 0 {
            // streamlink kappt teils mit non-zero exit, obwohl Daten gültig sind.
            tracing::debug!(rc = returncode, bytes = size_bytes, "streamlink rc!=0, Datei aber gültig — fahre fort");
        }

        Ok(CaptureResult {
            media_path,
            workdir,
            quality: quality.to_string(),
            requested_duration_seconds: duration_seconds,
            actual_duration_seconds: (elapsed * 100.0).round() / 100.0,
            bytes: size_bytes,
        })
    }

    /// Startet streamlink, kappt nach 1.5 × duration hart (Python `_run_streamlink`).
    /// `kill_on_drop` killt den Prozess beim Timeout-Drop.
    async fn run_streamlink(&self, args: &[String], duration_seconds: u64) -> Result<(i32, Vec<u8>), String> {
        let hard_timeout = Duration::from_secs((duration_seconds as f64 * 1.5) as u64 + 15).max(Duration::from_secs(30));
        let fut = Command::new(&self.streamlink_bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(hard_timeout, fut).await {
            Ok(Ok(out)) => Ok((out.status.code().unwrap_or(0), out.stderr)),
            Ok(Err(e)) => Err(format!("streamlink-Aufruf fehlgeschlagen: {e}")),
            Err(_) => {
                tracing::warn!(timeout = hard_timeout.as_secs(), "streamlink-Timeout — Prozess gekappt");
                Ok((124, b"timeout".to_vec()))
            }
        }
    }
}

/// `HH:MM:SS` aus Sekunden (Python `_format_hls_duration`).
fn format_hls_duration(seconds: u64) -> String {
    let seconds = seconds.max(1);
    let (minutes, sec) = (seconds / 60, seconds % 60);
    let (hours, minutes) = (minutes / 60, minutes % 60);
    format!("{hours:02}:{minutes:02}:{sec:02}")
}

/// Legt ein frisches Capture-Verzeichnis an: `<root>/voice-reaction-<hex>`.
async fn make_workdir(workdir_root: Option<&Path>) -> Result<PathBuf, String> {
    let root = workdir_root.map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    let workdir = root.join(format!("{CAPTURE_TMP_PREFIX}{}", tb_crypto::random_hex_token(6)));
    tokio::fs::create_dir_all(&workdir).await.map_err(|e| format!("workdir nicht anlegbar: {e}"))?;
    Ok(workdir)
}

/// Löscht ein Capture-Verzeichnis, wenn der Name dem Präfix entspricht.
async fn cleanup_workdir(workdir: &Path) {
    let matches_prefix = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with(CAPTURE_TMP_PREFIX))
        .unwrap_or(false);
    if matches_prefix {
        let _ = tokio::fs::remove_dir_all(workdir).await;
    }
}

/// Byte-sichere Kürzung.
fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hls_duration_format() {
        assert_eq!(format_hls_duration(75), "00:01:15");
        assert_eq!(format_hls_duration(3661), "01:01:01");
        assert_eq!(format_hls_duration(0), "00:00:01"); // min 1
    }

    /// Schreibt ein Fake-streamlink-Script, das `bytes` Nullbytes an die `-o`-Datei
    /// schreibt — testet den vollen capture-Pfad ohne echtes streamlink.
    async fn fake_streamlink(bytes: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fakesl-{}", tb_crypto::random_hex_token(6)));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let script = dir.join("streamlink.sh");
        let body = format!(
            "#!/bin/sh\nout=\"\"\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"-o\" ]; then shift; out=\"$1\"; fi\n  shift\ndone\nhead -c {bytes} /dev/zero > \"$out\"\n"
        );
        tokio::fs::write(&script, body).await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&script).await.unwrap().permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(&script, perms).await.unwrap();
        }
        script
    }

    #[tokio::test]
    async fn capture_erfolg_mit_fake_streamlink() {
        let script = fake_streamlink(40 * 1024).await; // > 32 KB
        let cap = AudioCapturer::with_bin(script.to_string_lossy().to_string());
        let root = std::env::temp_dir().join(format!("captest-{}", tb_crypto::random_hex_token(6)));
        let result = cap.capture("Nani", 10, "worst", Some(&root)).await.unwrap();
        assert!(result.media_path.exists());
        assert_eq!(result.bytes, 40 * 1024);
        assert_eq!(result.requested_duration_seconds, 10);
        // workdir trägt das Präfix.
        assert!(result.workdir.file_name().unwrap().to_str().unwrap().starts_with("voice-reaction-"));
        // cleanup entfernt das Verzeichnis.
        result.cleanup().await;
        assert!(!result.workdir.exists());
        let _ = tokio::fs::remove_dir_all(script.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn capture_zu_klein_ist_fehler() {
        let script = fake_streamlink(100).await; // < 32 KB → Fehler
        let cap = AudioCapturer::with_bin(script.to_string_lossy().to_string());
        let root = std::env::temp_dir().join(format!("captest-{}", tb_crypto::random_hex_token(6)));
        let err = cap.capture("nani", 10, "worst", Some(&root)).await.unwrap_err();
        assert!(err.0.contains("zu klein"));
        let _ = tokio::fs::remove_dir_all(&root).await;
        let _ = tokio::fs::remove_dir_all(script.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn capture_leerer_login_ist_fehler() {
        let cap = AudioCapturer::with_bin("/bin/true");
        assert!(cap.capture("  ", 10, "worst", None).await.is_err());
        assert!(cap.capture("nani", 3, "worst", None).await.is_err()); // duration < 5
    }
}

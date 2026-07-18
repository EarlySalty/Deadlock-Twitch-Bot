//! Audio-Capture eines kurzen Twitch-Stream-Ausschnitts via `streamlink`
//! (Port von `bot/community/voice_reaction/audio_capture.py`, soweit der
//! Engagement-Transkript-Loop es nutzt).
//!
//! `streamlink` zieht `duration` Sekunden in eine Temp-`.ts`-Datei; der Aufrufer
//! transkribiert sie und ruft danach [`CaptureResult::cleanup`].

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;
use tokio::process::Command;

/// Default-streamlink-Quality (Python `DEFAULT_QUALITY`).
pub const DEFAULT_QUALITY: &str = "worst";
/// Präfix der Capture-Temp-Verzeichnisse (Python `CAPTURE_TMP_PREFIX`).
const CAPTURE_TMP_PREFIX: &str = "voice-reaction-";
/// < 32 KB → wahrscheinlich Connect-Failure (Python `_MIN_USEFUL_BYTES`).
const MIN_USEFUL_BYTES: u64 = 32 * 1024;
const SOURCE_STDOUT_LIMIT: usize = 16 * 1024;
const AUDIO_STDOUT_LIMIT: usize = 2 * 1024 * 1024;
const STDERR_BUFFER_BYTES: usize = 8 * 1024;
const SOURCE_TIMEOUT: Duration = Duration::from_secs(15);
const FFMPEG_TIMEOUT_GRACE: Duration = Duration::from_secs(15);

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

/// Klassifizierter Capture-Fehler ohne Prozessausgabe oder Quell-URL.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CaptureError {
    #[error("invalid_input")]
    InvalidInput,
    #[error("source_start")]
    SourceStart,
    #[error("source_timeout")]
    SourceTimeout,
    #[error("source_unavailable")]
    SourceUnavailable,
    #[error("ffmpeg_start")]
    FfmpegStart,
    #[error("ffmpeg_timeout")]
    FfmpegTimeout,
    #[error("ffmpeg_failed")]
    FfmpegFailed,
    #[error("audio_empty")]
    AudioEmpty,
    #[error("audio_too_large")]
    AudioTooLarge,
    #[error("{0}")]
    Legacy(String),
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
        Self {
            streamlink_bin: bin.into(),
        }
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
            return Err(CaptureError::Legacy("login leer".to_string()));
        }
        if duration_seconds < 5 {
            return Err(CaptureError::Legacy(format!(
                "duration_seconds zu klein: {duration_seconds}"
            )));
        }

        let workdir = make_workdir(workdir_root)
            .await
            .map_err(CaptureError::Legacy)?;
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
                return Err(CaptureError::Legacy(e));
            }
        };

        let size_bytes = match tokio::fs::metadata(&media_path).await {
            Ok(m) => m.len(),
            Err(_) => {
                cleanup_workdir(&workdir).await;
                let tail = truncate(&String::from_utf8_lossy(&stderr), 300);
                return Err(CaptureError::Legacy(format!(
                    "streamlink lieferte keine Datei (rc={returncode}): {tail}"
                )));
            }
        };
        if size_bytes < MIN_USEFUL_BYTES {
            cleanup_workdir(&workdir).await;
            let tail = truncate(&String::from_utf8_lossy(&stderr), 300);
            return Err(CaptureError::Legacy(format!(
                "Capture zu klein ({size_bytes} bytes, rc={returncode}): {tail}"
            )));
        }
        if returncode != 0 {
            // streamlink kappt teils mit non-zero exit, obwohl Daten gültig sind.
            tracing::debug!(
                rc = returncode,
                bytes = size_bytes,
                "streamlink rc!=0, Datei aber gültig — fahre fort"
            );
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
    async fn run_streamlink(
        &self,
        args: &[String],
        duration_seconds: u64,
    ) -> Result<(i32, Vec<u8>), String> {
        let hard_timeout = Duration::from_secs((duration_seconds as f64 * 1.5) as u64 + 15)
            .max(Duration::from_secs(30));
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
                tracing::warn!(
                    timeout = hard_timeout.as_secs(),
                    "streamlink-Timeout — Prozess gekappt"
                );
                Ok((124, b"timeout".to_vec()))
            }
        }
    }
}

/// Captured ein kurzes PCM-WAV ausschließlich über Prozess-Pipes in den RAM.
pub struct MemoryAudioCapturer {
    ytdlp_bin: String,
    ffmpeg_bin: String,
    #[cfg(test)]
    source_timeout: Duration,
    #[cfg(test)]
    ffmpeg_timeout_grace: Duration,
}

impl MemoryAudioCapturer {
    pub fn new(ytdlp_bin: impl Into<String>, ffmpeg_bin: impl Into<String>) -> Self {
        Self {
            ytdlp_bin: ytdlp_bin.into(),
            ffmpeg_bin: ffmpeg_bin.into(),
            #[cfg(test)]
            source_timeout: SOURCE_TIMEOUT,
            #[cfg(test)]
            ffmpeg_timeout_grace: FFMPEG_TIMEOUT_GRACE,
        }
    }

    #[cfg(test)]
    fn with_timeouts(
        ytdlp_bin: impl Into<String>,
        ffmpeg_bin: impl Into<String>,
        source_timeout: Duration,
        ffmpeg_timeout_grace: Duration,
    ) -> Self {
        Self {
            ytdlp_bin: ytdlp_bin.into(),
            ffmpeg_bin: ffmpeg_bin.into(),
            source_timeout,
            ffmpeg_timeout_grace,
        }
    }

    /// Löst die öffentliche Stream-URL und gibt das ffmpeg-WAV aus stdout zurück.
    pub async fn capture_wav(
        &self,
        channel_login: &str,
        duration: Duration,
    ) -> Result<Vec<u8>, CaptureError> {
        let login = channel_login.trim().to_ascii_lowercase();
        if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(CaptureError::InvalidInput);
        }

        let target = format!("https://twitch.tv/{login}");
        let mut source = Command::new(&self.ytdlp_bin);
        source
            .args(["--get-url", "--no-playlist", &target])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let source = source.spawn().map_err(|_| CaptureError::SourceStart)?;
        let source = collect_child(source, SOURCE_STDOUT_LIMIT, self.source_timeout())
            .await
            .map_err(|error| match error {
                ProcessError::Timeout => CaptureError::SourceTimeout,
                ProcessError::TooLarge | ProcessError::Io => CaptureError::SourceUnavailable,
            })?;
        if !source.status.success() {
            return Err(CaptureError::SourceUnavailable);
        }
        let source_url = validate_source_url(source.stdout)?;

        let duration_arg = duration.as_secs().to_string();
        let ffmpeg_timeout = duration
            .checked_add(self.ffmpeg_timeout_grace())
            .ok_or(CaptureError::InvalidInput)?;
        let mut ffmpeg = Command::new(&self.ffmpeg_bin);
        ffmpeg
            .args(["-t", &duration_arg, "-i", &source_url])
            .args([
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let ffmpeg = ffmpeg.spawn().map_err(|_| CaptureError::FfmpegStart)?;
        let output = collect_child(ffmpeg, AUDIO_STDOUT_LIMIT, ffmpeg_timeout)
            .await
            .map_err(|error| match error {
                ProcessError::Timeout => CaptureError::FfmpegTimeout,
                ProcessError::TooLarge => CaptureError::AudioTooLarge,
                ProcessError::Io => CaptureError::FfmpegFailed,
            })?;
        if !output.status.success() {
            return Err(CaptureError::FfmpegFailed);
        }
        if output.stdout.is_empty() {
            return Err(CaptureError::AudioEmpty);
        }
        Ok(output.stdout)
    }

    fn source_timeout(&self) -> Duration {
        #[cfg(test)]
        {
            self.source_timeout
        }
        #[cfg(not(test))]
        {
            SOURCE_TIMEOUT
        }
    }

    fn ffmpeg_timeout_grace(&self) -> Duration {
        #[cfg(test)]
        {
            self.ffmpeg_timeout_grace
        }
        #[cfg(not(test))]
        {
            FFMPEG_TIMEOUT_GRACE
        }
    }
}

struct ProcessOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
}

#[derive(Clone, Copy)]
enum ProcessError {
    Timeout,
    TooLarge,
    Io,
}

async fn collect_child(
    mut child: Child,
    stdout_limit: usize,
    timeout: Duration,
) -> Result<ProcessOutput, ProcessError> {
    let Some(stdout) = child.stdout.take() else {
        kill_and_wait(&mut child).await;
        return Err(ProcessError::Io);
    };
    let Some(stderr) = child.stderr.take() else {
        kill_and_wait(&mut child).await;
        return Err(ProcessError::Io);
    };
    let outcome = tokio::time::timeout(timeout, async {
        let wait = async { child.wait().await.map_err(|_| ProcessError::Io) };
        let (status, stdout, ()) =
            tokio::try_join!(wait, read_capped(stdout, stdout_limit), drain(stderr))?;
        Ok(ProcessOutput { status, stdout })
    })
    .await;

    match outcome {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => {
            kill_and_wait(&mut child).await;
            Err(error)
        }
        Err(_) => {
            kill_and_wait(&mut child).await;
            Err(ProcessError::Timeout)
        }
    }
}

async fn read_capped(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, ProcessError> {
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let remaining = limit + 1 - output.len();
        let chunk_len = remaining.min(buffer.len());
        let read = reader
            .read(&mut buffer[..chunk_len])
            .await
            .map_err(|_| ProcessError::Io)?;
        if read == 0 {
            return Ok(output);
        }
        output.extend_from_slice(&buffer[..read]);
        if output.len() > limit {
            return Err(ProcessError::TooLarge);
        }
    }
}

async fn drain(mut reader: impl AsyncRead + Unpin) -> Result<(), ProcessError> {
    let mut buffer = [0_u8; STDERR_BUFFER_BYTES];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(_) => return Err(ProcessError::Io),
        }
    }
}

async fn kill_and_wait(child: &mut Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

fn validate_source_url(stdout: Vec<u8>) -> Result<String, CaptureError> {
    let output = String::from_utf8(stdout).map_err(|_| CaptureError::SourceUnavailable)?;
    let mut lines = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let url = lines.next().ok_or(CaptureError::SourceUnavailable)?;
    let parsed = reqwest::Url::parse(url).ok();
    let valid_http_url = parsed
        .as_ref()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some());
    if lines.next().is_some() || !valid_http_url || url.chars().any(char::is_whitespace) {
        return Err(CaptureError::SourceUnavailable);
    }
    Ok(url.to_string())
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
    let root = workdir_root
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let workdir = root.join(format!(
        "{CAPTURE_TMP_PREFIX}{}",
        tb_crypto::random_hex_token(6)
    ));
    tokio::fs::create_dir_all(&workdir)
        .await
        .map_err(|e| format!("workdir nicht anlegbar: {e}"))?;
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
    use std::collections::BTreeSet;

    #[cfg(unix)]
    async fn fake_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = dir.join(name);
        tokio::fs::write(&script, format!("#!/bin/sh\nset -eu\n{body}\n"))
            .await
            .unwrap();
        let mut permissions = tokio::fs::metadata(&script).await.unwrap().permissions();
        permissions.set_mode(0o755);
        tokio::fs::set_permissions(&script, permissions)
            .await
            .unwrap();
        script
    }

    #[cfg(unix)]
    async fn fixture_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("memory-audio-{}", tb_crypto::random_hex_token(6)));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        dir
    }

    #[cfg(unix)]
    fn names_in(dir: &Path) -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[cfg(target_os = "linux")]
    async fn assert_process_reaped(pid_file: &Path) {
        let pid = tokio::fs::read_to_string(pid_file).await.unwrap();
        let proc_path = PathBuf::from(format!("/proc/{}", pid.trim()));
        for _ in 0..20 {
            if !proc_path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!proc_path.exists(), "Kindprozess wurde nicht gewartet");
    }

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
        assert!(result
            .workdir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("voice-reaction-"));
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
        let err = cap
            .capture("nani", 10, "worst", Some(&root))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("zu klein"));
        let _ = tokio::fs::remove_dir_all(&root).await;
        let _ = tokio::fs::remove_dir_all(script.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn capture_leerer_login_ist_fehler() {
        let cap = AudioCapturer::with_bin("/bin/true");
        assert!(cap.capture("  ", 10, "worst", None).await.is_err());
        assert!(cap.capture("nani", 3, "worst", None).await.is_err()); // duration < 5
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_capture_nutzt_ytdlp_url_und_ffmpeg_stdout() {
        let dir = fixture_dir().await;
        let ytdlp_args = dir.join("ytdlp.args");
        let ffmpeg_args = dir.join("ffmpeg.args");
        let ytdlp = fake_script(
            &dir,
            "yt-dlp",
            &format!(
                "printf '%s\\n' \"$@\" > '{}'; printf '%s\\n' 'https://example.invalid/live.m3u8'",
                ytdlp_args.display()
            ),
        )
        .await;
        let ffmpeg = fake_script(
            &dir,
            "ffmpeg",
            &format!(
                "printf '%s\\n' \"$@\" > '{}'; printf 'RIFF....WAVE'",
                ffmpeg_args.display()
            ),
        )
        .await;

        let capturer = MemoryAudioCapturer::new(ytdlp.to_string_lossy(), ffmpeg.to_string_lossy());
        let wav = capturer
            .capture_wav(" Nani ", Duration::from_secs(20))
            .await
            .unwrap();

        assert_eq!(wav, b"RIFF....WAVE");
        assert_eq!(
            tokio::fs::read_to_string(ytdlp_args).await.unwrap(),
            "--get-url\n--no-playlist\nhttps://twitch.tv/nani\n"
        );
        let args = tokio::fs::read_to_string(ffmpeg_args).await.unwrap();
        assert_eq!(
            args,
            "-t\n20\n-i\nhttps://example.invalid/live.m3u8\n-vn\n-ac\n1\n-ar\n16000\n-c:a\npcm_s16le\n-f\nwav\npipe:1\n"
        );
        assert!(!args.lines().any(|arg| arg == "-o"));
        assert!(!args
            .lines()
            .any(|arg| arg.ends_with(".wav") || arg.ends_with(".ts")));
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_capture_schreibt_keine_audio_datei() {
        let dir = fixture_dir().await;
        let ytdlp = fake_script(
            &dir,
            "yt-dlp",
            "printf '%s\\n' 'https://example.invalid/live.m3u8'",
        )
        .await;
        let ffmpeg = fake_script(&dir, "ffmpeg", "printf 'RIFF....WAVE'").await;
        let before = names_in(&dir);

        let capturer = MemoryAudioCapturer::new(ytdlp.to_string_lossy(), ffmpeg.to_string_lossy());
        capturer
            .capture_wav("nani", Duration::from_secs(20))
            .await
            .unwrap();

        assert_eq!(names_in(&dir), before);
        assert!(!names_in(&dir).iter().any(|name| {
            [".wav", ".ts", ".mp3", ".m4a", ".ogg", ".webm"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        }));
        let source = include_str!("audio_capture.rs");
        let memory_path = source
            .split("pub struct MemoryAudioCapturer")
            .nth(1)
            .unwrap()
            .split("fn format_hls_duration")
            .next()
            .unwrap();
        for forbidden in [
            "temp_dir",
            "create_dir",
            "tokio::fs::write",
            "NamedTempFile",
            ".arg(\"-o\")",
        ] {
            assert!(
                !memory_path.contains(forbidden),
                "RAM-Pfad enthält verbotene Dateiausgabe"
            );
        }
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn memory_capture_begrenzt_ffmpeg_stdout_und_reapt_kind() {
        let dir = fixture_dir().await;
        let pid_file = dir.join("ffmpeg.pid");
        let ytdlp = fake_script(
            &dir,
            "yt-dlp",
            "printf '%s\\n' 'https://example.invalid/live.m3u8'",
        )
        .await;
        let ffmpeg = fake_script(
            &dir,
            "ffmpeg",
            &format!(
                "printf '%s' \"$$\" > '{}'; head -c 2097153 /dev/zero; exec tail -f /dev/null",
                pid_file.display()
            ),
        )
        .await;
        let capturer = MemoryAudioCapturer::new(ytdlp.to_string_lossy(), ffmpeg.to_string_lossy());

        let err = capturer
            .capture_wav("nani", Duration::from_secs(20))
            .await
            .unwrap_err();

        assert_eq!(err, CaptureError::AudioTooLarge);
        assert_process_reaped(&pid_file).await;
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn memory_capture_timeouts_killen_und_reapen_kinder() {
        let source_dir = fixture_dir().await;
        let source_pid = source_dir.join("source.pid");
        let ytdlp = fake_script(
            &source_dir,
            "yt-dlp",
            &format!(
                "printf '%s' \"$$\" > '{}'; exec tail -f /dev/null",
                source_pid.display()
            ),
        )
        .await;
        let ffmpeg = fake_script(&source_dir, "ffmpeg", "printf 'RIFF....WAVE'").await;
        let capturer = MemoryAudioCapturer::with_timeouts(
            ytdlp.to_string_lossy(),
            ffmpeg.to_string_lossy(),
            Duration::from_millis(50),
            Duration::from_millis(50),
        );
        let err = capturer
            .capture_wav("nani", Duration::ZERO)
            .await
            .unwrap_err();
        assert_eq!(err, CaptureError::SourceTimeout);
        assert_process_reaped(&source_pid).await;

        let ffmpeg_dir = fixture_dir().await;
        let ffmpeg_pid = ffmpeg_dir.join("ffmpeg.pid");
        let ytdlp = fake_script(
            &ffmpeg_dir,
            "yt-dlp",
            "printf '%s\\n' 'https://example.invalid/live.m3u8'",
        )
        .await;
        let ffmpeg = fake_script(
            &ffmpeg_dir,
            "ffmpeg",
            &format!(
                "printf '%s' \"$$\" > '{}'; exec tail -f /dev/null",
                ffmpeg_pid.display()
            ),
        )
        .await;
        let capturer = MemoryAudioCapturer::with_timeouts(
            ytdlp.to_string_lossy(),
            ffmpeg.to_string_lossy(),
            Duration::from_millis(50),
            Duration::from_millis(50),
        );
        let err = capturer
            .capture_wav("nani", Duration::ZERO)
            .await
            .unwrap_err();
        assert_eq!(err, CaptureError::FfmpegTimeout);
        assert_process_reaped(&ffmpeg_pid).await;

        let _ = tokio::fs::remove_dir_all(source_dir).await;
        let _ = tokio::fs::remove_dir_all(ffmpeg_dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_capture_leere_mehrzeilige_ungueltige_oder_zu_grosse_quelle_ist_fehler() {
        let dir = fixture_dir().await;
        let ffmpeg = fake_script(&dir, "ffmpeg", "exit 99").await;
        for (index, output) in [
            "",
            "https://example.invalid/one\\nhttps://example.invalid/two\\n",
            "file:///tmp/not-allowed\\n",
            "https://\\n",
        ]
        .into_iter()
        .enumerate()
        {
            let ytdlp = fake_script(
                &dir,
                &format!("yt-dlp-{index}"),
                &format!("printf '{output}'"),
            )
            .await;
            let capturer =
                MemoryAudioCapturer::new(ytdlp.to_string_lossy(), ffmpeg.to_string_lossy());
            assert_eq!(
                capturer
                    .capture_wav("nani", Duration::from_secs(20))
                    .await
                    .unwrap_err(),
                CaptureError::SourceUnavailable
            );
        }
        let ytdlp = fake_script(&dir, "yt-dlp-large", "head -c 16385 /dev/zero").await;
        let capturer = MemoryAudioCapturer::new(ytdlp.to_string_lossy(), ffmpeg.to_string_lossy());
        assert_eq!(
            capturer
                .capture_wav("nani", Duration::from_secs(20))
                .await
                .unwrap_err(),
            CaptureError::SourceUnavailable
        );
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_capture_leeres_audio_und_fehlerstatus_sind_sanitized() {
        let dir = fixture_dir().await;
        let ytdlp = fake_script(
            &dir,
            "yt-dlp",
            "printf '%s\\n' 'https://example.invalid/private?token=secret'",
        )
        .await;
        let ffmpeg_empty = fake_script(&dir, "ffmpeg-empty", "exit 0").await;
        let capturer =
            MemoryAudioCapturer::new(ytdlp.to_string_lossy(), ffmpeg_empty.to_string_lossy());
        assert_eq!(
            capturer
                .capture_wav("nani", Duration::from_secs(20))
                .await
                .unwrap_err(),
            CaptureError::AudioEmpty
        );

        let ffmpeg_failed = fake_script(
            &dir,
            "ffmpeg-failed",
            "printf '%s' 'private?token=secret' >&2; exit 2",
        )
        .await;
        let capturer =
            MemoryAudioCapturer::new(ytdlp.to_string_lossy(), ffmpeg_failed.to_string_lossy());
        let err = capturer
            .capture_wav("nani", Duration::from_secs(20))
            .await
            .unwrap_err();
        assert_eq!(err, CaptureError::FfmpegFailed);
        assert!(!format!("{err:?}").contains("secret"));
        assert!(!err.to_string().contains("secret"));
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_capture_drainiert_grosses_stderr_parallel() {
        let dir = fixture_dir().await;
        let ytdlp = fake_script(
            &dir,
            "yt-dlp",
            "head -c 131072 /dev/zero >&2; printf '%s\\n' 'https://example.invalid/live.m3u8'",
        )
        .await;
        let ffmpeg = fake_script(
            &dir,
            "ffmpeg",
            "head -c 131072 /dev/zero >&2; printf 'RIFF....WAVE'",
        )
        .await;
        let capturer = MemoryAudioCapturer::with_timeouts(
            ytdlp.to_string_lossy(),
            ffmpeg.to_string_lossy(),
            Duration::from_secs(2),
            Duration::from_secs(2),
        );

        assert_eq!(
            capturer.capture_wav("nani", Duration::ZERO).await.unwrap(),
            b"RIFF....WAVE"
        );
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}

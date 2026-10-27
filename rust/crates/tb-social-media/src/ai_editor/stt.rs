use std::{
    ffi::OsStr,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tb_config::{stt::SttConfig, BotConfigSnapshot};
use tb_engagement::transcribe::{OpenAiTranscriber, TranscriptSegment};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, ChildStdout, Command},
};

const NICE_BINARY: &str = "/usr/bin/nice";
const CLIENT_UPLOAD_LIMIT: u64 = 25 * 1024 * 1024;
const CLIENT_EXTRACTION_LIMIT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
pub struct SttJobConfig {
    pub binary: PathBuf,
    pub model_directory: PathBuf,
    pub legacy_cache_home: Option<PathBuf>,
    pub port: u16,
    pub threads: u16,
    pub nice: u8,
    pub startup_deadline: Duration,
    pub request_deadline: Duration,
    pub extraction_deadline: Duration,
    pub shutdown_deadline: Duration,
    pub poll_interval: Duration,
    pub max_upload_bytes: u64,
    pub prompt: String,
    pub voice_filter: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SttError {
    #[error("Die lokale Spracherkennung ist nicht sicher konfiguriert.")]
    InvalidConfig,
    #[error("Der Arbeitsordner für die Spracherkennung ist nicht frei.")]
    DirectoryCollision,
    #[error("Der lokale STT-Port ist bereits belegt.")]
    PortOccupied,
    #[error("Die lokalen Sprachmodelle fehlen im angegebenen Verzeichnis.")]
    ModelsMissing,
    #[error("Die lokale Spracherkennung konnte nicht gestartet werden.")]
    StartFailed,
    #[error("Die lokale Spracherkennung ist nicht rechtzeitig bereit.")]
    StartupTimeout,
    #[error("Der lokale STT-Dienst hat eine andere Konfiguration.")]
    WrongServer,
    #[error("Die lokale Spracherkennung wurde vorzeitig beendet.")]
    EarlyExit,
    #[error("Die lokale Transkription ist fehlgeschlagen oder hat ihr Zeitlimit erreicht.")]
    TranscriptionFailed,
    #[error("Die lokale Spracherkennung wurde nicht rechtzeitig beendet.")]
    ShutdownFailed,
    #[error("Die lokale Spracherkennung wurde bereits verwendet oder beendet.")]
    AlreadyUsed,
}

#[derive(Debug, Clone)]
pub struct SttTranscript {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    pub language: Option<String>,
    pub model: String,
    pub duration_seconds: f64,
    pub startup_elapsed: Duration,
    pub transcription_elapsed: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct SttTiming {
    pub startup_elapsed: Duration,
    pub total_elapsed: Duration,
    pub stopped: bool,
}

pub struct SttLease {
    child: Option<Child>,
    directory: PathBuf,
    client: OpenAiTranscriber,
    extraction_deadline: Duration,
    shutdown_deadline: Duration,
    started: Instant,
    startup_elapsed: Duration,
    attempted: bool,
    stopped: bool,
    reaped: bool,
}

#[derive(Serialize)]
struct JobDocument {
    schema_version: u32,
    twitch: JobTwitch,
    stt: SttConfig,
}

#[derive(Serialize)]
struct JobTwitch {
    bot_user_id: String,
    notify_channel_id: &'static str,
    eventsub_callback_url: &'static str,
}

#[derive(Deserialize)]
struct Health {
    status: String,
    model: String,
    threads: u16,
    backend: String,
    config_fingerprint: String,
}

impl SttJobConfig {
    pub fn validate(&self, seed: &BotConfigSnapshot) -> Result<(), SttError> {
        let configured_models = serde_json::to_value(&seed.settings().stt)
            .ok()
            .and_then(|value| {
                value
                    .get("model_directory")
                    .or_else(|| value.get("model_dir"))
                    .and_then(serde_json::Value::as_str)
                    .map(PathBuf::from)
            });
        let legacy_models = self
            .legacy_cache_home
            .as_ref()
            .map(|home| home.join(".cache/deadlock-stt"));
        if !regular_binary(&self.binary)
            || !canonical_directory(&self.model_directory)
            || (self.legacy_cache_home.is_some() && !cfg!(test))
            || (self.legacy_cache_home.is_none() && !root_controlled(&self.binary, true))
            || (self.legacy_cache_home.is_none() && !root_controlled(&self.model_directory, false))
            || match &self.legacy_cache_home {
                Some(home) => {
                    !canonical_directory(home)
                        || legacy_models.as_ref() != Some(&self.model_directory)
                }
                None => configured_models.as_ref() != Some(&self.model_directory),
            }
            || !(1..=19).contains(&self.nice)
            || !(1..=4).contains(&self.threads)
            || self.port == 0
            || self.port == 8791
            || self.startup_deadline.is_zero()
            || self.startup_deadline > Duration::from_secs(600)
            || self.request_deadline.is_zero()
            || self.request_deadline.subsec_nanos() != 0
            || self.request_deadline > Duration::from_secs(3600)
            || self.extraction_deadline.is_zero()
            || self.extraction_deadline.subsec_nanos() != 0
            || self.extraction_deadline > CLIENT_EXTRACTION_LIMIT
            || self.extraction_deadline < self.request_deadline
            || self.shutdown_deadline.is_zero()
            || self.shutdown_deadline > Duration::from_secs(60)
            || self.poll_interval.is_zero()
            || self.poll_interval > Duration::from_secs(10)
            || self.poll_interval > self.startup_deadline
            || self.max_upload_bytes != CLIENT_UPLOAD_LIMIT
            || self.max_upload_bytes > seed.settings().stt.max_upload_bytes
            || self.request_deadline > Duration::from_secs(seed.settings().stt.timeout_seconds)
            || self.extraction_deadline
                > Duration::from_secs(seed.settings().stt.extraction_timeout_seconds)
            || seed.settings().stt.model != "ggml-large-v3-turbo-q5_0"
        {
            return Err(SttError::InvalidConfig);
        }
        for name in ["ggml-large-v3-turbo-q5_0.bin", "ggml-silero-v6.2.0.bin"] {
            if !self
                .model_directory
                .join(name)
                .symlink_metadata()
                .is_ok_and(|metadata| {
                    metadata.is_file()
                        && metadata.len() > 0
                        && (self.legacy_cache_home.is_some()
                            || root_controlled(&self.model_directory.join(name), true))
                })
            {
                return Err(SttError::ModelsMissing);
            }
        }
        Ok(())
    }
}

fn canonical_directory(path: &Path) -> bool {
    path.is_absolute()
        && path.canonicalize().ok().as_deref() == Some(path)
        && std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn regular_binary(path: &Path) -> bool {
    if !path.is_absolute()
        || path.file_name() != Some(OsStr::new("tb-stt-server"))
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return false;
    }
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

fn root_controlled(path: &Path, final_file: bool) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !path.is_absolute() || path.starts_with("/home") {
            return false;
        }
        let Ok(root) = std::fs::symlink_metadata("/") else {
            return false;
        };
        if !root.is_dir() || root.uid() != 0 || root.mode() & 0o022 != 0 {
            return false;
        }
        let mut current = PathBuf::from("/");
        let Ok(relative) = path.strip_prefix("/") else {
            return false;
        };
        let mut components = relative.components().peekable();
        if components.peek().is_none() {
            return false;
        }
        while let Some(component) = components.next() {
            let Component::Normal(name) = component else {
                return false;
            };
            current.push(name);
            let last = components.peek().is_none();
            let Ok(metadata) = std::fs::symlink_metadata(&current) else {
                return false;
            };
            if metadata.uid() != 0
                || metadata.mode() & 0o022 != 0
                || (last && final_file && !metadata.is_file())
                || ((!last || !final_file) && !metadata.is_dir())
            {
                return false;
            }
        }
        true
    }
    #[cfg(not(unix))]
    {
        let _ = (path, final_file);
        false
    }
}

impl SttLease {
    pub async fn start(
        seed: &BotConfigSnapshot,
        job_dir: &Path,
        config: &SttJobConfig,
    ) -> Result<Self, SttError> {
        config.validate(seed)?;
        validate_job_dir(job_dir)?;
        check_port_available(config.port)?;
        let directory = job_dir.join("stt-transient");
        std::fs::create_dir(&directory).map_err(|_| SttError::DirectoryCollision)?;
        let result = Self::start_owned(seed, config, directory.clone()).await;
        if result.is_err() {
            cleanup(&directory);
        }
        result
    }

    async fn start_owned(
        seed: &BotConfigSnapshot,
        config: &SttJobConfig,
        directory: PathBuf,
    ) -> Result<Self, SttError> {
        let mut stt = seed.settings().stt.clone();
        stt.host = IpAddr::V4(Ipv4Addr::LOCALHOST);
        stt.port = config.port;
        stt.threads = config.threads;
        stt.language = None;
        stt.remote_endpoint = None;
        stt.max_upload_bytes = config.max_upload_bytes;
        stt.timeout_seconds = config.request_deadline.as_secs();
        stt.extraction_timeout_seconds = config.extraction_deadline.as_secs();
        let nonce = u64::from_str_radix(&tb_crypto::random_hex_token(6), 16)
            .map_err(|_| SttError::StartFailed)?;
        let document = JobDocument {
            schema_version: seed.settings().schema_version,
            twitch: JobTwitch {
                bot_user_id: nonce.max(1).to_string(),
                notify_channel_id: "2",
                eventsub_callback_url: "https://example.invalid/callback",
            },
            stt,
        };
        let content = toml::to_string(&document).map_err(|_| SttError::InvalidConfig)?;
        let path = directory.join("config.toml");
        let snapshot =
            BotConfigSnapshot::parse(&content, &path).map_err(|_| SttError::InvalidConfig)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| SttError::DirectoryCollision)?;
        std::io::Write::write_all(&mut file, content.as_bytes())
            .map_err(|_| SttError::StartFailed)?;
        drop(file);
        let started = Instant::now();
        let child = Command::new(NICE_BINARY)
            .arg("-n")
            .arg(config.nice.to_string())
            .arg(&config.binary)
            .arg("--config")
            .arg(&path)
            .env_clear()
            .env(
                "HOME",
                config.legacy_cache_home.as_deref().unwrap_or(&directory),
            )
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .current_dir(&directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| SttError::StartFailed)?;
        let endpoint = format!("http://127.0.0.1:{}/v1/audio/transcriptions", config.port);
        let client = OpenAiTranscriber::from_local_config(
            &endpoint,
            &snapshot.settings().stt.model,
            config.request_deadline,
        )
        .map_err(|_| SttError::InvalidConfig)?
        .with_temp_dir(&directory)
        .with_prompt(config.prompt.clone())
        .with_audio_filter(config.voice_filter.clone());
        let mut lease = Self {
            child: Some(child),
            directory,
            client,
            extraction_deadline: config.extraction_deadline,
            shutdown_deadline: config.shutdown_deadline,
            started,
            startup_elapsed: Duration::ZERO,
            attempted: false,
            stopped: false,
            reaped: false,
        };
        let stdout = lease
            .child
            .as_mut()
            .and_then(|child| child.stdout.take())
            .ok_or(SttError::StartFailed)?;
        let fingerprint =
            match tokio::time::timeout(config.startup_deadline, owned_fingerprint(stdout)).await {
                Ok(Ok(fingerprint)) => fingerprint,
                Ok(Err(error)) => return Err(lease.fail_start(error).await),
                Err(_) => return Err(lease.fail_start(SttError::StartupTimeout).await),
            };
        let health_url = format!("http://127.0.0.1:{}/health", config.port);
        if let Err(error) = lease
            .wait_ready(
                &health_url,
                &fingerprint,
                &snapshot.settings().stt.model,
                config.threads,
                config.startup_deadline,
                config.poll_interval,
            )
            .await
        {
            return Err(lease.fail_start(error).await);
        }
        lease.startup_elapsed = started.elapsed();
        Ok(lease)
    }

    async fn fail_start(&mut self, error: SttError) -> SttError {
        if self.stop_inner().await {
            error
        } else {
            SttError::ShutdownFailed
        }
    }

    async fn wait_ready(
        &mut self,
        url: &str,
        fingerprint: &str,
        model: &str,
        threads: u16,
        deadline: Duration,
        poll: Duration,
    ) -> Result<(), SttError> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(poll)
            .build()
            .map_err(|_| SttError::StartFailed)?;
        loop {
            if self.started.elapsed() >= deadline {
                return Err(SttError::StartupTimeout);
            }
            if self
                .child
                .as_mut()
                .ok_or(SttError::EarlyExit)?
                .try_wait()
                .map_err(|_| SttError::EarlyExit)?
                .is_some()
            {
                return Err(SttError::EarlyExit);
            }
            match probe_health(&http, url, fingerprint, model, threads).await {
                Ok(true) if self.started.elapsed() < deadline => return Ok(()),
                Ok(true) => return Err(SttError::StartupTimeout),
                Ok(false) => return Err(SttError::WrongServer),
                Err(()) => {}
            }
            let remaining = deadline.saturating_sub(self.started.elapsed());
            if remaining.is_zero() {
                return Err(SttError::StartupTimeout);
            }
            tokio::time::sleep(poll.min(remaining)).await;
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped && self.reaped
    }

    #[cfg(test)]
    pub fn child_id_for_test(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    pub async fn transcribe_clip(&mut self, clip: &Path) -> Result<SttTranscript, SttError> {
        if self.stopped || self.attempted {
            return Err(SttError::AlreadyUsed);
        }
        self.attempted = true;
        let exited = match self.child.as_mut().ok_or(SttError::EarlyExit)?.try_wait() {
            Ok(status) => status.is_some(),
            Err(_) => true,
        };
        if exited {
            self.stop_inner().await;
            return Err(SttError::EarlyExit);
        }
        let start = Instant::now();
        let output =
            match tokio::time::timeout(self.extraction_deadline, self.client.transcribe_clip(clip))
                .await
            {
                Ok(Ok(output)) => output,
                _ => {
                    self.stop_inner().await;
                    return Err(SttError::TranscriptionFailed);
                }
            };
        let transcription_elapsed = start.elapsed();
        if !self.stop_inner().await {
            return Err(SttError::ShutdownFailed);
        }
        let (segments, language) = if output.text.is_empty() {
            (Vec::new(), None)
        } else {
            (output.segments, output.language)
        };
        Ok(SttTranscript {
            text: output.text,
            segments,
            language,
            model: output.model,
            duration_seconds: output.duration_seconds,
            startup_elapsed: self.startup_elapsed,
            transcription_elapsed,
        })
    }

    pub async fn stop(mut self) -> SttTiming {
        let reaped = self.stop_inner().await;
        SttTiming {
            startup_elapsed: self.startup_elapsed,
            total_elapsed: self.started.elapsed(),
            stopped: reaped,
        }
    }

    async fn stop_inner(&mut self) -> bool {
        if self.stopped {
            return self.reaped;
        }
        self.stopped = true;
        let reaped = if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            match tokio::time::timeout(self.shutdown_deadline, child.wait()).await {
                Ok(Ok(_)) => true,
                _ => {
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(async move {
                            let _ = child.wait().await;
                        });
                    }
                    false
                }
            }
        } else {
            true
        };
        self.reaped = reaped;
        cleanup(&self.directory);
        reaped
    }
}

impl Drop for SttLease {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = child.wait().await;
                });
            } else {
                std::thread::spawn(move || {
                    if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        let _ = runtime.block_on(child.wait());
                    }
                });
            }
        }
        cleanup(&self.directory);
    }
}

pub(crate) fn validate_job_dir(job_dir: &Path) -> Result<(), SttError> {
    if !job_dir.is_absolute()
        || job_dir.canonicalize().ok().as_deref() != Some(job_dir)
        || !std::fs::symlink_metadata(job_dir).is_ok_and(|metadata| metadata.is_dir())
    {
        return Err(SttError::DirectoryCollision);
    }
    Ok(())
}

pub(crate) fn check_port_available(port: u16) -> Result<(), SttError> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = TcpListener::bind(addr).map_err(|_| SttError::PortOccupied)?;
    drop(listener);
    Ok(())
}

async fn owned_fingerprint(stdout: ChildStdout) -> Result<String, SttError> {
    let mut limited = BufReader::new(stdout).take(8192);
    for _ in 0..16 {
        let mut line = Vec::new();
        let count = limited
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| SttError::StartFailed)?;
        if count == 0 || line.last() != Some(&b'\n') {
            return Err(SttError::StartFailed);
        }
        let line = std::str::from_utf8(&line).map_err(|_| SttError::StartFailed)?;
        if !line.contains("TWITCH_STT_CONFIG_V1") {
            continue;
        }
        if !line.contains("config_fingerprint") {
            return Err(SttError::StartFailed);
        }
        let mut fingerprints = line
            .split(|character: char| !character.is_ascii_hexdigit())
            .filter(|part| part.len() == 64 && part.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let fingerprint = fingerprints.next().ok_or(SttError::StartFailed)?;
        if fingerprints.next().is_some() {
            return Err(SttError::StartFailed);
        }
        let fingerprint = fingerprint.to_owned();
        let mut reader = limited.into_inner();
        tokio::spawn(async move {
            let mut buffer = [0_u8; 4096];
            while matches!(reader.read(&mut buffer).await, Ok(count) if count > 0) {}
        });
        return Ok(fingerprint);
    }
    Err(SttError::StartFailed)
}

fn cleanup(directory: &Path) {
    let _ = std::fs::remove_dir_all(directory);
}

pub(crate) async fn probe_health(
    client: &reqwest::Client,
    url: &str,
    fingerprint: &str,
    model: &str,
    threads: u16,
) -> Result<bool, ()> {
    let response = client.get(url).send().await.map_err(|_| ())?;
    if response.status() != reqwest::StatusCode::OK
        || response.content_length().is_some_and(|len| len > 8192)
    {
        return Ok(false);
    }
    let mut response = response;
    let mut bytes = Vec::new();
    loop {
        let chunk = response.chunk().await.map_err(|_| ())?;
        let Some(chunk) = chunk else { break };
        if chunk.len() > 8192usize.saturating_sub(bytes.len()) {
            return Ok(false);
        }
        bytes.extend_from_slice(&chunk);
    }
    let health: Health = serde_json::from_slice(&bytes).map_err(|_| ())?;
    Ok(health.status == "ok"
        && health.model == model
        && health.threads == threads
        && health.backend == "whisper.cpp/whisper-rs"
        && health.config_fingerprint == fingerprint)
}

#[cfg(all(test, unix))]
mod tests {
    use super::root_controlled;
    use std::path::Path;

    #[test]
    fn executable_ancestry_must_be_root_controlled() {
        assert!(root_controlled(Path::new("/usr/bin/nice"), true));
        assert!(!root_controlled(Path::new("/tmp"), false));
        assert!(!root_controlled(Path::new("/home"), false));
        assert!(!root_controlled(Path::new("/bin/nice"), true));
    }
}

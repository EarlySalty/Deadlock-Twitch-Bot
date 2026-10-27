mod cdp;
mod gemini;
mod keeper;
mod steel;

pub use keeper::{check_session, BrowserPasskey, Session};

use std::{fmt, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

static BROWSER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserverError {
    Configuration,
    Source,
    Unavailable,
    NotLoggedIn,
    ModelUnavailable,
    RateLimited,
    Refused,
    Timeout,
    Ui,
}

impl fmt::Display for ObserverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "configuration",
            Self::Source => "source",
            Self::Unavailable => "browser_unavailable",
            Self::NotLoggedIn => "not_logged_in",
            Self::ModelUnavailable => "model_unavailable",
            Self::RateLimited => "rate_limited",
            Self::Refused => "refused",
            Self::Timeout => "timeout",
            Self::Ui => "ui",
        })
    }
}

impl std::error::Error for ObserverError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoObserverConfig {
    pub steel_endpoint: String,
    #[serde(default = "default_devtools_endpoint")]
    pub devtools_endpoint: String,
    pub chat_url: String,
    pub model_item: String,
    pub thinking_menu: String,
    pub thinking_item: String,
    pub active_label: String,
    pub max_video_bytes: u64,
    pub max_answer_bytes: usize,
    pub step_timeout_ms: u64,
    pub upload_timeout_ms: u64,
    pub response_timeout_ms: u64,
    pub total_deadline_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserverVariant {
    pub model_item: String,
    pub thinking_item: String,
    pub active_label: String,
}

impl ObserverVariant {
    pub fn valid(&self) -> bool {
        [&self.model_item, &self.thinking_item, &self.active_label]
            .iter()
            .all(|label| !label.trim().is_empty() && label.len() <= 80)
    }
}

fn default_devtools_endpoint() -> String {
    "http://127.0.0.1:9223".to_owned()
}

fn local_endpoint(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
            && url.port().is_some()
            && url.path() == "/"
            && !value.ends_with('/')
    })
}

impl VideoObserverConfig {
    pub fn valid(&self) -> bool {
        let chat = reqwest::Url::parse(&self.chat_url).ok();
        let labels = [
            &self.model_item,
            &self.thinking_menu,
            &self.thinking_item,
            &self.active_label,
        ];
        local_endpoint(&self.steel_endpoint)
            && local_endpoint(&self.devtools_endpoint)
            && chat.is_some_and(|url| {
                url.scheme() == "https" && url.host_str() == Some("gemini.google.com")
            })
            && labels
                .iter()
                .all(|label| !label.trim().is_empty() && label.len() <= 80)
            && (1..=512 * 1024 * 1024).contains(&self.max_video_bytes)
            && (1_000..=64_000).contains(&self.max_answer_bytes)
            && (1_000..=120_000).contains(&self.step_timeout_ms)
            && (1_000..=600_000).contains(&self.upload_timeout_ms)
            && (1_000..=900_000).contains(&self.response_timeout_ms)
            && self.total_deadline_ms >= self.response_timeout_ms
            && self.total_deadline_ms <= 1_800_000
    }

    pub fn label(&self) -> String {
        format!("gemini-web:{}", self.active_label)
    }

    pub fn with_variant(&self, variant: &ObserverVariant) -> Self {
        Self {
            model_item: variant.model_item.clone(),
            thinking_item: variant.thinking_item.clone(),
            active_label: variant.active_label.clone(),
            ..self.clone()
        }
    }

    fn step(&self) -> Duration {
        Duration::from_millis(self.step_timeout_ms)
    }
}

pub async fn observe(
    config: &VideoObserverConfig,
    video: &Path,
    prompt: &str,
    passkey: &BrowserPasskey,
) -> Result<String, ObserverError> {
    if !config.valid() {
        return Err(ObserverError::Configuration);
    }
    let bytes = read_video(video, config.max_video_bytes).await?;
    let config = config.clone();
    let prompt = prompt.to_owned();
    let passkey = passkey.clone();
    tokio::spawn(async move {
        let _browser = BROWSER.lock().await;
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(config.total_deadline_ms);
        attempt(&config, &bytes, &prompt, &passkey, deadline).await
    })
    .await
    .map_err(|_| ObserverError::Unavailable)?
}

async fn attempt(
    config: &VideoObserverConfig,
    bytes: &[u8],
    prompt: &str,
    passkey: &BrowserPasskey,
    deadline: tokio::time::Instant,
) -> Result<String, ObserverError> {
    ensure_until(config, passkey, deadline).await?;
    match run(config, bytes, prompt, passkey.account(), deadline).await {
        Err(ObserverError::NotLoggedIn) => {
            ensure_until(config, passkey, deadline).await?;
            run(config, bytes, prompt, passkey.account(), deadline).await
        }
        result => result,
    }
}

async fn ensure_until(
    config: &VideoObserverConfig,
    passkey: &BrowserPasskey,
    deadline: tokio::time::Instant,
) -> Result<Session, ObserverError> {
    tokio::time::timeout_at(deadline, keeper::ensure(config, passkey))
        .await
        .unwrap_or(Err(ObserverError::Timeout))
}

async fn read_video(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ObserverError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| ObserverError::Source)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(ObserverError::Source);
    }
    tokio::fs::read(path)
        .await
        .map_err(|_| ObserverError::Source)
}

async fn run(
    config: &VideoObserverConfig,
    bytes: &[u8],
    prompt: &str,
    account: &str,
    deadline: tokio::time::Instant,
) -> Result<String, ObserverError> {
    let session = steel::SteelSession::attach(&config.steel_endpoint, config.step()).await?;
    let result = async {
        let path = session
            .upload(
                "clip.mp4",
                bytes,
                Duration::from_millis(config.upload_timeout_ms),
            )
            .await?;
        let mut browser = cdp::Cdp::connect(&session.websocket_url(), config.step()).await?;
        gemini::ask(&mut browser, config, &path, prompt, account, deadline).await
    }
    .await;
    session.clear_files().await;
    result
}

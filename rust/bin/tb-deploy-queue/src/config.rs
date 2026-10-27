use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub database_url: String,
    pub quiet_minutes: i64,
    pub max_wait_minutes: i64,
    pub poll_seconds: u64,
    pub idle_wait_minutes: i64,
    pub compaction_timeout_minutes: i64,
    pub pruefen_wait_seconds: u64,
    pub repo_dir: PathBuf,
    pub remote: String,
    pub log_dir: PathBuf,
    pub release_root: PathBuf,
    pub frozen_root: PathBuf,
    pub current_link: PathBuf,
    pub deploy_wrapper: PathBuf,
    pub cargo: PathBuf,
    pub child_path: String,
    pub t3: T3Config,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct T3Config {
    pub base_url: String,
    pub bearer_file: PathBuf,
}

pub fn default_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DEPLOY_QUEUE_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(Path::new(&home).join(".config/deploy-queue/config.toml"))
}

pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read config {}", path.display()))?;
    let config: Config =
        toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))?;
    anyhow::ensure!(config.quiet_minutes > 0, "quiet_minutes must be positive");
    anyhow::ensure!(
        config.max_wait_minutes >= config.quiet_minutes,
        "max_wait_minutes must not be below quiet_minutes"
    );
    anyhow::ensure!(config.poll_seconds > 0, "poll_seconds must be positive");
    Ok(config)
}

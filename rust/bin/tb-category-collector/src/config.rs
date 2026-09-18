//! Explicit JSON configuration and protected credentials. No environment lookup.
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub database_url: String,
    pub secrets: SecretSource,
    #[serde(default = "poll_default")]
    pub poll_seconds: u64,
    #[serde(default = "retention_default")]
    pub retention_days: i32,
    #[serde(default = "budget_default")]
    pub raw_budget_bytes: i64,
    #[serde(default)]
    pub media_enabled: bool,
}
fn poll_default() -> u64 {
    60
}
fn retention_default() -> i32 {
    90
}
fn budget_default() -> i64 {
    20 * 1024 * 1024 * 1024
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecretSource {
    File {
        path: PathBuf,
    },
    Infisical {
        socket_path: PathBuf,
        credential_file: PathBuf,
        project_id: String,
        environment: String,
        secret_path: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.client_secret.zeroize();
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|_| "configuration file unavailable")?;
        if bytes.len() > 65536 {
            return Err("configuration file too large".into());
        }
        let config: Self =
            serde_json::from_slice(&bytes).map_err(|_| "invalid collector JSON configuration")?;
        if !(60..=300).contains(&config.poll_seconds)
            || !(1..=90).contains(&config.retention_days)
            || config.raw_budget_bytes < 100 * 1024 * 1024
        {
            return Err("invalid poll, retention or storage budget".into());
        }
        if !config.database_url.starts_with("postgres") {
            return Err("Postgres is required".into());
        }
        Ok(config)
    }

    pub async fn credentials(&self) -> Result<Credentials, String> {
        match &self.secrets {
            SecretSource::File { path } => {
                let bytes = protected_text(path)?;
                let credentials: Credentials =
                    serde_json::from_str(&bytes).map_err(|_| "invalid credential JSON")?;
                validate(credentials)
            }
            SecretSource::Infisical {
                socket_path,
                credential_file,
                project_id,
                environment,
                secret_path,
            } => {
                let bootstrap = protected_text(credential_file)?;
                let client = uplink_infisical_transport::client_builder(socket_path, 0)?
                    .timeout(Duration::from_secs(15))
                    .build()
                    .map_err(|_| "secret client unavailable")?;
                let response = client
                    .get(format!(
                        "{}/api/v4/secrets/",
                        uplink_infisical_transport::BASE_URL
                    ))
                    .query(&[
                        ("projectId", project_id.as_str()),
                        ("environment", environment.as_str()),
                        ("secretPath", secret_path.as_str()),
                        ("viewSecretValue", "true"),
                        ("includeImports", "true"),
                        ("recursive", "false"),
                    ])
                    .bearer_auth(bootstrap.trim())
                    .send()
                    .await
                    .map_err(|_| "secret source unavailable")?;
                if !response.status().is_success() {
                    return Err("secret source rejected collector credentials".into());
                }
                if response
                    .content_length()
                    .is_some_and(|n| n > 4 * 1024 * 1024)
                {
                    return Err("secret response too large".into());
                }
                let body = Zeroizing::new(
                    response
                        .bytes()
                        .await
                        .map_err(|_| "secret read failed")?
                        .to_vec(),
                );
                if body.len() > 4 * 1024 * 1024 {
                    return Err("secret response too large".into());
                }
                let reply: Reply =
                    serde_json::from_slice(&body).map_err(|_| "invalid secret source response")?;
                let mut values = HashMap::new();
                for mut entry in reply
                    .imports
                    .into_iter()
                    .flat_map(|i| i.secrets)
                    .chain(reply.secrets)
                {
                    if matches!(
                        entry.name.as_str(),
                        "TWITCH_CLIENT_ID" | "TWITCH_CLIENT_SECRET"
                    ) {
                        values.insert(
                            entry.name.clone(),
                            Zeroizing::new(std::mem::take(&mut entry.value)),
                        );
                    }
                }
                validate(Credentials {
                    client_id: values
                        .remove("TWITCH_CLIENT_ID")
                        .ok_or("missing Twitch client ID")?
                        .to_string(),
                    client_secret: values
                        .remove("TWITCH_CLIENT_SECRET")
                        .ok_or("missing Twitch client secret")?
                        .to_string(),
                })
            }
        }
    }
}
fn validate(credentials: Credentials) -> Result<Credentials, String> {
    if credentials.client_id.trim().is_empty() || credentials.client_secret.trim().is_empty() {
        return Err("empty Twitch application credentials".into());
    }
    Ok(credentials)
}

pub fn protected_text(path: &Path) -> Result<Zeroizing<String>, String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|_| "credential file unavailable")?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 || metadata.len() > 65536 {
        return Err("credential file must be private and bounded".into());
    }
    std::fs::read_to_string(path)
        .map(Zeroizing::new)
        .map_err(|_| "credential file unreadable".into())
}
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "secretKey")]
    name: String,
    #[serde(rename = "secretValue")]
    value: String,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}
#[derive(Deserialize)]
struct Import {
    #[serde(default)]
    secrets: Vec<Entry>,
}
#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    secrets: Vec<Entry>,
    #[serde(default)]
    imports: Vec<Import>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_is_not_a_configuration_source() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(),r#"{"database_url":"postgresql:///test","secrets":{"type":"file","path":"/test"},"poll_seconds":1}"#).unwrap();
        assert!(Config::load(file.path()).is_err());
        std::fs::write(file.path(),r#"{"database_url":"postgresql:///test","secrets":{"type":"file","path":"/test"},"retention_days":91}"#).unwrap();
        assert!(Config::load(file.path()).is_err());
        std::fs::write(
            file.path(),
            r#"{"database_url":"postgresql:///test","secrets":{"type":"file","path":"/test"}}"#,
        )
        .unwrap();
        assert_eq!(Config::load(file.path()).unwrap().retention_days, 90);
    }
}

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
    // Accepted only to keep old bootstrap files readable. Runtime behavior is
    // exclusively in category_collector_config; no legacy value enables deletion.
    #[serde(default, rename = "poll_seconds")]
    _legacy_poll_seconds: Option<u64>,
    #[serde(default, rename = "retention_days")]
    _legacy_retention_days: Option<i32>,
    #[serde(default, rename = "raw_budget_bytes")]
    _legacy_raw_budget_bytes: Option<i64>,
    #[serde(default, rename = "media_enabled")]
    _legacy_media_enabled: Option<bool>,
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
        if !(config.database_url.starts_with("postgres://")
            || config.database_url.starts_with("postgresql://"))
        {
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
    protected_text_at(path, Path::new("/run/credentials"), 0, 0)
}

fn protected_text_at(
    path: &Path,
    systemd_root: &Path,
    systemd_owner: u32,
    systemd_group: u32,
) -> Result<Zeroizing<String>, String> {
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    // Open once and inspect the same inode that will be read. A symlink must not
    // redirect a credential check to a different file.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "credential file unavailable")?;
    let metadata = file.metadata().map_err(|_| "credential file unavailable")?;
    let mode = metadata.permissions().mode() & 0o7777;
    let private_file = mode & 0o077 == 0;
    let systemd_file = path
        == systemd_root
            .join("tb-category-collector.service")
            .join("category-twitch")
        && mode == 0o440
        && metadata.uid() == systemd_owner
        && metadata.gid() == systemd_group
        && trusted_systemd_directory(systemd_root, systemd_owner, systemd_group)
        && trusted_systemd_directory(
            &systemd_root.join("tb-category-collector.service"),
            systemd_owner,
            systemd_group,
        );
    if !metadata.is_file() || metadata.len() > 65536 || !(private_file || systemd_file) {
        return Err("credential file must be private and bounded".into());
    }
    let mut text = Zeroizing::new(String::new());
    file.take(65537)
        .read_to_string(&mut text)
        .map_err(|_| "credential file unreadable")?;
    if text.len() > 65536 {
        return Err("credential file too large".into());
    }
    Ok(text)
}

fn trusted_systemd_directory(path: &Path, owner: u32, group: u32) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    std::fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_dir()
            && metadata.uid() == owner
            && metadata.gid() == group
            && metadata.permissions().mode() & 0o022 == 0
    })
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
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn credential_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        let unit = root.join("tb-category-collector.service");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&unit, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = unit.join("category-twitch");
        std::fs::write(&path, "bootstrap-token").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
        (dir, root, path)
    }

    #[test]
    fn only_the_root_owned_systemd_credential_can_be_group_readable() {
        let (_dir, root, path) = credential_fixture();
        let owner = unsafe { nix::libc::geteuid() };
        let group = unsafe { nix::libc::getegid() };
        assert_eq!(
            protected_text_at(&path, &root, owner, group)
                .unwrap()
                .as_str(),
            "bootstrap-token"
        );
        assert!(protected_text_at(&path, &root, owner + 1, group).is_err());
        assert!(protected_text_at(&path, &root, owner, group + 1).is_err());

        let other = root.join("tb-category-collector.service").join("other");
        std::fs::write(&other, "wrong-path").unwrap();
        std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o440)).unwrap();
        assert!(protected_text_at(&other, &root, owner, group).is_err());
        assert!(protected_text_at(&path, &root.join("wrong-root"), owner, group).is_err());

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(protected_text_at(&path, &root, owner, group).is_err());
    }

    #[test]
    fn systemd_credential_rejects_symlinks_and_writable_directories() {
        let (_dir, root, path) = credential_fixture();
        let owner = unsafe { nix::libc::geteuid() };
        let group = unsafe { nix::libc::getegid() };
        let unit = root.join("tb-category-collector.service");
        std::fs::remove_file(&path).unwrap();
        std::fs::write(root.join("target"), "target").unwrap();
        symlink(root.join("target"), &path).unwrap();
        assert!(protected_text_at(&path, &root, owner, group).is_err());

        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "bootstrap-token").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
        std::fs::set_permissions(&unit, std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(protected_text_at(&path, &root, owner, group).is_err());
    }

    #[test]
    fn regular_credential_files_keep_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ordinary-credential");
        std::fs::write(&path, "private-token").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(protected_text(&path).unwrap().as_str(), "private-token");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(protected_text(&path).is_err());
    }

    #[test]
    fn fifo_is_rejected_without_waiting_for_a_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fifo");
        nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRUSR).unwrap();
        assert!(protected_text(&path).is_err());
    }
    #[test]
    fn bootstrap_requires_postgres_and_ignores_all_legacy_retention_values() {
        let file = tempfile::NamedTempFile::new().unwrap();
        for retention in [
            serde_json::Value::Null,
            serde_json::json!(1),
            serde_json::json!(90),
            serde_json::json!(1000),
        ] {
            let content = serde_json::json!({"database_url":"postgresql:///test",
                "secrets":{"type":"file","path":"/test"},"retention_days":retention});
            std::fs::write(file.path(), content.to_string()).unwrap();
            assert!(Config::load(file.path()).is_ok());
        }
        std::fs::write(
            file.path(),
            r#"{"database_url":"sqlite:///test","secrets":{"type":"file","path":"/test"}}"#,
        )
        .unwrap();
        assert!(Config::load(file.path()).is_err());
        // No environment fallback when an explicit bootstrap is missing.
        assert!(Config::load(Path::new("/nonexistent/category-bootstrap.json")).is_err());
    }
}

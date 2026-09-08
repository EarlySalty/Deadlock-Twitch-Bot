//! Einmaliger Uplink-Anschluss im bestehenden Dashboardprozess.
//! Normale JSON-Konfiguration, expliziter Infisical-FD, Geheimnisse nur im RAM.

use std::{collections::HashMap, path::Path, sync::OnceLock, time::Duration};

use serde::Deserialize;
use tb_transport_twitch::{HelixClient, HelixConfig};
use tokio::io::AsyncReadExt;
use zeroize::{Zeroize, Zeroizing};

static RUNTIME: OnceLock<UplinkRuntime> = OnceLock::new();
const SECRET_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UplinkConfig {
    pub relay_base_url: String,
    pub infisical_base_url: String,
    pub project_id: String,
    pub environment: String,
    pub secret_path: String,
    pub credential_fd: u32,
    #[serde(default)]
    pub kick_redirect_uri: Option<String>,
    #[serde(default)]
    pub youtube_redirect_uri: Option<String>,
}

pub struct UplinkRuntime {
    pub(crate) base: String,
    api: Zeroizing<String>,
    admin: Zeroizing<String>,
    platform: HashMap<String, Zeroizing<String>>,
    pub(crate) helix: Option<HelixClient>,
    pub(crate) kick_redirect_uri: String,
    pub(crate) youtube_redirect_uri: String,
}

impl std::fmt::Debug for UplinkRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UplinkRuntime([geschützt])")
    }
}

impl UplinkRuntime {
    pub(crate) fn secret(&self, path: &str) -> &str {
        if path.starts_with("/v1/admin/") {
            &self.admin
        } else {
            &self.api
        }
    }
}

pub(crate) fn runtime() -> Result<&'static UplinkRuntime, &'static str> {
    RUNTIME.get().ok_or("Uplink ist noch nicht eingerichtet.")
}

/// Ausschließlich bekannte Uplink-Integrationswerte, aus derselben RAM-Quelle.
pub(crate) fn platform_value(name: &str) -> Option<String> {
    runtime()
        .ok()?
        .platform
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
}

pub fn install(runtime: UplinkRuntime) -> Result<(), &'static str> {
    RUNTIME
        .set(runtime)
        .map_err(|_| "Uplink wurde bereits eingerichtet.")
}

/// Lokale Verwaltungsendpunkte ohne DNS, Zugangsdaten, Pfad oder Redirect.
pub(crate) fn local_origin(value: &str) -> Result<String, &'static str> {
    let url = reqwest::Url::parse(value).map_err(|_| "Uplink-Endpunkt ist ungültig.")?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if url.scheme() != "http"
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Uplink-Endpunkt muss eine lokale Verwaltungsadresse sein.");
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub(crate) fn http_client() -> Result<reqwest::Client, &'static str> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "Uplink-Verbindung konnte nicht vorbereitet werden.")
}

pub(crate) async fn bounded_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("Uplink-Antwort ist zu groß.");
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Uplink-Antwort ist unvollständig.")?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err("Uplink-Antwort ist zu groß.");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn credential(fd: u32) -> Result<Zeroizing<String>, &'static str> {
    use nix::fcntl::{fcntl, FcntlArg, FdFlag};
    let raw = i32::try_from(fd)
        .ok()
        .filter(|fd| *fd >= 3)
        .ok_or("Infisical-FD ist ungültig.")?;
    let flags = fcntl(raw, FcntlArg::F_GETFD).map_err(|_| "Infisical-FD ist nicht verfügbar.")?;
    fcntl(
        raw,
        FcntlArg::F_SETFD(FdFlag::from_bits_retain(flags) | FdFlag::FD_CLOEXEC),
    )
    .map_err(|_| "Infisical-FD konnte nicht geschützt werden.")?;
    let descriptor = filedescriptor::FileDescriptor::dup(&raw)
        .map_err(|_| "Infisical-FD ist nicht verfügbar.")?;
    let file = descriptor
        .as_file()
        .map_err(|_| "Infisical-FD ist nicht verfügbar.")?;
    if !file
        .metadata()
        .map_err(|_| "Infisical-FD ist nicht lesbar.")?
        .is_file()
    {
        return Err("Infisical-FD muss eine begrenzte reguläre Quelle sein.");
    }
    let mut bytes = Zeroizing::new(Vec::new());
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::fs::File::from_std(file)
            .take(8193)
            .read_to_end(&mut bytes),
    )
    .await
    .map_err(|_| "Infisical-FD liefert nicht rechtzeitig.")?
    .map_err(|_| "Infisical-FD konnte nicht gelesen werden.")?;
    if bytes.is_empty() || bytes.len() > 8192 {
        return Err("Infisical-Zugang hat eine ungültige Größe.");
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "Infisical-Zugang ist ungültig.")?
        .trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return Err("Infisical-Zugang ist ungültig.");
    }
    Ok(Zeroizing::new(text.to_string()))
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

fn required(
    values: &mut HashMap<String, Zeroizing<String>>,
    name: &str,
) -> Result<Zeroizing<String>, &'static str> {
    values
        .remove(name)
        .filter(|value| !value.trim().is_empty())
        .ok_or("Der Uplink-Dienstzugang fehlt in Infisical.")
}

async fn fetch(config: &UplinkConfig, token: &str) -> Result<UplinkRuntime, &'static str> {
    let base = local_origin(&config.relay_base_url)?;
    let infisical = local_origin(&config.infisical_base_url)?;
    let kick_redirect_uri = redirect_uri(
        config.kick_redirect_uri.as_deref(),
        "https://deutsche-deadlock-community.de/callback/kick",
    )?;
    let youtube_redirect_uri = redirect_uri(
        config.youtube_redirect_uri.as_deref(),
        "https://deutsche-deadlock-community.de/callback/youtube",
    )?;
    if config.project_id.trim().is_empty()
        || config.environment.trim().is_empty()
        || !config.secret_path.starts_with('/')
    {
        return Err("Infisical-Projektkonfiguration ist unvollständig.");
    }
    let response = http_client()?
        .get(format!("{infisical}/api/v4/secrets/"))
        .query(&[
            ("projectId", config.project_id.as_str()),
            ("environment", config.environment.as_str()),
            ("secretPath", config.secret_path.as_str()),
            ("viewSecretValue", "true"),
            ("includeImports", "true"),
            ("recursive", "false"),
        ])
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "Infisical ist nicht erreichbar.")?;
    if !response.status().is_success() {
        return Err("Infisical hat den Uplink-Zugang abgelehnt.");
    }
    let body = bounded_body(response, SECRET_LIMIT).await?;
    let reply: Reply =
        serde_json::from_slice(&body).map_err(|_| "Infisical-Antwort ist ungültig.")?;
    let mut values = HashMap::new();
    // Lokale Werte haben wie beim vorhandenen Infisical-Leser Vorrang vor Imports.
    for mut entry in reply
        .imports
        .into_iter()
        .flat_map(|import| import.secrets)
        .chain(reply.secrets)
    {
        if [
            "RS_RELAY_API_SECRET",
            "RS_RELAY_ADMIN_SECRET",
            "TWITCH_CLIENT_ID",
            "TWITCH_CLIENT_SECRET",
            "DB_MASTER_KEY_V1",
            "KICK_CLIENT_ID",
            "KICK_CLIENT_SECRET",
            "GOOGLE_OAUTH_ID",
            "GOOGLE_CLIENT_ID",
            "GOOGLE_CLIENT_SECRET",
            "YOUTUBE_CLIENT_ID",
            "YOUTUBE_CLIENT_SECRET",
        ]
        .contains(&entry.name.as_str())
        {
            values.insert(
                entry.name.clone(),
                Zeroizing::new(std::mem::take(&mut entry.value)),
            );
        }
    }
    let api = required(&mut values, "RS_RELAY_API_SECRET")?;
    let admin = required(&mut values, "RS_RELAY_ADMIN_SECRET")?;
    let helix = match (
        values.get("TWITCH_CLIENT_ID"),
        values.get("TWITCH_CLIENT_SECRET"),
    ) {
        (Some(id), Some(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => Some(
            HelixClient::new(HelixConfig::new(id.trim(), secret.trim()))
                .map_err(|_| "Twitch-Anschluss für Uplink ist ungültig.")?,
        ),
        _ => None,
    };
    Ok(UplinkRuntime {
        base,
        api,
        admin,
        helix,
        platform: values,
        kick_redirect_uri,
        youtube_redirect_uri,
    })
}

fn redirect_uri(configured: Option<&str>, fallback: &str) -> Result<String, &'static str> {
    let value = configured.unwrap_or(fallback);
    let url = reqwest::Url::parse(value).map_err(|_| "Uplink-OAuth-Rückadresse ist ungültig.")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Uplink-OAuth-Rückadresse ist ungültig.");
    }
    Ok(url.to_string())
}

pub async fn load(path: &Path) -> Result<UplinkRuntime, &'static str> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut data = Vec::new();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "Uplink-Konfiguration ist nicht lesbar.")?;
    let file = tokio::fs::File::from_std(file);
    if !file
        .metadata()
        .await
        .map_err(|_| "Uplink-Konfiguration ist nicht lesbar.")?
        .is_file()
    {
        return Err("Uplink-Konfiguration muss eine reguläre Datei sein.");
    }
    file.take(65537)
        .read_to_end(&mut data)
        .await
        .map_err(|_| "Uplink-Konfiguration ist nicht lesbar.")?;
    if data.len() > 65536 {
        return Err("Uplink-Konfiguration ist zu groß.");
    }
    let config: UplinkConfig =
        serde_json::from_slice(&data).map_err(|_| "Uplink-Konfiguration ist ungültig.")?;
    // Schützt den Original-FD vor dem ersten möglichen Start eines Fremdprozesses.
    let token = credential(config.credential_fd).await?;
    fetch(&config, &token).await
}

/// Gemeinsamer Argumentvertrag des echten Dashboardstarts und der FD-Probe.
pub async fn load_arguments(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Option<UplinkRuntime>, &'static str> {
    let mut arguments = arguments.into_iter();
    let Some(argument) = arguments.next() else {
        return Ok(None);
    };
    if argument != "--uplink-config" {
        return Err("Unbekanntes Dashboard-Startargument.");
    }
    let path = arguments
        .next()
        .ok_or("Der Pfad der Uplink-Konfiguration fehlt.")?;
    if arguments.next().is_some() {
        return Err("Unbekanntes oder doppeltes Dashboard-Startargument.");
    }
    load(Path::new(&path)).await.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    fn config(base: String) -> UplinkConfig {
        UplinkConfig {
            relay_base_url: base.clone(),
            infisical_base_url: base,
            project_id: "public-test-project".into(),
            environment: "test".into(),
            secret_path: "/uplink".into(),
            credential_fd: 3,
            kick_redirect_uri: None,
            youtube_redirect_uri: None,
        }
    }

    #[test]
    fn control_endpoint_never_accepts_credentials_dns_or_external_addresses() {
        for value in ["http://127.0.0.1:8891", "http://[::1]:8891"] {
            assert!(local_origin(value).is_ok());
        }
        for value in [
            "https://example.com",
            "http://localhost:8891",
            "http://127.0.0.1/key",
            "http://key@127.0.0.1",
            "http://127.0.0.1?token=x",
            "http://10.0.0.1",
        ] {
            assert!(local_origin(value).is_err());
        }
    }

    #[tokio::test]
    async fn infisical_contract_keeps_api_and_admin_separate_and_redacts_debug() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v4/secrets/"))
            .and(header("Authorization", "Bearer synthetic-bootstrap"))
            .and(query_param("includeImports", "true"))
            .and(query_param("projectId", "public-test-project"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "secrets":[{"secretKey":"RS_RELAY_API_SECRET","secretValue":"synthetic-api"},{"secretKey":"RS_RELAY_ADMIN_SECRET","secretValue":"synthetic-admin"}],
                "imports":[{"secrets":[{"secretKey":"RS_RELAY_API_SECRET","secretValue":"import-loses"}]}]
            }))).expect(1).mount(&server).await;
        let runtime = fetch(&config(server.uri()), "synthetic-bootstrap")
            .await
            .unwrap();
        assert_eq!(runtime.secret("/v1/me"), "synthetic-api");
        assert_eq!(runtime.secret("/v1/admin/waitlist"), "synthetic-admin");
        assert!(!format!("{runtime:?}").contains("synthetic"));
    }

    #[tokio::test]
    async fn redirects_never_receive_the_bootstrap_and_errors_never_echo_it() {
        let first = MockServer::start().await;
        let other = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&other)
            .await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", other.uri())
                    .set_body_string("synthetic-bootstrap"),
            )
            .mount(&first)
            .await;
        let error = fetch(&config(first.uri()), "synthetic-bootstrap")
            .await
            .unwrap_err();
        assert!(!error.contains("synthetic"));
    }

    fn memory_credential(value: &[u8]) -> std::fs::File {
        use std::io::{Seek, Write};
        let fd = nix::sys::memfd::memfd_create(
            c"uplink-public-test",
            nix::sys::memfd::MemFdCreateFlag::empty(),
        )
        .unwrap();
        let mut file = std::fs::File::from(fd);
        file.write_all(value).unwrap();
        file.rewind().unwrap();
        file
    }

    #[tokio::test]
    async fn bootstrap_fd_is_private_even_for_later_children_and_rejects_unbounded_sources() {
        use nix::fcntl::{fcntl, FcntlArg, FdFlag};
        use std::os::fd::AsRawFd;
        let file = memory_credential(b"synthetic-bootstrap\n");
        assert_eq!(fcntl(file.as_raw_fd(), FcntlArg::F_GETFD).unwrap(), 0);
        assert_eq!(
            &*credential(file.as_raw_fd() as u32).await.unwrap(),
            "synthetic-bootstrap"
        );
        assert_ne!(
            fcntl(file.as_raw_fd(), FcntlArg::F_GETFD).unwrap() & FdFlag::FD_CLOEXEC.bits(),
            0
        );
        for value in [
            Vec::new(),
            vec![b'x'; 8193],
            b"synthetic bad value".to_vec(),
        ] {
            let file = memory_credential(&value);
            assert!(credential(file.as_raw_fd() as u32).await.is_err());
        }
        let (reader, _writer) = nix::unistd::pipe().unwrap();
        assert!(tokio::time::timeout(
            Duration::from_secs(1),
            credential(reader.as_raw_fd() as u32)
        )
        .await
        .unwrap()
        .is_err());
    }

    #[tokio::test]
    async fn normal_config_and_explicit_memory_fd_reach_the_existing_infisical_contract() {
        use std::os::fd::AsRawFd;
        let credential = memory_credential(b"synthetic-bootstrap");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Authorization", "Bearer synthetic-bootstrap"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"secrets":[
                    {"secretKey":"RS_RELAY_API_SECRET","secretValue":"synthetic-api"},
                    {"secretKey":"RS_RELAY_ADMIN_SECRET","secretValue":"synthetic-admin"}
                ]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("uplink.json");
        std::fs::write(&path, serde_json::to_vec(&serde_json::json!({
            "relay_base_url":server.uri(),"infisical_base_url":server.uri(),"project_id":"public-test",
            "environment":"test","secret_path":"/uplink","credential_fd":credential.as_raw_fd(),
            "kick_redirect_uri":"https://dashboard.example/callback/kick"
        })).unwrap()).unwrap();
        let runtime = load(&path).await.unwrap();
        assert_eq!(
            runtime.kick_redirect_uri,
            "https://dashboard.example/callback/kick"
        );
        assert_eq!(runtime.secret("/v1/me"), "synthetic-api");
        assert_eq!(runtime.secret("/v1/admin/users"), "synthetic-admin");
    }

    #[tokio::test]
    async fn missing_admin_and_oversized_replies_fail_closed() {
        for reply in [
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"secrets":[
                {"secretKey":"RS_RELAY_API_SECRET","secretValue":"synthetic-api"}
            ]})),
            ResponseTemplate::new(200).set_body_string("x".repeat(SECRET_LIMIT + 1)),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(reply)
                .mount(&server)
                .await;
            assert!(fetch(&config(server.uri()), "synthetic-bootstrap")
                .await
                .is_err());
        }
    }

    #[test]
    fn existing_dashboard_launcher_inherits_only_the_fd_and_preserves_arguments() {
        use std::{
            io::Read,
            os::{fd::AsRawFd, unix::fs::PermissionsExt},
            process::{Command, Stdio},
            time::Instant,
        };
        let source = include_str!("../../../scripts/run_tb_dashboard_service.sh");
        let function = source
            .split("start_dashboard_with_uplink() {")
            .nth(1)
            .unwrap()
            .split("\n}\n")
            .next()
            .unwrap();
        let command = format!(
            "start_dashboard_with_uplink() {{{function}\n}}\nstart_dashboard_with_uplink \"$@\""
        );
        let credential = memory_credential(b"synthetic-bootstrap\n");
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("public-probe.sh");
        std::fs::write(&executable, "#!/bin/bash\nset -euo pipefail\nread -r value <&9\n[[ $value == synthetic-bootstrap ]]\nprintf '%s\\n' \"$@\"\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        for (args, expected) in [
            (
                vec!["--extra", "public-value"],
                "--uplink-config\n/etc/deadlock-twitch/uplink.json\n--extra\npublic-value\n",
            ),
            (
                vec!["--uplink-config", "/normal/other.json", "--extra"],
                "--uplink-config\n/normal/other.json\n--extra\n",
            ),
        ] {
            let mut child = Child(
                Command::new("/bin/bash")
                    .env_clear()
                    .args(["-c", &command, "uplink-public-test"])
                    .arg(&executable)
                    .arg(format!(
                        "/proc/{}/fd/{}",
                        std::process::id(),
                        credential.as_raw_fd()
                    ))
                    .arg("/etc/deadlock-twitch/uplink.json")
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                assert!(Instant::now() < deadline, "Launcher überschreitet Frist");
                std::thread::sleep(Duration::from_millis(10));
            }
            let mut output = String::new();
            child
                .0
                .stdout
                .take()
                .unwrap()
                .take(4096)
                .read_to_string(&mut output)
                .unwrap();
            assert_eq!(output, expected);
            assert!(!output.contains("synthetic-bootstrap"));
        }
    }
}

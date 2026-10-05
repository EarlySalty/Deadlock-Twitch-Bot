//! Einziger Katalogwriter. Keine Chattexte, keine Konfiguration aus ENV.
use crate::{hub::LlmError, LlmEndpoint, Request};
use chrono::{DateTime, Utc};
use fireworks_model_selection::{Selection, STATE_PATH, TRUSTED_UID};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

const CATALOG: &str = "https://api.fireworks.ai/v1/accounts/fireworks/models";
const LIMIT: usize = 2 * 1024 * 1024;
fn error(reason: &str) -> LlmError {
    LlmError::Unavailable(reason.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub ledger: LedgerConfig,
    pub project_id: String,
    pub environment: String,
    pub secret_path: String,
    pub credential_path: PathBuf,
    pub infisical_socket: PathBuf,
    pub infisical_socket_owner_uid: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerConfig {
    pub socket: PathBuf,
    pub database: String,
    pub user: String,
}
impl Default for LedgerConfig {
    fn default() -> Self {
        Self {
            socket: "/var/run/postgresql".into(),
            database: "twitch_analytics".into(),
            user: "nathanael".into(),
        }
    }
}
impl Config {
    pub fn load(path: &Path) -> Result<Self, LlmError> {
        let text = protected_read(path, 16 * 1024)?;
        let config: Self = toml::from_str(
            std::str::from_utf8(&text).map_err(|_| error("Resolverkonfiguration ungültig"))?,
        )
        .map_err(|_| error("Resolverkonfiguration ungültig"))?;
        if config.project_id.is_empty()
            || config.environment.is_empty()
            || !config.secret_path.starts_with('/')
        {
            return Err(error("Infisical-Konfiguration unvollständig"));
        }
        Ok(config)
    }
}
fn protected_read(path: &Path, max: usize) -> Result<Vec<u8>, LlmError> {
    if !path.is_absolute() {
        return Err(error("Konfigurationspfad ist nicht absolut"));
    }
    for parent in path.ancestors().skip(1) {
        let m =
            std::fs::symlink_metadata(parent).map_err(|_| error("Konfiguration nicht lesbar"))?;
        if !m.is_dir() || ![0, TRUSTED_UID].contains(&m.uid()) || m.mode() & 0o022 != 0 {
            return Err(error("Konfigurationspfad ist nicht geschützt"));
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| error("Konfiguration nicht lesbar"))?;
    let m = file
        .metadata()
        .map_err(|_| error("Konfiguration nicht lesbar"))?;
    if !m.is_file()
        || m.nlink() != 1
        || ![0, TRUSTED_UID].contains(&m.uid())
        || m.mode() & 0o022 != 0
    {
        return Err(error("Konfigurationsdatei ist nicht geschützt"));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("Konfiguration nicht lesbar"))?;
    if bytes.len() > max {
        return Err(error("Konfiguration zu groß"));
    }
    Ok(bytes)
}
async fn bounded(mut response: reqwest::Response, stage: &str) -> Result<Vec<u8>, LlmError> {
    if !response.status().is_success() {
        return Err(error(&format!(
            "{stage}: HTTP {}",
            response.status().as_u16()
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| error("Antwort unvollständig"))?
    {
        if bytes.len() + chunk.len() > LIMIT {
            return Err(error("Antwort überschreitet Größenlimit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
#[derive(Deserialize)]
struct Secret {
    #[serde(rename = "secretKey")]
    name: String,
    #[serde(rename = "secretValue")]
    value: Zeroizing<String>,
}
#[derive(Deserialize)]
struct Import {
    #[serde(default)]
    secrets: Vec<Secret>,
}
#[derive(Deserialize)]
struct Secrets {
    #[serde(default)]
    secrets: Vec<Secret>,
    #[serde(default)]
    imports: Vec<Import>,
}
async fn credential_named(config: &Config, name: &str) -> Result<Zeroizing<String>, LlmError> {
    let raw = Zeroizing::new(protected_read(&config.credential_path, 4096)?);
    let token = std::str::from_utf8(&raw)
        .map_err(|_| error("Infisical-Credential ungültig"))?
        .trim();
    if token.is_empty() {
        return Err(error("Infisical-Credential fehlt"));
    }
    let client = uplink_infisical_transport::client_builder(
        &config.infisical_socket,
        config.infisical_socket_owner_uid,
    )
    .map_err(error)?
    .timeout(Duration::from_secs(15))
    .build()
    .map_err(|_| error("Infisical-Transport nicht verfügbar"))?;
    let response = client
        .get(format!(
            "{}/api/v4/secrets/",
            uplink_infisical_transport::BASE_URL
        ))
        .bearer_auth(token)
        .query(&[
            ("projectId", config.project_id.as_str()),
            ("environment", config.environment.as_str()),
            ("secretPath", config.secret_path.as_str()),
            ("viewSecretValue", "true"),
            ("includeImports", "true"),
            ("recursive", "false"),
        ])
        .send()
        .await
        .map_err(|_| error("Infisical nicht erreichbar"))?;
    let bytes = Zeroizing::new(bounded(response, "Infisical-Zugriff").await?);
    let values: Secrets =
        serde_json::from_slice(&bytes).map_err(|_| error("Infisical-Antwort ungültig"))?;
    values
        .secrets
        .into_iter()
        .chain(values.imports.into_iter().flat_map(|i| i.secrets))
        .find(|s| {
            (s.name == name || (name == "FIREWORKS_API_KEY" && s.name == "FIREWORK_API_KEY"))
                && !s.value.trim().is_empty()
        })
        .map(|s| s.value)
        .ok_or_else(|| error("Benötigter Infisical-Zugang fehlt"))
}
#[derive(Clone, Debug)]
struct Candidate {
    model: String,
    release: Option<DateTime<Utc>>,
}
fn candidate(v: &serde_json::Value) -> Option<Candidate> {
    if !v.get("public")?.as_bool()?
        || v.get("state")?.as_str()? != "READY"
        || !v.get("supportsServerless")?.as_bool()?
        || !v.get("supportsImageInput")?.as_bool()?
        || !v.get("supportsTools")?.as_bool()?
    {
        return None;
    }
    let model = v.get("name")?.as_str()?;
    fireworks_model_selection::version(model)?;
    let release = v
        .get("createTime")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));
    if release.is_some_and(|d| d > Utc::now()) {
        return None;
    }
    Some(Candidate {
        model: model.into(),
        release,
    })
}
fn ranked(mut entries: Vec<Candidate>) -> Result<Vec<Candidate>, LlmError> {
    entries.sort_by_key(|e| {
        std::cmp::Reverse((
            fireworks_model_selection::version(&e.model).expect("validated"),
            e.release,
        ))
    });
    entries.dedup_by(|a, b| a.model == b.model);
    for pair in entries.windows(2) {
        if fireworks_model_selection::version(&pair[0].model)
            == fireworks_model_selection::version(&pair[1].model)
            && pair[0].release == pair[1].release
            && pair[0].model != pair[1].model
        {
            return Err(error(
                "Modellrevision ohne eindeutigen Veröffentlichungszeitpunkt",
            ));
        }
    }
    Ok(entries)
}
async fn catalog(key: &str, url: &str) -> Result<Vec<Candidate>, LlmError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| error("Katalogclient nicht verfügbar"))?;
    let mut result = Vec::new();
    let mut token = String::new();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..32 {
        let response = client
            .get(url)
            .bearer_auth(key)
            .query(&[("pageSize", "200"), ("pageToken", token.as_str())])
            .send()
            .await
            .map_err(|_| error("Modellkatalog nicht erreichbar"))?;
        let json: serde_json::Value =
            serde_json::from_slice(&bounded(response, "Modellkatalog").await?)
                .map_err(|_| error("Modellkatalog ungültig"))?;
        let models = json
            .get("models")
            .and_then(|v| v.as_array())
            .ok_or_else(|| error("Modellkatalogschema ungültig"))?;
        result.extend(models.iter().filter_map(candidate));
        token = json
            .get("nextPageToken")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into();
        if token.is_empty() {
            return ranked(result);
        }
        if token.len() > 4096 || !seen.insert(token.clone()) {
            return Err(error("Katalogseitennummer ungültig"));
        }
    }
    Err(error("Modellkatalog überschreitet Seitenlimit"))
}
async fn probe_at(key: &str, model: &str, base_url: &str) -> Result<(), LlmError> {
    if fireworks_model_selection::version(model).is_none() {
        return Err(error("Modellprobe außerhalb der freigegebenen Familie"));
    }
    let endpoint = LlmEndpoint {
        provider: "fireworks",
        base_url: base_url.into(),
        model: model.into(),
        api_key: Some(key.into()),
    };
    let request = Request::simple(
        "Synthetic service check. Return only JSON.",
        "Return exactly {\"ready\":true}.",
    )
    .json_object()
    .denken_aus()
    .strip_think()
    .max_tokens(32)
    .ledger_purpose("model_selection_probe");
    let response = crate::hub::call_endpoint(
        &endpoint,
        &request,
        Some("model_selection_probe"),
        Duration::from_secs(30),
    )
    .await?;
    let json: serde_json::Value = serde_json::from_str(response.text.trim())
        .map_err(|_| error("Modellprobe liefert kein JSON"))?;
    if json != serde_json::json!({"ready":true}) {
        return Err(error("Modellprobe liefert kein vollständiges Ergebnis"));
    }
    Ok(())
}
fn atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), LlmError> {
    let parent = path.parent().ok_or_else(|| error("Statepfad ungültig"))?;
    let m = std::fs::symlink_metadata(parent).map_err(|_| error("Stateverzeichnis fehlt"))?;
    if !m.is_dir() || ![0, TRUSTED_UID].contains(&m.uid()) || m.mode() & 0o022 != 0 {
        return Err(error("Stateverzeichnis ist nicht geschützt"));
    }
    let tmp = parent.join(format!(".model-selection-{}.tmp", std::process::id()));
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|_| error("State-Tempdatei nicht verfügbar"))?;
        created = true;
        let bytes =
            serde_json::to_vec(value).map_err(|_| error("State-Encoding fehlgeschlagen"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| error("State-Schreiben fehlgeschlagen"))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o644))
            .map_err(|_| error("State-Rechte fehlgeschlagen"))?;
        file.sync_all()
            .map_err(|_| error("State-Sync fehlgeschlagen"))?;
        std::fs::rename(&tmp, path).map_err(|_| error("State-Austausch fehlgeschlagen"))?;
        File::open(parent)
            .and_then(|d| d.sync_all())
            .map_err(|_| error("Stateverzeichnis-Sync fehlgeschlagen"))
    })();
    if result.is_err() && created {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attempts {
    checked: i64,
    notices: Vec<i64>,
    suppressed: u64,
}
fn attempts(path: &Path) -> Result<Attempts, LlmError> {
    if !path
        .try_exists()
        .map_err(|_| error("Prüfstatus nicht erreichbar"))?
    {
        return Ok(Attempts::default());
    }
    serde_json::from_slice(&protected_read(path, 4096)?).map_err(|_| error("Prüfstatus ungültig"))
}
/// Prozesslock plus persistente 24h-Frist, auch für manuell gestartete Läufe.
pub async fn run(config: Config) -> Result<(), LlmError> {
    run_at(
        &config,
        Path::new(STATE_PATH),
        &Utc::now,
        CATALOG,
        crate::selection::FIREWORKS_BASE_URL,
    )
    .await
}
async fn run_at(
    config: &Config,
    state: &Path,
    time: &dyn Fn() -> DateTime<Utc>,
    catalog_url: &str,
    base_url: &str,
) -> Result<(), LlmError> {
    let dir = state.parent().expect("constant absolute path");
    let metadata = std::fs::symlink_metadata(dir).map_err(|_| error("Stateverzeichnis fehlt"))?;
    if !metadata.is_dir()
        || ![0, TRUSTED_UID].contains(&metadata.uid())
        || metadata.mode() & 0o022 != 0
    {
        return Err(error("Stateverzeichnis ist nicht geschützt"));
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("llm-model-selection.lock"))
        .map_err(|_| error("Resolverlock nicht verfügbar"))?;
    let meta = lock
        .metadata()
        .map_err(|_| error("Resolverlock nicht verfügbar"))?;
    if !meta.is_file() || meta.uid() != TRUSTED_UID || meta.mode() & 0o077 != 0 || meta.nlink() != 1
    {
        return Err(error("Resolverlock ist nicht geschützt"));
    }
    match lock.try_lock_exclusive() {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
        Err(_) => return Err(error("Resolverlock fehlgeschlagen")),
    }
    let attempt_path = dir.join("llm-model-selection-attempts.json");
    let mut status = attempts(&attempt_path)?;
    let now = time().timestamp();
    if status.checked > now || status.notices.len() > 2 || status.notices.iter().any(|at| *at > now)
    {
        return Err(error("Prüfstatus liegt in der Zukunft"));
    }
    let verified = fireworks_model_selection::read_selection(state, TRUSTED_UID)
        .map(|s| s.checked_at.timestamp())
        .unwrap_or(0);
    status.checked = status.checked.min(verified);
    if !daily_due(status.checked, now) {
        println!("Tagesprüfung bereits durch geprüfte Auswahl belegt");
        return Ok(());
    }
    let result = refresh(config, state, catalog_url, base_url, time).await;
    if let Err(ref failure) = result {
        status.notices.retain(|at| now - *at < 7 * 86400);
        if status.notices.len() < 2 && status.notices.last().is_none_or(|at| now - *at >= 86400) {
            eprintln!("KI-Modellauswahl konnte nicht geprüft werden; letzter geprüfter Stand bleibt erhalten ({} Wiederholungen).", status.suppressed);
            status.notices.push(now);
            status.suppressed = 0;
        } else {
            status.suppressed = status.suppressed.saturating_add(1);
        }
        atomic(&attempt_path, &status)
            .map_err(|status_error| error(&format!("{failure}; {status_error}")))?;
    } else {
        status.checked = time().timestamp();
        atomic(&attempt_path, &status)?;
    }
    result
}
fn daily_due(previous: i64, now: i64) -> bool {
    previous.div_euclid(86400) < now.div_euclid(86400)
}
async fn refresh(
    config: &Config,
    path: &Path,
    catalog_url: &str,
    base_url: &str,
    time: &dyn Fn() -> DateTime<Utc>,
) -> Result<(), LlmError> {
    let key = credential_named(config, "FIREWORKS_API_KEY").await?;
    if !crate::hub::is_loopback_endpoint(base_url) && crate::ledger::pool().await.is_none() {
        if !config.ledger.socket.is_absolute()
            || config.ledger.database != "twitch_analytics"
            || config.ledger.user.is_empty()
        {
            return Err(error("Verbrauchs-Peervertrag ungültig"));
        }
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(
                sqlx::postgres::PgConnectOptions::new()
                    .host(
                        config
                            .ledger
                            .socket
                            .to_str()
                            .ok_or_else(|| error("Socketpfad ungültig"))?,
                    )
                    .database(&config.ledger.database)
                    .username(&config.ledger.user),
            )
            .await
            .map_err(|_| error("Verbrauchsdatenbank nicht erreichbar"))?;
        crate::ledger::initialize(pool, "Deadlock-Twitch-Bot", "deadlock-llm-model-resolver")
            .map_err(error)?;
    }
    let entries = catalog(&key, catalog_url).await?;
    let previous = fireworks_model_selection::read_selection(path, TRUSTED_UID).ok();
    for mut entry in entries.into_iter().take(3) {
        if !can_replace(previous.as_ref(), &entry) {
            return Err(error(
                "Modellkatalog bietet ältere oder uneindeutige Revision",
            ));
        }
        if let Some(old) = previous.as_ref().filter(|p| p.model == entry.model) {
            entry.release = entry.release.or(old.release);
        }
        match probe_and_publish_at(&key, &entry, path, base_url, time).await {
            Ok(()) => {
                println!("Geprüfte DeepSeek-Flash-Auswahl: {}", entry.model);
                return Ok(());
            }
            Err(LlmError::Http {
                status: 404 | 410, ..
            }) => continue,
            Err(failure) => return Err(failure),
        }
    }
    Err(error("Kein serverloses, geprüftes Flash-Modell verfügbar"))
}
fn can_replace(previous: Option<&Selection>, entry: &Candidate) -> bool {
    let Some(old) = previous else {
        return true;
    };
    let new_version = fireworks_model_selection::version(&entry.model);
    let old_version = fireworks_model_selection::version(&old.model);
    match new_version.cmp(&old_version) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (entry.release, old.release) {
            (Some(new), Some(previous_release)) => {
                new > previous_release || (new == previous_release && entry.model == old.model)
            }
            _ => entry.model == old.model,
        },
    }
}
#[cfg(test)]
async fn probe_and_publish(
    key: &str,
    entry: &Candidate,
    path: &Path,
    base_url: &str,
) -> Result<(), LlmError> {
    probe_and_publish_at(key, entry, path, base_url, &Utc::now).await
}
async fn probe_and_publish_at(
    key: &str,
    entry: &Candidate,
    path: &Path,
    base_url: &str,
    time: &dyn Fn() -> DateTime<Utc>,
) -> Result<(), LlmError> {
    probe_at(key, &entry.model, base_url)
        .await
        .map_err(|failure| match failure {
            LlmError::Http { status, .. } => LlmError::Http {
                status,
                body: String::new(),
            },
            LlmError::Timeout(_) => error("Modellprobe: Zeitgrenze überschritten"),
            LlmError::Transport(_) => error("Modellprobe: Transport fehlgeschlagen"),
            LlmError::Unparsable(_) => error("Modellprobe: Antwort ungültig"),
            LlmError::Unavailable(_) => error("Modellprobe: kein gültiges Prüfergebnis"),
        })?;
    let now = time();
    let selection = Selection {
        schema_version: 1,
        provider: "fireworks".into(),
        model: entry.model.clone(),
        release: entry.release,
        probed_at: now,
        checked_at: now,
    };
    selection
        .validate()
        .map_err(|_| error("Geprüfte Auswahl ungültig"))?;
    atomic(path, &selection)
}
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

    struct Fixture {
        dir: PathBuf,
        config: Config,
        server: MockServer,
        bridge: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.bridge.abort();
            std::fs::remove_dir_all(&self.dir).unwrap();
        }
    }
    impl Fixture {
        async fn new(name: &str) -> Self {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let cwd = std::env::current_dir().unwrap();
            let base = cwd
                .ancestors()
                .find(|p| {
                    p.as_os_str().len() < 60
                        && p.ancestors().all(|a| {
                            std::fs::symlink_metadata(a).is_ok_and(|m| {
                                m.is_dir()
                                    && [0, TRUSTED_UID].contains(&m.uid())
                                    && m.mode() & 0o022 == 0
                            })
                        })
                })
                .unwrap();
            let dir = base.join(format!(".resolver-{}-{name}", std::process::id()));
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            let credential_path = dir.join("credential");
            std::fs::write(&credential_path, "synthetic-infisical-credential").unwrap();
            std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600))
                .unwrap();
            let socket = dir.join("api.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
            let bridge = tokio::spawn(async move {
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    let mut chunk = [0; 4096];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let count = stream.read(&mut chunk).await.unwrap();
                        if count == 0 {
                            break;
                        }
                        request.extend_from_slice(&chunk[..count]);
                    }
                    let body = r#"{"secrets":[{"secretKey":"FIREWORKS_API_KEY","secretValue":"synthetic-fireworks-credential"}]}"#;
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            });
            Self {
                config: Config {
                    ledger: LedgerConfig::default(),
                    project_id: "synthetic-project".into(),
                    environment: "test".into(),
                    secret_path: "/test".into(),
                    credential_path,
                    infisical_socket: socket,
                    infisical_socket_owner_uid: TRUSTED_UID,
                },
                dir,
                server: MockServer::start().await,
                bridge,
            }
        }
        async fn respond(&self, status: u16) {
            self.server.reset().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "models": [{"name":"accounts/fireworks/models/deepseek-v4p1-flash", "public":true, "state":"READY", "supportsServerless":true, "supportsImageInput":true, "supportsTools":true}]
                })))
                .mount(&self.server).await;
            let body = if status == 200 {
                serde_json::json!({"choices":[{"message":{"content":"{\"ready\":true}"}}]})
            } else {
                serde_json::json!({"error":"synthetic-fireworks-credential synthetic-infisical-credential"})
            };
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&self.server)
                .await;
        }
        async fn run(&self, time: DateTime<Utc>) -> Result<(), LlmError> {
            run_at(
                &self.config,
                &self.dir.join("selection.json"),
                &|| time,
                &self.server.uri(),
                &self.server.uri(),
            )
            .await
        }
        fn status(&self) -> Attempts {
            attempts(&self.dir.join("llm-model-selection-attempts.json")).unwrap()
        }
        fn selection(&self) -> Vec<u8> {
            std::fs::read(self.dir.join("selection.json")).unwrap()
        }
    }
    fn test_time() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }
    #[tokio::test]
    async fn failed_refresh_then_regular_retry_records_only_success() {
        let fixture = Fixture::new("retry").await;
        let now = test_time();
        let yesterday = now - chrono::Duration::days(1);
        fixture.respond(200).await;
        fixture.run(yesterday).await.unwrap();
        let original = fixture.selection();
        fixture.respond(503).await;
        assert!(fixture.run(now).await.is_err());
        assert_eq!(fixture.selection(), original);
        assert_eq!(fixture.status().checked, yesterday.timestamp());
        fixture.respond(200).await;
        fixture.run(now).await.unwrap();
        assert_eq!(fixture.server.received_requests().await.unwrap().len(), 2);
        assert_eq!(fixture.status().checked, now.timestamp());
        let selection = fireworks_model_selection::read_selection(
            &fixture.dir.join("selection.json"),
            TRUSTED_UID,
        )
        .unwrap();
        assert_eq!(selection.checked_at, now);
        assert_eq!(selection.probed_at, now);
        assert_eq!(
            selection.model,
            "accounts/fireworks/models/deepseek-v4p1-flash"
        );
        fixture.run(now).await.unwrap();
        assert_eq!(fixture.server.received_requests().await.unwrap().len(), 2);
    }
    #[tokio::test]
    async fn refresh_error_preserves_http_status_without_credentials() {
        let fixture = Fixture::new("diagnosis").await;
        fixture.respond(200).await;
        fixture
            .run(test_time() - chrono::Duration::days(1))
            .await
            .unwrap();
        let original = fixture.selection();
        fixture.respond(503).await;
        let failure = fixture.run(test_time()).await.unwrap_err();
        assert_eq!(fixture.selection(), original);
        assert_eq!(failure.code(), "http_status");
        assert!(matches!(failure, LlmError::Http { status: 503, ref body } if body.is_empty()));
        let text = failure.to_string();
        assert!(!text.contains("synthetic-fireworks-credential"));
        assert!(!text.contains("synthetic-infisical-credential"));
    }
    #[tokio::test]
    async fn legacy_failed_day_marker_does_not_skip_actual_refresh() {
        let fixture = Fixture::new("legacy").await;
        let now = test_time();
        fixture.respond(200).await;
        fixture.run(now - chrono::Duration::days(1)).await.unwrap();
        let legacy = Attempts {
            checked: now.timestamp(),
            notices: vec![now.timestamp()],
            suppressed: 0,
        };
        atomic(
            &fixture.dir.join("llm-model-selection-attempts.json"),
            &legacy,
        )
        .unwrap();
        fixture.respond(200).await;
        fixture.run(now).await.unwrap();
        assert_eq!(fixture.server.received_requests().await.unwrap().len(), 2);
        assert_eq!(
            fireworks_model_selection::read_selection(
                &fixture.dir.join("selection.json"),
                TRUSTED_UID
            )
            .unwrap()
            .checked_at,
            now
        );
    }
    #[test]
    fn family_availability_and_numeric_ranking() {
        let item = |name: &str, created: Option<&str>| serde_json::json!({"name":format!("accounts/fireworks/models/{name}"),"public":true,"state":"READY","supportsServerless":true,"supportsImageInput":true,"supportsTools":true,"createTime":created});
        let entries = ranked(
            [
                item("deepseek-v4p9-flash", Some("2026-09-30T00:00:00Z")),
                item("deepseek-v4p10-flash", None),
                item("deepseek-v10-flash", None),
            ]
            .iter()
            .filter_map(candidate)
            .collect(),
        )
        .unwrap();
        assert!(entries[0].model.ends_with("v10-flash"));
        assert!(entries[1].model.ends_with("v4p10-flash"));
        for field in [
            "public",
            "supportsServerless",
            "supportsImageInput",
            "supportsTools",
        ] {
            let mut v = item("deepseek-v4p1-flash", None);
            v[field] = false.into();
            assert!(candidate(&v).is_none());
        }
        let mut v = item("deepseek-v4p1-flash", None);
        v["state"] = "CREATING".into();
        assert!(candidate(&v).is_none());
        assert!(candidate(&item("deepseek-v4p1-flash-preview", None)).is_none());
    }
    #[test]
    fn daily_calendar_boundary_not_elapsed_24_hours() {
        assert!(!daily_due(2 * 86400 + 10, 2 * 86400 + 86000));
        assert!(daily_due(2 * 86400 + 86000, 3 * 86400 + 900));
        assert!(daily_due(2 * 86400 + 3600, 3 * 86400));
    }
    #[test]
    fn ambiguous_equal_versions_never_lexically_choose() {
        let c = |model: &str| Candidate {
            model: model.into(),
            release: None,
        };
        assert!(ranked(vec![
            c("accounts/fireworks/models/deepseek-v4p1-flash"),
            c("accounts/fireworks/models/deepseek-v4p1-flash-1001")
        ])
        .is_err());
    }
    #[test]
    fn release_checkpoint_never_downgrades_same_version() {
        let now = Utc::now();
        let old = Selection {
            schema_version: 1,
            provider: "fireworks".into(),
            model: "accounts/fireworks/models/deepseek-v4p1-flash-1001".into(),
            release: Some(now),
            probed_at: now,
            checked_at: now,
        };
        let mut entry = Candidate {
            model: "accounts/fireworks/models/deepseek-v4p1-flash-0930".into(),
            release: Some(now - chrono::Duration::days(1)),
        };
        assert!(!can_replace(Some(&old), &entry));
        entry.release = old.release;
        assert!(!can_replace(Some(&old), &entry));
        entry.release = None;
        assert!(!can_replace(Some(&old), &entry));
        entry.model = "accounts/fireworks/models/deepseek-v4p2-flash".into();
        assert!(can_replace(Some(&old), &entry));
        entry.model = old.model.clone();
        assert!(can_replace(Some(&old), &entry));
    }
    #[tokio::test]
    async fn actual_probe_requires_content_json_and_disables_thinking() {
        use wiremock::matchers::{body_string_contains, method};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        for body in [
            serde_json::json!({"choices":[{"message":{"content":"","reasoning_content":"ready"}}]}),
            serde_json::json!({"choices":[{"message":{"content":"{\"ready\":false}"}}]}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(body_string_contains("\"reasoning_effort\":\"none\""))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            assert!(probe_at(
                "synthetic-test-key",
                "accounts/fireworks/models/deepseek-v4p1-flash",
                &server.uri()
            )
            .await
            .is_err());
        }
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains("\"reasoning_effort\":\"none\""))
            .and(body_string_contains(
                "\"model\":\"accounts/fireworks/models/deepseek-v4p2-flash\"",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"choices":[{"message":{"content":"{\"ready\":true}"}}]}),
            ))
            .mount(&server)
            .await;
        assert!(probe_at(
            "synthetic-test-key",
            "accounts/fireworks/models/deepseek-v4p2-flash",
            &server.uri()
        )
        .await
        .is_ok());
    }
    #[tokio::test]
    async fn probe_failure_preserves_last_known_working_state() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let cwd = std::env::current_dir().unwrap();
        let base = cwd
            .ancestors()
            .find(|p| {
                p.ancestors().all(|a| {
                    std::fs::symlink_metadata(a).is_ok_and(|m| {
                        m.is_dir() && [0, TRUSTED_UID].contains(&m.uid()) && m.mode() & 0o022 == 0
                    })
                })
            })
            .unwrap();
        let dir = base.join(format!(".writer-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("selection.json");
        let now = Utc::now();
        let old = Selection {
            schema_version: 1,
            provider: "fireworks".into(),
            model: "accounts/fireworks/models/deepseek-v4p1-flash".into(),
            release: None,
            probed_at: now,
            checked_at: now,
        };
        atomic(&path, &old).unwrap();
        let original = std::fs::read(&path).unwrap();
        let next = Candidate {
            model: "accounts/fireworks/models/deepseek-v4p2-flash".into(),
            release: None,
        };
        for status in [404, 503] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            assert!(
                probe_and_publish("synthetic-key", &next, &path, &server.uri())
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        let first = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.join("lock"))
            .unwrap();
        first.try_lock_exclusive().unwrap();
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join("lock"))
            .unwrap();
        assert!(second.try_lock_exclusive().is_err());
        drop(first);
        assert!(second.try_lock_exclusive().is_ok());
        drop(second);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

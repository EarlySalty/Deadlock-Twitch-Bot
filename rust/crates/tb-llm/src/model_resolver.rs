//! YAML-gesteuerte Fortsetzung des früheren Fireworks-Resolvers (988d7b56).
//!
//! Gleiche Zuständigkeit und gleiche Tabelle `llm_model_cache`, aber kein
//! einkompilierter Modellname und kein ungeprüfter Katalog-/Cache-Treffer mehr.
//! Die neueste stabile Flash-Version wird numerisch verglichen und vor der
//! Übernahme mit ausschließlich synthetischen Daten geprüft. Ein 404 verwirft
//! nur das abgelehnte Modell; parallele Aufrufer teilen sich einen Refresh.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::{hub::LlmError, LlmEndpoint, Request};

const CATALOG_URL: &str = "https://api.fireworks.ai/v1/accounts/fireworks/models";
const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_CATALOG_PAGES: usize = 32;
const MAX_PROBES: usize = 3;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPolicy {
    pub schema_version: u32,
    pub provider: String,
    pub family: String,
    pub selection: String,
    pub bootstrap_model: String,
    pub refresh_seconds: u64,
    pub retry_seconds: u64,
    pub max_stale_seconds: u64,
    pub catalog_timeout_seconds: u64,
    pub probe_timeout_seconds: u64,
}

fn unavailable(detail: &str) -> LlmError {
    LlmError::Unavailable(detail.to_string())
}

impl ModelPolicy {
    pub fn from_yaml(text: &str) -> Result<Self, LlmError> {
        if text.len() > 16 * 1024 {
            return Err(unavailable("LLM-YAML ist zu groß"));
        }
        // Parserfehler können Eingabewerte enthalten. Niemals ungeprüft loggen.
        let policy: Self = serde_saphyr::from_str(text)
            .map_err(|_| unavailable("LLM-YAML ist ungültig oder enthält unbekannte Felder"))?;
        if policy.schema_version != 1
            || policy.provider != "fireworks"
            || policy.family != "deepseek-flash"
            || policy.selection != "latest"
            || model_version(&policy.bootstrap_model).is_none()
            || !(60..=86_400).contains(&policy.refresh_seconds)
            || !(1..=3_600).contains(&policy.retry_seconds)
            || policy.retry_seconds > policy.refresh_seconds
            || !(policy.refresh_seconds..=604_800).contains(&policy.max_stale_seconds)
            || !(1..=60).contains(&policy.catalog_timeout_seconds)
            || !(1..=120).contains(&policy.probe_timeout_seconds)
        {
            return Err(unavailable("LLM-YAML verletzt die DeepSeek-Flash-Policy"));
        }
        Ok(policy)
    }

    pub fn load(path: &Path) -> Result<Self, LlmError> {
        use std::io::Read;
        let file = std::fs::File::open(path)
            .map_err(|_| unavailable("LLM-YAML fehlt oder ist nicht lesbar"))?;
        let mut text = String::new();
        file.take(16 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|_| unavailable("LLM-YAML ist nicht lesbar"))?;
        Self::from_yaml(&text)
    }

    pub fn allows(&self, model: &str) -> bool {
        self.allows_with_created(model, None)
    }

    fn allows_with_created(&self, model: &str, created: Option<i64>) -> bool {
        match (model_version(model), model_version(&self.bootstrap_model)) {
            (Some(version), Some(minimum)) => {
                if version.parts != minimum.parts {
                    return version.parts > minimum.parts;
                }
                if minimum.revision.is_none() {
                    true
                } else if version.revision.is_none() {
                    false
                } else {
                    revision_date(&version, created) >= revision_date(&minimum, None)
                }
            }
            _ => false,
        }
    }

    fn cache_family(&self) -> String {
        // Bindet auch Mindestversion, Fristen und Schema an den Cache. Eine
        // geänderte Policy darf keinen alten Freigabenachweis übernehmen.
        let encoded = serde_json::to_vec(self).expect("ModelPolicy ist serialisierbar");
        use std::fmt::Write;
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(encoded) {
            let _ = write!(digest, "{byte:02x}");
        }
        format!("{}:yaml-v1:{digest}", self.family)
    }
}

/// Versionen wie v4, v4p1, v4.1 und zukünftige v10p2. Keine Pro-, Lite-,
/// Preview-, Vision- oder Thinking-Abzweigung, keine bloße Präfixprüfung.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelVersion {
    parts: Vec<u32>,
    revision: Option<String>,
}

fn model_version(id: &str) -> Option<ModelVersion> {
    let rest = id.strip_prefix("accounts/fireworks/models/deepseek-v")?;
    let (version, suffix) = rest.split_once("-flash")?;
    if version.is_empty() || version.len() > 24 {
        return None;
    }
    let mut parts = version
        .split(['p', '.'])
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                part.parse::<u32>().ok()
            }
        })
        .collect::<Option<Vec<_>>>()?;
    if parts.len() > 4 || parts[0] == 0 {
        return None;
    }
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    let revision = if suffix.is_empty() {
        None
    } else {
        let digits = suffix.strip_prefix('-')?;
        if !matches!(digits.len(), 4 | 8) || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let valid = if digits.len() == 8 {
            NaiveDate::parse_from_str(digits, "%Y%m%d").is_ok()
        } else {
            NaiveDate::parse_from_str(&format!("2000{digits}"), "%Y%m%d").is_ok()
        };
        if !valid {
            return None;
        }
        Some(digits.to_owned())
    };
    Some(ModelVersion { parts, revision })
}

#[derive(Debug, Clone)]
struct ModelEntry {
    id: String,
    created: Option<i64>,
}

fn revision_date(version: &ModelVersion, created: Option<i64>) -> i64 {
    let Some(revision) = &version.revision else {
        return 0;
    };
    if revision.len() == 8 {
        return revision.parse().unwrap_or(0);
    }
    let today = Utc::now().date_naive();
    let mut year = created
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map_or(today.year(), |date| date.year());
    let month_day = revision.parse::<u32>().unwrap_or(0);
    if created.is_none() && month_day > today.month() * 100 + today.day() {
        year -= 1;
    }
    i64::from(year) * 10_000 + i64::from(month_day)
}

fn newest_first(entries: &mut Vec<ModelEntry>, policy: &ModelPolicy) {
    entries.retain(|entry| policy.allows_with_created(&entry.id, entry.created));
    // Ein einheitlicher Sortierschlüssel bleibt auch bei gemischten fehlenden
    // Zeitstempeln transitiv. Keine paarweise wechselnden Vergleichsregeln.
    entries.sort_by_cached_key(|entry| {
        let version = model_version(&entry.id).expect("vorher gefiltert");
        let date = if version.revision.is_some() {
            revision_date(&version, entry.created)
        } else {
            entry
                .created
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map_or(0, |date| {
                    i64::from(date.year()) * 10_000
                        + i64::from(date.month()) * 100
                        + i64::from(date.day())
                })
        };
        std::cmp::Reverse((
            version.parts,
            date,
            entry.created.unwrap_or(0),
            entry.id.clone(),
        ))
    });
    entries.dedup_by(|a, b| a.id == b.id);
}

#[derive(Clone)]
struct VerifiedModel {
    model: String,
    verified_at: i64,
}

#[derive(Default)]
struct RefreshState {
    next_attempt: Option<Instant>,
    rejected: HashMap<String, Instant>,
    cache_loaded: bool,
}

pub struct ModelResolver {
    policy: ModelPolicy,
    catalog_url: String,
    inference_url: String,
    client: reqwest::Client,
    current: RwLock<Option<VerifiedModel>>,
    refresh: Mutex<RefreshState>,
}

impl ModelResolver {
    pub fn new(policy: ModelPolicy) -> Result<Self, LlmError> {
        Self::with_urls(policy, CATALOG_URL, crate::selection::FIREWORKS_BASE_URL)
    }

    fn with_urls(policy: ModelPolicy, catalog: &str, inference: &str) -> Result<Self, LlmError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable("Modellkatalog-Client nicht verfügbar"))?;
        Ok(Self {
            policy,
            catalog_url: catalog.to_owned(),
            inference_url: inference.to_owned(),
            client,
            current: RwLock::new(None),
            refresh: Mutex::new(RefreshState::default()),
        })
    }

    fn current(&self) -> Option<VerifiedModel> {
        self.current
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn selected_model(&self) -> Option<String> {
        self.current().map(|model| model.model)
    }

    /// Nur Metadaten und die feste Probe verlassen den Prozess. Chattexte
    /// werden weder zum Auflösen noch zum Testen neuer Modelle verwendet.
    pub async fn resolve(
        &self,
        api_key: &str,
        rejected: Option<&str>,
        pool: Option<&PgPool>,
    ) -> Result<String, LlmError> {
        let mut state = self.refresh.lock().await;
        let now = Instant::now();
        state.rejected.retain(|_, at| {
            now.duration_since(*at) < Duration::from_secs(self.policy.refresh_seconds)
        });
        if let Some(rejected) = rejected {
            if !state.rejected.contains_key(rejected) {
                state.next_attempt = None;
                state.rejected.insert(rejected.to_owned(), now);
            }
        }
        let usable = self.current().filter(|entry| {
            !state.rejected.contains_key(&entry.model)
                && self.policy.allows(&entry.model)
                && Utc::now().timestamp().saturating_sub(entry.verified_at)
                    <= self.policy.max_stale_seconds as i64
        });
        // Ein anderer Aufrufer hat denselben 404 bereits geheilt.
        if let (Some(_), Some(cached)) = (rejected, &usable) {
            return Ok(cached.model.clone());
        }
        if state.next_attempt.is_some_and(|next| now < next) {
            return usable.map(|entry| entry.model).ok_or_else(|| {
                unavailable("Kein geprüftes Flash-Modell; erneute Prüfung nach Fehlerpause")
            });
        }
        // Vor dem Netzaufruf setzen, damit auch Abbruch/Timeout keine Sturmfolge auslöst.
        state.next_attempt = Some(now + Duration::from_secs(self.policy.retry_seconds));
        let catalog = tokio::time::timeout(
            Duration::from_secs(self.policy.catalog_timeout_seconds),
            self.fetch_models(api_key),
        )
        .await;
        let catalog_ok = matches!(&catalog, Ok(Ok(_)));
        let mut entries = match catalog {
            Ok(Ok(entries)) => entries,
            _ => {
                tracing::warn!(family = %self.policy.family,
                    "Modellkatalog nicht verfügbar; nur geprüfter Cache oder YAML-Startkandidat erlaubt");
                Vec::new()
            }
        };
        if !state.cache_loaded {
            state.cache_loaded = true;
            if let Some(pool) = pool {
                if let Some(model) = self.load_from_db(pool).await {
                    entries.push(ModelEntry {
                        id: model,
                        created: None,
                    });
                }
            }
        }
        entries.push(ModelEntry {
            id: self.policy.bootstrap_model.clone(),
            created: None,
        });
        if let Some(entry) = &usable {
            entries.push(ModelEntry {
                id: entry.model.clone(),
                created: None,
            });
        }
        newest_first(&mut entries, &self.policy);
        entries.retain(|entry| !state.rejected.contains_key(&entry.id));
        let mut probed = std::collections::HashSet::new();
        for entry in entries {
            if !probed.insert(entry.id.clone()) {
                continue;
            }
            if probed.len() > MAX_PROBES {
                break;
            }
            match self.probe(api_key, &entry.id).await {
                Ok(()) => {
                    let verified = VerifiedModel {
                        model: entry.id.clone(),
                        verified_at: Utc::now().timestamp(),
                    };
                    *self.current.write().unwrap_or_else(|p| p.into_inner()) = Some(verified);
                    state.next_attempt = Some(
                        Instant::now()
                            + Duration::from_secs(if catalog_ok {
                                self.policy.refresh_seconds
                            } else {
                                self.policy.retry_seconds
                            }),
                    );
                    if let Some(pool) = pool {
                        self.save_to_db(pool, &entry).await;
                    }
                    tracing::info!(model = %entry.id, family = %self.policy.family,
                        selection = "latest", catalog_ok,
                        "DeepSeek-Flash-Modell geprüft und aus YAML-Policy aufgelöst");
                    return Ok(entry.id);
                }
                Err(error) => {
                    let status = match error {
                        LlmError::Http { status, .. } => Some(status),
                        _ => None,
                    };
                    if matches!(status, Some(404 | 410)) {
                        state.rejected.insert(entry.id.clone(), Instant::now());
                    }
                    tracing::warn!(model = %entry.id, status, code = error.code(),
                        "Flash-Modellprobe fehlgeschlagen; Modell nicht übernommen");
                    // Ein vorübergehender Fehler beweist keine Abschaltung.
                    // Nicht auf eine ältere YAML-Version zurückstufen, solange
                    // der letzte geprüfte Stand noch gültig ist.
                    if !matches!(status, Some(404 | 410)) {
                        if let Some(cached) = &usable {
                            tracing::warn!(model = %cached.model,
                                "Modellprobe vorübergehend fehlgeschlagen; geprüfter Stand bleibt aktiv");
                            return Ok(cached.model.clone());
                        }
                    }
                    // Zugang/Rate-Limit ist kein Grund, mehrere Modelle abzufragen.
                    if matches!(status, Some(401 | 403 | 429)) {
                        break;
                    }
                }
            }
        }
        if let Some(entry) = usable.filter(|entry| !state.rejected.contains_key(&entry.model)) {
            tracing::warn!(model = %entry.model,
                "Modellrefresh fehlgeschlagen; letzter geprüfter Stand bleibt aktiv");
            return Ok(entry.model);
        }
        Err(unavailable(
            "Kein verfügbares, geprüftes DeepSeek-Flash-Modell",
        ))
    }

    async fn fetch_models(&self, api_key: &str) -> Result<Vec<ModelEntry>, LlmError> {
        let mut entries = Vec::new();
        let mut page_token = String::new();
        let mut tokens = std::collections::HashSet::new();
        for _ in 0..MAX_CATALOG_PAGES {
            let mut request = self
                .client
                .get(&self.catalog_url)
                .bearer_auth(api_key)
                .query(&[("pageSize", "200")]);
            if !page_token.is_empty() {
                request = request.query(&[("pageToken", page_token.as_str())]);
            }
            let mut response = request
                .send()
                .await
                .map_err(|_| unavailable("Modellkatalog nicht erreichbar"))?;
            if !response.status().is_success() {
                return Err(unavailable("Modellkatalog hat einen Fehlerstatus"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| unavailable("Modellkatalog unvollständig"))?
            {
                if bytes.len() + chunk.len() > MAX_CATALOG_BYTES {
                    return Err(unavailable("Modellkatalog überschreitet Größenlimit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            let body: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| unavailable("Modellkatalog ist kein JSON"))?;
            let models = body
                .get("models")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| unavailable("Modellkatalog hat ein unerwartetes Schema"))?;
            for model in models {
                if model
                    .get("supportsServerless")
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
                    || model.get("state").and_then(serde_json::Value::as_str) != Some("READY")
                {
                    continue;
                }
                if let Some(date) = model.get("deprecationDate") {
                    let deprecated = date
                        .get("year")
                        .and_then(serde_json::Value::as_i64)
                        .zip(date.get("month").and_then(serde_json::Value::as_u64))
                        .zip(date.get("day").and_then(serde_json::Value::as_u64))
                        .and_then(|((y, m), d)| {
                            NaiveDate::from_ymd_opt(
                                i32::try_from(y).ok()?,
                                u32::try_from(m).ok()?,
                                u32::try_from(d).ok()?,
                            )
                        });
                    if deprecated.is_some_and(|date| date <= Utc::now().date_naive()) {
                        continue;
                    }
                }
                if let Some(id) = model.get("name").and_then(serde_json::Value::as_str) {
                    let created = model
                        .get("createTime")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
                        .map(|date| date.timestamp());
                    entries.push(ModelEntry {
                        id: id.to_owned(),
                        created,
                    });
                }
            }
            page_token = body
                .get("nextPageToken")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if page_token.is_empty() {
                return Ok(entries);
            }
            if page_token.len() > 4096 || !tokens.insert(page_token.clone()) {
                return Err(unavailable("Modellkatalog wiederholt seine Seitennummer"));
            }
        }
        Err(unavailable("Modellkatalog überschreitet Seitenlimit"))
    }

    async fn probe(&self, api_key: &str, model: &str) -> Result<(), LlmError> {
        let endpoint = LlmEndpoint {
            provider: "fireworks",
            model: model.to_owned(),
            base_url: self.inference_url.clone(),
            api_key: Some(api_key.to_owned()),
        };
        let request = Request::simple(
            "This is a synthetic service health check. Return only the JSON object requested.",
            "Return exactly {\"ready\":true}.",
        )
        .json_object()
        .denken_aus()
        .strip_think()
        .max_tokens(32)
        .no_ledger();
        let response = crate::hub::call_endpoint(
            &endpoint,
            &request,
            None,
            Duration::from_secs(self.policy.probe_timeout_seconds),
        )
        .await?;
        let value: serde_json::Value = serde_json::from_str(response.text.trim())
            .map_err(|_| unavailable("Modellprobe liefert kein gültiges JSON"))?;
        if value != serde_json::json!({"ready": true}) {
            return Err(unavailable("Modellprobe liefert ein unerwartetes Ergebnis"));
        }
        Ok(())
    }

    async fn load_from_db(&self, pool: &PgPool) -> Option<String> {
        let query = sqlx::query_scalar::<_, String>(
            "SELECT model FROM llm_model_cache WHERE provider = 'fireworks' AND family = $1 \
             AND resolved_at BETWEEN now() - ($2 * interval '1 second') AND now()",
        )
        .bind(self.policy.cache_family())
        .bind(self.policy.max_stale_seconds as i64)
        .fetch_optional(pool);
        // Der Cache ist nur ein zusätzlicher Probekandidat, nie ein ungeprüfter Endpunkt.
        tokio::time::timeout(Duration::from_secs(2), query)
            .await
            .ok()?
            .ok()?
            .filter(|model| self.policy.allows(model))
    }

    async fn save_to_db(&self, pool: &PgPool, entry: &ModelEntry) {
        let query = sqlx::query(
            "INSERT INTO llm_model_cache (provider, family, model, model_created, resolved_at) \
             VALUES ('fireworks', $1, $2, $3, now()) \
             ON CONFLICT (provider, family) DO UPDATE SET model = EXCLUDED.model, \
             model_created = EXCLUDED.model_created, resolved_at = now()",
        )
        .bind(self.policy.cache_family())
        .bind(&entry.id)
        .bind(entry.created)
        .execute(pool);
        if !matches!(
            tokio::time::timeout(Duration::from_secs(2), query).await,
            Ok(Ok(_))
        ) {
            tracing::warn!("Geprüfter Modellname konnte nicht im Cache gespeichert werden");
        }
    }
}

/// Die Release-YAML liegt im bereits paketierten knowledge-Verzeichnis.
/// Kein eingebetteter Versions-Fallback und keine Modell-ENV-Überschreibung.
pub fn policy_path() -> Result<PathBuf, LlmError> {
    let cwd = std::env::current_dir().map_err(|_| unavailable("Arbeitsverzeichnis fehlt"))?;
    let mut candidates = vec![
        cwd.join("rust/knowledge/llm.yaml"),
        cwd.join("knowledge/llm.yaml"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        // Auch ausführbare Release-Werkzeuge außerhalb ihres Arbeitsverzeichnisses.
        for parent in exe.ancestors().skip(1).take(4) {
            candidates.push(parent.join("knowledge/llm.yaml"));
        }
    }
    for path in candidates {
        match path.try_exists() {
            Ok(true) => return Ok(path),
            Ok(false) => {}
            Err(_) => return Err(unavailable("LLM-YAML ist nicht zugänglich")),
        }
    }
    Err(unavailable("LLM-YAML fehlt: rust/knowledge/llm.yaml"))
}

pub fn global() -> Result<&'static ModelResolver, LlmError> {
    static INSTANCE: OnceLock<Result<ModelResolver, LlmError>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| ModelResolver::new(ModelPolicy::load(&policy_path()?)?))
        .as_ref()
        .map_err(Clone::clone)
}

pub fn configured_fireworks_model() -> &'static str {
    global()
        .map(|resolver| resolver.policy.bootstrap_model.as_str())
        .unwrap_or("")
}

pub fn allowed_fireworks_model(model: &str) -> bool {
    global().is_ok_and(|resolver| resolver.policy.allows(model))
}

pub fn resolved_fireworks_model() -> Option<String> {
    global().ok().and_then(ModelResolver::selected_model)
}

/// Beaufsichtigter Betriebsjob: sofort prüfen und anschließend in der
/// YAML-Fehlerpause nachsehen. Der gemeinsame Resolver drosselt erfolgreiche
/// Katalogläufe auf refresh_seconds, auch bei gleichzeitigem Chatverkehr.
pub async fn run_refresh_loop(pool: PgPool) {
    let Ok(resolver) = global() else {
        tracing::error!("Modellrefresh ohne gültige YAML-Policy nicht gestartet");
        return;
    };
    let Some(key) = crate::keys::fireworks_api_key() else {
        tracing::warn!("Modellrefresh ohne Fireworks-Schlüssel nicht gestartet");
        return;
    };
    let deadline = Duration::from_secs(
        resolver.policy.catalog_timeout_seconds
            + resolver.policy.probe_timeout_seconds * MAX_PROBES as u64
            + 5,
    );
    loop {
        match tokio::time::timeout(deadline, resolver.resolve(&key, None, Some(&pool))).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::error!(
                code = error.code(),
                "KI-Modellüberwachung: kein geprüftes DeepSeek-Flash-Modell verfügbar"
            ),
            Err(_) => tracing::error!("KI-Modellüberwachung: Gesamtfrist überschritten"),
        }
        tokio::time::sleep(Duration::from_secs(resolver.policy.retry_seconds)).await;
    }
}

#[cfg(test)]
mod tests;

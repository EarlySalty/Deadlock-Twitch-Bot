use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const LEDGER_BASE_DEFAULT: &str = "http://127.0.0.1:8901";
pub const PENDING_PATH_DEFAULT: &str = "data/steam_web_api/pending_observation.json";
const TOKEN_HEADER: &str = "X-Internal-Token";
const TOKEN_CHAIN: [&str; 4] = [
    "SERVERSYNC_INTERNAL_TOKEN",
    "MASTER_BROKER_TOKEN",
    "MAIN_BOT_INTERNAL_TOKEN",
    "TWITCH_INTERNAL_API_TOKEN",
];
const LEDGER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reservation {
    Granted(i64),
    Denied(DateTime<Utc>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    Clear,
    Confirmed(Option<DateTime<Utc>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    Unavailable,
    Rejected(u16),
    Malformed,
    PendingUnreadable,
    PendingUnwritable,
    PendingUnconfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub reservation_id: i64,
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<String>,
}

impl Observation {
    pub fn dispatched(reservation_id: i64) -> Self {
        Self {
            reservation_id,
            http_status: None,
            retry_after: None,
        }
    }

    pub fn from_response(reservation_id: i64, status: u16, retry_after: Option<&str>) -> Self {
        let retry_after = if status == 429 {
            retry_after.map(str::to_string)
        } else {
            None
        };
        Self {
            reservation_id,
            http_status: Some(status),
            retry_after,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PendingObservation {
    #[serde(flatten)]
    observation: Observation,
    ready: bool,
}

pub struct SteamWebApiLedger {
    base: Option<String>,
    token: Option<String>,
    caller: String,
    pending_path: PathBuf,
    http: reqwest::Client,
}

fn loopback_base(base: &str) -> Option<String> {
    let url = reqwest::Url::parse(base).ok()?;
    if url.scheme() != "http" {
        return None;
    }
    let loopback = match url.host()? {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(_) => false,
    };
    loopback.then(|| base.trim_end_matches('/').to_string())
}

fn valid_caller(caller: &str) -> bool {
    let mut chars = caller.chars();
    caller.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn token_from_env() -> Option<String> {
    TOKEN_CHAIN.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn parse_time(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

impl SteamWebApiLedger {
    pub fn from_env(caller: &str) -> Self {
        Self::new(
            LEDGER_BASE_DEFAULT,
            token_from_env(),
            caller,
            PENDING_PATH_DEFAULT,
        )
    }

    pub fn new(
        base: &str,
        token: Option<String>,
        caller: &str,
        pending_path: impl Into<PathBuf>,
    ) -> Self {
        let base = loopback_base(base);
        if base.is_none() {
            tracing::error!(
                "Steam-Web-API-Kontingent: Adresse ist nicht loopback, Abrufe gesperrt"
            );
        }
        let caller = if valid_caller(caller) {
            caller.to_string()
        } else {
            tracing::error!(
                "Steam-Web-API-Kontingent: ungültige Aufrufer-Kennung, Abrufe gesperrt"
            );
            String::new()
        };
        let http = reqwest::Client::builder()
            .timeout(LEDGER_TIMEOUT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok();
        Self {
            base: base.filter(|_| http.is_some()),
            token: token.filter(|value| !value.is_empty()),
            caller,
            pending_path: pending_path.into(),
            http: http.unwrap_or_default(),
        }
    }

    async fn post(&self, route: &str, body: Value) -> Result<(u16, Value), LedgerError> {
        let (Some(base), Some(token)) = (self.base.as_deref(), self.token.as_deref()) else {
            return Err(LedgerError::Unavailable);
        };
        let response = self
            .http
            .post(format!("{base}{route}"))
            .header(TOKEN_HEADER, token)
            .json(&body)
            .send()
            .await
            .map_err(|_| LedgerError::Unavailable)?;
        let status = response.status().as_u16();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        Ok((status, value))
    }

    pub async fn reserve(&self) -> Result<Reservation, LedgerError> {
        if self.caller.is_empty() {
            return Err(LedgerError::Unavailable);
        }
        let (status, body) = self
            .post(
                "/steam-web-api/reserve",
                json!({ "caller": self.caller, "caller_class": "standard" }),
            )
            .await?;
        if status != 200 {
            return Err(LedgerError::Rejected(status));
        }
        if body.get("ok") != Some(&Value::Bool(true)) {
            return Err(LedgerError::Malformed);
        }
        match body.get("granted") {
            Some(Value::Bool(true)) => body
                .get("reservation_id")
                .and_then(Value::as_i64)
                .filter(|id| *id > 0)
                .map(Reservation::Granted)
                .ok_or(LedgerError::Malformed),
            Some(Value::Bool(false)) => body
                .get("retry_at")
                .and_then(parse_time)
                .map(Reservation::Denied)
                .ok_or(LedgerError::Malformed),
            _ => Err(LedgerError::Malformed),
        }
    }

    async fn observe(&self, observation: &Observation) -> Result<Settlement, LedgerError> {
        let body = serde_json::to_value(observation).map_err(|_| LedgerError::Malformed)?;
        let (status, body) = self.post("/steam-web-api/observe", body).await?;
        match status {
            200 if body.get("ok") == Some(&Value::Bool(true)) => {
                let cooldown = match body.get("cooldown_until") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(parse_time(value).ok_or(LedgerError::Malformed)?),
                };
                Ok(Settlement::Confirmed(cooldown))
            }
            200 => Err(LedgerError::Malformed),
            409 => {
                tracing::warn!(
                    reservation_id = observation.reservation_id,
                    "Steam-Web-API-Kontingent: Beobachtung lag bereits abweichend vor"
                );
                Err(LedgerError::Rejected(409))
            }
            other => Err(LedgerError::Rejected(other)),
        }
    }

    async fn read_pending(&self) -> Result<Option<Observation>, LedgerError> {
        match tokio::fs::read(&self.pending_path).await {
            Ok(bytes) => {
                let pending: PendingObservation =
                    serde_json::from_slice(&bytes).map_err(|_| LedgerError::PendingUnreadable)?;
                if !pending.ready {
                    return Err(LedgerError::PendingUnconfirmed);
                }
                Ok(Some(pending.observation))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(LedgerError::PendingUnreadable),
        }
    }

    pub async fn record_dispatch(&self, reservation_id: i64) -> Result<(), LedgerError> {
        self.write_pending(&Observation::dispatched(reservation_id), false)
            .await
    }

    pub async fn record_pending(&self, observation: &Observation) -> Result<(), LedgerError> {
        self.write_pending(observation, true).await
    }

    async fn write_pending(
        &self,
        observation: &Observation,
        ready: bool,
    ) -> Result<(), LedgerError> {
        let bytes = serde_json::to_vec(&PendingObservation {
            observation: observation.clone(),
            ready,
        })
        .map_err(|_| LedgerError::PendingUnwritable)?;
        if let Some(dir) = self
            .pending_path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|_| LedgerError::PendingUnwritable)?;
        }
        let staging = self.pending_path.with_extension("tmp");
        tokio::fs::write(&staging, bytes)
            .await
            .map_err(|_| LedgerError::PendingUnwritable)?;
        tokio::fs::rename(&staging, &self.pending_path)
            .await
            .map_err(|_| LedgerError::PendingUnwritable)
    }

    pub async fn settle_pending(&self) -> Result<Settlement, LedgerError> {
        match self.read_pending().await? {
            Some(observation) => self.settle(&observation).await,
            None => Ok(Settlement::Clear),
        }
    }

    pub async fn settle(&self, observation: &Observation) -> Result<Settlement, LedgerError> {
        let settlement = self.observe(observation).await?;
        match tokio::fs::remove_file(&self.pending_path).await {
            Ok(()) => Ok(settlement),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(settlement),
            Err(_) => Err(LedgerError::PendingUnwritable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nur_loopback_ip_als_adresse() {
        assert_eq!(
            loopback_base("http://127.0.0.1:8901/").as_deref(),
            Some("http://127.0.0.1:8901")
        );
        assert!(loopback_base("http://[::1]:8901").is_some());
        assert!(loopback_base("http://localhost:8901").is_none());
        assert!(loopback_base("http://10.0.0.5:8901").is_none());
        assert!(loopback_base("https://127.0.0.1:8901").is_none());
    }

    #[test]
    fn aufrufer_kennung_nach_vertrag() {
        assert!(valid_caller("twitch_engagement_patches"));
        assert!(!valid_caller("Twitch"));
        assert!(!valid_caller("1abc"));
        assert!(!valid_caller(&"a".repeat(65)));
    }

    #[test]
    fn retry_after_nur_bei_429_und_unveraendert() {
        assert_eq!(
            Observation::from_response(1, 429, Some("90"))
                .retry_after
                .as_deref(),
            Some("90")
        );
        assert_eq!(
            Observation::from_response(1, 503, Some("90")).retry_after,
            None
        );
        let lang = "9".repeat(129);
        assert_eq!(
            Observation::from_response(1, 429, Some(&lang)).retry_after,
            Some(lang)
        );
        assert_eq!(
            serde_json::to_value(Observation::dispatched(7)).unwrap(),
            json!({ "reservation_id": 7, "http_status": null })
        );
    }

    #[tokio::test]
    async fn ohne_schluessel_keine_reservierung() {
        let ledger = SteamWebApiLedger::new(
            "http://127.0.0.1:1",
            None,
            "twitch_test",
            "/nonexistent/p.json",
        );
        assert_eq!(ledger.reserve().await, Err(LedgerError::Unavailable));
    }
}

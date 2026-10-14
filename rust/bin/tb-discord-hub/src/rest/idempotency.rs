use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use tokio::sync::watch;

pub(crate) const CONFLICT_MESSAGE: &str = "idempotency key already used with different payload";

#[derive(Debug, Clone)]
pub(crate) struct IdempotencyConfig {
    pub ttl: Duration,
    pub max_entries: usize,
    pub inflight_ttl: Duration,
    pub waiter_timeout: Duration,
}

impl Default for IdempotencyConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(600),
            max_entries: 1024,
            inflight_ttl: Duration::from_secs(600),
            waiter_timeout: Duration::from_secs(15),
        }
    }
}

type Outcome = Option<(u16, Value)>;

struct Record {
    payload_hash: String,
    status: u16,
    body: Value,
    created: Instant,
}

struct Inflight {
    payload_hash: String,
    tx: watch::Sender<Outcome>,
    rx: watch::Receiver<Outcome>,
    created: Instant,
}

#[derive(Default)]
struct Inner {
    records: HashMap<String, Record>,
    inflight: HashMap<String, Inflight>,
}

pub(crate) enum Decision {
    Execute,
    Cached(u16, Value),
    Pending(watch::Receiver<Outcome>),
}

pub(crate) struct IdempotencyStore {
    config: IdempotencyConfig,
    inner: tokio::sync::Mutex<Inner>,
}

impl IdempotencyStore {
    pub(crate) fn new(config: IdempotencyConfig) -> Self {
        Self {
            config,
            inner: tokio::sync::Mutex::new(Inner::default()),
        }
    }

    pub(crate) fn waiter_timeout(&self) -> Duration {
        self.config.waiter_timeout
    }

    pub(crate) async fn begin(&self, cache_key: &str, payload_hash: &str) -> Result<Decision, ()> {
        let now = Instant::now();
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner, &self.config, now);
        if let Some(record) = inner.records.get(cache_key) {
            if record.payload_hash != payload_hash {
                return Err(());
            }
            return Ok(Decision::Cached(record.status, record.body.clone()));
        }
        if let Some(inflight) = inner.inflight.get(cache_key) {
            if inflight.payload_hash != payload_hash {
                return Err(());
            }
            return Ok(Decision::Pending(inflight.rx.clone()));
        }
        let (tx, rx) = watch::channel(None);
        inner.inflight.insert(
            cache_key.to_string(),
            Inflight {
                payload_hash: payload_hash.to_string(),
                tx,
                rx,
                created: now,
            },
        );
        Ok(Decision::Execute)
    }

    pub(crate) async fn settle(
        &self,
        cache_key: &str,
        payload_hash: &str,
        status: u16,
        body: &Value,
        cache_response: bool,
    ) {
        let now = Instant::now();
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner, &self.config, now);
        if cache_response {
            inner.records.insert(
                cache_key.to_string(),
                Record {
                    payload_hash: payload_hash.to_string(),
                    status,
                    body: body.clone(),
                    created: now,
                },
            );
            while inner.records.len() > self.config.max_entries {
                let Some(oldest) = inner
                    .records
                    .iter()
                    .min_by_key(|(_, r)| r.created)
                    .map(|(k, _)| k.clone())
                else {
                    break;
                };
                inner.records.remove(&oldest);
            }
        }
        if let Some(inflight) = inner.inflight.remove(cache_key) {
            let _ = inflight.tx.send(Some((status, body.clone())));
        }
    }

    fn prune(inner: &mut Inner, config: &IdempotencyConfig, now: Instant) {
        inner
            .records
            .retain(|_, r| now.duration_since(r.created) <= config.ttl);
        inner
            .inflight
            .retain(|_, s| now.duration_since(s.created) <= config.inflight_ttl);
    }
}

pub(crate) fn payload_hash(payload: &Map<String, Value>) -> String {
    use sha2::Digest;
    let sorted: BTreeMap<&String, &Value> = payload.iter().collect();
    let encoded = serde_json::to_string(&sorted).unwrap_or_default();
    hex::encode(sha2::Sha256::digest(encoded.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store(max: usize) -> IdempotencyStore {
        IdempotencyStore::new(IdempotencyConfig {
            max_entries: max,
            ..IdempotencyConfig::default()
        })
    }

    #[tokio::test]
    async fn cache_konflikt_und_verdraengung() {
        let s = store(1);
        assert!(matches!(s.begin("a:k1", "h1").await, Ok(Decision::Execute)));
        s.settle("a:k1", "h1", 200, &json!({"ok": true}), true)
            .await;
        assert!(matches!(
            s.begin("a:k1", "h1").await,
            Ok(Decision::Cached(200, _))
        ));
        assert!(s.begin("a:k1", "anders").await.is_err());
        assert!(matches!(s.begin("a:k2", "h2").await, Ok(Decision::Execute)));
        s.settle("a:k2", "h2", 200, &json!({"ok": true}), true)
            .await;
        assert!(matches!(s.begin("a:k1", "h1").await, Ok(Decision::Execute)));
    }

    #[tokio::test]
    async fn fehler_werden_nicht_gespeichert() {
        let s = store(8);
        let _ = s.begin("a:k", "h").await;
        s.settle("a:k", "h", 502, &json!({"ok": false}), false)
            .await;
        assert!(matches!(s.begin("a:k", "h").await, Ok(Decision::Execute)));
    }
}

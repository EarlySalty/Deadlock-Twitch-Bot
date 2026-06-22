//! `RaidObservabilityService` — Flow-ID-Generierung, In-Memory-Counter,
//! Event-Payload-Bau und Emission.
//!
//! Parität zu Pythons `RaidObservabilityService`
//! (`bot/raid/observability.py:59-179`). Der Service ist thread-safe (interne
//! Mutabilität via `Mutex`/`AtomicU64`), damit er hinter einem `Arc` geteilt
//! werden kann. Die tatsächliche Persistenz erfolgt über einen austauschbaren
//! `EventSink` (im Prod der `ObservabilityWriter`).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tb_domain::normalize_twitch_login;

use crate::event::ObservabilityEvent;

/// Senke, die ein fertig gebautes Event entgegennimmt (z. B. der DB-Writer).
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &ObservabilityEvent);
}

/// Zeitquelle in Millisekunden seit Epoch (für Tests injizierbar).
pub type MillisSource = Arc<dyn Fn() -> u64 + Send + Sync>;

fn system_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Service für Raid-Flow-Observability.
pub struct RaidObservabilityService {
    event_sink: Option<Arc<dyn EventSink>>,
    millis_source: MillisSource,
    sequence: AtomicU64,
    counters: Mutex<HashMap<String, i64>>,
}

impl RaidObservabilityService {
    /// Erzeugt einen Service mit optionaler Event-Senke und Systemzeit.
    pub fn new(event_sink: Option<Arc<dyn EventSink>>) -> Self {
        Self {
            event_sink,
            millis_source: Arc::new(system_millis),
            sequence: AtomicU64::new(0),
            counters: Mutex::new(HashMap::new()),
        }
    }

    /// Wie `new`, aber mit injizierter Millisekunden-Zeitquelle (Tests).
    pub fn with_millis_source(
        event_sink: Option<Arc<dyn EventSink>>,
        millis_source: MillisSource,
    ) -> Self {
        Self {
            event_sink,
            millis_source,
            sequence: AtomicU64::new(0),
            counters: Mutex::new(HashMap::new()),
        }
    }

    /// Generiert eine monoton steigende Flow-ID `{prefix}-{millis}-{seq}`
    /// (Python `next_flow_id`).
    pub fn next_flow_id(&self, prefix: &str) -> String {
        let normalized = {
            let p = prefix.trim().to_lowercase();
            if p.is_empty() {
                "raid".to_string()
            } else {
                p
            }
        };
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let millis = (self.millis_source)();
        format!("{normalized}-{millis}-{seq}")
    }

    /// Erhöht einen In-Memory-Counter und gibt den neuen Wert zurück
    /// (Python `increment_counter`). Leerer Name → no-op (0).
    pub fn increment_counter(&self, name: &str, amount: i64) -> i64 {
        let key = name.trim();
        if key.is_empty() {
            return 0;
        }
        let mut counters = match self.counters.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = counters.entry(key.to_string()).or_insert(0);
        *entry += amount;
        *entry
    }

    /// Snapshot aller Counter (Python `counters`).
    pub fn counters(&self) -> HashMap<String, i64> {
        match self.counters.lock() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Baut ein Event-Payload mit normalisierten Logins/IDs (Python
    /// `build_event_payload`).
    #[allow(clippy::too_many_arguments)]
    pub fn build_event_payload(
        &self,
        flow_type: &str,
        flow_id: &str,
        step: &str,
        decision: &str,
        from_broadcaster_login: Option<&str>,
        from_broadcaster_id: Option<&str>,
        to_broadcaster_login: Option<&str>,
        to_broadcaster_id: Option<&str>,
        details: BTreeMap<String, Value>,
    ) -> ObservabilityEvent {
        ObservabilityEvent {
            flow_type: non_empty(flow_type, "raid"),
            flow_id: flow_id.trim().to_string(),
            step: non_empty(step, "event"),
            decision: non_empty(decision, "unknown"),
            from_broadcaster_login: normalize_login_opt(from_broadcaster_login),
            from_broadcaster_id: normalize_identifier(from_broadcaster_id),
            to_broadcaster_login: normalize_login_opt(to_broadcaster_login),
            to_broadcaster_id: normalize_identifier(to_broadcaster_id),
            details,
        }
    }

    /// Baut ein Event und leitet es an die Senke weiter (Python `emit_event`).
    #[allow(clippy::too_many_arguments)]
    pub fn emit_event(
        &self,
        flow_type: &str,
        flow_id: &str,
        step: &str,
        decision: &str,
        from_broadcaster_login: Option<&str>,
        from_broadcaster_id: Option<&str>,
        to_broadcaster_login: Option<&str>,
        to_broadcaster_id: Option<&str>,
        details: BTreeMap<String, Value>,
    ) -> ObservabilityEvent {
        let event = self.build_event_payload(
            flow_type,
            flow_id,
            step,
            decision,
            from_broadcaster_login,
            from_broadcaster_id,
            to_broadcaster_login,
            to_broadcaster_id,
            details,
        );
        if let Some(sink) = &self.event_sink {
            sink.emit(&event);
        }
        event
    }
}

fn non_empty(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_login_opt(raw: Option<&str>) -> Option<String> {
    raw.and_then(normalize_twitch_login)
}

fn normalize_identifier(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct RecordingSink {
        events: StdMutex<Vec<ObservabilityEvent>>,
    }

    impl EventSink for RecordingSink {
        fn emit(&self, event: &ObservabilityEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    fn fixed_millis(v: u64) -> MillisSource {
        Arc::new(move || v)
    }

    #[test]
    fn next_flow_id_increments_and_formats() {
        let svc = RaidObservabilityService::with_millis_source(None, fixed_millis(1000));
        assert_eq!(svc.next_flow_id("Raid"), "raid-1000-1");
        assert_eq!(svc.next_flow_id(""), "raid-1000-2");
        assert_eq!(svc.next_flow_id("Recruit"), "recruit-1000-3");
    }

    #[test]
    fn increment_counter_accumulates_and_skips_empty() {
        let svc = RaidObservabilityService::new(None);
        assert_eq!(svc.increment_counter("raid_flow_started_total", 1), 1);
        assert_eq!(svc.increment_counter("raid_flow_started_total", 2), 3);
        assert_eq!(svc.increment_counter("  ", 5), 0);
        assert_eq!(svc.counters().get("raid_flow_started_total"), Some(&3));
    }

    #[test]
    fn build_event_payload_normalizes_logins_and_defaults() {
        let svc = RaidObservabilityService::new(None);
        let event = svc.build_event_payload(
            "",
            "  raid-1  ",
            "",
            "",
            Some("@From_Login"),
            Some("  111  "),
            Some(""),
            None,
            BTreeMap::new(),
        );
        assert_eq!(event.flow_type, "raid");
        assert_eq!(event.flow_id, "raid-1");
        assert_eq!(event.step, "event");
        assert_eq!(event.decision, "unknown");
        assert_eq!(event.from_broadcaster_login.as_deref(), Some("from_login"));
        assert_eq!(event.from_broadcaster_id.as_deref(), Some("111"));
        assert_eq!(event.to_broadcaster_login, None);
        assert_eq!(event.to_broadcaster_id, None);
    }

    #[test]
    fn emit_event_forwards_to_sink() {
        let sink = Arc::new(RecordingSink::default());
        let svc = RaidObservabilityService::new(Some(sink.clone()));
        svc.emit_event(
            "raid",
            "raid-1",
            "execute",
            "success",
            None,
            None,
            Some("target"),
            Some("999"),
            BTreeMap::new(),
        );
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity_login(), "target");
    }
}

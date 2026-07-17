//! `AnalyticsObservabilityService` — Counter, Decision-Logging und
//! Diagnose-Snapshot für Analytics-Flows (chatters/subscriptions/ads/followers).
//!
//! Parität zu Pythons Analytics-Observability-Subsystem
//! (`bot/analytics/mixin.py:411-676`): `_increment_analytics_observability_counter`,
//! `_next_analytics_observability_flow_id`, `_log_analytics_decision`
//! (schreibt `flow_type='analytics' step='terminal_decision'` über den
//! Event-Writer) und `get_analytics_observability_snapshot`.
//!
//! Der Service hält die In-Memory-Counter und die letzten Diagnose-Samples
//! (chatters/followers/decision) und emittiert das Terminal-Decision-Event an
//! eine austauschbare Senke (im Prod der `ObservabilityWriter`).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::event::ObservabilityEvent;
use crate::raid_service::{EventSink, MillisSource};

/// Eingabe für eine Analytics-Terminal-Entscheidung (Python-Argumente von
/// `_log_analytics_decision`).
#[derive(Debug, Clone, Default)]
pub struct AnalyticsDecision {
    pub flow_id: String,
    /// Flow-Name (`chatters`/`subscriptions`/`ads`/`followers`); wird kleingeschrieben.
    pub flow: String,
    pub login: String,
    pub session_id: Option<i64>,
    pub decision: String,
    pub reason: String,
    pub request_attempted: Option<bool>,
    pub request_result: String,
    pub http_status: Option<i64>,
    /// Beliebige Scope-Status-Felder (JSON-Objekt).
    pub scope_state: BTreeMap<String, Value>,
    /// Beliebige Runtime-Status-Felder (JSON-Objekt).
    pub runtime_state: BTreeMap<String, Value>,
    /// Zusätzliche Detailfelder.
    pub extra: BTreeMap<String, Value>,
}

/// Diagnose-Snapshot (Python `get_analytics_observability_snapshot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsObservabilitySnapshot {
    pub runtime_available: bool,
    pub chat_bot_available: bool,
    pub bot_token_manager_available: bool,
    pub counters: HashMap<String, i64>,
    pub last_chatters_diagnostic: Option<Value>,
    pub last_followers_diagnostic: Option<Value>,
    pub last_decision_sample: Option<Value>,
}

fn system_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct Diagnostics {
    last_chatters: Option<Value>,
    last_followers: Option<Value>,
    last_decision: Option<Value>,
}

/// Service für Analytics-Observability.
pub struct AnalyticsObservabilityService {
    event_sink: Option<Arc<dyn EventSink>>,
    millis_source: MillisSource,
    sequence: AtomicU64,
    counters: Mutex<HashMap<String, i64>>,
    diagnostics: Mutex<Diagnostics>,
    runtime_available: bool,
    chat_bot_available: bool,
    bot_token_manager_available: bool,
}

impl AnalyticsObservabilityService {
    /// Erzeugt den Service. Die Runtime-Verfügbarkeits-Flags spiegeln Pythons
    /// `bool(self.api)` etc. wider und werden vom Aufrufer (Composition-Root)
    /// gesetzt.
    pub fn new(
        event_sink: Option<Arc<dyn EventSink>>,
        runtime_available: bool,
        chat_bot_available: bool,
        bot_token_manager_available: bool,
    ) -> Self {
        Self {
            event_sink,
            millis_source: Arc::new(system_millis),
            sequence: AtomicU64::new(0),
            counters: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(Diagnostics::default()),
            runtime_available,
            chat_bot_available,
            bot_token_manager_available,
        }
    }

    /// Wie `new`, aber mit injizierter Zeitquelle (Tests).
    pub fn with_millis_source(
        event_sink: Option<Arc<dyn EventSink>>,
        millis_source: MillisSource,
    ) -> Self {
        Self {
            event_sink,
            millis_source,
            sequence: AtomicU64::new(0),
            counters: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(Diagnostics::default()),
            runtime_available: false,
            chat_bot_available: false,
            bot_token_manager_available: false,
        }
    }

    /// Erhöht einen Counter; leerer Name → no-op (Python
    /// `_increment_analytics_observability_counter`).
    pub fn increment_counter(&self, name: &str, amount: i64) -> i64 {
        let key = name.trim();
        if key.is_empty() {
            return 0;
        }
        let mut counters = lock(&self.counters);
        let entry = counters.entry(key.to_string()).or_insert(0);
        *entry += amount;
        *entry
    }

    /// Generiert eine Flow-ID `{prefix}-{millis}-{seq}` (Python
    /// `_next_analytics_observability_flow_id`).
    pub fn next_flow_id(&self, prefix: &str) -> String {
        let normalized = {
            let p = prefix.trim().to_lowercase();
            if p.is_empty() {
                "analytics".to_string()
            } else {
                p
            }
        };
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let millis = (self.millis_source)();
        format!("{normalized}-{millis}-{seq}")
    }

    /// Protokolliert eine Terminal-Entscheidung: aktualisiert die Diagnose-Samples
    /// und emittiert ein `flow_type='analytics' step='terminal_decision'`-Event an
    /// die Senke. Gibt den (normalisierten) Detail-Payload zurück (Python
    /// `_log_analytics_decision`).
    pub fn log_decision(&self, decision: AnalyticsDecision) -> BTreeMap<String, Value> {
        let payload = build_payload(&decision);
        let flow_key = decision.flow.trim().to_lowercase();
        self.store_diagnostic(&flow_key, &payload);

        if let Some(sink) = &self.event_sink {
            let event = ObservabilityEvent {
                flow_type: "analytics".to_string(),
                flow_id: payload
                    .get("flow_id")
                    .and_then(string_or_none)
                    .unwrap_or_default(),
                step: "terminal_decision".to_string(),
                decision: payload
                    .get("decision")
                    .and_then(string_or_none)
                    .unwrap_or_else(|| "unknown".to_string()),
                from_broadcaster_login: None,
                from_broadcaster_id: None,
                to_broadcaster_login: None,
                to_broadcaster_id: None,
                details: payload.clone(),
            };
            // Python setzt entity_login=login, entity_id=session_id explizit;
            // wir transportieren sie über die to_broadcaster_*-Felder, damit
            // `entity_login()`/`entity_id()` exakt diese Werte liefern.
            let event = with_entity(
                event,
                payload.get("login").and_then(string_or_none),
                decision.session_id.map(|s| s.to_string()),
            );
            sink.emit(&event);
        }
        payload
    }

    /// Liefert den Diagnose-Snapshot (Python `get_analytics_observability_snapshot`).
    pub fn snapshot(&self) -> AnalyticsObservabilitySnapshot {
        let diag = lock(&self.diagnostics);
        AnalyticsObservabilitySnapshot {
            runtime_available: self.runtime_available,
            chat_bot_available: self.chat_bot_available,
            bot_token_manager_available: self.bot_token_manager_available,
            counters: lock(&self.counters).clone(),
            last_chatters_diagnostic: diag.last_chatters.clone(),
            last_followers_diagnostic: diag.last_followers.clone(),
            last_decision_sample: diag.last_decision.clone(),
        }
    }

    fn store_diagnostic(&self, flow_key: &str, payload: &BTreeMap<String, Value>) {
        let value = Value::Object(payload.clone().into_iter().collect());
        let mut diag = lock(&self.diagnostics);
        diag.last_decision = Some(value.clone());
        if flow_key == "chatters" {
            diag.last_chatters = Some(value.clone());
        }
        if flow_key.contains("followers") {
            diag.last_followers = Some(value);
        }
    }
}

/// Setzt entity_login/entity_id eines Events so, dass `entity_login()`/
/// `entity_id()` die gewünschten Werte liefern (über die to_broadcaster_*-Felder).
fn with_entity(
    mut event: ObservabilityEvent,
    login: Option<String>,
    entity_id: Option<String>,
) -> ObservabilityEvent {
    event.to_broadcaster_login = login.filter(|s| !s.is_empty());
    event.to_broadcaster_id = entity_id.filter(|s| !s.is_empty());
    event
}

fn build_payload(decision: &AnalyticsDecision) -> BTreeMap<String, Value> {
    let mut payload: BTreeMap<String, Value> = BTreeMap::new();
    payload.insert("flow_id".into(), opt_string(decision.flow_id.trim()));
    payload.insert(
        "flow".into(),
        json!(non_empty_lower(&decision.flow, "analytics")),
    );
    payload.insert(
        "login".into(),
        opt_string(&normalize_login(&decision.login)),
    );
    payload.insert(
        "session_id".into(),
        decision.session_id.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    payload.insert(
        "decision".into(),
        json!(non_empty(&decision.decision, "unknown")),
    );
    payload.insert(
        "reason".into(),
        json!(non_empty(&decision.reason, "unknown")),
    );
    payload.insert(
        "request_attempted".into(),
        decision
            .request_attempted
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
    );
    payload.insert(
        "request_result".into(),
        json!(non_empty(&decision.request_result, "unknown")),
    );
    payload.insert(
        "http_status".into(),
        decision
            .http_status
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
    );
    payload.insert(
        "scope_state".into(),
        Value::Object(decision.scope_state.clone().into_iter().collect()),
    );
    payload.insert(
        "runtime_state".into(),
        Value::Object(decision.runtime_state.clone().into_iter().collect()),
    );
    for (k, v) in &decision.extra {
        payload.insert(k.clone(), v.clone());
    }
    payload
}

fn normalize_login(login: &str) -> String {
    login
        .trim()
        .to_lowercase()
        .trim_start_matches('#')
        .to_string()
}

fn non_empty(value: &str, fallback: &str) -> String {
    let t = value.trim();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t.to_string()
    }
}

fn non_empty_lower(value: &str, fallback: &str) -> String {
    let t = value.trim().to_lowercase();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t
    }
}

fn opt_string(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    }
}

fn string_or_none(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
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

    fn decision() -> AnalyticsDecision {
        AnalyticsDecision {
            flow_id: "analytics-1".into(),
            flow: "Chatters".into(),
            login: "#StreamerName".into(),
            session_id: Some(42),
            decision: "success".into(),
            reason: "bot_path_success".into(),
            request_attempted: Some(true),
            request_result: "success".into(),
            http_status: Some(200),
            scope_state: BTreeMap::new(),
            runtime_state: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn counter_accumulates_and_skips_empty() {
        let svc = AnalyticsObservabilityService::new(None, true, false, false);
        assert_eq!(
            svc.increment_counter("analytics_chatters_success_total", 1),
            1
        );
        assert_eq!(
            svc.increment_counter("analytics_chatters_success_total", 1),
            2
        );
        assert_eq!(svc.increment_counter("", 9), 0);
    }

    #[test]
    fn flow_id_increments() {
        let svc = AnalyticsObservabilityService::with_millis_source(None, Arc::new(|| 7000));
        assert_eq!(svc.next_flow_id("Chatters"), "chatters-7000-1");
        assert_eq!(svc.next_flow_id(""), "analytics-7000-2");
    }

    #[test]
    fn log_decision_normalizes_payload() {
        let svc = AnalyticsObservabilityService::new(None, true, true, true);
        let payload = svc.log_decision(decision());
        assert_eq!(payload.get("flow"), Some(&json!("chatters")));
        assert_eq!(payload.get("login"), Some(&json!("streamername")));
        assert_eq!(payload.get("session_id"), Some(&json!(42)));
        assert_eq!(payload.get("decision"), Some(&json!("success")));
    }

    #[test]
    fn log_decision_emits_terminal_decision_event() {
        let sink = Arc::new(RecordingSink::default());
        let svc = AnalyticsObservabilityService::new(Some(sink.clone()), true, true, true);
        svc.log_decision(decision());
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].flow_type, "analytics");
        assert_eq!(events[0].step, "terminal_decision");
        assert_eq!(events[0].entity_login(), "streamername");
        assert_eq!(events[0].entity_id(), "42");
    }

    #[test]
    fn snapshot_reports_counters_and_last_samples() {
        let svc = AnalyticsObservabilityService::new(None, true, false, true);
        svc.increment_counter("c", 3);
        svc.log_decision(decision());
        let snap = svc.snapshot();
        assert!(snap.runtime_available);
        assert!(!snap.chat_bot_available);
        assert!(snap.bot_token_manager_available);
        assert_eq!(snap.counters.get("c"), Some(&3));
        assert!(snap.last_chatters_diagnostic.is_some());
        assert!(snap.last_followers_diagnostic.is_none());
        assert!(snap.last_decision_sample.is_some());
    }

    #[test]
    fn followers_flow_updates_followers_diagnostic() {
        let svc = AnalyticsObservabilityService::new(None, true, true, true);
        let mut d = decision();
        d.flow = "followers".into();
        svc.log_decision(d);
        let snap = svc.snapshot();
        assert!(snap.last_followers_diagnostic.is_some());
        assert!(snap.last_chatters_diagnostic.is_none());
    }
}

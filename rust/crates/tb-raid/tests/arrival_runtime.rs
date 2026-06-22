//! Test der Arrival-Runtime: echte Korrelations-Engine erzeugt Pläne, ein
//! Recording-Sink prüft, dass die Runtime die richtigen Effekte dispatcht.
//! Kein DB-Zugriff.

use std::sync::{Arc, Mutex};

use serde_json::json;
use tb_observability::{EventSink, MillisSource, ObservabilityEvent, RaidObservabilityService};
use tb_raid::arrival_runtime::{RaidArrivalRuntime, RaidArrivalSink};
use tb_raid::pending_raids::PendingRaid;
use tb_raid::signal_correlation::{
    ChatNotificationInput, ChatUnraidInput, RaidArrivalInput, RaidSignalCorrelationService,
};

#[derive(Default)]
struct RecordingSink {
    calls: Mutex<Vec<String>>,
}
impl RecordingSink {
    fn names(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[derive(Default)]
struct RecordingObservabilitySink {
    events: Mutex<Vec<ObservabilityEvent>>,
}
impl RecordingObservabilitySink {
    fn events(&self) -> Vec<ObservabilityEvent> {
        self.events.lock().unwrap().clone()
    }
}
impl EventSink for RecordingObservabilitySink {
    fn emit(&self, event: &ObservabilityEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

fn fixed_millis(value: u64) -> MillisSource {
    Arc::new(move || value)
}

#[async_trait::async_trait]
impl RaidArrivalSink for RecordingSink {
    #[allow(clippy::too_many_arguments)]
    async fn record_secondary_signal(
        &self,
        _s: &str,
        _fl: &str,
        _fi: Option<&str>,
        _tl: &str,
        _ti: &str,
        _v: i32,
        _u: bool,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push("record_secondary_signal".into());
    }
    async fn record_pending_observation(
        &self,
        _p: &PendingRaid,
        _s: &str,
        status: &str,
        _r: Option<&str>,
        _d: Option<&str>,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("record_pending_observation:{status}"));
    }
    async fn store_pending_raid(&self, _p: &PendingRaid) {
        self.calls.lock().unwrap().push("store_pending_raid".into());
    }
    async fn store_orphan_chat_notification(
        &self,
        _ti: &str,
        _tl: &str,
        _fi: Option<&str>,
        _fl: &str,
        _v: i32,
        _m: Option<&str>,
        _e: Option<&str>,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push("store_orphan_chat_notification".into());
    }
    async fn confirm_pending_raid(
        &self,
        _s: &str,
        _ti: &str,
        _tl: &str,
        _fl: &str,
        _fi: Option<&str>,
        _v: i32,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push("confirm_pending_raid".into());
    }
    async fn mark_manual_raid_started(&self, _k: &str, _t: f64) {
        self.calls
            .lock()
            .unwrap()
            .push("mark_manual_raid_started".into());
    }
    async fn record_independent_raid_arrival(
        &self,
        _s: &str,
        _fl: &str,
        _fi: Option<&str>,
        _tl: &str,
        _ti: &str,
        _v: i32,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push("record_independent_raid_arrival".into());
    }
}

fn arrival_input(pending: Option<PendingRaid>, from: &str) -> RaidArrivalInput {
    RaidArrivalInput {
        to_broadcaster_id: "200".into(),
        to_broadcaster_login: "dst".into(),
        from_broadcaster_login: from.into(),
        from_broadcaster_id: Some("100".into()),
        viewer_count: 42,
        pending_raid: pending,
        recent_arrival_present: false,
        independent_manual_detected: false,
        manual_raid_source_key: None,
    }
}

#[tokio::test]
async fn matched_pending_dispatcht_store_und_confirm() {
    let plan = RaidSignalCorrelationService
        .plan_raid_arrival(arrival_input(Some(PendingRaid::new("src", "200")), "src"));
    let sink = Arc::new(RecordingSink::default());
    RaidArrivalRuntime::new(sink.clone())
        .execute_plan(&plan)
        .await;

    let names = sink.names();
    assert!(names.contains(&"record_pending_observation:matched_pending".to_string()));
    assert!(names.contains(&"store_pending_raid".to_string()));
    assert!(
        names.contains(&"confirm_pending_raid".to_string()),
        "Match → confirm"
    );
}

#[tokio::test]
async fn mismatch_pending_dispatcht_store_aber_kein_confirm() {
    // Pending kommt von anderem Quell-Login → mismatch.
    let plan = RaidSignalCorrelationService.plan_raid_arrival(arrival_input(
        Some(PendingRaid::new("anderer", "200")),
        "src",
    ));
    let sink = Arc::new(RecordingSink::default());
    RaidArrivalRuntime::new(sink.clone())
        .execute_plan(&plan)
        .await;

    let names = sink.names();
    assert!(names
        .iter()
        .any(|n| n.starts_with("record_pending_observation")));
    assert!(names.contains(&"store_pending_raid".to_string()));
    assert!(
        !names.contains(&"confirm_pending_raid".to_string()),
        "Mismatch → KEIN confirm"
    );
}

#[tokio::test]
async fn kein_pending_kein_manual_dispatcht_nichts() {
    let plan = RaidSignalCorrelationService.plan_raid_arrival(arrival_input(None, "src"));
    let sink = Arc::new(RecordingSink::default());
    RaidArrivalRuntime::new(sink.clone())
        .execute_plan(&plan)
        .await;
    assert!(
        sink.names().is_empty(),
        "no_pending ohne Manual → leerer Plan"
    );
}

// --- B7-01: chat.notification-Raidmeldung → Arrival-Pfad genau 1× ---

fn chat_notification_input(pending: Option<PendingRaid>, from: &str) -> ChatNotificationInput {
    ChatNotificationInput {
        to_broadcaster_id: "200".into(),
        to_broadcaster_login: "dst".into(),
        from_broadcaster_login: from.into(),
        from_broadcaster_id: Some("100".into()),
        viewer_count: 42,
        message_id: Some("msg-1".into()),
        event_timestamp: None,
        pending_raid: pending,
        recent_arrival_present: false,
    }
}

#[tokio::test]
async fn chat_notification_match_confirmt_genau_einmal() {
    // Eingehende chat.notification-Raidmeldung mit passendem Pending → der
    // Arrival-Pfad (confirm_pending_raid) wird genau 1× dispatcht.
    let plan = RaidSignalCorrelationService.plan_chat_notification(chat_notification_input(
        Some(PendingRaid::new("src", "200")),
        "src",
    ));
    let sink = Arc::new(RecordingSink::default());
    RaidArrivalRuntime::new(sink.clone())
        .execute_plan(&plan)
        .await;

    let names = sink.names();
    let confirms = names
        .iter()
        .filter(|n| n.as_str() == "confirm_pending_raid")
        .count();
    assert_eq!(confirms, 1, "chat.notification-Match → genau 1× confirm");
    assert!(!names.contains(&"store_orphan_chat_notification".to_string()));
}

#[tokio::test]
async fn chat_notification_ohne_pending_ist_orphan_kein_confirm() {
    let plan =
        RaidSignalCorrelationService.plan_chat_notification(chat_notification_input(None, "src"));
    let sink = Arc::new(RecordingSink::default());
    RaidArrivalRuntime::new(sink.clone())
        .execute_plan(&plan)
        .await;

    let names = sink.names();
    assert!(names.contains(&"store_orphan_chat_notification".to_string()));
    assert!(
        !names.contains(&"confirm_pending_raid".to_string()),
        "Orphan → kein confirm"
    );
}

#[tokio::test]
async fn orphan_chat_notification_emittiert_observability_event_und_counter() {
    let plan =
        RaidSignalCorrelationService.plan_chat_notification(chat_notification_input(None, "src"));
    let sink = Arc::new(RecordingSink::default());
    let obs_sink = Arc::new(RecordingObservabilitySink::default());
    let observability = Arc::new(RaidObservabilityService::with_millis_source(
        Some(obs_sink.clone()),
        fixed_millis(4444),
    ));

    RaidArrivalRuntime::new(sink.clone())
        .with_observability(observability.clone())
        .execute_plan(&plan)
        .await;

    assert!(sink
        .names()
        .contains(&"store_orphan_chat_notification".to_string()));
    assert_eq!(
        observability
            .counters()
            .get("raid_orphan_chat_notification_total"),
        Some(&1)
    );
    let events = obs_sink.events();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.flow_type, "raid");
    assert_eq!(event.flow_id, "raid-orphan-4444-1");
    assert_eq!(event.step, "orphan_chat");
    assert_eq!(event.decision, "stored");
    assert_eq!(event.from_broadcaster_login.as_deref(), Some("src"));
    assert_eq!(event.from_broadcaster_id.as_deref(), Some("100"));
    assert_eq!(event.to_broadcaster_login.as_deref(), Some("dst"));
    assert_eq!(event.to_broadcaster_id.as_deref(), Some("200"));
    assert_eq!(event.details.get("viewer_count"), Some(&json!(42)));
    assert_eq!(event.details.get("message_id"), Some(&json!("msg-1")));
}

#[tokio::test]
async fn chat_unraid_bestaetigt_keinen_raid() {
    // Unraid bestätigt nie einen Raid — nur diagnostische Observation, nie confirm.
    let plan = RaidSignalCorrelationService.plan_chat_unraid(ChatUnraidInput {
        to_broadcaster_id: "200".into(),
        to_broadcaster_login: "dst".into(),
        from_broadcaster_login: "src".into(),
        from_broadcaster_id: Some("100".into()),
        pending_raid: Some(PendingRaid::new("src", "200")),
        recent_arrival_present: false,
        event_timestamp: None,
    });
    let sink = Arc::new(RecordingSink::default());
    RaidArrivalRuntime::new(sink.clone())
        .execute_plan(&plan)
        .await;
    assert!(
        !sink.names().contains(&"confirm_pending_raid".to_string()),
        "Unraid → niemals confirm"
    );
}

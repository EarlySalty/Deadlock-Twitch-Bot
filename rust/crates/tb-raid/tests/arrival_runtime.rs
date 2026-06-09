//! Test der Arrival-Runtime: echte Korrelations-Engine erzeugt Pläne, ein
//! Recording-Sink prüft, dass die Runtime die richtigen Effekte dispatcht.
//! Kein DB-Zugriff.

use std::sync::{Arc, Mutex};

use tb_raid::arrival_runtime::{RaidArrivalRuntime, RaidArrivalSink};
use tb_raid::pending_raids::PendingRaid;
use tb_raid::signal_correlation::{RaidArrivalInput, RaidSignalCorrelationService};

#[derive(Default)]
struct RecordingSink {
    calls: Mutex<Vec<String>>,
}
impl RecordingSink {
    fn names(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
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

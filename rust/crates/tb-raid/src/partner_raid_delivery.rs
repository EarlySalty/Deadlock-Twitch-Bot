//! Purer Planner für den Partner-Netzwerk-Raid-Versand (B3, Send-Slice).
//!
//! Faithful-Port des `PartnerRaidDeliveryPlanner` aus
//! `bot/raid/services/partner_raid_delivery.py` (Config: `delay_seconds=5.0`;
//! `plan`: Gating-Reihenfolge chat_bot_available → target_id → outbound_chat_suppressed
//! → ready). Reine Entscheidungs-/Präsentationslogik ohne Seiteneffekte.
//!
//! Der eigentliche Versand (`join_chat_channel`, 5s-Delay via `sleep`,
//! Chat-Send über `ChatApi`, `count_received_network_raids`-Quelle und der
//! `lookup_outbound_chat_suppression`-Gate-Aufruf) wird im Arrival-Sink in
//! `bin/tb-bot` verdrahtet — analog zur Recruitment-Send-Slice. Siehe
//! `WIRING-TODO(P1.11)` weiter unten. Der Nachrichtentext kommt aus
//! [`crate::raid_messaging::build_partner_raid_message`] (1:1 Python-Template),
//! daher trägt der Planner ihn nur als fertigen String.

use crate::raid_messaging::build_partner_raid_message;

/// Verzögerung vor dem Partner-Raid-Send (Python `delay_seconds = 5.0`).
pub const PARTNER_RAID_DELAY_SECONDS: f64 = 5.0;

/// Konfiguration des Partner-Raid-Delivery-Planners
/// (Python `PartnerRaidDeliveryConfig`, partner_raid_delivery.py:18).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartnerRaidDeliveryConfig {
    /// Verzögerung vor dem Senden (Python `delay_seconds = 5.0`).
    pub delay_seconds: f64,
}

impl Default for PartnerRaidDeliveryConfig {
    fn default() -> Self {
        Self {
            delay_seconds: PARTNER_RAID_DELAY_SECONDS,
        }
    }
}

/// Status eines Delivery-Plans (Python `PartnerRaidDeliveryStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartnerRaidDeliveryStatus {
    Ready,
    Blocked,
}

/// Eingabe für den Planner (Python `PartnerRaidDeliveryRequest`).
///
/// `received_raid_count` ist die laufende Nummer dieses Netzwerk-Raids für das
/// Ziel — vom Aufrufer aus `count_received_network_raids(to_broadcaster_id)`
/// geliefert (Python klemmt `<= 0` auf 1 *vor* dem Planner; der Planner klemmt
/// zusätzlich `< 0` auf 0). `outbound_chat_suppressed` spiegelt das Ergebnis des
/// `lookup_outbound_chat_suppression`-Gates wider.
#[derive(Debug, Clone)]
pub struct PartnerRaidDeliveryRequest {
    pub from_broadcaster_login: String,
    pub to_broadcaster_login: String,
    pub to_broadcaster_id: Option<String>,
    pub viewer_count: i32,
    pub received_raid_count: i64,
    pub chat_bot_available: bool,
    pub outbound_chat_suppressed: bool,
}

/// Ergebnis des Planners (Python `PartnerRaidDeliveryPlan`).
#[derive(Debug, Clone, PartialEq)]
pub struct PartnerRaidDeliveryPlan {
    pub status: PartnerRaidDeliveryStatus,
    pub reason: Option<&'static str>,
    pub delay_seconds: f64,
    pub target_id: Option<String>,
    pub target_login: String,
    pub from_login: String,
    pub viewer_count: i32,
    pub received_raid_count: i64,
    /// Fertiger Chat-Text (nur bei `Ready`), 1:1 aus dem Python-Template.
    pub message: Option<String>,
}

impl PartnerRaidDeliveryPlan {
    /// `true` wenn `status == Ready` (Python `should_deliver`).
    pub fn should_deliver(&self) -> bool {
        matches!(self.status, PartnerRaidDeliveryStatus::Ready)
    }
}

fn blocked_plan(
    reason: &'static str,
    config: &PartnerRaidDeliveryConfig,
    target_id: Option<String>,
    target_login: &str,
    from_login: &str,
    viewer_count: i32,
    received_raid_count: i64,
) -> PartnerRaidDeliveryPlan {
    PartnerRaidDeliveryPlan {
        status: PartnerRaidDeliveryStatus::Blocked,
        reason: Some(reason),
        delay_seconds: config.delay_seconds,
        target_id,
        target_login: target_login.to_string(),
        from_login: from_login.to_string(),
        viewer_count,
        received_raid_count,
        message: None,
    }
}

/// Plant den Partner-Raid-Versand (Python `PartnerRaidDeliveryPlanner.plan`).
///
/// Gating-Reihenfolge 1:1 zu Python (partner_raid_delivery.py:111–175):
/// 1. `chat_bot_available == false` → blocked `chat_bot_unavailable`
/// 2. kein `target_id` (nach Trim) → blocked `target_id_unresolved`
/// 3. `outbound_chat_suppressed == true` → blocked `outbound_chat_suppressed`
/// 4. sonst → ready mit Nachricht + 5s-Delay.
///
/// Normalisierung wie Python: `target_login`/`from_login` getrimmt + lowercased,
/// `viewer_count`/`received_raid_count` auf `>= 0` geklemmt.
pub fn plan_partner_raid_delivery(
    request: &PartnerRaidDeliveryRequest,
    config: &PartnerRaidDeliveryConfig,
) -> PartnerRaidDeliveryPlan {
    let target_id: Option<String> = request
        .to_broadcaster_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let target_login = request.to_broadcaster_login.trim().to_lowercase();
    let from_login = request.from_broadcaster_login.trim().to_lowercase();
    let viewer_count = request.viewer_count.max(0);
    let received_raid_count = request.received_raid_count.max(0);

    if !request.chat_bot_available {
        return blocked_plan(
            "chat_bot_unavailable",
            config,
            target_id,
            &target_login,
            &from_login,
            viewer_count,
            received_raid_count,
        );
    }

    let Some(target_id) = target_id else {
        return blocked_plan(
            "target_id_unresolved",
            config,
            None,
            &target_login,
            &from_login,
            viewer_count,
            received_raid_count,
        );
    };

    if request.outbound_chat_suppressed {
        return blocked_plan(
            "outbound_chat_suppressed",
            config,
            Some(target_id),
            &target_login,
            &from_login,
            viewer_count,
            received_raid_count,
        );
    }

    // Python baut hier den Text inline; wir nutzen das geteilte Template
    // (build_partner_raid_message klemmt received_raid_count zusätzlich < 1 auf 1,
    // identisch zu partner_raid_delivery.py:251–253).
    let message = build_partner_raid_message(
        &from_login,
        &target_login,
        viewer_count,
        received_raid_count,
    );

    PartnerRaidDeliveryPlan {
        status: PartnerRaidDeliveryStatus::Ready,
        reason: None,
        delay_seconds: config.delay_seconds,
        target_id: Some(target_id),
        target_login,
        from_login,
        viewer_count,
        received_raid_count,
        message: Some(message),
    }
}

// bin/tb-bot Arrival-Sink (confirm-Pfad, raid_arrival_wiring.rs) verdrahtet
// den Partner-Raid-Send analog zu Python
// PartnerRaidDeliveryService.send_partner_raid_message (partner_raid_delivery.py:224+)
// und make_partner_raid_delivery_service (runtime_factories.py:104–142).
// Vertrag dieser Crate:
//   1. get_chat_bot(); wenn keiner → skip.
//   2. lookup_outbound_chat_suppression(target_login, target_id, source="partner_raid");
//      wenn Some → skip (outbound_chat_suppressed=true).
//   3. received_raid_count = count_received_network_raids(to_broadcaster_id); <=0 → 1.
//   4. plan_partner_raid_delivery(request, &PartnerRaidDeliveryConfig::default());
//      wenn !should_deliver oder message None → skip mit plan.reason.
//   5. sleep(plan.delay_seconds == 5.0) → send plan.message über den ChatApi.
// Aufruf nur wenn ArrivalConfirmationDecision.should_send_partner_raid_message.

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_request() -> PartnerRaidDeliveryRequest {
        PartnerRaidDeliveryRequest {
            from_broadcaster_login: "Raider".into(),
            to_broadcaster_login: "Ziel".into(),
            to_broadcaster_id: Some("999".into()),
            viewer_count: 12,
            received_raid_count: 3,
            chat_bot_available: true,
            outbound_chat_suppressed: false,
        }
    }

    #[test]
    fn ready_plan_normalisiert_baut_nachricht_und_5s_delay() {
        let plan =
            plan_partner_raid_delivery(&ready_request(), &PartnerRaidDeliveryConfig::default());
        assert!(plan.should_deliver());
        assert_eq!(plan.reason, None);
        assert_eq!(plan.delay_seconds, 5.0);
        // Login getrimmt + lowercased.
        assert_eq!(plan.target_login, "ziel");
        assert_eq!(plan.from_login, "raider");
        assert_eq!(plan.received_raid_count, 3);
        // Nachricht 1:1 aus dem geteilten Template (korrekte received-raid count).
        assert_eq!(
            plan.message.as_deref(),
            Some(
                "Hey @ziel! 🎮 @raider hat dich gerade mit 12 Viewern geraidet. \
                 Das ist dein Raid Nr. 3 aus dem Deadlock Streamer-Netzwerk. ❤️"
            )
        );
    }

    #[test]
    fn suppression_gate_blockt_und_sendet_nicht() {
        // KERN-CONTRACT P1.11 (suppression-gated skip): outbound_chat_suppressed=true
        // → blocked, keine Nachricht.
        let mut r = ready_request();
        r.outbound_chat_suppressed = true;
        let plan = plan_partner_raid_delivery(&r, &PartnerRaidDeliveryConfig::default());
        assert!(!plan.should_deliver());
        assert_eq!(plan.reason, Some("outbound_chat_suppressed"));
        assert!(plan.message.is_none());
    }

    #[test]
    fn gating_reihenfolge_1zu1_python() {
        let cfg = PartnerRaidDeliveryConfig::default();

        // 1. chat_bot_unavailable hat Vorrang vor allem.
        let mut r = ready_request();
        r.chat_bot_available = false;
        r.to_broadcaster_id = None; // auch unresolved, aber chat_bot zuerst
        assert_eq!(
            plan_partner_raid_delivery(&r, &cfg).reason,
            Some("chat_bot_unavailable")
        );

        // 2. target_id_unresolved (leer nach Trim).
        let mut r = ready_request();
        r.to_broadcaster_id = Some("   ".into());
        let p = plan_partner_raid_delivery(&r, &cfg);
        assert_eq!(p.reason, Some("target_id_unresolved"));
        assert_eq!(p.target_id, None);

        // 3. outbound_chat_suppressed.
        let mut r = ready_request();
        r.outbound_chat_suppressed = true;
        assert_eq!(
            plan_partner_raid_delivery(&r, &cfg).reason,
            Some("outbound_chat_suppressed")
        );
    }

    #[test]
    fn received_raid_count_im_text_korrekt() {
        // KERN-CONTRACT P1.11: korrekte received-raid count im Send-Text.
        let mut r = ready_request();
        r.received_raid_count = 7;
        r.viewer_count = 1; // Singular-Pfad zusätzlich abdecken
        let plan = plan_partner_raid_delivery(&r, &PartnerRaidDeliveryConfig::default());
        let msg = plan.message.unwrap();
        assert!(msg.contains("dein Raid Nr. 7 "));
        assert!(msg.contains("mit 1 Viewer geraidet"));
    }

    #[test]
    fn negative_counts_werden_geklemmt() {
        let mut r = ready_request();
        r.viewer_count = -5;
        r.received_raid_count = -2;
        let plan = plan_partner_raid_delivery(&r, &PartnerRaidDeliveryConfig::default());
        assert_eq!(plan.viewer_count, 0);
        assert_eq!(plan.received_raid_count, 0);
        // Template klemmt count < 1 → 1 für den sichtbaren Text.
        assert!(plan.message.unwrap().contains("dein Raid Nr. 1 "));
    }
}

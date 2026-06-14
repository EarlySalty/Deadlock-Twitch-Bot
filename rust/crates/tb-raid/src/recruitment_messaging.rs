//! Purer Planner + Textbausteine für Recruitment-Nachrichten an extern
//! geraidete Nicht-Partner-Channels (B3, Recruitment-Teil).
//!
//! Faithful-Port von `bot/raid/recruitment_delivery.py` (Planner: Variant-Wahl,
//! Gating, Delay/Invite-Variant) + dem Stage-Bogen aus
//! `recruitment_messaging.py::_build_message` (s1..s10).
//!
//! Reine Entscheidungs-/Präsentationslogik ohne Seiteneffekte. Der Versand
//! (ChatApi-Send, Suppression, Recent-/Total-Counts, Follower-Auflösung,
//! Ban-Check-Scheduling) folgt als eigene Wiring-Slice im Arrival-Sink — analog
//! zur Partner-Raid-Send-Slice. Der Send geht über denselben `ChatApi`-Pfad wie
//! die Partner-Raid-Nachricht; lehnt Twitch den externen Channel ab, ist das ein
//! gracefuler `Dropped`-Ausgang, kein Grund das Feature wegzulassen.

/// Konfiguration des Recruitment-Delivery-Planners
/// (Python `RecruitmentDeliveryConfig`, recruitment_delivery.py:15).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecruitmentDeliveryConfig {
    /// Verzögerung vor dem Senden (Python `delay_seconds = 15.0`).
    pub delay_seconds: f64,
    /// Wenn der Ziel-Channel innerhalb des Recent-Fensters öfter als so geraidet
    /// wurde, wird NICHT recruited (Python `recent_raid_threshold = 2`).
    pub recent_raid_threshold: i64,
    /// Hartes Limit gegen Dauerbeschallung (Python `max_recruitment_messages = 50`).
    pub max_recruitment_messages: i64,
    /// Ziele mit ≤ so vielen Followern bekommen die „direct"-Invite-Variante
    /// (Python `direct_invite_max_followers = 120`).
    pub direct_invite_max_followers: i64,
}

impl Default for RecruitmentDeliveryConfig {
    fn default() -> Self {
        Self {
            delay_seconds: 15.0,
            recent_raid_threshold: 2,
            max_recruitment_messages: 50,
            direct_invite_max_followers: 120,
        }
    }
}

/// Etappe des fortlaufenden Recruitment-Bogens (Python `RecruitmentMessageVariant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecruitmentMessageVariant {
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
    S8,
    S9,
    S10,
}

/// Invite-Stil je nach Reichweite des Ziels (Python `RecruitmentInviteVariant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecruitmentInviteVariant {
    Direct,
    Standard,
}

/// Status eines Delivery-Plans (Python `RecruitmentDeliveryStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecruitmentDeliveryStatus {
    Ready,
    Blocked,
}

/// Eingabe für den Planner (Python `RecruitmentDeliveryRequest`).
#[derive(Debug, Clone)]
pub struct RecruitmentDeliveryRequest {
    pub from_broadcaster_login: String,
    pub to_broadcaster_login: String,
    pub target_id: Option<String>,
    pub recent_raid_count: i64,
    pub total_recruitment_raid_count: Option<i64>,
    pub followers_total: Option<i64>,
    pub chat_bot_available: bool,
    pub outbound_chat_suppressed: bool,
}

/// Ergebnis des Planners (Python `RecruitmentDeliveryPlan`).
#[derive(Debug, Clone, PartialEq)]
pub struct RecruitmentDeliveryPlan {
    pub status: RecruitmentDeliveryStatus,
    pub reason: Option<&'static str>,
    pub delay_seconds: f64,
    pub target_id: Option<String>,
    pub target_login: String,
    pub recent_raid_count: i64,
    pub total_recruitment_raid_count: Option<i64>,
    pub message_variant: Option<RecruitmentMessageVariant>,
    pub invite_variant: Option<RecruitmentInviteVariant>,
}

impl RecruitmentDeliveryPlan {
    /// `true` wenn `status == Ready` (Python `should_deliver`).
    pub fn should_deliver(&self) -> bool {
        matches!(self.status, RecruitmentDeliveryStatus::Ready)
    }
}

fn blocked_plan(
    reason: &'static str,
    config: &RecruitmentDeliveryConfig,
    target_id: Option<String>,
    target_login: &str,
    recent_raid_count: i64,
    total_recruitment_raid_count: Option<i64>,
) -> RecruitmentDeliveryPlan {
    RecruitmentDeliveryPlan {
        status: RecruitmentDeliveryStatus::Blocked,
        reason: Some(reason),
        delay_seconds: config.delay_seconds,
        target_id,
        target_login: target_login.to_string(),
        recent_raid_count,
        total_recruitment_raid_count,
        message_variant: None,
        invite_variant: None,
    }
}

/// Etappe = Kontaktzähler als Kapitel-Index (Python `_message_variant`):
/// 1→S1 … 10→S10, alles darüber bleibt bei S10 (Dauer-Nudge).
fn message_variant_for(total_recruitment_raid_count: i64) -> RecruitmentMessageVariant {
    match total_recruitment_raid_count.clamp(1, 10) {
        1 => RecruitmentMessageVariant::S1,
        2 => RecruitmentMessageVariant::S2,
        3 => RecruitmentMessageVariant::S3,
        4 => RecruitmentMessageVariant::S4,
        5 => RecruitmentMessageVariant::S5,
        6 => RecruitmentMessageVariant::S6,
        7 => RecruitmentMessageVariant::S7,
        8 => RecruitmentMessageVariant::S8,
        9 => RecruitmentMessageVariant::S9,
        _ => RecruitmentMessageVariant::S10,
    }
}

/// Python `_invite_variant`: ≤ direct_invite_max_followers → Direct, sonst Standard.
fn invite_variant_for(
    followers_total: Option<i64>,
    config: &RecruitmentDeliveryConfig,
) -> RecruitmentInviteVariant {
    match followers_total {
        Some(f) if f <= config.direct_invite_max_followers => RecruitmentInviteVariant::Direct,
        _ => RecruitmentInviteVariant::Standard,
    }
}

/// Plant die Recruitment-Zustellung (Python `RecruitmentDeliveryPlanner.plan`).
/// Gating-Reihenfolge 1:1 zu Python.
pub fn plan_recruitment_delivery(
    request: &RecruitmentDeliveryRequest,
    config: &RecruitmentDeliveryConfig,
) -> RecruitmentDeliveryPlan {
    let target_id: Option<String> = request
        .target_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let target_login = request.to_broadcaster_login.trim().to_lowercase();
    let recent_raid_count = request.recent_raid_count.max(0);
    let total = request.total_recruitment_raid_count;
    let followers_total = request.followers_total;

    if !request.chat_bot_available {
        return blocked_plan(
            "chat_bot_unavailable",
            config,
            target_id,
            &target_login,
            recent_raid_count,
            total,
        );
    }

    let Some(target_id) = target_id else {
        return blocked_plan(
            "target_id_unresolved",
            config,
            None,
            &target_login,
            recent_raid_count,
            total,
        );
    };

    if request.outbound_chat_suppressed {
        return blocked_plan(
            "outbound_chat_suppressed",
            config,
            Some(target_id),
            &target_login,
            recent_raid_count,
            total,
        );
    }

    if recent_raid_count > config.recent_raid_threshold {
        return blocked_plan(
            "recent_raids_exceed_threshold",
            config,
            Some(target_id),
            &target_login,
            recent_raid_count,
            total,
        );
    }

    let Some(total) = total else {
        return blocked_plan(
            "total_recruitment_raid_count_unresolved",
            config,
            Some(target_id),
            &target_login,
            recent_raid_count,
            None,
        );
    };

    if total > config.max_recruitment_messages {
        return blocked_plan(
            "max_recruitment_messages_reached",
            config,
            Some(target_id),
            &target_login,
            recent_raid_count,
            Some(total),
        );
    }

    RecruitmentDeliveryPlan {
        status: RecruitmentDeliveryStatus::Ready,
        reason: None,
        delay_seconds: config.delay_seconds,
        target_id: Some(target_id),
        target_login,
        recent_raid_count,
        total_recruitment_raid_count: Some(total),
        message_variant: Some(message_variant_for(total)),
        invite_variant: Some(invite_variant_for(followers_total, config)),
    }
}

/// Stage-Bogen-Text für die gewählte Variante (Python
/// `recruitment_messaging.py::_build_message`). CTA immer „in der Bio", KEINE
/// URLs (Twitch-AutoMod bannt Links); `@name` nur im Erstkontakt (S1/S2).
pub fn build_recruitment_message(
    variant: RecruitmentMessageVariant,
    to_broadcaster_login: &str,
) -> String {
    let name = to_broadcaster_login;
    match variant {
        RecruitmentMessageVariant::S1 => format!(
            "Hey @{name} — die Zuschauer, die grad reinkamen, sind echt und kamen \
             nicht zufällig: ein anderer deutscher Deadlock-Streamer hat sie dir \
             geschickt, als er offline ging. Genau das macht unser Bot — Zuschauer \
             zwischen Deadlock-Streamern weiterreichen, statt sie verpuffen zu lassen. \
             Wer „wir“ sind, steht in der Bio. 👀"
        ),
        RecruitmentMessageVariant::S2 => format!(
            "Schon der zweite Support-Raid, @{name} — beim ersten hast du wahrscheinlich \
             „Scam“ gedacht. Verständlich, ist aber keiner: wir vernetzen die deutschen \
             Deadlock-Streamer und schieben uns gegenseitig Zuschauer zu. Wer dabei ist, \
             dem hält der Bot nebenbei Viewer-Bot-Spam aus dem Chat. Mehr in der Bio."
        ),
        RecruitmentMessageVariant::S3 => "Dritter Raid, und ja, das hat System. Wir sind die größte aktive deutsche \
             Deadlock-Community — Streamer reichen sich gegenseitig Zuschauer weiter, \
             keiner sendet allein. Kein Haken: einmal verbinden, der Rest läuft von selbst. \
             Wie das für dich aussieht, steht in der Bio."
            .to_string(),
        RecruitmentMessageVariant::S4 => "Fragst dich langsam, was wir wollen? Ganz einfach: hier supportet jeder jeden — \
             Streamer zocken mit der Community, und das zieht alle hoch. \
             Die Bio erklärt den Rest."
            .to_string(),
        RecruitmentMessageVariant::S5 => "Deadlock ist grad klein, und genau das ist die Chance. Wir vernetzen jetzt die, \
             die dranbleiben, damit wir zusammen oben stehen, wenn's zurückkommt. \
             Steht alles in der Bio."
            .to_string(),
        RecruitmentMessageVariant::S6 => "Mal Butter bei die Fische, weil du immer noch hier bist: Wir koordinieren Raids, \
             gemeinsame Sessions, gegenseitigen Support. Kein Vertrag, kein Haken — Community. \
             In der Bio steht, wie's läuft."
            .to_string(),
        RecruitmentMessageVariant::S7 => "Die Streamer bei uns raiden sich gegenseitig, zocken zusammen, wachsen zusammen. \
             Genau dieser Kreislauf fehlt den meisten, die allein streamen. \
             In der Bio siehst du, wie du reinkommst."
            .to_string(),
        RecruitmentMessageVariant::S8 => "Du bist jetzt oft genug von uns geraidet worden, dass man sagen kann: \
             wir mögen deinen Stream. Der nächste Schritt liegt bei dir — \
             alles dazu in der Bio."
            .to_string(),
        RecruitmentMessageVariant::S9 => "Du gehörst hier eigentlich schon halb dazu. Der Platz in der Community steht \
             für dich offen — schau in die Bio, da erfährst du mehr."
            .to_string(),
        RecruitmentMessageVariant::S10 => {
            "Wieder wir 👋 Wir halten dir den Platz frei. Alles Wichtige liegt in der Bio."
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_request() -> RecruitmentDeliveryRequest {
        RecruitmentDeliveryRequest {
            from_broadcaster_login: "raider".into(),
            to_broadcaster_login: "Ziel".into(),
            target_id: Some("999".into()),
            recent_raid_count: 1,
            total_recruitment_raid_count: Some(1),
            followers_total: Some(50),
            chat_bot_available: true,
            outbound_chat_suppressed: false,
        }
    }

    #[test]
    fn ready_plan_normalisiert_und_waehlt_varianten() {
        let plan = plan_recruitment_delivery(&ready_request(), &RecruitmentDeliveryConfig::default());
        assert!(plan.should_deliver());
        assert_eq!(plan.reason, None);
        assert_eq!(plan.delay_seconds, 15.0);
        // to_broadcaster_login wird getrimmt + lowercased.
        assert_eq!(plan.target_login, "ziel");
        assert_eq!(plan.message_variant, Some(RecruitmentMessageVariant::S1));
        // 50 Follower ≤ 120 → Direct.
        assert_eq!(plan.invite_variant, Some(RecruitmentInviteVariant::Direct));
    }

    #[test]
    fn gating_reihenfolge_1zu1_python() {
        let cfg = RecruitmentDeliveryConfig::default();

        let mut r = ready_request();
        r.chat_bot_available = false;
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("chat_bot_unavailable")
        );

        let mut r = ready_request();
        r.target_id = Some("   ".into()); // leer nach Trim → unresolved
        let p = plan_recruitment_delivery(&r, &cfg);
        assert_eq!(p.reason, Some("target_id_unresolved"));
        assert_eq!(p.target_id, None);

        let mut r = ready_request();
        r.outbound_chat_suppressed = true;
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("outbound_chat_suppressed")
        );

        let mut r = ready_request();
        r.recent_raid_count = 3; // > Schwelle 2
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("recent_raids_exceed_threshold")
        );

        let mut r = ready_request();
        r.total_recruitment_raid_count = None;
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("total_recruitment_raid_count_unresolved")
        );

        let mut r = ready_request();
        r.total_recruitment_raid_count = Some(51); // > 50
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("max_recruitment_messages_reached")
        );
    }

    #[test]
    fn recent_raid_count_genau_auf_schwelle_ist_erlaubt() {
        // Python: > threshold blockt; == threshold (2) ist erlaubt.
        let mut r = ready_request();
        r.recent_raid_count = 2;
        assert!(plan_recruitment_delivery(&r, &RecruitmentDeliveryConfig::default()).should_deliver());
    }

    #[test]
    fn message_variant_clamp_1_bis_10() {
        assert_eq!(message_variant_for(0), RecruitmentMessageVariant::S1); // max(.,1)
        assert_eq!(message_variant_for(1), RecruitmentMessageVariant::S1);
        assert_eq!(message_variant_for(5), RecruitmentMessageVariant::S5);
        assert_eq!(message_variant_for(10), RecruitmentMessageVariant::S10);
        assert_eq!(message_variant_for(99), RecruitmentMessageVariant::S10); // Dauer-Nudge
    }

    #[test]
    fn invite_variant_schwelle_120() {
        let cfg = RecruitmentDeliveryConfig::default();
        assert_eq!(invite_variant_for(Some(120), &cfg), RecruitmentInviteVariant::Direct);
        assert_eq!(invite_variant_for(Some(121), &cfg), RecruitmentInviteVariant::Standard);
        assert_eq!(invite_variant_for(None, &cfg), RecruitmentInviteVariant::Standard);
    }

    #[test]
    fn message_s1_s2_interpolieren_namen() {
        let s1 = build_recruitment_message(RecruitmentMessageVariant::S1, "victim");
        assert!(s1.starts_with("Hey @victim — "));
        assert!(s1.contains("Wer „wir“ sind, steht in der Bio. 👀"));

        let s2 = build_recruitment_message(RecruitmentMessageVariant::S2, "victim");
        assert!(s2.contains("@victim"));
        assert!(s2.contains("„Scam“"));

        // s3..s10 ohne @name.
        let s10 = build_recruitment_message(RecruitmentMessageVariant::S10, "victim");
        assert!(!s10.contains("@victim"));
        assert_eq!(
            s10,
            "Wieder wir 👋 Wir halten dir den Platz frei. Alles Wichtige liegt in der Bio."
        );
    }
}

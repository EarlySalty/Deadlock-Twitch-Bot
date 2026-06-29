//! Purer Planner + Textbausteine für Recruitment-Nachrichten an extern
//! geraidete Nicht-Partner-Channels (B3, Recruitment-Teil).
//!
//! Faithful-Port von `bot/raid/recruitment_delivery.py` (Planner: Delay/
//! Invite-Variant) + Outreach-Trust-Leiter (Raid-Zähler pro Ziel).
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
    /// Legacy-Feld: der Trust-Leiter-Funnel blockt nicht mehr nach Recent-Raids.
    pub recent_raid_threshold: i64,
    /// Legacy-Feld: der Trust-Leiter-Funnel läuft 11+ endlos.
    pub max_recruitment_messages: i64,
    /// Ziele mit ≤ so vielen Followern bekommen die „direct"-Invite-Variante
    /// (Python `direct_invite_max_followers = 120`).
    pub direct_invite_max_followers: i64,
}

impl Default for RecruitmentDeliveryConfig {
    fn default() -> Self {
        Self {
            delay_seconds: 15.0,
            recent_raid_threshold: i64::MAX,
            max_recruitment_messages: i64::MAX,
            direct_invite_max_followers: 120,
        }
    }
}

/// Etappe des Outreach-Trust-Leiter-Funnels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecruitmentMessageVariant {
    S1,
    S2,
    S3,
    S4,
    Light { pool_index: usize },
    Pitch { pool_index: usize },
    Cheeky { pool_index: usize, raid_count: i64 },
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
    pub target_blacklisted: bool,
    pub target_is_partner: bool,
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

/// Etappe = echter Gesamt-Raid-Zähler pro Ziel:
/// 1→S1, 2→S2, 3→S3, 4→S4, 5–6→LIGHT, 7–10→PITCH, 11+→CHEEKY.
fn message_variant_for(total_recruitment_raid_count: i64) -> RecruitmentMessageVariant {
    let stage = total_recruitment_raid_count.max(1);
    match stage {
        1 => RecruitmentMessageVariant::S1,
        2 => RecruitmentMessageVariant::S2,
        3 => RecruitmentMessageVariant::S3,
        4 => RecruitmentMessageVariant::S4,
        5 | 6 => RecruitmentMessageVariant::Light {
            pool_index: pool_index(stage, OUTREACH_TRUST_LIGHT_POOL.len()),
        },
        7..=10 => RecruitmentMessageVariant::Pitch {
            pool_index: pool_index(stage, OUTREACH_TRUST_PITCH_POOL.len()),
        },
        _ => RecruitmentMessageVariant::Cheeky {
            pool_index: pool_index(stage, OUTREACH_TRUST_CHEEKY_POOL.len()),
            raid_count: stage,
        },
    }
}

fn pool_index(stage: i64, pool_len: usize) -> usize {
    stage as usize % pool_len
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

/// Plant die Recruitment-Zustellung für die Trust-Leiter.
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

    if request.target_blacklisted {
        return blocked_plan(
            "target_blacklisted",
            config,
            Some(target_id),
            &target_login,
            recent_raid_count,
            total,
        );
    }

    if request.target_is_partner {
        return blocked_plan(
            "target_is_partner",
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

pub const OUTREACH_TRUST_STAGE_1_MESSAGE: &str = "Hey! Wir bringen dir gerade ein bisschen Unterstützung aus der Deutschen Deadlock Community 💜 Wir wünschen dir noch nen geilen Stream! Und falls du öfter mal Support bekommen möchtest, schau gerne bei uns im Profil vorbei. (Keine Sorge, wir sind kein Scam oder sowas. 😅)";
pub const OUTREACH_TRUST_STAGE_2_MESSAGE: &str = "Und wieder ein bisschen Support für dich 👋 Falls du uns noch nicht kennst: Wir sind die größte und aktivste Deutsche Deadlock Community. Viel Spaß weiterhin!";
pub const OUTREACH_TRUST_STAGE_3_MESSAGE: &str = "Schon wieder wir 😄 Bei uns sind echte Leute, echte Streamer, die Deadlock genauso lieben wie du. Schön, dich dabei zu haben — viel Spaß weiterhin!";
pub const OUTREACH_TRUST_STAGE_4_MESSAGE: &str = "Nächste Ladung Support für dich 💜 Wenn du Bock hast, dauerhaft dabei zu sein — alles dazu findest du auf unserer Website (Link im Profil), und unser Discord ist auch da. Kein Stress, schau einfach mal rein. Weiter so!";

pub const OUTREACH_TRUST_LIGHT_1_MESSAGE: &str =
    "Wieder wir 👋 Hau rein und viel Spaß beim Stream!";
pub const OUTREACH_TRUST_LIGHT_2_MESSAGE: &str =
    "Kleiner Support von der Deutschen Deadlock Community 💜 Schön, dich regelmäßig zu sehen!";
pub const OUTREACH_TRUST_LIGHT_3_MESSAGE: &str = "Und wieder ein paar Leute für dich 😄 Wir halten die deutsche Deadlock-Szene zusammen am Leben — freut uns, dass du dabei bist!";
pub const OUTREACH_TRUST_LIGHT_POOL: [&str; 3] = [
    OUTREACH_TRUST_LIGHT_1_MESSAGE,
    OUTREACH_TRUST_LIGHT_2_MESSAGE,
    OUTREACH_TRUST_LIGHT_3_MESSAGE,
];

pub const OUTREACH_TRUST_PITCH_1_MESSAGE: &str = "Falls du mehr willst als nur Viewer: Bei uns gibt's Turniere, Coaching und regelmäßige Events, über 2.400 Leute sind dabei. Alles dazu auf unserer Website (Link im Profil), Discord auch 💜";
pub const OUTREACH_TRUST_PITCH_2_MESSAGE: &str = "Wir machen für die deutsche Deadlock-Szene richtig was — Turniere, Coaching, Events, 2.400+ Mitglieder. Wenn du Bock hast mitzumachen, schau auf unsere Website (im Profil)!";
pub const OUTREACH_TRUST_PITCH_3_MESSAGE: &str = "Du kriegst hier nicht nur Viewer: Turniere, Coaching und 'ne richtig aktive Community warten auf dich. Mehr auf unserer Website (Profil), bis dahin viel Spaß! 💜";
pub const OUTREACH_TRUST_PITCH_POOL: [&str; 3] = [
    OUTREACH_TRUST_PITCH_1_MESSAGE,
    OUTREACH_TRUST_PITCH_2_MESSAGE,
    OUTREACH_TRUST_PITCH_3_MESSAGE,
];

pub const OUTREACH_TRUST_CHEEKY_1_MESSAGE: &str = "Raid Nr. {n} 💀 Du genießt unseren Support echt gern, was? 😄 Aber Teil der Community werden willst du nicht? Komm schon — Website im Profil, Discord auch!";
pub const OUTREACH_TRUST_CHEEKY_2_MESSAGE: &str = "Und täglich grüßt der Support 😏 Das ist Raid #{n} für dich. Wie oft willst du eigentlich noch Viewer abgreifen, bevor du mal vorbeischaust? Website + Discord im Profil 💜";
pub const OUTREACH_TRUST_CHEEKY_3_MESSAGE: &str = "Raid #{n} und immer noch nicht dabei? Langsam wird's persönlich 😂 Alles zum Mitmachen auf unserer Website (Profil). Den Support gibt's natürlich trotzdem weiter 👋";
pub const OUTREACH_TRUST_CHEEKY_4_MESSAGE: &str = "Nr. {n} 🫡 Ehre für die Treue — aber so langsam könntest du auch mal offiziell mitmachen 😅 Website im Profil!";
pub const OUTREACH_TRUST_CHEEKY_POOL: [&str; 4] = [
    OUTREACH_TRUST_CHEEKY_1_MESSAGE,
    OUTREACH_TRUST_CHEEKY_2_MESSAGE,
    OUTREACH_TRUST_CHEEKY_3_MESSAGE,
    OUTREACH_TRUST_CHEEKY_4_MESSAGE,
];

/// Trust-Leiter-Text für die gewählte Variante. CTA bleibt ohne rohe Links.
pub fn build_recruitment_message(
    variant: RecruitmentMessageVariant,
    _to_broadcaster_login: &str,
) -> String {
    match variant {
        RecruitmentMessageVariant::S1 => OUTREACH_TRUST_STAGE_1_MESSAGE.to_string(),
        RecruitmentMessageVariant::S2 => OUTREACH_TRUST_STAGE_2_MESSAGE.to_string(),
        RecruitmentMessageVariant::S3 => OUTREACH_TRUST_STAGE_3_MESSAGE.to_string(),
        RecruitmentMessageVariant::S4 => OUTREACH_TRUST_STAGE_4_MESSAGE.to_string(),
        RecruitmentMessageVariant::Light { pool_index } => OUTREACH_TRUST_LIGHT_POOL
            .get(pool_index)
            .copied()
            .unwrap_or(OUTREACH_TRUST_LIGHT_1_MESSAGE)
            .to_string(),
        RecruitmentMessageVariant::Pitch { pool_index } => OUTREACH_TRUST_PITCH_POOL
            .get(pool_index)
            .copied()
            .unwrap_or(OUTREACH_TRUST_PITCH_1_MESSAGE)
            .to_string(),
        RecruitmentMessageVariant::Cheeky {
            pool_index,
            raid_count,
        } => OUTREACH_TRUST_CHEEKY_POOL
            .get(pool_index)
            .copied()
            .unwrap_or(OUTREACH_TRUST_CHEEKY_1_MESSAGE)
            .replace("{n}", &raid_count.to_string()),
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
            target_blacklisted: false,
            target_is_partner: false,
            outbound_chat_suppressed: false,
        }
    }

    #[test]
    fn ready_plan_normalisiert_und_waehlt_varianten() {
        let plan =
            plan_recruitment_delivery(&ready_request(), &RecruitmentDeliveryConfig::default());
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
    fn gating_stoppt_bei_infrastruktur_und_sicherheitsbedingungen() {
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
        r.target_blacklisted = true;
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("target_blacklisted")
        );

        let mut r = ready_request();
        r.target_is_partner = true;
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("target_is_partner")
        );

        let mut r = ready_request();
        r.total_recruitment_raid_count = None;
        assert_eq!(
            plan_recruitment_delivery(&r, &cfg).reason,
            Some("total_recruitment_raid_count_unresolved")
        );
    }

    #[test]
    fn recent_und_max_caps_blocken_leiter_nicht_mehr() {
        let mut r = ready_request();
        r.recent_raid_count = 500;
        r.total_recruitment_raid_count = Some(50_000);
        let plan = plan_recruitment_delivery(&r, &RecruitmentDeliveryConfig::default());
        assert!(plan.should_deliver());
        assert_eq!(
            plan.message_variant,
            Some(RecruitmentMessageVariant::Cheeky {
                pool_index: 0,
                raid_count: 50_000
            })
        );
    }

    #[test]
    fn message_variant_trust_leiter_1_bis_11_plus() {
        assert_eq!(message_variant_for(0), RecruitmentMessageVariant::S1); // max(.,1)
        assert_eq!(message_variant_for(1), RecruitmentMessageVariant::S1);
        assert_eq!(message_variant_for(2), RecruitmentMessageVariant::S2);
        assert_eq!(message_variant_for(3), RecruitmentMessageVariant::S3);
        assert_eq!(message_variant_for(4), RecruitmentMessageVariant::S4);
        assert_eq!(
            message_variant_for(5),
            RecruitmentMessageVariant::Light { pool_index: 2 }
        );
        assert_eq!(
            message_variant_for(6),
            RecruitmentMessageVariant::Light { pool_index: 0 }
        );
        assert_eq!(
            message_variant_for(7),
            RecruitmentMessageVariant::Pitch { pool_index: 1 }
        );
        assert_eq!(
            message_variant_for(8),
            RecruitmentMessageVariant::Pitch { pool_index: 2 }
        );
        assert_eq!(
            message_variant_for(9),
            RecruitmentMessageVariant::Pitch { pool_index: 0 }
        );
        assert_eq!(
            message_variant_for(10),
            RecruitmentMessageVariant::Pitch { pool_index: 1 }
        );
        assert_eq!(
            message_variant_for(11),
            RecruitmentMessageVariant::Cheeky {
                pool_index: 3,
                raid_count: 11
            }
        );
        assert_eq!(
            message_variant_for(99),
            RecruitmentMessageVariant::Cheeky {
                pool_index: 3,
                raid_count: 99
            }
        );
    }

    #[test]
    fn invite_variant_schwelle_120() {
        let cfg = RecruitmentDeliveryConfig::default();
        assert_eq!(
            invite_variant_for(Some(120), &cfg),
            RecruitmentInviteVariant::Direct
        );
        assert_eq!(
            invite_variant_for(Some(121), &cfg),
            RecruitmentInviteVariant::Standard
        );
        assert_eq!(
            invite_variant_for(None, &cfg),
            RecruitmentInviteVariant::Standard
        );
    }

    #[test]
    fn message_texts_sind_spec_verbatim_und_ohne_rohe_links() {
        let s1 = build_recruitment_message(RecruitmentMessageVariant::S1, "victim");
        assert_eq!(s1, OUTREACH_TRUST_STAGE_1_MESSAGE);

        let s2 = build_recruitment_message(RecruitmentMessageVariant::S2, "victim");
        assert_eq!(s2, OUTREACH_TRUST_STAGE_2_MESSAGE);
        assert_eq!(
            build_recruitment_message(RecruitmentMessageVariant::S3, "victim"),
            OUTREACH_TRUST_STAGE_3_MESSAGE
        );
        assert_eq!(
            build_recruitment_message(RecruitmentMessageVariant::S4, "victim"),
            OUTREACH_TRUST_STAGE_4_MESSAGE
        );
        assert_eq!(
            build_recruitment_message(RecruitmentMessageVariant::Light { pool_index: 2 }, "victim"),
            OUTREACH_TRUST_LIGHT_3_MESSAGE
        );
        assert_eq!(
            build_recruitment_message(RecruitmentMessageVariant::Pitch { pool_index: 1 }, "victim"),
            OUTREACH_TRUST_PITCH_2_MESSAGE
        );

        for msg in [
            s1,
            s2,
            OUTREACH_TRUST_STAGE_3_MESSAGE.to_string(),
            OUTREACH_TRUST_STAGE_4_MESSAGE.to_string(),
            OUTREACH_TRUST_LIGHT_1_MESSAGE.to_string(),
            OUTREACH_TRUST_PITCH_1_MESSAGE.to_string(),
        ] {
            assert!(!msg.contains("http://"));
            assert!(!msg.contains("https://"));
            assert!(!msg.contains("@victim"));
        }
    }

    #[test]
    fn cheeky_pool_setzt_echten_raid_zaehler_ein() {
        let msg = build_recruitment_message(
            RecruitmentMessageVariant::Cheeky {
                pool_index: 1,
                raid_count: 42,
            },
            "victim",
        );
        assert_eq!(
            msg,
            "Und täglich grüßt der Support 😏 Das ist Raid #42 für dich. Wie oft willst du eigentlich noch Viewer abgreifen, bevor du mal vorbeischaust? Website + Discord im Profil 💜"
        );
        assert!(!msg.contains("{n}"));
    }

    #[test]
    fn oeffentliche_pool_index_varianten_haben_sicheren_fallback() {
        assert_eq!(
            build_recruitment_message(
                RecruitmentMessageVariant::Light {
                    pool_index: usize::MAX
                },
                "victim"
            ),
            OUTREACH_TRUST_LIGHT_1_MESSAGE
        );
        assert_eq!(
            build_recruitment_message(
                RecruitmentMessageVariant::Pitch {
                    pool_index: usize::MAX
                },
                "victim"
            ),
            OUTREACH_TRUST_PITCH_1_MESSAGE
        );
        assert_eq!(
            build_recruitment_message(
                RecruitmentMessageVariant::Cheeky {
                    pool_index: usize::MAX,
                    raid_count: 42
                },
                "victim"
            ),
            OUTREACH_TRUST_CHEEKY_1_MESSAGE.replace("{n}", "42")
        );
    }
}

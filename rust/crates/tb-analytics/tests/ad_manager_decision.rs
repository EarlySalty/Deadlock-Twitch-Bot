use chrono::{DateTime, Duration, TimeZone, Utc};
use tb_analytics::ad_manager::{
    assess_plan, decide, plan_next_block, AdPlan, DecisionAction, DecisionInput, LiveState,
    Settings, SteamMatchState, Strategy, COMMERCIAL_SCOPE, READ_SCOPE, SNOOZE_SCOPE,
};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap()
}

fn steam_state(in_match: bool, in_deadlock: bool) -> SteamMatchState {
    SteamMatchState {
        in_match,
        in_deadlock,
        hero: Some("Haze".into()),
        stage: in_match.then_some("laning".into()),
        observed_at: now() - Duration::seconds(30),
    }
}

fn due_plan() -> AdPlan {
    AdPlan {
        next_block_at: Some(now() - Duration::seconds(60)),
        block_seconds: 30,
        blocks_per_hour: 6,
        budget_used_seconds_this_hour: 0,
    }
}

fn base(strategy: Strategy) -> DecisionInput {
    let now = now();
    DecisionInput {
        now,
        settings: Settings {
            enabled: true,
            strategy,
            ..Settings::default()
        },
        stream_started_at: Some(now - Duration::hours(1)),
        next_ad_at: None,
        last_ad_at: None,
        snooze_count: 1,
        quiet_chat_messages: 0,
        recent_chat_messages: 0,
        chat_ingest_healthy: true,
        steam_match_state: None,
        plan: due_plan(),
        match_started_at: None,
        match_ended_at: None,
        last_raid_at: None,
        last_raider: None,
        last_first_chatter_at: None,
        last_first_chatter: None,
        retry_after_seconds: 480,
        pull_forward_seconds: 90,
        plan_fit: "good",
    }
}

#[test]
fn strategien_und_scopes_kennen_kein_monitor_mehr() {
    assert_eq!(Strategy::parse("snooze"), Some(Strategy::Snooze));
    assert_eq!(Strategy::parse("smart"), Some(Strategy::Smart));
    for invalid in ["", "monitor", "SMART", " smart ", "💸"] {
        assert_eq!(Strategy::parse(invalid), None, "{invalid:?}");
    }
    assert_eq!(
        Strategy::Snooze.required_scopes(),
        &[READ_SCOPE, SNOOZE_SCOPE]
    );
    assert_eq!(
        Strategy::Smart.required_scopes(),
        &[READ_SCOPE, SNOOZE_SCOPE, COMMERCIAL_SCOPE]
    );
}

#[test]
fn budgetgrenzen_werden_geprueft() {
    let mut settings = Settings::default();
    for valid in [1, 3, 8] {
        settings.budget_minutes_per_hour = valid;
        assert!(settings.validate().is_ok(), "Budget {valid}");
    }
    for invalid in [0, -1, 9, 100] {
        settings.budget_minutes_per_hour = invalid;
        assert!(settings.validate().is_err(), "Budget {invalid}");
    }
}

#[test]
fn planer_verteilt_budget_und_waehlt_blocklaenge() {
    let three = plan_next_block(now(), Some(now() - Duration::hours(1)), 3, 0, None, 480);
    assert_eq!(three.block_seconds, 30);
    assert_eq!(three.blocks_per_hour, 6);

    // Dichtes Budget passt nicht in 30-Sekunden-Blöcke mit der Sperrzeit.
    let eight = plan_next_block(now(), Some(now() - Duration::hours(1)), 8, 0, None, 480);
    assert_eq!(eight.block_seconds, 60);
    assert_eq!(eight.blocks_per_hour, 8);

    // Budget ausgeschöpft: kein weiterer Block in dieser Stunde.
    let spent = plan_next_block(now(), Some(now() - Duration::hours(1)), 3, 180, None, 480);
    assert!(spent.next_block_at.is_none());
}

#[test]
fn ausgeschaltet_haelt_still() {
    let mut value = base(Strategy::Smart);
    value.settings.enabled = false;
    assert_eq!(decide(&value).reason, "disabled");
    assert_eq!(decide(&value).action, DecisionAction::None);
}

#[test]
fn snooze_strategie_bewegt_nur_geplante_werbung() {
    let mut value = base(Strategy::Snooze);
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert_eq!(decide(&value).reason, "twitch_ad_moved");

    value.snooze_count = 0;
    assert_eq!(decide(&value).reason, "no_snoozes");

    value.snooze_count = 1;
    value.next_ad_at = None;
    assert_eq!(decide(&value).reason, "cooldown");
    assert_eq!(decide(&value).action, DecisionAction::None);
}

#[test]
fn eigenes_budget_startet_block_bei_ruhigem_chat() {
    let value = base(Strategy::Smart);
    assert_eq!(
        decide(&value).action,
        DecisionAction::Commercial {
            duration_seconds: 30
        }
    );
    assert_eq!(decide(&value).reason, "quiet_chat");
}

#[test]
fn eigenes_budget_ohne_faelligen_block_wartet() {
    let mut value = base(Strategy::Smart);
    value.plan.next_block_at = Some(value.now + Duration::seconds(60));
    assert_eq!(decide(&value).reason, "cooldown");

    value.plan.next_block_at = None;
    assert_eq!(decide(&value).reason, "budget_reached");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);
}

#[test]
fn startschutz_raid_und_erstchatter_sperren() {
    let mut value = base(Strategy::Smart);
    value.stream_started_at = Some(value.now - Duration::minutes(14));
    assert_eq!(decide(&value).reason, "startup_protection");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);

    let mut value = base(Strategy::Smart);
    value.last_raid_at = Some(value.now - Duration::minutes(5));
    value.last_raider = Some("1337cammy".into());
    let decision = decide(&value);
    assert_eq!(decision.reason, "recent_raid");
    assert_eq!(decision.detail.as_deref(), Some("1337cammy"));

    let mut value = base(Strategy::Smart);
    value.last_first_chatter_at = Some(value.now - Duration::minutes(2));
    value.last_first_chatter = Some("neuling".into());
    assert_eq!(decide(&value).reason, "recent_first_chatter");
}

#[test]
fn match_fenster_und_match_sperre() {
    // Erste Minute eines Matches ist ein Werbefenster.
    let mut value = base(Strategy::Smart);
    value.steam_match_state = Some(steam_state(true, true));
    value.match_started_at = Some(value.now - Duration::seconds(30));
    assert_eq!(decide(&value).reason, "match_start_window");
    assert!(matches!(
        decide(&value).action,
        DecisionAction::Commercial { .. }
    ));

    // Ab Minute 1 sperrt das Match.
    value.match_started_at = Some(value.now - Duration::minutes(2));
    assert_eq!(decide(&value).reason, "in_match");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);

    // Queue oder Menü ist das Werbefenster.
    let mut value = base(Strategy::Smart);
    value.steam_match_state = Some(steam_state(false, true));
    assert_eq!(decide(&value).reason, "in_queue");
}

#[test]
fn nach_matchende_erst_warten_dann_chat_pruefen() {
    let mut value = base(Strategy::Smart);
    value.match_ended_at = Some(value.now - Duration::seconds(30));
    assert_eq!(decide(&value).reason, "post_match_wait");

    value.match_ended_at = Some(value.now - Duration::seconds(90));
    value.recent_chat_messages = 0;
    assert_eq!(decide(&value).reason, "post_match_quiet");

    value.recent_chat_messages = 3;
    assert_eq!(decide(&value).reason, "post_match_chat_active");
}

#[test]
fn ueberfaelliger_block_nimmt_den_am_wenigsten_schlechten_moment() {
    let mut value = base(Strategy::Smart);
    value.quiet_chat_messages = 2;
    // Nicht überfällig: verschieben.
    assert_eq!(decide(&value).reason, "cooldown");

    // Über eine halbe Blockperiode überfällig: Notbremse.
    value.plan.next_block_at = Some(value.now - Duration::seconds(600));
    assert_eq!(decide(&value).reason, "fallback_least_bad");
    assert!(matches!(
        decide(&value).action,
        DecisionAction::Commercial { .. }
    ));

    // Eine Sperre schlägt die Notbremse: im Match nie selbst starten.
    value.steam_match_state = Some(steam_state(true, true));
    value.match_started_at = Some(value.now - Duration::minutes(3));
    assert_eq!(decide(&value).reason, "in_match");
}

#[test]
fn twitch_plan_ist_budgetquelle_und_wird_vorgezogen() {
    // Aktiver Twitch-Plan: keine eigenen Blöcke, nur warten.
    let mut value = base(Strategy::Smart);
    value.next_ad_at = Some(value.now + Duration::minutes(6));
    assert_eq!(decide(&value).reason, "twitch_plan_active");
    assert_eq!(decide(&value).action, DecisionAction::None);

    // Offenes Queue-Fenster: geplante Werbung vorziehen.
    value.steam_match_state = Some(steam_state(false, true));
    let decision = decide(&value);
    assert_eq!(decision.reason, "pulled_forward");
    assert_eq!(
        decision.action,
        DecisionAction::Commercial {
            duration_seconds: 90
        }
    );

    // Sperre und anstehende Werbung: per Pause verschieben.
    let mut value = base(Strategy::Smart);
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    value.last_raid_at = Some(value.now - Duration::minutes(3));
    assert_eq!(decide(&value).reason, "twitch_ad_moved");
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
}

#[test]
fn dichter_plan_spart_pausen_fuer_wertvolle_momente() {
    // Sperre ist nur ein Erstchatter, Plan dicht: Pause aufsparen, Werbung läuft.
    let mut value = base(Strategy::Smart);
    value.plan_fit = "unprotectable";
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    value.last_first_chatter_at = Some(value.now - Duration::minutes(2));
    assert_eq!(decide(&value).reason, "twitch_plan_active");
    assert_eq!(decide(&value).action, DecisionAction::None);

    // Wertvoller Moment (im Match): Pause trotzdem einsetzen.
    let mut value = base(Strategy::Smart);
    value.plan_fit = "unprotectable";
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    value.steam_match_state = Some(steam_state(true, true));
    value.match_started_at = Some(value.now - Duration::minutes(3));
    assert_eq!(decide(&value).reason, "twitch_ad_moved");
}

#[test]
fn plan_schaetzung_ist_bei_wenig_daten_gutmuetig() {
    assert_eq!(assess_plan(false, None, 0, None, None), "good");
    assert_eq!(assess_plan(true, None, 1, Some(1800), None), "good");
    // Abstand größer als Match plus Queue: gut schützbar.
    assert_eq!(
        assess_plan(true, Some(2400), 1, Some(1500), Some(300)),
        "good"
    );
    // Abstand kleiner als der Zyklus, aber Pausen da: eng.
    assert_eq!(
        assess_plan(true, Some(1600), 1, Some(1500), Some(300)),
        "tight"
    );
    // Abstand kürzer als ein Match, keine Pausen: nicht schützbar.
    assert_eq!(
        assess_plan(true, Some(600), 0, Some(1500), Some(300)),
        "unprotectable"
    );
}

#[test]
fn live_state_freshness_hat_exakte_zeit_und_session_grenzen() {
    let now = now();
    let state = |observed_at| LiveState {
        is_live: true,
        active_session_id: Some(7),
        stream_started_at: Some(now - Duration::hours(1)),
        observed_at: Some(observed_at),
    };

    assert!(state(now - Duration::minutes(5)).is_fresh_live(now));
    assert!(!state(now - Duration::minutes(5) - Duration::seconds(1)).is_fresh_live(now));
    assert!(state(now + Duration::minutes(1)).is_fresh_live(now));
    assert!(!state(now + Duration::minutes(1) + Duration::seconds(1)).is_fresh_live(now));

    let mut invalid = state(now);
    invalid.is_live = false;
    assert!(!invalid.is_fresh_live(now));
    invalid.is_live = true;
    invalid.active_session_id = Some(0);
    assert!(!invalid.is_fresh_live(now));
    invalid.active_session_id = None;
    assert!(!invalid.is_fresh_live(now));
    invalid.active_session_id = Some(7);
    invalid.observed_at = None;
    assert!(!invalid.is_fresh_live(now));
}

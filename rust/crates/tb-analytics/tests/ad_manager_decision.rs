use chrono::{DateTime, Duration, TimeZone, Utc};
use tb_analytics::ad_manager::{
    ad_hint, ad_hint_text, assess_plan, decide, plan_next_block, AdHint, AdPlan, DecisionAction,
    DecisionInput, LiveState, Settings, SteamMatchState, Strategy, COMMERCIAL_SCOPE,
    HINT_WINDOW_SECS, READ_SCOPE, SNOOZE_SCOPE,
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
        // Budget/lock tests use an explicitly fresh non-match. Unknown has its own tests.
        steam_match_state: Some(steam_state(false, false)),
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
    let start = now() - Duration::hours(1);
    let three = plan_next_block(now(), Some(start), 3, 0, None, 480);
    assert_eq!(three.block_seconds, 30);
    assert_eq!(three.blocks_per_hour, 6);

    let last = now() - Duration::minutes(10);
    let three_next = plan_next_block(now(), Some(start), 3, 0, Some(last), 480);
    assert_eq!(
        three_next.next_block_at,
        Some(last + Duration::seconds(600))
    );

    let eight = plan_next_block(now(), Some(start), 8, 0, None, 480);
    assert_eq!(eight.block_seconds, 60);
    assert_eq!(eight.blocks_per_hour, 7);

    let eight_next = plan_next_block(now(), Some(start), 8, 0, Some(last), 480);
    assert_eq!(
        eight_next.next_block_at,
        Some(last + Duration::seconds(480))
    );

    let spent = plan_next_block(now(), Some(start), 3, 180, None, 480);
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
fn match_sperrt_ab_dem_ersten_erkannten_tick() {
    let mut value = base(Strategy::Smart);
    value.steam_match_state = Some(steam_state(true, true));
    value.match_started_at = Some(value.now - Duration::seconds(30));
    assert_eq!(decide(&value).reason, "in_match");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);

    value.match_started_at = None;
    assert_eq!(decide(&value).reason, "in_match");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);

    value.match_started_at = Some(value.now - Duration::minutes(2));
    assert_eq!(decide(&value).reason, "in_match");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);

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
fn matchende_schlaegt_erneute_queue() {
    let mut value = base(Strategy::Smart);
    value.steam_match_state = Some(steam_state(false, true));

    value.match_ended_at = Some(value.now - Duration::seconds(30));
    assert_eq!(decide(&value).reason, "post_match_wait");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);

    value.match_ended_at = Some(value.now - Duration::seconds(90));
    value.recent_chat_messages = 0;
    assert_eq!(decide(&value).reason, "post_match_quiet");

    value.recent_chat_messages = 3;
    assert_eq!(decide(&value).reason, "post_match_chat_active");

    value.match_ended_at = Some(value.now - Duration::seconds(150));
    assert_eq!(decide(&value).reason, "in_queue");
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
    value.last_ad_at = Some(value.now - Duration::minutes(10));
    let decision = decide(&value);
    assert_eq!(decision.reason, "pulled_forward");
    assert_eq!(
        decision.action,
        DecisionAction::Commercial {
            duration_seconds: 90
        }
    );

    value.last_ad_at = Some(value.now - Duration::minutes(7));
    assert_eq!(decide(&value).reason, "twitch_plan_active");
    assert_eq!(decide(&value).action, DecisionAction::None);

    // Sperre und anstehende Werbung: per Pause verschieben.
    let mut value = base(Strategy::Smart);
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    value.last_raid_at = Some(value.now - Duration::minutes(3));
    assert_eq!(decide(&value).reason, "twitch_ad_moved");
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
}

#[test]
fn twitch_plan_nutzt_matchrisiko_nur_ausserhalb_des_matches() {
    let mut value = base(Strategy::Smart);
    value.next_ad_at = Some(value.now + Duration::minutes(25));
    value.last_ad_at = Some(value.now - Duration::minutes(10));
    value.steam_match_state = Some(steam_state(false, true));
    value.plan_fit = "tight";

    let decision = decide(&value);
    assert_eq!(decision.reason, "pulled_forward");
    assert!(matches!(decision.action, DecisionAction::Commercial { .. }));

    value.steam_match_state = Some(steam_state(true, true));
    value.match_started_at = Some(value.now - Duration::seconds(30));
    assert_eq!(decide(&value).reason, "in_match");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);

    value.match_started_at = None;
    assert_eq!(decide(&value).reason, "in_match");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);
}

#[test]
fn imminente_twitch_werbung_wird_im_match_sofort_verschoben() {
    let mut value = base(Strategy::Smart);
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    value.steam_match_state = Some(steam_state(true, true));
    value.match_started_at = Some(value.now - Duration::seconds(5));
    value.stream_started_at = Some(value.now - Duration::minutes(5));
    value.last_first_chatter_at = Some(value.now - Duration::minutes(1));
    value.plan_fit = "unprotectable";
    value.snooze_count = 1;

    assert_eq!(decide(&value).reason, "twitch_ad_moved");
    assert_eq!(decide(&value).action, DecisionAction::Snooze);

    value.snooze_count = 0;
    assert_eq!(decide(&value).reason, "in_match");
    assert_eq!(decide(&value).action, DecisionAction::Postpone);
}

#[test]
fn twitch_plan_respektiert_matchende_vor_dem_vorziehen() {
    let mut value = base(Strategy::Smart);
    value.next_ad_at = Some(value.now + Duration::minutes(25));
    value.last_ad_at = Some(value.now - Duration::minutes(10));
    value.steam_match_state = Some(steam_state(false, true));
    value.plan_fit = "tight";
    value.match_ended_at = Some(value.now - Duration::seconds(30));
    assert_eq!(decide(&value).reason, "twitch_plan_active");

    value.match_ended_at = Some(value.now - Duration::seconds(90));
    value.recent_chat_messages = 2;
    assert_eq!(decide(&value).reason, "twitch_plan_active");

    value.recent_chat_messages = 0;
    assert_eq!(decide(&value).reason, "pulled_forward");
}

#[test]
fn imminente_twitch_werbung_wird_im_queuefenster_selbst_gestartet() {
    let mut value = base(Strategy::Smart);
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    value.last_ad_at = Some(value.now - Duration::minutes(10));
    value.steam_match_state = Some(steam_state(false, true));
    let decision = decide(&value);
    assert_eq!(decision.reason, "pulled_forward");
    assert_eq!(decision.action, DecisionAction::Commercial { duration_seconds: 90 });
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

fn own_block_plan(next_block_at: DateTime<Utc>) -> AdPlan {
    AdPlan {
        next_block_at: Some(next_block_at),
        block_seconds: 30,
        blocks_per_hour: 6,
        budget_used_seconds_this_hour: 0,
    }
}

#[test]
fn hinweis_vor_eigenem_block_traegt_dauer_und_schluessel() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = None;
    input.steam_match_state = Some(steam_state(false, true));
    let block_at = now() + Duration::seconds(30);
    input.plan = own_block_plan(block_at);
    let decision = decide(&input);
    let hint = ad_hint(&input, &decision, None, HINT_WINDOW_SECS).expect("Hinweis erwartet");
    assert_eq!(
        hint,
        AdHint {
            key: format!("own:{}", block_at.to_rfc3339()),
            duration_seconds: Some(30),
        }
    );
}

#[test]
fn kein_hinweis_bei_aktiver_sperre() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = None;
    input.steam_match_state = Some(steam_state(false, true));
    input.plan = own_block_plan(now() + Duration::seconds(30));
    input.last_raid_at = Some(now() - Duration::minutes(1));
    input.last_raider = Some("cammy".into());
    let decision = decide(&input);
    assert_eq!(ad_hint(&input, &decision, None, HINT_WINDOW_SECS), None);
}

#[test]
fn kein_hinweis_fuer_eigenen_block_ohne_offenes_fenster() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = None;
    input.steam_match_state = None;
    input.plan = own_block_plan(now() + Duration::seconds(30));
    let decision = decide(&input);
    assert_eq!(ad_hint(&input, &decision, None, HINT_WINDOW_SECS), None);
}

#[test]
fn kein_hinweis_bei_ausgeschaltetem_schalter_oder_manager() {
    let mut off = base(Strategy::Smart);
    off.next_ad_at = None;
    off.plan = own_block_plan(now() + Duration::seconds(30));
    off.settings.chat_notice_before_ad = false;
    let decision = decide(&off);
    assert_eq!(ad_hint(&off, &decision, None, HINT_WINDOW_SECS), None);

    let mut disabled = base(Strategy::Smart);
    disabled.next_ad_at = None;
    disabled.plan = own_block_plan(now() + Duration::seconds(30));
    disabled.settings.enabled = false;
    let decision = decide(&disabled);
    assert_eq!(ad_hint(&disabled, &decision, None, HINT_WINDOW_SECS), None);
}

#[test]
fn hinweis_vor_geplanter_twitch_werbung_die_laeuft() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = Some(now() + Duration::seconds(30));
    let decision = decide(&input);
    assert_eq!(decision.action, DecisionAction::None);
    let hint = ad_hint(&input, &decision, Some(90), HINT_WINDOW_SECS).expect("Hinweis erwartet");
    assert_eq!(hint.duration_seconds, Some(90));
    assert!(hint.key.starts_with("twitch:"));
}

#[test]
fn kein_hinweis_wenn_der_bot_die_twitch_werbung_verschiebt() {
    let mut input = base(Strategy::Snooze);
    input.next_ad_at = Some(now() + Duration::seconds(30));
    input.snooze_count = 2;
    let decision = decide(&input);
    assert_eq!(decision.action, DecisionAction::Snooze);
    assert_eq!(ad_hint(&input, &decision, Some(90), HINT_WINDOW_SECS), None);
}

#[test]
fn kein_hinweis_wenn_twitch_werbung_im_match_nicht_verschiebbar_ist() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = Some(now() + Duration::seconds(30));
    input.steam_match_state = Some(steam_state(true, true));
    input.match_started_at = Some(now() - Duration::seconds(5));
    input.snooze_count = 0;

    let decision = decide(&input);
    assert_eq!(decision.reason, "in_match");
    assert_eq!(decision.action, DecisionAction::Postpone);
    assert_eq!(ad_hint(&input, &decision, Some(90), HINT_WINDOW_SECS), None);
}

#[test]
fn kein_hinweis_wenn_die_werbung_noch_zu_weit_weg_ist() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = None;
    input.steam_match_state = Some(steam_state(false, true));
    input.plan = own_block_plan(now() + Duration::seconds(HINT_WINDOW_SECS + 20));
    let decision = decide(&input);
    assert_eq!(ad_hint(&input, &decision, None, HINT_WINDOW_SECS), None);
}

#[test]
fn hinweistext_wiederholt_die_variante_nicht_und_fuellt_die_dauer() {
    let (mit_dauer, index) = ad_hint_text(Some(60), None, 0, false);
    assert!(mit_dauer.contains("60 Sekunden"));
    let (_, folge) = ad_hint_text(Some(60), Some(index), 0, false);
    assert_ne!(folge, index);
    let (ohne_dauer, _) = ad_hint_text(None, None, 2, false);
    assert!(!ohne_dauer.contains("{dur}"));
    assert!(!ohne_dauer.contains("Sekunden lang"));
    assert!(!mit_dauer.contains('—'));
}

#[test]
fn sofort_hinweistext_nennt_kein_sekundenversprechen() {
    for seed in 0..6 {
        let (text, _) = ad_hint_text(Some(90), None, seed, true);
        assert!(text.contains("90 Sekunden"), "{text}");
        assert!(!text.contains("in etwa 30 Sekunden"), "{text}");
        assert!(!text.contains("in 30 Sekunden"), "{text}");
        assert!(!text.contains("halben Minute"), "{text}");
        assert!(!text.contains('—'), "{text}");
    }
    let (ohne, _) = ad_hint_text(None, None, 1, true);
    assert!(!ohne.contains("{dur}"));
}

#[test]
fn vorgezogene_werbung_bekommt_sofort_hinweis() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = Some(input.now + Duration::minutes(6));
    input.steam_match_state = Some(steam_state(false, true));
    let decision = decide(&input);
    assert_eq!(decision.reason, "pulled_forward");
    let hint = ad_hint(&input, &decision, None, HINT_WINDOW_SECS).expect("Hinweis erwartet");
    assert!(hint.key.starts_with("pull:"), "{}", hint.key);
    assert_eq!(hint.duration_seconds, Some(90));
    let (text, _) = ad_hint_text(
        hint.duration_seconds,
        None,
        0,
        hint.key.starts_with("pull:"),
    );
    assert!(!text.contains("in etwa 30 Sekunden"), "{text}");
}

#[test]
fn kein_hinweis_direkt_nach_matchende() {
    let mut input = base(Strategy::Smart);
    input.next_ad_at = None;
    input.steam_match_state = Some(steam_state(false, true));
    input.plan = own_block_plan(now() + Duration::seconds(30));
    input.match_ended_at = Some(now() - Duration::seconds(30));
    let decision = decide(&input);
    assert_eq!(ad_hint(&input, &decision, None, HINT_WINDOW_SECS), None);
}

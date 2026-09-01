use chrono::{Duration, TimeZone, Utc};
use tb_analytics::ad_manager::{
    decide, DecisionAction, DecisionInput, LiveState, Settings, Strategy, COMMERCIAL_SCOPE,
    READ_SCOPE, SNOOZE_SCOPE,
};

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap()
}

fn input(strategy: Strategy) -> DecisionInput {
    let now = now();
    DecisionInput {
        now,
        settings: Settings {
            enabled: true,
            strategy,
            ..Settings::default()
        },
        stream_started_at: Some(now - Duration::hours(1)),
        next_ad_at: Some(now + Duration::seconds(60)),
        last_ad_at: Some(now - Duration::minutes(30)),
        snooze_count: 1,
        quiet_chat_messages: 0,
        chat_ingest_healthy: true,
    }
}

#[test]
fn strategien_und_scopes_sind_ein_strikter_oeffentlicher_vertrag() {
    assert_eq!(Strategy::parse("monitor"), Some(Strategy::Monitor));
    assert_eq!(Strategy::parse("snooze"), Some(Strategy::Snooze));
    assert_eq!(Strategy::parse("smart"), Some(Strategy::Smart));
    for invalid in ["", "SMART", " smart ", "intelligent", "💸"] {
        assert_eq!(Strategy::parse(invalid), None, "{invalid:?}");
    }

    assert_eq!(Strategy::Monitor.required_scopes(), &[READ_SCOPE]);
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
fn einstellungsgrenzen_akzeptieren_nur_den_definierten_bereich() {
    let mut settings = Settings::default();

    for duration in [30, 60, 90, 120, 150, 180] {
        settings.ad_duration_seconds = duration;
        assert!(settings.validate().is_ok(), "Dauer {duration}");
    }
    for invalid in [-30, 0, 29, 31, 179, 181, 10_000] {
        settings.ad_duration_seconds = invalid;
        assert!(settings.validate().is_err(), "Dauer {invalid}");
    }
    settings.ad_duration_seconds = 90;

    for (field, valid_min, valid_max, below, above) in [
        ("Mindestabstand", 8, 180, 7, 181),
        ("Startschutz", 0, 180, -1, 181),
        ("Chat-Ruhe", 0, 60, -1, 61),
        ("Vorlauf", 10, 300, 9, 301),
    ] {
        match field {
            "Mindestabstand" => settings.min_interval_minutes = valid_min,
            "Startschutz" => settings.startup_delay_minutes = valid_min,
            "Chat-Ruhe" => settings.quiet_window_minutes = valid_min,
            "Vorlauf" => settings.action_lead_seconds = valid_min,
            _ => unreachable!(),
        }
        assert!(settings.validate().is_ok(), "{field} Untergrenze");
        match field {
            "Mindestabstand" => settings.min_interval_minutes = valid_max,
            "Startschutz" => settings.startup_delay_minutes = valid_max,
            "Chat-Ruhe" => settings.quiet_window_minutes = valid_max,
            "Vorlauf" => settings.action_lead_seconds = valid_max,
            _ => unreachable!(),
        }
        assert!(settings.validate().is_ok(), "{field} Obergrenze");
        match field {
            "Mindestabstand" => settings.min_interval_minutes = below,
            "Startschutz" => settings.startup_delay_minutes = below,
            "Chat-Ruhe" => settings.quiet_window_minutes = below,
            "Vorlauf" => settings.action_lead_seconds = below,
            _ => unreachable!(),
        }
        assert!(settings.validate().is_err(), "{field} unter Bereich");
        match field {
            "Mindestabstand" => settings.min_interval_minutes = valid_min,
            "Startschutz" => settings.startup_delay_minutes = valid_min,
            "Chat-Ruhe" => settings.quiet_window_minutes = valid_min,
            "Vorlauf" => settings.action_lead_seconds = valid_min,
            _ => unreachable!(),
        }
        match field {
            "Mindestabstand" => settings.min_interval_minutes = above,
            "Startschutz" => settings.startup_delay_minutes = above,
            "Chat-Ruhe" => settings.quiet_window_minutes = above,
            "Vorlauf" => settings.action_lead_seconds = above,
            _ => unreachable!(),
        }
        assert!(settings.validate().is_err(), "{field} über Bereich");
        settings = Settings::default();
    }
}

#[test]
fn harte_gates_laufen_vor_jeder_aktion() {
    let mut value = input(Strategy::Smart);
    value.settings.enabled = false;
    assert_eq!(decide(&value).reason, "disabled");
    assert_eq!(decide(&value).action, DecisionAction::None);

    value = input(Strategy::Monitor);
    assert_eq!(decide(&value).reason, "monitor_only");

    value = input(Strategy::Smart);
    value.next_ad_at = None;
    assert_eq!(decide(&value).reason, "no_next_ad");
}

#[test]
fn vorlauf_und_stale_grenze_sind_inklusive() {
    let mut value = input(Strategy::Smart);
    value.settings.action_lead_seconds = 60;

    value.next_ad_at = Some(value.now + Duration::seconds(61));
    assert_eq!(decide(&value).reason, "outside_lead_window");

    value.next_ad_at = Some(value.now + Duration::seconds(60));
    assert_eq!(
        decide(&value).action,
        DecisionAction::Commercial {
            duration_seconds: 90
        }
    );

    value.next_ad_at = Some(value.now);
    assert_eq!(
        decide(&value).action,
        DecisionAction::Commercial {
            duration_seconds: 90
        }
    );
    value.next_ad_at = Some(value.now - Duration::milliseconds(1));
    assert_eq!(decide(&value).action, DecisionAction::None);
    assert_eq!(decide(&value).reason, "ad_already_due");
}

#[test]
fn snooze_strategie_verbraucht_nur_vorhandene_snoozes() {
    let mut value = input(Strategy::Snooze);
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert_eq!(decide(&value).reason, "snooze_due");

    value.snooze_count = 0;
    assert_eq!(decide(&value).action, DecisionAction::None);
    assert_eq!(decide(&value).reason, "no_snoozes");

    value.snooze_count = -1;
    assert_eq!(decide(&value).action, DecisionAction::None);
}

#[test]
fn smart_startschutz_endet_exakt_an_der_minutengrenze() {
    let mut value = input(Strategy::Smart);
    value.settings.startup_delay_minutes = 15;
    value.stream_started_at = Some(value.now - Duration::minutes(15) + Duration::seconds(1));
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert_eq!(decide(&value).reason, "startup_protection");

    value.stream_started_at = Some(value.now - Duration::minutes(15));
    assert_eq!(
        decide(&value).action,
        DecisionAction::Commercial {
            duration_seconds: 90
        }
    );
}

#[test]
fn smart_mindestabstand_chatruhe_und_fallbacks() {
    let mut value = input(Strategy::Smart);
    value.settings.ad_duration_seconds = 180;

    value.last_ad_at = Some(value.now - Duration::minutes(30) + Duration::seconds(1));
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert_eq!(decide(&value).reason, "commercial_cooldown");

    value.last_ad_at = Some(value.now - Duration::minutes(30));
    assert_eq!(
        decide(&value).action,
        DecisionAction::Commercial {
            duration_seconds: 180
        }
    );

    value.last_ad_at = None;
    value.quiet_chat_messages = 1;
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert_eq!(decide(&value).reason, "chat_active");

    value.snooze_count = 0;
    assert_eq!(decide(&value).action, DecisionAction::None);
    assert_eq!(decide(&value).reason, "chat_active_no_snooze");

    value.last_ad_at = Some(value.now);
    assert_eq!(decide(&value).reason, "cooldown_no_snooze");
}

#[test]
fn smart_ist_bei_unbekanntem_streamstart_oder_krankem_chat_fail_closed() {
    let mut value = input(Strategy::Smart);
    value.stream_started_at = None;
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert_eq!(decide(&value).reason, "stream_start_unknown");

    value = input(Strategy::Smart);
    value.chat_ingest_healthy = false;
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert_eq!(decide(&value).reason, "chat_ingest_unhealthy");

    value.settings.quiet_window_minutes = 0;
    assert!(matches!(
        decide(&value).action,
        DecisionAction::Commercial { .. }
    ));
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

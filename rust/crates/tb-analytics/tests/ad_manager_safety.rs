use chrono::{DateTime, Duration, Utc};
use tb_analytics::ad_manager::{
    ad_hint, decide, AdPlan, DecisionAction, DecisionInput, Settings, SteamMatchState, Strategy,
};

fn input() -> DecisionInput {
    let now = DateTime::from_timestamp(1_780_000_000, 0).unwrap();
    DecisionInput {
        now,
        settings: Settings {
            enabled: true,
            strategy: Strategy::Smart,
            ..Settings::default()
        },
        stream_started_at: Some(now - Duration::hours(2)),
        next_ad_at: None,
        last_ad_at: None,
        snooze_count: 3,
        quiet_chat_messages: 0,
        recent_chat_messages: 0,
        chat_ingest_healthy: true,
        steam_match_state: None,
        plan: AdPlan {
            next_block_at: Some(now - Duration::hours(1)),
            block_seconds: 30,
            blocks_per_hour: 6,
            budget_used_seconds_this_hour: 0,
        },
        match_started_at: None,
        match_ended_at: None,
        last_raid_at: None,
        last_raider: None,
        last_first_chatter_at: None,
        last_first_chatter: None,
        retry_after_seconds: 480,
        pull_forward_seconds: 90,
        plan_fit: "unprotectable",
    }
}
fn queue(now: DateTime<Utc>) -> SteamMatchState {
    SteamMatchState {
        in_match: false,
        in_deadlock: true,
        hero: None,
        stage: Some("lobby".into()),
        observed_at: now,
    }
}
#[test]
fn unknown_status_blocks_quiet_chat_and_overdue_fallback() {
    let mut value = input();
    for messages in [0, 99] {
        value.quiet_chat_messages = messages;
        let d = decide(&value);
        assert_eq!(d.reason, "match_status_unknown");
        assert_eq!(d.action, DecisionAction::Postpone);
    }
    value.next_ad_at = Some(value.now + Duration::minutes(10));
    assert_eq!(decide(&value).reason, "match_status_unknown");
}
#[test]
fn stale_or_future_queue_is_not_a_window() {
    let mut value = input();
    for stamp in [
        value.now - Duration::seconds(181),
        value.now + Duration::seconds(31),
    ] {
        value.steam_match_state = Some(queue(stamp));
        assert_eq!(decide(&value).reason, "match_status_unknown");
        assert_eq!(decide(&value).action, DecisionAction::Postpone);
    }
    value.steam_match_state = Some(queue(value.now));
    assert!(matches!(
        decide(&value).action,
        DecisionAction::Commercial { .. }
    ));
}
#[test]
fn unknown_status_uses_snooze_even_with_dense_plan_and_short_lead() {
    let mut value = input();
    value.settings.action_lead_seconds = 10;
    value.next_ad_at = Some(value.now + Duration::seconds(50));
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    value.next_ad_at = Some(value.now + Duration::seconds(25));
    assert_eq!(decide(&value).action, DecisionAction::Snooze);
    assert!(ad_hint(&value, &decide(&value), Some(30), 45).is_none());
}
#[test]
fn depleted_snoozes_never_start_an_own_replacement() {
    let mut value = input();
    value.next_ad_at = Some(value.now + Duration::seconds(30));
    value.snooze_count = 0;
    let d = decide(&value);
    assert_eq!(d.action, DecisionAction::Postpone);
    assert_eq!(d.reason, "match_status_unknown");
    assert!(ad_hint(&value, &d, Some(30), 45).is_none());
}

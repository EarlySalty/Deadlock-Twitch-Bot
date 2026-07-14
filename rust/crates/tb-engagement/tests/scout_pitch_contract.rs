use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tb_engagement::scout_pitch::{
    decide, parse_judge_json, parse_pitch_json, Decision, DecisionInput, JudgeState, LedgerAction,
    LedgerEntry, ScoutPitchLedger, TriggerType,
};

fn input() -> DecisionInput {
    DecisionInput {
        trigger_type: TriggerType::SpamBots,
        blacklisted: false,
        cooldown_active: false,
        posted_for_stream: false,
        judge: JudgeState::Triggered { confidence: 0.9 },
        sanitized_message_count: 1,
    }
}

#[test]
fn decide_contract_covers_every_suppression_gate() {
    let mut case = input();
    case.cooldown_active = true;
    assert_eq!(
        decide(&case),
        Decision::Record(LedgerAction::SuppressedCooldown)
    );

    let mut case = input();
    case.posted_for_stream = true;
    assert_eq!(
        decide(&case),
        Decision::Record(LedgerAction::SuppressedPerStreamLimit)
    );

    let mut case = input();
    case.blacklisted = true;
    assert_eq!(
        decide(&case),
        Decision::Record(LedgerAction::SuppressedBlacklist)
    );

    let mut case = input();
    case.judge = JudgeState::Triggered { confidence: 0.69 };
    assert_eq!(
        decide(&case),
        Decision::Record(LedgerAction::SuppressedLowConfidence)
    );

    let mut case = input();
    case.sanitized_message_count = 0;
    assert_eq!(
        decide(&case),
        Decision::Record(LedgerAction::SuppressedSanitizer)
    );
}

#[test]
fn decide_contract_covers_judge_and_post_outcomes() {
    let mut case = input();
    case.judge = JudgeState::None;
    assert_eq!(decide(&case), Decision::Record(LedgerAction::JudgeNone));

    let mut case = input();
    case.judge = JudgeState::Error;
    assert_eq!(decide(&case), Decision::Record(LedgerAction::JudgeError));

    let mut case = input();
    case.judge = JudgeState::Timeout;
    assert_eq!(decide(&case), Decision::Record(LedgerAction::JudgeTimeout));

    assert_eq!(decide(&input()), Decision::Post);

    let mut radar = input();
    radar.trigger_type = TriggerType::NewStreamer;
    radar.judge = JudgeState::NotNeeded;
    radar.sanitized_message_count = 0;
    assert_eq!(decide(&radar), Decision::Post);
}

#[test]
fn judge_json_parses_valid_and_rejects_broken_payloads() {
    let verdict = parse_judge_json(
        r#"{"trigger":"spam_bots","confidence":0.91,"quote":"schon wieder ein bot"}"#,
    )
    .expect("valides Judge-JSON");
    assert_eq!(verdict.trigger_type, Some(TriggerType::SpamBots));
    assert_eq!(verdict.confidence, 0.91);
    assert_eq!(verdict.quote, "schon wieder ein bot");

    let none = parse_judge_json(r#"{"trigger":"none","confidence":0.2,"quote":""}"#)
        .expect("none ist valides Judge-JSON");
    assert_eq!(none.trigger_type, None);

    assert!(parse_judge_json("kein json").is_err());
    assert!(parse_judge_json(r#"{"trigger":"unbekannt","confidence":0.8,"quote":"x"}"#).is_err());
    assert!(parse_judge_json(r#"{"trigger":"lfg","confidence":1.2,"quote":"x"}"#).is_err());

    // MiniMax-M3-Realität (Live-judge_error 2026-07-14): Reasoning-Block,
    // Code-Fence oder Prosa um das JSON herum muessen toleriert werden.
    let think = parse_judge_json(
        "<think>der streamer klagt ueber bots</think>\n{\"trigger\":\"spam_bots\",\"confidence\":0.8,\"quote\":\"bots overall\"}",
    )
    .expect("Think-Block vor dem JSON");
    assert_eq!(think.trigger_type, Some(TriggerType::SpamBots));
    let fenced = parse_judge_json(
        "```json\n{\"trigger\":\"lfg\",\"confidence\":0.9,\"quote\":\"wer will zocken\"}\n```",
    )
    .expect("Code-Fence um das JSON");
    assert_eq!(fenced.trigger_type, Some(TriggerType::Lfg));
    let prose = parse_judge_json(
        "Hier ist meine Analyse: {\"trigger\":\"none\",\"confidence\":0.3,\"quote\":\"\"} Ende.",
    )
    .expect("Prosa um das JSON");
    assert_eq!(prose.trigger_type, None);
}

#[test]
fn pitch_json_requires_a_messages_array() {
    assert_eq!(
        parse_pitch_json(r#"{"messages":["eins","zwei"]}"#).expect("valides Pitch-JSON"),
        vec!["eins", "zwei"]
    );
    assert!(parse_pitch_json(r#"{"messages":"eins"}"#).is_err());
    assert!(parse_pitch_json("kaputt").is_err());
}

#[tokio::test]
async fn ledger_writes_every_action_and_blacklist_is_case_insensitive() {
    let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    let schema = format!("tb_scout_pitch_{}", std::process::id());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("Test-DB erreichbar");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("altes Schema loeschbar");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("Schema erstellbar");
    // Repo-Muster (tb-db/tests/hermetic.rs): die Migrationen brauchen
    // create_hypertable, also muss die Extension vor dem Migrator existieren.
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&admin)
        .await
        .expect("timescaledb-Extension verfuegbar");
    admin.close().await;

    let options = PgConnectOptions::from_str(&dsn)
        .expect("valide Test-DSN")
        .options([("search_path", format!("{schema},public").as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("Schema-Pool erstellbar");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("Migrationen laufen");

    let ledger = ScoutPitchLedger::new(pool.clone());
    for action in LedgerAction::ALL {
        ledger
            .record(&LedgerEntry {
                streamer_login: "Tester".to_string(),
                trigger_type: TriggerType::SpamBots,
                judge_input_excerpt: Some("auszug".to_string()),
                judge_verdict: "spam_bots".to_string(),
                confidence: Some(0.9),
                action,
                detail: Some("detail".to_string()),
                discord_message_id: None,
            })
            .await
            .expect("Ledger-Eintrag schreibbar");
    }
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM twitch_scout_pitch_ledger ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("Actions lesbar");
    assert_eq!(
        actions,
        LedgerAction::ALL
            .into_iter()
            .map(|action| action.as_str().to_string())
            .collect::<Vec<_>>()
    );

    sqlx::query(
        "INSERT INTO twitch_scout_pitch_blacklist (streamer_login, reason) VALUES ('TeStEr', 'manuell')",
    )
    .execute(&pool)
    .await
    .expect("Blacklist-Eintrag schreibbar");
    assert!(ledger
        .is_blacklisted("tester")
        .await
        .expect("Blacklist lesbar"));
}

#[tokio::test]
async fn feedback_sync_selects_only_recent_posted_messages_and_updates_found_state() {
    let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    let schema = format!("tb_scout_feedback_{}", std::process::id());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("Test-DB erreichbar");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("altes Schema loeschbar");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("Schema erstellbar");
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&admin)
        .await
        .expect("timescaledb-Extension verfuegbar");
    admin.close().await;

    let options = PgConnectOptions::from_str(&dsn)
        .expect("valide Test-DSN")
        .options([("search_path", format!("{schema},public").as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("Schema-Pool erstellbar");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("Migrationen laufen");

    for (action, message_id, age) in [
        ("posted", Some("recent"), "1 day"),
        ("posted", Some("old"), "15 days"),
        ("discord_error", Some("failed"), "1 day"),
        ("posted", None, "1 day"),
    ] {
        sqlx::query(
            "INSERT INTO twitch_scout_pitch_ledger \
             (streamer_login, trigger_type, judge_verdict, action, discord_message_id, created_at) \
             VALUES ('tester', 'spam_bots', 'spam_bots', $1, $2, NOW() - $3::interval)",
        )
        .bind(action)
        .bind(message_id)
        .bind(age)
        .execute(&pool)
        .await
        .expect("Test-Ledger-Eintrag schreibbar");
    }

    let ledger = ScoutPitchLedger::new(pool.clone());
    let targets = ledger
        .feedback_sync_targets()
        .await
        .expect("Sync-Ziele lesbar");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].discord_message_id, "recent");

    ledger
        .update_feedback(targets[0].id, Some(4), Some(1))
        .await
        .expect("gefundenes Feedback aktualisierbar");
    let found: (Option<i32>, Option<i32>, bool) = sqlx::query_as(
        "SELECT feedback_up, feedback_down, feedback_synced_at IS NOT NULL \
         FROM twitch_scout_pitch_ledger WHERE id = $1",
    )
    .bind(targets[0].id)
    .fetch_one(&pool)
    .await
    .expect("Feedback lesbar");
    assert_eq!(found, (Some(4), Some(1), true));

    sqlx::query(
        "UPDATE twitch_scout_pitch_ledger \
         SET feedback_up = 7, feedback_down = 8, feedback_synced_at = NULL WHERE id = $1",
    )
    .bind(targets[0].id)
    .execute(&pool)
    .await
    .expect("Feedback-Testzustand schreibbar");
    ledger
        .update_feedback(targets[0].id, None, None)
        .await
        .expect("nicht gefundenes Feedback als synchronisiert markierbar");
    let not_found: (Option<i32>, Option<i32>, bool) = sqlx::query_as(
        "SELECT feedback_up, feedback_down, feedback_synced_at IS NOT NULL \
         FROM twitch_scout_pitch_ledger WHERE id = $1",
    )
    .bind(targets[0].id)
    .fetch_one(&pool)
    .await
    .expect("unveraendertes Feedback lesbar");
    assert_eq!(not_found, (Some(7), Some(8), true));
}

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

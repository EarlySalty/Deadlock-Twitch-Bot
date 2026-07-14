use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::crew_guard::{persist_radar_log, CrewRadarLog};
use tb_chat::style_score::{build_centroid, score, StyleBreakdown};

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        pool_in_schema(&dsn, $schema).await
    }};
}

async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .expect("Test-DB-Verbindung");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("altes Testschema löschen");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("Testschema anlegen");
    admin.close().await;

    let options = PgConnectOptions::from_str(dsn)
        .expect("Test-DSN")
        .options([("search_path", schema)]);
    PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("Testschema verbinden")
}

#[tokio::test]
async fn ledger_speichert_auch_clean_entscheidung_vollstaendig() {
    let pool = pool_or_skip!("tb_crew_radar_ledger");
    sqlx::query(
        "CREATE TABLE twitch_crew_radar_log (\
         id BIGSERIAL PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
         channel_login TEXT NOT NULL, chatter_login TEXT NOT NULL, chatter_id TEXT, \
         account_age_days BIGINT, style_score SMALLINT NOT NULL, style_breakdown JSONB NOT NULL, \
         time_window_match BOOLEAN NOT NULL, messages JSONB NOT NULL, llm_verdict TEXT NOT NULL, \
         llm_confidence REAL, llm_reasoning TEXT, action_taken TEXT NOT NULL DEFAULT 'none', \
         source TEXT NOT NULL DEFAULT 'network')",
    )
    .execute(&pool)
    .await
    .expect("Ledger-Tabelle");

    persist_radar_log(
        &pool,
        &CrewRadarLog {
            channel_login: "kanal".to_string(),
            chatter_login: "viewer".to_string(),
            chatter_id: Some("42".to_string()),
            account_age_days: Some(3),
            style_score: 5,
            style_breakdown: StyleBreakdown {
                pitch: 0,
                campaign: 0,
                typo: 0,
                bro: 0,
                lowercase: 0,
                opener: 5,
                cosine: 0,
            },
            time_window_match: false,
            messages: vec!["Was geht".to_string()],
            llm_verdict: "clean".to_string(),
            llm_confidence: Some(0.2),
            llm_reasoning: Some("harmlos".to_string()),
            action_taken: "none".to_string(),
            source: "network".to_string(),
        },
    )
    .await
    .expect("Ledger schreiben");

    let row: (String, String, Option<String>, i64, i16, serde_json::Value, bool, serde_json::Value, String, Option<f32>, Option<String>, String, String) =
        sqlx::query_as("SELECT channel_login, chatter_login, chatter_id, account_age_days, style_score, style_breakdown, time_window_match, messages, llm_verdict, llm_confidence, llm_reasoning, action_taken, source FROM twitch_crew_radar_log")
            .fetch_one(&pool)
            .await
            .expect("Ledger lesen");
    assert_eq!(row.0, "kanal");
    assert_eq!(row.1, "viewer");
    assert_eq!(row.2.as_deref(), Some("42"));
    assert_eq!(row.3, 3);
    assert_eq!(row.4, 5);
    assert_eq!(row.5["opener"], 5);
    assert!(!row.6);
    assert_eq!(row.7, serde_json::json!(["Was geht"]));
    assert_eq!(row.8, "clean");
    assert_eq!(row.9, Some(0.2));
    assert_eq!(row.10.as_deref(), Some("harmlos"));
    assert_eq!(row.11, "none");
    assert_eq!(row.12, "network");
}

#[tokio::test]
async fn centroid_wird_aus_chat_dokumenten_gebaut() {
    let pool = pool_or_skip!("tb_crew_radar_centroid");
    sqlx::query(
        "CREATE TABLE twitch_chat_messages (\
         chatter_login TEXT, content TEXT, message_ts TIMESTAMPTZ NOT NULL DEFAULT now())",
    )
    .execute(&pool)
    .await
    .expect("Chat-Tabelle");
    for (login, content) in [
        ("crew", "hast du bock auf unseren dc"),
        ("crew", "wir sind eine neue community"),
        ("crew", "kompetitiv spielen bro"),
        ("crew", "komm gern zu uns"),
        ("crew", "discord ist im aufbau"),
        ("normal", "gutes spiel heute"),
        ("normal", "welchen held spielst du"),
        ("normal", "gleich noch eine runde"),
        ("normal", "das war knapp"),
        ("normal", "bis morgen"),
    ] {
        sqlx::query("INSERT INTO twitch_chat_messages (chatter_login, content) VALUES ($1, $2)")
            .bind(login)
            .bind(content)
            .execute(&pool)
            .await
            .expect("Fixture schreiben");
    }

    let centroid = build_centroid(&pool, &["crew"])
        .await
        .expect("Zentroid bauen");
    let result = score(&["wir sind eine neue community".to_string()], &centroid);
    assert!(result.breakdown.cosine > 0, "{result:?}");
}

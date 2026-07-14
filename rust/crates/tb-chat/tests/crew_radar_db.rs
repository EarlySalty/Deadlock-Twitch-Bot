use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::crew_guard::{persist_radar_log, CrewJudge, CrewRadarLog, CrewVerdict};
use tb_chat::style_score::{build_centroid, score, StyleBreakdown};
use tb_chat::types::{ChatBadge, ChatMessageBody};
use tb_chat::{BanOutcome, ChatApi, ChatMessageEvent, CrewGuard, ModAlerter, SendOutcome};
use tokio::time::{sleep, Duration};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

struct StubJudge;

#[async_trait]
impl CrewJudge for StubJudge {
    async fn judge(&self, _content: &str, _recent_context: &[String]) -> CrewVerdict {
        CrewVerdict::unsure()
    }
}

struct StubApi;

#[async_trait]
impl ChatApi for StubApi {
    async fn send_message(
        &self,
        _broadcaster_id: &str,
        _message: &str,
    ) -> Result<SendOutcome, String> {
        unreachable!("CrewGuard darf keine Chat-Nachricht senden")
    }

    async fn send_announcement(
        &self,
        _broadcaster_id: &str,
        _message: &str,
        _color: &str,
    ) -> Result<bool, String> {
        unreachable!("CrewGuard darf kein Announcement senden")
    }

    async fn ban_user(
        &self,
        _broadcaster_id: &str,
        _target_user_id: &str,
        _reason: &str,
    ) -> Result<BanOutcome, String> {
        unreachable!("CrewGuard darf nicht bannen")
    }

    async fn timeout_user(
        &self,
        _broadcaster_id: &str,
        _target_user_id: &str,
        _duration_secs: u32,
        _reason: &str,
    ) -> Result<BanOutcome, String> {
        unreachable!("CrewGuard darf keinen Timeout setzen")
    }

    async fn unban_user(
        &self,
        _broadcaster_id: &str,
        _target_user_id: &str,
    ) -> Result<bool, String> {
        unreachable!("CrewGuard darf nicht entbannen")
    }

    async fn delete_message(
        &self,
        _broadcaster_id: &str,
        _message_id: &str,
    ) -> Result<bool, String> {
        unreachable!("CrewGuard darf keine Nachricht loeschen")
    }

    async fn user_created_at(&self, _user_id: &str) -> Result<Option<DateTime<Utc>>, String> {
        Ok(None)
    }

    async fn resolve_user_id(&self, _login: &str) -> Result<Option<String>, String> {
        unreachable!("CrewGuard darf keine User-ID aufloesen")
    }

    async fn bot_user_id(&self) -> String {
        "bot-id".to_string()
    }
}

async fn prepare_observe_schema(pool: &PgPool, first_time: bool) {
    for statement in [
        "CREATE TABLE twitch_stream_sessions (id BIGINT PRIMARY KEY, started_at TIMESTAMPTZ NOT NULL DEFAULT now(), ended_at TIMESTAMPTZ)",
        "CREATE TABLE twitch_session_chatters (session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL, is_first_time_streamer BOOLEAN)",
        "CREATE TABLE twitch_chatter_rollup (streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL)",
        "CREATE TABLE twitch_crew_radar_log (id BIGSERIAL PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), channel_login TEXT NOT NULL, chatter_login TEXT NOT NULL, chatter_id TEXT, account_age_days BIGINT, style_score SMALLINT NOT NULL, style_breakdown JSONB NOT NULL, time_window_match BOOLEAN NOT NULL, messages JSONB NOT NULL, llm_verdict TEXT NOT NULL, llm_confidence REAL, llm_reasoning TEXT, action_taken TEXT NOT NULL DEFAULT 'none', source TEXT NOT NULL DEFAULT 'network')",
        "INSERT INTO twitch_stream_sessions (id) VALUES (1)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("Observe-Fixture anlegen");
    }
    sqlx::query("INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login, is_first_time_streamer) VALUES (1, 'kanal', 'viewer', $1), (1, 'kanal', 'helmbombenricky', $1)")
        .bind(first_time)
        .execute(pool)
        .await
        .expect("Erstschreiber-Fixture anlegen");
}

async fn observe_guard(pool: PgPool, server: &MockServer, event: &ChatMessageEvent) {
    Mock::given(method("POST"))
        .and(path("/changelog"))
        .respond_with(ResponseTemplate::new(204))
        .mount(server)
        .await;
    let guard = CrewGuard::new(
        true,
        Arc::new(StubJudge),
        Arc::new(ModAlerter::with_endpoint(
            reqwest::Client::new(),
            format!("{}/changelog", server.uri()),
        )),
        pool,
        "bot-id".to_string(),
        Arc::new(StubApi),
        Arc::new(Default::default()),
    );
    guard.observe(event);
}

fn event(login: &str, chatter_id: &str, content: &str, badge: Option<&str>) -> ChatMessageEvent {
    ChatMessageEvent {
        broadcaster_user_id: "channel-id".to_string(),
        broadcaster_user_login: "kanal".to_string(),
        chatter_user_id: chatter_id.to_string(),
        chatter_user_login: login.to_string(),
        message_id: "message-id".to_string(),
        message: ChatMessageBody {
            text: content.to_string(),
            fragments: Vec::new(),
        },
        badges: badge
            .map(|set_id| ChatBadge {
                set_id: set_id.to_string(),
                id: String::new(),
                info: String::new(),
            })
            .into_iter()
            .collect(),
        ..Default::default()
    }
}

async fn wait_for_ledger(pool: &PgPool, expected: i64) {
    for _ in 0..50 {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_crew_radar_log")
            .fetch_one(pool)
            .await
            .expect("Ledger zaehlen");
        if count >= expected {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("Ledger erhielt nicht {expected} Zeilen");
}

async fn wait_for_alerts(server: &MockServer, expected: usize) {
    for _ in 0..50 {
        if server
            .received_requests()
            .await
            .expect("Discord-Requests lesen")
            .len()
            >= expected
        {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("Discord erhielt nicht {expected} Meldungen");
}

async fn ledger_verdicts(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT llm_verdict FROM twitch_crew_radar_log ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("Ledger lesen")
}

#[tokio::test]
async fn observe_meldet_hard_id_auch_bei_etabliertem_chatter() {
    let pool = pool_or_skip!("tb_crew_observe_hard_id_returning");
    prepare_observe_schema(&pool, false).await;
    let server = MockServer::start().await;

    observe_guard(
        pool.clone(),
        &server,
        &event("helmbombenricky", "147713656", "hallo", None),
    )
    .await;

    wait_for_ledger(&pool, 1).await;
    wait_for_alerts(&server, 1).await;
    assert_eq!(ledger_verdicts(&pool).await, ["hard_hit"]);
}

#[tokio::test]
async fn observe_meldet_hard_id_auch_mit_subscriber_badge() {
    let pool = pool_or_skip!("tb_crew_observe_hard_id_sub");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;

    observe_guard(
        pool.clone(),
        &server,
        &event("helmbombenricky", "147713656", "hallo", Some("subscriber")),
    )
    .await;

    wait_for_ledger(&pool, 1).await;
    wait_for_alerts(&server, 1).await;
    assert_eq!(ledger_verdicts(&pool).await, ["hard_hit"]);
}

#[tokio::test]
async fn observe_meldet_hard_invite_auch_bei_etabliertem_chatter() {
    let pool = pool_or_skip!("tb_crew_observe_hard_invite_returning");
    prepare_observe_schema(&pool, false).await;
    let server = MockServer::start().await;

    observe_guard(
        pool.clone(),
        &server,
        &event("viewer", "999999999", "https://discord.gg/ZWSNyNfdG", None),
    )
    .await;

    wait_for_ledger(&pool, 1).await;
    wait_for_alerts(&server, 1).await;
    assert_eq!(ledger_verdicts(&pool).await, ["hard_hit"]);
}

#[tokio::test]
async fn observe_startet_keinen_stil_radar_fuer_etablierten_chatter() {
    let pool = pool_or_skip!("tb_crew_observe_returning_clean");
    prepare_observe_schema(&pool, false).await;
    let server = MockServer::start().await;

    observe_guard(
        pool.clone(),
        &server,
        &event("viewer", "999999999", "harmloser etablierter chat", None),
    )
    .await;
    sleep(Duration::from_millis(250)).await;

    assert!(ledger_verdicts(&pool).await.is_empty());
    assert!(server
        .received_requests()
        .await
        .expect("Discord-Requests lesen")
        .is_empty());
}

#[tokio::test]
async fn observe_schreibt_radar_ledger_fuer_unprivilegierten_erstschreiber() {
    let pool = pool_or_skip!("tb_crew_observe_first_time");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;

    observe_guard(
        pool.clone(),
        &server,
        &event("viewer", "999999999", "harmloser erster chat", None),
    )
    .await;

    wait_for_ledger(&pool, 1).await;
    assert_eq!(ledger_verdicts(&pool).await, ["skipped"]);
}

#[tokio::test]
async fn observe_sendet_pro_vorfall_genau_eine_discord_meldung() {
    let pool = pool_or_skip!("tb_crew_observe_single_alert");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;

    observe_guard(
        pool.clone(),
        &server,
        &event("helmbombenricky", "147713656", "hallo", None),
    )
    .await;

    wait_for_alerts(&server, 1).await;
    sleep(Duration::from_millis(100)).await;
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("Discord-Requests lesen")
            .len(),
        1
    );
    assert_eq!(ledger_verdicts(&pool).await, ["hard_hit"]);
}

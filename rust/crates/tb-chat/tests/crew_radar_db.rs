use std::str::FromStr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::crew_guard::{persist_radar_log, CrewJudge, CrewRadarLog, CrewVerdict};
use tb_chat::scam_pitch::AccountAgePort;
use tb_chat::style_score::{build_centroid, score, StyleBreakdown};
use tb_chat::types::{ChatBadge, ChatMessageBody};
use tb_chat::{ChatMessageEvent, CrewGuard, ModAlerter};
use tokio::sync::Semaphore;
use tokio::time::{sleep, timeout, Duration};
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

    type CrewRadarLogRow = (
        String,
        String,
        Option<String>,
        i64,
        i16,
        serde_json::Value,
        bool,
        serde_json::Value,
        String,
        Option<f32>,
        Option<String>,
        String,
        String,
    );
    let row: CrewRadarLogRow =
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

    for verdict in [
        "error",
        "timeout",
        "unsure",
        "skipped",
        "campaign",
        "hard_id",
        "hard_invite",
    ] {
        let record = CrewRadarLog {
            channel_login: "kanal".to_string(),
            chatter_login: verdict.to_string(),
            chatter_id: None,
            account_age_days: None,
            style_score: 0,
            style_breakdown: StyleBreakdown {
                pitch: 0,
                campaign: 0,
                typo: 0,
                bro: 0,
                lowercase: 0,
                opener: 0,
                cosine: 0,
            },
            time_window_match: false,
            messages: Vec::new(),
            llm_verdict: verdict.to_string(),
            llm_confidence: None,
            llm_reasoning: None,
            action_taken: "none".to_string(),
            source: "network".to_string(),
        };
        persist_radar_log(&pool, &record)
            .await
            .expect("Ledger schreiben");
    }

    let verdicts: Vec<String> =
        sqlx::query_scalar("SELECT llm_verdict FROM twitch_crew_radar_log ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("Verdicts lesen");
    assert_eq!(
        verdicts,
        [
            "clean",
            "error",
            "timeout",
            "unsure",
            "skipped",
            "campaign",
            "hard_id",
            "hard_invite",
        ]
    );
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

struct RecordingJudge {
    contexts: Arc<Mutex<Vec<Vec<String>>>>,
    called: Arc<Semaphore>,
}

#[async_trait]
impl CrewJudge for RecordingJudge {
    async fn judge(&self, _content: &str, recent_context: &[String]) -> CrewVerdict {
        self.contexts
            .lock()
            .expect("Judge-Kontexte sperren")
            .push(recent_context.to_vec());
        self.called.add_permits(1);
        CrewVerdict::unsure()
    }
}

struct StubAccountAge;

#[async_trait]
impl AccountAgePort for StubAccountAge {
    async fn user_created_at_days(&self, _user_id: &str, _login: &str) -> Option<i64> {
        None
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
    let guard = crew_guard(pool, server);
    guard.observe(event);
}

fn crew_guard(pool: PgPool, server: &MockServer) -> CrewGuard {
    CrewGuard::new(
        true,
        Arc::new(StubJudge),
        Arc::new(ModAlerter::with_endpoint(
            reqwest::Client::new(),
            format!("{}/changelog", server.uri()),
        )),
        pool,
        "bot-id".to_string(),
        Arc::new(StubAccountAge),
        Arc::new(Default::default()),
        false,
    )
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

async fn observation_counts(pool: &PgPool, server: &MockServer) -> (i64, usize) {
    let ledger = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_crew_radar_log")
        .fetch_one(pool)
        .await
        .expect("Ledger zaehlen");
    let alerts = server
        .received_requests()
        .await
        .expect("Discord-Requests lesen")
        .len();
    (ledger, alerts)
}

#[tokio::test]
async fn observe_meldet_hard_id_auch_bei_etabliertem_chatter() {
    let pool = pool_or_skip!("tb_crew_observe_hard_id_returning");
    prepare_observe_schema(&pool, false).await;
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/changelog"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let guard = crew_guard(pool.clone(), &server);
    for content in ["hallo", "noch da", "dritte nachricht"] {
        guard.observe(&event("helmbombenricky", "147713656", content, None));
    }

    wait_for_ledger(&pool, 3).await;
    wait_for_alerts(&server, 3).await;
    assert_eq!(ledger_verdicts(&pool).await, ["hard_id"; 3]);
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
    assert_eq!(ledger_verdicts(&pool).await, ["hard_id"]);
}

#[tokio::test]
async fn observe_meldet_hard_invite_auch_bei_etabliertem_chatter() {
    let pool = pool_or_skip!("tb_crew_observe_hard_invite_returning");
    prepare_observe_schema(&pool, false).await;
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/changelog"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let guard = crew_guard(pool.clone(), &server);
    for _ in 0..2 {
        guard.observe(&event(
            "viewer",
            "999999999",
            "https://discord.gg/ZWSNyNfdG",
            None,
        ));
    }

    wait_for_ledger(&pool, 2).await;
    wait_for_alerts(&server, 2).await;
    assert_eq!(ledger_verdicts(&pool).await, ["hard_invite"; 2]);
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
async fn observe_meldet_eine_harmlose_nachricht_noch_nicht() {
    let pool = pool_or_skip!("tb_crew_observe_no_substance");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;

    observe_guard(
        pool.clone(),
        &server,
        &event("viewer", "999999999", "harmloser erster chat", None),
    )
    .await;
    sleep(Duration::from_millis(250)).await;

    assert_eq!(observation_counts(&pool, &server).await, (0, 0));
}

#[tokio::test]
async fn observe_meldet_nach_drei_harmlosen_nachrichten_genau_einmal() {
    let pool = pool_or_skip!("tb_crew_observe_three_messages");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/changelog"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let guard = crew_guard(pool.clone(), &server);

    for content in [
        "wie geht es dir heute an diesem abend",
        "welchen held spielst du gerade am liebsten",
        "das war eben wirklich eine gute runde",
    ] {
        guard.observe(&event("viewer", "999999999", content, None));
    }

    wait_for_ledger(&pool, 1).await;
    wait_for_alerts(&server, 1).await;
    sleep(Duration::from_millis(100)).await;
    assert_eq!(observation_counts(&pool, &server).await, (1, 1));
    assert_eq!(ledger_verdicts(&pool).await, ["skipped"]);
}

#[tokio::test]
async fn observe_drosselt_zehn_weitere_harmlose_nachrichten() {
    let pool = pool_or_skip!("tb_crew_observe_once_per_chatter");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/changelog"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let guard = crew_guard(pool.clone(), &server);

    for number in 1..=3 {
        guard.observe(&event(
            "viewer",
            "999999999",
            &format!("das ist harmlose nachricht nummer {number} heute"),
            None,
        ));
    }

    wait_for_ledger(&pool, 1).await;
    wait_for_alerts(&server, 1).await;
    for number in 4..=13 {
        guard.observe(&event(
            "viewer",
            "999999999",
            &format!("das ist harmlose nachricht nummer {number} heute"),
            None,
        ));
    }
    sleep(Duration::from_millis(250)).await;
    assert_eq!(observation_counts(&pool, &server).await, (1, 1));
    assert_eq!(ledger_verdicts(&pool).await, ["skipped"]);
}

#[tokio::test]
async fn observe_drosselung_ueberlebt_neuen_guard() {
    let pool = pool_or_skip!("tb_crew_observe_restart");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/changelog"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let guard = crew_guard(pool.clone(), &server);
    for number in 1..=3 {
        guard.observe(&event(
            "viewer",
            "999999999",
            &format!("harmlose nachricht nummer {number} fuer den test"),
            None,
        ));
    }
    wait_for_ledger(&pool, 1).await;
    wait_for_alerts(&server, 1).await;
    drop(guard);

    let restarted_guard = crew_guard(pool.clone(), &server);
    for number in 1..=3 {
        restarted_guard.observe(&event(
            "viewer",
            "999999999",
            &format!("harmlose nachricht nummer {number} nach neustart"),
            None,
        ));
    }
    sleep(Duration::from_millis(250)).await;

    assert_eq!(observation_counts(&pool, &server).await, (1, 1));
}

#[tokio::test]
async fn observe_gibt_zweitem_aufruf_den_kontext_des_ersten_mit() {
    let pool = pool_or_skip!("tb_crew_observe_context_order");
    prepare_observe_schema(&pool, true).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/changelog"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let contexts = Arc::new(Mutex::new(Vec::new()));
    let judge_called = Arc::new(Semaphore::new(0));
    let guard = CrewGuard::new(
        true,
        Arc::new(RecordingJudge {
            contexts: Arc::clone(&contexts),
            called: Arc::clone(&judge_called),
        }),
        Arc::new(ModAlerter::with_endpoint(
            reqwest::Client::new(),
            format!("{}/changelog", server.uri()),
        )),
        pool.clone(),
        "bot-id".to_string(),
        Arc::new(StubAccountAge),
        Arc::new(Default::default()),
        false,
    );

    let connection_one = pool.acquire().await.expect("erste DB-Verbindung halten");
    let connection_two = pool.acquire().await.expect("zweite DB-Verbindung halten");
    let first = "erste nachricht ohne signal";
    let second =
        "hast du den bot von nani drinne? du bannst unbewusst viele leute wegen der bannliste";

    guard.observe(&event("viewer", "999999999", first, None));
    guard.observe(&event("viewer", "999999999", second, None));

    let _permit = timeout(Duration::from_secs(1), judge_called.acquire())
        .await
        .expect("zweiter Aufruf erreichte Judge nicht")
        .expect("Judge-Semaphore geschlossen");
    assert_eq!(
        *contexts.lock().expect("Judge-Kontexte lesen"),
        vec![vec![first.to_string()]]
    );

    drop((connection_one, connection_two));
    wait_for_ledger(&pool, 1).await;
    let messages: serde_json::Value = sqlx::query_scalar(
        "SELECT messages FROM twitch_crew_radar_log WHERE llm_verdict = 'unsure'",
    )
    .fetch_one(&pool)
    .await
    .expect("Kontext aus Radar-Ledger lesen");
    assert_eq!(messages, serde_json::json!([first, second]));
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
    assert_eq!(ledger_verdicts(&pool).await, ["hard_id"]);
}

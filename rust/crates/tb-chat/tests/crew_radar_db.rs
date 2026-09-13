use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;
use tb_chat::crew_guard::{persist_radar_alert, persist_radar_log, CrewRadarLog};
use tb_chat::scam_pitch::AccountAgePort;
use tb_chat::style_score::{build_centroid, score, StyleBreakdown};
use tb_chat::types::ChatMessageBody;
use tb_chat::zuschauer_register::unauffaellig;
use tb_chat::{ChatMessageEvent, CrewGuard, ModAlerter};
use tokio::time::{sleep, Duration};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};
#[path = "../../../test-support/postgres.rs"]
mod postgres;
use postgres::TestPostgres;

#[tokio::test]
async fn ledger_speichert_auch_clean_entscheidung_vollstaendig() {
    let database = TestPostgres::start().await;
    let pool = database.pool.clone();
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
    let database = TestPostgres::start().await;
    let pool = database.pool.clone();
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

struct StubAccountAge;
#[async_trait]
impl AccountAgePort for StubAccountAge {
    async fn user_created_at_days(&self, _user_id: &str, _login: &str) -> Option<i64> {
        Some(42)
    }
}

async fn schema(pool: &PgPool) {
    sqlx::raw_sql("CREATE TABLE twitch_partners (twitch_user_id TEXT); CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, discord_user_id TEXT, is_on_discord INT); CREATE TABLE twitch_session_chatters (chatter_id TEXT, session_id BIGINT, messages INT, first_message_at TIMESTAMPTZ);
        CREATE TABLE twitch_spam_review_decisions (chatter_id TEXT, verdict TEXT);
        CREATE TABLE twitch_scam_guard_verdicts (chatter_id TEXT, verdict TEXT, action_taken TEXT);
        CREATE TABLE twitch_chatter_global_ban (chatter_id TEXT);
        CREATE TABLE tb_chat_autoban_log (chatter_id TEXT, action TEXT, source_path TEXT);")
        .execute(pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260906140000_twitch_zuschauer_register.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260714120000_twitch_crew_radar_log.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260913190000_passives_kontoregister.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn history(pool: &PgPool, id: &str) {
    sqlx::query("INSERT INTO twitch_session_chatters VALUES ($1, 1, 20, NOW() - INTERVAL '10 days'), ($1, 2, 10, NOW() - INTERVAL '5 days'), ($1, 3, 13, NOW())")
        .bind(id).execute(pool).await.unwrap();
}

fn guard(pool: PgPool, server: &MockServer) -> CrewGuard {
    CrewGuard::new(
        true,
        Arc::new(ModAlerter::with_endpoint(
            reqwest::Client::new(),
            server.uri(),
        )),
        pool,
        "bot".into(),
        Arc::new(StubAccountAge),
        Arc::new(Default::default()),
        false,
    )
}

fn event(id: &str, channel: &str, content: &str) -> ChatMessageEvent {
    ChatMessageEvent {
        chatter_user_id: id.into(),
        chatter_user_login: "viewer".into(),
        broadcaster_user_login: channel.into(),
        message: ChatMessageBody {
            text: content.into(),
            fragments: vec![],
        },
        ..Default::default()
    }
}

async fn wait_log(pool: &PgPool) {
    for _ in 0..100 {
        if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_crew_radar_log")
            .fetch_one(pool)
            .await
            .unwrap()
            > 0
        {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("Musterprotokoll fehlt");
}

#[tokio::test]
async fn historie_ueberlebt_neustart_und_namenswechsel_ohne_community_score_zu_veraendern() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    sqlx::query("INSERT INTO twitch_zuschauer_register (twitch_user_id, community_probability, signals) VALUES ('42', 0.8, '{\"hard_match\":true}')").execute(&db.pool).await.unwrap();
    assert!(unauffaellig(&db.pool, "42").await.unwrap());
    sqlx::raw_sql("DELETE FROM twitch_session_chatters; UPDATE twitch_zuschauer_register SET twitch_login = 'neuername' WHERE twitch_user_id = '42'").execute(&db.pool).await.unwrap();
    assert!(unauffaellig(&db.pool.clone(), "42").await.unwrap());
    let p: f64 = sqlx::query_scalar(
        "SELECT community_probability FROM twitch_zuschauer_register WHERE twitch_user_id = '42'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(p, 0.8);
}

#[tokio::test]
async fn fluten_und_doppelte_session_reichen_nicht_fuer_vertrauen() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    sqlx::query("INSERT INTO twitch_session_chatters VALUES ('42', 1, 2000, NOW() - INTERVAL '10 days'), ('42', 1, 1000, NOW()), ('42', 1, 1000, NOW()), ('43', 1, 1000, NOW()), ('43', 2, 1000, NOW()), ('43', 3, 1000, NOW()), ('44', 1, 20, NOW() - INTERVAL '8 days'), ('44', 2, 20, NOW() - INTERVAL '8 days' + INTERVAL '1 minute'), ('44', 3, 20, NOW() - INTERVAL '8 days' + INTERVAL '2 minutes')").execute(&db.pool).await.unwrap();
    assert!(!unauffaellig(&db.pool, "42").await.unwrap());
    assert!(!unauffaellig(&db.pool, "43").await.unwrap());
    assert!(!unauffaellig(&db.pool, "44").await.unwrap());
    assert!(!unauffaellig(&db.pool, "").await.unwrap());
}

#[tokio::test]
async fn bestaetigter_spam_scam_globalban_und_regelaktion_widerrufen_vertrauen() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    for (id, statement) in [
        (
            "1",
            "INSERT INTO twitch_spam_review_decisions VALUES ('1','spam')",
        ),
        (
            "2",
            "INSERT INTO twitch_scam_guard_verdicts VALUES ('2','scam','banned')",
        ),
        ("3", "INSERT INTO twitch_chatter_global_ban VALUES ('3')"),
        (
            "4",
            "INSERT INTO tb_chat_autoban_log VALUES ('4','ban','spam')",
        ),
    ] {
        history(&db.pool, id).await;
        assert!(unauffaellig(&db.pool, id).await.unwrap());
        sqlx::query(statement).execute(&db.pool).await.unwrap();
        assert!(!unauffaellig(&db.pool, id).await.unwrap());
        assert!(!unauffaellig(&db.pool.clone(), id).await.unwrap());
    }
    history(&db.pool, "5").await;
    sqlx::query("INSERT INTO twitch_scam_guard_verdicts VALUES ('5','scam','overturned')")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(unauffaellig(&db.pool, "5").await.unwrap());
}

#[tokio::test]
async fn radar_slot_ist_atomar_netzwerkweit_und_auf_zwei_pro_woche_begrenzt() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    let record = radar_record("42");
    let (a, b) = tokio::join!(
        persist_radar_alert(&db.pool, &record),
        persist_radar_alert(&db.pool, &record)
    );
    assert_eq!(
        usize::from(a.unwrap().is_some()) + usize::from(b.unwrap().is_some()),
        1
    );
    sqlx::query("UPDATE twitch_zuschauer_register SET radar_meldung_am = NOW() - INTERVAL '2 days' WHERE twitch_user_id = '42'").execute(&db.pool).await.unwrap();
    assert_eq!(
        persist_radar_alert(&db.pool, &record).await.unwrap(),
        Some(1)
    );
    sqlx::query("UPDATE twitch_zuschauer_register SET radar_meldung_am = NOW() - INTERVAL '1 day' WHERE twitch_user_id = '42'").execute(&db.pool).await.unwrap();
    assert_eq!(persist_radar_alert(&db.pool, &record).await.unwrap(), None);
}

#[tokio::test]
async fn korrigierter_oder_entfernter_letzter_beleg_erlaubt_normale_neupruefung() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    for (index, insert, undo) in [
        (0, "INSERT INTO twitch_scam_guard_verdicts VALUES ($1,'scam','banned')", "UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' WHERE chatter_id = $1"),
        (1, "INSERT INTO twitch_spam_review_decisions VALUES ($1,'spam')", "UPDATE twitch_spam_review_decisions SET verdict = 'clean' WHERE chatter_id = $1"),
        (2, "INSERT INTO twitch_chatter_global_ban VALUES ($1)", "DELETE FROM twitch_chatter_global_ban WHERE chatter_id = $1"),
        (3, "INSERT INTO tb_chat_autoban_log VALUES ($1,'ban','spam')", "UPDATE tb_chat_autoban_log SET action = 'unban' WHERE chatter_id = $1"),
        (4, "INSERT INTO twitch_scam_guard_verdicts VALUES ($1,'scam','banned')", "DELETE FROM twitch_scam_guard_verdicts WHERE chatter_id = $1"),
        (5, "INSERT INTO twitch_spam_review_decisions VALUES ($1,'spam')", "DELETE FROM twitch_spam_review_decisions WHERE chatter_id = $1"),
        (6, "INSERT INTO tb_chat_autoban_log VALUES ($1,'timeout','spam')", "DELETE FROM tb_chat_autoban_log WHERE chatter_id = $1"),
    ] {
        let id = format!("undo-{index}");
        if index == 2 {
            sqlx::query("INSERT INTO twitch_partners VALUES ($1)").bind(&id).execute(&db.pool).await.unwrap();
        } else {
            history(&db.pool, &id).await;
        }
        assert!(unauffaellig(&db.pool, &id).await.unwrap());
        sqlx::query(insert).bind(&id).execute(&db.pool).await.unwrap();
        assert!(!unauffaellig(&db.pool, &id).await.unwrap());
        sqlx::query(undo).bind(&id).execute(&db.pool).await.unwrap();
        let pending: bool = sqlx::query_scalar("SELECT unauffaellig_seit IS NULL AND vertrauen_widerrufen_am IS NULL AND historie_geprueft_am IS NULL FROM twitch_zuschauer_register WHERE twitch_user_id = $1")
            .bind(&id).fetch_one(&db.pool).await.unwrap();
        assert!(pending, "keine direkte Freigabe für {id}");
        assert!(unauffaellig(&db.pool, &id).await.unwrap(), "normale Neuprüfung für {id}");
    }
}

#[tokio::test]
async fn undo_gibt_weder_anderweitig_belastete_noch_unbekannte_konten_frei() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    sqlx::raw_sql(
        "INSERT INTO twitch_scam_guard_verdicts VALUES ('42','scam','banned');
        INSERT INTO twitch_spam_review_decisions VALUES ('42','spam');
        INSERT INTO twitch_chatter_global_ban VALUES ('42');
        INSERT INTO tb_chat_autoban_log VALUES ('42','ban','spam');
        UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' WHERE chatter_id = '42'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(!unauffaellig(&db.pool, "42").await.unwrap());
    for statement in [
        "UPDATE twitch_spam_review_decisions SET verdict = 'clean' WHERE chatter_id = '42'",
        "DELETE FROM twitch_chatter_global_ban WHERE chatter_id = '42'",
    ] {
        sqlx::query(statement).execute(&db.pool).await.unwrap();
        assert!(!unauffaellig(&db.pool, "42").await.unwrap());
    }
    sqlx::query("UPDATE tb_chat_autoban_log SET action = 'unban' WHERE chatter_id = '42'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(unauffaellig(&db.pool, "42").await.unwrap());
    sqlx::raw_sql("INSERT INTO twitch_scam_guard_verdicts VALUES ('unknown','scam','banned');
        UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' WHERE chatter_id = 'unknown'")
        .execute(&db.pool).await.unwrap();
    assert!(!unauffaellig(&db.pool, "unknown").await.unwrap());
}

#[tokio::test]
async fn gleichzeitiges_undo_neuer_beleg_und_historie_lassen_widerruf_bestehen() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    sqlx::query("INSERT INTO twitch_scam_guard_verdicts VALUES ('42','scam','banned')")
        .execute(&db.pool)
        .await
        .unwrap();
    let (undo, adverse, history) = tokio::join!(
        sqlx::query("UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' WHERE chatter_id = '42'").execute(&db.pool),
        sqlx::query("INSERT INTO twitch_spam_review_decisions VALUES ('42','spam')").execute(&db.pool),
        unauffaellig(&db.pool, "42"),
    );
    undo.unwrap();
    adverse.unwrap();
    history.unwrap();
    assert!(!unauffaellig(&db.pool, "42").await.unwrap());
    assert!(sqlx::query_scalar::<_, bool>("SELECT unauffaellig_seit IS NULL AND vertrauen_widerrufen_am IS NOT NULL FROM twitch_zuschauer_register WHERE twitch_user_id = '42'").fetch_one(&db.pool).await.unwrap());
}

#[tokio::test]
async fn scam_timeout_undo_erlaubt_neupruefung_trotz_erhaltenem_audit() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    sqlx::raw_sql(
        "INSERT INTO tb_chat_autoban_log VALUES ('42','timeout','scam');
        INSERT INTO twitch_scam_guard_verdicts VALUES ('42','scam','timed_out')",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(!unauffaellig(&db.pool, "42").await.unwrap());
    sqlx::query(
        "UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' WHERE chatter_id = '42'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(unauffaellig(&db.pool, "42").await.unwrap());
    let audit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tb_chat_autoban_log WHERE chatter_id = '42' AND action = 'timeout' AND source_path = 'scam'")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(audit_count, 1);
    sqlx::query("INSERT INTO twitch_scam_guard_verdicts VALUES ('42','scam','suggested')")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(!unauffaellig(&db.pool, "42").await.unwrap());
}

fn radar_record(id: &str) -> CrewRadarLog {
    CrewRadarLog {
        channel_login: "kanal".into(),
        chatter_login: "viewer".into(),
        chatter_id: Some(id.into()),
        account_age_days: Some(42),
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
        messages: vec!["Muster".into()],
        llm_verdict: "pattern".into(),
        llm_confidence: None,
        llm_reasoning: None,
        action_taken: "none".into(),
        source: "passive_patterns".into(),
    }
}

#[tokio::test]
async fn fehlgeschlagenes_radar_protokoll_verbraucht_keine_meldungsquote() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    sqlx::query("INSERT INTO twitch_zuschauer_register (twitch_user_id, community_probability, computed_at, radar_meldung_am, radar_wiederholungen) VALUES ('42', 0.2, NOW(), NOW() - INTERVAL '2 days', 7)")
        .execute(&db.pool).await.unwrap();
    sqlx::query("ALTER TABLE twitch_crew_radar_log ADD CONSTRAINT test_write_failure CHECK (source <> 'passive_patterns')")
        .execute(&db.pool).await.unwrap();
    let record = radar_record("42");
    assert!(persist_radar_alert(&db.pool, &record).await.is_err());
    let state: (bool, bool, i64) = sqlx::query_as("SELECT radar_meldung_am < NOW() - INTERVAL '1 day', radar_vorherige_meldung_am IS NULL, radar_wiederholungen FROM twitch_zuschauer_register WHERE twitch_user_id = '42'")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(state, (true, true, 7));
    sqlx::query("ALTER TABLE twitch_crew_radar_log DROP CONSTRAINT test_write_failure")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        persist_radar_alert(&db.pool, &record).await.unwrap(),
        Some(7)
    );
    assert_eq!(persist_radar_alert(&db.pool, &record).await.unwrap(), None);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_crew_radar_log")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn zwei_guards_und_kanaele_melden_selben_treffer_nur_einmal_passiv() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let a = guard(db.pool.clone(), &server);
    let b = guard(db.pool.clone(), &server);
    a.observe(&event("42", "eins", "was ist mit der bannliste"));
    b.observe(&event("42", "zwei", "was ist mit der bannliste"));
    wait_log(&db.pool).await;
    sleep(Duration::from_millis(200)).await;
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let payload: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(payload["crew_radar"]["notify_only"], true);
    assert_eq!(payload["crew_radar"]["verdict"], "pattern");
    let row: (Option<f32>, String) =
        sqlx::query_as("SELECT llm_confidence, action_taken FROM twitch_crew_radar_log")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row, (None, "none".into()));
}

#[tokio::test]
async fn normale_unterhaltung_und_unauffaellige_stammgaeste_bleiben_still() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    let server = MockServer::start().await;
    let guard = guard(db.pool.clone(), &server);
    for _ in 0..12 {
        guard.observe(&event("42", "eins", "nani spielt heute wirklich gut"));
        guard.observe(&event("43", "zwei", "das war ein gutes spiel"));
    }
    sleep(Duration::from_millis(400)).await;
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_crew_radar_log")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        0
    );
    assert!(unauffaellig(&db.pool, "42").await.unwrap());
    assert!(!unauffaellig(&db.pool, "43").await.unwrap());
}

#[tokio::test]
async fn harte_muster_bleiben_trotz_sauberer_historie_sichtbar() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let guard = guard(db.pool.clone(), &server);
    guard.observe(&event("42", "eins", "https://discord.gg/ZWSNyNfdG"));
    wait_log(&db.pool).await;
    let verdict: String = sqlx::query_scalar("SELECT llm_verdict FROM twitch_crew_radar_log")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(verdict, "hard_invite");
}

#[tokio::test]
async fn partner_und_harte_discord_verknuepfung_zaehlen_namensaehnlichkeit_nicht() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    sqlx::raw_sql("INSERT INTO twitch_partners VALUES ('1'); INSERT INTO twitch_streamer_identities VALUES ('2','discord-id',1); INSERT INTO twitch_zuschauer_register (twitch_user_id,community_probability,discord_user_id) VALUES ('3',0.99,'guessed-name');").execute(&db.pool).await.unwrap();
    assert!(unauffaellig(&db.pool, "1").await.unwrap());
    assert!(unauffaellig(&db.pool, "2").await.unwrap());
    assert!(!unauffaellig(&db.pool, "3").await.unwrap());
    sqlx::query("INSERT INTO twitch_chatter_global_ban VALUES ('1')")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(!unauffaellig(&db.pool, "1").await.unwrap());
}

#[tokio::test]
async fn spam_judge_ueberspringt_bekannte_id_auch_nach_neustart_ohne_ki_aufruf() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    let chatter = event("42", "eins", "need more viewers?");
    let reviewer = tb_chat::scam_pitch::SpamAiReviewer::new(db.pool.clone());
    assert!(matches!(
        reviewer.review_for_verdict(&chatter).await,
        tb_chat::scam_pitch::AiReviewOutcome::Skipped
    ));
    let reviewer = tb_chat::scam_pitch::SpamAiReviewer::new(db.pool.clone());
    let mut renamed = chatter;
    renamed.chatter_user_login = "renamed".into();
    renamed.broadcaster_user_login = "zwei".into();
    assert!(matches!(
        reviewer.review_for_verdict(&renamed).await,
        tb_chat::scam_pitch::AiReviewOutcome::Skipped
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_spam_review_decisions")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn kontoregister_migration_passt_zur_frischen_schema_kette() {
    let db = TestPostgres::start_with_timescaledb().await;
    sqlx::query("CREATE EXTENSION timescaledb")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations")
        .run(&db.pool)
        .await
        .unwrap();
    let actual = sqlx::query_as::<_,(String,String,String,String,String)>(
        "SELECT table_name, column_name, data_type, is_nullable, COALESCE(column_default,'') \
         FROM information_schema.columns WHERE table_schema='public' AND table_name = 'twitch_zuschauer_register' \
         ORDER BY table_name, column_name")
        .fetch_all(&db.pool).await.unwrap().into_iter()
        .map(|(t,c,d,n,v)| format!("{t}|{c}|{d}|{n}|{v}"))
        .collect::<std::collections::BTreeSet<_>>();
    let expected = include_str!("../../tb-db/tests/fresh_schema_snapshot.txt")
        .lines()
        .filter(|line| line.starts_with("twitch_zuschauer_register|"))
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        actual.difference(&expected).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "Neue Schemaspalten"
    );
    assert_eq!(
        expected.difference(&actual).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "Fehlende Schemaspalten"
    );
}

#[tokio::test]
async fn gleichzeitiger_widerruf_und_historienpruefung_lassen_kein_vertrauen_zurueck() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    history(&db.pool, "42").await;
    let (qualified, revoked) = tokio::join!(
        unauffaellig(&db.pool, "42"),
        sqlx::query("INSERT INTO twitch_spam_review_decisions VALUES ('42', 'spam')")
            .execute(&db.pool)
    );
    qualified.unwrap();
    revoked.unwrap();
    assert!(!unauffaellig(&db.pool, "42").await.unwrap());
    assert!(sqlx::query_scalar::<_, bool>("SELECT unauffaellig_seit IS NULL AND vertrauen_widerrufen_am IS NOT NULL FROM twitch_zuschauer_register WHERE twitch_user_id = '42'").fetch_one(&db.pool).await.unwrap());
}

#[tokio::test]
async fn explizite_safe_id_zaehlt_und_widerruf_gewinnt_auch_dort() {
    let db = TestPostgres::start().await;
    schema(&db.pool).await;
    let id = tb_chat::safe_list::SAFE_ACCOUNTS[0].twitch_user_id;
    assert!(unauffaellig(&db.pool, id).await.unwrap());
    sqlx::query("INSERT INTO twitch_chatter_global_ban VALUES ($1)")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(!unauffaellig(&db.pool, id).await.unwrap());
}

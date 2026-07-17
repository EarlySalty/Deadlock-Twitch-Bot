//! Hermetische DB-Tests für ModerationEngine — tb_chat_autoban_log.
//!
//! Testet den DB-Schreibpfad von [`ModerationEngine::auto_ban_and_cleanup`].
//! Schema-isoliert; tb_chat_autoban_log wird prod-treu wie beim Bot-Start angelegt.

use std::io::{self, Write};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::api::{BanOutcome, ChatApi};
use tb_chat::moderation::{
    AutoBanRequest, ModerationEngine, ModerationEvidence, BAN_REASON_GLOBAL, BAN_REASON_SPAM,
    NOTICE_GLOBAL_BAN,
};
use tb_chat::types::SendOutcome;

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
        .unwrap();
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    apply_ddl(&pool).await;
    pool
}

async fn apply_ddl(pool: &PgPool) {
    // tb_chat_autoban_log: canonical schema lives in migration 20260630141000.
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS tb_chat_autoban_log (
            id BIGSERIAL PRIMARY KEY,
            channel_login TEXT,
            chatter_id TEXT NOT NULL,
            chatter_login TEXT,
            content TEXT,
            banned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            action TEXT,
            source_path TEXT,
            reason TEXT,
            score REAL,
            account_age_days BIGINT
        )"#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS twitch_chat_messages (
            id BIGSERIAL PRIMARY KEY,
            message_id TEXT,
            content TEXT,
            moderation_action TEXT,
            moderation_reason TEXT
        )"#,
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_ban_events: Prod-Tabelle, die die öffentliche recent-bans-Statistik speist.
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS twitch_ban_events (
            id BIGSERIAL PRIMARY KEY,
            session_id BIGINT,
            twitch_user_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            target_login TEXT,
            target_id TEXT,
            moderator_login TEXT,
            reason TEXT,
            ends_at TEXT,
            received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
    )
    .execute(pool)
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// Mock-ChatApi
// ---------------------------------------------------------------------------

#[derive(Default)]
struct OkApi {
    timeout_calls: AtomicUsize,
}

#[async_trait]
impl ChatApi for OkApi {
    async fn send_message(&self, _b: &str, _m: &str) -> Result<SendOutcome, String> {
        Ok(SendOutcome::Sent)
    }
    async fn send_announcement(&self, _b: &str, _m: &str, _c: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn ban_user(&self, _b: &str, _u: &str, _r: &str) -> Result<BanOutcome, String> {
        Ok(BanOutcome::Banned)
    }
    async fn timeout_user(
        &self,
        _b: &str,
        _u: &str,
        _d: u32,
        _r: &str,
    ) -> Result<BanOutcome, String> {
        self.timeout_calls.fetch_add(1, Ordering::SeqCst);
        Ok(BanOutcome::Banned)
    }
    async fn unban_user(&self, _b: &str, _u: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn delete_message(&self, _b: &str, _m: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn user_created_at(
        &self,
        _u: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        Ok(None)
    }
    async fn resolve_user_id(&self, _l: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn bot_user_id(&self) -> String {
        "ok-bot-id".to_string()
    }
}

#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter(self.0.clone())
    }
}

impl LogCapture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn assert_login_is_not_id(field: &str, login: Option<&str>) {
    let login = login.unwrap_or_else(|| panic!("{field} fehlt"));
    assert!(
        !login.chars().all(|character| character.is_ascii_digit()),
        "{field} enthaelt eine numerische ID statt eines Logins: {login}"
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn autoban_schreibt_in_db() {
    let pool = pool_or_skip!("autoban_write");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool.clone());

    sqlx::query("INSERT INTO twitch_chat_messages (message_id, content) VALUES ('msg-id-1', 'Spam-Inhalt hier')")
        .execute(&pool)
        .await
        .unwrap();

    let ok = engine
        .auto_ban_and_cleanup_with_evidence(
            AutoBanRequest {
                channel_login: "testkanal",
                broadcaster_id: "broadcaster-id",
                bot_id: "bot-id",
                chatter_login: "spammer42",
                chatter_id: "user-42",
                message_id: "msg-id-1",
                content: "Spam-Inhalt hier",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: true,
            },
            ModerationEvidence {
                source_path: "spam",
                reason: "Phrase(Exact): Best viewers streamboo.com",
                score: Some(4.0),
                account_age_days: Some(3),
            },
        )
        .await;
    assert!(ok, "AutoBan soll true zurückgeben");

    // DB-Eintrag prüfen
    let (channel, chatter_login, content, action, source_path, reason): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT channel_login, chatter_login, content, action, source_path, reason \
         FROM tb_chat_autoban_log LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(channel, "testkanal");
    assert_eq!(chatter_login, "spammer42");
    assert_eq!(content.as_deref(), Some("Spam-Inhalt hier"));
    assert_eq!(action.as_deref(), Some("ban"));
    assert_eq!(source_path.as_deref(), Some("spam"));
    assert!(reason.as_deref().unwrap().contains("streamboo.com"));

    let (message_action, message_reason): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT moderation_action, moderation_reason FROM twitch_chat_messages WHERE message_id = 'msg-id-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(message_action.as_deref(), Some("ban"));
    assert!(message_reason.as_deref().unwrap().contains("streamboo.com"));

    // Auto-Ban muss zusätzlich die öffentliche recent-bans-Statistik speisen:
    // ein 'ban'-Event in twitch_ban_events mit Spam-Inhalt als Grund.
    let (uid, ev_type, target, reason): (String, String, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT twitch_user_id, event_type, target_login, reason \
             FROM twitch_ban_events LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(uid, "broadcaster-id");
    assert_eq!(ev_type, "ban");
    assert_eq!(target.as_deref(), Some("spammer42"));
    assert_eq!(reason.as_deref(), Some("Spam-Inhalt hier"));
}

#[tokio::test]
async fn autoban_last_record_in_memory_gesetzt() {
    let pool = pool_or_skip!("autoban_mem");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool);

    engine
        .auto_ban_and_cleanup(AutoBanRequest {
            channel_login: "memkanal",
            broadcaster_id: "bid",
            bot_id: "bot",
            chatter_login: "user_x",
            chatter_id: "u_x",
            message_id: "m_x",
            content: "Inhalt X",
            ban: true,
            reason_text: BAN_REASON_GLOBAL,
            notice_text: Some(NOTICE_GLOBAL_BAN),
            silent: true,
        })
        .await;

    let rec = engine.last_autoban("memkanal").await;
    assert!(rec.is_some());
    let r = rec.unwrap();
    assert_eq!(r.user_id, "u_x");
    assert_eq!(r.login, "user_x");
    assert_eq!(r.content, "Inhalt X");
    assert!(r.ts <= Utc::now());
}

#[tokio::test]
async fn delete_only_schreibt_auch_in_db() {
    let pool = pool_or_skip!("autoban_delete_only");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool.clone());

    sqlx::query(
        "INSERT INTO twitch_chat_messages (message_id, content) VALUES ('del_msg', 'Del-Inhalt')",
    )
    .execute(&pool)
    .await
    .unwrap();

    engine
        .auto_ban_and_cleanup_with_evidence(
            AutoBanRequest {
                channel_login: "delkanal",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "del_user",
                chatter_id: "del_u1",
                message_id: "del_msg",
                content: "Del-Inhalt",
                ban: false,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            },
            ModerationEvidence {
                source_path: "spam",
                reason: "Fragment: streamboo",
                score: Some(1.0),
                account_age_days: None,
            },
        )
        .await;

    let (action, message_action): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT l.action, m.moderation_action \
         FROM tb_chat_autoban_log l \
         JOIN twitch_chat_messages m ON m.message_id = 'del_msg' \
         WHERE l.channel_login = 'delkanal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(action.as_deref(), Some("delete_only"));
    assert_eq!(message_action.as_deref(), Some("delete_only"));
}

#[tokio::test]
async fn safe_list_unterdrueckung_wird_sichtbar() {
    let pool = pool_or_skip!("autoban_safe_suppressed");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool.clone());
    sqlx::query("INSERT INTO twitch_chat_messages (message_id, content) VALUES ('safe_msg', 'aiviewers bei streamboo')")
        .execute(&pool)
        .await
        .unwrap();

    let enforced = engine
        .auto_ban_and_cleanup_with_evidence(
            AutoBanRequest {
                channel_login: "safechannel",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "kubi_kubi_kubi",
                chatter_id: "19123804",
                message_id: "safe_msg",
                content: "aiviewers bei streamboo",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            },
            ModerationEvidence {
                source_path: "spam",
                reason: "Fragment: streamboo",
                score: Some(3.0),
                account_age_days: None,
            },
        )
        .await;

    assert!(!enforced);
    let (log_action, message_action): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT l.action, m.moderation_action \
         FROM tb_chat_autoban_log l \
         JOIN twitch_chat_messages m ON m.message_id = 'safe_msg' \
         WHERE l.channel_login = 'safechannel'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(log_action.as_deref(), Some("suppressed_safe_list"));
    assert_eq!(message_action.as_deref(), Some("suppressed_safe_list"));
}

#[tokio::test]
async fn normale_nachricht_bleibt_ohne_moderationsaktion() {
    let pool = pool_or_skip!("autoban_normal_message");
    sqlx::query("INSERT INTO twitch_chat_messages (message_id, content) VALUES ('normal_msg', 'Hallo zusammen')")
        .execute(&pool)
        .await
        .unwrap();

    let action: Option<String> = sqlx::query_scalar(
        "SELECT moderation_action FROM twitch_chat_messages WHERE message_id = 'normal_msg'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(action.is_none());
}

#[tokio::test]
async fn timeout_schreibt_aktion_und_judge_confidence() {
    let pool = pool_or_skip!("autoban_timeout");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool.clone());
    sqlx::query("INSERT INTO twitch_chat_messages (message_id, content) VALUES ('timeout_msg', 'verdächtig')")
        .execute(&pool)
        .await
        .unwrap();

    let enforced = engine
        .timeout_and_cleanup_with_evidence(
            "timeoutkanal",
            "bid",
            "timeout_user",
            "timeout_uid",
            "timeout_msg",
            "verdächtig",
            86_400,
            BAN_REASON_SPAM,
            ModerationEvidence {
                source_path: "spam",
                reason: "Judge: Werbung; Confidence: 0.92",
                score: Some(0.92),
                account_age_days: Some(2),
            },
        )
        .await;

    assert!(enforced);
    let (log_action, source, reason, message_action): (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT l.action, l.source_path, l.reason, m.moderation_action \
         FROM tb_chat_autoban_log l JOIN twitch_chat_messages m ON m.message_id = 'timeout_msg'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(log_action.as_deref(), Some("timeout"));
    assert_eq!(source.as_deref(), Some("spam"));
    assert!(reason.as_deref().unwrap().contains("0.92"));
    assert_eq!(message_action.as_deref(), Some("timeout"));
}

#[tokio::test]
async fn conversation_scam_timeout_schreibt_echte_evidence_und_message_action() {
    let pool = pool_or_skip!("conversation_scam_timeout_evidence");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool.clone());
    sqlx::query(
        "INSERT INTO twitch_chat_messages (message_id, content) \
         VALUES ('scam_wrapper_msg', 'echter Scam-Inhalt')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let enforced = engine
        .timeout_and_cleanup(
            Some("echter_kanal"),
            "123456789",
            Some("echter_chatter"),
            "987654321",
            "scam_wrapper_msg",
            Some("echter Scam-Inhalt"),
            86_400,
            "Judge: Scam erkannt",
        )
        .await;

    assert!(enforced);
    type ModerationLogRow = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let (channel, chatter, content, action, reason): ModerationLogRow = sqlx::query_as(
        "SELECT channel_login, chatter_login, content, action, reason \
         FROM tb_chat_autoban_log WHERE chatter_id = '987654321'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(channel.as_deref(), Some("echter_kanal"));
    assert_eq!(chatter.as_deref(), Some("echter_chatter"));
    assert_eq!(content.as_deref(), Some("echter Scam-Inhalt"));
    assert_eq!(action.as_deref(), Some("timeout"));
    assert_eq!(reason.as_deref(), Some("Judge: Scam erkannt"));

    let (message_action, message_reason): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT moderation_action, moderation_reason FROM twitch_chat_messages \
         WHERE message_id = 'scam_wrapper_msg'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(message_action.as_deref(), Some("timeout"));
    assert_eq!(message_reason.as_deref(), Some("Judge: Scam erkannt"));
}

#[tokio::test(flavor = "current_thread")]
async fn timeout_ohne_content_schreibt_null_warnt_und_wird_ausgefuehrt() {
    let pool = pool_or_skip!("conversation_scam_timeout_missing_content");
    let api = Arc::new(OkApi::default());
    let engine = ModerationEngine::new(api.clone(), pool.clone());
    let logs = LogCapture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .without_time()
        .with_writer(logs.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let enforced = engine
        .timeout_and_cleanup(
            Some("contentloser_kanal"),
            "123456789",
            Some("contentloser_chatter"),
            "987654321",
            "contentlos_msg",
            None,
            600,
            "Judge: Scam ohne archivierten Text",
        )
        .await;

    assert!(
        enforced,
        "fehlende Evidence darf den Timeout nicht blockieren"
    );
    assert_eq!(api.timeout_calls.load(Ordering::SeqCst), 1);
    let content: Option<String> = sqlx::query_scalar(
        "SELECT content FROM tb_chat_autoban_log WHERE chatter_id = '987654321'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(content.is_none(), "unbekannter Inhalt muss NULL bleiben");
    let logs = logs.text();
    assert!(
        logs.contains("content") && logs.contains("fehlt"),
        "fehlendes Evidence-Feld wurde nicht als WARN geloggt: {logs}"
    );
}

#[tokio::test]
async fn timeout_evidence_login_felder_sind_keine_numerischen_ids() {
    let pool = pool_or_skip!("conversation_scam_timeout_login_shape");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool.clone());

    assert!(
        engine
            .timeout_and_cleanup(
                Some("login_shape_kanal"),
                "123456789",
                Some("login_shape_chatter"),
                "987654321",
                "login_shape_msg",
                Some("Scam-Inhalt"),
                600,
                "Judge: Scam erkannt",
            )
            .await
    );

    let (channel, chatter): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT channel_login, chatter_login FROM tb_chat_autoban_log \
         WHERE chatter_id = '987654321'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_login_is_not_id("channel_login", channel.as_deref());
    assert_login_is_not_id("chatter_login", chatter.as_deref());
}

#[tokio::test]
async fn content_wird_auf_500_zeichen_begrenzt() {
    let pool = pool_or_skip!("autoban_trunc");
    let engine = ModerationEngine::new(Arc::new(OkApi::default()), pool.clone());

    let long_content = "x".repeat(1000);
    engine
        .auto_ban_and_cleanup(AutoBanRequest {
            channel_login: "trunckanal",
            broadcaster_id: "bid",
            bot_id: "bot",
            chatter_login: "trunc_user",
            chatter_id: "t_u",
            message_id: "t_m",
            content: &long_content,
            ban: true,
            reason_text: BAN_REASON_SPAM,
            notice_text: None,
            silent: true,
        })
        .await;

    let content: Option<String> = sqlx::query_scalar(
        "SELECT content FROM tb_chat_autoban_log WHERE channel_login = 'trunckanal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        content.unwrap().len(),
        500,
        "content auf 500 Zeichen begrenzt (moderation.py Z. 576)"
    );
}

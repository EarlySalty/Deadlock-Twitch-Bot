use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::api::{BanOutcome, ChatApi};
use tb_chat::conversation_scam::add_conversation_scam_global_ban;
use tb_chat::conversation_scam::enforce_verdict;
use tb_chat::conversation_scam::{
    fetch_learning_corpus, load_learnings, persist_learnings, revoke_verdict,
    ConversationScamGuard, DialogState, ScamGuardCommands, ScamJudge, Verdict, VerdictKind,
};
use tb_chat::moderation::ModerationEngine;
use tb_chat::types::{ChatMessageBody, ChatMessageEvent, SendOutcome};
use tb_engagement::llm_chat::EngagementLlmClient;

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

async fn drop_schema(pool: &PgPool, schema: &str) {
    pool.close().await;
    let dsn = std::env::var("TB_TEST_DATABASE_URL").unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

async fn apply_ddl(pool: &PgPool) {
    for ddl in [
        "CREATE TABLE twitch_scam_guard_settings (\
            channel_login TEXT PRIMARY KEY, channel_user_id TEXT, \
            enabled BOOLEAN NOT NULL DEFAULT TRUE, \
            mode TEXT NOT NULL DEFAULT 'auto_ban', threshold REAL NOT NULL DEFAULT 0.90, \
            suggestion_floor REAL NOT NULL DEFAULT 0.70)",
        "CREATE TABLE twitch_scam_guard_verdicts (\
            id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, chatter_login TEXT NOT NULL, \
            chatter_id TEXT, verdict TEXT NOT NULL, confidence REAL NOT NULL, category TEXT NOT NULL, \
            reasoning TEXT NOT NULL, transcript_snapshot TEXT NOT NULL, action_taken TEXT NOT NULL, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        "CREATE TABLE twitch_stream_sessions (\
            id BIGINT PRIMARY KEY, streamer_login TEXT NOT NULL, started_at TIMESTAMPTZ NOT NULL, \
            ended_at TIMESTAMPTZ)",
        "CREATE TABLE twitch_session_chatters (\
            session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL, \
            is_first_time_streamer BOOLEAN)",
        "CREATE TABLE twitch_chatter_rollup (\
            streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL)",
        "CREATE TABLE twitch_chat_messages (\
            id BIGINT PRIMARY KEY, session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL, \
            chatter_login TEXT, message_ts TIMESTAMPTZ NOT NULL, is_command BOOLEAN DEFAULT FALSE, \
            content TEXT)",
        "CREATE TABLE twitch_scam_guard_learnings (\
            id BOOLEAN PRIMARY KEY DEFAULT TRUE, guidance TEXT NOT NULL, \
            source_count INTEGER NOT NULL DEFAULT 0, \
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
            CONSTRAINT learnings_singleton CHECK (id))",
        "CREATE TABLE twitch_chatter_global_ban (\
            chatter_login TEXT PRIMARY KEY, chatter_id TEXT, reason TEXT, added_by TEXT, \
            added_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        "CREATE TABLE twitch_chatter_global_ban_applied (\
            id BIGSERIAL PRIMARY KEY, chatter_login TEXT NOT NULL, \
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
    ] {
        sqlx::query(ddl).execute(pool).await.unwrap();
    }
}

struct FixedJudge;

#[async_trait]
impl ScamJudge for FixedJudge {
    async fn judge(&self, _dialog: &mut DialogState) -> Verdict {
        Verdict {
            verdict: VerdictKind::Scam,
            confidence: 0.95,
            category: "growth-pitch".to_string(),
            reasoning: "clear Discord growth pitch".to_string(),
        }
    }
}

struct ZeroDayRiskJudge;

#[async_trait]
impl ScamJudge for ZeroDayRiskJudge {
    async fn judge(&self, _dialog: &mut DialogState) -> Verdict {
        Verdict {
            verdict: VerdictKind::Scam,
            confidence: 0.80,
            category: "befriending_pivot".to_string(),
            reasoning: "generic English befriending followed by a Discord pivot".to_string(),
        }
    }
}

struct CrossChannelRecordingJudge {
    inputs: Arc<std::sync::Mutex<Vec<(String, i64)>>>,
}

#[async_trait]
impl ScamJudge for CrossChannelRecordingJudge {
    async fn judge(&self, dialog: &mut DialogState) -> Verdict {
        let input: serde_json::Value = serde_json::from_str(&dialog.messages()[1].content).unwrap();
        self.inputs.lock().unwrap().push((
            input["message"].as_str().unwrap().to_string(),
            input["other_channels_last_hour"].as_i64().unwrap(),
        ));
        FixedJudge.judge(dialog).await
    }
}

struct NoopApi;

#[async_trait]
impl ChatApi for NoopApi {
    async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
        Ok(SendOutcome::Sent)
    }
    async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
        Ok(BanOutcome::Banned)
    }
    async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<BanOutcome, String> {
        Ok(BanOutcome::Banned)
    }
    async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn user_created_at(
        &self,
        _: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        Ok(None)
    }
    async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn bot_user_id(&self) -> String {
        "bot-id".to_string()
    }
}

/// ChatApi-Stub für den Revoke-Pfad: löst Logins zu `<login>-id` auf und
/// protokolliert jeden Unban-Aufruf (broadcaster_id, target_user_id).
struct RecordingApi {
    unbans: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl ChatApi for RecordingApi {
    async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
        Ok(SendOutcome::Sent)
    }
    async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
        Ok(BanOutcome::Banned)
    }
    async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<BanOutcome, String> {
        Ok(BanOutcome::Banned)
    }
    async fn unban_user(&self, broadcaster_id: &str, target_user_id: &str) -> Result<bool, String> {
        self.unbans
            .lock()
            .unwrap()
            .push((broadcaster_id.to_string(), target_user_id.to_string()));
        Ok(true)
    }
    async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn user_created_at(
        &self,
        _: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        Ok(None)
    }
    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
        Ok(Some(format!("{login}-id")))
    }
    async fn bot_user_id(&self) -> String {
        "bot-id".to_string()
    }
}

struct EnforceRecordingApi {
    bans: Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
}

#[async_trait]
impl ChatApi for EnforceRecordingApi {
    async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
        Ok(SendOutcome::Sent)
    }
    async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn ban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        self.bans.lock().unwrap().push((
            broadcaster_id.to_string(),
            target_user_id.to_string(),
            reason.to_string(),
        ));
        Ok(BanOutcome::Banned)
    }
    async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<BanOutcome, String> {
        Ok(BanOutcome::Banned)
    }
    async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn user_created_at(
        &self,
        _: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        Ok(None)
    }
    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
        Ok(Some(format!("{login}-id")))
    }
    async fn bot_user_id(&self) -> String {
        "bot-id".to_string()
    }
}

#[derive(Default)]
struct ActionCalls {
    bans: Vec<(String, String, String)>,
    timeouts: Vec<(String, String, u32, String)>,
    deletes: Vec<(String, String)>,
}

struct ActionRecordingApi {
    created_at: Option<DateTime<Utc>>,
    calls: Arc<std::sync::Mutex<ActionCalls>>,
}

#[async_trait]
impl ChatApi for ActionRecordingApi {
    async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
        Ok(SendOutcome::Sent)
    }
    async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn ban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        self.calls.lock().unwrap().bans.push((
            broadcaster_id.to_string(),
            target_user_id.to_string(),
            reason.to_string(),
        ));
        Ok(BanOutcome::Banned)
    }
    async fn timeout_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        duration_secs: u32,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        self.calls.lock().unwrap().timeouts.push((
            broadcaster_id.to_string(),
            target_user_id.to_string(),
            duration_secs,
            reason.to_string(),
        ));
        Ok(BanOutcome::Banned)
    }
    async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    async fn delete_message(&self, broadcaster_id: &str, message_id: &str) -> Result<bool, String> {
        self.calls
            .lock()
            .unwrap()
            .deletes
            .push((broadcaster_id.to_string(), message_id.to_string()));
        Ok(true)
    }
    async fn user_created_at(
        &self,
        _: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        Ok(self.created_at)
    }
    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
        Ok(Some(format!("{login}-id")))
    }
    async fn bot_user_id(&self) -> String {
        "bot-id".to_string()
    }
}

async fn seed_first_time_guard(pool: &PgPool, mode: &str) {
    sqlx::query(
        "INSERT INTO twitch_scam_guard_settings (channel_login, mode) \
         VALUES ('testchannel', $1)",
    )
    .bind(mode)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) \
         VALUES (1, 'testchannel', NOW())",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_session_chatters \
         (session_id, streamer_login, chatter_login, is_first_time_streamer) \
         VALUES (1, 'testchannel', 'sam_09995', TRUE)",
    )
    .execute(pool)
    .await
    .unwrap();
}

fn scam_event() -> ChatMessageEvent {
    ChatMessageEvent {
        broadcaster_user_id: "channel-id".to_string(),
        broadcaster_user_login: "testchannel".to_string(),
        chatter_user_id: "sam-id".to_string(),
        chatter_user_login: "sam_09995".to_string(),
        message_id: "message-id".to_string(),
        message: ChatMessageBody {
            text: "Yo bro, I know a big streamer who can help you grow with real viewers. Add him on Discord.".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn wait_action_taken(pool: &PgPool) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(action) = sqlx::query_scalar::<_, String>(
                "SELECT action_taken FROM twitch_scam_guard_verdicts LIMIT 1",
            )
            .fetch_optional(pool)
            .await
            .unwrap()
            {
                break action;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Verdict wurde nicht persistiert")
}

async fn run_action_guard(
    pool: &PgPool,
    created_at: Option<DateTime<Utc>>,
) -> Arc<std::sync::Mutex<ActionCalls>> {
    run_action_guard_with_message(pool, created_at, None).await
}

async fn run_action_guard_with_message(
    pool: &PgPool,
    created_at: Option<DateTime<Utc>>,
    message: Option<&str>,
) -> Arc<std::sync::Mutex<ActionCalls>> {
    run_action_guard_with_judge(pool, created_at, message, Arc::new(FixedJudge)).await
}

async fn run_action_guard_with_judge(
    pool: &PgPool,
    created_at: Option<DateTime<Utc>>,
    message: Option<&str>,
    judge: Arc<dyn ScamJudge>,
) -> Arc<std::sync::Mutex<ActionCalls>> {
    let calls = Arc::new(std::sync::Mutex::new(ActionCalls::default()));
    let api: Arc<dyn ChatApi> = Arc::new(ActionRecordingApi {
        created_at,
        calls: calls.clone(),
    });
    let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api), pool.clone()));
    let guard = Arc::new(ConversationScamGuard::new(
        pool.clone(),
        "bot-id".to_string(),
        judge,
        api,
        moderation,
    ));
    let mut event = scam_event();
    if let Some(message) = message {
        event.message.text = message.to_string();
    }
    guard.observe(&event);
    calls
}

/// Nach einer Umbenennung trägt die Settings-Zeile noch den alten Namen. Ohne
/// die Kanal-ID fände der Guard sie nicht und würde stillschweigend nichts tun
/// — ein abgeschalteter Scam-Schutz sieht von außen aus wie ein Kanal ohne
/// Vorfälle. Die ID kommt als `broadcaster_user_id` an jedem Event mit.
#[tokio::test]
async fn guard_findet_settings_nach_umbenennung_ueber_die_kanal_id() {
    let pool = pool_or_skip!("tb_conversation_scam_rename");
    // Settings unter dem alten Login, aber mit der stabilen ID.
    sqlx::query(
        "INSERT INTO twitch_scam_guard_settings (channel_login, channel_user_id, mode) \
         VALUES ('derechtecoolys', 'channel-id', 'alert_only')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Session und Chatter laufen bereits unter dem neuen Namen.
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) \
         VALUES (1, 'coolysdl', NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_session_chatters \
         (session_id, streamer_login, chatter_login, is_first_time_streamer) \
         VALUES (1, 'coolysdl', 'sam_09995', TRUE)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let api: Arc<dyn ChatApi> = Arc::new(NoopApi);
    let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api), pool.clone()));
    let guard = Arc::new(ConversationScamGuard::new(
        pool.clone(),
        "bot-id".to_string(),
        Arc::new(FixedJudge),
        api,
        moderation,
    ));
    let mut event = scam_event();
    event.broadcaster_user_login = "coolysdl".to_string();
    // Gleiche ID wie in der Settings-Zeile — nur der Name hat sich geändert.
    event.broadcaster_user_id = "channel-id".to_string();

    guard.observe(&event);
    // Geprüft wird `action_taken`, nicht `verdict`: ein Verdict entsteht auch
    // ohne gefundene Settings, weil `load_settings` dann auf
    // `GuardSettings::default()` fällt — und die stehen auf `auto_ban`. Nur
    // `suggested` kann aus der `alert_only`-Zeile stammen, die es allein über
    // `channel_user_id` zu finden gab.
    let action = wait_action_taken(&pool).await;
    assert_eq!(
        action, "suggested",
        "Settings wurden nicht über die Kanal-ID gefunden — der Guard lief auf \
         den auto_ban-Defaults statt auf der alert_only-Zeile des Kanals"
    );
    drop_schema(&pool, "tb_conversation_scam_rename").await;
}

#[tokio::test]
async fn guard_laedt_settings_trigger_und_persistiert_verdict() {
    let pool = pool_or_skip!("tb_conversation_scam_guard");
    sqlx::query(
        "INSERT INTO twitch_scam_guard_settings (channel_login, mode) \
         VALUES ('testchannel', 'alert_only')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) \
         VALUES (1, 'testchannel', NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_session_chatters \
         (session_id, streamer_login, chatter_login, is_first_time_streamer) \
         VALUES (1, 'testchannel', 'sam_09995', TRUE)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let api: Arc<dyn ChatApi> = Arc::new(NoopApi);
    let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api), pool.clone()));
    let guard = Arc::new(ConversationScamGuard::new(
        pool.clone(),
        "bot-id".to_string(),
        Arc::new(FixedJudge),
        api,
        moderation,
    ));
    let event = ChatMessageEvent {
        broadcaster_user_id: "channel-id".to_string(),
        broadcaster_user_login: "testchannel".to_string(),
        chatter_user_id: "sam-id".to_string(),
        chatter_user_login: "sam_09995".to_string(),
        message_id: "message-id".to_string(),
        message: ChatMessageBody {
            text: "ʏᴏ ʙʀᴏ, ᴀᴅᴅ ʜɪᴍ ᴏɴ ᴅɪꜱᴄᴏʀᴅ ᴀɴᴅ ɢʀᴏᴡ ᴡɪᴛʜ ʀᴇᴀʟ ᴠɪᴇᴡᴇʀꜱ.".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    guard.observe(&event);
    let row = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(row) = sqlx::query_as::<_, (String, String, String, f32)>(
                "SELECT verdict, action_taken, transcript_snapshot, confidence \
                 FROM twitch_scam_guard_verdicts LIMIT 1",
            )
            .fetch_optional(&pool)
            .await
            .unwrap()
            {
                break row;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Verdict wurde nicht persistiert");

    assert_eq!(row.0, "scam");
    assert_eq!(row.1, "suggested");
    assert!(row.2.contains("yo bro"));
    assert!((row.3 - 0.95).abs() < f32::EPSILON);
}

#[tokio::test]
async fn auto_ban_bannt_jungen_account() {
    let pool = pool_or_skip!("tb_conversation_scam_young_autoban");
    seed_first_time_guard(&pool, "auto_ban").await;

    let calls = run_action_guard(&pool, Some(Utc::now() - ChronoDuration::days(10))).await;
    assert_eq!(wait_action_taken(&pool).await, "banned");

    let calls = calls.lock().unwrap();
    assert_eq!(calls.bans.len(), 1);
    assert!(calls.timeouts.is_empty());
}

#[tokio::test]
async fn gemeldeter_befriending_pivot_bannt_null_tage_account_und_loescht_nachricht() {
    let pool = pool_or_skip!("tb_conversation_scam_reported_befriending");
    seed_first_time_guard(&pool, "auto_ban").await;

    let calls = run_action_guard_with_judge(
        &pool,
        Some(Utc::now()),
        Some(
            "Yo bruh, love ❤️ your stream Let's sometimes play together and share tips together. Let's connect on Discord",
        ),
        Arc::new(ZeroDayRiskJudge),
    )
    .await;
    assert_eq!(wait_action_taken(&pool).await, "banned");

    let calls = calls.lock().unwrap();
    assert_eq!(calls.bans.len(), 1);
    assert!(calls.timeouts.is_empty());
    assert_eq!(calls.deletes.len(), 1);
}

#[tokio::test]
async fn auto_ban_timeoutet_alten_account_und_loescht_nachricht() {
    let pool = pool_or_skip!("tb_conversation_scam_old_autoban");
    seed_first_time_guard(&pool, "auto_ban").await;

    let calls = run_action_guard(&pool, Some(Utc::now() - ChronoDuration::days(400))).await;
    assert_eq!(wait_action_taken(&pool).await, "timed_out");

    let calls = calls.lock().unwrap();
    assert!(calls.bans.is_empty());
    assert_eq!(calls.timeouts.len(), 1);
    assert_eq!(calls.deletes.len(), 1);
}

#[tokio::test]
async fn auto_ban_timeoutet_bei_unbekanntem_account_alter() {
    let pool = pool_or_skip!("tb_conversation_scam_unknown_age_autoban");
    seed_first_time_guard(&pool, "auto_ban").await;

    let calls = run_action_guard(&pool, None).await;
    let action_taken = wait_action_taken(&pool).await;

    let (ban_count, timeout_count) = {
        let calls = calls.lock().unwrap();
        (calls.bans.len(), calls.timeouts.len())
    };
    drop_schema(&pool, "tb_conversation_scam_unknown_age_autoban").await;

    assert_eq!(action_taken, "timed_out");
    assert_eq!(ban_count, 0);
    assert_eq!(timeout_count, 1);
}

#[tokio::test]
async fn cross_channel_erstnachricht_loest_judge_bei_kurzem_hallo_aus() {
    let pool = pool_or_skip!("tb_conversation_scam_cross_channel");
    seed_first_time_guard(&pool, "alert_only").await;
    sqlx::query(
        "UPDATE twitch_scam_guard_settings SET channel_login = 'suelze_' \
         WHERE channel_login = 'testchannel'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chat_messages \
         (id, session_id, streamer_login, chatter_login, message_ts, is_command, content) VALUES \
         (-1, 1, 'EaRlYsAlTy', 'FrEyA_1278', NOW() - INTERVAL '3 minutes', FALSE, 'hello'), \
         (-2, 2, 'SuElZe_', 'FrEyA_1278', NOW() - INTERVAL '1 minute', FALSE, 'hello'), \
         (-3, 3, 'earlysalty', 'old_chatter', NOW() - INTERVAL '3 hours', FALSE, 'hello'), \
         (-4, 4, 'suelze_', 'old_chatter', NOW() - INTERVAL '1 minute', FALSE, 'hello'), \
         (-5, 5, 'suelze_', 'current_only', NOW() - INTERVAL '1 minute', FALSE, 'hello')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let judge_inputs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let api: Arc<dyn ChatApi> = Arc::new(ActionRecordingApi {
        created_at: Some(Utc::now() - ChronoDuration::days(10)),
        calls: Arc::new(std::sync::Mutex::new(ActionCalls::default())),
    });
    let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api), pool.clone()));
    let guard = Arc::new(ConversationScamGuard::new(
        pool.clone(),
        "bot-id".to_string(),
        Arc::new(CrossChannelRecordingJudge {
            inputs: Arc::clone(&judge_inputs),
        }),
        api,
        moderation,
    ));
    for (chatter, message) in [
        ("FREYA_1278", "hey, how are you??"),
        ("old_chatter", "hello"),
        ("current_only", "hi"),
    ] {
        let mut event = scam_event();
        event.broadcaster_user_login = "SUELZE_".to_string();
        event.chatter_user_login = chatter.to_string();
        event.message.text = message.to_string();
        guard.observe(&event);
    }

    let judge_called = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if !judge_inputs.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .is_ok();
    let action_taken = if judge_called {
        Some(wait_action_taken(&pool).await)
    } else {
        None
    };
    let inputs = judge_inputs.lock().unwrap().clone();
    drop(guard);
    drop_schema(&pool, "tb_conversation_scam_cross_channel").await;

    assert_eq!(action_taken.as_deref(), Some("suggested"));
    assert_eq!(inputs, vec![("hey, how are you??".to_string(), 1)]);
}

#[tokio::test]
async fn timeout_mode_timeoutet_und_loescht_nachricht() {
    let pool = pool_or_skip!("tb_conversation_scam_timeout_mode");
    seed_first_time_guard(&pool, "timeout").await;

    let calls = run_action_guard(&pool, Some(Utc::now() - ChronoDuration::days(10))).await;
    assert_eq!(wait_action_taken(&pool).await, "timed_out");

    let calls = calls.lock().unwrap();
    assert_eq!(calls.timeouts.len(), 1);
    assert_eq!(calls.deletes.len(), 1);
}

#[tokio::test]
async fn self_learning_korpus_und_persistenz() {
    let pool = pool_or_skip!("tb_conversation_scam_learning");

    // Anfangs noch keine Erkenntnisse hinterlegt.
    assert_eq!(load_learnings(&pool).await.unwrap(), None);

    // Drei Verdicts: bestätigt (banned), Vorschlag (suggested), Fehlalarm (overturned).
    // Ein 'clean'/'none' darf NICHT in den Korpus geraten.
    for (chatter, verdict, action) in [
        ("scammer_a", "scam", "banned"),
        ("scammer_b", "scam", "suggested"),
        ("falsepos_c", "scam", "overturned"),
        ("legit_d", "clean", "none"),
    ] {
        sqlx::query(
            "INSERT INTO twitch_scam_guard_verdicts \
             (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
              reasoning, transcript_snapshot, action_taken) \
             VALUES ($1, $2, $3, $4, 0.9, 'cat', $5, '[\"msg\"]', $6)",
        )
        .bind("testchannel")
        .bind(chatter)
        .bind(format!("{chatter}-id"))
        .bind(verdict)
        .bind(format!("grund-{chatter}"))
        .bind(action)
        .execute(&pool)
        .await
        .unwrap();
    }

    let corpus = fetch_learning_corpus(&pool, 40).await;
    assert_eq!(corpus.confirmed.len(), 2, "banned + suggested = bestätigt");
    assert_eq!(corpus.false_positives.len(), 1, "overturned = Fehlalarm");
    assert_eq!(corpus.total(), 3);
    assert!(corpus
        .false_positives
        .iter()
        .any(|s| s.reasoning == "grund-falsepos_c"));

    // Persistenz + UPSERT (zweiter Schreibvorgang überschreibt die Singleton-Zeile).
    persist_learnings(&pool, "ERSTE ERKENNTNIS", corpus.total() as i32)
        .await
        .unwrap();
    assert_eq!(
        load_learnings(&pool).await.unwrap().as_deref(),
        Some("ERSTE ERKENNTNIS")
    );
    persist_learnings(&pool, "AKTUALISIERTE ERKENNTNIS", 5)
        .await
        .unwrap();
    assert_eq!(
        load_learnings(&pool).await.unwrap().as_deref(),
        Some("AKTUALISIERTE ERKENNTNIS")
    );

    // Genau eine Zeile (Singleton-Constraint).
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_scam_guard_learnings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn chat_unban_overturn_entfernt_ai_globalban_aber_keinen_manuellen() {
    let pool = pool_or_skip!("tb_conversation_scam_chat_unban_globalban");
    let commands = ScamGuardCommands::new(
        pool.clone(),
        EngagementLlmClient::new(None, None, None, None),
    );

    let ai_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_scam_guard_verdicts \
         (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
          reasoning, transcript_snapshot, action_taken) \
         VALUES ('testchannel', 'aiscam', 'ai-uid', 'scam', 0.95, 'cat', \
                 'grund', '[\"msg\"]', 'banned') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban \
         (chatter_login, chatter_id, reason, added_by) \
         VALUES ('aiscam', 'ai-uid', 'grund', 'conversation_scam_ai')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO twitch_chatter_global_ban_applied (chatter_login) VALUES ('aiscam')")
        .execute(&pool)
        .await
        .unwrap();

    assert!(commands.overturn("testchannel", "ai-uid").await);
    let action: String =
        sqlx::query_scalar("SELECT action_taken FROM twitch_scam_guard_verdicts WHERE id = $1")
            .bind(ai_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(action, "overturned");
    let ai_global_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban WHERE chatter_login = 'aiscam'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ai_global_count, 0);
    let ai_applied_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban_applied WHERE chatter_login = 'aiscam'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ai_applied_count, 0);

    sqlx::query(
        "INSERT INTO twitch_scam_guard_verdicts \
         (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
          reasoning, transcript_snapshot, action_taken) \
         VALUES ('testchannel', 'manualscam', 'manual-uid', 'scam', 0.95, 'cat', \
                 'grund', '[\"msg\"]', 'banned')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban \
         (chatter_login, chatter_id, reason, added_by) \
         VALUES ('manualscam', 'manual-uid', 'manual grund', 'manual')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban_applied (chatter_login) VALUES ('manualscam')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(commands.overturn("testchannel", "manual-uid").await);
    let manual_global_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban WHERE chatter_login = 'manualscam'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(manual_global_count, 1);
    let manual_applied_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban_applied WHERE chatter_login = 'manualscam'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(manual_applied_count, 1);
}

#[tokio::test]
async fn ai_globalban_ueberschreibt_keine_manuellen_eintraege() {
    let pool = pool_or_skip!("tb_conversation_scam_globalban_owner");

    add_conversation_scam_global_ban(&pool, "NewScam", Some("new-id"), "ai grund")
        .await
        .unwrap();
    let added: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT chatter_id, reason, added_by FROM twitch_chatter_global_ban \
         WHERE chatter_login = 'newscam'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        added,
        (
            Some("new-id".to_string()),
            Some("ai grund".to_string()),
            Some("conversation_scam_ai".to_string())
        )
    );

    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban \
         (chatter_login, chatter_id, reason, added_by) \
         VALUES ('manualscam', 'manual-id', 'manual grund', 'manual')",
    )
    .execute(&pool)
    .await
    .unwrap();
    add_conversation_scam_global_ban(&pool, "manualscam", Some("ai-id"), "ai neu")
        .await
        .unwrap();
    let manual: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT chatter_id, reason, added_by FROM twitch_chatter_global_ban \
         WHERE chatter_login = 'manualscam'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        manual,
        (
            Some("manual-id".to_string()),
            Some("manual grund".to_string()),
            Some("manual".to_string())
        )
    );

    add_conversation_scam_global_ban(&pool, "NewScam", Some("newer-id"), "ai neu")
        .await
        .unwrap();
    let updated: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT chatter_id, reason, added_by FROM twitch_chatter_global_ban \
         WHERE chatter_login = 'newscam'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        updated,
        (
            Some("newer-id".to_string()),
            Some("ai neu".to_string()),
            Some("conversation_scam_ai".to_string())
        )
    );
}

#[tokio::test]
async fn revoke_verdict_entbannt_bei_ban_und_markiert_overturned() {
    let pool = pool_or_skip!("tb_conversation_scam_revoke");

    // Echter Ban: muss entbannen UND als overturned markieren.
    let banned_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_scam_guard_verdicts \
         (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
          reasoning, transcript_snapshot, action_taken) \
         VALUES ('testchannel', 'scammer', 'scammer-uid', 'scam', 0.95, 'cat', \
                 'grund', '[\"msg\"]', 'banned') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban \
         (chatter_login, chatter_id, reason, added_by) \
         VALUES ('scammer', 'scammer-uid', 'grund', 'conversation_scam_ai')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO twitch_chatter_global_ban_applied (chatter_login) VALUES ('scammer')")
        .execute(&pool)
        .await
        .unwrap();

    let recorder = Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
    let api: Arc<dyn ChatApi> = Arc::new(RecordingApi {
        unbans: recorder.clone(),
    });

    let outcome = revoke_verdict(&pool, api.as_ref(), banned_id).await;
    assert_eq!(outcome.status, "revoked");
    assert!(outcome.was_banned);
    assert!(outcome.unbanned);
    // Unban lief mit aufgelöster Broadcaster-ID + gespeicherter Chatter-ID.
    assert_eq!(
        recorder.lock().unwrap().clone(),
        vec![("testchannel-id".to_string(), "scammer-uid".to_string())]
    );

    let action: String =
        sqlx::query_scalar("SELECT action_taken FROM twitch_scam_guard_verdicts WHERE id = $1")
            .bind(banned_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(action, "overturned");
    let global_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban WHERE chatter_login = 'scammer'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(global_count, 0, "AI-Globalban wurde beim Revoke entfernt");
    let applied_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban_applied WHERE chatter_login = 'scammer'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        applied_count, 0,
        "AI-Sweep-Ledger wurde beim Revoke entfernt"
    );

    // Vorschlag (kein Ban): KEIN Unban, trotzdem overturned.
    let suggested_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_scam_guard_verdicts \
         (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
          reasoning, transcript_snapshot, action_taken) \
         VALUES ('testchannel', 'maybe', 'maybe-uid', 'scam', 0.8, 'cat', \
                 'grund', '[\"msg\"]', 'suggested') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    recorder.lock().unwrap().clear();

    let outcome = revoke_verdict(&pool, api.as_ref(), suggested_id).await;
    assert_eq!(outcome.status, "revoked");
    assert!(!outcome.was_banned);
    assert!(!outcome.unbanned);
    assert!(
        recorder.lock().unwrap().is_empty(),
        "ein Vorschlag darf keinen Unban auslösen"
    );
    let action: String =
        sqlx::query_scalar("SELECT action_taken FROM twitch_scam_guard_verdicts WHERE id = $1")
            .bind(suggested_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(action, "overturned");

    // Timeout zählt wie ein Ban: muss ebenfalls entbannen + markieren.
    recorder.lock().unwrap().clear();
    let timed_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_scam_guard_verdicts \
         (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
          reasoning, transcript_snapshot, action_taken) \
         VALUES ('testchannel', 'timed', 'timed-uid', 'scam', 0.9, 'cat', \
                 'grund', '[\"msg\"]', 'timed_out') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban \
         (chatter_login, chatter_id, reason, added_by) \
         VALUES ('timed', 'timed-uid', 'manuell', 'manual')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO twitch_chatter_global_ban_applied (chatter_login) VALUES ('timed')")
        .execute(&pool)
        .await
        .unwrap();
    let outcome = revoke_verdict(&pool, api.as_ref(), timed_id).await;
    assert!(outcome.was_banned, "timed_out zählt als Ban");
    assert!(outcome.unbanned);
    assert_eq!(
        recorder.lock().unwrap().clone(),
        vec![("testchannel-id".to_string(), "timed-uid".to_string())]
    );
    let action: String =
        sqlx::query_scalar("SELECT action_taken FROM twitch_scam_guard_verdicts WHERE id = $1")
            .bind(timed_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(action, "overturned");
    let manual_global_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban WHERE chatter_login = 'timed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        manual_global_count, 1,
        "manuelle Globalbans darf Revoke nicht entfernen"
    );
    let manual_applied_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban_applied WHERE chatter_login = 'timed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        manual_applied_count, 1,
        "manuelle Sweep-Ledger bleiben bestehen"
    );

    // Unbekannte ID → not_found.
    let missing = revoke_verdict(&pool, api.as_ref(), 9_999_999).await;
    assert_eq!(missing.status, "not_found");
}

#[tokio::test]
async fn conversation_scam_enforce_verdict_bannt_vorschlag_mit_gespeicherter_begruendung() {
    let pool = pool_or_skip!("tb_conversation_scam_enforce");
    let verdict_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_scam_guard_verdicts \
         (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
          reasoning, transcript_snapshot, action_taken) \
         VALUES ('testchannel', 'scammer', 'scammer-uid', 'scam', 0.85, 'cat', \
                 'gespeicherte begruendung', '[\"msg\"]', 'suggested') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let recorder = Arc::new(std::sync::Mutex::new(Vec::<(String, String, String)>::new()));
    let api: Arc<dyn ChatApi> = Arc::new(EnforceRecordingApi {
        bans: recorder.clone(),
    });

    let outcome = enforce_verdict(&pool, api.as_ref(), verdict_id).await;

    assert_eq!(outcome.status, "enforced");
    assert_eq!(outcome.channel_login, "testchannel");
    assert_eq!(outcome.chatter_login, "scammer");
    assert!(outcome.banned);
    assert_eq!(
        recorder.lock().unwrap().clone(),
        vec![(
            "testchannel-id".to_string(),
            "scammer-uid".to_string(),
            "gespeicherte begruendung".to_string()
        )]
    );
    let action: String =
        sqlx::query_scalar("SELECT action_taken FROM twitch_scam_guard_verdicts WHERE id = $1")
            .bind(verdict_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(action, "banned");
}

#[tokio::test]
async fn conversation_scam_enforce_verdict_lehnt_bearbeitetes_urteil_ohne_ban_ab() {
    let pool = pool_or_skip!("tb_conversation_scam_enforce_not_eligible");
    let verdict_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_scam_guard_verdicts \
         (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
          reasoning, transcript_snapshot, action_taken) \
         VALUES ('testchannel', 'scammer', 'scammer-uid', 'scam', 0.95, 'cat', \
                 'grund', '[\"msg\"]', 'banned') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let recorder = Arc::new(std::sync::Mutex::new(Vec::<(String, String, String)>::new()));
    let api: Arc<dyn ChatApi> = Arc::new(EnforceRecordingApi {
        bans: recorder.clone(),
    });

    let outcome = enforce_verdict(&pool, api.as_ref(), verdict_id).await;

    assert_eq!(outcome.status, "not_eligible");
    assert!(!outcome.banned);
    assert!(recorder.lock().unwrap().is_empty());
}

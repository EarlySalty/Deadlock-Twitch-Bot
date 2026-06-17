use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::api::{BanOutcome, ChatApi};
use tb_chat::conversation_scam::{
    ConversationScamGuard, DialogState, ScamJudge, Verdict, VerdictKind,
};
use tb_chat::moderation::ModerationEngine;
use tb_chat::types::{ChatMessageBody, ChatMessageEvent, SendOutcome};

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
    for ddl in [
        "CREATE TABLE twitch_scam_guard_settings (\
            channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT TRUE, \
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

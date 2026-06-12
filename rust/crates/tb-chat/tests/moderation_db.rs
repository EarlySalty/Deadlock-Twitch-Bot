//! Hermetische DB-Tests für ModerationEngine — tb_chat_autoban_log.
//!
//! Testet den DB-Schreibpfad von [`ModerationEngine::auto_ban_and_cleanup`].
//! Schema-isoliert; tb_chat_autoban_log ist eine neue Tabelle (nicht in Prod-Schema —
//! muss durch den Bot-Start angelegt werden).

use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::api::{BanOutcome, ChatApi};
use tb_chat::moderation::{AutoBanRequest, ModerationEngine, BAN_REASON_SPAM, BAN_REASON_GLOBAL, NOTICE_GLOBAL_BAN};
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
    // tb_chat_autoban_log: neue Tabelle, nicht in Prod-Schema vorhanden.
    // Wird durch den Bot-Start angelegt; hier prod-treu nachgebaut.
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS tb_chat_autoban_log (
            id BIGSERIAL PRIMARY KEY,
            channel_login TEXT NOT NULL,
            chatter_id TEXT NOT NULL,
            chatter_login TEXT NOT NULL,
            content TEXT,
            banned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
    )
    .execute(pool)
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// Mock-ChatApi
// ---------------------------------------------------------------------------

struct OkApi;

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn autoban_schreibt_in_db() {
    let pool = pool_or_skip!("autoban_write");
    let engine = ModerationEngine::new(Arc::new(OkApi), pool.clone());

    let ok = engine
        .auto_ban_and_cleanup(AutoBanRequest {
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
        })
        .await;
    assert!(ok, "AutoBan soll true zurückgeben");

    // DB-Eintrag prüfen
    let (channel, chatter_login, content): (String, String, Option<String>) = sqlx::query_as(
        "SELECT channel_login, chatter_login, content FROM tb_chat_autoban_log LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(channel, "testkanal");
    assert_eq!(chatter_login, "spammer42");
    assert_eq!(content.as_deref(), Some("Spam-Inhalt hier"));
}

#[tokio::test]
async fn autoban_last_record_in_memory_gesetzt() {
    let pool = pool_or_skip!("autoban_mem");
    let engine = ModerationEngine::new(Arc::new(OkApi), pool);

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

    let rec = engine.last_autoban("memkanal");
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
    let engine = ModerationEngine::new(Arc::new(OkApi), pool.clone());

    engine
        .auto_ban_and_cleanup(AutoBanRequest {
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
        })
        .await;

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM tb_chat_autoban_log WHERE channel_login = 'delkanal'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "Delete-only soll auch in DB persistiert werden");
}

#[tokio::test]
async fn content_wird_auf_500_zeichen_begrenzt() {
    let pool = pool_or_skip!("autoban_trunc");
    let engine = ModerationEngine::new(Arc::new(OkApi), pool.clone());

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

    let content: Option<String> =
        sqlx::query_scalar("SELECT content FROM tb_chat_autoban_log WHERE channel_login = 'trunckanal'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        content.unwrap().len(),
        500,
        "content auf 500 Zeichen begrenzt (moderation.py Z. 576)"
    );
}

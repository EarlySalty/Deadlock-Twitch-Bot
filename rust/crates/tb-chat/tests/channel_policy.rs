use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tb_chat::api::{AnnouncementOutcome, BanOutcome};
use tb_chat::channel_policy::{ChannelPolicyChatApi, PolicyContext};
use tb_chat::global_ban_sweep::PartnerRoster;
use tb_chat::types::SendOutcome;
use tb_chat::ChatApi;

#[derive(Default)]
struct RecordingApi {
    calls: Mutex<Vec<&'static str>>,
}

impl RecordingApi {
    fn record(&self, action: &'static str) {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(action);
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[async_trait]
impl ChatApi for RecordingApi {
    async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
        self.record("send_message");
        Ok(SendOutcome::Sent)
    }

    async fn send_whisper(&self, _: &str, _: &str) -> Result<bool, String> {
        self.record("send_whisper");
        Ok(true)
    }

    async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
        self.record("send_announcement");
        Ok(true)
    }

    async fn send_announcement_detailed(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<AnnouncementOutcome, String> {
        self.record("send_announcement_detailed");
        Ok(AnnouncementOutcome::accepted())
    }

    async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
        self.record("ban_user");
        Ok(BanOutcome::Banned)
    }

    async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<BanOutcome, String> {
        self.record("timeout_user");
        Ok(BanOutcome::Banned)
    }

    async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
        self.record("unban_user");
        Ok(true)
    }

    async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
        self.record("delete_message");
        Ok(true)
    }

    async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
        self.record("user_created_at");
        Ok(None)
    }

    async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
        self.record("resolve_user_id");
        Ok(Some("resolved".to_string()))
    }

    async fn bot_user_id(&self) -> String {
        self.record("bot_user_id");
        "bot".to_string()
    }
}

struct StaticRoster {
    partner: bool,
}

#[async_trait]
impl PartnerRoster for StaticRoster {
    async fn all_active_partners(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    async fn valid_auth_ids(&self) -> HashSet<String> {
        HashSet::new()
    }

    async fn live_broadcaster_ids(&self) -> HashSet<String> {
        HashSet::new()
    }

    async fn is_operational_partner_channel(&self, _: &str) -> bool {
        self.partner
    }

    async fn global_ban_enforcement_enabled(&self, _: &str) -> bool {
        true
    }
}

fn policy(partner: bool) -> (Arc<RecordingApi>, ChannelPolicyChatApi) {
    let inner = Arc::new(RecordingApi::default());
    let api = ChannelPolicyChatApi::new(
        Arc::clone(&inner) as Arc<dyn ChatApi>,
        PolicyContext::Standard(Arc::new(StaticRoster { partner })),
    );
    (inner, api)
}

fn assert_denied<T>(result: Result<T, String>, inner: &RecordingApi) {
    assert_eq!(result.err().as_deref(), Some("channel_policy_denied"));
    assert!(inner.calls().is_empty(), "inner API must not be called");
}

#[derive(Clone, Default)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn non_partner_send_message_is_denied_without_inner_call_and_warned() {
    let writer = LogWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer({
            let writer = writer.clone();
            move || writer.clone()
        })
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let (inner, api) = policy(false);

    assert_denied(api.send_message("outsider", "hello").await, &inner);

    let logs = String::from_utf8(
        writer
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
    )
    .expect("tracing output must be UTF-8");
    assert!(logs.contains("WARN"));
    assert!(logs.contains("channel=\"outsider\""));
    assert!(logs.contains("action=\"send_message\""));
    assert!(logs.contains("target_user=\"-\""));
    assert!(logs.contains("reason=\"channel_policy_denied\""));
}

#[tokio::test]
async fn non_partner_ban_user_is_denied() {
    let (inner, api) = policy(false);
    assert_denied(api.ban_user("outsider", "target", "reason").await, &inner);
}

#[tokio::test]
async fn non_partner_timeout_user_is_denied() {
    let (inner, api) = policy(false);
    assert_denied(
        api.timeout_user("outsider", "target", 60, "reason").await,
        &inner,
    );
}

#[tokio::test]
async fn non_partner_unban_user_is_denied() {
    let (inner, api) = policy(false);
    assert_denied(api.unban_user("outsider", "target").await, &inner);
}

#[tokio::test]
async fn non_partner_delete_message_is_denied() {
    let (inner, api) = policy(false);
    assert_denied(api.delete_message("outsider", "message").await, &inner);
}

#[tokio::test]
async fn non_partner_send_whisper_is_denied() {
    let (inner, api) = policy(false);
    assert_denied(api.send_whisper("outsider", "hello").await, &inner);
}

#[tokio::test]
async fn non_partner_announcements_are_denied() {
    let (inner, api) = policy(false);
    assert_denied(
        api.send_announcement("outsider", "hello", "primary").await,
        &inner,
    );
    assert_denied(
        api.send_announcement_detailed("outsider", "hello", "primary")
            .await,
        &inner,
    );
}

#[tokio::test]
async fn partner_writes_and_reads_are_forwarded() {
    let (inner, api) = policy(true);

    assert_eq!(
        api.send_message("partner", "hello").await,
        Ok(SendOutcome::Sent)
    );
    assert_eq!(api.send_whisper("partner", "hello").await, Ok(true));
    assert_eq!(
        api.send_announcement("partner", "hello", "primary").await,
        Ok(true)
    );
    assert_eq!(
        api.send_announcement_detailed("partner", "hello", "primary")
            .await,
        Ok(AnnouncementOutcome::accepted())
    );
    assert_eq!(
        api.ban_user("partner", "target", "reason").await,
        Ok(BanOutcome::Banned)
    );
    assert_eq!(
        api.timeout_user("partner", "target", 60, "reason").await,
        Ok(BanOutcome::Banned)
    );
    assert_eq!(api.unban_user("partner", "target").await, Ok(true));
    assert_eq!(api.delete_message("partner", "message").await, Ok(true));
    assert_eq!(api.user_created_at("target").await, Ok(None));
    assert_eq!(
        api.resolve_user_id("target").await,
        Ok(Some("resolved".to_string()))
    );
    assert_eq!(api.bot_user_id().await, "bot");
    assert_eq!(
        inner.calls(),
        vec![
            "send_message",
            "send_whisper",
            "send_announcement",
            "send_announcement_detailed",
            "ban_user",
            "timeout_user",
            "unban_user",
            "delete_message",
            "user_created_at",
            "resolve_user_id",
            "bot_user_id",
        ]
    );
}

#[tokio::test]
async fn raid_context_allows_message_and_whisper_but_not_moderation() {
    let inner = Arc::new(RecordingApi::default());
    let api =
        ChannelPolicyChatApi::new(Arc::clone(&inner) as Arc<dyn ChatApi>, PolicyContext::Raid);

    assert_eq!(
        api.send_message("non_partner", "hello").await,
        Ok(SendOutcome::Sent)
    );
    assert_eq!(api.send_whisper("non_partner", "hello").await, Ok(true));
    assert_eq!(inner.calls(), vec!["send_message", "send_whisper"]);

    let result = api.ban_user("non_partner", "target", "reason").await;
    assert_eq!(result.err().as_deref(), Some("channel_policy_denied"));
    assert_eq!(inner.calls(), vec!["send_message", "send_whisper"]);
}

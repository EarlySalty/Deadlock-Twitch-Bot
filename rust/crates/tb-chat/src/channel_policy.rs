//! Zentraler Kanal-Policy-Decorator fuer alle Twitch-Schreibaktionen.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::api::{AnnouncementOutcome, BanOutcome, ChatApi, SourceOnlyPreSendCheck};
use crate::global_ban_sweep::PartnerRoster;
use crate::types::SendOutcome;

const DENIED_REASON: &str = "channel_policy_denied";

pub enum PolicyContext {
    Standard(Arc<dyn PartnerRoster>),
    Raid(Arc<dyn PartnerRoster>),
}

pub struct ChannelPolicyChatApi {
    inner: Arc<dyn ChatApi>,
    context: PolicyContext,
}

impl ChannelPolicyChatApi {
    pub fn new(inner: Arc<dyn ChatApi>, context: PolicyContext) -> Self {
        Self { inner, context }
    }

    async fn authorize(
        &self,
        channel: &str,
        action: WriteAction,
        target_user: Option<&str>,
    ) -> Result<(), String> {
        let allowed = match &self.context {
            PolicyContext::Standard(roster) => roster.is_operational_partner_channel(channel).await,
            PolicyContext::Raid(roster) => {
                action.allowed_for_raid() && roster.is_operational_partner_channel(channel).await
            }
        };
        if allowed {
            return Ok(());
        }
        tracing::warn!(
            channel,
            action = action.name(),
            target_user = target_user.unwrap_or("-"),
            reason = DENIED_REASON,
            "Channel-Policy verweigert Twitch-Schreibaktion"
        );
        Err(DENIED_REASON.to_string())
    }
}

#[derive(Clone, Copy)]
enum WriteAction {
    SendMessage,
    SendSourceOnlyMessage,
    SendWhisper,
    SendAnnouncement,
    SendAnnouncementDetailed,
    BanUser,
    TimeoutUser,
    UnbanUser,
    DeleteMessage,
}

impl WriteAction {
    fn name(self) -> &'static str {
        match self {
            Self::SendMessage => "send_message",
            Self::SendSourceOnlyMessage => "send_source_only_message",
            Self::SendWhisper => "send_whisper",
            Self::SendAnnouncement => "send_announcement",
            Self::SendAnnouncementDetailed => "send_announcement_detailed",
            Self::BanUser => "ban_user",
            Self::TimeoutUser => "timeout_user",
            Self::UnbanUser => "unban_user",
            Self::DeleteMessage => "delete_message",
        }
    }

    fn allowed_for_raid(self) -> bool {
        match self {
            Self::SendMessage | Self::SendWhisper => true,
            Self::SendAnnouncement
            | Self::SendSourceOnlyMessage
            | Self::SendAnnouncementDetailed
            | Self::BanUser
            | Self::TimeoutUser
            | Self::UnbanUser
            | Self::DeleteMessage => false,
        }
    }
}

#[async_trait]
impl ChatApi for ChannelPolicyChatApi {
    async fn send_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String> {
        self.authorize(broadcaster_id, WriteAction::SendMessage, None)
            .await?;
        self.inner.send_message(broadcaster_id, message).await
    }

    async fn send_source_only_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String> {
        self.send_source_only_message_guarded(broadcaster_id, message, Box::new(|| Ok(())))
            .await
    }

    async fn send_source_only_message_guarded(
        &self,
        broadcaster_id: &str,
        message: &str,
        pre_send_check: SourceOnlyPreSendCheck,
    ) -> Result<SendOutcome, String> {
        self.authorize(broadcaster_id, WriteAction::SendSourceOnlyMessage, None)
            .await?;
        self.inner
            .send_source_only_message_guarded(broadcaster_id, message, pre_send_check)
            .await
    }

    async fn send_thread_reply(
        &self,
        broadcaster_id: &str,
        parent_message_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String> {
        self.authorize(broadcaster_id, WriteAction::SendMessage, None)
            .await?;
        self.inner
            .send_thread_reply(broadcaster_id, parent_message_id, message)
            .await
    }

    async fn send_whisper(&self, to_user_id: &str, message: &str) -> Result<bool, String> {
        self.authorize(to_user_id, WriteAction::SendWhisper, Some(to_user_id))
            .await?;
        self.inner.send_whisper(to_user_id, message).await
    }

    async fn send_announcement(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<bool, String> {
        self.authorize(broadcaster_id, WriteAction::SendAnnouncement, None)
            .await?;
        self.inner
            .send_announcement(broadcaster_id, message, color)
            .await
    }

    async fn send_announcement_detailed(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<AnnouncementOutcome, String> {
        self.authorize(broadcaster_id, WriteAction::SendAnnouncementDetailed, None)
            .await?;
        self.inner
            .send_announcement_detailed(broadcaster_id, message, color)
            .await
    }

    async fn ban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        self.authorize(broadcaster_id, WriteAction::BanUser, Some(target_user_id))
            .await?;
        self.inner
            .ban_user(broadcaster_id, target_user_id, reason)
            .await
    }

    async fn timeout_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        duration_secs: u32,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        self.authorize(
            broadcaster_id,
            WriteAction::TimeoutUser,
            Some(target_user_id),
        )
        .await?;
        self.inner
            .timeout_user(broadcaster_id, target_user_id, duration_secs, reason)
            .await
    }

    async fn unban_user(&self, broadcaster_id: &str, target_user_id: &str) -> Result<bool, String> {
        self.authorize(broadcaster_id, WriteAction::UnbanUser, Some(target_user_id))
            .await?;
        self.inner.unban_user(broadcaster_id, target_user_id).await
    }

    async fn delete_message(&self, broadcaster_id: &str, message_id: &str) -> Result<bool, String> {
        self.authorize(broadcaster_id, WriteAction::DeleteMessage, None)
            .await?;
        self.inner.delete_message(broadcaster_id, message_id).await
    }

    async fn user_created_at(&self, user_id: &str) -> Result<Option<DateTime<Utc>>, String> {
        self.inner.user_created_at(user_id).await
    }

    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
        self.inner.resolve_user_id(login).await
    }

    async fn bot_user_id(&self) -> String {
        self.inner.bot_user_id().await
    }
}

#[cfg(test)]
mod source_only_tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Roster(bool);

    #[async_trait]
    impl PartnerRoster for Roster {
        async fn all_active_partners(&self) -> Vec<(String, String)> {
            Vec::new()
        }
        async fn valid_auth_ids(&self) -> HashSet<String> {
            HashSet::new()
        }
        async fn live_broadcaster_ids(&self) -> HashSet<String> {
            HashSet::new()
        }
        async fn is_operational_partner_channel(&self, id: &str) -> bool {
            self.0 && id == "111"
        }
        async fn global_ban_enforcement_enabled(&self, _: &str) -> bool {
            true
        }
        async fn streamer_global_ban_enabled(&self, _: &str) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct RecordingApi {
        ordinary: AtomicUsize,
        source_only: AtomicUsize,
    }

    #[async_trait]
    impl ChatApi for RecordingApi {
        async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
            self.ordinary.fetch_add(1, Ordering::SeqCst);
            Ok(SendOutcome::Sent)
        }
        async fn send_source_only_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
            self.source_only.fetch_add(1, Ordering::SeqCst);
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            unimplemented!()
        }
        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
            unimplemented!()
        }
        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &str,
        ) -> Result<BanOutcome, String> {
            unimplemented!()
        }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            unimplemented!()
        }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            unimplemented!()
        }
        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
            unimplemented!()
        }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            unimplemented!()
        }
        async fn bot_user_id(&self) -> String {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn source_only_requires_operational_partner_and_standard_context() {
        let inner = Arc::new(RecordingApi::default());
        let standard = ChannelPolicyChatApi::new(
            inner.clone(),
            PolicyContext::Standard(Arc::new(Roster(true))),
        );
        assert_eq!(
            standard
                .send_source_only_message("111", "Patch!")
                .await
                .unwrap(),
            SendOutcome::Sent
        );
        assert_eq!(
            standard
                .send_source_only_message("999", "Patch!")
                .await
                .unwrap_err(),
            DENIED_REASON
        );
        let denied = ChannelPolicyChatApi::new(
            inner.clone(),
            PolicyContext::Standard(Arc::new(Roster(false))),
        );
        assert_eq!(
            denied
                .send_source_only_message("111", "Patch!")
                .await
                .unwrap_err(),
            DENIED_REASON
        );
        let raid =
            ChannelPolicyChatApi::new(inner.clone(), PolicyContext::Raid(Arc::new(Roster(true))));
        assert_eq!(
            raid.send_source_only_message("111", "Patch!")
                .await
                .unwrap_err(),
            DENIED_REASON
        );
        assert_eq!(inner.source_only.load(Ordering::SeqCst), 1);
        assert_eq!(inner.ordinary.load(Ordering::SeqCst), 0);
    }
}

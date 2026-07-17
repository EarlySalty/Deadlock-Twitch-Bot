//! HeadlessNoop — kein Netz, kein Panic. Für Tests und headless Builds.

use crate::backend::{
    DeleteMessage, DiscordBackend, DiscordError, EditRichMessage, SendAlertEmbed, SendResult,
    SendResultInner, SendRichMessage, SendUserDm,
};

/// Verwirft alle Discord-Nachrichten stillschweigend. Nützlich in Tests
/// und in Umgebungen ohne Bridge-Zugang.
#[derive(Debug, Default, Clone)]
pub struct HeadlessNoop;

#[async_trait::async_trait]
impl DiscordBackend for HeadlessNoop {
    async fn send_rich_message(
        &self,
        _payload: SendRichMessage,
    ) -> Result<SendResult, DiscordError> {
        Ok(SendResult {
            ok: true,
            result: SendResultInner {
                message_id: "noop-0".to_string(),
            },
        })
    }

    async fn edit_rich_message(&self, _payload: EditRichMessage) -> Result<(), DiscordError> {
        Ok(())
    }

    async fn delete_message(&self, _payload: DeleteMessage) -> Result<(), DiscordError> {
        Ok(())
    }

    async fn send_user_dm(&self, _payload: SendUserDm) -> Result<SendResult, DiscordError> {
        Ok(SendResult {
            ok: true,
            result: SendResultInner {
                message_id: "noop-0".to_string(),
            },
        })
    }

    async fn send_alert_embed(&self, _payload: SendAlertEmbed) -> Result<SendResult, DiscordError> {
        Ok(SendResult {
            ok: true,
            result: SendResultInner {
                message_id: "noop-0".to_string(),
            },
        })
    }

    async fn remove_member_role(
        &self,
        _guild_id: u64,
        _user_id: u64,
        _role_id: u64,
        _reason: &str,
    ) -> Result<(), DiscordError> {
        Ok(())
    }
}

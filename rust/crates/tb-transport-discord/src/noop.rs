//! HeadlessNoop — kein Netz, kein Panic. Für Tests und headless Builds.

use crate::backend::{
    DiscordBackend, DiscordError, EditRichMessage, SendResult, SendResultInner, SendRichMessage,
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
}

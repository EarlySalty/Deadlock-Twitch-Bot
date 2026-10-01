use crate::client::{check_status_and_json, HelixClient, HelixError};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct SharedChatSession {
    pub session_id: String,
    pub host_broadcaster_id: String,
    pub participants: Vec<SharedChatParticipant>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SharedChatParticipant {
    pub broadcaster_id: String,
}

impl HelixClient {
    pub async fn get_shared_chat_session(
        &self,
        broadcaster_id: &str,
    ) -> Result<Option<SharedChatSession>, HelixError> {
        if broadcaster_id.is_empty() || !broadcaster_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(HelixError::InvalidResponse("invalid broadcaster id"));
        }
        let request = self
            .get("/shared_chat/session")
            .await?
            .query(&[("broadcaster_id", broadcaster_id)]);
        let response = self.send_with_retry(request).await?;
        #[derive(Deserialize)]
        struct Envelope {
            data: Vec<SharedChatSession>,
        }
        let mut body: Envelope = check_status_and_json(response).await?;
        if body.data.len() > 1 {
            return Err(HelixError::InvalidResponse("multiple shared chat sessions"));
        }
        let Some(session) = body.data.pop() else {
            return Ok(None);
        };
        if session.session_id.is_empty()
            || session.host_broadcaster_id.is_empty()
            || !session
                .participants
                .iter()
                .any(|p| p.broadcaster_id == broadcaster_id)
            || session.participants.iter().any(|p| {
                p.broadcaster_id.is_empty() || !p.broadcaster_id.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err(HelixError::InvalidResponse(
                "incomplete shared chat identity",
            ));
        }
        Ok(Some(session))
    }
}

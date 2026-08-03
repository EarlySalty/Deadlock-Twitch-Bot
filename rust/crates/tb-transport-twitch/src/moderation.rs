//! Helix-Moderations-Endpoints — Bot als Kanal-Moderator einsetzen.
//! Port von `bot/raid/services/partner_setup_service.py:440-479`
//! (`POST /moderation/moderators` mit dem **Streamer-Token**).

use crate::client::{HelixClient, HelixError};

/// Ausgang der Moderator-Einsetzung (Python behandelt alle Fälle nur per Log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddModeratorOutcome {
    /// 200/204 — Bot ist jetzt Moderator.
    Added,
    /// 422 oder 400 mit "already a mod" im Body — war bereits Moderator.
    AlreadyModerator,
    /// 400 mit "user is banned" — der Bot ist im Zielkanal gebannt und kann
    /// nicht automatisch wieder als Moderator gesetzt werden.
    BotBanned,
    /// Alle übrigen Antworten (Python: Warning, kein Abbruch).
    Failed { status: u16, body: String },
}

/// Ausgang der Moderator-Entfernung (Gegenstück zu [`AddModeratorOutcome`]).
///
/// Genutzt vom bewussten Trennen: der Bot gibt die Mod-Rechte in einem Kanal ab.
/// Twitch verlangt dafür den **Broadcaster-Token** — ein Moderator kann sich
/// nicht selbst entmoderieren.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveModeratorOutcome {
    /// 200/204 — die Mod-Rechte sind weg.
    Removed,
    /// 422 oder 400 mit "not a mod"/"is not a moderator" im Body — war ohnehin
    /// kein Moderator. Für den Aufrufer derselbe Zielzustand wie `Removed`.
    NotModerator,
    /// Alle übrigen Antworten (Aufrufer entscheidet, ob er das meldet).
    Failed { status: u16, body: String },
}

impl HelixClient {
    /// Nimmt `user_id` die Moderator-Rechte im Kanal `broadcaster_id`.
    /// `user_token` ist der Access-Token des Broadcasters
    /// (Scope `channel:manage:moderators`).
    pub async fn remove_channel_moderator(
        &self,
        broadcaster_id: &str,
        user_id: &str,
        user_token: &str,
    ) -> Result<RemoveModeratorOutcome, HelixError> {
        let resp = self
            .delete_with_user_token("/moderation/moderators", user_token)
            .query(&[("broadcaster_id", broadcaster_id), ("user_id", user_id)])
            .send()
            .await?;
        let status = resp.status().as_u16();
        match status {
            200 | 204 => Ok(RemoveModeratorOutcome::Removed),
            422 => Ok(RemoveModeratorOutcome::NotModerator),
            _ => {
                let body = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Remove-Moderator: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                let body_lower = body.to_lowercase();
                if status == 400
                    && (body_lower.contains("not a mod")
                        || body_lower.contains("is not a moderator"))
                {
                    Ok(RemoveModeratorOutcome::NotModerator)
                } else {
                    let snippet: String = body.chars().take(200).collect();
                    Ok(RemoveModeratorOutcome::Failed {
                        status,
                        body: snippet,
                    })
                }
            }
        }
    }

    /// Setzt `user_id` als Moderator im Kanal `broadcaster_id` ein.
    /// `user_token` ist der Access-Token des Broadcasters
    /// (Scope `channel:manage:moderators`).
    pub async fn add_channel_moderator(
        &self,
        broadcaster_id: &str,
        user_id: &str,
        user_token: &str,
    ) -> Result<AddModeratorOutcome, HelixError> {
        let resp = self
            .post_with_user_token("/moderation/moderators", user_token)
            .query(&[("broadcaster_id", broadcaster_id), ("user_id", user_id)])
            .send()
            .await?;
        let status = resp.status().as_u16();
        match status {
            200 | 204 => Ok(AddModeratorOutcome::Added),
            422 => Ok(AddModeratorOutcome::AlreadyModerator),
            _ => {
                let body = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Add-Moderator: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                let body_lower = body.to_lowercase();
                if status == 400 && body_lower.contains("already a mod") {
                    Ok(AddModeratorOutcome::AlreadyModerator)
                } else if status == 400 && body_lower.contains("user is banned") {
                    Ok(AddModeratorOutcome::BotBanned)
                } else {
                    let snippet: String = body.chars().take(200).collect();
                    Ok(AddModeratorOutcome::Failed {
                        status,
                        body: snippet,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HelixConfig;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_with(server: &MockServer) -> HelixClient {
        let mut config = HelixConfig::new("client-id", "client-secret");
        config.helix_base = server.uri();
        config.token_url = format!("{}/oauth2/token", server.uri());
        HelixClient::new(config).unwrap()
    }

    #[tokio::test]
    async fn added_bei_204() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/moderation/moderators"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("user_id", "bot"))
            .and(header("Authorization", "Bearer streamer-token"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .add_channel_moderator("111", "bot", "streamer-token")
            .await
            .unwrap();
        assert_eq!(outcome, AddModeratorOutcome::Added);
    }

    #[tokio::test]
    async fn already_moderator_bei_422_und_400_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/moderation/moderators"))
            .respond_with(ResponseTemplate::new(422))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .add_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert_eq!(outcome, AddModeratorOutcome::AlreadyModerator);

        server.reset().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string(r#"{"message":"user is already a mod"}"#),
            )
            .mount(&server)
            .await;
        let outcome = client
            .add_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert_eq!(outcome, AddModeratorOutcome::AlreadyModerator);
    }

    #[tokio::test]
    async fn failed_bei_sonstigem_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .add_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            AddModeratorOutcome::Failed { status: 401, .. }
        ));
    }

    #[tokio::test]
    async fn removed_bei_204() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/moderation/moderators"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("user_id", "bot"))
            .and(header("Authorization", "Bearer streamer-token"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .remove_channel_moderator("111", "bot", "streamer-token")
            .await
            .unwrap();
        assert_eq!(outcome, RemoveModeratorOutcome::Removed);
    }

    #[tokio::test]
    async fn not_moderator_bei_422_und_400_body() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(422))
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .remove_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert_eq!(outcome, RemoveModeratorOutcome::NotModerator);

        server.reset().await;
        Mock::given(method("DELETE"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string(r#"{"message":"user is not a moderator"}"#),
            )
            .mount(&server)
            .await;
        let outcome = client
            .remove_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert_eq!(outcome, RemoveModeratorOutcome::NotModerator);
    }

    #[tokio::test]
    async fn remove_failed_bei_sonstigem_status() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .remove_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            RemoveModeratorOutcome::Failed { status: 401, .. }
        ));
    }

    #[tokio::test]
    async fn bot_banned_bei_400_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(400).set_body_string(r#"{"message":"user is banned"}"#),
            )
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .add_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert_eq!(outcome, AddModeratorOutcome::BotBanned);
    }
}

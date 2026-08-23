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
    /// 400/403 mit eindeutigem Ban-/Block-Hinweis im Body und ohne Auth-Signal.
    /// Der `body` bleibt erhalten: ohne ihn ist ein Fehlurteil hinterher nicht
    /// mehr nachvollziehbar, und genau das ist schon passiert.
    BotBanned { status: u16, body: String },
    /// Token abgelaufen, ungültig oder ohne passenden Scope. Ein Fall für den
    /// Token-Lifecycle, ausdrücklich **kein** Bann.
    AuthError { status: u16, body: String },
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

/// Erkennt an einem Helix-Fehlerbody ein Autorisierungsproblem: abgelaufener oder
/// ungültiger Token, fehlender Scope, falscher Broadcaster.
///
/// Hat Vorrang vor [`looks_like_banned_body`]. Ein kaputter Token ist ein Fall für
/// den Token-Lifecycle mit seinem Re-Auth-Flow, kein Bann. Die Verwechslung ist
/// teuer: sie pausiert einen gesunden Partner und schickt ihm eine DM über einen
/// Bann, den es nie gab.
pub fn looks_like_auth_error(status: u16, body: &str) -> bool {
    if status == 401 {
        // 401 ist per Definition ein Authentifizierungsfehler. Twitch benutzt
        // ihn nicht, um einen Kanal-Bann zu melden.
        return true;
    }
    let body = body.to_lowercase();
    body.contains("oauth")
        || body.contains("unauthorized")
        || body.contains("invalid token")
        || body.contains("token is invalid")
        || body.contains("token expired")
        || body.contains("missing scope")
        || body.contains("scope")
        || body.contains("must match the user id")
}

/// Erkennt an einem Helix-Fehlerbody, dass der Bot im Zielkanal gebannt ist.
///
/// Twitch formuliert das je nach Endpunkt und Zeitpunkt unterschiedlich
/// ("user is banned", "is banned from the broadcaster's chat room", "blocked from
/// the broadcaster's chat room"). Eine Prüfung auf exakt `"user is banned"` würde
/// die Block-Formulierung durchlassen.
///
/// Der Aufrufer muss [`looks_like_auth_error`] zuerst prüfen: ein Auth-Fehler darf
/// niemals als Bann durchgehen.
pub fn looks_like_banned_body(body: &str) -> bool {
    let body = body.to_lowercase();
    body.contains("is banned")
        || body.contains("banned from")
        || body.contains("is blocked")
        || body.contains("blocked from")
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
                let snippet: String = body.chars().take(200).collect();
                if status == 400 && body_lower.contains("already a mod") {
                    Ok(AddModeratorOutcome::AlreadyModerator)
                } else if looks_like_auth_error(status, &body_lower) {
                    // Reihenfolge ist entscheidend: Auth schlägt Bann. Ein
                    // kaputter Token hat schon einmal als Kanal-Bann gegolten und
                    // einem gesunden Partner eine Bann-DM eingebracht.
                    tracing::info!(
                        status,
                        body = %snippet,
                        "Twitch Add-Moderator: Autorisierungsproblem, kein Bann"
                    );
                    Ok(AddModeratorOutcome::AuthError {
                        status,
                        body: snippet,
                    })
                } else if matches!(status, 400 | 403) && looks_like_banned_body(&body_lower) {
                    // Body mitloggen: die Ban-Klassifikation loest eine Reaktion
                    // mit Aussenwirkung aus und muss nachpruefbar bleiben.
                    tracing::warn!(
                        status,
                        body = %snippet,
                        "Twitch Add-Moderator: Antwort als Kanal-Bann gewertet"
                    );
                    Ok(AddModeratorOutcome::BotBanned {
                        status,
                        body: snippet,
                    })
                } else {
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

    /// `Failed` ist der Rest-Topf für alles, was weder Erfolg, noch Bann, noch
    /// Auth-Problem ist. Ein 401 gehört seit der miracleghost9-Regression
    /// ausdrücklich nicht mehr dazu, der ist `AuthError`.
    #[tokio::test]
    async fn failed_bei_sonstigem_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
            .mount(&server)
            .await;
        let client = client_with(&server).await;
        let outcome = client
            .add_channel_moderator("111", "bot", "tok")
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            AddModeratorOutcome::Failed { status: 500, .. }
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
        assert!(matches!(outcome, AddModeratorOutcome::BotBanned { .. }));
    }

    /// Regression: Twitch liefert den Ban auch als "blocked from the
    /// broadcaster's chat room" und gelegentlich als 403. Die alte Prüfung auf
    /// exakt "user is banned" hat beides als `Failed` durchgereicht — der Ban
    /// blieb dann unerkannt und niemand wurde benachrichtigt.
    #[tokio::test]
    async fn bot_banned_auch_bei_block_formulierung_und_403() {
        for (status, body) in [
            (
                400u16,
                r#"{"message":"The user specified in the user_id query parameter is blocked from the broadcaster's chat room."}"#,
            ),
            (
                403u16,
                r#"{"message":"The user is banned from the broadcaster's chat room."}"#,
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .mount(&server)
                .await;
            let client = client_with(&server).await;
            let outcome = client
                .add_channel_moderator("111", "bot", "tok")
                .await
                .unwrap();
            assert!(
                matches!(outcome, AddModeratorOutcome::BotBanned { .. }),
                "Status {status} mit Body {body} muss als Bot-Ban gelten, war: {outcome:?}"
            );
        }
    }

    /// Gegenprobe: ein normaler Fehler darf nicht als Ban durchgehen, sonst
    /// pausiert ein Netz- oder Scope-Problem ganze Partner-Kanäle.
    #[test]
    fn banned_body_erkennung_ist_nicht_uebergriffig() {
        assert!(!looks_like_banned_body(
            "missing scope channel:manage:moderators"
        ));
        assert!(!looks_like_banned_body("internal server error"));
        assert!(!looks_like_banned_body(""));
        assert!(looks_like_banned_body("The user is banned"));
    }
}

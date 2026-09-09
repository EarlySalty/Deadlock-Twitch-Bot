use serde::Deserialize;

use crate::client::{check_status_and_json, HelixClient, HelixError};

#[derive(Debug, Clone, Deserialize)]
pub struct HelixCustomReward {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub is_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct CustomRewardsResponse {
    #[serde(default)]
    data: Vec<HelixCustomReward>,
}

impl HelixClient {
    pub async fn get_custom_rewards(
        &self,
        broadcaster_id: &str,
        user_token: &str,
    ) -> Result<Vec<HelixCustomReward>, HelixError> {
        let resp = self
            .get_with_user_token("/channel_points/custom_rewards", user_token)
            .query(&[("broadcaster_id", broadcaster_id)])
            .send()
            .await?;
        let body: CustomRewardsResponse = check_status_and_json(resp).await?;
        Ok(body.data)
    }
}

#[cfg(test)]
mod tests {
    use crate::client::{HelixClient, HelixConfig};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_with(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok",
                "expires_in": 3600
            })))
            .mount(server)
            .await;
        HelixClient::new(HelixConfig {
            client_id: "cid".to_string(),
            client_secret: "sec".to_string(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn get_custom_rewards_parst_und_schickt_user_token() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/channel_points/custom_rewards"))
            .and(query_param("broadcaster_id", "42"))
            .and(header("Authorization", "Bearer streamer-tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "id": "r1", "title": "Lurker Steuer", "is_enabled": true },
                    { "id": "r2", "title": "Highlight My Message", "is_enabled": false }
                ]
            })))
            .mount(&server)
            .await;

        let rewards = client.get_custom_rewards("42", "streamer-tok").await.unwrap();
        assert_eq!(rewards.len(), 2);
        assert_eq!(rewards[0].id, "r1");
        assert_eq!(rewards[0].title, "Lurker Steuer");
        assert!(rewards[0].is_enabled);
        assert!(!rewards[1].is_enabled);
    }
}

use std::time::Duration;

use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

#[derive(serde::Serialize)]
pub struct DisconnectOutcome {
    pub deleted: usize,
    pub revocation_pending: bool,
}

#[derive(sqlx::FromRow)]
struct DeletedConnection {
    streamer_login: Option<String>,
    access_token_enc: Option<Vec<u8>>,
    refresh_token_enc: Option<Vec<u8>>,
    client_id: Option<String>,
    client_secret_enc: Option<Vec<u8>>,
    enc_version: Option<i32>,
}

pub async fn disconnect_platform(
    pool: &PgPool,
    platform: &str,
    twitch_user_id: Option<&str>,
) -> Result<DisconnectOutcome, sqlx::Error> {
    let rows = sqlx::query_as::<_, DeletedConnection>(
        "DELETE FROM social_media_platform_auth WHERE platform = $1 AND \
         (twitch_user_id = $2 OR ($2::text IS NULL AND twitch_user_id IS NULL AND streamer_login IS NULL)) \
         RETURNING streamer_login, access_token_enc, refresh_token_enc, client_id, client_secret_enc, enc_version",
    )
    .bind(platform)
    .bind(twitch_user_id)
    .fetch_all(pool)
    .await?;
    let cipher = FieldCipher::from_env().ok();
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok();
    let mut revocation_pending = false;
    for row in &rows {
        let revoked = match (&cipher, &http) {
            (Some(cipher), Some(http)) => revoke_connection(http, cipher, platform, row).await,
            _ => false,
        };
        if !revoked {
            revocation_pending = true;
            tracing::warn!(
                platform,
                "Kontozugang gelöscht, Widerruf beim Anbieter nicht bestätigt"
            );
        }
    }
    tracing::info!(
        platform,
        deleted = rows.len(),
        revocation_pending,
        "Social-Media-Verbindung getrennt"
    );
    Ok(DisconnectOutcome {
        deleted: rows.len(),
        revocation_pending,
    })
}

fn decrypt(
    cipher: &FieldCipher,
    platform: &str,
    row: &DeletedConnection,
    column: &str,
    bytes: Option<&[u8]>,
) -> Option<String> {
    cipher
        .decrypt_field(
            bytes?,
            &aad::social_media(
                column,
                platform,
                row.streamer_login.as_deref(),
                i64::from(row.enc_version.unwrap_or(1)),
            ),
        )
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn revoke_connection(
    http: &reqwest::Client,
    cipher: &FieldCipher,
    platform: &str,
    row: &DeletedConnection,
) -> bool {
    let access = decrypt(
        cipher,
        platform,
        row,
        "access_token",
        row.access_token_enc.as_deref(),
    );
    match platform {
        "youtube" => {
            let token = decrypt(
                cipher,
                platform,
                row,
                "refresh_token",
                row.refresh_token_enc.as_deref(),
            )
            .or(access);
            let Some(token) = token else {
                return false;
            };
            revoke_google(http, "https://oauth2.googleapis.com/revoke", &token).await
        }
        "tiktok" => {
            let secret = decrypt(
                cipher,
                platform,
                row,
                "client_secret",
                row.client_secret_enc.as_deref(),
            );
            let (Some(token), Some(client_key), Some(secret)) =
                (access, row.client_id.as_deref(), secret)
            else {
                return false;
            };
            revoke_tiktok(
                http,
                "https://open.tiktokapis.com/v2/oauth/revoke/",
                &token,
                client_key,
                &secret,
            )
            .await
        }
        _ => false,
    }
}

async fn revoke_google(http: &reqwest::Client, endpoint: &str, token: &str) -> bool {
    match http.post(endpoint).form(&[("token", token)]).send().await {
        Ok(response) => response.status().as_u16() == 200,
        Err(_) => false,
    }
}

async fn revoke_tiktok(
    http: &reqwest::Client,
    endpoint: &str,
    token: &str,
    client_key: &str,
    secret: &str,
) -> bool {
    let Ok(response) = http
        .post(endpoint)
        .form(&[
            ("token", token),
            ("client_key", client_key),
            ("client_secret", secret),
        ])
        .send()
        .await
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .json::<serde_json::Value>()
        .await
        .is_ok_and(|body| body.as_object().is_some_and(|object| object.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn revoke_checks_provider_success_not_just_http_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/google"))
            .and(body_string_contains("token=synthetic"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/tiktok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rejected"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"error":"invalid_request"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        assert!(revoke_google(&http, &format!("{}/google", server.uri()), "synthetic").await);
        assert!(
            revoke_tiktok(
                &http,
                &format!("{}/tiktok", server.uri()),
                "synthetic",
                "key",
                "secret"
            )
            .await
        );
        assert!(
            !revoke_tiktok(
                &http,
                &format!("{}/rejected", server.uri()),
                "synthetic",
                "key",
                "secret"
            )
            .await
        );
    }
}

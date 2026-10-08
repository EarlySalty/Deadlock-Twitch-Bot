use std::sync::Arc;

use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tb_crypto::FieldCipher;
use tb_social_media::credentials::CredentialManager;
use tb_social_media::upload_worker::youtube_uploader;
use tb_social_media::uploaders::UploadError;

fn error_class(error: &UploadError) -> &'static str {
    match error {
        UploadError::NotAuthenticated => "connection",
        UploadError::QuotaExceeded(_) => "quota",
        UploadError::Api(message)
            if message.contains("HTTP 401")
                || message.contains("HTTP 403")
                || message.contains("Token-Refresh abgelehnt") =>
        {
            "connection"
        }
        _ => "request",
    }
}

#[tokio::main]
async fn main() {
    if probe().await.is_err() {
        println!("{}", json!({"probe":"failed"}));
        std::process::exit(1);
    }
}

async fn probe() -> Result<(), Box<dyn std::error::Error>> {
    let settings = tb_config::Settings::from_env()?;
    let cipher = Arc::new(FieldCipher::from_env()?);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_read_only=on")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&settings.db.dsn)
        .await?;
    let manager = CredentialManager::new(pool.clone(), cipher);
    let accounts: Vec<(String, Vec<i64>)> = sqlx::query_as("SELECT twitch_user_id,array_agg(id ORDER BY id) FROM twitch_vod_archive_vods WHERE id=ANY($1) AND twitch_user_id IS NOT NULL GROUP BY twitch_user_id")
        .bind(vec![10_i64,11,2941,2995,2996]).fetch_all(&pool).await?;
    if accounts.is_empty() {
        return Err("historical accounts missing".into());
    }
    let mut failed = false;
    for (user, cases) in accounts {
        let Some(credentials) = manager
            .get_channel_credentials_for_id("youtube", &user)
            .await
        else {
            println!(
                "{}",
                json!({"cases":cases,"result":"connection","observed_at":chrono::Utc::now()})
            );
            failed = true;
            continue;
        };
        let client = youtube_uploader(&credentials).with_retry(1, std::time::Duration::ZERO);
        let channel = match client.upload_kanal().await {
            Ok(channel)
                if credentials
                    .platform_user_id
                    .as_deref()
                    .is_none_or(|expected| expected == channel.id) =>
            {
                channel
            }
            result => {
                let class = result.err().as_ref().map_or("channel_changed", error_class);
                println!(
                    "{}",
                    json!({"cases":cases,"result":class,"observed_at":chrono::Utc::now()})
                );
                failed = true;
                continue;
            }
        };
        let result = match client.upload_seite(&channel.playlist_id, None).await {
            Ok(page) => match client.archiv_videos(&page.ids).await {
                Ok(videos) if videos.iter().all(|video| video.channel_id == channel.id) => {
                    json!({"result":"readable","first_page_ids":page.ids.len(),"returned_videos":videos.len(),"more_pages":page.next.is_some()})
                }
                result => {
                    json!({"result":result.err().as_ref().map_or("channel_changed", error_class)})
                }
            },
            Err(error) => json!({"result":error_class(&error)}),
        };
        failed |= result["result"] != "readable";
        println!(
            "{}",
            json!({"cases":cases,"observed_at":chrono::Utc::now(),"provider":result,"historical_decisions":"pending_regular_worker"})
        );
    }
    pool.close().await;
    if failed {
        return Err("provider read failed".into());
    }
    Ok(())
}

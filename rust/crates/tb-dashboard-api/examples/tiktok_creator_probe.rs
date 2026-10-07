use std::sync::Arc;
use tb_crypto::FieldCipher;
use tb_social_media::credentials::CredentialManager;
use tb_social_media::uploaders::tiktok::TikTokUploader;

fn main() {
    let result = load_environment().and_then(|()| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(run())
    });
    if result.is_err() {
        eprintln!("TikTok read-only probe failed; no post attempted and no credentials logged.");
        std::process::exit(1);
    }
}

fn load_environment() -> Result<(), Box<dyn std::error::Error>> {
    let pid: u32 = std::env::args()
        .nth(1)
        .ok_or("service pid required")?
        .parse()?;
    let environment = std::fs::read(format!("/proc/{pid}/environ"))?;
    for entry in environment.split(|byte| *byte == 0) {
        let Ok(entry) = std::str::from_utf8(entry) else {
            continue;
        };
        let Some((key, value)) = entry.split_once('=') else {
            continue;
        };
        if matches!(
            key,
            "CREDENTIALS_DIRECTORY"
                | "INFISICAL_CONFIG_FILE"
                | "TWITCH_RUNTIME_CONFIG_FILE"
                | "TWITCH_ANALYTICS_DSN"
                | "TWITCH_INTERNAL_API_TOKEN"
                | "MASTER_BROKER_TOKEN"
                | "MAIN_BOT_INTERNAL_TOKEN"
                | "DB_MASTER_KEY_V1"
        ) {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("probe stage: runtime configuration");
    let (snapshot, _) = tb_config::runtime::start([
        "--config".into(),
        "/var/lib/deadlock-twitch/config/bot.toml".into(),
    ])?;
    eprintln!("probe stage: runtime settings");
    let settings = snapshot.runtime_settings(&|key| std::env::var(key).ok())?;
    eprintln!("probe stage: read-only database");
    let pool = tb_db::pool::connect_readonly(&settings.db).await?;
    eprintln!("probe stage: earlysalty lookup");
    let user: String = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = 'earlysalty'",
    )
    .fetch_one(&pool)
    .await?;
    eprintln!("probe stage: field cipher");
    let manager = CredentialManager::new(pool.clone(), Arc::new(FieldCipher::from_env()?));
    eprintln!("probe stage: channel connection");
    let credential = manager
        .get_channel_credentials_for_id("tiktok", &user)
        .await
        .ok_or("account unavailable")?;
    eprintln!("probe stage: creator information");
    let creator = TikTokUploader::new(credential.access_token)
        .query_creator_info()
        .await?;
    println!("{}", serde_json::to_string(&creator)?);
    let clips: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id', id, 'clip_id', clip_id, 'title', clip_title, 'game_name', game_name, 'preview_path', preview_path, 'status', status) FROM twitch_clips_social_media WHERE twitch_user_id = $1 AND discarded_at IS NULL AND preview_status = 'ready' ORDER BY created_at DESC LIMIT 10",
    ).bind(&user).fetch_all(&pool).await?;
    println!("clips={}", serde_json::to_string(&clips)?);
    pool.close().await;
    Ok(())
}

use tb_llm::video_observer::{observe, BrowserPasskey, VideoObserverConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let config: VideoObserverConfig =
        serde_json::from_slice(&std::fs::read(args.next().ok_or("config")?)?)?;
    let video = std::path::PathBuf::from(args.next().ok_or("video")?);
    let prompt = args.next().ok_or("prompt")?;
    let passkey = BrowserPasskey::parse(&std::env::var("GOOGLE_STEEL_PASSKEY")?)?;
    println!("{}", observe(&config, &video, &prompt, &passkey).await?);
    Ok(())
}

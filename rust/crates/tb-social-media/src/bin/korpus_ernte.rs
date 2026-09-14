use std::sync::Arc;

use tb_social_media::clip::helix::HelixClipSource;
use tb_transport_twitch::{HelixClient, HelixConfig};

const DEFAULT_GAME_ID: &str = "2132205352";
const DEFAULT_LIMIT: u32 = 800;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut game_id = DEFAULT_GAME_ID.to_string();
    let mut limit = DEFAULT_LIMIT;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--game-id" => {
                game_id = args
                    .next()
                    .ok_or("--game-id braucht einen Wert")?;
            }
            "--limit" => {
                let raw = args.next().ok_or("--limit braucht einen Wert")?;
                limit = raw.parse::<u32>().map_err(|e| format!("--limit ungültig: {e}"))?;
            }
            other => {
                return Err(format!("unbekanntes Argument: {other}").into());
            }
        }
    }

    let client_id =
        std::env::var("TWITCH_CLIENT_ID").map_err(|_| "TWITCH_CLIENT_ID fehlt in der Umgebung")?;
    let client_secret = std::env::var("TWITCH_CLIENT_SECRET")
        .map_err(|_| "TWITCH_CLIENT_SECRET fehlt in der Umgebung")?;

    let client = HelixClient::new(HelixConfig::new(client_id, client_secret))?;
    let source = HelixClipSource::new(Arc::new(client));

    let clips = source.fetch_top_game_clips(&game_id, limit).await?;

    let ausgabe: Vec<serde_json::Value> = clips
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "clip_id": c.clip_id,
                "url": c.clip_url,
                "views": c.view_count,
                "dauer": c.duration_seconds,
                "ersteller": c.broadcaster_name,
                "streamer_twitch_id": c.twitch_user_id,
                "created_at": c.created_at,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&ausgabe)?);
    Ok(())
}

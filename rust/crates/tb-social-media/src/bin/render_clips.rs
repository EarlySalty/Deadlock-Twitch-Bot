use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tb_social_media::batch::render_all_for_user;
use tb_social_media::clip_prep_worker::YtDlpDownloader;
use tb_social_media::video_processor::VideoProcessor;

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(twitch_user_id) = arg_value(&args, "--twitch-user-id") else {
        eprintln!("Aufruf: render_clips --twitch-user-id <id> --out <verzeichnis> [--clips-dir <verzeichnis>]");
        std::process::exit(2);
    };
    let Some(out_dir) = arg_value(&args, "--out") else {
        eprintln!("--out <verzeichnis> fehlt");
        std::process::exit(2);
    };
    let clips_dir = arg_value(&args, "--clips-dir").unwrap_or_else(|| format!("{out_dir}/_download"));

    let dsn = std::env::var("DEADLOCK_CENTRAL_DSN")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_default();
    if dsn.is_empty() {
        eprintln!("Keine Datenbank-DSN: DEADLOCK_CENTRAL_DSN (oder DATABASE_URL) setzen.");
        std::process::exit(2);
    }
    let pool = match PgPoolOptions::new().max_connections(4).connect(&dsn).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("DB-Verbindung fehlgeschlagen: {e}");
            std::process::exit(1);
        }
    };

    let vp = VideoProcessor::default();
    let downloader = Arc::new(YtDlpDownloader::new("yt-dlp"));

    println!("Rendere Clips fuer Twitch-User-ID {twitch_user_id} nach {out_dir} ...");
    let result = render_all_for_user(&pool, &vp, downloader, &twitch_user_id, &out_dir, &clips_dir).await;

    println!("Fertig: {} gerendert, {} fehlgeschlagen.", result.rendered.len(), result.failed.len());
    for path in &result.rendered {
        println!("  ok   {path}");
    }
    for (clip_id, err) in &result.failed {
        println!("  FAIL {clip_id}: {err}");
    }
    if result.rendered.is_empty() {
        std::process::exit(1);
    }
}

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tb_social_media::clip::helix::HelixClipSource;
use tb_social_media::clip_context_harvest::{
    available_stt, harvest, learn_and_store, load_clips, recommend_for_clip,
};
use tb_transport_twitch::{HelixClient, HelixConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut limit = 25_i64;
    let mut clip_id = None;
    let mut backfill = false;
    let mut force = false;
    let mut learn_only = false;
    let mut recommend_id = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => limit = args.next().ok_or("--limit braucht eine Zahl")?.parse()?,
            "--clip-id" => clip_id = Some(args.next().ok_or("--clip-id braucht eine ID")?),
            "--backfill" => backfill = true,
            "--force" => force = true,
            "--learn-only" => learn_only = true,
            "--recommend" => recommend_id = Some(args.next().ok_or("--recommend braucht eine ID")?),
            other => return Err(format!("unbekanntes Argument: {other}").into()),
        }
    }
    if !(1..=200).contains(&limit) {
        return Err("--limit muss zwischen 1 und 200 liegen".into());
    }
    let write_url = std::env::var("DATABASE_URL")?;
    let read_url = std::env::var("TB_CLIP_CONTEXT_READ_DSN").unwrap_or_else(|_| write_url.clone());
    let read_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&read_url)
        .await?;
    let write_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&write_url)
        .await?;

    if let Some(id) = recommend_id {
        match recommend_for_clip(&write_pool, &id).await? {
            Some(cut) => println!(
                "clip={id} start={} peak={} end={}",
                cut.start_s, cut.peak_s, cut.end_s
            ),
            None => println!("clip={id} recommendation=unavailable"),
        }
        return Ok(());
    }

    if backfill {
        let client = HelixClient::new(HelixConfig::new(
            std::env::var("TWITCH_CLIENT_ID")?,
            std::env::var("TWITCH_CLIENT_SECRET")?,
        ))?;
        let source = HelixClipSource::new(Arc::new(client));
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT clip_id FROM twitch_clips_social_media WHERE (vod_id IS NULL OR vod_offset_s IS NULL) AND source_kind='twitch' ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&write_pool)
        .await?;
        let mut updated = 0;
        for id in ids {
            match source.fetch_clip_by_id(&id, "").await {
                Ok(Some(clip)) if clip.vod_id.is_some() && clip.vod_offset_s.is_some() => {
                    sqlx::query("UPDATE twitch_clips_social_media SET vod_id=$2,vod_offset_s=$3 WHERE clip_id=$1 AND (vod_id IS NULL OR vod_offset_s IS NULL)")
                        .bind(&id).bind(clip.vod_id).bind(clip.vod_offset_s)
                        .execute(&write_pool).await?;
                    updated += 1;
                }
                Ok(_) => println!("backfill_unavailable={id}"),
                Err(error) => eprintln!("backfill_error={id}: {error}"),
            }
        }
        println!("backfill_updated={updated}");
        return Ok(());
    }

    if !learn_only {
        let clips = load_clips(&read_pool, limit, clip_id.as_deref()).await?;
        let stt = available_stt().await;
        println!("candidates={} stt_available={stt}", clips.len());
        for clip in clips {
            let previous: Option<(String, String)> = sqlx::query_as(
                "SELECT stt_status,visual_status FROM twitch_clip_context_runs WHERE clip_id=$1",
            )
            .bind(&clip.clip_id)
            .fetch_optional(&write_pool)
            .await?;
            if !force
                && previous.is_some_and(|(speech, visual)| {
                    visual == "sampled" && (!stt || speech == "timestamped")
                })
            {
                println!("clip={} status=already_stored", clip.clip_id);
                continue;
            }
            match harvest(&read_pool, &write_pool, &clip, stt).await {
                Ok(result) => println!(
                    "clip={} status={} seconds={} stt={} visual={}",
                    result.clip_id,
                    result.status,
                    result.seconds,
                    result.stt_status,
                    result.visual_status
                ),
                Err(error) => eprintln!("clip={} error={error}", clip.clip_id),
            }
        }
    }
    match learn_and_store(&write_pool).await? {
        Some(template) => println!(
            "template=chat_clip_v1 samples={} lead={} trail={}",
            template.sample_count, template.lead_seconds, template.trail_seconds
        ),
        None => println!("template=none"),
    }
    Ok(())
}

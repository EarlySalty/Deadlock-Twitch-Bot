include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

use std::sync::Arc;

use chrono::{DateTime, Utc};

use tb_config::{file::ConfigArguments, BotConfigSnapshot};
use tb_social_media::clip::helix::HelixClipSource;
use tb_social_media::clip_context_harvest::{
    available_stt, harvest, learn_and_store, load_clips, recommend_for_clip,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if print_build_revision() {
        return Ok(());
    }
    let config_arguments = ConfigArguments::parse(std::env::args_os().skip(1))?;
    let snapshot = BotConfigSnapshot::load(&config_arguments.path)?;
    let stt_config = snapshot.settings().stt.clone();
    if stt_config.remote_endpoint.is_some() {
        return Err("Clip-Kontext erlaubt keinen entfernten STT-Endpunkt.".into());
    }
    let mut uplink_arguments = Vec::new();
    let mut operation_arguments = Vec::new();
    let mut arguments = config_arguments.remaining.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--uplink-config" {
            uplink_arguments.push(argument);
            uplink_arguments.push(
                arguments
                    .next()
                    .ok_or("Der Pfad der Uplink-Konfiguration fehlt.")?,
            );
        } else {
            operation_arguments.push(argument);
        }
    }
    tb_config::private::load(snapshot.source())?;
    let uplink_runtime = tb_dashboard_api::uplink_config::load_shared_arguments(uplink_arguments)
        .await?
        .ok_or("Die Uplink-Konfiguration fehlt.")?;
    tb_dashboard_api::uplink_config::install(uplink_runtime)?;
    let runtime = tb_dashboard_api::uplink_config::clip_context_runtime(&snapshot).await?;
    let read_pool = runtime.read_pool;
    let write_pool = runtime.write_pool;

    let mut limit = 25_i64;
    let mut clip_id = None;
    let mut backfill = false;
    let mut force = false;
    let mut learn_only = false;
    let mut recommend_id = None;
    let mut args = operation_arguments.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.to_str().ok_or("Argument ist nicht gültiger Text.")?;
        match arg {
            "--limit" => {
                limit = args
                    .next()
                    .ok_or("--limit braucht eine Zahl")?
                    .to_str()
                    .ok_or("Limit ist ungültig.")?
                    .parse()?
            }
            "--clip-id" => {
                clip_id = Some(
                    args.next()
                        .ok_or("--clip-id braucht eine ID")?
                        .into_string()
                        .map_err(|_| "Clip-ID ist ungültig.")?,
                )
            }
            "--backfill" => backfill = true,
            "--force" => force = true,
            "--learn-only" => learn_only = true,
            "--recommend" => {
                recommend_id = Some(
                    args.next()
                        .ok_or("--recommend braucht eine ID")?
                        .into_string()
                        .map_err(|_| "Clip-ID ist ungültig.")?,
                )
            }
            other => return Err(format!("unbekanntes Argument: {other}").into()),
        }
    }
    if !(1..=200).contains(&limit) {
        return Err("--limit muss zwischen 1 und 200 liegen".into());
    }
    if let Some(id) = recommend_id {
        match recommend_for_clip(&read_pool, &id).await? {
            Some(cut) => println!(
                "clip={id} start={} peak={} end={}",
                cut.start_s, cut.peak_s, cut.end_s
            ),
            None => println!("clip={id} recommendation=unavailable"),
        }
        return Ok(());
    }

    if backfill {
        let helix = Arc::new(
            runtime
                .helix
                .ok_or("Der Twitch-Zugang für den Helix-Backfill fehlt in Infisical.")?,
        );
        let source = HelixClipSource::new(helix);
        let mut cursor: Option<(DateTime<Utc>, String)> = None;
        let mut updated = 0;
        loop {
            let ids: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
                "SELECT clip_id,created_at FROM twitch_clips_social_media
                  WHERE (vod_id IS NULL OR vod_offset_s IS NULL) AND source_kind='twitch'
                    AND ($2::timestamptz IS NULL OR (created_at,clip_id)<($2::timestamptz,$3::text))
                  ORDER BY created_at DESC,clip_id DESC LIMIT $1",
            )
            .bind(limit)
            .bind(cursor.as_ref().map(|(at, _)| at))
            .bind(cursor.as_ref().map(|(_, id)| id.as_str()))
            .fetch_all(&read_pool)
            .await?;
            if ids.is_empty() {
                break;
            }
            cursor = ids.last().map(|(id, at)| (*at, id.clone()));
            for (id, _) in ids {
                match source.fetch_clip_by_id(&id, "").await {
                    Ok(Some(clip)) if clip.vod_id.is_some() && clip.vod_offset_s.is_some() => {
                        sqlx::query("UPDATE twitch_clips_social_media SET vod_id=$2,vod_offset_s=$3,duration_seconds=COALESCE(duration_seconds,$4) WHERE clip_id=$1 AND (vod_id IS NULL OR vod_offset_s IS NULL)")
                            .bind(&id).bind(clip.vod_id).bind(clip.vod_offset_s).bind(clip.duration_seconds)
                            .execute(&write_pool).await?;
                        updated += 1;
                    }
                    Ok(_) => println!("backfill_unavailable={id}"),
                    Err(error) => eprintln!("backfill_error={id}: {error}"),
                }
            }
        }
        println!("backfill_updated={updated}");
        return Ok(());
    }

    if !learn_only {
        let stt = available_stt(&stt_config).await;
        let clips = load_clips(&read_pool, limit, clip_id.as_deref(), stt, force).await?;
        println!("candidates={} stt_available={stt}", clips.len());
        for clip in clips {
            match harvest(&read_pool, &write_pool, &clip, stt, &stt_config).await {
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

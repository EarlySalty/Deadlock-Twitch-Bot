use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use tb_config::stt::SttConfig;
use tb_engagement::transcribe::OpenAiTranscriber;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[cfg(test)]
#[path = "../../../test-support/database.rs"]
mod test_database;

use crate::clip_context::{
    clip_moment_from_start, learn_template, suggest_cut, ContextSecond, CutProposal, CutTemplate,
};

#[derive(Clone, Debug)]
pub struct ClipInput {
    pub clip_id: String,
    pub clip_url: String,
    pub streamer_login: String,
    pub requested_at: DateTime<Utc>,
    pub vod_id: String,
    pub moment_offset_s: i32,
    pub duration_s: f64,
}

#[derive(Debug)]
pub struct HarvestResult {
    pub clip_id: String,
    pub status: String,
    pub stt_status: String,
    pub visual_status: String,
    pub seconds: usize,
}

struct TemporaryMedia(PathBuf);

impl TemporaryMedia {
    async fn create() -> Result<Self, String> {
        let path =
            std::env::temp_dir().join(format!("clip-context-{}", tb_crypto::random_hex_token(12)));
        tokio::fs::create_dir(&path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for TemporaryMedia {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn command(
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("{program}: Zeitgrenze erreicht"))?
        .map_err(|e| format!("{program}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let urls = regex::Regex::new(r"https?://\S+").map_err(|e| e.to_string())?;
        let safe = urls.replace_all(&stderr, "[URL]");
        let suffix: String = safe
            .chars()
            .rev()
            .take(350)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(format!("{program} ({}) : {suffix}", output.status));
    }
    Ok(output)
}

async fn download_window(
    clip: &ClipInput,
    dir: &Path,
    start: i32,
    end: i32,
) -> Result<PathBuf, String> {
    let output = dir.join("context.mp4");
    let args = vec![
        "--no-warnings".to_owned(),
        "--no-progress".to_owned(),
        "--no-part".to_owned(),
        "-f".to_owned(),
        "best[height<=480]/best".to_owned(),
        "--download-sections".to_owned(),
        format!("*{start}-{end}"),
        "--merge-output-format".to_owned(),
        "mp4".to_owned(),
        "-o".to_owned(),
        output.display().to_string(),
        format!(
            "https://www.twitch.tv/videos/{}",
            clip.vod_id.trim_start_matches('v')
        ),
    ];
    command("yt-dlp", &args, Duration::from_secs(240)).await?;
    if !output.is_file() {
        return Err("yt-dlp hat keinen VOD-Ausschnitt geliefert".to_owned());
    }
    Ok(output)
}

fn metadata_samples(stderr: &str, key: &str, count: usize) -> Vec<Option<f64>> {
    let mut result: Vec<Option<f64>> = vec![None; count];
    let mut second = None;
    for line in stderr.lines() {
        if let Some(time) = line.split("pts_time:").nth(1) {
            second = time
                .split_whitespace()
                .next()
                .and_then(|text| text.parse::<f64>().ok())
                .map(|t| t.floor() as usize);
        }
        if let (Some(index), Some(value)) = (
            second,
            line.split(key)
                .nth(1)
                .and_then(|text| text.trim().parse::<f64>().ok()),
        ) {
            if let Some(slot) = result.get_mut(index) {
                if value.is_finite() {
                    *slot = Some(slot.map_or(value, |previous| previous.max(value)));
                }
            }
        }
    }
    result
}

async fn audio_samples(
    path: &Path,
    count: usize,
) -> Result<(Vec<Option<f64>>, Vec<Option<f64>>), String> {
    let common = [
        "-hide_banner",
        "-nostats",
        "-i",
        path.to_str().ok_or("ungültiger Dateipfad")?,
        "-vn",
        "-af",
    ];
    let mut loud_args: Vec<String> = common.iter().map(|s| s.to_string()).collect();
    loud_args.extend(
        [
            "ebur128=metadata=1:framelog=quiet,ametadata=print:key=lavfi.r128.M",
            "-f",
            "null",
            "-",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    let loud = command("ffmpeg", &loud_args, Duration::from_secs(120)).await?;
    let mut peak_args: Vec<String> = common.iter().map(|s| s.to_string()).collect();
    peak_args.extend(["aresample=48000,asetnsamples=n=48000:p=1,astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.Peak_level", "-f", "null", "-"] .iter().map(|s| s.to_string()));
    let peak = command("ffmpeg", &peak_args, Duration::from_secs(120)).await?;
    Ok((
        metadata_samples(
            &String::from_utf8_lossy(&loud.stderr),
            "lavfi.r128.M=",
            count,
        ),
        metadata_samples(
            &String::from_utf8_lossy(&peak.stderr),
            "lavfi.astats.Overall.Peak_level=",
            count,
        ),
    ))
}

async fn visual_frames(
    path: &Path,
    dir: &Path,
    count: usize,
) -> Result<(Vec<Option<f64>>, Vec<bool>), String> {
    let frames = dir.join("frames");
    tokio::fs::create_dir(&frames)
        .await
        .map_err(|e| e.to_string())?;
    let args = vec![
        "-hide_banner".to_owned(),
        "-nostats".to_owned(),
        "-i".to_owned(),
        path.display().to_string(),
        "-vf".to_owned(),
        "fps=1,signalstats,metadata=print:key=lavfi.signalstats.SATAVG".to_owned(),
        "-q:v".to_owned(),
        "3".to_owned(),
        "-y".to_owned(),
        frames.join("%04d.jpg").display().to_string(),
    ];
    let output = command("ffmpeg", &args, Duration::from_secs(120)).await?;
    let saturation = metadata_samples(
        &String::from_utf8_lossy(&output.stderr),
        "lavfi.signalstats.SATAVG=",
        count,
    );
    let args = vec![
        "-hide_banner".to_owned(),
        "-nostats".to_owned(),
        "-i".to_owned(),
        path.display().to_string(),
        "-vf".to_owned(),
        "fps=2,select=gt(scene\\,0.35),showinfo".to_owned(),
        "-f".to_owned(),
        "null".to_owned(),
        "-".to_owned(),
    ];
    let output = command("ffmpeg", &args, Duration::from_secs(120)).await?;
    let mut scenes = vec![false; count];
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        if line.contains("showinfo") {
            if let Some(second) = line
                .split("pts_time:")
                .nth(1)
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse::<f64>().ok())
            {
                if let Some(slot) = scenes.get_mut(second.floor() as usize) {
                    *slot = true;
                }
            }
        }
    }
    Ok((saturation, scenes))
}

async fn read_region(
    frame: &Path,
    region: &str,
    mode: &str,
    whitelist: Option<&str>,
) -> Result<String, String> {
    let filter = match region {
        "souls" => "crop=iw*0.074:ih*0.036:iw*0.058:ih*0.822,scale=iw*3:ih*3,negate",
        "feed" => "crop=iw*0.25:ih*0.04:iw*0.02:ih*0.173,scale=iw*3:ih*3,negate",
        _ => "crop=iw*0.40:ih*0.25:iw*0.30:ih*0.19,scale=iw*2:ih*2,negate",
    };
    let args = vec![
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
        "-i".to_owned(),
        frame.display().to_string(),
        "-vf".to_owned(),
        filter.to_owned(),
        "-frames:v".to_owned(),
        "1".to_owned(),
        "-f".to_owned(),
        "image2pipe".to_owned(),
        "-vcodec".to_owned(),
        "png".to_owned(),
        "pipe:1".to_owned(),
    ];
    let image = command("ffmpeg", &args, Duration::from_secs(12))
        .await?
        .stdout;
    let mut process = Command::new("tesseract");
    process.args(["stdin", "stdout", "--psm", mode, "-l", "eng"]);
    process.env("OMP_THREAD_LIMIT", "1");
    if let Some(allowed) = whitelist {
        process
            .arg("-c")
            .arg(format!("tessedit_char_whitelist={allowed}"));
    }
    let mut child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("tesseract: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&image).await.map_err(|e| e.to_string())?;
    }
    let result = tokio::time::timeout(Duration::from_secs(40), child.wait_with_output())
        .await
        .map_err(|_| "tesseract: Zeitgrenze erreicht".to_owned())?
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("tesseract konnte Bild nicht lesen".to_owned());
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

fn parse_souls(text: &str) -> Option<i32> {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 2 || digits.len() > 7 {
        return None;
    }
    digits.parse().ok()
}

fn speech_flags(text: &str) -> (bool, bool) {
    let lower = text.to_lowercase();
    let laughter = ["haha", "hahaha", "hehe", "lacht", "lachen", "lol", "rofl"]
        .iter()
        .any(|term| lower.contains(term));
    let exclamation = text.contains('!')
        || [
            "wow",
            "oh mein gott",
            "nein",
            "was",
            "krass",
            "scheiße",
            "holy",
        ]
        .iter()
        .any(|term| {
            if term.contains(' ') {
                lower.contains(term)
            } else {
                lower
                    .split_whitespace()
                    .any(|word| word.trim_matches(|c: char| !c.is_alphabetic()) == *term)
            }
        });
    (laughter, exclamation)
}

async fn ocr_samples(
    frames: &Path,
    timeline: &mut [ContextSecond],
    moment_s: i32,
) -> Result<(), String> {
    let mut previous_souls = None;
    let mut pending_lower_souls: Option<i32> = None;
    let mut previous_feed = String::new();
    let mut sampled = 0;
    let mut levels: Vec<_> = timeline.iter().filter_map(|row| row.lufs).collect();
    levels.sort_by(f64::total_cmp);
    let audio_floor = levels.get(levels.len() / 2).copied().unwrap_or(-40.0);
    for (index, row) in timeline.iter_mut().enumerate() {
        let focused = (moment_s - 35..=moment_s + 20).contains(&row.vod_second)
            || row.lufs.is_some_and(|value| value >= audio_floor + 8.0)
            || row.peak_dbfs.is_some_and(|value| value >= -12.0);
        let cadence = if focused { 3 } else { 9 };
        if index % cadence != 0 {
            continue;
        }
        let frame = frames.join(format!("{:04}.jpg", index + 1));
        if !frame.is_file() {
            continue;
        }
        let (souls, feed, banner) = tokio::try_join!(
            read_region(&frame, "souls", "7", Some("0123456789.,$")),
            read_region(
                &frame,
                "feed",
                "7",
                Some("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+_-"),
            ),
            read_region(&frame, "banner", "6", None),
        )?;
        row.ocr_sampled = true;
        sampled += 1;
        row.souls = parse_souls(&souls);
        if let Some(current) = row.souls {
            match previous_souls {
                Some(previous) if current - previous > 6000 => {
                    row.souls = None;
                }
                Some(previous) if current < previous - 120 => {
                    if pending_lower_souls
                        .is_some_and(|pending| (current - pending).abs() < pending / 5 + 50)
                    {
                        previous_souls = Some(current);
                        pending_lower_souls = None;
                    } else {
                        pending_lower_souls = Some(current);
                    }
                }
                Some(previous) => {
                    if (120..=6000).contains(&(current - previous)) {
                        row.soul_jump = Some(current - previous);
                    }
                    previous_souls = Some(current);
                    pending_lower_souls = None;
                }
                None => previous_souls = Some(current),
            }
        }
        let feed = feed.split_whitespace().collect::<Vec<_>>().join(" ");
        if feed.len() >= 6 && feed != previous_feed {
            row.kill_feed = Some(feed.clone());
        }
        previous_feed = feed;
        let banner = banner.to_lowercase();
        row.objective = Some(
            [
                "guardian",
                "walker",
                "shrine",
                "patron",
                "weakened",
                "urn",
                "midboss",
                "rejuvenator",
                "wachter",
            ]
            .iter()
            .any(|word| banner.contains(word)),
        );
    }
    if sampled == 0 {
        return Err("keine auswertbaren VOD-Bilder".to_owned());
    }
    Ok(())
}

async fn apply_chat(
    read_pool: &PgPool,
    clip: &ClipInput,
    start: i32,
    timeline: &mut [ContextSecond],
) -> Result<(), String> {
    let beginning =
        clip.requested_at + chrono::Duration::seconds((start - clip.moment_offset_s) as i64);
    let ending = beginning + chrono::Duration::seconds(timeline.len() as i64);
    let rows = sqlx::query(
        "SELECT FLOOR(EXTRACT(EPOCH FROM (message_ts - $2::timestamptz)))::integer AS second, COUNT(*)::integer AS total
           FROM twitch_chat_messages
          WHERE streamer_login = $1 AND message_ts >= $2 AND message_ts < $3
          GROUP BY 1",
    )
    .bind(&clip.streamer_login)
    .bind(beginning)
    .bind(ending)
    .fetch_all(read_pool)
    .await
    .map_err(|e| e.to_string())?;
    for row in rows {
        let second: i32 = row.try_get("second").map_err(|e| e.to_string())?;
        let total: i32 = row.try_get("total").map_err(|e| e.to_string())?;
        if let Some(slot) = timeline.get_mut(second as usize) {
            slot.chat_messages = total;
        }
    }
    Ok(())
}

async fn apply_speech(
    path: &Path,
    temp_dir: &Path,
    timeline: &mut [ContextSecond],
    enabled: bool,
    config: &SttConfig,
) -> String {
    if !enabled || config.remote_endpoint.is_some() || !config.host.is_loopback() {
        return "unavailable".to_owned();
    }
    let Some(stt) = clip_context_transcriber(config, temp_dir) else {
        return "unavailable".to_owned();
    };
    let transcription = tokio::time::timeout(
        Duration::from_secs(config.extraction_timeout_seconds),
        stt.transcribe_clip(path),
    )
    .await;
    match transcription {
        Ok(Ok(result)) if !result.segments.is_empty() => {
            for segment in result.segments {
                let start = segment.start_seconds.floor().max(0.0) as usize;
                let end = segment.end_seconds.ceil().max(segment.start_seconds + 1.0) as usize;
                let (laughter, exclamation) = speech_flags(&segment.text);
                for slot in timeline
                    .iter_mut()
                    .skip(start)
                    .take(end.saturating_sub(start))
                {
                    slot.speech = Some(segment.text.clone());
                    slot.laughter = Some(slot.laughter.unwrap_or(false) || laughter);
                    slot.exclamation = Some(slot.exclamation.unwrap_or(false) || exclamation);
                }
            }
            "timestamped".to_owned()
        }
        Ok(Ok(_)) => "no_segments".to_owned(),
        Ok(Err(error)) => format!("failed:{error}"),
        Err(_) => "failed:timeout".to_owned(),
    }
}

fn clip_context_transcriber(config: &SttConfig, temp_dir: &Path) -> Option<OpenAiTranscriber> {
    if config.remote_endpoint.is_some() || !config.host.is_loopback() {
        return None;
    }
    let endpoint = format!("{}/v1/audio/transcriptions", config.local_origin());
    OpenAiTranscriber::from_local_config(
        &endpoint,
        &config.model,
        Duration::from_secs(config.timeout_seconds),
    )
    .ok()
    .map(|stt| stt.with_temp_dir(temp_dir))
}

async fn stt_health(config: &SttConfig) -> bool {
    if config.remote_endpoint.is_some() || !config.host.is_loopback() {
        return false;
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(format!("{}/health", config.local_origin()))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

async fn save_timeline(
    write_pool: &PgPool,
    clip: &ClipInput,
    timeline: &[ContextSecond],
    stt_status: &str,
    visual_status: &str,
) -> Result<(), String> {
    let first = timeline.first().ok_or("leere Zeitleiste")?.vod_second;
    let last = timeline.last().ok_or("leere Zeitleiste")?.vod_second;
    let mut transaction = write_pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO twitch_clip_context_runs
            (clip_id, clip_url, streamer_login, vod_id, moment_offset_s, window_start_s, window_end_s, clip_duration_s, stt_status, visual_status)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         ON CONFLICT (clip_id) DO UPDATE SET
            clip_url=EXCLUDED.clip_url, streamer_login=EXCLUDED.streamer_login, vod_id=EXCLUDED.vod_id,
            moment_offset_s=EXCLUDED.moment_offset_s, window_start_s=EXCLUDED.window_start_s,
            window_end_s=EXCLUDED.window_end_s, clip_duration_s=EXCLUDED.clip_duration_s,
            peak_s=NULL, recommended_start_s=NULL, recommended_end_s=NULL,
            stt_status=EXCLUDED.stt_status, visual_status=EXCLUDED.visual_status, analyzed_at=now()",
    )
    .bind(&clip.clip_id).bind(&clip.clip_url).bind(&clip.streamer_login).bind(&clip.vod_id)
    .bind(clip.moment_offset_s).bind(first).bind(last + 1).bind(clip.duration_s)
    .bind(stt_status).bind(visual_status)
    .execute(&mut *transaction).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM twitch_clip_context_seconds WHERE clip_id=$1")
        .bind(&clip.clip_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    for row in timeline {
        sqlx::query(
            "INSERT INTO twitch_clip_context_seconds
                (clip_id,vod_second,lufs,peak_dbfs,speech,laughter,exclamation,kill_feed,souls,soul_jump,objective,death_screen,scene_change,chat_messages,ocr_sampled)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
        )
        .bind(&clip.clip_id).bind(row.vod_second).bind(row.lufs).bind(row.peak_dbfs)
        .bind(&row.speech).bind(row.laughter).bind(row.exclamation).bind(&row.kill_feed)
        .bind(row.souls).bind(row.soul_jump).bind(row.objective).bind(row.death_screen)
        .bind(row.scene_change).bind(row.chat_messages).bind(row.ocr_sampled)
        .execute(&mut *transaction).await.map_err(|e| e.to_string())?;
    }
    transaction.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn harvest(
    read_pool: &PgPool,
    write_pool: &PgPool,
    clip: &ClipInput,
    stt_enabled: bool,
    stt_config: &SttConfig,
) -> Result<HarvestResult, String> {
    let start = clip.moment_offset_s.saturating_sub(90).max(0);
    let end = clip.moment_offset_s.saturating_add(90);
    let temporary = TemporaryMedia::create().await?;
    let media = download_window(clip, &temporary.0, start, end).await?;
    let duration_output = command(
        "ffprobe",
        &[
            "-v".to_owned(),
            "error".to_owned(),
            "-show_entries".to_owned(),
            "format=duration".to_owned(),
            "-of".to_owned(),
            "default=noprint_wrappers=1:nokey=1".to_owned(),
            media.display().to_string(),
        ],
        Duration::from_secs(20),
    )
    .await?;
    let duration: f64 = String::from_utf8_lossy(&duration_output.stdout)
        .trim()
        .parse()
        .map_err(|_| "VOD-Ausschnitt hat keine gültige Dauer")?;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("VOD-Ausschnitt hat keine gültige Dauer".to_owned());
    }
    let count = (duration.ceil() as usize).min((end - start) as usize);
    if start + count as i32 <= clip.moment_offset_s {
        return Err("Clip-Moment liegt hinter dem verfügbaren VOD".to_owned());
    }
    let mut timeline: Vec<_> = (start..start + count as i32)
        .map(|vod_second| ContextSecond {
            vod_second,
            ..Default::default()
        })
        .collect();
    let (loudness, peaks) = audio_samples(&media, count).await?;
    for (index, row) in timeline.iter_mut().enumerate() {
        row.lufs = loudness[index];
        row.peak_dbfs = peaks[index];
    }
    let (saturation, scenes) = visual_frames(&media, &temporary.0, count).await?;
    let sats: Vec<f64> = saturation.iter().flatten().copied().collect();
    let median_sat = if sats.is_empty() {
        None
    } else {
        let mut ordered = sats;
        ordered.sort_by(f64::total_cmp);
        Some(ordered[ordered.len() / 2])
    };
    for (index, row) in timeline.iter_mut().enumerate() {
        row.death_screen = saturation[index]
            .zip(median_sat)
            .map(|(sat, median)| sat < median * 0.55);
        row.scene_change = Some(scenes[index]);
    }
    let visual_status = match ocr_samples(
        &temporary.0.join("frames"),
        &mut timeline,
        clip.moment_offset_s,
    )
    .await
    {
        Ok(()) => "sampled".to_owned(),
        Err(error) => format!("ocr_failed:{error}"),
    };
    apply_chat(read_pool, clip, start, &mut timeline).await?;
    let stt_status =
        apply_speech(&media, &temporary.0, &mut timeline, stt_enabled, stt_config).await;
    save_timeline(write_pool, clip, &timeline, &stt_status, &visual_status).await?;
    Ok(HarvestResult {
        clip_id: clip.clip_id.clone(),
        status: "stored".to_owned(),
        stt_status,
        visual_status,
        seconds: timeline.len(),
    })
}

pub async fn available_stt(config: &SttConfig) -> bool {
    stt_health(config).await
}

pub async fn load_clips(
    read_pool: &PgPool,
    limit: i64,
    clip_id: Option<&str>,
    stt_available: bool,
    force: bool,
) -> Result<Vec<ClipInput>, String> {
    let rows = sqlx::query(
        "SELECT c.clip_id, c.clip_url, c.streamer_login, c.created_at, c.vod_id, c.vod_offset_s,
                c.duration_seconds::double precision AS duration_s,
                e.requested_at AS event_requested_at,
                e.moment_offset_s AS event_moment_offset_s,
                e.vod_id AS event_vod_id
           FROM twitch_clips_social_media c
           LEFT JOIN twitch_clip_command_events e ON e.clip_id=c.clip_id
           CROSS JOIN LATERAL (
               SELECT CASE
                   WHEN c.duration_seconds >= 0.5
                    AND c.duration_seconds < 2147483647.5
                   THEN CASE
                       WHEN c.vod_offset_s::bigint
                            + trunc(c.duration_seconds)::bigint
                            + CASE WHEN c.duration_seconds - trunc(c.duration_seconds) >= 0.5 THEN 1 ELSE 0 END
                            <= 2147483647
                       THEN (c.vod_offset_s::bigint
                            + trunc(c.duration_seconds)::bigint
                            + CASE WHEN c.duration_seconds - trunc(c.duration_seconds) >= 0.5 THEN 1 ELSE 0 END
                       )::integer
                       ELSE NULL
                   END
                   ELSE NULL
               END AS fallback_moment
           ) clip_time
          WHERE c.vod_id IS NOT NULL AND c.vod_offset_s IS NOT NULL AND c.vod_offset_s >= 0
            AND c.duration_seconds IS NOT NULL AND c.duration_seconds >= 0.5
            AND c.game_id = '2132205352'
            AND c.created_at > NOW() - INTERVAL '21 days'
            AND ($2::text IS NULL OR c.clip_id=$2)
            AND ($3::boolean OR NOT EXISTS (
                SELECT 1 FROM twitch_clip_context_runs r
                 WHERE clip_time.fallback_moment IS NOT NULL
                   AND r.clip_id=c.clip_id AND r.visual_status='sampled' AND r.vod_id=c.vod_id
                   AND r.moment_offset_s = CASE
                       WHEN e.vod_id=c.vod_id AND e.moment_offset_s IS NOT NULL
                           THEN e.moment_offset_s
                       ELSE clip_time.fallback_moment
                   END
                   AND (NOT $4::boolean OR r.stt_status='timestamped')
            ))
          ORDER BY c.created_at DESC LIMIT $1",
    )
    .bind(limit)
    .bind(clip_id)
    .bind(force)
    .bind(stt_available)
    .fetch_all(read_pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut clips = Vec::with_capacity(rows.len());
    for row in rows {
        let clip_id: String = row.try_get("clip_id").map_err(|e| e.to_string())?;
        let vod_id: String = row.try_get("vod_id").map_err(|e| e.to_string())?;
        let start: i32 = row.try_get("vod_offset_s").map_err(|e| e.to_string())?;
        let duration_s: f64 = row.try_get("duration_s").map_err(|e| e.to_string())?;
        let fallback_moment = clip_moment_from_start(start, duration_s)
            .ok_or_else(|| format!("ungültiger Clip-Zeitbereich: {clip_id}"))?;
        let created_at: DateTime<Utc> = row.try_get("created_at").map_err(|e| e.to_string())?;
        let event_requested_at: Option<DateTime<Utc>> = row
            .try_get("event_requested_at")
            .map_err(|e| e.to_string())?;
        let event_moment: Option<i32> = row
            .try_get("event_moment_offset_s")
            .map_err(|e| e.to_string())?;
        let event_vod: Option<String> = row.try_get("event_vod_id").map_err(|e| e.to_string())?;
        let (requested_at, moment_offset_s) = match (event_requested_at, event_moment, event_vod) {
            (Some(requested_at), Some(moment), Some(event_vod)) if event_vod == vod_id => {
                (requested_at, moment)
            }
            (Some(requested_at), _, _) => (requested_at, fallback_moment),
            (None, _, _) => (created_at, fallback_moment),
        };
        clips.push(ClipInput {
            clip_id,
            clip_url: row.try_get("clip_url").map_err(|e| e.to_string())?,
            streamer_login: row.try_get("streamer_login").map_err(|e| e.to_string())?,
            requested_at,
            vod_id,
            moment_offset_s,
            duration_s,
        });
    }
    Ok(clips)
}

async fn load_timeline(pool: &PgPool, clip_id: &str) -> Result<Vec<ContextSecond>, String> {
    let rows = sqlx::query("SELECT vod_second,lufs,peak_dbfs,speech,laughter,exclamation,kill_feed,souls,soul_jump,objective,death_screen,scene_change,chat_messages,ocr_sampled FROM twitch_clip_context_seconds WHERE clip_id=$1 ORDER BY vod_second")
        .bind(clip_id).fetch_all(pool).await.map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|row| {
            Ok(ContextSecond {
                vod_second: row.try_get("vod_second").map_err(|e| e.to_string())?,
                lufs: row.try_get("lufs").map_err(|e| e.to_string())?,
                peak_dbfs: row.try_get("peak_dbfs").map_err(|e| e.to_string())?,
                speech: row.try_get("speech").map_err(|e| e.to_string())?,
                laughter: row.try_get("laughter").map_err(|e| e.to_string())?,
                exclamation: row.try_get("exclamation").map_err(|e| e.to_string())?,
                kill_feed: row.try_get("kill_feed").map_err(|e| e.to_string())?,
                souls: row.try_get("souls").map_err(|e| e.to_string())?,
                soul_jump: row.try_get("soul_jump").map_err(|e| e.to_string())?,
                objective: row.try_get("objective").map_err(|e| e.to_string())?,
                death_screen: row.try_get("death_screen").map_err(|e| e.to_string())?,
                scene_change: row.try_get("scene_change").map_err(|e| e.to_string())?,
                chat_messages: row.try_get("chat_messages").map_err(|e| e.to_string())?,
                ocr_sampled: row.try_get("ocr_sampled").map_err(|e| e.to_string())?,
            })
        })
        .collect()
}

pub async fn read_template(pool: &PgPool, name: &str) -> Result<Option<CutTemplate>, String> {
    let row = sqlx::query("SELECT weights,lead_seconds,trail_seconds,sample_count FROM twitch_clip_cut_templates WHERE name=$1")
        .bind(name).fetch_optional(pool).await.map_err(|e| e.to_string())?;
    row.map(|row| {
        Ok(CutTemplate {
            weights: serde_json::from_value(row.try_get("weights").map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            lead_seconds: row.try_get("lead_seconds").map_err(|e| e.to_string())?,
            trail_seconds: row.try_get("trail_seconds").map_err(|e| e.to_string())?,
            sample_count: row.try_get("sample_count").map_err(|e| e.to_string())?,
        })
    })
    .transpose()
}

pub async fn recommend_for_clip(
    pool: &PgPool,
    clip_id: &str,
) -> Result<Option<CutProposal>, String> {
    let Some(template) = read_template(pool, "chat_clip_v1").await? else {
        return Ok(None);
    };
    let moment: Option<i32> =
        sqlx::query_scalar("SELECT moment_offset_s FROM twitch_clip_context_runs WHERE clip_id=$1")
            .bind(clip_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let Some(moment) = moment else {
        return Ok(None);
    };
    let timeline = load_timeline(pool, clip_id).await?;
    Ok(suggest_cut(&timeline, moment, &template))
}

pub async fn learn_and_store(write_pool: &PgPool) -> Result<Option<CutTemplate>, String> {
    let runs = sqlx::query(
        "SELECT clip_id,moment_offset_s FROM twitch_clip_context_runs WHERE visual_status='sampled' ORDER BY clip_id",
    )
    .fetch_all(write_pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut corpus = Vec::with_capacity(runs.len());
    for run in runs {
        let id: String = run.try_get("clip_id").map_err(|e| e.to_string())?;
        let moment: i32 = run.try_get("moment_offset_s").map_err(|e| e.to_string())?;
        let timeline = load_timeline(write_pool, &id).await?;
        corpus.push((id, moment, timeline));
    }
    let training: Vec<_> = corpus
        .iter()
        .map(|(_, moment, timeline)| (*moment, timeline.clone()))
        .collect();
    let Some(template) = learn_template(&training) else {
        return Ok(None);
    };
    let weights = serde_json::to_value(&template.weights).map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO twitch_clip_cut_templates (name,weights,lead_seconds,trail_seconds,sample_count) VALUES ('chat_clip_v1',$1,$2,$3,$4) ON CONFLICT (name) DO UPDATE SET weights=EXCLUDED.weights,lead_seconds=EXCLUDED.lead_seconds,trail_seconds=EXCLUDED.trail_seconds,sample_count=EXCLUDED.sample_count,updated_at=now()")
        .bind(weights).bind(template.lead_seconds).bind(template.trail_seconds).bind(template.sample_count)
        .execute(write_pool).await.map_err(|e| e.to_string())?;
    for (id, moment, timeline) in corpus {
        if let Some(cut) = suggest_cut(&timeline, moment, &template) {
            sqlx::query("UPDATE twitch_clip_context_runs SET peak_s=$2,recommended_start_s=$3,recommended_end_s=$4 WHERE clip_id=$1")
                .bind(id).bind(cut.peak_s).bind(cut.start_s).bind(cut.end_s)
                .execute(write_pool).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(Some(template))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::str::FromStr;
    use std::time::{SystemTime, UNIX_EPOCH};

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    async fn isolated_schema(label: &str) -> Option<(PgPool, PgPool, String)> {
        let Some(dsn) = test_database::database_url() else {
            assert!(
                !test_database::required(),
                "PostgreSQL test config is required"
            );
            return None;
        };
        let options = PgConnectOptions::from_str(&dsn).expect("parse configured test DSN");
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .expect("connect configured test database");
        let schema = format!(
            "clip_ctx_{label}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .expect("create isolated test schema");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.options([("search_path", schema.as_str())]))
            .await
            .expect("connect isolated test schema");
        Some((pool, admin, schema))
    }

    async fn drop_isolated_schema(pool: PgPool, admin: PgPool, schema: String) {
        pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .expect("drop isolated test schema");
        admin.close().await;
    }

    #[test]
    fn audio_metadata_aggregates_at_second_boundaries() {
        let raw = "frame:0 pts_time:0\nlavfi.r128.M=-30.0\nframe:1 pts_time:0.9\nlavfi.r128.M=-9.0\nframe:2 pts_time:1.0\nlavfi.r128.M=-25.0\n";
        assert_eq!(
            metadata_samples(raw, "lavfi.r128.M=", 2),
            vec![Some(-9.0), Some(-25.0)]
        );
    }

    #[test]
    fn transcript_flags_are_not_always_absent() {
        assert_eq!(speech_flags("Haha, wow!"), (true, true));
        assert_eq!(speech_flags("Wir laufen zur Lane"), (false, false));
        assert_eq!(parse_souls("$3,665"), Some(3665));
    }

    #[test]
    fn overlapping_neutral_transcript_cannot_erase_reaction_flags() {
        let mut timeline = [ContextSecond::default()];
        timeline[0].laughter = Some(true);
        timeline[0].exclamation = Some(true);
        timeline[0].laughter = Some(timeline[0].laughter.unwrap_or(false) || false);
        timeline[0].exclamation = Some(timeline[0].exclamation.unwrap_or(false) || false);

        assert_eq!(timeline[0].laughter, Some(true));
        assert_eq!(timeline[0].exclamation, Some(true));
    }

    #[tokio::test]
    async fn cancellation_leaves_owned_audio_for_temporary_media_cleanup() {
        let media_owner = TemporaryMedia::create().await.unwrap();
        let ffmpeg = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let ffmpeg_path: &Path = ffmpeg.as_ref();
        tokio::fs::write(
            ffmpeg_path,
            b"#!/bin/sh\nfor last do :; done\nprintf wav > \"$last\"\nexec sleep 30\n",
        )
        .await
        .unwrap();
        std::fs::set_permissions(ffmpeg_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let transcriber = clip_context_transcriber(&SttConfig::default(), &media_owner.0)
            .expect("current local caller configuration builds")
            .with_ffmpeg_bin(ffmpeg_path.display().to_string());
        let audio = media_owner.0.join("input.mp4");
        tokio::fs::write(&audio, b"fixture").await.unwrap();

        let mut transcription = Box::pin(transcriber.transcribe_clip(&audio));
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                result = &mut transcription => panic!("fixture ffmpeg unexpectedly returned: {result:?}"),
                _ = async {
                    loop {
                        if std::fs::read_dir(&media_owner.0)
                            .unwrap()
                            .flatten()
                            .any(|entry| entry.path().join("audio.wav").exists())
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                } => {}
            }
        })
        .await
        .expect("fixture ffmpeg created the owned WAV directory");
        drop(transcription);
        let audio_dir = std::fs::read_dir(&media_owner.0)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("eng-whisper-")
            })
            .expect("transcription created its WAV directory")
            .path();
        assert!(audio_dir.join("audio.wav").exists());

        let owner_path = media_owner.0.clone();
        drop(media_owner);
        assert!(
            !owner_path.exists(),
            "outer RAII owner removes cancelled WAV directory"
        );
    }

    #[tokio::test]
    async fn failed_reharvest_clears_old_cut_and_replaces_old_evidence() {
        let Some((pool, admin, schema)) = isolated_schema("stale_cut").await else {
            return;
        };
        sqlx::raw_sql(
            "CREATE TABLE twitch_clip_context_runs (
                 clip_id text PRIMARY KEY, clip_url text NOT NULL, streamer_login text NOT NULL,
                 vod_id text NOT NULL, moment_offset_s integer NOT NULL, window_start_s integer NOT NULL,
                 window_end_s integer NOT NULL, clip_duration_s double precision NOT NULL,
                 peak_s integer, recommended_start_s integer, recommended_end_s integer,
                 stt_status text NOT NULL, visual_status text NOT NULL,
                 analyzed_at timestamptz NOT NULL DEFAULT now()
             );
             CREATE TABLE twitch_clip_context_seconds (
                 clip_id text NOT NULL, vod_second integer NOT NULL, lufs double precision,
                 peak_dbfs double precision, speech text, laughter boolean, exclamation boolean,
                 kill_feed text, souls integer, soul_jump integer, objective boolean,
                 death_screen boolean, scene_change boolean, chat_messages integer NOT NULL,
                 ocr_sampled boolean NOT NULL, PRIMARY KEY (clip_id, vod_second)
             );",
        )
        .execute(&pool)
        .await
        .expect("create reharvest schema");
        sqlx::query("INSERT INTO twitch_clip_context_runs (clip_id,clip_url,streamer_login,vod_id,moment_offset_s,window_start_s,window_end_s,clip_duration_s,peak_s,recommended_start_s,recommended_end_s,stt_status,visual_status) VALUES ('same-clip','old-url','streamer','old-vod',90,0,181,180,90,60,110,'timestamped','sampled')")
            .execute(&pool)
            .await
            .expect("seed valid previous analysis and cut");
        sqlx::query("INSERT INTO twitch_clip_context_seconds (clip_id,vod_second,lufs,peak_dbfs,speech,laughter,exclamation,kill_feed,souls,soul_jump,objective,death_screen,scene_change,chat_messages,ocr_sampled) VALUES ('same-clip',90,-4.0,-3.0,'old evidence',true,true,'eliminated',3000,150,true,false,true,8,true)")
            .execute(&pool)
            .await
            .expect("seed previous measurements");

        let clip = ClipInput {
            clip_id: "same-clip".to_owned(),
            clip_url: "new-url".to_owned(),
            streamer_login: "streamer".to_owned(),
            requested_at: chrono::Utc::now(),
            vod_id: "new-vod".to_owned(),
            moment_offset_s: 100,
            duration_s: 20.0,
        };
        save_timeline(
            &pool,
            &clip,
            &[ContextSecond {
                vod_second: 100,
                ..Default::default()
            }],
            "failed:timeout",
            "ocr_failed:fixture",
        )
        .await
        .expect("save replacement analysis atomically");
        let current: (String, String, Option<i32>, Option<i32>, Option<i32>) = sqlx::query_as(
            "SELECT vod_id,visual_status,peak_s,recommended_start_s,recommended_end_s FROM twitch_clip_context_runs WHERE clip_id='same-clip'",
        )
        .fetch_one(&pool)
        .await
        .expect("read replacement run");
        assert_eq!(current.0, "new-vod");
        assert_eq!(current.1, "ocr_failed:fixture");
        assert_eq!((current.2, current.3, current.4), (None, None, None));
        let measurements: (i32, Option<f64>, Option<String>) = sqlx::query_as(
            "SELECT vod_second,lufs,speech FROM twitch_clip_context_seconds WHERE clip_id='same-clip'",
        )
        .fetch_one(&pool)
        .await
        .expect("read replacement measurements");
        assert_eq!(measurements, (100, None, None));

        drop_isolated_schema(pool, admin, schema).await;
    }

    #[tokio::test]
    async fn completed_newest_clips_do_not_consume_eligible_batch_limit() {
        let Some((pool, admin, schema)) = isolated_schema("batch_limit").await else {
            return;
        };
        sqlx::raw_sql(
            "CREATE TABLE twitch_clips_social_media (
                 clip_id text PRIMARY KEY, clip_url text NOT NULL, streamer_login text NOT NULL,
                 created_at timestamptz NOT NULL, vod_id text, vod_offset_s integer,
                 duration_seconds double precision, game_id text
             );
             CREATE TABLE twitch_clip_command_events (
                 clip_id text PRIMARY KEY, requested_at timestamptz NOT NULL,
                 moment_offset_s integer, vod_id text
             );
             CREATE TABLE twitch_clip_context_runs (
                 clip_id text PRIMARY KEY, vod_id text NOT NULL, moment_offset_s integer NOT NULL,
                 visual_status text NOT NULL, stt_status text NOT NULL
             );
             INSERT INTO twitch_clips_social_media VALUES
                 ('newest','url-newest','streamer',now()-interval '1 minute','vod-a',100,50,'2132205352'),
                 ('second','url-second','streamer',now()-interval '2 minutes','vod-b',100,50,'2132205352'),
                 ('older-open-a','url-open-a','streamer',now()-interval '3 minutes','vod-c',100,50,'2132205352'),
                 ('older-open-b','url-open-b','streamer',now()-interval '4 minutes','vod-d',100,50,'2132205352');
             INSERT INTO twitch_clip_command_events VALUES
                 ('newest',now(),'175','vod-a'),
                 ('second',now(),'999','different-vod');
             INSERT INTO twitch_clips_social_media VALUES
                 ('half-round','url-half','streamer',now()-interval '5 minutes','vod-half',100,30.5,'2132205352'),
                 ('nan-duration','url-nan','streamer',now()-interval '6 minutes','vod-nan',100,'NaN'::double precision,'2132205352'),
                 ('infinite-duration','url-infinity','streamer',now()-interval '7 minutes','vod-infinity',100,'Infinity'::double precision,'2132205352'),
                 ('overflow-moment','url-overflow','streamer',now()-interval '8 minutes','vod-overflow',2147483640,20,'2132205352');
             INSERT INTO twitch_clip_command_events VALUES
                 ('nan-duration',now(),'999','vod-nan'),
                 ('infinite-duration',now(),'999','vod-infinity'),
                 ('overflow-moment',now(),'999','vod-overflow');
             INSERT INTO twitch_clip_context_runs VALUES
                 ('newest','vod-a',175,'sampled','timestamped'),
                 ('second','vod-b',150,'sampled','unavailable'),
                 ('half-round','vod-half',131,'sampled','unavailable'),
                 ('nan-duration','vod-nan',999,'sampled','unavailable'),
                 ('infinite-duration','vod-infinity',999,'sampled','unavailable'),
                 ('overflow-moment','vod-overflow',999,'sampled','unavailable');",
        )
        .execute(&pool)
        .await
        .expect("seed completed and open clip candidates");

        let unavailable = load_clips(&pool, 2, None, false, false)
            .await
            .expect("select only eligible clips before applying limit");
        assert_eq!(
            unavailable
                .iter()
                .map(|clip| clip.clip_id.as_str())
                .collect::<Vec<_>>(),
            ["older-open-a", "older-open-b"]
        );
        let available = load_clips(&pool, 2, None, true, false)
            .await
            .expect("speech-incomplete clip remains eligible when STT is available");
        assert_eq!(
            available
                .iter()
                .map(|clip| clip.clip_id.as_str())
                .collect::<Vec<_>>(),
            ["second", "older-open-a"]
        );
        assert!(load_clips(&pool, 20, Some("half-round"), false, false)
            .await
            .expect("the 30.5-second half-up match is complete")
            .is_empty());
        let forced_half = load_clips(&pool, 20, Some("half-round"), false, true)
            .await
            .expect("force includes the completed half-up boundary");
        assert_eq!(forced_half[0].moment_offset_s, 131);
        assert_eq!(
            load_clips(&pool, 20, Some("nan-duration"), false, false)
                .await
                .expect_err("NaN remains in Rust's invalid-range path"),
            "ungültiger Clip-Zeitbereich: nan-duration"
        );
        for clip_id in ["infinite-duration", "overflow-moment"] {
            assert_eq!(
                load_clips(&pool, 20, Some(clip_id), false, false)
                    .await
                    .expect_err("invalid clip range remains in Rust's validation path"),
                format!("ungültiger Clip-Zeitbereich: {clip_id}")
            );
        }
        let forced = load_clips(&pool, 1, None, true, true)
            .await
            .expect("force includes completed candidates");
        assert_eq!(forced[0].clip_id, "newest");

        drop_isolated_schema(pool, admin, schema).await;
    }
}

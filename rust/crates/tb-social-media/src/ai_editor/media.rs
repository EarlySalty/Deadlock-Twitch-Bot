use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::layout::{StreamerLayout, TARGET_HEIGHT, TARGET_WIDTH};
use crate::video_processor::{bounded_ffmpeg_command, RenderResourceLimits, VideoProcessor};

use super::model::ResourceLimits;
use super::validation::{EditorError, ErrorCode, ValidatedPlan};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaInfo {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}

#[derive(Debug, Deserialize)]
struct Probe {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: String,
    codec_name: Option<String>,
    pix_fmt: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: String,
}

#[derive(Debug)]
struct OwnedDirectory(PathBuf);

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, "Could not remove AI editor temporary media");
            }
        }
    }
}

#[derive(Debug)]
pub struct FrameBatch {
    owned: OwnedDirectory,
    timestamps_ms: Vec<u64>,
}

impl FrameBatch {
    pub fn frames(&self) -> Vec<(u64, PathBuf)> {
        self.timestamps_ms
            .iter()
            .enumerate()
            .map(|(i, at)| (*at, self.owned.0.join(format!("frame-{i:04}.jpg"))))
            .collect()
    }
}

#[derive(Debug)]
pub struct PreparedMedia {
    owned: OwnedDirectory,
    source_info: MediaInfo,
    expected_duration_ms: u64,
}

impl PreparedMedia {
    pub fn path(&self) -> PathBuf {
        self.owned.0.join("prepared.mp4")
    }

    pub fn source_info(&self) -> &MediaInfo {
        &self.source_info
    }

    pub fn expected_duration_ms(&self) -> u64 {
        self.expected_duration_ms
    }
}

fn check_resources(limits: &ResourceLimits) -> Result<(), EditorError> {
    if limits.max_source_bytes == 0
        || limits.max_output_bytes == 0
        || limits.max_output_bytes > i64::MAX as u64
        || limits.max_probe_bytes == 0
        || limits.max_media_duration_ms == 0
        || limits.max_media_width < TARGET_WIDTH as u32
        || limits.max_media_height < TARGET_HEIGHT as u32
        || limits.max_frames == 0
        || limits.max_frames > 256
        || !(2..=4096).contains(&limits.frame_width)
        || !limits.frame_width.is_multiple_of(2)
        || limits.max_frame_bytes == 0
        || limits.max_frame_bytes > i64::MAX as u64
        || limits.max_ass_bytes == 0
        || limits.process_deadline_ms == 0
        || limits.total_frames_deadline_ms == 0
        || limits.max_duration_drift_ms == 0
        || !(1..=8).contains(&limits.encoder_threads)
    {
        return Err(EditorError::new(ErrorCode::InvalidLimits));
    }
    Ok(())
}

fn source_path(root: &Path, path: &Path, max_bytes: u64) -> Result<PathBuf, EditorError> {
    let root =
        std::fs::canonicalize(root).map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    let path =
        std::fs::canonicalize(path).map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    let metadata =
        std::fs::metadata(&path).map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    if !path.starts_with(root)
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > max_bytes
    {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    Ok(path)
}

fn make_temp_directory(root: &Path) -> Result<OwnedDirectory, EditorError> {
    let root =
        std::fs::canonicalize(root).map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    if !root.is_dir() {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    let path = root.join(format!("ai-editor-{}", tb_crypto::random_hex_token(16)));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&path)
        .map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    Ok(OwnedDirectory(path))
}

async fn run_status(mut command: Command, deadline_ms: u64) -> Result<(), EditorError> {
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| EditorError::new(ErrorCode::ProcessFailed))?;
    let status = tokio::time::timeout(Duration::from_millis(deadline_ms), child.wait())
        .await
        .map_err(|_| EditorError::new(ErrorCode::ProcessDeadline))?
        .map_err(|_| EditorError::new(ErrorCode::ProcessFailed))?;
    if status.success() {
        Ok(())
    } else {
        Err(EditorError::new(ErrorCode::ProcessFailed))
    }
}

async fn probe(path: &Path, limits: &ResourceLimits) -> Result<(MediaInfo, Probe), EditorError> {
    let mut command = Command::new("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,codec_name,pix_fmt,width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| EditorError::new(ErrorCode::ProcessFailed))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EditorError::new(ErrorCode::ProcessFailed))?;
    let max_probe_bytes = limits
        .max_probe_bytes
        .checked_add(1)
        .ok_or_else(|| EditorError::new(ErrorCode::InvalidLimits))?;
    let (data, status) =
        tokio::time::timeout(Duration::from_millis(limits.process_deadline_ms), async {
            let mut data = Vec::new();
            stdout
                .take(max_probe_bytes as u64)
                .read_to_end(&mut data)
                .await
                .map_err(|_| EditorError::new(ErrorCode::ProcessFailed))?;
            if data.len() > limits.max_probe_bytes {
                return Err(EditorError::new(ErrorCode::MediaTooLarge));
            }
            let status = child
                .wait()
                .await
                .map_err(|_| EditorError::new(ErrorCode::ProcessFailed))?;
            Ok((data, status))
        })
        .await
        .map_err(|_| EditorError::new(ErrorCode::ProcessDeadline))??;
    if !status.success() {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    let probe: Probe =
        serde_json::from_slice(&data).map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    let duration_seconds: f64 = probe
        .format
        .duration
        .parse()
        .map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    let duration_ms = duration_seconds * 1000.0;
    if !duration_ms.is_finite() || duration_ms < 1.0 || duration_ms > u64::MAX as f64 {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    let video = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video")
        .ok_or_else(|| EditorError::new(ErrorCode::InvalidMedia))?;
    let (Some(width), Some(height)) = (video.width, video.height) else {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    };
    if width == 0
        || height == 0
        || width > limits.max_media_width
        || height > limits.max_media_height
        || duration_ms > limits.max_media_duration_ms as f64
    {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    let info = MediaInfo {
        duration_ms: duration_ms.round() as u64,
        width,
        height,
        has_audio: probe
            .streams
            .iter()
            .any(|stream| stream.codec_type == "audio"),
    };
    Ok((info, probe))
}

async fn probe_path(path: &Path, limits: &ResourceLimits) -> Result<MediaInfo, EditorError> {
    probe(path, limits).await.map(|(info, _)| info)
}

fn check_output(path: &Path, max_bytes: u64) -> Result<(), EditorError> {
    let meta = std::fs::metadata(path).map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > max_bytes {
        return Err(EditorError::new(ErrorCode::MediaTooLarge));
    }
    Ok(())
}

pub async fn inspect_media(
    source_root: &Path,
    input: &Path,
    limits: &ResourceLimits,
) -> Result<MediaInfo, EditorError> {
    check_resources(limits)?;
    let path = source_path(source_root, input, limits.max_source_bytes)?;
    probe_path(&path, limits).await
}

pub const MAX_CONTACT_SHEET_FPS: u32 = 16;
const CONTACT_SHEET_WINDOW_MS: u64 = 1000;
const MAX_CONTACT_SHEET_WIDTH: u32 = 2576;
const CONTACT_SHEET_QUALITIES: [u8; 5] = [4, 7, 11, 16, 23];

pub fn contact_sheet_grid(fps: u32) -> (u32, u32) {
    let fps = fps.clamp(1, MAX_CONTACT_SHEET_FPS);
    let columns = (1..=fps).find(|side| side * side >= fps).unwrap_or(fps);
    (columns, fps.div_ceil(columns))
}

fn contact_sheet_windows(duration_ms: u64, fps: u32) -> u64 {
    let interval_ms = CONTACT_SHEET_WINDOW_MS.div_ceil(u64::from(fps.max(1)));
    let full = duration_ms / CONTACT_SHEET_WINDOW_MS;
    let tail = duration_ms % CONTACT_SHEET_WINDOW_MS;
    (full + u64::from(tail >= interval_ms)).max(1)
}

fn contact_sheet_starts(duration_ms: u64, max_sheets: usize, fps: u32) -> Vec<u64> {
    let seconds = contact_sheet_windows(duration_ms, fps);
    let count = seconds.min(max_sheets as u64);
    let last_start = duration_ms.saturating_sub(CONTACT_SHEET_WINDOW_MS);
    (0..count)
        .map(|index| {
            if count == seconds {
                index * CONTACT_SHEET_WINDOW_MS
            } else {
                let center =
                    ((index as u128 * 2 + 1) * duration_ms as u128 / (count as u128 * 2)) as u64;
                center
                    .saturating_sub(CONTACT_SHEET_WINDOW_MS / 2)
                    .min(last_start)
            }
        })
        .collect()
}

fn contact_sheet_tile_width(fps: u32, frame_width: u32) -> u32 {
    let (columns, _) = contact_sheet_grid(fps);
    (frame_width.min(MAX_CONTACT_SHEET_WIDTH / columns) & !1).max(2)
}

fn contact_sheet_padding(window_ms: u64) -> String {
    let pad_ms = CONTACT_SHEET_WINDOW_MS.saturating_sub(window_ms);
    if pad_ms == 0 {
        return String::new();
    }
    format!(
        "tpad=stop_mode=clone:stop_duration={}.{:03},",
        pad_ms / 1000,
        pad_ms % 1000
    )
}

fn contact_sheet_filter(fps: u32, frame_width: u32, start_ms: u64, window_ms: u64) -> String {
    let (columns, rows) = contact_sheet_grid(fps);
    let tile_width = contact_sheet_tile_width(fps, frame_width);
    let font_size = (tile_width / 36).max(12);
    format!(
        "{}fps={fps},scale={tile_width}:-2,drawtext=text='%{{pts\\:hms\\:{}}}':x=4:y=(h-th)/2:fontsize={font_size}:fontcolor=white:box=1:boxcolor=black@0.7:boxborderw=3,tile={columns}x{rows}",
        contact_sheet_padding(window_ms),
        format_time(start_ms)
    )
}

async fn write_contact_sheet(
    path: &Path,
    output: &Path,
    start_ms: u64,
    fps: u32,
    limits: &ResourceLimits,
    duration_ms: u64,
) -> Result<(), EditorError> {
    let window_ms = CONTACT_SHEET_WINDOW_MS.min(duration_ms.saturating_sub(start_ms).max(1));
    let mut failure = EditorError::new(ErrorCode::ProcessFailed);
    for quality in CONTACT_SHEET_QUALITIES {
        let mut command = bounded_ffmpeg_command("ffmpeg", limits.max_frame_bytes);
        command
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-filter_threads", &limits.encoder_threads.to_string()])
            .args(["-threads", &limits.encoder_threads.to_string()])
            .arg("-ss")
            .arg(format_time(start_ms))
            .arg("-t")
            .arg(format_time(window_ms))
            .arg("-i")
            .arg(path)
            .args(["-an", "-vf"])
            .arg(contact_sheet_filter(
                fps,
                limits.frame_width,
                start_ms,
                window_ms,
            ))
            .args(["-frames:v", "1", "-q:v"])
            .arg(quality.to_string())
            .arg("-threads:v")
            .arg(limits.encoder_threads.to_string())
            .args(["-y"])
            .arg(output);
        match run_status(command, limits.process_deadline_ms).await {
            Ok(()) => match check_output(output, limits.max_frame_bytes) {
                Ok(()) => return Ok(()),
                Err(error) => failure = error,
            },
            Err(error) if error.code == ErrorCode::ProcessDeadline => return Err(error),
            Err(error) => failure = error,
        }
    }
    Err(failure)
}

async fn has_jpeg_magic(output: &Path) -> Result<bool, EditorError> {
    let mut image = tokio::fs::File::open(output)
        .await
        .map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    let mut magic = [0; 2];
    image
        .read_exact(&mut magic)
        .await
        .map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    Ok(magic == [0xff, 0xd8])
}

pub async fn extract_frames(
    source_root: &Path,
    input: &Path,
    workspace_root: &Path,
    limits: &ResourceLimits,
    fps: u32,
) -> Result<FrameBatch, EditorError> {
    check_resources(limits)?;
    if !(1..=MAX_CONTACT_SHEET_FPS).contains(&fps) {
        return Err(EditorError::new(ErrorCode::InvalidLimits));
    }
    let path = source_path(source_root, input, limits.max_source_bytes)?;
    let info = probe_path(&path, limits).await?;
    let owned = make_temp_directory(workspace_root)?;
    let starts = contact_sheet_starts(info.duration_ms, limits.max_frames, fps);
    tokio::time::timeout(
        Duration::from_millis(limits.total_frames_deadline_ms),
        async {
            for (index, start_ms) in starts.iter().enumerate() {
                let output = owned.0.join(format!("frame-{index:04}.jpg"));
                write_contact_sheet(&path, &output, *start_ms, fps, limits, info.duration_ms)
                    .await?;
                if !has_jpeg_magic(&output).await? {
                    return Err(EditorError::new(ErrorCode::InvalidMedia));
                }
            }
            Ok(())
        },
    )
    .await
    .map_err(|_| EditorError::new(ErrorCode::ProcessDeadline))??;
    Ok(FrameBatch {
        owned,
        timestamps_ms: starts,
    })
}

pub const AUDIO_LEVEL_STEP_MS: u64 = 250;
const AUDIO_LEVEL_SAMPLE_RATE: u64 = 8000;
const AUDIO_LEVEL_FLOOR_DBFS: f64 = -90.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioLevels {
    pub step_ms: u64,
    pub rms_dbfs: Vec<i16>,
}

impl AudioLevels {
    pub fn capped(self, max_levels: usize) -> Self {
        if max_levels == 0 || self.rms_dbfs.len() <= max_levels {
            return self;
        }
        let factor = self.rms_dbfs.len().div_ceil(max_levels);
        Self {
            step_ms: self.step_ms.saturating_mul(factor as u64),
            rms_dbfs: self
                .rms_dbfs
                .chunks(factor)
                .filter_map(|chunk| chunk.iter().copied().max())
                .collect(),
        }
    }
}

fn rms_dbfs(samples: &[u8]) -> Vec<i16> {
    let step_bytes = (AUDIO_LEVEL_SAMPLE_RATE * AUDIO_LEVEL_STEP_MS / 1000 * 2) as usize;
    samples
        .chunks(step_bytes)
        .map(|chunk| {
            let values = chunk
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| f64::from(i16::from_le_bytes(*pair)) / 32768.0);
            let (sum, count) = values.fold((0.0, 0usize), |(sum, count), value| {
                (sum + value * value, count + 1)
            });
            let rms = if count == 0 {
                0.0
            } else {
                (sum / count as f64).sqrt()
            };
            let level = if rms > 0.0 {
                (20.0 * rms.log10()).max(AUDIO_LEVEL_FLOOR_DBFS)
            } else {
                AUDIO_LEVEL_FLOOR_DBFS
            };
            level.round() as i16
        })
        .collect()
}

pub async fn audio_levels(
    source_root: &Path,
    input: &Path,
    workspace_root: &Path,
    limits: &ResourceLimits,
    filter: &str,
) -> Result<AudioLevels, EditorError> {
    check_resources(limits)?;
    let path = source_path(source_root, input, limits.max_source_bytes)?;
    let owned = make_temp_directory(workspace_root)?;
    let output = owned.0.join("levels.pcm");
    let mut command = bounded_ffmpeg_command("ffmpeg", limits.max_output_bytes);
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(["-threads", &limits.encoder_threads.to_string()])
        .arg("-i")
        .arg(&path)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(AUDIO_LEVEL_SAMPLE_RATE.to_string());
    if !filter.trim().is_empty() {
        command.arg("-af").arg(filter);
    }
    command
        .args(["-f", "s16le", "-c:a", "pcm_s16le", "-y"])
        .arg(&output);
    run_status(command, limits.process_deadline_ms).await?;
    check_output(&output, limits.max_output_bytes)?;
    let samples = tokio::fs::read(&output)
        .await
        .map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
    Ok(AudioLevels {
        step_ms: AUDIO_LEVEL_STEP_MS,
        rms_dbfs: rms_dbfs(&samples),
    })
}

fn format_time(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

fn cut_filter(plan: &ValidatedPlan, has_audio: bool) -> String {
    let mut steps = Vec::new();
    for (index, interval) in plan.intervals().iter().enumerate() {
        let start = format_time(interval.start_ms);
        let end = format_time(interval.end_ms);
        steps.push(format!(
            "[{index}:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[v{index}]"
        ));
        if has_audio {
            steps.push(format!(
                "[{index}:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[a{index}]"
            ));
        }
    }
    let mut inputs = String::new();
    for index in 0..plan.intervals().len() {
        inputs.push_str(&format!("[v{index}]"));
        if has_audio {
            inputs.push_str(&format!("[a{index}]"));
        }
    }
    let out = if has_audio { "[vout][aout]" } else { "[vout]" };
    steps.push(format!(
        "{inputs}concat=n={}:v=1:a={out_audio}{out}",
        plan.intervals().len(),
        out_audio = u8::from(has_audio)
    ));
    steps.join(";")
}

pub async fn prepare_intervals(
    source_root: &Path,
    input: &Path,
    workspace_root: &Path,
    plan: &ValidatedPlan,
    limits: &ResourceLimits,
) -> Result<PreparedMedia, EditorError> {
    check_resources(limits)?;
    let path = source_path(source_root, input, limits.max_source_bytes)?;
    let source_info = probe_path(&path, limits).await?;
    if source_info.duration_ms.abs_diff(plan.source_duration_ms()) > limits.max_duration_drift_ms {
        return Err(EditorError::new(ErrorCode::InvalidDuration));
    }
    if plan
        .intervals()
        .iter()
        .any(|interval| interval.end_ms > source_info.duration_ms)
    {
        return Err(EditorError::new(ErrorCode::InvalidInterval));
    }
    let owned = make_temp_directory(workspace_root)?;
    let output = owned.0.join("prepared.mp4");
    let mut command = bounded_ffmpeg_command("ffmpeg", limits.max_output_bytes);
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(["-filter_threads", &limits.encoder_threads.to_string()])
        .args([
            "-filter_complex_threads",
            &limits.encoder_threads.to_string(),
        ]);
    for _ in plan.intervals() {
        command
            .args(["-threads", &limits.encoder_threads.to_string()])
            .arg("-i")
            .arg(&path);
    }
    command.args([
        "-filter_complex",
        &cut_filter(plan, source_info.has_audio),
        "-map",
        "[vout]",
    ]);
    if source_info.has_audio {
        command.args(["-map", "[aout]"]);
    }
    command
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-pix_fmt",
            "yuv420p",
            "-threads:v",
        ])
        .arg(limits.encoder_threads.to_string());
    if source_info.has_audio {
        command.args(["-c:a", "aac"]);
    }
    command.args(["-movflags", "+faststart", "-y"]).arg(&output);
    run_status(command, limits.process_deadline_ms).await?;
    check_output(&output, limits.max_output_bytes)?;
    let rendered = probe_path(&output, limits).await?;
    if rendered.duration_ms.abs_diff(plan.output_duration_ms()) > limits.max_duration_drift_ms
        || rendered.has_audio != source_info.has_audio
        || rendered.width != source_info.width
        || rendered.height != source_info.height
    {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    Ok(PreparedMedia {
        owned,
        source_info,
        expected_duration_ms: plan.output_duration_ms(),
    })
}

pub async fn render_prepared(
    prepared: &PreparedMedia,
    layout: Option<&StreamerLayout>,
    ass: &str,
    limits: &ResourceLimits,
) -> Result<PathBuf, EditorError> {
    check_resources(limits)?;
    if ass.len() > limits.max_ass_bytes {
        return Err(EditorError::new(ErrorCode::MediaTooLarge));
    }
    let output = prepared.owned.0.join("rendered.mp4");
    if output.exists() {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    let max_duration = prepared
        .expected_duration_ms
        .checked_add(999)
        .ok_or_else(|| EditorError::new(ErrorCode::InvalidLimits))?
        / 1000;
    let max_duration =
        i64::try_from(max_duration).map_err(|_| EditorError::new(ErrorCode::InvalidLimits))?;
    let processor = VideoProcessor::default();
    tokio::time::timeout(
        Duration::from_millis(limits.process_deadline_ms),
        processor.render_branded_limited(
            &prepared.path().to_string_lossy(),
            &output.to_string_lossy(),
            max_duration,
            layout,
            ass,
            RenderResourceLimits {
                encoder_threads: limits.encoder_threads,
                max_output_bytes: limits.max_output_bytes,
            },
        ),
    )
    .await
    .map_err(|_| EditorError::new(ErrorCode::ProcessDeadline))?
    .map_err(|_| EditorError::new(ErrorCode::ProcessFailed))?;
    check_output(&output, limits.max_output_bytes)?;
    let info = probe_path(&output, limits).await?;
    if info.width != TARGET_WIDTH as u32
        || info.height != TARGET_HEIGHT as u32
        || info.has_audio != prepared.source_info.has_audio
        || info.duration_ms.abs_diff(prepared.expected_duration_ms) > limits.max_duration_drift_ms
    {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    Ok(output)
}

pub(super) async fn render_original_preview(
    root: &Path,
    input: &Path,
    workspace: &Path,
    source: &MediaInfo,
    limits: &ResourceLimits,
    config: &super::runtime_config::OriginalPreviewConfig,
) -> Result<PathBuf, EditorError> {
    check_resources(limits)?;
    let path = source_path(root, input, limits.max_source_bytes)?;
    let output = workspace.join("original-preview.mp4");
    let mut command = bounded_ffmpeg_command("nice", config.max_output_bytes);
    command
        .args(["-n", &config.nice.to_string(), "ffmpeg"])
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-xerror"])
        .args(["-threads", &config.encoder_threads.to_string()])
        .arg("-i")
        .arg(path)
        .args(["-map", "0:v:0", "-map", "0:a:0?", "-map_metadata", "-1"])
        .args([
            "-filter_threads",
            &config.encoder_threads.to_string(),
            "-vf",
        ])
        .arg(format!(
            "scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1",
            config.max_width, config.max_height
        ))
        .args([
            "-c:v",
            "libx264",
            "-preset",
            &config.preset,
            "-crf",
            &config.crf.to_string(),
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-threads:v", &config.encoder_threads.to_string()])
        .args(["-c:a", "aac", "-movflags", "+faststart", "-n"])
        .arg(&output);
    run_status(command, config.process_deadline_ms).await?;
    validate_original_preview(workspace, &output, source, limits, config).await?;
    Ok(output)
}

pub(super) async fn validate_original_preview(
    root: &Path,
    output: &Path,
    source: &MediaInfo,
    limits: &ResourceLimits,
    config: &super::runtime_config::OriginalPreviewConfig,
) -> Result<MediaInfo, EditorError> {
    let path = source_path(root, output, config.max_output_bytes)?;
    let (info, codecs) = probe(&path, limits).await?;
    if info.width > config.max_width
        || info.height > config.max_height
        || !info.width.is_multiple_of(2)
        || !info.height.is_multiple_of(2)
        || (u64::from(info.width) * u64::from(source.height))
            .abs_diff(u64::from(info.height) * u64::from(source.width))
            > u64::from(source.width.max(source.height)) * 2
        || info.has_audio != source.has_audio
        || info.duration_ms.abs_diff(source.duration_ms) > limits.max_duration_drift_ms
        || codecs
            .streams
            .iter()
            .any(|stream| match stream.codec_type.as_str() {
                "video" => {
                    stream.codec_name.as_deref() != Some("h264")
                        || stream.pix_fmt.as_deref() != Some("yuv420p")
                }
                "audio" => stream.codec_name.as_deref() != Some("aac"),
                _ => true,
            })
    {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    let mut command = bounded_ffmpeg_command("nice", config.max_output_bytes);
    command
        .args(["-n", &config.nice.to_string(), "ffmpeg"])
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-xerror",
            "-threads",
        ])
        .arg(config.encoder_threads.to_string())
        .arg("-i")
        .arg(path)
        .args(["-map", "0:v:0", "-map", "0:a:0?", "-f", "null", "-"]);
    run_status(command, config.process_deadline_ms).await?;
    Ok(info)
}

pub(super) async fn validate_output(
    root: &Path,
    output: &Path,
    expected_duration_ms: u64,
    source: &MediaInfo,
    limits: &ResourceLimits,
) -> Result<MediaInfo, EditorError> {
    check_resources(limits)?;
    let path = source_path(root, output, limits.max_output_bytes)?;
    let info = probe_path(&path, limits).await?;
    if info.width != TARGET_WIDTH as u32
        || info.height != TARGET_HEIGHT as u32
        || info.has_audio != source.has_audio
        || info.duration_ms.abs_diff(expected_duration_ms) > limits.max_duration_drift_ms
    {
        return Err(EditorError::new(ErrorCode::InvalidMedia));
    }
    let mut command = bounded_ffmpeg_command("ffmpeg", limits.max_output_bytes);
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-xerror",
            "-threads",
        ])
        .arg(limits.encoder_threads.to_string())
        .arg("-i")
        .arg(path)
        .args(["-map", "0:v:0", "-map", "0:a?", "-f", "null", "-"]);
    run_status(command, limits.process_deadline_ms).await?;
    Ok(info)
}

pub(super) fn framing_reasons(layout: Option<&StreamerLayout>, source: &MediaInfo) -> Vec<String> {
    let Some(layout) = layout else {
        return Vec::new();
    };
    if !layout.cam_enabled || layout.mode.trim().eq_ignore_ascii_case("blur_pad") {
        return Vec::new();
    }
    let width = i64::from(source.width);
    let height = i64::from(source.height);
    let outside = |crop: &crate::layout::LayoutBox| {
        crop.x < 0 || crop.y < 0 || crop.x + crop.w > width || crop.y + crop.h > height
    };
    if layout.source.width != width
        || layout.source.height != height
        || outside(&layout.game_crop)
        || outside(&layout.cam_crop)
    {
        return vec!["Das gespeicherte Layout passt nicht zur Auflösung dieses Clips. Bitte prüfe Bildausschnitt und Facecam.".into()];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_editor::{validate_plan, EditPlan, KeptInterval, LayoutIntent, PlanLimits};

    fn limits() -> ResourceLimits {
        ResourceLimits {
            max_source_bytes: 5_000_000,
            max_output_bytes: 5_000_000,
            max_probe_bytes: 8192,
            max_media_duration_ms: 5000,
            max_media_width: 1920,
            max_media_height: 1920,
            max_frames: 3,
            frame_width: 160,
            max_frame_bytes: 500_000,
            max_ass_bytes: 8192,
            process_deadline_ms: 15_000,
            total_frames_deadline_ms: 40_000,
            max_duration_drift_ms: 400,
            encoder_threads: 1,
        }
    }

    #[test]
    fn contact_sheets_cover_one_second_each_and_scale_with_fps() {
        assert_eq!(contact_sheet_grid(4), (2, 2));
        assert_eq!(contact_sheet_grid(8), (3, 3));
        assert_eq!(contact_sheet_tile_width(4, 640), 640);
        assert_eq!(contact_sheet_tile_width(8, 640), 640);
        assert_eq!(contact_sheet_tile_width(4, 960), 960);
        assert_eq!(contact_sheet_tile_width(8, 960), 858);
        assert_eq!(
            contact_sheet_starts(3_500, 4, 4),
            vec![0, 1_000, 2_000, 3_000]
        );
        assert_eq!(
            contact_sheet_starts(60_000, 4, 4),
            vec![7_000, 22_000, 37_000, 52_000]
        );
    }

    #[test]
    fn short_contact_sheet_windows_are_padded_to_a_full_second() {
        assert_eq!(contact_sheet_padding(1_000), "");
        assert_eq!(
            contact_sheet_padding(5),
            "tpad=stop_mode=clone:stop_duration=0.995,"
        );
        assert!(contact_sheet_filter(16, 640, 0, 20)
            .starts_with("tpad=stop_mode=clone:stop_duration=0.980,fps=16,"));
        assert!(contact_sheet_filter(4, 640, 0, 1_000).starts_with("fps=4,"));
    }

    #[test]
    fn contact_sheets_skip_tail_windows_without_a_frame() {
        assert_eq!(contact_sheet_starts(3_010, 10, 4), vec![0, 1_000, 2_000]);
        assert_eq!(
            contact_sheet_starts(3_250, 10, 4),
            vec![0, 1_000, 2_000, 3_000]
        );
        assert_eq!(contact_sheet_starts(3_050, 10, 16), vec![0, 1_000, 2_000]);
        assert_eq!(
            contact_sheet_starts(3_063, 10, 16),
            vec![0, 1_000, 2_000, 3_000]
        );
        assert_eq!(contact_sheet_starts(3_999, 10, 1), vec![0, 1_000, 2_000]);
        assert_eq!(contact_sheet_starts(400, 10, 4), vec![0]);
        assert_eq!(contact_sheet_starts(5, 10, 16), vec![0]);
        for fps in 1..=MAX_CONTACT_SHEET_FPS {
            let interval = CONTACT_SHEET_WINDOW_MS.div_ceil(u64::from(fps));
            for duration in [1_000, 2_001, 4_499, 7_900, 12_345] {
                let starts = contact_sheet_starts(duration, 100, fps);
                let last = starts.last().copied().unwrap_or_default();
                assert!(duration - last >= interval, "fps {fps} duration {duration}");
            }
        }
    }

    #[test]
    fn audio_levels_report_dbfs_per_quarter_second() {
        let step = (AUDIO_LEVEL_SAMPLE_RATE * AUDIO_LEVEL_STEP_MS / 1000) as usize;
        let mut samples = vec![0u8; step * 2];
        samples.extend(std::iter::repeat_n(16_384i16.to_le_bytes(), step).flatten());
        assert_eq!(rms_dbfs(&samples), vec![-90, -6]);
    }

    #[test]
    fn rejects_file_limits_that_disable_the_kernel_ceiling() {
        let mut resources = limits();
        resources.max_output_bytes = u64::MAX;
        assert_eq!(
            check_resources(&resources).err().map(|error| error.code),
            Some(ErrorCode::InvalidLimits)
        );
        resources.max_output_bytes = 5_000_000;
        resources.max_frame_bytes = u64::MAX;
        assert_eq!(
            check_resources(&resources).err().map(|error| error.code),
            Some(ErrorCode::InvalidLimits)
        );
    }

    #[test]
    fn hook_first_plan_reads_each_interval_from_its_own_input() {
        let plan_limits = PlanLimits {
            max_plan_bytes: 4096,
            max_source_duration_ms: 60_010,
            max_source_duration_drift_ms: 100,
            max_output_duration_ms: 70_000,
            max_intervals: 3,
            max_highlights: 1,
            max_review_reasons: 1,
            max_text_bytes: 50,
            max_subtitle_segments: 2,
            max_subtitle_text_bytes: 50,
            min_kept_percent: 0,
        };
        let plan = EditPlan {
            schema_version: 1,
            source_duration_ms: 60_010,
            kept_intervals: vec![KeptInterval {
                start_ms: 0,
                end_ms: 60_010,
                reason: "Main".into(),
            }],
            hook: Some(KeptInterval {
                start_ms: 56_000,
                end_ms: 59_130,
                reason: "Hook".into(),
            }),
            observed_highlights: vec![],
            significant_uncertainty: false,
            review_reasons: vec![],
            layout: LayoutIntent::ExistingStable,
        };
        let validated = validate_plan(plan, 60_010, &plan_limits).expect("hook-first plan");
        assert_eq!(
            cut_filter(&validated, true),
            "[0:v]trim=start=56.000:end=59.130,setpts=PTS-STARTPTS[v0];\
             [0:a]atrim=start=56.000:end=59.130,asetpts=PTS-STARTPTS[a0];\
             [1:v]trim=start=0.000:end=60.010,setpts=PTS-STARTPTS[v1];\
             [1:a]atrim=start=0.000:end=60.010,asetpts=PTS-STARTPTS[a1];\
             [v0][a0][v1][a1]concat=n=2:v=1:a=1[vout][aout]"
        );
    }

    #[tokio::test]
    async fn hevc_original_preview_is_uncut_browser_safe_and_bounded() {
        let root = tempfile::tempdir().expect("media fixture");
        let source = root.path().join("uncut-hevc.mp4");
        let mut fixture = Command::new("ffmpeg");
        fixture
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "2",
                "-c:v",
                "libx265",
                "-x265-params",
                "pools=1:frame-threads=1:log-level=error",
                "-c:a",
                "aac",
                "-y",
            ])
            .arg(&source);
        run_status(fixture, 15_000)
            .await
            .expect("real synthetic HEVC source");
        let resources = limits();
        let (info, codecs) = probe(&source, &resources).await.expect("source codec");
        assert_eq!(
            codecs
                .streams
                .iter()
                .find(|stream| stream.codec_type == "video")
                .and_then(|stream| stream.codec_name.as_deref()),
            Some("hevc")
        );
        let config = super::super::runtime_config::OriginalPreviewConfig {
            max_width: 320,
            max_height: 180,
            max_output_bytes: 5_000_000,
            encoder_threads: 1,
            nice: 10,
            crf: 26,
            preset: "veryfast".into(),
            process_deadline_ms: 15_000,
        };
        let before = tokio::fs::read(&source).await.expect("original bytes");
        let workspace = make_temp_directory(root.path()).expect("own workspace");
        let preview = render_original_preview(
            root.path(),
            &source,
            &workspace.0,
            &info,
            &resources,
            &config,
        )
        .await
        .expect("uncut preview");
        let output = validate_original_preview(&workspace.0, &preview, &info, &resources, &config)
            .await
            .expect("full H.264/AAC decode");
        assert_eq!(
            (output.width, output.height, output.has_audio),
            (320, 180, true)
        );
        assert!(output.duration_ms.abs_diff(info.duration_ms) <= resources.max_duration_drift_ms);
        assert_eq!(
            tokio::fs::read(&source).await.expect("unchanged original"),
            before
        );
        let pcm = workspace.0.join("preview.pcm");
        let mut decode = Command::new("ffmpeg");
        decode
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
            .arg(&preview)
            .args(["-vn", "-ac", "1", "-ar", "48000", "-f", "s16le", "-y"])
            .arg(&pcm);
        run_status(decode, 15_000)
            .await
            .expect("real full preview audio");
        let audio = tokio::fs::read(&pcm).await.expect("decoded uncut audio");
        assert!(audio.len() >= 48_000 * 2 * 2);
        assert!(audio
            .as_chunks::<2>()
            .0
            .iter()
            .any(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 500));
        let small_workspace = make_temp_directory(root.path()).expect("bounded failure workspace");
        let small = super::super::runtime_config::OriginalPreviewConfig {
            max_output_bytes: 64,
            ..config.clone()
        };
        assert!(render_original_preview(
            root.path(),
            &source,
            &small_workspace.0,
            &info,
            &resources,
            &small
        )
        .await
        .is_err());
        let retained = preview.clone();
        drop(workspace);
        assert!(!retained.exists());
    }

    async fn synthetic_video(root: &Path, name: &str, seconds: &str) -> PathBuf {
        let source = root.join(name);
        let mut fixture = Command::new("ffmpeg");
        fixture
            .kill_on_drop(true)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=160x90:r=60",
                "-t",
                seconds,
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-y",
            ])
            .arg(&source);
        run_status(fixture, 15_000)
            .await
            .expect("create synthetic video");
        source
    }

    #[tokio::test]
    async fn short_windows_still_yield_a_contact_sheet() {
        let root = tempfile::tempdir().expect("tempdir");
        let resources = limits();
        for (name, seconds, fps, sheets) in [
            ("tiny.mp4", "0.02", 16, 1),
            ("tail.mp4", "2.3", 16, 3),
            ("tail4.mp4", "1.3", 4, 2),
        ] {
            let source = synthetic_video(root.path(), name, seconds).await;
            let frames = extract_frames(root.path(), &source, root.path(), &resources, fps)
                .await
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(frames.frames().len(), sheets, "{name}");
        }
    }

    #[tokio::test]
    async fn synthetic_audio_video_probe_frames_and_cuts() {
        let root = tempfile::tempdir().expect("tempdir");
        let source = root.path().join("source.mp4");
        let mut fixture = Command::new("ffmpeg");
        fixture
            .kill_on_drop(true)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=160x90:r=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=16000",
                "-t",
                "3",
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-c:a",
                "aac",
                "-y",
            ])
            .arg(&source);
        run_status(fixture, 15_000)
            .await
            .expect("create synthetic media");
        let resources = limits();
        let source_info = inspect_media(root.path(), &source, &resources)
            .await
            .expect("probe source");
        assert!(source_info.has_audio);
        assert_eq!((source_info.width, source_info.height), (160, 90));
        let frames = extract_frames(root.path(), &source, root.path(), &resources, 4)
            .await
            .expect("extract across duration");
        let limited_frames = ResourceLimits {
            max_frame_bytes: 64,
            ..resources.clone()
        };
        let frame_error = extract_frames(root.path(), &source, root.path(), &limited_frames, 4)
            .await
            .expect_err("frame output must stay bounded");
        assert!(matches!(
            frame_error.code,
            ErrorCode::ProcessFailed | ErrorCode::MediaTooLarge
        ));
        let frame_paths = frames.frames();
        assert_eq!(frame_paths.len(), 3);
        assert!(frame_paths.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(frame_paths.iter().all(|(_, path)| path.exists()));
        let plan_limits = PlanLimits {
            max_plan_bytes: 4096,
            max_source_duration_ms: 5_000,
            max_source_duration_drift_ms: 100,
            max_output_duration_ms: 5_000,
            max_intervals: 3,
            max_highlights: 1,
            max_review_reasons: 1,
            max_text_bytes: 50,
            max_subtitle_segments: 2,
            max_subtitle_text_bytes: 50,
            min_kept_percent: 0,
        };
        let plan = EditPlan {
            schema_version: 1,
            source_duration_ms: source_info.duration_ms,
            kept_intervals: vec![
                KeptInterval {
                    start_ms: 500,
                    end_ms: 1500,
                    reason: "First event".into(),
                },
                KeptInterval {
                    start_ms: 2000,
                    end_ms: 2800,
                    reason: "Reaction".into(),
                },
            ],
            hook: None,
            observed_highlights: vec![],
            significant_uncertainty: false,
            review_reasons: vec![],
            layout: LayoutIntent::ExistingStable,
        };
        let mut stale_plan = plan.clone();
        stale_plan.source_duration_ms = source_info.duration_ms + 100;
        stale_plan.kept_intervals[1].end_ms = source_info.duration_ms + 100;
        let stale = validate_plan(stale_plan, source_info.duration_ms + 100, &plan_limits)
            .expect("plan matches earlier measurement");
        assert_eq!(
            prepare_intervals(root.path(), &source, root.path(), &stale, &resources)
                .await
                .err()
                .map(|error| error.code),
            Some(ErrorCode::InvalidInterval)
        );
        let validated =
            validate_plan(plan, source_info.duration_ms, &plan_limits).expect("valid intervals");
        let bounded_prep = ResourceLimits {
            max_output_bytes: 64,
            ..resources.clone()
        };
        let prep_error =
            prepare_intervals(root.path(), &source, root.path(), &validated, &bounded_prep)
                .await
                .expect_err("prepared output must stay bounded");
        assert!(matches!(
            prep_error.code,
            ErrorCode::ProcessFailed | ErrorCode::MediaTooLarge
        ));
        let prepared = prepare_intervals(root.path(), &source, root.path(), &validated, &resources)
            .await
            .expect("prepare cuts");
        let cut = inspect_media(root.path(), &prepared.path(), &resources)
            .await
            .expect("probe prepared");
        assert!(cut.has_audio);
        assert!(cut.duration_ms.abs_diff(1800) <= resources.max_duration_drift_ms);
        let pcm = root.path().join("cut.pcm");
        let mut decode = Command::new("ffmpeg");
        decode
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
            .arg(prepared.path())
            .args(["-vn", "-ac", "1", "-f", "s16le", "-y"])
            .arg(&pcm);
        run_status(decode, 15_000).await.expect("decode cut audio");
        let audio = tokio::fs::read(&pcm).await.expect("decoded audio");
        assert!(audio
            .as_chunks::<2>()
            .0
            .iter()
            .any(|sample| { i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 500 }));
        let bounded_render = ResourceLimits {
            max_output_bytes: 1024,
            ..resources.clone()
        };
        assert!(render_prepared(
            &prepared,
            None,
            &crate::subtitles::build_ass(&[]),
            &bounded_render,
        )
        .await
        .is_err());
        assert!(!prepared.owned.0.join("rendered.mp4").exists());
        let rendered = render_prepared(
            &prepared,
            None,
            &crate::subtitles::build_ass(&[]),
            &resources,
        )
        .await
        .expect("render branded audio and video");
        let rendered_info = inspect_media(root.path(), &rendered, &resources)
            .await
            .expect("probe rendered media");
        assert_eq!(
            (rendered_info.width, rendered_info.height),
            (TARGET_WIDTH as u32, TARGET_HEIGHT as u32)
        );
        assert!(rendered_info.has_audio);
        let decoded_frame = root.path().join("rendered-frame.jpg");
        let mut decode = Command::new("ffmpeg");
        decode
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-threads",
                "1",
                "-i",
            ])
            .arg(&rendered)
            .args(["-frames:v", "1", "-an", "-threads:v", "1", "-y"])
            .arg(&decoded_frame);
        run_status(decode, 15_000)
            .await
            .expect("decode rendered video");
        let frame = tokio::fs::read(&decoded_frame)
            .await
            .expect("decoded rendered frame");
        assert!(frame.starts_with(&[0xff, 0xd8]) && frame.ends_with(&[0xff, 0xd9]));
        let rendered_pcm = root.path().join("rendered.pcm");
        let mut decode = Command::new("ffmpeg");
        decode
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
            .arg(&rendered)
            .args(["-vn", "-ac", "1", "-f", "s16le", "-y"])
            .arg(&rendered_pcm);
        run_status(decode, 15_000)
            .await
            .expect("decode rendered audio");
        let rendered_audio = tokio::fs::read(&rendered_pcm)
            .await
            .expect("decoded rendered audio");
        assert!(rendered_audio
            .as_chunks::<2>()
            .0
            .iter()
            .any(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 500));
        assert!(source.exists());
        let owned_path = prepared.path();
        drop(prepared);
        assert!(!owned_path.exists());
        let owned_frames = frame_paths[0].1.clone();
        drop(frames);
        assert!(!owned_frames.exists());
    }
}

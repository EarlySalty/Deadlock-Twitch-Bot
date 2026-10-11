use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::layout::{StreamerLayout, TARGET_HEIGHT, TARGET_WIDTH};
use crate::video_processor::VideoProcessor;

use super::model::ResourceLimits;
use super::validation::{EditorError, ErrorCode, ValidatedPlan};

#[derive(Debug, Clone, PartialEq, Eq)]
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
        || limits.max_probe_bytes == 0
        || limits.max_media_duration_ms == 0
        || limits.max_media_width < TARGET_WIDTH as u32
        || limits.max_media_height < TARGET_HEIGHT as u32
        || limits.max_frames == 0
        || limits.max_frames > 256
        || !(2..=4096).contains(&limits.frame_width)
        || limits.frame_width % 2 != 0
        || limits.max_frame_bytes == 0
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

async fn probe_path(path: &Path, limits: &ResourceLimits) -> Result<MediaInfo, EditorError> {
    let mut command = Command::new("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,width,height",
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
    Ok(MediaInfo {
        duration_ms: duration_ms.round() as u64,
        width,
        height,
        has_audio: probe
            .streams
            .iter()
            .any(|stream| stream.codec_type == "audio"),
    })
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

pub async fn extract_frames(
    source_root: &Path,
    input: &Path,
    workspace_root: &Path,
    limits: &ResourceLimits,
) -> Result<FrameBatch, EditorError> {
    check_resources(limits)?;
    let path = source_path(source_root, input, limits.max_source_bytes)?;
    let info = probe_path(&path, limits).await?;
    let owned = make_temp_directory(workspace_root)?;
    let mut timestamps = Vec::with_capacity(limits.max_frames);
    tokio::time::timeout(
        Duration::from_millis(limits.total_frames_deadline_ms),
        async {
            for index in 0..limits.max_frames {
                let timestamp = ((index as u128 * 2 + 1) * info.duration_ms as u128
                    / (limits.max_frames as u128 * 2)) as u64;
                let output = owned.0.join(format!("frame-{index:04}.jpg"));
                let mut command = Command::new("ffmpeg");
                command
                    .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-ss"])
                    .arg(format_time(timestamp))
                    .arg("-i")
                    .arg(&path)
                    .args(["-frames:v", "1", "-vf"])
                    .arg(format!("scale={}:-2", limits.frame_width))
                    .args(["-q:v", "4", "-y"])
                    .arg(&output);
                run_status(command, limits.process_deadline_ms).await?;
                check_output(&output, limits.max_frame_bytes)?;
                let mut image = tokio::fs::File::open(&output)
                    .await
                    .map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
                let mut magic = [0; 2];
                image
                    .read_exact(&mut magic)
                    .await
                    .map_err(|_| EditorError::new(ErrorCode::InvalidMedia))?;
                if magic != [0xff, 0xd8] {
                    return Err(EditorError::new(ErrorCode::InvalidMedia));
                }
                timestamps.push(timestamp);
            }
            Ok(())
        },
    )
    .await
    .map_err(|_| EditorError::new(ErrorCode::ProcessDeadline))??;
    Ok(FrameBatch {
        owned,
        timestamps_ms: timestamps,
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
            "[0:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[v{index}]"
        ));
        if has_audio {
            steps.push(format!(
                "[0:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[a{index}]"
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
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(&path)
        .args([
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
            "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-threads",
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
        processor.render_branded(
            &prepared.path().to_string_lossy(),
            &output.to_string_lossy(),
            max_duration,
            layout,
            ass,
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
        let frames = extract_frames(root.path(), &source, root.path(), &resources)
            .await
            .expect("extract across duration");
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
            .chunks_exact(2)
            .any(|sample| { i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 500 }));
        let owned_path = prepared.path();
        drop(prepared);
        assert!(!owned_path.exists());
        let owned_frames = frame_paths[0].1.clone();
        drop(frames);
        assert!(!owned_frames.exists());
    }
}

//! FFmpeg-Wrapper für Video-Konvertierung (Port von
//! `bot/social_media/uploaders/video_processor.py`).
//!
//! Konvertiert Landscape-Clips ins Hochformat (9:16) für Shorts/Reels. Zwei
//! Pfade: der einfache `convert_and_trim` (Center-Crop, vom Upload-Worker
//! genutzt) und das layout-bewusste `compose_vertical` (Game + Cam als
//! PiP/Stacked, vom Dashboard-Compose genutzt). Die Filtergraph-Erzeugung ist
//! rein und getestet; die ffmpeg/ffprobe-Aufrufe sind dünne Subprocess-Wrapper.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::layout::{StreamerLayout, TARGET_HEIGHT, TARGET_WIDTH};
#[cfg(target_os = "linux")]
use crate::media_sandbox::{media_command, SandboxError, SandboxOutput};

#[derive(Debug, thiserror::Error)]
pub enum VideoProcessorError {
    #[error("subprocess spawn failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("ffprobe failed: {0}")]
    Ffprobe(String),
    #[error("ffmpeg failed: {0}")]
    Ffmpeg(String),
    #[error("could not parse ffprobe output: {0}")]
    Parse(String),
    #[error("output file not created: {0}")]
    OutputMissing(String),
    #[error("{0} timed out")]
    Timeout(&'static str),
}

/// Video-Metadaten aus ffprobe.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub width: i64,
    pub height: i64,
    pub duration: f64,
    pub aspect_ratio: f64,
    pub fps: f64,
    pub has_audio: bool,
}

/// Baut den `-filter_complex`-Graph fürs layout-bewusste Compositing
/// (Game-Crop + optional Cam als PiP oder Stacked).
///
/// `game_crop`/`cam_crop` sind Ausschnitte aus dem Twitch-Bild, `cam_position`
/// ist das Zielrechteck im 1080x1920-Frame. Im PiP-Modus bestimmt es Größe und
/// Position der Cam-Kachel; im Stacked-Modus zählt bewusst nur `h` (die Höhe des
/// Cam-Streifens oben), weil der Streifen immer oben sitzt und immer die volle
/// Breite hat — `x`, `y` und `w` werden dort ignoriert.
pub fn build_compose_filter(layout: &StreamerLayout, mode: &str, cam_enabled: bool) -> String {
    let g = &layout.game_crop;
    let c = &layout.cam_crop;
    // Defensiv clampen: ein Altlayout darf keinen Overlay ausserhalb des Frames
    // erzeugen (gelesene Layouts sind bereits geclampt, direkt gebaute nicht).
    let p = layout.cam_position.clamped_to_target();
    // `clamped_to_target` liefert gerade Kantenlängen; die 2 px Reserve halten
    // auch die Game-Fläche darunter gerade und größer als null.
    let top_height = p.h.clamp(2, TARGET_HEIGHT - 2);
    let game_height = TARGET_HEIGHT - top_height;
    let source_normalization = format!(
        "scale={sw}:{sh}:force_original_aspect_ratio=decrease,\
         pad={sw}:{sh}:(ow-iw)/2:(oh-ih)/2,setsar=1",
        sw = layout.source.width,
        sh = layout.source.height
    );

    let base_game = format!(
        "[0:v]{normalize},crop={gw}:{gh}:{gx}:{gy},\
         scale={tw}:{th}:force_original_aspect_ratio=increase,\
         crop={tw}:{th},setsar=1[gamefull]",
        normalize = source_normalization,
        gw = g.w,
        gh = g.h,
        gx = g.x,
        gy = g.y,
        tw = TARGET_WIDTH,
        th = TARGET_HEIGHT
    );

    if !cam_enabled {
        return base_game.replace("[gamefull]", "[vout]");
    }

    if mode == "stacked" {
        let cam = format!(
            "[0:v]{normalize},crop={cw}:{ch}:{cx}:{cy},\
             scale={tw}:{top}:force_original_aspect_ratio=increase,\
             crop={tw}:{top},setsar=1[cam]",
            normalize = source_normalization,
            cw = c.w,
            ch = c.h,
            cx = c.x,
            cy = c.y,
            tw = TARGET_WIDTH,
            top = top_height
        );
        let game = format!(
            "[0:v]{normalize},crop={gw}:{gh}:{gx}:{gy},\
             scale={tw}:{gh2}:force_original_aspect_ratio=increase,\
             crop={tw}:{gh2},setsar=1[game]",
            normalize = source_normalization,
            gw = g.w,
            gh = g.h,
            gx = g.x,
            gy = g.y,
            tw = TARGET_WIDTH,
            gh2 = game_height
        );
        return [cam, game, "[cam][game]vstack=inputs=2[vout]".to_string()].join(";");
    }

    // PiP: die Cam landet exakt auf dem Zielrechteck aus cam_position.
    let cam = format!(
        "[0:v]{normalize},crop={cw}:{ch}:{cx}:{cy},\
         scale={pw}:{ph}:force_original_aspect_ratio=increase,\
         crop={pw}:{ph},setsar=1[cam]",
        normalize = source_normalization,
        cw = c.w,
        ch = c.h,
        cx = c.x,
        cy = c.y,
        pw = p.w,
        ph = p.h
    );
    let overlay = format!("[gamefull][cam]overlay={px}:{py}[vout]", px = p.x, py = p.y);
    [base_game, cam, overlay].join(";")
}

/// Baut den `-vf`-Crop-Filter für `convert_to_vertical` (Center/Top/Bottom bzw.
/// Center/Left/Right je nach Quell-Seitenverhältnis). Rein, mirror Pythons
/// `_build_crop_filter`.
pub fn build_crop_filter(
    src_width: i64,
    src_height: i64,
    target_width: i64,
    target_height: i64,
    crop_mode: &str,
) -> String {
    let target_ratio = target_width as f64 / target_height as f64;
    let src_ratio = src_width as f64 / src_height as f64;

    let (crop_w, crop_h, crop_x, crop_y);
    if src_ratio > target_ratio {
        // Quelle breiter → an den Seiten beschneiden.
        crop_w = (src_height as f64 * target_ratio) as i64;
        crop_h = src_height;
        crop_y = 0;
        crop_x = match crop_mode {
            "center" => (src_width - crop_w) / 2,
            "left" => 0,
            _ => src_width - crop_w,
        };
    } else {
        // Quelle höher → oben/unten beschneiden.
        crop_w = src_width;
        crop_h = (src_width as f64 / target_ratio) as i64;
        crop_x = 0;
        crop_y = match crop_mode {
            "center" => (src_height - crop_h) / 2,
            "top" => 0,
            _ => src_height - crop_h,
        };
    }
    format!("crop={crop_w}:{crop_h}:{crop_x}:{crop_y},scale={target_width}:{target_height}")
}

/// FFmpeg/ffprobe-Wrapper.
#[derive(Debug, Clone)]
pub struct VideoProcessor {
    ffmpeg: String,
    ffprobe: String,
}

impl Default for VideoProcessor {
    fn default() -> Self {
        Self {
            ffmpeg: "/usr/bin/ffmpeg".to_string(),
            ffprobe: "/usr/bin/ffprobe".to_string(),
        }
    }
}

impl VideoProcessor {
    pub fn new(ffmpeg_path: impl Into<String>, ffprobe_path: impl Into<String>) -> Self {
        Self {
            ffmpeg: ffmpeg_path.into(),
            ffprobe: ffprobe_path.into(),
        }
    }

    /// Liest Breite/Höhe/Dauer via ffprobe.
    pub async fn get_video_info(&self, video_path: &str) -> Result<VideoInfo, VideoProcessorError> {
        let output = sandboxed_media_output(
            &self.ffprobe,
            video_path,
            None,
            [
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-threads",
                "1",
                "-show_entries",
                "stream=codec_type,width,height,duration,r_frame_rate:format=duration",
                "-of",
                "json",
                "/input.mp4",
            ],
            "ffprobe",
            Duration::from_secs(30),
        )
        .await?;
        if !output.process.status.success() {
            return Err(VideoProcessorError::Ffprobe(
                "ffprobe lieferte keinen erfolgreichen Status".to_string(),
            ));
        }
        let data: Value = serde_json::from_slice(&output.process.stdout)
            .map_err(|e| VideoProcessorError::Parse(e.to_string()))?;
        let streams = data
            .get("streams")
            .and_then(Value::as_array)
            .ok_or_else(|| VideoProcessorError::Parse("no streams".to_string()))?;
        let stream = streams
            .iter()
            .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("video"))
            .ok_or_else(|| VideoProcessorError::Parse("no video stream".to_string()))?;
        let width = num_field(stream, "width")
            .ok_or_else(|| VideoProcessorError::Parse("width".to_string()))?;
        let height = num_field(stream, "height")
            .ok_or_else(|| VideoProcessorError::Parse("height".to_string()))?;
        let duration = stream
            .get("duration")
            .and_then(|d| {
                d.as_f64()
                    .or_else(|| d.as_str().and_then(|s| s.parse().ok()))
            })
            .or_else(|| {
                data.pointer("/format/duration").and_then(|d| {
                    d.as_f64()
                        .or_else(|| d.as_str().and_then(|s| s.parse().ok()))
                })
            })
            .unwrap_or(0.0);
        let aspect_ratio = if height > 0 {
            width as f64 / height as f64
        } else {
            0.0
        };
        let fps = stream
            .get("r_frame_rate")
            .and_then(Value::as_str)
            .and_then(parse_rate)
            .unwrap_or(0.0);
        Ok(VideoInfo {
            width,
            height,
            duration,
            aspect_ratio,
            fps,
            has_audio: streams
                .iter()
                .any(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("audio")),
        })
    }

    /// Layout-bewusstes Compositing (Game + optional Cam) ins Hochformat.
    pub async fn compose_vertical(
        &self,
        input_path: &str,
        output_path: &str,
        layout: &StreamerLayout,
        mode: &str,
        cam_enabled: bool,
    ) -> Result<(), VideoProcessorError> {
        let resolved_mode = if mode.trim().is_empty() {
            layout.mode.clone()
        } else {
            mode.to_string()
        };
        let filter_graph = build_compose_filter(
            layout,
            resolved_mode.trim().to_lowercase().as_str(),
            cam_enabled,
        );
        let output = sandboxed_media_output(
            &self.ffmpeg,
            input_path,
            Some(output_path),
            [
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-filter_threads",
                "2",
                "-filter_complex_threads",
                "2",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
                "/input.mp4",
                "-filter_complex",
                &filter_graph,
                "-map",
                "[vout]",
                "-map",
                "0:a?",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "23",
                "-threads",
                "4",
                "-c:a",
                "aac",
                "-af",
                "loudnorm",
                "-movflags",
                "+faststart",
                "-fs",
                "536870912",
                "-y",
                "/output.mp4",
            ],
            "ffmpeg",
            Duration::from_secs(15 * 60),
        )
        .await?;
        if !output.process.status.success() {
            return Err(VideoProcessorError::Ffmpeg(
                "ffmpeg lieferte keinen erfolgreichen Status".to_string(),
            ));
        }
        ensure_sandbox_output(&output, output_path)
    }

    /// Layout-bewusstes Compositing und harter Längendeckel in einem FFmpeg-Lauf.
    /// Das ist der universelle Prepare-only-Renderer und vermeidet eine zweite,
    /// plattformspezifische Neukodierung im Upload-Worker.
    pub async fn compose_and_trim(
        &self,
        input_path: &str,
        output_path: &str,
        layout: &StreamerLayout,
        max_duration: i64,
    ) -> Result<(), VideoProcessorError> {
        let args = compose_and_trim_args("/input.mp4", "/output.mp4", layout, max_duration);
        let output = sandboxed_media_output(
            &self.ffmpeg,
            input_path,
            Some(output_path),
            args,
            "ffmpeg",
            Duration::from_secs(15 * 60),
        )
        .await?;
        if !output.process.status.success() {
            return Err(VideoProcessorError::Ffmpeg(
                "ffmpeg lieferte keinen erfolgreichen Status".to_string(),
            ));
        }
        ensure_sandbox_output(&output, output_path)
    }

    /// Konvertiert 16:9 → 9:16 (Center-Crop bzw. Scale bei bereits hochformatig).
    pub async fn convert_to_vertical(
        &self,
        input_path: &str,
        output_path: &str,
        target_width: i64,
        target_height: i64,
        crop_mode: &str,
    ) -> Result<(), VideoProcessorError> {
        let info = self.get_video_info(input_path).await?;
        let filter = if info.aspect_ratio > 1.0 {
            build_crop_filter(
                info.width,
                info.height,
                target_width,
                target_height,
                crop_mode,
            )
        } else {
            format!("scale={target_width}:{target_height}")
        };
        let output = sandboxed_media_output(
            &self.ffmpeg,
            input_path,
            Some(output_path),
            [
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-filter_threads",
                "2",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
                "/input.mp4",
                "-vf",
                &filter,
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "23",
                "-threads",
                "4",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-movflags",
                "+faststart",
                "-fs",
                "536870912",
                "-y",
                "/output.mp4",
            ],
            "ffmpeg",
            Duration::from_secs(15 * 60),
        )
        .await?;
        if !output.process.status.success() {
            return Err(VideoProcessorError::Ffmpeg(
                "ffmpeg lieferte keinen erfolgreichen Status".to_string(),
            ));
        }
        ensure_sandbox_output(&output, output_path)
    }

    /// Schneidet das Video auf `max_duration` Sekunden (oder kopiert es, wenn
    /// bereits kürzer).
    pub async fn trim_video(
        &self,
        input_path: &str,
        output_path: &str,
        max_duration: i64,
    ) -> Result<(), VideoProcessorError> {
        let duration = max_duration.max(1).to_string();
        let output = sandboxed_media_output(
            &self.ffmpeg,
            input_path,
            Some(output_path),
            [
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-threads",
                "2",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
                "/input.mp4",
                "-t",
                &duration,
                "-c",
                "copy",
                "-fs",
                "536870912",
                "-y",
                "/output.mp4",
            ],
            "ffmpeg",
            Duration::from_secs(15 * 60),
        )
        .await?;
        if !output.process.status.success() {
            return Err(VideoProcessorError::Ffmpeg(
                "ffmpeg lieferte keinen erfolgreichen Status".to_string(),
            ));
        }
        ensure_sandbox_output(&output, output_path)
    }

    /// All-in-one: erst auf `max_duration` schneiden (falls nötig), dann ins
    /// Hochformat konvertieren. Vom Upload-Worker genutzt.
    pub async fn convert_and_trim(
        &self,
        input_path: &str,
        output_path: &str,
        max_duration: i64,
        target_width: i64,
        target_height: i64,
    ) -> Result<(), VideoProcessorError> {
        let info = self.get_video_info(input_path).await?;
        let mut temp_path = input_path.to_string();
        if info.duration > max_duration as f64 {
            temp_path = Path::new(output_path)
                .with_extension("temp.mp4")
                .to_string_lossy()
                .into_owned();
            self.trim_video(input_path, &temp_path, max_duration)
                .await?;
        }
        self.convert_to_vertical(
            &temp_path,
            output_path,
            target_width,
            target_height,
            "center",
        )
        .await?;
        if temp_path != input_path {
            let _ = tokio::fs::remove_file(&temp_path).await;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
async fn sandboxed_media_output<I, S>(
    program_path: &str,
    input_path: &str,
    output_path: Option<&str>,
    args: I,
    program: &'static str,
    timeout: Duration,
) -> Result<SandboxOutput, VideoProcessorError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = media_command(
        Path::new(program_path),
        Path::new(input_path),
        output_path.map(Path::new),
    )
    .map_err(|error| map_sandbox_error(error, program))?;
    if program == "ffprobe" {
        command.capture_stdout();
    }
    command.args(args);
    command
        .output(timeout)
        .await
        .map_err(|error| map_sandbox_error(error, program))
}

#[cfg(not(target_os = "linux"))]
async fn sandboxed_media_output<I, S>(
    _: &str,
    _: &str,
    _: Option<&str>,
    _: I,
    _: &'static str,
    _: Duration,
) -> Result<(), VideoProcessorError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Err(VideoProcessorError::Spawn(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Medien-Sandbox wird auf diesem System nicht unterstützt",
    )))
}

#[cfg(target_os = "linux")]
fn map_sandbox_error(error: SandboxError, program: &'static str) -> VideoProcessorError {
    match error {
        SandboxError::Timeout => VideoProcessorError::Timeout(program),
        SandboxError::OutputLimit => {
            VideoProcessorError::Parse("Sandbox-Ausgabelimit überschritten".to_string())
        }
        SandboxError::Io(error) => VideoProcessorError::Spawn(error),
    }
}

#[cfg(target_os = "linux")]
fn ensure_sandbox_output(
    output: &SandboxOutput,
    output_path: &str,
) -> Result<(), VideoProcessorError> {
    if output.output_len.is_some_and(|length| length > 0) {
        Ok(())
    } else {
        Err(VideoProcessorError::OutputMissing(output_path.to_string()))
    }
}

fn compose_and_trim_args(
    input_path: &str,
    output_path: &str,
    layout: &StreamerLayout,
    max_duration: i64,
) -> Vec<String> {
    vec![
        "-nostdin".to_string(),
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-filter_threads".to_string(),
        "2".to_string(),
        "-filter_complex_threads".to_string(),
        "2".to_string(),
        "-protocol_whitelist".to_string(),
        "file,pipe".to_string(),
        "-i".to_string(),
        input_path.to_string(),
        "-t".to_string(),
        max_duration.max(1).to_string(),
        "-filter_complex".to_string(),
        build_compose_filter(layout, &layout.mode, layout.cam_enabled),
        "-map".to_string(),
        "[vout]".to_string(),
        "-map".to_string(),
        "0:a?".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "medium".to_string(),
        "-crf".to_string(),
        "23".to_string(),
        "-threads".to_string(),
        "4".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "128k".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        "-fs".to_string(),
        "536870912".to_string(),
        "-y".to_string(),
        output_path.to_string(),
    ]
}

/// `#tag`-Liste, leere Tags werden übersprungen (mirror `format_hashtags`).
pub fn format_hashtags(hashtags: &[String]) -> String {
    hashtags
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| format!("#{t}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn num_field(stream: &Value, key: &str) -> Option<i64> {
    let v = stream.get(key)?;
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn parse_rate(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator > 0.0).then_some(numerator / denominator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{default_streamer_layout, LayoutBox};

    #[test]
    fn compose_and_trim_verwendet_layout_und_harten_60s_deckel() {
        let mut layout = default_streamer_layout();
        layout.cam_enabled = false;
        let args = compose_and_trim_args("source.mp4", "preview.mp4", &layout, 60);
        assert_eq!(
            args.windows(2).find(|pair| pair[0] == "-t"),
            Some(&["-t".to_string(), "60".to_string()][..])
        );
        let filter_index = args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        assert_eq!(
            args[filter_index + 1],
            build_compose_filter(&layout, "pip", false)
        );
        assert_eq!(args.last().map(String::as_str), Some("preview.mp4"));
    }

    #[tokio::test]
    async fn compose_and_trim_rendert_720p_quelle_mit_ton_als_1080x1920() {
        match tokio::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .await
        {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("ffmpeg konnte nicht geprüft werden: {error}"),
            Ok(output) if !output.status.success() => panic!("ffmpeg -version ist fehlgeschlagen"),
            Ok(_) => {}
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tb_sm_compose_720p_{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.mp4");
        let output = root.join("output.mp4");
        let generated = tokio::process::Command::new("ffmpeg")
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1280x720:rate=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "0.4",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-y",
                source.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .unwrap();
        assert!(
            generated.status.success(),
            "720p-Testquelle fehlgeschlagen: {}",
            String::from_utf8_lossy(&generated.stderr)
        );

        // Produktionspfad: Quelle und privates Job-Verzeichnis werden als
        // gehaltene Parent-FDs übergeben. Die Sandbox darf nur den exakten
        // Output-Inode sehen; eine Geschwisterdatei muss unangetastet bleiben.
        use std::os::fd::AsRawFd;
        let source_handle = std::fs::File::open(&source).unwrap();
        let directory_handle = std::fs::File::open(&root).unwrap();
        let held_source = format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            source_handle.as_raw_fd()
        );
        let held_output = format!(
            "/proc/{}/fd/{}/output.mp4",
            std::process::id(),
            directory_handle.as_raw_fd()
        );
        let sentinel = root.join("anderer-job.mp4");
        std::fs::write(&sentinel, b"nicht anfassen").unwrap();

        let processor = VideoProcessor::default();
        processor
            .compose_and_trim(&held_source, &held_output, &default_streamer_layout(), 60)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"nicht anfassen");
        let info = processor
            .get_video_info(output.to_string_lossy().as_ref())
            .await
            .unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
        assert!(info.duration > 0.0 && info.duration <= 60.0);
        assert!(info.fps > 0.0 && info.fps <= 60.0);
        assert!(info.has_audio);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sandbox_rendert_1440p_stacked_unter_den_produktionslimits() {
        if !Path::new("/usr/bin/ffmpeg").is_file() || !Path::new("/usr/bin/bwrap").is_file() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "tb-sm-compose-1440-stacked-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.mp4");
        let output = root.join("output.mp4");
        let generated = tokio::process::Command::new("/usr/bin/ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=2560x1440:rate=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "0.4",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-threads",
                "4",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-y",
                source.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .unwrap();
        assert!(
            generated.status.success(),
            "1440p-Testquelle fehlgeschlagen: {}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let mut layout = default_streamer_layout();
        layout.mode = "stacked".to_string();
        let processor = VideoProcessor::default();
        processor
            .compose_and_trim(
                source.to_string_lossy().as_ref(),
                output.to_string_lossy().as_ref(),
                &layout,
                60,
            )
            .await
            .unwrap();
        let info = processor
            .get_video_info(output.to_string_lossy().as_ref())
            .await
            .unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
        assert!(info.duration > 0.0 && info.duration <= 60.0);
        assert!(info.fps > 0.0 && info.fps <= 60.0);
        assert!(info.has_audio);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compose_filter_cam_off() {
        let f = build_compose_filter(&default_streamer_layout(), "pip", false);
        assert_eq!(
            f,
            "[0:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1,crop=1080:1080:0:0,scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,setsar=1[vout]"
        );
        assert!(!f.contains("[gamefull]"));
    }

    #[test]
    fn compose_filter_pip() {
        // Default-cam_position ist die Kachel rechts oben: 320x320 bei (712,48).
        let f = build_compose_filter(&default_streamer_layout(), "pip", true);
        let parts: Vec<&str> = f.split(';').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].ends_with("setsar=1[gamefull]"));
        assert_eq!(
            parts[1],
            "[0:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1,crop=380:380:1500:50,scale=320:320:force_original_aspect_ratio=increase,crop=320:320,setsar=1[cam]"
        );
        assert_eq!(parts[2], "[gamefull][cam]overlay=712:48[vout]");
    }

    #[test]
    fn compose_filter_pip_folgt_cam_position() {
        // Frei gesetztes Zielrechteck: Groesse UND Position kommen aus cam_position.
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox {
            x: 60,
            y: 1200,
            w: 420,
            h: 560,
        };
        let f = build_compose_filter(&layout, "pip", true);
        let parts: Vec<&str> = f.split(';').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[1],
            "[0:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1,crop=380:380:1500:50,scale=420:560:force_original_aspect_ratio=increase,crop=420:560,setsar=1[cam]"
        );
        assert_eq!(parts[2], "[gamefull][cam]overlay=60:1200[vout]");
        // Keine festen 320/48-Reste mehr im Graphen.
        assert!(!f.contains("W-w-"));
    }

    #[test]
    fn compose_filter_pip_clampt_in_den_zielframe() {
        // Defensiv: ein Altlayout, das ueber den Rand ragt, darf keinen
        // ffmpeg-Graphen erzeugen, der ausserhalb des Frames landet.
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox {
            x: 900,
            y: 1800,
            w: 600,
            h: 400,
        };
        let f = build_compose_filter(&layout, "pip", true);
        let parts: Vec<&str> = f.split(';').collect();
        assert_eq!(parts[2], "[gamefull][cam]overlay=480:1520[vout]");
    }

    #[test]
    fn compose_filter_erzwingt_gerade_kantenlaengen() {
        // Ungerade Maße im Filtergraphen lassen libx264 mit yuv420p abbrechen.
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox {
            x: 60,
            y: 1200,
            w: 421,
            h: 561,
        };
        let f = build_compose_filter(&layout, "pip", true);
        assert!(f.contains("scale=420:560:"), "{f}");
        assert!(f.contains("crop=420:560,"), "{f}");

        // Auch der Streifen und die Restflaeche darunter bleiben gerade.
        layout.cam_position = LayoutBox {
            x: 0,
            y: 0,
            w: 1080,
            h: 541,
        };
        let f = build_compose_filter(&layout, "stacked", true);
        assert!(f.contains("scale=1080:540:"), "{f}");
        assert!(f.contains("scale=1080:1380:"), "{f}");
    }

    #[test]
    fn compose_filter_stacked() {
        // Stacked nutzt nur cam_position.h: 320 → game_height = 1600.
        let f = build_compose_filter(&default_streamer_layout(), "stacked", true);
        let parts: Vec<&str> = f.split(';').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[0],
            "[0:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1,crop=380:380:1500:50,scale=1080:320:force_original_aspect_ratio=increase,crop=1080:320,setsar=1[cam]"
        );
        assert_eq!(
            parts[1],
            "[0:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1,crop=1080:1080:0:0,scale=1080:1600:force_original_aspect_ratio=increase,crop=1080:1600,setsar=1[game]"
        );
        assert_eq!(parts[2], "[cam][game]vstack=inputs=2[vout]");
    }

    #[test]
    fn compose_filter_stacked_ignoriert_x_y_w() {
        // x/y/w sind im Streifen-Modus bewusst wirkungslos: der Streifen sitzt
        // immer oben und ist immer 1080 breit.
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox {
            x: 333,
            y: 777,
            w: 444,
            h: 320,
        };
        assert_eq!(
            build_compose_filter(&layout, "stacked", true),
            build_compose_filter(&default_streamer_layout(), "stacked", true)
        );
    }

    #[test]
    fn crop_filter_landscape_modi() {
        // 1920x1080 → 1080x1920, target_ratio=0.5625 → crop_w=607.
        assert_eq!(
            build_crop_filter(1920, 1080, 1080, 1920, "center"),
            "crop=607:1080:656:0,scale=1080:1920"
        );
        assert_eq!(
            build_crop_filter(1920, 1080, 1080, 1920, "left"),
            "crop=607:1080:0:0,scale=1080:1920"
        );
        assert_eq!(
            build_crop_filter(1920, 1080, 1080, 1920, "right"),
            "crop=607:1080:1313:0,scale=1080:1920"
        );
    }

    #[test]
    fn crop_filter_portrait_modi() {
        // 720x1280 (ratio == target) → höher-Branch, crop_h=1280.
        assert_eq!(
            build_crop_filter(720, 1280, 1080, 1920, "center"),
            "crop=720:1280:0:0,scale=1080:1920"
        );
        // 1080x2400 → höher als target → oben/unten beschneiden.
        assert_eq!(
            build_crop_filter(1080, 2400, 1080, 1920, "top"),
            "crop=1080:1920:0:0,scale=1080:1920"
        );
        assert_eq!(
            build_crop_filter(1080, 2400, 1080, 1920, "bottom"),
            "crop=1080:1920:0:480,scale=1080:1920"
        );
    }

    #[test]
    fn format_hashtags_skip_leer() {
        assert_eq!(
            format_hashtags(&["deadlock".into(), "".into(), "haze".into()]),
            "#deadlock #haze"
        );
        assert_eq!(format_hashtags(&[]), "");
    }
}

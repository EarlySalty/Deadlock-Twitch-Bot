//! FFmpeg-Wrapper für Video-Konvertierung (Port von
//! `bot/social_media/uploaders/video_processor.py`).
//!
//! Konvertiert Landscape-Clips ins Hochformat (9:16) für Shorts/Reels. Zwei
//! Pfade: der einfache `convert_and_trim` (Center-Crop, vom Upload-Worker
//! genutzt) und das layout-bewusste `compose_vertical` (Game + Cam als
//! PiP/Stacked, vom Dashboard-Compose genutzt). Die Filtergraph-Erzeugung ist
//! rein und getestet; die ffmpeg/ffprobe-Aufrufe sind dünne Subprocess-Wrapper.

use std::path::Path;

use serde_json::Value;

use crate::layout::{LayoutBox, StreamerLayout, TARGET_HEIGHT, TARGET_WIDTH};

const VIDEO_PRESET: &str = "medium";
const VIDEO_CRF: &str = "18";
const VIDEO_PROFILE: &str = "high";
const PIXEL_FORMAT: &str = "yuv420p";
const AUDIO_BITRATE: &str = "192k";
const AUDIO_SAMPLE_RATE: &str = "48000";
const MAX_FPS: &str = "60";
const BRAND_GOLD: &str = "0xC5A059";
const LOGO: &[u8] = include_bytes!("../assets/ddc-logo.png");
const FONT: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");

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
}

/// Video-Metadaten aus ffprobe.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub width: i64,
    pub height: i64,
    pub duration: f64,
    pub aspect_ratio: f64,
}

fn fit_stacked_region(
    region: LayoutBox,
    center_x: i64,
    center_y: i64,
    game_height: i64,
) -> Option<LayoutBox> {
    if region.w < 2 || region.h < 2 {
        return None;
    }
    let (w, h) = if region.w * game_height >= region.h * TARGET_WIDTH {
        let h = region.h - region.h % 2;
        let w = h * TARGET_WIDTH / game_height;
        (w - w % 2, h)
    } else {
        let w = region.w - region.w % 2;
        let h = w * game_height / TARGET_WIDTH;
        (w, h - h % 2)
    };
    if w < 2 || h < 2 {
        return None;
    }
    Some(LayoutBox {
        x: (center_x - w / 2).clamp(region.x, region.x + region.w - w),
        y: (center_y - h / 2).clamp(region.y, region.y + region.h - h),
        w,
        h,
    })
}

fn crops_overlap(a: LayoutBox, b: LayoutBox) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn stacked_game_crop(layout: &StreamerLayout, game_height: i64) -> LayoutBox {
    let game = layout.game_crop;
    let cam = layout.cam_crop;
    let center_x = game.x + game.w / 2;
    let center_y = game.y + game.h / 2;
    let centered = fit_stacked_region(game, center_x, center_y, game_height).unwrap_or(game);
    if !crops_overlap(centered, cam) {
        return centered;
    }
    let gap = 8;
    let regions = [
        LayoutBox {
            x: game.x,
            y: game.y,
            w: (cam.x - gap - game.x).max(0),
            h: game.h,
        },
        LayoutBox {
            x: (cam.x + cam.w + gap).max(game.x),
            y: game.y,
            w: (game.x + game.w - cam.x - cam.w - gap).max(0),
            h: game.h,
        },
        LayoutBox {
            x: game.x,
            y: game.y,
            w: game.w,
            h: (cam.y - gap - game.y).max(0),
        },
        LayoutBox {
            x: game.x,
            y: (cam.y + cam.h + gap).max(game.y),
            w: game.w,
            h: (game.y + game.h - cam.y - cam.h - gap).max(0),
        },
    ];
    regions
        .into_iter()
        .filter_map(|region| fit_stacked_region(region, center_x, center_y, game_height))
        .filter(|crop| !crops_overlap(*crop, cam))
        .max_by(|a, b| {
            let score = |crop: &LayoutBox| {
                let dx = (crop.x + crop.w / 2 - center_x).abs() as f64 / game.w as f64;
                let dy = (crop.y + crop.h / 2 - center_y).abs() as f64 / game.h as f64;
                let coverage = (crop.w * crop.h) as f64 / (game.w * game.h) as f64;
                coverage * 0.1 - dx - dy
            };
            score(a).total_cmp(&score(b))
        })
        .unwrap_or(centered)
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

    if mode == "blur_pad" || !cam_enabled {
        return build_fallback_filter();
    }

    let base_game = format!(
        "[0:v]crop={gw}:{gh}:{gx}:{gy},\
         scale={tw}:{th}:force_original_aspect_ratio=increase,\
         crop={tw}:{th},setsar=1[gamefull]",
        gw = g.w,
        gh = g.h,
        gx = g.x,
        gy = g.y,
        tw = TARGET_WIDTH,
        th = TARGET_HEIGHT
    );

    if mode == "stacked" {
        let cam = format!(
            "[0:v]crop={cw}:{ch}:{cx}:{cy},\
             scale={tw}:{top}:force_original_aspect_ratio=increase,\
             crop={tw}:{top},setsar=1[cam]",
            cw = c.w,
            ch = c.h,
            cx = c.x,
            cy = c.y,
            tw = TARGET_WIDTH,
            top = top_height
        );
        let game_crop = stacked_game_crop(layout, game_height);
        let game = format!(
            "[0:v]crop={gw}:{gh}:{gx}:{gy},\
             scale={tw}:{area}:force_original_aspect_ratio=increase,\
             crop={tw}:{area},setsar=1[game]",
            gw = game_crop.w,
            gh = game_crop.h,
            gx = game_crop.x,
            gy = game_crop.y,
            tw = TARGET_WIDTH,
            area = game_height,
        );
        return [
            cam,
            game,
            format!(
                "[cam][game]vstack=inputs=2[stack];[stack]drawbox=x=0:y={top_height}:w=iw:h=8:color={BRAND_GOLD}:t=fill[vout]"
            ),
        ]
        .join(";");
    }

    // PiP: die Cam landet exakt auf dem Zielrechteck aus cam_position.
    let cam = format!(
        "[0:v]crop={cw}:{ch}:{cx}:{cy},\
         scale={pw}:{ph}:force_original_aspect_ratio=increase,\
         crop={pw}:{ph},setsar=1[cam]",
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

pub fn build_fallback_filter() -> String {
    format!(
        "[0:v]setsar=1,split=2[bg][fg];\
         [bg]scale={tw}:{th}:force_original_aspect_ratio=increase,\
         crop={tw}:{th},boxblur=20:1[blur];\
         [fg]scale={tw}:{th}:force_original_aspect_ratio=decrease[front];\
         [blur][front]overlay=(W-w)/2:(H-h)/2[vout]",
        tw = TARGET_WIDTH,
        th = TARGET_HEIGHT,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerticalRender {
    Compose { filter: String, cam_enabled: bool },
    BlurPad,
}

pub fn plan_vertical_render(layout: Option<&StreamerLayout>) -> VerticalRender {
    match layout {
        Some(l) => VerticalRender::Compose {
            filter: build_compose_filter(l, l.mode.trim().to_lowercase().as_str(), l.cam_enabled),
            cam_enabled: l.cam_enabled,
        },
        None => VerticalRender::BlurPad,
    }
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
            ffmpeg: "ffmpeg".to_string(),
            ffprobe: "ffprobe".to_string(),
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

    pub async fn render_branded(
        &self,
        input_path: &str,
        output_path: &str,
        max_duration: i64,
        layout: Option<&StreamerLayout>,
        ass: &str,
    ) -> Result<(), VideoProcessorError> {
        let input = std::fs::canonicalize(input_path)?;
        let output = if Path::new(output_path).is_absolute() {
            Path::new(output_path).to_path_buf()
        } else {
            std::env::current_dir()?.join(output_path)
        };
        let parent = output
            .parent()
            .ok_or_else(|| VideoProcessorError::OutputMissing(output_path.to_string()))?;
        tokio::fs::create_dir_all(parent).await?;
        let assets = output.with_extension(format!("assets-{}", tb_crypto::random_hex_token(8)));
        tokio::fs::create_dir(&assets).await?;
        let result = async {
            tokio::fs::write(assets.join("ddc-logo.png"), LOGO).await?;
            tokio::fs::write(assets.join("DejaVuSans-Bold.ttf"), FONT).await?;
            tokio::fs::write(assets.join("overlay.ass"), ass).await?;
            let base_filter = match plan_vertical_render(layout) {
                VerticalRender::Compose { filter, .. } => filter,
                VerticalRender::BlurPad => build_fallback_filter(),
            };
            let filter = format!(
                "{};[1:v]scale=120:120[logo];[vout][logo]overlay=48:300:shortest=1[branded];[branded]ass=overlay.ass:fontsdir=.[final]",
                base_filter
            );
            let mut cmd = tokio::process::Command::new(&self.ffmpeg);
            cmd.current_dir(&assets)
                .args(["-hide_banner", "-loglevel", "error", "-i"])
                .arg(&input)
                .args(["-loop", "1", "-i", "ddc-logo.png", "-filter_complex", &filter,
                    "-map", "[final]", "-map", "0:a?", "-t"])
                .arg(max_duration.max(1).to_string());
            encode_options(&mut cmd);
            let result = cmd.arg("-y").arg(&output).output().await?;
            if !result.status.success() {
                return Err(VideoProcessorError::Ffmpeg(String::from_utf8_lossy(&result.stderr).trim().to_string()));
            }
            ensure_output(output.to_str().unwrap_or(output_path))
        }
        .await;
        if let Err(error) = tokio::fs::remove_dir_all(&assets).await {
            tracing::warn!(%error, path = %assets.display(), "Render-Assets konnten nicht gelöscht werden");
        }
        result
    }

    /// Liest Breite/Höhe/Dauer via ffprobe.
    pub async fn get_video_info(&self, video_path: &str) -> Result<VideoInfo, VideoProcessorError> {
        let output = tokio::process::Command::new(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,duration,r_frame_rate",
                "-of",
                "json",
                video_path,
            ])
            .output()
            .await?;
        if !output.status.success() {
            return Err(VideoProcessorError::Ffprobe(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let data: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| VideoProcessorError::Parse(e.to_string()))?;
        let stream = data
            .get("streams")
            .and_then(|s| s.get(0))
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
            .unwrap_or(0.0);
        let aspect_ratio = if height > 0 {
            width as f64 / height as f64
        } else {
            0.0
        };
        Ok(VideoInfo {
            width,
            height,
            duration,
            aspect_ratio,
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
        let mut cmd = tokio::process::Command::new(&self.ffmpeg);
        cmd.args([
            "-i",
            input_path,
            "-filter_complex",
            &filter_graph,
            "-map",
            "[vout]",
            "-map",
            "0:a?",
        ]);
        encode_options(&mut cmd);
        let output = cmd.args(["-y", output_path]).output().await?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(VideoProcessorError::Ffmpeg(if err.is_empty() {
                "ffmpeg composition failed".to_string()
            } else {
                err
            }));
        }
        ensure_output(output_path)
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
        let mut cmd = tokio::process::Command::new(&self.ffmpeg);
        cmd.args(["-i", input_path, "-vf", &filter]);
        encode_options(&mut cmd);
        let output = cmd.args(["-y", output_path]).output().await?;
        if !output.status.success() {
            return Err(VideoProcessorError::Ffmpeg(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        ensure_output(output_path)
    }

    /// Brennt eine ASS-Untertiteldatei ins Video. Der `subtitles`-Filter ist bei
    /// Pfaden mit Sonderzeichen (Doppelpunkt) fragil; deshalb laeuft ffmpeg im
    /// Verzeichnis der ASS-Datei und bekommt nur den Dateinamen.
    pub async fn burn_subtitles(
        &self,
        input_path: &str,
        output_path: &str,
        ass_path: &str,
    ) -> Result<(), VideoProcessorError> {
        let ass = Path::new(ass_path);
        let dir = ass.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = ass
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| VideoProcessorError::OutputMissing(ass_path.to_string()))?;
        let input_abs = std::fs::canonicalize(input_path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| input_path.to_string());
        // Sowohl Eingang als auch Ausgang muessen absolut sein: ffmpeg laeuft im
        // ASS-Ordner (gegen Pfad-Sonderzeichen im subtitles-Filter), ein relativer
        // Ausgabepfad wuerde sonst gegen dieses Verzeichnis aufgeloest.
        let output_abs = if Path::new(output_path).is_absolute() {
            output_path.to_string()
        } else {
            std::env::current_dir()
                .map(|c| c.join(output_path).to_string_lossy().into_owned())
                .unwrap_or_else(|_| output_path.to_string())
        };
        let mut cmd = tokio::process::Command::new(&self.ffmpeg);
        cmd.current_dir(&dir)
            .args(["-i", &input_abs, "-vf", &format!("subtitles={name}")]);
        encode_options(&mut cmd);
        let output = cmd.args(["-y", &output_abs]).output().await?;
        if !output.status.success() {
            return Err(VideoProcessorError::Ffmpeg(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        ensure_output(&output_abs)
    }

    /// Schneidet das Video auf `max_duration` Sekunden (oder kopiert es, wenn
    /// bereits kürzer).
    pub async fn trim_video(
        &self,
        input_path: &str,
        output_path: &str,
        max_duration: i64,
    ) -> Result<(), VideoProcessorError> {
        let info = self.get_video_info(input_path).await?;
        if info.duration <= max_duration as f64 {
            tokio::fs::copy(input_path, output_path).await?;
            return Ok(());
        }
        let output = tokio::process::Command::new(&self.ffmpeg)
            .args([
                "-i",
                input_path,
                "-t",
                &max_duration.to_string(),
                "-c",
                "copy",
                "-y",
                output_path,
            ])
            .output()
            .await?;
        if !output.status.success() {
            return Err(VideoProcessorError::Ffmpeg(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
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

    /// Wie [`Self::convert_and_trim`], aber layout-bewusst: erst auf
    /// `max_duration` schneiden (falls nötig), dann per [`Self::compose_vertical`]
    /// das Streamer-Layout (Game + optional Facecam) ins Hochformat rendern.
    pub async fn compose_and_trim(
        &self,
        input_path: &str,
        output_path: &str,
        max_duration: i64,
        layout: &StreamerLayout,
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
        self.compose_vertical(
            &temp_path,
            output_path,
            layout,
            &layout.mode,
            layout.cam_enabled,
        )
        .await?;
        if temp_path != input_path {
            let _ = tokio::fs::remove_file(&temp_path).await;
        }
        Ok(())
    }
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

fn encode_options(cmd: &mut tokio::process::Command) {
    cmd.args([
        "-c:v",
        "libx264",
        "-preset",
        VIDEO_PRESET,
        "-crf",
        VIDEO_CRF,
        "-profile:v",
        VIDEO_PROFILE,
        "-pix_fmt",
        PIXEL_FORMAT,
        "-fpsmax",
        MAX_FPS,
        "-c:a",
        "aac",
        "-b:a",
        AUDIO_BITRATE,
        "-ar",
        AUDIO_SAMPLE_RATE,
        "-af",
        "loudnorm",
        "-movflags",
        "+faststart",
    ]);
}

fn ensure_output(output_path: &str) -> Result<(), VideoProcessorError> {
    if Path::new(output_path).exists() {
        Ok(())
    } else {
        Err(VideoProcessorError::OutputMissing(output_path.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{default_streamer_layout, LayoutBox, DEFAULT_PIP_TILE};

    fn pip_layout() -> StreamerLayout {
        let mut layout = default_streamer_layout();
        layout.mode = "pip".to_string();
        layout.game_crop = LayoutBox {
            x: 0,
            y: 0,
            w: 1080,
            h: 1080,
        };
        layout.cam_position = DEFAULT_PIP_TILE;
        layout
    }

    #[test]
    fn compose_filter_cam_off() {
        let f = build_compose_filter(&pip_layout(), "pip", false);
        assert_eq!(f, build_fallback_filter());
    }

    #[test]
    fn compose_filter_pip() {
        // Default-cam_position ist die Kachel rechts oben: 320x320 bei (712,48).
        let f = build_compose_filter(&pip_layout(), "pip", true);
        let parts: Vec<&str> = f.split(';').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].ends_with("setsar=1[gamefull]"));
        assert_eq!(
            parts[1],
            "[0:v]crop=380:380:1500:50,scale=320:320:force_original_aspect_ratio=increase,crop=320:320,setsar=1[cam]"
        );
        assert_eq!(parts[2], "[gamefull][cam]overlay=712:48[vout]");
    }

    #[test]
    fn compose_filter_pip_folgt_cam_position() {
        // Frei gesetztes Zielrechteck: Groesse UND Position kommen aus cam_position.
        let mut layout = pip_layout();
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
            "[0:v]crop=380:380:1500:50,scale=420:560:force_original_aspect_ratio=increase,crop=420:560,setsar=1[cam]"
        );
        assert_eq!(parts[2], "[gamefull][cam]overlay=60:1200[vout]");
        // Keine festen 320/48-Reste mehr im Graphen.
        assert!(!f.contains("W-w-"));
    }

    #[test]
    fn compose_filter_pip_clampt_in_den_zielframe() {
        // Defensiv: ein Altlayout, das ueber den Rand ragt, darf keinen
        // ffmpeg-Graphen erzeugen, der ausserhalb des Frames landet.
        let mut layout = pip_layout();
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
        let mut layout = pip_layout();
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
    fn compose_filter_blur_pad_hat_boxblur_und_zentriertes_overlay() {
        // Blur-Rand: 16:9-Bild mittig, oben/unten eine verschwommene, vergroesserte
        // Kopie. Kein separates Cam-Tile.
        let f = build_compose_filter(&default_streamer_layout(), "blur_pad", true);
        assert!(
            f.contains("boxblur"),
            "blur_pad braucht einen Blur-Hintergrund: {f}"
        );
        assert!(
            f.contains("split"),
            "Hintergrund und Vordergrund aus einer Quelle: {f}"
        );
        assert!(
            f.contains("overlay=(W-w)/2:(H-h)/2"),
            "16:9-Bild sitzt zentriert im Frame: {f}"
        );
        assert!(f.ends_with("[vout]"), "{f}");
        assert!(
            !f.contains("crop=380:380"),
            "im Blur-Rand gibt es keine Cam-Kachel: {f}"
        );
        // cam_enabled darf am Ergebnis nichts aendern.
        assert_eq!(
            build_compose_filter(&default_streamer_layout(), "blur_pad", false),
            f
        );
    }

    #[test]
    fn compose_filter_stacked() {
        let f = build_compose_filter(&default_streamer_layout(), "stacked", true);
        assert!(
            f.contains("scale=1080:600:force_original_aspect_ratio=increase"),
            "{f}"
        );
        assert!(f.contains("crop=882:1080:519:0"), "{f}");
        assert!(
            f.contains("scale=1080:1320:force_original_aspect_ratio=increase"),
            "{f}"
        );
        assert!(!f.contains("boxblur"), "{f}");
        assert!(f.contains("vstack=inputs=2"), "{f}");
        assert!(
            f.contains("drawbox=x=0:y=600:w=iw:h=8:color=0xC5A059"),
            "{f}"
        );
        assert!(f.ends_with("[vout]"));
    }

    #[test]
    fn compose_filter_stacked_nutzt_game_crop() {
        let mut layout = default_streamer_layout();
        layout.game_crop = LayoutBox {
            x: 400,
            y: 0,
            w: 1000,
            h: 1080,
        };
        let filter = build_compose_filter(&layout, "stacked", true);
        assert!(filter.contains("crop=882:1080:459:0"), "{filter}");
    }

    #[test]
    fn compose_filter_stacked_meidet_cam_im_gameplay() {
        let mut layout = default_streamer_layout();
        layout.cam_crop = LayoutBox {
            x: 700,
            y: 0,
            w: 350,
            h: 500,
        };
        let crop = stacked_game_crop(&layout, 1320);
        assert!(!crops_overlap(crop, layout.cam_crop), "{crop:?}");
        assert!((crop.x + crop.w / 2 - 960).abs() < 200, "{crop:?}");
        let filter = build_compose_filter(&layout, "stacked", true);
        assert!(filter.contains(&format!("crop={}:{}:{}:{}", crop.w, crop.h, crop.x, crop.y)));
        assert!(!filter.contains("boxblur"));
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
        let mut same_height = default_streamer_layout();
        same_height.cam_position.h = 320;
        assert_eq!(
            build_compose_filter(&layout, "stacked", true),
            build_compose_filter(&same_height, "stacked", true)
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

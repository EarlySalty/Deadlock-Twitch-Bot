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

use crate::layout::{StreamerLayout, TARGET_HEIGHT, TARGET_WIDTH};

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

    let base_game = format!(
        "[0:v]crop={gw}:{gh}:{gx}:{gy},\
         scale={tw}:{th}:force_original_aspect_ratio=increase,\
         crop={tw}:{th},setsar=1[gamefull]",
        gw = g.w, gh = g.h, gx = g.x, gy = g.y, tw = TARGET_WIDTH, th = TARGET_HEIGHT
    );

    if !cam_enabled {
        return base_game.replace("[gamefull]", "[vout]");
    }

    if mode == "stacked" {
        let cam = format!(
            "[0:v]crop={cw}:{ch}:{cx}:{cy},\
             scale={tw}:{top}:force_original_aspect_ratio=increase,\
             crop={tw}:{top},setsar=1[cam]",
            cw = c.w, ch = c.h, cx = c.x, cy = c.y, tw = TARGET_WIDTH, top = top_height
        );
        let game = format!(
            "[0:v]crop={gw}:{gh}:{gx}:{gy},\
             scale={tw}:{gh2}:force_original_aspect_ratio=increase,\
             crop={tw}:{gh2},setsar=1[game]",
            gw = g.w, gh = g.h, gx = g.x, gy = g.y, tw = TARGET_WIDTH, gh2 = game_height
        );
        return [cam, game, "[cam][game]vstack=inputs=2[vout]".to_string()].join(";");
    }

    // PiP: die Cam landet exakt auf dem Zielrechteck aus cam_position.
    let cam = format!(
        "[0:v]crop={cw}:{ch}:{cx}:{cy},\
         scale={pw}:{ph}:force_original_aspect_ratio=increase,\
         crop={pw}:{ph},setsar=1[cam]",
        cw = c.w, ch = c.h, cx = c.x, cy = c.y, pw = p.w, ph = p.h
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
        Self { ffmpeg: "ffmpeg".to_string(), ffprobe: "ffprobe".to_string() }
    }
}

impl VideoProcessor {
    pub fn new(ffmpeg_path: impl Into<String>, ffprobe_path: impl Into<String>) -> Self {
        Self { ffmpeg: ffmpeg_path.into(), ffprobe: ffprobe_path.into() }
    }

    /// Liest Breite/Höhe/Dauer via ffprobe.
    pub async fn get_video_info(&self, video_path: &str) -> Result<VideoInfo, VideoProcessorError> {
        let output = tokio::process::Command::new(&self.ffprobe)
            .args([
                "-v", "error", "-select_streams", "v:0", "-show_entries",
                "stream=width,height,duration,r_frame_rate", "-of", "json", video_path,
            ])
            .output()
            .await?;
        if !output.status.success() {
            return Err(VideoProcessorError::Ffprobe(String::from_utf8_lossy(&output.stderr).trim().to_string()));
        }
        let data: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| VideoProcessorError::Parse(e.to_string()))?;
        let stream = data.get("streams").and_then(|s| s.get(0))
            .ok_or_else(|| VideoProcessorError::Parse("no video stream".to_string()))?;
        let width = num_field(stream, "width").ok_or_else(|| VideoProcessorError::Parse("width".to_string()))?;
        let height = num_field(stream, "height").ok_or_else(|| VideoProcessorError::Parse("height".to_string()))?;
        let duration = stream.get("duration")
            .and_then(|d| d.as_f64().or_else(|| d.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);
        let aspect_ratio = if height > 0 { width as f64 / height as f64 } else { 0.0 };
        Ok(VideoInfo { width, height, duration, aspect_ratio })
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
        let resolved_mode = if mode.trim().is_empty() { layout.mode.clone() } else { mode.to_string() };
        let filter_graph = build_compose_filter(layout, resolved_mode.trim().to_lowercase().as_str(), cam_enabled);
        let output = tokio::process::Command::new(&self.ffmpeg)
            .args([
                "-i", input_path, "-filter_complex", &filter_graph,
                "-map", "[vout]", "-map", "0:a?",
                "-c:v", "libx264", "-preset", "medium", "-crf", "23",
                "-c:a", "aac", "-af", "loudnorm", "-movflags", "+faststart", "-y", output_path,
            ])
            .output()
            .await?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(VideoProcessorError::Ffmpeg(if err.is_empty() { "ffmpeg composition failed".to_string() } else { err }));
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
            build_crop_filter(info.width, info.height, target_width, target_height, crop_mode)
        } else {
            format!("scale={target_width}:{target_height}")
        };
        let output = tokio::process::Command::new(&self.ffmpeg)
            .args([
                "-i", input_path, "-vf", &filter,
                "-c:v", "libx264", "-preset", "medium", "-crf", "23",
                "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart", "-y", output_path,
            ])
            .output()
            .await?;
        if !output.status.success() {
            return Err(VideoProcessorError::Ffmpeg(String::from_utf8_lossy(&output.stderr).trim().to_string()));
        }
        ensure_output(output_path)
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
            .args(["-i", input_path, "-t", &max_duration.to_string(), "-c", "copy", "-y", output_path])
            .output()
            .await?;
        if !output.status.success() {
            return Err(VideoProcessorError::Ffmpeg(String::from_utf8_lossy(&output.stderr).trim().to_string()));
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
            temp_path = Path::new(output_path).with_extension("temp.mp4").to_string_lossy().into_owned();
            self.trim_video(input_path, &temp_path, max_duration).await?;
        }
        self.convert_to_vertical(&temp_path, output_path, target_width, target_height, "center").await?;
        if temp_path != input_path {
            let _ = tokio::fs::remove_file(&temp_path).await;
        }
        Ok(())
    }
}

/// `#tag`-Liste, leere Tags werden übersprungen (mirror `format_hashtags`).
pub fn format_hashtags(hashtags: &[String]) -> String {
    hashtags.iter().filter(|t| !t.is_empty()).map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")
}

fn num_field(stream: &Value, key: &str) -> Option<i64> {
    let v = stream.get(key)?;
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
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
    use crate::layout::{default_streamer_layout, LayoutBox};

    #[test]
    fn compose_filter_cam_off() {
        let f = build_compose_filter(&default_streamer_layout(), "pip", false);
        assert_eq!(
            f,
            "[0:v]crop=1080:1080:0:0,scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,setsar=1[vout]"
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
            "[0:v]crop=380:380:1500:50,scale=320:320:force_original_aspect_ratio=increase,crop=320:320,setsar=1[cam]"
        );
        assert_eq!(parts[2], "[gamefull][cam]overlay=712:48[vout]");
    }

    #[test]
    fn compose_filter_pip_folgt_cam_position() {
        // Frei gesetztes Zielrechteck: Groesse UND Position kommen aus cam_position.
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox { x: 60, y: 1200, w: 420, h: 560 };
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
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox { x: 900, y: 1800, w: 600, h: 400 };
        let f = build_compose_filter(&layout, "pip", true);
        let parts: Vec<&str> = f.split(';').collect();
        assert_eq!(parts[2], "[gamefull][cam]overlay=480:1520[vout]");
    }

    #[test]
    fn compose_filter_erzwingt_gerade_kantenlaengen() {
        // Ungerade Maße im Filtergraphen lassen libx264 mit yuv420p abbrechen.
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox { x: 60, y: 1200, w: 421, h: 561 };
        let f = build_compose_filter(&layout, "pip", true);
        assert!(f.contains("scale=420:560:"), "{f}");
        assert!(f.contains("crop=420:560,"), "{f}");

        // Auch der Streifen und die Restflaeche darunter bleiben gerade.
        layout.cam_position = LayoutBox { x: 0, y: 0, w: 1080, h: 541 };
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
            "[0:v]crop=380:380:1500:50,scale=1080:320:force_original_aspect_ratio=increase,crop=1080:320,setsar=1[cam]"
        );
        assert_eq!(
            parts[1],
            "[0:v]crop=1080:1080:0:0,scale=1080:1600:force_original_aspect_ratio=increase,crop=1080:1600,setsar=1[game]"
        );
        assert_eq!(parts[2], "[cam][game]vstack=inputs=2[vout]");
    }

    #[test]
    fn compose_filter_stacked_ignoriert_x_y_w() {
        // x/y/w sind im Streifen-Modus bewusst wirkungslos: der Streifen sitzt
        // immer oben und ist immer 1080 breit.
        let mut layout = default_streamer_layout();
        layout.cam_position = LayoutBox { x: 333, y: 777, w: 444, h: 320 };
        assert_eq!(
            build_compose_filter(&layout, "stacked", true),
            build_compose_filter(&default_streamer_layout(), "stacked", true)
        );
    }

    #[test]
    fn crop_filter_landscape_modi() {
        // 1920x1080 → 1080x1920, target_ratio=0.5625 → crop_w=607.
        assert_eq!(build_crop_filter(1920, 1080, 1080, 1920, "center"), "crop=607:1080:656:0,scale=1080:1920");
        assert_eq!(build_crop_filter(1920, 1080, 1080, 1920, "left"), "crop=607:1080:0:0,scale=1080:1920");
        assert_eq!(build_crop_filter(1920, 1080, 1080, 1920, "right"), "crop=607:1080:1313:0,scale=1080:1920");
    }

    #[test]
    fn crop_filter_portrait_modi() {
        // 720x1280 (ratio == target) → höher-Branch, crop_h=1280.
        assert_eq!(build_crop_filter(720, 1280, 1080, 1920, "center"), "crop=720:1280:0:0,scale=1080:1920");
        // 1080x2400 → höher als target → oben/unten beschneiden.
        assert_eq!(build_crop_filter(1080, 2400, 1080, 1920, "top"), "crop=1080:1920:0:0,scale=1080:1920");
        assert_eq!(build_crop_filter(1080, 2400, 1080, 1920, "bottom"), "crop=1080:1920:0:480,scale=1080:1920");
    }

    #[test]
    fn format_hashtags_skip_leer() {
        assert_eq!(format_hashtags(&["deadlock".into(), "".into(), "haze".into()]), "#deadlock #haze");
        assert_eq!(format_hashtags(&[]), "");
    }
}

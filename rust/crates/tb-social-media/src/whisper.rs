//! OpenAI-Whisper-Transcriber (Port von `social_media/transcription/whisper.py`,
//! `openai_api`-Engine).
//!
//! Pipeline: `ffmpeg` extrahiert 16 kHz Mono-WAV aus dem Clip → die OpenAI
//! `audio/transcriptions`-API (verbose_json) liefert Text + Segmente + Sprache.
//! Die lokale `faster_whisper`-Engine (ctranslate2-Python-Lib) ist kein
//! Rust-Port; `openai_api` erzeugt dasselbe Ergebnis per HTTP. Der `none`-Modus
//! = kein Transcriber injiziert (Pipeline überspringt die Stage).

use std::path::Path;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::enrich_pipeline::{TranscribeError, Transcriber, TranscriptionOutput};

const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const DEFAULT_MODEL: &str = "whisper-1";

/// OpenAI-Whisper-Transcriber.
pub struct OpenAiTranscriber {
    api_key: String,
    model: String,
    base_url: String,
    ffmpeg_bin: String,
    http: reqwest::Client,
}

fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

impl OpenAiTranscriber {
    /// Aus Env: `OPENAI_API_KEY` (Pflicht → `None` ohne Key = `none`-Modus),
    /// `OPENAI_WHISPER_MODEL` (Default `whisper-1`), `FFMPEG_BIN`.
    pub fn from_env() -> Option<Self> {
        let api_key = nonempty_env("OPENAI_API_KEY")?;
        Some(Self {
            api_key,
            model: nonempty_env("OPENAI_WHISPER_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            base_url: DEFAULT_OPENAI_URL.to_string(),
            ffmpeg_bin: nonempty_env("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".to_string()),
            http: reqwest::Client::new(),
        })
    }

    /// ffmpeg → 16 kHz Mono PCM-WAV (Python `_extract_audio`).
    async fn extract_audio(&self, video_path: &str, wav_path: &str) -> Result<(), TranscribeError> {
        let output = tokio::process::Command::new(&self.ffmpeg_bin)
            .args(["-y", "-i", video_path, "-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", wav_path])
            .output()
            .await
            .map_err(|e| TranscribeError::Unavailable(format!("ffmpeg nicht startbar: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TranscribeError::Failed(format!("ffmpeg-Audio-Extraktion fehlgeschlagen: {}", truncate(&stderr, 300))));
        }
        Ok(())
    }

    /// Lädt das WAV hoch und parst die verbose_json-Antwort (Text + Segmente +
    /// Sprache).
    async fn transcribe_wav(&self, wav_path: &str) -> Result<TranscriptionOutput, TranscribeError> {
        let bytes = tokio::fs::read(wav_path).await.map_err(|e| TranscribeError::Failed(format!("WAV nicht lesbar: {e}")))?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| TranscribeError::Failed(e.to_string()))?;
        let form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .text("response_format", "verbose_json")
            .text("timestamp_granularities[]", "segment")
            .part("file", part);
        let resp = self
            .http
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| TranscribeError::Failed(e.to_string()))?
            .error_for_status()
            .map_err(|e| TranscribeError::Failed(e.to_string()))?;
        let payload: Value = resp.json().await.map_err(|e| TranscribeError::Failed(e.to_string()))?;

        let text = payload.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let language = payload.get("language").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let segments: Vec<Value> = payload
            .get("segments")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|seg| {
                        let seg_text = seg.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
                        if seg_text.is_empty() {
                            return None;
                        }
                        Some(json!({
                            "start": seg.get("start").and_then(Value::as_f64).unwrap_or(0.0),
                            "end": seg.get("end").and_then(Value::as_f64).unwrap_or(0.0),
                            "text": seg_text,
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(TranscriptionOutput { text, segments, language })
    }
}

#[async_trait]
impl Transcriber for OpenAiTranscriber {
    async fn transcribe_clip(&self, video_path: &Path) -> Result<TranscriptionOutput, TranscribeError> {
        if !video_path.exists() {
            return Err(TranscribeError::NotFound(video_path.display().to_string()));
        }
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let wav = std::env::temp_dir().join(format!("tb-sm-whisper-{}-{}.wav", std::process::id(), nanos));
        let wav_str = wav.to_string_lossy().into_owned();
        self.extract_audio(&video_path.to_string_lossy(), &wav_str).await?;
        let result = self.transcribe_wav(&wav_str).await;
        let _ = tokio::fs::remove_file(&wav).await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn transcriber(base: &str) -> OpenAiTranscriber {
        OpenAiTranscriber {
            api_key: "k".to_string(),
            model: "whisper-1".to_string(),
            base_url: base.to_string(),
            ffmpeg_bin: "ffmpeg".to_string(),
            http: reqwest::Client::new(),
        }
    }

    #[tokio::test]
    async fn transcribe_wav_parst_text_segments_language() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "text": "  Haze ist stark  ",
                "language": "german",
                "duration": 12.5,
                "segments": [
                    { "start": 0.0, "end": 2.0, "text": " Haze " },
                    { "start": 2.0, "end": 4.0, "text": "   " },
                    { "start": 4.0, "end": 6.0, "text": "ist stark" }
                ]
            })))
            .mount(&server)
            .await;

        let wav = std::env::temp_dir().join("tb_whisper_test.wav");
        tokio::fs::write(&wav, b"RIFFfake").await.unwrap();
        let out = transcriber(&format!("{}/v1/audio/transcriptions", server.uri())).transcribe_wav(&wav.to_string_lossy()).await.unwrap();
        assert_eq!(out.text, "Haze ist stark"); // getrimmt
        assert_eq!(out.language.as_deref(), Some("german"));
        // Leeres Segment übersprungen → 2 Segmente, getrimmt.
        assert_eq!(out.segments.len(), 2);
        assert_eq!(out.segments[0]["text"], "Haze");
        assert_eq!(out.segments[0]["start"], 0.0);
        assert_eq!(out.segments[1]["text"], "ist stark");
    }

    #[tokio::test]
    async fn transcribe_clip_fehlende_datei_ist_notfound() {
        let t = transcriber("http://127.0.0.1:1/x");
        let err = t.transcribe_clip(Path::new("/nope/missing.mp4")).await.unwrap_err();
        assert!(matches!(err, TranscribeError::NotFound(_)));
    }
}

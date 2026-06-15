//! Stream-Transkription für den Engagement-Layer (Port des `openai_api`-Pfads
//! aus `bot/social_media/transcription/whisper.py`, beschränkt auf das, was der
//! Engagement-Transkript-Loop nutzt).
//!
//! Pipeline: `ffmpeg` extrahiert 16 kHz Mono-WAV aus dem Capture-Clip → die
//! OpenAI-Whisper-API (`audio/transcriptions`, HTTP-Multipart) transkribiert.
//! Der Engagement-Pfad nutzt nur Text + Dauer (keine Segment-Persistenz, keine
//! Vokabular-Korrektur — die gehören zur Social-Media-Pipeline).

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const DEFAULT_MODEL: &str = "whisper-1";

/// Transkriptions-Ergebnis (Teilmenge von Pythons `TranscriptionResult`, die der
/// Engagement-Loop verwendet).
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
    pub duration_seconds: f64,
    pub engine: String,
    pub model: String,
}

/// ffmpeg-Extraktion + OpenAI-Whisper-API.
pub struct OpenAiTranscriber {
    api_key: String,
    model: String,
    base_url: String,
    ffmpeg_bin: String,
    http: reqwest::Client,
}

impl OpenAiTranscriber {
    /// Aus Env: `OPENAI_API_KEY` (Pflicht), `OPENAI_WHISPER_MODEL` (Default
    /// `whisper-1`), `FFMPEG_BIN` (Default `ffmpeg`). `None` ohne API-Key
    /// (Python `_OpenAIWhisperEngine` wirft dann `TranscriberUnavailable`).
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

    /// Setzt den API-Endpoint (Tests).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Setzt das ffmpeg-Binary (Tests).
    pub fn with_ffmpeg_bin(mut self, bin: impl Into<String>) -> Self {
        self.ffmpeg_bin = bin.into();
        self
    }

    /// Extrahiert Audio in ein Temp-WAV und transkribiert es (Python
    /// `transcribe_clip`). Das Temp-Verzeichnis wird immer aufgeräumt.
    pub async fn transcribe_clip(&self, video_path: &Path) -> Result<TranscriptionResult, String> {
        let dir = std::env::temp_dir().join(format!("eng-whisper-{}", tb_crypto::random_hex_token(8)));
        tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
        let wav = dir.join("audio.wav");
        let result = match self.extract_audio(video_path, &wav).await {
            Ok(()) => self.transcribe_wav(&wav).await,
            Err(e) => Err(e),
        };
        let _ = tokio::fs::remove_dir_all(&dir).await;
        result
    }

    /// ffmpeg → 16 kHz Mono PCM-WAV (Python `_extract_audio`).
    async fn extract_audio(&self, video: &Path, wav: &Path) -> Result<(), String> {
        let output = Command::new(&self.ffmpeg_bin)
            .arg("-y")
            .arg("-i")
            .arg(video)
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
            .arg(wav)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("ffmpeg nicht startbar: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ffmpeg-Audio-Extraktion fehlgeschlagen: {}", truncate(&stderr, 300)));
        }
        Ok(())
    }

    /// Lädt das WAV hoch und parst die verbose_json-Antwort.
    async fn transcribe_wav(&self, wav: &Path) -> Result<TranscriptionResult, String> {
        let bytes = tokio::fs::read(wav).await.map_err(|e| format!("WAV nicht lesbar: {e}"))?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| e.to_string())?;
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
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let payload: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let text = payload.get("text").and_then(serde_json::Value::as_str).unwrap_or("").trim().to_string();
        let duration = payload.get("duration").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        Ok(TranscriptionResult {
            text,
            duration_seconds: duration,
            engine: "openai_api".to_string(),
            model: self.model.clone(),
        })
    }
}

/// Env-Var nur wenn gesetzt UND nicht leer.
fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Byte-sichere Kürzung für Log-/Fehler-Texte.
fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
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
    async fn transcribe_wav_parst_text_und_dauer() {
        let dir = std::env::temp_dir().join(format!("eng-whisper-test-{}", tb_crypto::random_hex_token(6)));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let wav = dir.join("audio.wav");
        tokio::fs::write(&wav, b"RIFFfake-wav-bytes").await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "  haze ist echt stark grad  ",
                "language": "de",
                "duration": 12.5,
                "segments": [{"start": 0.0, "end": 12.5, "text": "haze ist echt stark grad"}]
            })))
            .mount(&server)
            .await;

        let t = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));
        let result = t.transcribe_wav(&wav).await.unwrap();
        assert_eq!(result.text, "haze ist echt stark grad"); // getrimmt
        assert_eq!(result.duration_seconds, 12.5);
        assert_eq!(result.engine, "openai_api");
        assert_eq!(result.model, "whisper-1");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn transcribe_wav_http_fehler_ist_err() {
        let dir = std::env::temp_dir().join(format!("eng-whisper-test-{}", tb_crypto::random_hex_token(6)));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let wav = dir.join("audio.wav");
        tokio::fs::write(&wav, b"x").await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("no key"))
            .mount(&server)
            .await;

        let t = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));
        assert!(t.transcribe_wav(&wav).await.is_err());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}

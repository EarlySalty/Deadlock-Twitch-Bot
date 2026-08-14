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
use std::time::Duration;

use tokio::process::Command;

/// Transkribiert wird lokal. `ops/stt-server` haelt ein Whisper-Modell im
/// Speicher und spricht dieselbe Schnittstelle; der Default zeigt darum auf ihn
/// und nicht nach draussen. Wer trotzdem OpenAI will, setzt
/// `ENGAGEMENT_STT_BASE_URL` explizit auf deren Endpunkt — sonst geht nie
/// Stream-Audio an einen Fremdanbieter, auch nicht versehentlich, nur weil ein
/// `OPENAI_API_KEY` in der Umgebung steht.
/// Vorgabe-Endpunkt: der lokale `ops/stt-server`.
///
/// Oeffentlich, damit ein Aufrufer denselben Wert pruefen kann, den
/// [`OpenAiTranscriber::from_env`] nimmt - sonst haelt eine Pruefung auf der
/// leeren Umgebungsvariable den Vorgabewert faelschlich fuer auswaertig.
pub const DEFAULT_STT_URL: &str = "http://127.0.0.1:8791/v1/audio/transcriptions";
const DEFAULT_MODEL: &str = "whisper-1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Zeitgrenze der ffmpeg-Extraktion.
///
/// Grosszuegig gewaehlt: ein Zwei-Minuten-Block ist in Sekunden umgewandelt,
/// diese Grenze faengt nur den haengenden Prozess. Ohne sie blockiert eine
/// abgeschnittene Aufnahme den Aufrufer dauerhaft.
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

/// Transkriptions-Ergebnis (Teilmenge von Pythons `TranscriptionResult`, die der
/// Engagement-Loop verwendet).
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
    pub duration_seconds: f64,
    pub engine: String,
    pub model: String,
    /// Zeitstempel aus der `verbose_json`-Antwort, falls der Dienst welche
    /// liefert.
    ///
    /// Ohne sie muessen Aufrufer die Zeit aus dem Textanteil schaetzen - eine
    /// Naeherung, die bei Sprechpausen um Minuten danebenliegt. Angefragt
    /// werden sie ohnehin (`timestamp_granularities[]=segment`); frueher
    /// wurden sie nur weggeworfen.
    pub segments: Vec<TranscriptSegment>,
}

/// Ein Abschnitt mit eigener Zeit, so wie Whisper ihn liefert.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TranscriptSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

/// Klassifizierter Whisper-Fehler ohne Provider-Body, Request oder Secret.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TranscribeError {
    #[error("unavailable")]
    Unavailable,
    #[error("timeout")]
    Timeout,
    #[error("transport")]
    Transport,
    #[error("http_status({0})")]
    HttpStatus(u16),
    #[error("decode")]
    Decode,
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
    /// Aus Env: `OPENAI_WHISPER_MODEL` (Legacy, Default `whisper-1`) und
    /// `FFMPEG_BIN` (Default `ffmpeg`).
    ///
    /// Endpunkt: [`DEFAULT_STT_URL`], also der lokale `ops/stt-server`.
    /// `ENGAGEMENT_STT_BASE_URL` überschreibt ihn. Ein Key wird nur mitgeschickt,
    /// wenn einer da ist; der lokale Dienst ignoriert den
    /// `Authorization`-Header ohnehin, und ein fehlender `OPENAI_API_KEY` darf
    /// die lokale Transkription nicht blockieren.
    pub fn from_env() -> Option<Self> {
        Self::from_env_with_timeout(REQUEST_TIMEOUT)
    }

    /// Wie [`OpenAiTranscriber::from_env`], aber mit eigener Zeitgrenze.
    ///
    /// 60 Sekunden passen zu kurzen Reaktions-Clips. Wer laengere Bloecke
    /// schickt oder auf einen belegten Dienst trifft, braucht mehr: das lokale
    /// Whisper laeuft auf der CPU langsamer als Echtzeit, und eine zu knappe
    /// Grenze bricht den Block ab, statt ihn zu transkribieren.
    pub fn from_env_with_timeout(timeout: Duration) -> Option<Self> {
        Some(Self {
            api_key: nonempty_env("OPENAI_API_KEY").unwrap_or_else(|| "local".to_string()),
            model: nonempty_env("OPENAI_WHISPER_MODEL")
                .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            base_url: nonempty_env("ENGAGEMENT_STT_BASE_URL")
                .unwrap_or_else(|| DEFAULT_STT_URL.to_string()),
            ffmpeg_bin: nonempty_env("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".to_string()),
            http: reqwest::Client::builder()
                .timeout(timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok()?,
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
        let dir =
            std::env::temp_dir().join(format!("eng-whisper-{}", tb_crypto::random_hex_token(8)));
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| e.to_string())?;
        let wav = dir.join("audio.wav");
        let result = match self.extract_audio(video_path, &wav).await {
            Ok(()) => self
                .transcribe_wav(&wav)
                .await
                .map_err(|error| error.to_string()),
            Err(e) => Err(e),
        };
        let _ = tokio::fs::remove_dir_all(&dir).await;
        result
    }

    /// ffmpeg → 16 kHz Mono PCM-WAV (Python `_extract_audio`).
    async fn extract_audio(&self, video: &Path, wav: &Path) -> Result<(), String> {
        let kind = Command::new(&self.ffmpeg_bin)
            .arg("-y")
            .arg("-i")
            .arg(video)
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
            .arg(wav)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("ffmpeg nicht startbar: {e}"))?;
        // Die Zeitgrenze des HTTP-Clients deckt nur die Anfrage an den
        // STT-Dienst ab. Eine abgeschnittene Datei kann ffmpeg dagegen
        // dauerhaft haengen lassen, und der Aufrufer wartet dann fuer immer
        // auf einen Block, der nie fertig wird.
        let output = match tokio::time::timeout(FFMPEG_TIMEOUT, kind.wait_with_output()).await {
            Ok(fertig) => fertig.map_err(|e| format!("ffmpeg nicht lesbar: {e}"))?,
            Err(_) => {
                return Err(format!(
                    "ffmpeg-Audio-Extraktion nach {} Sekunden abgebrochen",
                    FFMPEG_TIMEOUT.as_secs()
                ));
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "ffmpeg-Audio-Extraktion fehlgeschlagen: {}",
                truncate(&stderr, 300)
            ));
        }
        Ok(())
    }

    /// Lädt das WAV hoch und parst die verbose_json-Antwort.
    async fn transcribe_wav(&self, wav: &Path) -> Result<TranscriptionResult, TranscribeError> {
        let bytes = tokio::fs::read(wav)
            .await
            .map_err(|_| TranscribeError::Unavailable)?;
        self.transcribe_bytes_with_model(bytes, &self.model).await
    }

    /// Transkribiert synthetische PCM-WAV-Bytes direkt aus dem Arbeitsspeicher.
    pub async fn transcribe_bytes(
        &self,
        wav_bytes: Vec<u8>,
    ) -> Result<TranscriptionResult, TranscribeError> {
        self.transcribe_bytes_with_model(wav_bytes, DEFAULT_MODEL)
            .await
    }

    async fn transcribe_bytes_with_model(
        &self,
        wav_bytes: Vec<u8>,
        model: &str,
    ) -> Result<TranscriptionResult, TranscribeError> {
        if wav_bytes.is_empty() || wav_bytes.len() > MAX_UPLOAD_BYTES {
            return Err(TranscribeError::Unavailable);
        }
        let part = reqwest::multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|_| TranscribeError::Unavailable)?;
        let form = reqwest::multipart::Form::new()
            .text("model", model.to_owned())
            .text("language", "de")
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
            .map_err(classify_transport_error)?;
        if !resp.status().is_success() {
            return Err(TranscribeError::HttpStatus(resp.status().as_u16()));
        }
        let payload: serde_json::Value = resp.json().await.map_err(classify_decode_error)?;
        let text = payload
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or(TranscribeError::Decode)?
            .trim()
            .to_string();
        let duration = payload
            .get("duration")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        // Entweder alle Zeitstempel oder keine. Einzelne fehlerhafte Eintraege
        // wegzuwerfen hiesse: der Aufrufer haelt die Liste fuer vollstaendig,
        // und genau die weggeworfene Stelle wird nie geprueft.
        let segments = match payload
            .get("segments")
            .and_then(serde_json::Value::as_array)
        {
            Some(roh) => {
                let mut gelesen = Vec::with_capacity(roh.len());
                let mut unvollstaendig = false;
                for eintrag in roh {
                    let text = eintrag
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .map(|s| s.trim().to_string());
                    let start = eintrag.get("start").and_then(serde_json::Value::as_f64);
                    let ende = eintrag.get("end").and_then(serde_json::Value::as_f64);
                    match (text, start, ende) {
                        (Some(text), Some(start_seconds), Some(end_seconds))
                            if !text.is_empty() =>
                        {
                            gelesen.push(TranscriptSegment {
                                start_seconds,
                                end_seconds,
                                text,
                            })
                        }
                        // Ein leerer Text ist normal (Stille zwischen zwei
                        // Saetzen); fehlende Zeiten sind es nicht.
                        (Some(text), _, _) if text.is_empty() => {}
                        _ => unvollstaendig = true,
                    }
                }
                if unvollstaendig {
                    tracing::warn!("Zeitstempel unvollstaendig - es gilt der Volltext");
                    Vec::new()
                } else {
                    gelesen
                }
            }
            None => Vec::new(),
        };
        // Der lokale Dienst ignoriert das angefragte Modell und nennt in der
        // Antwort, was er wirklich geladen hat. Steht dort etwas, gilt das -
        // sonst behauptet der Bericht "whisper-1", wo large-v3-turbo lief.
        let genutztes_modell = payload
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            // Nennt die Antwort kein Modell, bleibt es beim angefragten Namen.
            // Der lokale Dienst ignoriert ihn und laedt, was in seiner eigenen
            // Konfiguration steht - deshalb heisst das Feld im Audit-Bericht
            // ausdruecklich "angefragt".
            .unwrap_or_else(|| model.to_string());
        Ok(TranscriptionResult {
            text,
            duration_seconds: duration,
            engine: "openai_api".to_string(),
            model: genutztes_modell,
            segments,
        })
    }
}

fn classify_transport_error(error: reqwest::Error) -> TranscribeError {
    if error.is_timeout() {
        TranscribeError::Timeout
    } else {
        TranscribeError::Transport
    }
}

fn classify_decode_error(error: reqwest::Error) -> TranscribeError {
    if error.is_timeout() {
        TranscribeError::Timeout
    } else if error.is_decode() {
        TranscribeError::Decode
    } else {
        TranscribeError::Transport
    }
}

/// Env-Var nur wenn gesetzt UND nicht leer.
fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Byte-sichere Kürzung für Log-/Fehler-Texte.
fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn transcriber(base: &str) -> OpenAiTranscriber {
        transcriber_with_timeout(base, Duration::from_secs(60))
    }

    /// Stream-Audio darf nicht aus Versehen bei einem Fremdanbieter landen,
    /// nur weil irgendwo ein `OPENAI_API_KEY` in der Umgebung steht. Ohne
    /// explizite Konfiguration zeigt der Endpunkt auf localhost.
    #[test]
    fn default_endpunkt_ist_lokal() {
        assert!(DEFAULT_STT_URL.starts_with("http://127.0.0.1:"));
        if std::env::var("ENGAGEMENT_STT_BASE_URL").is_err() {
            let t = OpenAiTranscriber::from_env().expect("Client baubar");
            assert_eq!(t.base_url, DEFAULT_STT_URL);
        }
    }

    fn transcriber_with_timeout(base: &str, timeout: Duration) -> OpenAiTranscriber {
        OpenAiTranscriber {
            api_key: "test-key".to_string(),
            model: DEFAULT_MODEL.to_string(),
            base_url: base.to_string(),
            ffmpeg_bin: "ffmpeg".to_string(),
            http: reqwest::Client::builder()
                .timeout(timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        }
    }

    async fn successful_server(text: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": text,
                "duration": 12.5
            })))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn transcribe_wav_parst_text_und_dauer() {
        let dir = std::env::temp_dir().join(format!(
            "eng-whisper-test-{}",
            tb_crypto::random_hex_token(6)
        ));
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
    async fn transcribe_wav_behaelt_das_legacy_modell() {
        let dir = std::env::temp_dir().join(format!(
            "eng-whisper-test-{}",
            tb_crypto::random_hex_token(6)
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let wav = dir.join("audio.wav");
        tokio::fs::write(&wav, b"RIFFfake-wav-bytes").await.unwrap();
        let server = successful_server("legacy").await;
        let mut transcriber = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));
        transcriber.model = "legacy-whisper-model".to_string();

        let result = transcriber.transcribe_wav(&wav).await.unwrap();

        assert_eq!(result.model, "legacy-whisper-model");
        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        assert!(body.contains("name=\"model\"\r\n\r\nlegacy-whisper-model\r\n"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn transcribe_wav_http_fehler_ist_err() {
        let dir = std::env::temp_dir().join(format!(
            "eng-whisper-test-{}",
            tb_crypto::random_hex_token(6)
        ));
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
        assert_eq!(
            t.transcribe_wav(&wav).await.unwrap_err(),
            TranscribeError::HttpStatus(401)
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn transcribe_bytes_sendet_whisper_1_und_language_de() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "synthetisches transkript",
                "duration": 1.25
            })))
            .expect(1)
            .mount(&server)
            .await;
        let transcriber = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));

        let result = transcriber
            .transcribe_bytes(b"RIFF....WAVE".to_vec())
            .await
            .unwrap();

        assert_eq!(result.model, "whisper-1");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        let content_type_is_multipart = request
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("multipart/form-data; boundary="));
        assert!(
            content_type_is_multipart,
            "Content-Type ist nicht Multipart"
        );
        let body = String::from_utf8_lossy(&request.body);
        assert!(
            body.contains("name=\"file\"; filename=\"audio.wav\""),
            "WAV-Dateiname fehlt"
        );
        assert!(body.contains("Content-Type: audio/wav"), "WAV-MIME fehlt");
        assert!(
            body.contains("name=\"model\"\r\n\r\nwhisper-1\r\n"),
            "Whisper-Modell fehlt"
        );
        assert!(
            body.contains("name=\"language\"\r\n\r\nde\r\n"),
            "Sprachvorgabe fehlt"
        );
    }

    #[tokio::test]
    async fn transcribe_bytes_leerer_text_bleibt_leer() {
        let server = successful_server("  \n\t ").await;
        let transcriber = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));

        let result = transcriber
            .transcribe_bytes(b"RIFF....WAVE".to_vec())
            .await
            .unwrap();

        assert!(result.text.is_empty());
    }

    #[tokio::test]
    async fn transcribe_bytes_timeout_ist_klassifiziert() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(200))
                    .set_body_json(serde_json::json!({"text": "zu spaet"})),
            )
            .mount(&server)
            .await;
        let transcriber = transcriber_with_timeout(
            &format!("{}/v1/audio/transcriptions", server.uri()),
            Duration::from_millis(20),
        );

        assert_eq!(
            transcriber
                .transcribe_bytes(b"RIFF....WAVE".to_vec())
                .await
                .unwrap_err(),
            TranscribeError::Timeout
        );
    }

    #[tokio::test]
    async fn transcribe_bytes_status_ist_klassifiziert_und_sanitized() {
        for status in [401_u16, 429, 500] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/audio/transcriptions"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_string("SENSITIVE_PROVIDER_BODY_MARKER"),
                )
                .mount(&server)
                .await;
            let transcriber = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));

            let err = transcriber
                .transcribe_bytes(b"RIFF....WAVE".to_vec())
                .await
                .unwrap_err();

            assert_eq!(err, TranscribeError::HttpStatus(status));
            assert!(!err.to_string().contains("SENSITIVE_PROVIDER_BODY_MARKER"));
            assert!(!format!("{err:?}").contains("SENSITIVE_PROVIDER_BODY_MARKER"));
        }
    }

    #[tokio::test]
    async fn transcribe_bytes_folgt_keinen_redirects() {
        let target = successful_server("darf nicht erreicht werden").await;
        let source = MockServer::start().await;
        let target_url = format!("{}/v1/audio/transcriptions", target.uri());
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(307).insert_header("Location", target_url.as_str()))
            .mount(&source)
            .await;
        let transcriber = transcriber(&format!("{}/v1/audio/transcriptions", source.uri()));

        let result = transcriber.transcribe_bytes(b"RIFF....WAVE".to_vec()).await;

        assert_eq!(result.unwrap_err(), TranscribeError::HttpStatus(307));
        assert_eq!(target.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn transcribe_bytes_decode_fehler_ist_klassifiziert() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("kein json und kein provider-body im fehler"),
            )
            .mount(&server)
            .await;
        let transcriber = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));

        assert_eq!(
            transcriber
                .transcribe_bytes(b"RIFF....WAVE".to_vec())
                .await
                .unwrap_err(),
            TranscribeError::Decode
        );
    }

    #[tokio::test]
    async fn transcribe_bytes_ohne_textfeld_ist_decode_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"duration": 1.0})),
            )
            .mount(&server)
            .await;
        let transcriber = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));

        assert_eq!(
            transcriber
                .transcribe_bytes(b"RIFF....WAVE".to_vec())
                .await
                .unwrap_err(),
            TranscribeError::Decode
        );
    }

    #[tokio::test]
    async fn transcribe_bytes_25_mb_cap_greift_vor_request() {
        let server = successful_server("darf nicht aufgerufen werden").await;
        let transcriber = transcriber(&format!("{}/v1/audio/transcriptions", server.uri()));

        let err = transcriber
            .transcribe_bytes(vec![0; 25 * 1024 * 1024 + 1])
            .await
            .unwrap_err();

        assert_eq!(err, TranscribeError::Unavailable);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

#[cfg(test)]
mod tests_segmente {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn antwort_mit(payload: serde_json::Value) -> TranscriptionResult {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload))
            .mount(&server)
            .await;
        let transcriber = OpenAiTranscriber {
            api_key: "local".into(),
            model: "whisper-1".into(),
            base_url: format!("{}/v1/audio/transcriptions", server.uri()),
            ffmpeg_bin: "ffmpeg".into(),
            http: reqwest::Client::new(),
        };
        transcriber
            .transcribe_bytes(b"nicht wirklich wav".to_vec())
            .await
            .expect("Antwort")
    }

    #[tokio::test]
    async fn zeitstempel_werden_uebernommen() {
        let ergebnis = antwort_mit(serde_json::json!({
            "text": "Erster Satz. Zweiter Satz.",
            "duration": 12.0,
            "model": "large-v3-turbo-ct2",
            "segments": [
                {"start": 0.0, "end": 4.0, "text": "Erster Satz."},
                {"start": 4.0, "end": 12.0, "text": "Zweiter Satz."}
            ]
        }))
        .await;
        assert_eq!(ergebnis.segments.len(), 2);
        assert_eq!(ergebnis.segments[1].start_seconds, 4.0);
        assert_eq!(
            ergebnis.model, "large-v3-turbo-ct2",
            "der Bericht nennt das Modell, das wirklich lief"
        );

        // Nennt die Antwort kein Modell, bleibt es beim angefragten Namen.
        let ohne_modell = antwort_mit(serde_json::json!({
            "text": "Ein Satz.",
            "duration": 3.0
        }))
        .await;
        assert_eq!(ohne_modell.model, "whisper-1");
    }

    #[tokio::test]
    async fn unvollstaendige_zeitstempel_ergeben_gar_keine() {
        // Sonst haelt der Aufrufer die Liste fuer vollstaendig, und die Stelle
        // ohne Zeit wird nie geprueft.
        let ergebnis = antwort_mit(serde_json::json!({
            "text": "Erster Satz. Zweiter Satz.",
            "duration": 12.0,
            "segments": [
                {"start": 0.0, "end": 4.0, "text": "Erster Satz."},
                {"text": "Zweiter Satz ohne Zeit."}
            ]
        }))
        .await;
        assert!(
            ergebnis.segments.is_empty(),
            "es gilt der Volltext, nicht die halbe Liste"
        );
        assert!(!ergebnis.text.is_empty());
    }
}

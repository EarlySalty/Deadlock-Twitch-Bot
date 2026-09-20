use std::{
    env,
    io::Cursor,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use axum::{
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt, sync::Semaphore};
use tracing::{info, warn};
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
const DEFAULT_MODEL_NAME: &str = "ggml-large-v3-turbo-q5_0";
const DEFAULT_MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";
const DEFAULT_MODEL_SHA256: &str = "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2";
const DEFAULT_VAD_FILE: &str = "ggml-silero-v6.2.0.bin";
const DEFAULT_VAD_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";
const DEFAULT_VAD_SHA256: &str = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987";

#[derive(Clone)]
struct Config {
    host: String,
    port: u16,
    threads: i32,
    model_name: String,
    model_path: PathBuf,
    model_url: String,
    model_sha256: String,
    vad_path: PathBuf,
    vad_url: String,
    vad_sha256: String,
    forced_language: Option<String>,
    no_speech_max: f32,
    avg_logprob_min: f32,
}

impl Config {
    fn from_env() -> Result<Self, String> {
        let cache = env::var_os("STT_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/deadlock-stt")))
            .unwrap_or_else(|| PathBuf::from("/tmp/deadlock-stt"));

        Ok(Self {
            host: env_string("STT_HOST", "127.0.0.1"),
            port: env_parse("STT_PORT", 8791_u16)?,
            threads: env_parse("STT_THREADS", 8_i32)?,
            model_name: env_string("STT_MODEL", DEFAULT_MODEL_NAME),
            model_path: env::var_os("STT_MODEL_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| cache.join(DEFAULT_MODEL_FILE)),
            model_url: env_string("STT_MODEL_URL", DEFAULT_MODEL_URL),
            model_sha256: env_string("STT_MODEL_SHA256", DEFAULT_MODEL_SHA256),
            vad_path: env::var_os("STT_VAD_MODEL_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| cache.join(DEFAULT_VAD_FILE)),
            vad_url: env_string("STT_VAD_MODEL_URL", DEFAULT_VAD_URL),
            vad_sha256: env_string("STT_VAD_MODEL_SHA256", DEFAULT_VAD_SHA256),
            forced_language: env::var("STT_LANGUAGE")
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
            no_speech_max: env_parse("STT_NO_SPEECH_MAX", 0.6_f32)?,
            avg_logprob_min: env_parse("STT_AVG_LOGPROB_MIN", -1.0_f32)?,
        })
    }
}

fn env_string(name: &str, default: &str) -> String {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn env_parse<T>(name: &str, default: T) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(raw) if !raw.trim().is_empty() => raw
            .trim()
            .parse::<T>()
            .map_err(|e| format!("{name} ist ungueltig: {e}")),
        _ => Ok(default),
    }
}

struct Engine {
    context: WhisperContext,
}

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    engine: Arc<Engine>,
    gate: Arc<Semaphore>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    model: String,
    threads: i32,
    backend: &'static str,
}

#[derive(Serialize)]
struct SegmentResponse {
    id: usize,
    start: f64,
    end: f64,
    text: String,
}

#[derive(Serialize)]
struct TranscriptionResponse {
    text: String,
    duration: f64,
    language: String,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    segments: Option<Vec<SegmentResponse>>,
}

struct DecodedAudio {
    pcm: Vec<f32>,
    duration_seconds: f64,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    Unavailable(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, detail) = match self {
            Self::BadRequest(detail) => (StatusCode::BAD_REQUEST, detail),
            Self::Unavailable(detail) => (StatusCode::SERVICE_UNAVAILABLE, detail),
            Self::Internal(detail) => {
                warn!(error = %detail, "STT-Anfrage fehlgeschlagen");
                (StatusCode::INTERNAL_SERVER_ERROR, "Transkription fehlgeschlagen".to_owned())
            }
        };
        (status, Json(serde_json::json!({ "detail": detail }))).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=warn".into()),
        )
        .init();

    let config = Config::from_env()?;

    ensure_model(
        &config.model_path,
        &config.model_url,
        &config.model_sha256,
        "Whisper",
    )
    .await?;
    ensure_model(
        &config.vad_path,
        &config.vad_url,
        &config.vad_sha256,
        "VAD",
    )
    .await?;

    let started = Instant::now();
    let context = WhisperContext::new_with_params(
        &config.model_path,
        WhisperContextParameters::default(),
    )
    .map_err(|e| format!("Whisper-Modell konnte nicht geladen werden: {e}"))?;
    info!(
        model = %config.model_name,
        threads = config.threads,
        elapsed_seconds = started.elapsed().as_secs_f64(),
        "STT-Modell geladen"
    );

    let state = AppState {
        config: Arc::new(config.clone()),
        engine: Arc::new(Engine { context }),
        // Ein Whisper-Lauf nutzt selbst mehrere CPU-Threads. Mehrere parallele
        // Decoder würden den Host nur überbuchen und Latenz/Load verschlechtern.
        gate: Arc::new(Semaphore::new(1)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/audio/transcriptions", post(transcriptions))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES + 512 * 1024))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|e| format!("ungueltige STT_HOST/STT_PORT Kombination: {e}"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Port {addr} konnte nicht gebunden werden: {e}"))?;
    info!(%addr, "Rust-STT-Server bereit");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("HTTP-Server beendet: {e}"))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        model: state.config.model_name.clone(),
        threads: state.config.threads,
        backend: "whisper.cpp/whisper-rs",
    })
}

async fn transcriptions(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<TranscriptionResponse>, ApiError> {
    let mut file: Option<Vec<u8>> = None;
    let mut response_format = "verbose_json".to_owned();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("ungueltiges Multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_owned();
        match name.as_str() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("Audio nicht lesbar: {e}")))?;
                if bytes.is_empty() || bytes.len() > MAX_UPLOAD_BYTES {
                    return Err(ApiError::BadRequest(
                        "leeres oder zu grosses Audio".to_owned(),
                    ));
                }
                file = Some(bytes.to_vec());
            }
            "response_format" => {
                response_format = field
                    .text()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("response_format unlesbar: {e}")))?;
            }
            // model/language/timestamp_granularities[] werden aus
            // Kompatibilitaetsgruenden akzeptiert. Wie der Python-Dienst
            // entscheidet aber der lokale Server ueber Modell/Sprache.
            _ => {}
        }
    }

    let raw = file.ok_or_else(|| ApiError::BadRequest("Dateifeld fehlt".to_owned()))?;
    let decoded = decode_wav(&raw)?;
    let permit = state
        .gate
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ApiError::Unavailable("STT-Dienst beendet".to_owned()))?;
    let engine = Arc::clone(&state.engine);
    let config = Arc::clone(&state.config);
    let verbose = response_format == "verbose_json";

    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        transcribe(&engine.context, &config, decoded, verbose)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("STT-Worker abgebrochen: {e}")))??;

    Ok(Json(result))
}

fn decode_wav(raw: &[u8]) -> Result<DecodedAudio, ApiError> {
    let mut reader = hound::WavReader::new(Cursor::new(raw))
        .map_err(|e| ApiError::BadRequest(format!("ungueltiges WAV: {e}")))?;
    let spec = reader.spec();

    if spec.bits_per_sample != 16
        || spec.channels != 1
        || spec.sample_format != hound::SampleFormat::Int
        || spec.sample_rate != 16_000
    {
        return Err(ApiError::BadRequest(
            "erwartet 16-kHz 16-bit Mono-PCM".to_owned(),
        ));
    }

    let total_samples = reader.duration() as usize;
    let mut pcm = Vec::with_capacity(total_samples);
    for sample in reader.samples::<i16>() {
        let sample =
            sample.map_err(|e| ApiError::BadRequest(format!("PCM unlesbar: {e}")))?;
        pcm.push(sample as f32 / 32768.0);
    }

    Ok(DecodedAudio {
        duration_seconds: pcm.len() as f64 / spec.sample_rate as f64,
        pcm,
    })
}

fn transcribe(
    context: &WhisperContext,
    config: &Config,
    audio: DecodedAudio,
    verbose: bool,
) -> Result<TranscriptionResponse, ApiError> {
    let started = Instant::now();
    let mut state = context
        .create_state()
        .map_err(|e| ApiError::Internal(format!("Whisper-State: {e}")))?;

    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    params.set_n_threads(config.threads);
    params.set_translate(false);
    params.set_no_context(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_nst(true);
    params.set_logprob_thold(config.avg_logprob_min);
    params.set_no_speech_thold(config.no_speech_max);

    if let Some(language) = config.forced_language.as_deref() {
        params.set_language(Some(language));
    } else {
        params.set_language(None);
        params.set_detect_language(true);
    }

    let vad_path = config
        .vad_path
        .to_str()
        .ok_or_else(|| ApiError::Internal("VAD-Pfad ist kein UTF-8".to_owned()))?;
    params.set_vad_model_path(Some(vad_path));
    params.enable_vad(true);

    state
        .full(params, &audio.pcm)
        .map_err(|e| ApiError::Internal(format!("Whisper-Inferenz: {e}")))?;

    let mut kept = Vec::new();
    let mut raw_count = 0usize;

    for segment in state.as_iter() {
        raw_count += 1;
        let text = segment
            .to_str_lossy()
            .trim()
            .to_owned();
        if text.is_empty() {
            continue;
        }

        let no_speech = segment.no_speech_probability();
        let mut sum_logprob = 0.0_f32;
        let mut token_count = 0usize;
        for token_idx in 0..segment.n_tokens() {
            if let Some(token) = segment.get_token(token_idx) {
                let plog = token.token_data().plog;
                if plog.is_finite() {
                    sum_logprob += plog;
                    token_count += 1;
                }
            }
        }
        let avg_logprob = if token_count == 0 {
            0.0
        } else {
            sum_logprob / token_count as f32
        };

        if no_speech > config.no_speech_max || avg_logprob < config.avg_logprob_min {
            continue;
        }

        kept.push(SegmentResponse {
            id: kept.len(),
            start: segment.start_timestamp() as f64 / 100.0,
            end: segment.end_timestamp() as f64 / 100.0,
            text,
        });
    }

    let text = kept
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let language = get_lang_str(state.full_lang_id_from_state())
        .unwrap_or("unknown")
        .to_owned();
    let elapsed = started.elapsed().as_secs_f64();
    let dropped = raw_count.saturating_sub(kept.len());

    info!(
        duration_seconds = audio.duration_seconds,
        elapsed_seconds = elapsed,
        rtf = elapsed / audio.duration_seconds.max(0.01),
        segments = kept.len(),
        dropped,
        language = %language,
        "Transkription abgeschlossen"
    );

    Ok(TranscriptionResponse {
        text,
        duration: audio.duration_seconds,
        language,
        model: config.model_name.clone(),
        segments: verbose.then_some(kept),
    })
}

async fn ensure_model(
    path: &Path,
    url: &str,
    expected_sha256: &str,
    kind: &str,
) -> Result<(), String> {
    if fs::metadata(path).await.map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("{kind}-Modellpfad hat keinen Elternordner"))?;
    fs::create_dir_all(parent)
        .await
        .map_err(|e| format!("{kind}-Cache konnte nicht angelegt werden: {e}"))?;

    let part = path.with_extension("part");
    let _ = fs::remove_file(&part).await;
    info!(%url, target = %path.display(), "{kind}-Modell wird einmalig geladen");

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("Download-Client: {e}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("{kind}-Modell Download fehlgeschlagen: {e}"))?;

    let mut file = fs::File::create(&part)
        .await
        .map_err(|e| format!("{kind}-Tempdatei: {e}"))?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("{kind}-Downloadstream: {e}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("{kind}-Tempdatei schreiben: {e}"))?;
    }
    file.flush()
        .await
        .map_err(|e| format!("{kind}-Tempdatei flush: {e}"))?;
    drop(file);

    let actual = hex::encode(hasher.finalize());
    if !expected_sha256.is_empty() && !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(&part).await;
        return Err(format!(
            "{kind}-Modell SHA256 stimmt nicht (erwartet {expected_sha256}, erhalten {actual})"
        ));
    }

    fs::rename(&part, path)
        .await
        .map_err(|e| format!("{kind}-Modell aktivieren: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
            for sample in samples {
                writer.write_sample(*sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn accepts_exact_client_wav_shape() {
        let raw = wav(&[0, 16384, -16384, 32767], 16_000, 1);
        let decoded = decode_wav(&raw).unwrap();
        assert_eq!(decoded.pcm.len(), 4);
        assert!((decoded.duration_seconds - 0.00025).abs() < 1e-9);
    }

    #[test]
    fn rejects_non_mono_or_wrong_rate() {
        let stereo = wav(&[0, 0, 1, 1], 16_000, 2);
        assert!(matches!(decode_wav(&stereo), Err(ApiError::BadRequest(_))));

        let wrong_rate = wav(&[0, 1], 48_000, 1);
        assert!(matches!(
            decode_wav(&wrong_rate),
            Err(ApiError::BadRequest(_))
        ));
    }

    #[test]
    fn defaults_keep_loopback_and_existing_port() {
        env::remove_var("STT_HOST");
        env::remove_var("STT_PORT");
        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 8791);
    }
}

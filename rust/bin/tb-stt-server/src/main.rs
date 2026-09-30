use std::{
    env,
    io::Cursor,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hound::{SampleFormat, WavReader};
use serde::Serialize;
use tokio::sync::{OnceCell, Semaphore};
use tracing::{error, info, warn};
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    whisper: Arc<OnceCell<Arc<WhisperContext>>>,
    model_path: Arc<PathBuf>,
    model_label: Arc<str>,
    threads: i32,
    forced_language: Option<Arc<str>>,
    no_speech_max: f32,
    avg_logprob_min: f32,
    max_parallel: usize,
    max_inference: Duration,
    inference_gate: Arc<Semaphore>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    engine: &'static str,
    model: String,
    threads: i32,
    max_parallel_inference: usize,
    busy: bool,
    loaded: bool,
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
    engine: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    segments: Option<Vec<SegmentResponse>>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if print_build_revision() {
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tb_stt_server=info".into()),
        )
        .init();

    let model_path = env::var("STT_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/home/nathanael/stt-models/ggml-large-v3-turbo-q5_0.bin")
        });
    let model_label =
        env::var("STT_MODEL_LABEL").unwrap_or_else(|_| "ggml-large-v3-turbo-q5_0".to_owned());
    let threads = env_parse("STT_THREADS", 8_i32).clamp(1, 16);
    let forced_language = env::var("STT_LANGUAGE")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .map(Arc::<str>::from);
    let no_speech_max = env_parse("STT_NO_SPEECH_MAX", 0.6_f32);
    let avg_logprob_min = env_parse("STT_AVG_LOGPROB_MIN", -1.0_f32);
    let parallel = env_parse("STT_MAX_PARALLEL", 1_usize).clamp(1, 2);
    let max_inference = Duration::from_secs(env_parse("STT_MAX_INFERENCE_SECS", 90_u64).max(5));

    info!(
        model = %model_path.display(),
        threads,
        parallel,
        max_inference_secs = max_inference.as_secs(),
        "tb-stt-server konfiguriert; Modell wird erst beim ersten Audio geladen"
    );

    let state = AppState {
        whisper: Arc::new(OnceCell::new()),
        model_path: Arc::new(model_path),
        model_label: Arc::<str>::from(model_label),
        threads,
        forced_language,
        no_speech_max,
        avg_logprob_min,
        max_parallel: parallel,
        max_inference,
        inference_gate: Arc::new(Semaphore::new(parallel)),
    };

    let host = env::var("STT_HOST")
        .ok()
        .and_then(|s| s.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    if !host.is_loopback() {
        panic!("STT_HOST muss Loopback sein; der Dienst hat keine Authentifizierung");
    }
    let port = env_parse("STT_PORT", 8791_u16);
    let addr = SocketAddr::new(host, port);

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/audio/transcriptions", post(transcribe))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|error| panic!("{addr} konnte nicht gebunden werden: {error}"));
    info!(%addr, "tb-stt-server bereit");
    if let Err(error) = axum::serve(listener, app).await {
        error!(%error, "HTTP-Server beendet");
    }
}

fn env_parse<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr,
{
    env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<T>().ok())
        .unwrap_or(default)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        engine: "whisper.cpp/whisper-rs",
        model: state.model_label.to_string(),
        threads: state.threads,
        max_parallel_inference: state.max_parallel,
        busy: state.inference_gate.available_permits() < state.max_parallel,
        loaded: state.whisper.get().is_some(),
    })
}

async fn ensure_model(state: &AppState) -> Result<Arc<WhisperContext>, ApiError> {
    let model_path = state.model_path.clone();
    let model = state
        .whisper
        .get_or_try_init(|| async move {
            let model_display = model_path.display().to_string();
            let started = Instant::now();
            let loaded = tokio::task::spawn_blocking(move || {
                WhisperContext::new_with_params(
                    model_path.as_ref(),
                    WhisperContextParameters::default(),
                )
            })
            .await
            .map_err(|error| ApiError::internal(format!("STT-Modell-Worker abgebrochen: {error}")))?
            .map_err(|error| {
                ApiError::internal(format!(
                    "Whisper-Modell {model_display} konnte nicht geladen werden: {error}"
                ))
            })?;
            info!(
                model = %model_display,
                elapsed_ms = started.elapsed().as_millis(),
                "Whisper-Modell beim ersten Audio geladen"
            );
            Ok::<Arc<WhisperContext>, ApiError>(Arc::new(loaded))
        })
        .await?;
    Ok(model.clone())
}

fn effectively_silent(audio: &[f32]) -> bool {
    audio.iter().all(|sample| sample.abs() < 0.0005)
}

async fn transcribe(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<TranscriptionResponse>, ApiError> {
    let mut wav = None;
    let mut response_format = "verbose_json".to_owned();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request(format!("Multipart unlesbar: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|error| ApiError::bad_request(format!("Audio unlesbar: {error}")))?;
                if bytes.is_empty() || bytes.len() > MAX_UPLOAD_BYTES {
                    return Err(ApiError::bad_request("leeres oder zu grosses Audio"));
                }
                wav = Some(bytes.to_vec());
            }
            "response_format" => {
                response_format = field.text().await.map_err(|error| {
                    ApiError::bad_request(format!("response_format unlesbar: {error}"))
                })?;
            }
            // model/language bleiben absichtlich kompatible Eingabefelder. Das lokale
            // Modell ist serverseitig festgelegt; Sprache wird automatisch erkannt,
            // ausser STT_LANGUAGE erzwingt sie.
            _ => {}
        }
    }

    let wav = wav.ok_or_else(|| ApiError::bad_request("Multipart-Feld file fehlt"))?;
    let (audio, duration) = decode_wav(&wav)?;

    // Nahezu stummes Audio braucht weder Modell-Laden noch Whisper-Inferenz.
    // Damit bleibt der Dienst nach dem Boot und bei leeren Captures CPU-kalt.
    if effectively_silent(&audio) {
        return Ok(Json(TranscriptionResponse {
            text: String::new(),
            duration,
            language: state
                .forced_language
                .as_deref()
                .unwrap_or_default()
                .to_owned(),
            model: state.model_label.to_string(),
            engine: "whisper.cpp/whisper-rs",
            segments: (response_format == "verbose_json").then(Vec::new),
        }));
    }

    // Die Queue ist absichtlich seriell: mehrere Whisper-Inferenzen gleichzeitig
    // waren der Hauptgrund fuer interaktive Latenzspitzen auf dem gemeinsamen Host.
    let permit = state
        .inference_gate
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ApiError::internal("STT-Inferenzqueue geschlossen"))?;

    let context = ensure_model(&state).await?;
    let threads = state.threads;
    let forced_language = state.forced_language.clone();
    let no_speech_max = state.no_speech_max;
    let avg_logprob_min = state.avg_logprob_min;
    let max_inference = state.max_inference;
    let model = state.model_label.to_string();
    let started = Instant::now();

    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        run_whisper(
            &context,
            &audio,
            duration,
            threads,
            forced_language.as_deref(),
            no_speech_max,
            avg_logprob_min,
            max_inference,
            model,
            response_format == "verbose_json",
        )
    })
    .await
    .map_err(|error| ApiError::internal(format!("STT-Worker abgebrochen: {error}")))??;

    info!(
        audio_seconds = duration,
        elapsed_ms = started.elapsed().as_millis(),
        rtf = started.elapsed().as_secs_f64() / duration.max(0.01),
        segments = result.segments.as_ref().map_or(0, Vec::len),
        "Transkription fertig"
    );

    Ok(Json(result))
}

fn decode_wav(raw: &[u8]) -> Result<(Vec<f32>, f64), ApiError> {
    let mut reader = WavReader::new(Cursor::new(raw))
        .map_err(|error| ApiError::bad_request(format!("ungueltiges WAV: {error}")))?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != 16_000
        || spec.bits_per_sample != 16
        || spec.sample_format != SampleFormat::Int
    {
        return Err(ApiError::bad_request("erwartet 16-kHz 16-bit Mono-PCM-WAV"));
    }

    let samples: Result<Vec<f32>, _> = reader
        .samples::<i16>()
        .map(|sample| sample.map(|v| v as f32 / 32768.0))
        .collect();
    let samples =
        samples.map_err(|error| ApiError::bad_request(format!("WAV-Samples unlesbar: {error}")))?;
    if samples.is_empty() {
        return Err(ApiError::bad_request("Audio enthaelt keine Samples"));
    }
    let duration = samples.len() as f64 / 16_000.0;
    Ok((samples, duration))
}

fn run_whisper(
    context: &WhisperContext,
    audio: &[f32],
    duration: f64,
    threads: i32,
    forced_language: Option<&str>,
    no_speech_max: f32,
    avg_logprob_min: f32,
    max_inference: Duration,
    model: String,
    verbose: bool,
) -> Result<TranscriptionResponse, ApiError> {
    let mut state = context
        .create_state()
        .map_err(|error| ApiError::internal(format!("Whisper-State fehlgeschlagen: {error}")))?;
    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    params.set_n_threads(threads);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_translate(false);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_no_context(true);
    params.set_logprob_thold(avg_logprob_min);
    params.set_no_speech_thold(no_speech_max);
    let inference_started = Instant::now();
    let abort_callback: Box<dyn FnMut() -> bool> =
        Box::new(move || inference_started.elapsed() >= max_inference);
    params.set_abort_callback_safe::<Option<Box<dyn FnMut() -> bool>>, Box<dyn FnMut() -> bool>>(
        Some(abort_callback),
    );
    match forced_language {
        Some(language) => params.set_language(Some(language)),
        None => {
            params.set_language(None);
            params.set_detect_language(true);
        }
    }

    state
        .full(params, audio)
        .map_err(|error| ApiError::internal(format!("Whisper-Inferenz fehlgeschlagen: {error}")))?;

    let language = get_lang_str(state.full_lang_id_from_state())
        .unwrap_or(forced_language.unwrap_or(""))
        .to_owned();
    let mut kept = Vec::new();
    let mut dropped = 0usize;

    for (id, segment) in state.as_iter().enumerate() {
        let no_speech = segment.no_speech_probability();
        let mut sum_logprob = 0.0_f32;
        let mut token_count = 0usize;
        for token_index in 0..segment.n_tokens() {
            if let Some(token) = segment.get_token(token_index) {
                let probability = token.token_probability();
                if probability.is_finite() && probability > 0.0 {
                    sum_logprob += probability.max(1e-6).ln();
                    token_count += 1;
                }
            }
        }
        let avg_logprob = if token_count == 0 {
            0.0
        } else {
            sum_logprob / token_count as f32
        };
        if no_speech > no_speech_max || avg_logprob < avg_logprob_min {
            dropped += 1;
            continue;
        }
        let text = segment
            .to_str_lossy()
            .map_err(|error| ApiError::internal(format!("Whisper-Text unlesbar: {error}")))?
            .trim()
            .to_owned();
        if text.is_empty() {
            continue;
        }
        kept.push(SegmentResponse {
            id,
            start: segment.start_timestamp() as f64 / 100.0,
            end: segment.end_timestamp() as f64 / 100.0,
            text,
        });
    }

    if dropped > 0 {
        warn!(
            dropped,
            "Whisper-Segmente als Nicht-Sprache/Low-Confidence verworfen"
        );
    }

    let text = kept
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    Ok(TranscriptionResponse {
        text,
        duration,
        language,
        model,
        engine: "whisper.cpp/whisper-rs",
        segments: verbose.then_some(kept),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(sample_rate: u32, channels: u16) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
            for _ in 0..sample_rate / 10 * channels as u32 {
                writer.write_sample::<i16>(0).unwrap();
            }
            writer.finalize().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn akzeptiert_16khz_mono_pcm16() {
        let raw = wav_bytes(16_000, 1);
        let (samples, duration) = decode_wav(&raw).unwrap();
        assert_eq!(samples.len(), 1_600);
        assert!((duration - 0.1).abs() < 0.001);
    }

    #[test]
    fn lehnt_stereo_ab() {
        let raw = wav_bytes(16_000, 2);
        let error = decode_wav(&raw).unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }
}

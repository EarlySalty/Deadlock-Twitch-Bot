use std::{
    env,
    io::Cursor,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

mod recovery;
include!("../../build_revision.rs");

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
use tb_config::{file::ConfigArguments, BotConfigSnapshot};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
};
use tracing::{info, warn, Event, Level, Subscriber};
use tracing_subscriber::{filter::LevelFilter, layer::Context, prelude::*, Layer};
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    WhisperError, WhisperVadContext, WhisperVadContextParams, WhisperVadParams, WhisperVadSegments,
};

const DEFAULT_MODEL_NAME: &str = "ggml-large-v3-turbo-q5_0";
const DEFAULT_MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";
const DEFAULT_MODEL_SHA256: &str =
    "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2";
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
    pinned_models: bool,
    forced_language: Option<String>,
    no_speech_max: f32,
    avg_logprob_min: f32,
    max_upload_bytes: usize,
    config_fingerprint: String,
}

impl Config {
    fn from_snapshot(snapshot: &BotConfigSnapshot) -> Result<Self, String> {
        let settings = &snapshot.settings().stt;
        if settings.model != DEFAULT_MODEL_NAME {
            return Err(format!(
                "Das Rust-STT unterstützt aktuell nur das Modell {DEFAULT_MODEL_NAME}."
            ));
        }
        let cache = if let Some(directory) = &settings.model_directory {
            snapshot
                .resolve(directory)
                .map_err(|error| format!("stt.model_directory ist ungültig: {error}"))?
        } else {
            env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".cache/deadlock-stt"))
                .unwrap_or_else(|| PathBuf::from("/tmp/deadlock-stt"))
        };
        let max_upload_bytes = usize::try_from(settings.max_upload_bytes)
            .map_err(|_| "stt.max_upload_bytes ist auf diesem System zu groß.".to_owned())?;

        Ok(Self {
            host: settings.host.to_string(),
            port: settings.port,
            threads: i32::from(settings.threads),
            model_name: settings.model.clone(),
            model_path: cache.join(DEFAULT_MODEL_FILE),
            model_url: DEFAULT_MODEL_URL.to_owned(),
            model_sha256: DEFAULT_MODEL_SHA256.to_owned(),
            vad_path: cache.join(DEFAULT_VAD_FILE),
            vad_url: DEFAULT_VAD_URL.to_owned(),
            vad_sha256: DEFAULT_VAD_SHA256.to_owned(),
            pinned_models: settings.model_directory.is_some(),
            forced_language: settings.language.clone(),
            no_speech_max: settings.no_speech_max as f32,
            avg_logprob_min: settings.avg_logprob_min as f32,
            max_upload_bytes,
            config_fingerprint: snapshot.fingerprint().to_owned(),
        })
    }
}

struct Engine {
    context: WhisperContext,
    vad: Mutex<WhisperVadContext>,
    native_errors: Arc<AtomicU64>,
}

struct NativeErrorLayer {
    errors: Arc<AtomicU64>,
}

impl<S: Subscriber> Layer<S> for NativeErrorLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        if *metadata.level() == Level::ERROR
            && matches!(
                metadata.target(),
                "whisper_rs::whisper_logging_hook" | "whisper_rs::ggml_logging_hook"
            )
        {
            self.errors.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn observe_vad<T>(
    errors: &AtomicU64,
    call: impl FnOnce() -> Result<T, WhisperError>,
) -> Result<T, ApiError> {
    let before = errors.load(Ordering::SeqCst);
    let result = call();
    if errors.load(Ordering::SeqCst) != before {
        return Err(ApiError::Internal("VAD-Backendfehler".to_owned()));
    }
    result.map_err(|error| ApiError::Internal(format!("VAD-Inferenz: {error}")))
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
    config_fingerprint: String,
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
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Transkription fehlgeschlagen".to_owned(),
                )
            }
        };
        (status, Json(serde_json::json!({ "detail": detail }))).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    if print_build_revision() {
        return Ok(());
    }
    let arguments =
        ConfigArguments::parse(env::args_os().skip(1)).map_err(|error| error.to_string())?;
    let recovery_state = recovery::parse_arguments(&arguments.remaining)?;
    let snapshot = BotConfigSnapshot::load(&arguments.path).map_err(|error| error.to_string())?;
    let native_errors = Arc::new(AtomicU64::new(0));
    tracing_subscriber::registry()
        .with(NativeErrorLayer {
            errors: Arc::clone(&native_errors),
        })
        .with(
            tracing_subscriber::fmt::layer().with_filter(LevelFilter::from_level(
                snapshot.settings().logging.level.tracing_level(),
            )),
        )
        .init();
    whisper_rs::install_logging_hooks();

    if let Some(state_path) = recovery_state {
        recovery::run(&snapshot.settings().stt, &state_path).await?;
        return Ok(());
    }

    let config = Config::from_snapshot(&snapshot)?;
    info!(
        config_fingerprint = %config.config_fingerprint,
        "TWITCH_STT_CONFIG_V1"
    );

    if config.pinned_models {
        verify_pinned_model(&config.model_path, &config.model_sha256, "Whisper").await?;
        verify_pinned_model(&config.vad_path, &config.vad_sha256, "VAD").await?;
    } else {
        ensure_model(
            &config.model_path,
            &config.model_url,
            &config.model_sha256,
            "Whisper",
        )
        .await?;
        ensure_model(&config.vad_path, &config.vad_url, &config.vad_sha256, "VAD").await?;
    }

    let started = Instant::now();
    let context =
        WhisperContext::new_with_params(&config.model_path, WhisperContextParameters::default())
            .map_err(|e| format!("Whisper-Modell konnte nicht geladen werden: {e}"))?;
    info!(
        model = %config.model_name,
        threads = config.threads,
        elapsed_seconds = started.elapsed().as_secs_f64(),
        "STT-Modell geladen"
    );

    let vad_path = config
        .vad_path
        .to_str()
        .ok_or_else(|| "VAD-Pfad ist kein UTF-8".to_owned())?;
    let mut vad_params = WhisperVadContextParams::default();
    vad_params.set_n_threads(config.threads);
    let vad = WhisperVadContext::new(vad_path, vad_params)
        .map_err(|e| format!("VAD-Modell konnte nicht geladen werden: {e}"))?;

    let state = AppState {
        config: Arc::new(config.clone()),
        engine: Arc::new(Engine {
            context,
            vad: Mutex::new(vad),
            native_errors,
        }),
        gate: Arc::new(Semaphore::new(1)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/audio/transcriptions", post(transcriptions))
        .layer(DefaultBodyLimit::max(config.max_upload_bytes + 512 * 1024))
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
        config_fingerprint: state.config.config_fingerprint.clone(),
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
                if bytes.is_empty() || bytes.len() > state.config.max_upload_bytes {
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
        transcribe(&engine, &config, decoded, verbose)
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
        let sample = sample.map_err(|e| ApiError::BadRequest(format!("PCM unlesbar: {e}")))?;
        pcm.push(sample as f32 / 32768.0);
    }
    if pcm.is_empty() {
        return Err(ApiError::BadRequest("Die Audiodatei ist leer.".to_owned()));
    }

    Ok(DecodedAudio {
        duration_seconds: pcm.len() as f64 / spec.sample_rate as f64,
        pcm,
    })
}

fn speech_window(
    segments: Result<WhisperVadSegments, ApiError>,
    duration: f64,
) -> Result<Option<(i32, f64, f64)>, ApiError> {
    let segments = segments?;
    let count = segments.num_segments();
    if count == 0 {
        return Ok(None);
    }
    if count < 0 {
        return Err(ApiError::Internal(
            "VAD-Segmentzahl ist ungültig".to_owned(),
        ));
    }
    let first = segments
        .get_segment(0)
        .ok_or_else(|| ApiError::Internal("VAD-Segment fehlt".to_owned()))?;
    let last = segments
        .get_segment(count - 1)
        .ok_or_else(|| ApiError::Internal("VAD-Segment fehlt".to_owned()))?;
    let start = (f64::from(first.start) / 100.0).clamp(0.0, duration);
    let end = (f64::from(last.end) / 100.0).clamp(0.0, duration);
    if !start.is_finite() || !end.is_finite() || end <= start {
        return Err(ApiError::Internal(
            "VAD-Zeitfenster ist ungültig".to_owned(),
        ));
    }
    Ok(Some((count, start, end)))
}

fn bounded_timestamps(
    start_cs: i64,
    end_cs: i64,
    speech_start: f64,
    speech_end: f64,
) -> Option<(f64, f64)> {
    let start = (start_cs as f64 / 100.0).max(speech_start);
    let end = (end_cs as f64 / 100.0).min(speech_end);
    (end > start).then_some((start, end))
}

fn transcribe(
    engine: &Engine,
    config: &Config,
    audio: DecodedAudio,
    verbose: bool,
) -> Result<TranscriptionResponse, ApiError> {
    let started = Instant::now();
    let window = {
        let mut vad = engine
            .vad
            .lock()
            .map_err(|_| ApiError::Internal("VAD-State ist nicht verfügbar".to_owned()))?;
        speech_window(
            observe_vad(&engine.native_errors, || {
                vad.segments_from_samples(WhisperVadParams::default(), &audio.pcm)
            }),
            audio.duration_seconds,
        )?
    };
    let Some((vad_segments, speech_start, speech_end)) = window else {
        info!(
            duration_seconds = audio.duration_seconds,
            elapsed_seconds = started.elapsed().as_secs_f64(),
            vad_segments = 0,
            "Keine Sprache erkannt"
        );
        return Ok(TranscriptionResponse {
            text: String::new(),
            duration: audio.duration_seconds,
            language: "unknown".to_owned(),
            model: config.model_name.clone(),
            segments: verbose.then_some(Vec::new()),
        });
    };

    let mut state = engine
        .context
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
        params.set_detect_language(false);
    }

    state
        .full(params, &audio.pcm)
        .map_err(|e| ApiError::Internal(format!("Whisper-Inferenz: {e}")))?;

    let mut kept = Vec::new();
    let mut raw_count = 0usize;

    for segment in state.as_iter() {
        raw_count += 1;
        let text = segment
            .to_str_lossy()
            .map_err(|e| ApiError::Internal(format!("Whisper-Segmenttext: {e}")))?
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

        let Some((start, end)) = bounded_timestamps(
            segment.start_timestamp(),
            segment.end_timestamp(),
            speech_start,
            speech_end,
        ) else {
            continue;
        };

        kept.push(SegmentResponse {
            id: kept.len(),
            start,
            end,
            text,
        });
    }

    if kept.is_empty() {
        return Err(ApiError::Internal(
            "Sprache erkannt, aber kein verlässlicher Transkripttext".to_owned(),
        ));
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
        vad_segments,
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

#[cfg(unix)]
fn root_protected(uid: u32, mode: u32, file: bool) -> bool {
    uid == 0 && mode & (if file { 0o222 } else { 0o022 }) == 0
}

async fn verify_pinned_model(path: &Path, expected_sha256: &str, kind: &str) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| format!("{kind}-Modellpfad hat keinen Elternordner"))?;
    if !path.is_absolute() {
        return Err(format!("{kind}-Modellpfad muss absolut sein"));
    }
    let mut ancestor = PathBuf::new();
    for component in directory.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err(format!("{kind}-Modellpfad ist unsicher"));
        }
        ancestor.push(component);
        let metadata = fs::symlink_metadata(&ancestor)
            .await
            .map_err(|_| format!("{kind}-Modellverzeichnis fehlt oder ist nicht lesbar"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!("{kind}-Modellverzeichnis ist unsicher"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if !root_protected(metadata.uid(), metadata.permissions().mode(), false) {
                return Err(format!("{kind}-Modellverzeichnis ist nicht rootgeschützt"));
            }
        }
    }
    let metadata = fs::symlink_metadata(path)
        .await
        .map_err(|_| format!("{kind}-Modell fehlt oder ist nicht lesbar"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return Err(format!("{kind}-Modell ist keine reguläre Datei"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if !root_protected(metadata.uid(), metadata.permissions().mode(), true) {
            return Err(format!("{kind}-Modell ist nicht rootgeschützt"));
        }
    }
    verify_model_sha256(path, expected_sha256, kind).await
}

async fn verify_model_sha256(path: &Path, expected_sha256: &str, kind: &str) -> Result<(), String> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|_| format!("{kind}-Modell ist nicht lesbar"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|_| format!("{kind}-Modell konnte nicht geprüft werden"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if !hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected_sha256) {
        return Err(format!("{kind}-Modell hat eine ungültige SHA256-Prüfsumme"));
    }
    Ok(())
}

async fn ensure_model(
    path: &Path,
    url: &str,
    expected_sha256: &str,
    kind: &str,
) -> Result<(), String> {
    if fs::metadata(path)
        .await
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
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
    fn rejects_wav_without_audio_frames() {
        let empty = wav(&[], 16_000, 1);
        assert!(matches!(decode_wav(&empty), Err(ApiError::BadRequest(_))));
    }

    #[test]
    fn vad_failure_is_not_no_speech() {
        assert!(matches!(
            speech_window(
                observe_vad(&AtomicU64::new(0), || Err(WhisperError::NullPointer)),
                2.0
            ),
            Err(ApiError::Internal(_))
        ));
    }

    #[test]
    fn native_error_hook_rejects_successful_vad_return_with_reused_probabilities() {
        let errors = Arc::new(AtomicU64::new(0));
        let subscriber = tracing_subscriber::registry()
            .with(NativeErrorLayer {
                errors: Arc::clone(&errors),
            })
            .with(tracing_subscriber::fmt::layer().with_filter(LevelFilter::OFF));
        tracing::subscriber::set_global_default(subscriber).unwrap();
        whisper_rs::install_logging_hooks();

        let missing_model = std::env::temp_dir().join(format!(
            "tb-stt-missing-vad-model-{}.bin",
            std::process::id()
        ));
        assert!(!missing_model.exists());
        let before = errors.load(Ordering::SeqCst);
        assert!(WhisperVadContext::new(
            missing_model.to_str().unwrap(),
            WhisperVadContextParams::default()
        )
        .is_err());
        assert!(errors.load(Ordering::SeqCst) > before);

        let mut stale_probabilities = vec![0.85_f32; 64];
        assert!(observe_vad(&errors, || Ok::<_, WhisperError>(())).is_ok());
        let failure = observe_vad(&errors, || {
            stale_probabilities.resize(64, 0.0);
            assert_eq!(stale_probabilities, vec![0.85_f32; 64]);
            std::thread::spawn(|| {
                tracing::error!(
                    target: "whisper_rs::whisper_logging_hook",
                    "whisper_vad_detect_speech: failed to compute VAD graph"
                );
            })
            .join()
            .unwrap();
            Ok::<_, WhisperError>(stale_probabilities)
        })
        .unwrap_err();
        assert_eq!(
            failure.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        let ggml_failure = observe_vad(&errors, || {
            std::thread::spawn(|| {
                tracing::error!(target: "whisper_rs::ggml_logging_hook", "compute failed");
            })
            .join()
            .unwrap();
            Ok::<_, WhisperError>(())
        })
        .unwrap_err();
        assert_eq!(
            ggml_failure.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(observe_vad(&errors, || Ok::<_, WhisperError>(())).is_ok());
    }

    #[test]
    fn speech_timestamps_remain_on_original_timeline() {
        assert_eq!(bounded_timestamps(0, 700, 1.98, 6.98), Some((1.98, 6.98)));
        assert_eq!(bounded_timestamps(0, 512, 0.0, 5.025), Some((0.0, 5.025)));
        assert_eq!(bounded_timestamps(0, 100, 1.98, 6.98), None);
    }

    #[test]
    fn defaults_keep_loopback_and_existing_port() {
        let snapshot = BotConfigSnapshot::parse(
            "schema_version=1\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n",
            Path::new("/test/config/bot.toml"),
        )
        .unwrap();
        let cfg = Config::from_snapshot(&snapshot).unwrap();
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 8791);
        assert!(!cfg.pinned_models);
        assert!(
            cfg.model_path
                .ends_with(".cache/deadlock-stt/ggml-large-v3-turbo-q5_0.bin")
                || cfg
                    .model_path
                    .ends_with("/tmp/deadlock-stt/ggml-large-v3-turbo-q5_0.bin")
        );
    }

    #[test]
    fn configured_directory_uses_snapshot_resolution() {
        let snapshot = BotConfigSnapshot::parse(
            "schema_version=1\n[twitch]\nbot_user_id='1'\nnotify_channel_id='2'\neventsub_callback_url='https://example.invalid/callback'\n[stt]\nmodel_directory='../models'\n",
            Path::new("/test/config/bot.toml"),
        )
        .unwrap();
        let cfg = Config::from_snapshot(&snapshot).unwrap();
        assert!(cfg.pinned_models);
        assert_eq!(
            cfg.model_path,
            Path::new("/test/models").join(DEFAULT_MODEL_FILE)
        );
        assert_eq!(
            cfg.vad_path,
            Path::new("/test/models").join(DEFAULT_VAD_FILE)
        );
    }

    #[test]
    fn configured_directory_cannot_escape_filesystem_root() {
        let snapshot = BotConfigSnapshot::parse(
            "schema_version=1\n[twitch]\nbot_user_id='1'\nnotify_channel_id='2'\neventsub_callback_url='https://example.invalid/callback'\n[stt]\nmodel_directory='../../../../../../models'\n",
            Path::new("/test/config/bot.toml"),
        )
        .unwrap();
        assert!(Config::from_snapshot(&snapshot).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn pinned_models_require_root_control_of_every_entry() {
        assert!(root_protected(0, 0o755, false));
        assert!(!root_protected(1000, 0o755, false));
        assert!(!root_protected(0, 0o775, false));
        assert!(!root_protected(0, 0o757, false));
        assert!(root_protected(0, 0o444, true));
        assert!(!root_protected(1000, 0o444, true));
        assert!(!root_protected(0, 0o644, true));
        assert!(!root_protected(0, 0o464, true));
    }

    #[tokio::test]
    async fn pinned_models_reject_insecure_paths_without_network() {
        let directory =
            std::env::temp_dir().join(format!("tb-stt-model-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let path = directory.join(DEFAULT_VAD_FILE);
        let expected = hex::encode(Sha256::digest(b"fixture"));
        assert!(verify_pinned_model(&path, &expected, "VAD").await.is_err());
        std::fs::write(&path, b"wrong").unwrap();
        assert!(verify_model_sha256(&path, &expected, "VAD").await.is_err());
        std::fs::write(&path, b"fixture").unwrap();
        verify_model_sha256(&path, &expected, "VAD").await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        }
        let rejection = verify_pinned_model(&path, &expected, "VAD")
            .await
            .unwrap_err();
        assert!(rejection.contains("rootgeschützt"));
        let link = directory.join(DEFAULT_MODEL_FILE);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(verify_pinned_model(&link, &expected, "Whisper")
                .await
                .is_err());
            std::fs::remove_file(&link).unwrap();
            let directory_link = directory.with_extension("symlink");
            std::os::unix::fs::symlink(&directory, &directory_link).unwrap();
            assert!(
                verify_pinned_model(&directory_link.join(DEFAULT_VAD_FILE), &expected, "VAD")
                    .await
                    .is_err()
            );
            std::fs::remove_file(&directory_link).unwrap();
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
}

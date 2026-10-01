//! Betriebseigentümer für den lokalen Rust-Whisper-Dienst und seine Aufrufer.
//!
//! Dienst und Clients lesen denselben unveränderlichen Snapshot. Ein Modell wird
//! geladen, aber ohne Audio kein Transkriptionsjob gestartet. Änderungen an
//! Listener, Modell und Threads verlangen Neustart.

use crate::{
    file::FileError,
    global::{public_url, range},
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SttConfig {
    pub host: IpAddr,
    pub port: u16,
    pub model: String,
    pub threads: u16,
    /// `None` bedeutet Spracherkennung. Ein Client darf sie nicht überschreiben.
    pub language: Option<String>,
    pub no_speech_max: f64,
    pub avg_logprob_min: f64,
    pub max_upload_bytes: u64,
    pub timeout_seconds: u64,
    pub extraction_timeout_seconds: u64,
    /// Ausdrückliche bisherige Ausnahme für OpenAI. Kein Anbieterwechsel.
    /// Ohne diesen Wert bleibt Stream-Audio auf dem lokalen Rechner.
    pub remote_endpoint: Option<String>,
    pub remote_model: String,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8791,
            model: "ggml-large-v3-turbo-q5_0".into(),
            threads: 8,
            language: None,
            no_speech_max: 0.6,
            avg_logprob_min: -1.0,
            max_upload_bytes: 25 * 1024 * 1024,
            timeout_seconds: 60,
            extraction_timeout_seconds: 300,
            remote_endpoint: None,
            remote_model: "whisper-1".into(),
        }
    }
}

impl SttConfig {
    pub fn local_origin(&self) -> String {
        format!("http://{}", SocketAddr::new(self.host, self.port))
    }

    pub fn transcription_endpoint(&self) -> String {
        self.remote_endpoint
            .clone()
            .unwrap_or_else(|| format!("{}/v1/audio/transcriptions", self.local_origin()))
    }

    pub fn request_model(&self) -> &str {
        if self.remote_endpoint.is_some() {
            &self.remote_model
        } else {
            &self.model
        }
    }

    pub fn validate(&self) -> Result<(), FileError> {
        if !self.host.is_loopback() {
            return Err(FileError::invalid("stt.host"));
        }
        range(u64::from(self.port), 1, 65_535, "stt.port")?;
        range(u64::from(self.threads), 1, 64, "stt.threads")?;
        range(
            self.max_upload_bytes,
            1_024,
            25 * 1024 * 1024,
            "stt.max_upload_bytes",
        )?;
        range(self.timeout_seconds, 1, 3_600, "stt.timeout_seconds")?;
        range(
            self.extraction_timeout_seconds,
            1,
            3_600,
            "stt.extraction_timeout_seconds",
        )?;
        // Explizite lokale Pfade sind keine Hub-Bezeichner. Ihre Existenz
        // prüft der Launcher nach Auflösung gegen die Konfigurationsdatei.
        let local_model = std::path::Path::new(&self.model).is_absolute()
            || self.model.starts_with("./")
            || self.model.starts_with("../");
        let valid_model = if local_model {
            self.model.len() <= 4096 && !self.model.chars().any(char::is_control)
        } else {
            self.model.len() <= 512
                && self
                    .model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
        };
        if self.model.is_empty() || !valid_model {
            return Err(FileError::invalid("stt.model"));
        }
        if self.language.as_ref().is_some_and(|language| {
            language.is_empty()
                || language.len() > 16
                || !language
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-')
        }) {
            return Err(FileError::invalid("stt.language"));
        }
        if !self.no_speech_max.is_finite() || !(0.0..=1.0).contains(&self.no_speech_max) {
            return Err(FileError::invalid("stt.no_speech_max"));
        }
        if !self.avg_logprob_min.is_finite() || !(-20.0..=0.0).contains(&self.avg_logprob_min) {
            return Err(FileError::invalid("stt.avg_logprob_min"));
        }
        if let Some(endpoint) = &self.remote_endpoint {
            public_url(endpoint, "stt.remote_endpoint", false)?;
            let url =
                url::Url::parse(endpoint).map_err(|_| FileError::invalid("stt.remote_endpoint"))?;
            if url.scheme() != "https"
                || url.host_str() != Some("api.openai.com")
                || url.port_or_known_default() != Some(443)
                || url.path() != "/v1/audio/transcriptions"
            {
                return Err(FileError::invalid("stt.remote_endpoint"));
            }
            if self.remote_model != "whisper-1" {
                return Err(FileError::invalid("stt.remote_model"));
            }
        }
        Ok(())
    }
}

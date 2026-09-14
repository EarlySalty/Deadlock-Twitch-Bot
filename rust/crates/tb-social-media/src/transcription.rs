use std::path::Path;

use async_trait::async_trait;
use serde_json::json;
use tb_engagement::transcribe::OpenAiTranscriber;

use crate::enrich_pipeline::{TranscribeError, Transcriber, TranscriptionOutput};

pub struct SttTranscriber {
    inner: OpenAiTranscriber,
}

impl SttTranscriber {
    pub fn from_default() -> Option<Self> {
        let inner = OpenAiTranscriber::from_env()?;
        if !inner.is_local() {
            tracing::warn!(
                "Clip-Transkription: STT-Endpunkt ist nicht loopback, Stage bleibt aus"
            );
            return None;
        }
        Some(Self { inner })
    }

    pub fn new(inner: OpenAiTranscriber) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Transcriber for SttTranscriber {
    async fn transcribe_clip(
        &self,
        video_path: &Path,
    ) -> Result<TranscriptionOutput, TranscribeError> {
        if !video_path.exists() {
            return Err(TranscribeError::NotFound(video_path.display().to_string()));
        }
        match self.inner.transcribe_clip(video_path).await {
            Ok(result) => {
                let segments = result
                    .segments
                    .iter()
                    .map(|s| {
                        json!({
                            "start_seconds": s.start_seconds,
                            "end_seconds": s.end_seconds,
                            "text": s.text,
                        })
                    })
                    .collect();
                Ok(TranscriptionOutput {
                    text: result.text,
                    segments,
                    language: None,
                })
            }
            Err(error) => Err(TranscribeError::Failed(error)),
        }
    }
}

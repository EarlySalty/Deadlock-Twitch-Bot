mod media;
mod model;
mod pipeline;
pub mod revision;
mod runtime_config;
mod service;
mod skills;
mod source;
mod stage_contract;
mod validation;

pub use media::{
    extract_frames, inspect_media, prepare_intervals, render_prepared, FrameBatch, MediaInfo,
    PreparedMedia,
};
pub use model::{
    EditPlan, EditorOptions, Highlight, KeptInterval, LayoutIntent, PlanLimits, ResourceLimits,
};
pub use skills::{skill_snapshot, SkillId, SkillSnapshot};
pub use validation::{
    build_subtitle_ass, rebase_subtitles, validate_plan, validate_plan_json, EditorError,
    ErrorCode, MappedSubtitle, SourceSubtitle, ValidatedPlan,
};
pub mod brain;
pub mod store;
pub mod stt;

pub use pipeline::{produce, FrozenSkill, ProducedOutcome, ProductionContext};
pub use runtime_config::{EditorRuntime, RuntimeConfig};
pub use service::{
    create_manual_job, get_manual_job, list_manual_jobs, manual_original_file,
    manual_version_original_file, retry_manual_job, ManualWorker,
};
pub use source::authorized_path;

#[derive(Debug, thiserror::Error)]
pub enum AiEditorError {
    #[error("Die KI-Konfiguration ist nicht verfügbar.")]
    Configuration,
    #[error("Der ursprüngliche Deadlock-Clip ist nicht verfügbar oder hat sich geändert.")]
    Source,
    #[error("Für die externe KI-Verarbeitung fehlt die Einwilligung.")]
    Consent,
    #[error("Das freigegebene Kostenlimit ist erreicht.")]
    Budget,
    #[error("Der KI-Dienst ist derzeit nicht verfügbar.")]
    Provider,
    #[error("Die Untertitel konnten nicht erstellt werden.")]
    Transcription,
    #[error("Der Schnittplan ist ungültig.")]
    Plan,
    #[error("Das Video konnte nicht erstellt werden.")]
    Render,
    #[error("Das Video konnte nicht sicher geprüft werden.")]
    Validation,
    #[error("Die Auftragsdaten konnten nicht gespeichert oder geladen werden.")]
    Storage,
    #[error("Der Auftrag hat sich bereits geändert.")]
    Conflict,
    #[error("Die erlaubten Versuche sind aufgebraucht.")]
    Attempts,
    #[error("Das Zeitlimit für die Verarbeitung ist erreicht.")]
    Deadline,
    #[error("Bitte gib eine gültige Änderungsanweisung ein.")]
    Input,
    #[error("Die Freigabe oder Übernahme in den Zeitplan konnte nicht abgeschlossen werden: {0}")]
    Publishing(#[from] crate::produced_video::ProducedVideoError),
    #[error("Der Auftrag wurde nicht gefunden.")]
    NotFound,
}

impl From<store::AiStoreError> for AiEditorError {
    fn from(error: store::AiStoreError) -> Self {
        match error {
            store::AiStoreError::NotFound => Self::NotFound,
            store::AiStoreError::SourceUnavailable | store::AiStoreError::InvalidInput => {
                Self::Source
            }
            store::AiStoreError::Conflict => Self::Conflict,
            store::AiStoreError::AttemptsExhausted => Self::Attempts,
            _ => Self::Storage,
        }
    }
}

impl From<tb_llm::social_editor::EditorError> for AiEditorError {
    fn from(error: tb_llm::social_editor::EditorError) -> Self {
        match error.kind {
            tb_llm::social_editor::ErrorKind::Consent => Self::Consent,
            tb_llm::social_editor::ErrorKind::Budget => Self::Budget,
            tb_llm::social_editor::ErrorKind::Ledger => Self::Storage,
            tb_llm::social_editor::ErrorKind::Configuration => Self::Configuration,
            _ => Self::Provider,
        }
    }
}

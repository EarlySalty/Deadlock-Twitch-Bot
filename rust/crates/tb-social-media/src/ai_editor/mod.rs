pub mod autopilot_revision;
mod browser_passkey;
mod comparison;
pub mod learning;
mod media;
pub mod metadata;
mod model;
mod observation;
pub mod options;
mod pipeline;
mod scene;
pub mod revision;
pub mod runtime_config;
mod service;
pub mod skill_registry;
mod skills;
mod source;
mod stage_contract;
mod validation;

pub use media::{
    audio_levels, contact_sheet_grid, extract_frames, extract_stills, inspect_media, prepare_intervals,
    render_prepared, AudioLevels, FrameBatch, MediaInfo, PreparedMedia, MAX_CONTACT_SHEET_FPS,
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

pub use browser_passkey::keep_gemini_session;
pub use comparison::run_observer_comparisons;
pub use pipeline::{produce, FrozenSkill, ProducedOutcome, ProductionContext};
pub use runtime_config::{EditorRuntime, MediaRuntime, RuntimeConfig};
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
    #[error("Der Clip konnte nicht von Twitch geladen werden.")]
    SourceDownload,
    #[error("Der Clip ist auf Twitch nicht mehr verfügbar.")]
    SourceMissing,
    #[error("Die gespeicherte Clip-Datei ist nicht lesbar.")]
    SourceUnreadable,
    #[error("Für die externe KI-Verarbeitung fehlt die Einwilligung.")]
    Consent,
    #[error("Das freigegebene Kostenlimit ist erreicht.")]
    Budget,
    #[error("Der KI-Dienst ist derzeit nicht verfügbar.")]
    Provider,
    #[error("Der KI-Zugang ist auf dem Server nicht vollständig eingerichtet.")]
    ProviderAccess,
    #[error("Dieser Clip ist für die KI-Bearbeitung zu umfangreich.")]
    RequestTooLarge,
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
    #[error(
        "Die gewünschte Änderung ließ sich nicht umsetzen, deshalb gibt es keine neue Fassung."
    )]
    RevisionUnchanged,
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
    #[error("Für diesen Clip läuft auf dieser Plattform schon eine Veröffentlichung.")]
    PipelineBusy,
}

impl AiEditorError {
    pub(crate) fn plan_rejected(reason: &str) -> Self {
        tracing::warn!(reason, "AI edit plan rejected");
        Self::Plan
    }

    pub fn is_source_fetch_failure(&self) -> bool {
        matches!(
            self,
            Self::SourceDownload | Self::SourceMissing | Self::SourceUnreadable
        )
    }

    pub(crate) fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::SourceDownload
                | Self::Provider
                | Self::Transcription
                | Self::Render
                | Self::Validation
                | Self::Storage
                | Self::Deadline
        )
    }

    pub fn is_human_review_hold(&self) -> bool {
        matches!(self, Self::Consent)
    }

    const UNIT_VARIANTS: [AiEditorError; 21] = [
        Self::Configuration,
        Self::Source,
        Self::SourceDownload,
        Self::SourceMissing,
        Self::SourceUnreadable,
        Self::Consent,
        Self::Budget,
        Self::Provider,
        Self::ProviderAccess,
        Self::RequestTooLarge,
        Self::Transcription,
        Self::Plan,
        Self::Render,
        Self::Validation,
        Self::Storage,
        Self::Conflict,
        Self::Attempts,
        Self::Deadline,
        Self::Input,
        Self::NotFound,
        Self::PipelineBusy,
    ];

    pub fn failure_messages() -> Vec<String> {
        Self::UNIT_VARIANTS
            .iter()
            .filter(|error| !error.is_human_review_hold())
            .map(ToString::to_string)
            .collect()
    }
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
            store::AiStoreError::PipelineBusy => Self::PipelineBusy,
            _ => Self::Storage,
        }
    }
}

impl From<tb_llm::social_editor::EditorError> for AiEditorError {
    fn from(error: tb_llm::social_editor::EditorError) -> Self {
        if error.access.is_some() {
            return Self::ProviderAccess;
        }
        match error.kind {
            tb_llm::social_editor::ErrorKind::Consent => Self::Consent,
            tb_llm::social_editor::ErrorKind::Budget => Self::Budget,
            tb_llm::social_editor::ErrorKind::Ledger => Self::Storage,
            tb_llm::social_editor::ErrorKind::Configuration => Self::Configuration,
            _ => Self::Provider,
        }
    }
}

#[cfg(test)]
mod completion_mapping_tests {
    use super::AiEditorError;

    #[test]
    fn consent_is_the_only_produce_error_that_holds_for_review() {
        assert!(AiEditorError::Consent.is_human_review_hold());
        for error in AiEditorError::UNIT_VARIANTS
            .iter()
            .filter(|error| !matches!(error, AiEditorError::Consent))
        {
            assert!(
                !error.is_human_review_hold(),
                "{error} must finish as failed"
            );
        }
        assert!(!AiEditorError::failure_messages().contains(&AiEditorError::Consent.to_string()));
        assert!(AiEditorError::failure_messages().contains(&AiEditorError::Source.to_string()));
    }
}

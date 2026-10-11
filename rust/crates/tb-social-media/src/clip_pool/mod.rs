mod filter;
mod imports;
mod repository;
mod worker;

pub use filter::{bounds, eligible, ImportWindow, DEADLOCK_GAME_ID};
pub use imports::{get_import, latest_import, resume_import, start_import, ImportJob};
pub use repository::{
    is_managed, legacy_automatic_allowed, list, register_recent, save_preferences, set_selected,
    PoolClip, PoolPage, PoolPreferences,
};
pub use worker::ClipImportWorker;

#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    #[error("Der Zeitraum oder die Zeitzone ist ungültig")]
    InvalidFilter,
    #[error("Die Clipdaten sind unvollständig oder ungültig")]
    InvalidSource,
    #[error("Der Clip oder Import wurde für deinen Kanal nicht gefunden")]
    NotFound,
    #[error("Der Clip erfüllt den aktuellen Filter nicht oder wurde bereits verarbeitet")]
    NotSelectable,
    #[error("Die Auswahl kann nicht entfernt werden, weil der Clip bereits eingeplant oder verarbeitet wurde")]
    Reserved,
    #[error("Der Import wird bereits bearbeitet")]
    Busy,
    #[error("Die Bearbeitungsfrist des Imports ist abgelaufen")]
    LeaseLost,
    #[error("Die Clipdaten konnten nicht gespeichert oder geladen werden")]
    Database(#[from] sqlx::Error),
    #[error("Die gespeicherten Clipdaten sind ungültig")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Settings(#[from] crate::autopilot_settings::SettingsError),
}

#[cfg(test)]
mod tests;

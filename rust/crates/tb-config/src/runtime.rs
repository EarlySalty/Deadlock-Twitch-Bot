//! Die beim Dienststart geprüfte Momentaufnahme; niemals die gespeicherte Datei
//! als bereits aktive Konfiguration ausgeben.

use crate::{
    file::{ErrorKind, FileError},
    BotConfigSnapshot,
};
use std::sync::OnceLock;

static ACTIVE: OnceLock<BotConfigSnapshot> = OnceLock::new();

pub fn install(snapshot: BotConfigSnapshot) -> Result<&'static BotConfigSnapshot, FileError> {
    ACTIVE
        .set(snapshot)
        .map_err(|_| FileError::new(ErrorKind::RestartRequired))?;
    ACTIVE
        .get()
        .ok_or_else(|| FileError::new(ErrorKind::FileUnreadable))
}

pub fn active() -> Option<&'static BotConfigSnapshot> {
    ACTIVE.get()
}

/// Fachverbraucher dürfen keine zweite Betriebsquelle oder Ersatzkonfiguration
/// aufbauen. Fehlender Dienststart wird als Fehler weitergereicht.
pub fn settings() -> Result<&'static crate::global::BotConfig, FileError> {
    active()
        .map(BotConfigSnapshot::settings)
        .ok_or_else(|| FileError::new(ErrorKind::FileUnreadable))
}

pub fn start(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(&'static BotConfigSnapshot, Vec<std::ffi::OsString>), FileError> {
    let arguments = crate::file::ConfigArguments::parse(arguments)?;
    let snapshot = BotConfigSnapshot::load(&arguments.path)?;
    Ok((install(snapshot)?, arguments.remaining))
}

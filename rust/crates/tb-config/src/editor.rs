//! Begrenzter Betriebseditor. Keine Pfade, Identitäten oder Modelle aus einem
//! HTTP-Payload übernehmen. Jeder Schreibzugriff validiert das gesamte Schema.

use crate::{file::FileError, global::Database, BotConfigSnapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingOptions {
    pub pool_max: u32,
    pub acquire_timeout_ms: u64,
    pub connect_timeout_seconds: u64,
}

impl From<&Database> for OperatingOptions {
    fn from(value: &Database) -> Self {
        Self {
            pool_max: value.pool_max,
            acquire_timeout_ms: value.acquire_timeout_ms,
            connect_timeout_seconds: value.connect_timeout_seconds,
        }
    }
}

#[derive(Debug)]
pub enum EditError {
    Invalid(FileError),
    Conflict,
    Busy,
    UnsafeLocation,
    Io,
}

pub struct SavedConfig {
    pub snapshot: BotConfigSnapshot,
    pub revision: String,
}

fn revision(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn load_saved(source: &Path) -> Result<SavedConfig, FileError> {
    let (snapshot, text) = BotConfigSnapshot::load_document(source)?;
    Ok(SavedConfig {
        snapshot,
        revision: revision(&text),
    })
}

impl From<FileError> for EditError {
    fn from(value: FileError) -> Self {
        Self::Invalid(value)
    }
}

/// Gegen die feste beim Start geladene Quelle arbeiten. Ein Deploy-Checkout
/// ist kein Bedienerspeicher; Änderungen darin würden Start-Gates blockieren.
fn persistent_path(path: &Path) -> Result<PathBuf, EditError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| EditError::Io)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(EditError::UnsafeLocation);
    }
    let canonical = path.canonicalize().map_err(|_| EditError::Io)?;
    if canonical
        .ancestors()
        .any(|parent| parent.join(".git").exists())
    {
        return Err(EditError::UnsafeLocation);
    }
    Ok(canonical)
}

/// Die Sperrdatei bleibt bestehen: Entfernen würde bei wartenden Schreibern
/// zwei verschiedene Sperr-Inodes und damit verlorene Änderungen erlauben.
pub fn save(
    source: &Path,
    expected_revision: &str,
    options: &OperatingOptions,
) -> Result<SavedConfig, EditError> {
    let source = persistent_path(source)?;
    let directory = source.parent().ok_or(EditError::UnsafeLocation)?;
    let mut lock_options = OpenOptions::new();
    lock_options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        lock_options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let lock = lock_options
        .open(directory.join(".bot.toml.lock"))
        .map_err(|_| EditError::Io)?;
    if !lock.metadata().map_err(|_| EditError::Io)?.is_file() {
        return Err(EditError::UnsafeLocation);
    }
    lock.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => EditError::Busy,
        std::fs::TryLockError::Error(_) => EditError::Io,
    })?;
    let (current, original) = BotConfigSnapshot::load_document(&source)?;
    if revision(&original) != expected_revision {
        return Err(EditError::Conflict);
    }
    let mut candidate = current.settings().clone();
    candidate.database = Database {
        pool_max: options.pool_max,
        acquire_timeout_ms: options.acquire_timeout_ms,
        connect_timeout_seconds: options.connect_timeout_seconds,
    };
    use crate::file::Schema;
    candidate.validate()?;
    let mut document = original
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| EditError::Io)?;
    if !document.contains_key("database") {
        document["database"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    for (key, value) in [
        ("pool_max", i64::from(options.pool_max)),
        ("acquire_timeout_ms", options.acquire_timeout_ms as i64),
        (
            "connect_timeout_seconds",
            options.connect_timeout_seconds as i64,
        ),
    ] {
        // Die Ganzzahlen sind oben vollständig bereichsgeprüft. Auch ein
        // Kommentar hinter genau dem geänderten Wert bleibt erhalten.
        let mut replacement = toml_edit::value(value);
        let previous = document
            .get("database")
            .and_then(toml_edit::Item::as_table_like)
            .and_then(|table| table.get(key))
            .and_then(toml_edit::Item::as_value);
        if let (Some(previous), Some(next)) = (previous, replacement.as_value_mut()) {
            *next.decor_mut() = previous.decor().clone();
        }
        document["database"][key] = replacement;
    }
    let text = document.to_string();
    let checked = BotConfigSnapshot::parse(&text, &source)?;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let temporary = directory.join(format!(
        ".bot.toml.{}.{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut create = OpenOptions::new();
    create.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        create.mode(0o600);
    }
    let mut file = create.open(&temporary).map_err(|_| EditError::Io)?;
    // Nach erfolgreichem create_new besitzen wir ausschließlich diese Tempdatei.
    let result = (|| {
        let metadata = fs::metadata(&source).map_err(|_| EditError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{fchown, MetadataExt};
            // Bot und Dashboard laufen unter getrennten Konten. Den Besitzer
            // und insbesondere die gemeinsame Lesegruppe beim Rename erhalten.
            fchown(&file, Some(metadata.uid()), Some(metadata.gid())).map_err(|_| EditError::Io)?;
        }
        file.set_permissions(metadata.permissions())
            .map_err(|_| EditError::Io)?;
        file.write_all(text.as_bytes()).map_err(|_| EditError::Io)?;
        file.sync_all().map_err(|_| EditError::Io)?;
        // Nicht kooperierende externe Änderungen während der Serialisierung
        // ebenfalls erkennen. Manuelle Betreiber müssen denselben Lock nutzen.
        if load_saved(&source)?.revision != expected_revision {
            return Err(EditError::Conflict);
        }
        fs::rename(&temporary, &source).map_err(|_| EditError::Io)?;
        File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| EditError::Io)?;
        Ok(SavedConfig {
            snapshot: checked,
            revision: revision(&text),
        })
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

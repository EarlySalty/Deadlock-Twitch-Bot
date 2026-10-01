//! Begrenztes Einlesen und Prüfen einer Konfigurationsdatei ohne Nebenwirkungen.
//!
//! Fehler enthalten ausschließlich fest vorgegebene Kategorien und numerische
//! Positionen. Der TOML-Fehler selbst wird weder gespeichert noch als `source`
//! weitergereicht: dessen Display und Debug könnten Eingabewerte offenlegen.

use std::{
    ffi::OsString,
    fmt,
    fs::OpenOptions,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    ConfigArgumentMissing,
    ConfigArgumentDuplicate,
    AbsolutePathRequired,
    FileMissing,
    FileUnreadable,
    RegularFileRequired,
    FileTooLarge,
    InvalidEncoding,
    InvalidDocument,
    InvalidValue,
    RestartRequired,
}

/// Kein Eingabetext, kein dynamischer Feldname und keine verschachtelte Quelle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileError {
    pub kind: ErrorKind,
    pub field: Option<&'static str>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl FileError {
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            field: None,
            line: None,
            column: None,
        }
    }

    /// `field` muss ein im Schema fest vorgegebener Pfad sein, kein Eingabewert.
    pub fn invalid(field: &'static str) -> Self {
        Self {
            field: Some(field),
            ..Self::new(ErrorKind::InvalidValue)
        }
    }

    fn parse_at(input: &str, offset: Option<usize>) -> Self {
        let (line, column) = match offset {
            Some(offset) => {
                // Spans sind Bytepositionen. Nur Zahlen weitergeben, keine Zeile.
                let prefix = &input.as_bytes()[..offset.min(input.len())];
                let line = prefix.iter().filter(|&&byte| byte == b'\n').count() + 1;
                let column = prefix
                    .iter()
                    .rposition(|&byte| byte == b'\n')
                    .map_or(prefix.len() + 1, |last| prefix.len() - last);
                (Some(line), Some(column))
            }
            None => (None, None),
        };
        Self {
            line,
            column,
            ..Self::new(ErrorKind::InvalidDocument)
        }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            ErrorKind::ConfigArgumentMissing => "Die Option --config mit einem absoluten Dateipfad fehlt.",
            ErrorKind::ConfigArgumentDuplicate => "Die Option --config darf nicht mehrfach vorkommen.",
            ErrorKind::AbsolutePathRequired => "Der Konfigurationspfad muss absolut sein.",
            ErrorKind::FileMissing => "Die Konfigurationsdatei fehlt.",
            ErrorKind::FileUnreadable => "Die Konfigurationsdatei ist nicht lesbar.",
            ErrorKind::RegularFileRequired => "Die Konfiguration muss eine reguläre Datei sein.",
            ErrorKind::FileTooLarge => "Die Konfigurationsdatei überschreitet die Größenbegrenzung.",
            ErrorKind::InvalidEncoding => "Die Konfigurationsdatei muss UTF-8 enthalten.",
            ErrorKind::InvalidDocument => "Die Konfiguration enthält ungültiges TOML, unbekannte Felder, falsche Typen oder fehlende Pflichtwerte.",
            ErrorKind::InvalidValue => "Ein Konfigurationswert liegt außerhalb der erlaubten Werte.",
            ErrorKind::RestartRequired => "Die geprüfte Änderung benötigt einen Dienstneustart; die aktive Konfiguration bleibt unverändert.",
        };
        f.write_str(message)?;
        if let Some(field) = self.field {
            write!(f, " Feld: {field}.")?;
        }
        if let (Some(line), Some(column)) = (self.line, self.column) {
            write!(f, " Position: Zeile {line}, Spalte {column}.")?;
        }
        Ok(())
    }
}

impl std::error::Error for FileError {}

/// Schema-Prüfung ist rein: keine Clients, Verbindungen, Verzeichnisse oder Jobs.
pub trait Schema: DeserializeOwned + Serialize {
    fn validate(&self) -> Result<(), FileError>;
}

/// Nach erfolgreicher Gesamtprüfung unveränderliche Momentaufnahme.
///
/// Interne relative Pfade beziehen sich auf den aufgelösten Ort der Datei,
/// nicht auf das Arbeitsverzeichnis des Prozesses. Dateiinhalte werden nicht
/// durch `Debug` oder Fehlerketten ausgegeben.
pub struct Snapshot<T> {
    settings: Arc<T>,
    source: PathBuf,
    directory: PathBuf,
    fingerprint: String,
}

impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        Self {
            settings: Arc::clone(&self.settings),
            source: self.source.clone(),
            directory: self.directory.clone(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

impl<T> fmt::Debug for Snapshot<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigSnapshot")
            .field("validated", &true)
            .finish_non_exhaustive()
    }
}

impl<T: Schema> Snapshot<T> {
    pub fn load(path: &Path) -> Result<Self, FileError> {
        Self::load_document(path).map(|(snapshot, _)| snapshot)
    }

    /// Snapshot und Originaltext stammen aus demselben begrenzten Lesevorgang.
    /// Der Text ist nur für kommentarerhaltende Editoren, nie für Status-APIs.
    pub fn load_document(path: &Path) -> Result<(Self, String), FileError> {
        if !path.is_absolute() {
            return Err(FileError::new(ErrorKind::AbsolutePathRequired));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Ein als Datei angegebener FIFO darf schon das Öffnen nicht blockieren.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(path).map_err(|error| {
            FileError::new(if error.kind() == std::io::ErrorKind::NotFound {
                ErrorKind::FileMissing
            } else {
                ErrorKind::FileUnreadable
            })
        })?;
        let metadata = file
            .metadata()
            .map_err(|_| FileError::new(ErrorKind::FileUnreadable))?;
        if !metadata.is_file() {
            return Err(FileError::new(ErrorKind::RegularFileRequired));
        }
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(FileError::new(ErrorKind::FileTooLarge));
        }
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| FileError::new(ErrorKind::FileUnreadable))?;
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(FileError::new(ErrorKind::FileTooLarge));
        }
        let input =
            std::str::from_utf8(&bytes).map_err(|_| FileError::new(ErrorKind::InvalidEncoding))?;
        let source = path
            .canonicalize()
            .map_err(|_| FileError::new(ErrorKind::FileUnreadable))?;
        Self::parse(input, &source).map(|snapshot| (snapshot, input.to_string()))
    }

    /// Auch Importwerkzeuge und Tests benutzen denselben Prüfschritt.
    /// `source` ist dabei der absolute spätere Dateipfad, kein ENV-Wert.
    pub fn parse(input: &str, source: &Path) -> Result<Self, FileError> {
        if !source.is_absolute() {
            return Err(FileError::new(ErrorKind::AbsolutePathRequired));
        }
        if input.len() as u64 > MAX_CONFIG_BYTES {
            return Err(FileError::new(ErrorKind::FileTooLarge));
        }
        let settings: T = toml::from_str(input).map_err(|error: toml::de::Error| {
            FileError::parse_at(input, error.span().map(|s| s.start))
        })?;
        settings.validate()?;
        let source = normalized_path(source)?;
        let directory = source
            .parent()
            .ok_or_else(|| FileError::new(ErrorKind::AbsolutePathRequired))?
            .to_path_buf();
        let encoded = serde_json::to_vec(&settings)
            .map_err(|_| FileError::new(ErrorKind::InvalidDocument))?;
        let fingerprint = Sha256::digest(&encoded)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(Self {
            settings: Arc::new(settings),
            source,
            directory,
            fingerprint,
        })
    }

    pub fn settings(&self) -> &T {
        &self.settings
    }
    pub fn shared_settings(&self) -> Arc<T> {
        Arc::clone(&self.settings)
    }
    pub fn source(&self) -> &Path {
        &self.source
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn resolve(&self, path: &Path) -> Result<PathBuf, FileError> {
        if path.as_os_str().is_empty() {
            return Err(FileError::invalid("paths"));
        }
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.directory.join(path)
        };
        normalized_path(&joined)
    }

    /// Es gibt bewusst kein Hot-Reload. Ein Recheck prüft den gesamten Kandidaten
    /// und meldet Neustartpflicht, bevor ein neuer Wert an Verbraucher gelangen
    /// könnte. Der Aufrufer behält auch bei Parse- und Prüfungsfehlern `self`.
    pub fn recheck(&self) -> Result<(), FileError> {
        let candidate = Self::load(&self.source)?;
        if candidate.fingerprint != self.fingerprint || candidate.directory != self.directory {
            return Err(FileError::new(ErrorKind::RestartRequired));
        }
        Ok(())
    }
}

fn normalized_path(path: &Path) -> Result<PathBuf, FileError> {
    if !path.is_absolute() {
        return Err(FileError::new(ErrorKind::AbsolutePathRequired));
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err(FileError::invalid("paths"));
                }
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                result.push(component)
            }
        }
    }
    Ok(result)
}

/// Andere CLI-Argumente bleiben für bestehende Fach-CLIs unverändert erhalten.
#[derive(Debug, PartialEq, Eq)]
pub struct ConfigArguments {
    pub path: PathBuf,
    pub remaining: Vec<OsString>,
}

impl ConfigArguments {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, FileError> {
        let mut arguments = arguments.into_iter();
        let mut path = None;
        let mut remaining = Vec::new();
        while let Some(argument) = arguments.next() {
            let value = if argument == "--config" {
                Some(
                    arguments
                        .next()
                        .ok_or_else(|| FileError::new(ErrorKind::ConfigArgumentMissing))?,
                )
            } else {
                argument
                    .to_str()
                    .and_then(|arg| arg.strip_prefix("--config="))
                    .map(OsString::from)
            };
            if let Some(value) = value {
                if path.is_some() {
                    return Err(FileError::new(ErrorKind::ConfigArgumentDuplicate));
                }
                let value = PathBuf::from(value);
                if !value.is_absolute() {
                    return Err(FileError::new(ErrorKind::AbsolutePathRequired));
                }
                path = Some(value);
            } else {
                remaining.push(argument);
            }
        }
        Ok(Self {
            path: path.ok_or_else(|| FileError::new(ErrorKind::ConfigArgumentMissing))?,
            remaining,
        })
    }
}

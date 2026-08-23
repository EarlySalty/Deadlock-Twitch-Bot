//! Fehler des VOD-Archivs.

use tb_social_media::uploaders::UploadError;

#[derive(Debug, thiserror::Error)]
pub enum VodArchiveError {
    #[error("{programm} hat die Zeitgrenze von {sekunden}s ueberschritten")]
    Zeitgrenze { programm: String, sekunden: u64 },
    #[error("{schritt} fehlgeschlagen: {meldung}")]
    Werkzeug { schritt: String, meldung: String },
    #[error("Datei fehlt: {0}")]
    DateiFehlt(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("upload: {0}")]
    Upload(#[from] UploadError),
}

impl VodArchiveError {
    /// Das YouTube-Tageskontingent ist erschoepft. Kein Defekt, sondern ein
    /// Grund, den Rest auf den naechsten Lauf zu verschieben, ohne den Zustand
    /// als fehlerhaft zu markieren.
    pub fn ist_kontingent(&self) -> bool {
        matches!(self, Self::Upload(UploadError::QuotaExceeded(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kontingent_wird_erkannt() {
        let kontingent = VodArchiveError::Upload(UploadError::QuotaExceeded("voll".into()));
        assert!(kontingent.ist_kontingent());
        let anderer = VodArchiveError::Upload(UploadError::Api("kaputt".into()));
        assert!(!anderer.ist_kontingent());
        assert!(!VodArchiveError::DateiFehlt("/archiv/v1.mp4".into()).ist_kontingent());
    }
}

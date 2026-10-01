//! Fachlich geteilte Betriebswerte für Bot, Dashboard und Medienworker.
use crate::file::FileError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct KnowledgePaths {
    pub directory: PathBuf,
}
impl Default for KnowledgePaths {
    fn default() -> Self {
        Self {
            directory: "rust/knowledge".into(),
        }
    }
}
impl KnowledgePaths {
    pub(crate) fn validate(&self) -> Result<(), FileError> {
        let path = self
            .directory
            .to_str()
            .ok_or_else(|| FileError::invalid("knowledge.directory"))?;
        if path.trim().is_empty() || path.contains("://") || path.chars().any(char::is_control) {
            return Err(FileError::invalid("knowledge.directory"));
        }
        Ok(())
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaPublicOptions {
    pub youtube_audit_passed: bool,
    pub social_media_public_origin: String,
}
impl Default for MediaPublicOptions {
    fn default() -> Self {
        Self {
            youtube_audit_passed: false,
            social_media_public_origin: "https://admin.deutsche-deadlock-community.de".into(),
        }
    }
}
impl MediaPublicOptions {
    pub(crate) fn validate(&self) -> Result<(), FileError> {
        crate::global::public_url(
            &self.social_media_public_origin,
            "media.social_media_public_origin",
            false,
        )
    }
}

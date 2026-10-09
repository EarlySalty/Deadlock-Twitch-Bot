use crate::file::FileError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CategoryArchive {
    pub enabled: bool,
    pub remove_enabled: bool,
    pub temporary_directory: PathBuf,
    pub max_file_bytes: u64,
    pub min_free_bytes: u64,
    pub command_timeout_seconds: u64,
}
impl Default for CategoryArchive {
    fn default() -> Self {
        Self {
            enabled: false,
            remove_enabled: false,
            temporary_directory: PathBuf::from("/tmp"),
            max_file_bytes: 2 * 1024 * 1024 * 1024,
            min_free_bytes: 10 * 1024 * 1024 * 1024,
            command_timeout_seconds: 1800,
        }
    }
}
impl CategoryArchive {
    pub fn validate(&self) -> Result<(), FileError> {
        if !self.temporary_directory.is_absolute()
            || !(1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&self.max_file_bytes)
            || self.min_free_bytes < 1024 * 1024 * 1024
            || !(10..=7200).contains(&self.command_timeout_seconds)
        {
            return Err(FileError::invalid("category_archive"));
        }
        Ok(())
    }
}

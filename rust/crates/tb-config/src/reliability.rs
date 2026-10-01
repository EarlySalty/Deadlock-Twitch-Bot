//! Bestehende Transaktions- und Monitoring-Betriebsgrenzen.
use crate::file::FileError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TransactionRetry {
    pub attempts: u32,
    pub base_delay_seconds: f64,
    pub max_delay_seconds: f64,
}
impl Default for TransactionRetry {
    fn default() -> Self {
        Self { attempts: 3, base_delay_seconds: 0.1, max_delay_seconds: 0.75 }
    }
}
impl TransactionRetry {
    pub(crate) fn validate(&self) -> Result<(), FileError> {
        if self.attempts == 0 { return Err(FileError::invalid("database.retry.attempts")); }
        for (value, minimum, field) in [
            (self.base_delay_seconds, 0.01, "database.retry.base_delay_seconds"),
            (self.max_delay_seconds, 0.05, "database.retry.max_delay_seconds"),
        ] {
            if !value.is_finite() || value < minimum || std::time::Duration::try_from_secs_f64(value).is_err() {
                return Err(FileError::invalid(field));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MonitoringOptions {
    pub scout_enabled: bool,
    pub capacity_sample_seconds: u64,
    pub capacity_retention_days: i64,
    pub observability_retention_days: i64,
}
impl Default for MonitoringOptions {
    fn default() -> Self {
        Self { scout_enabled: false, capacity_sample_seconds: 300, capacity_retention_days: 45, observability_retention_days: 45 }
    }
}
impl MonitoringOptions {
    pub(crate) fn validate(&self) -> Result<(), FileError> {
        crate::global::range(self.capacity_sample_seconds, 30, 3600, "monitoring.capacity_sample_seconds")?;
        for (value, field) in [
            (self.capacity_retention_days, "monitoring.capacity_retention_days"),
            (self.observability_retention_days, "monitoring.observability_retention_days"),
        ] {
            if !(7..=365).contains(&value) { return Err(FileError::invalid(field)); }
        }
        Ok(())
    }
}

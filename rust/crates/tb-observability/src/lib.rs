//! Observability-Setup: strukturiertes Logging via `tracing` plus der
//! Observability-Event-Writer und die Flow-Services (raid + analytics).
//!
//! - `init_tracing`: Subscriber-Init (fmt + EnvFilter via `RUST_LOG`).
//! - [`ObservabilityWriter`]: deferred mpsc-Writer → `twitch_observability_events`.
//! - [`RaidObservabilityService`]: Raid-Flow-Events, Counter, Flow-IDs.
//! - [`AnalyticsObservabilityService`]: Analytics-Decision-Logging + Snapshot.

pub mod analytics_service;
pub mod event;
pub mod raid_service;
pub mod value;
pub mod writer;

pub use analytics_service::{
    AnalyticsDecision, AnalyticsObservabilityService, AnalyticsObservabilitySnapshot,
};
pub use event::{ObservabilityEvent, StoragePayload};
pub use raid_service::{EventSink, MillisSource, RaidObservabilityService};
pub use value::{format_fields, normalize_value, safe_observability_text, DEFAULT_VALUE_LIMIT};
pub use writer::{
    sanitize_payload, ObservabilityRow, ObservabilityWriter, DEFAULT_BATCH_SIZE,
    DEFAULT_QUEUE_CAPACITY,
};

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialisiert das globale Tracing-Subscriber. Idempotent: ein zweiter Aufruf
/// gibt `false` zurück, statt zu paniken (nützlich in Tests/Mehrfach-Init).
pub fn init_tracing() -> bool {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .try_init()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        // Beide Aufrufe dürfen nicht paniken — Rückgabewert wird bewusst ignoriert,
        // da er von der Test-Ausführungsreihenfolge abhängt.
        let _first = init_tracing();
        let _second = init_tracing();
    }
}

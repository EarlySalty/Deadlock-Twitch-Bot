//! Observability-Setup: strukturiertes Logging via `tracing`.
//!
//! Phase 0b: nur Subscriber-Init (fmt + EnvFilter via `RUST_LOG`). Der
//! Observability-Event-Writer (mpsc → `twitch_observability_events`) kommt mit
//! der Monitoring-Phase.

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

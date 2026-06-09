//! Prozess-Laufzeitinformationen für den Health-Endpoint.
//!
//! `PROCESS_START` wird beim ersten Aufruf von `uptime_secs()` via `OnceLock`
//! initialisiert. `main.rs` ruft die Funktion einmalig VOR `axum::serve` auf,
//! damit der Timestamp in der Start-Phase gesetzt wird.

use std::sync::OnceLock;
use std::time::Instant;

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Sekunden seit dem ersten Aufruf dieser Funktion.
///
/// Beim ersten Aufruf in `main.rs` (vor `axum::serve`) gesetzt —
/// danach unveränderlich.
pub fn uptime_secs() -> u64 {
    PROCESS_START.get_or_init(Instant::now).elapsed().as_secs()
}

/// RSS-Memory aus `/proc/self/status`, Zeile `VmRSS:`.
///
/// Gibt `None` zurück wenn Datei fehlt oder nicht parsbar (Nicht-Linux).
pub fn memory_rss_bytes() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // Format: "VmRSS:\t12345 kB"
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

/// PID des aktuellen Prozesses.
pub fn pid() -> u32 {
    std::process::id()
}

//! tb-monitoring — Monitoring-Subsystem des Twitch-Bots (Schritt 4).
//!
//! Slice 4a: das Idempotenz-Fundament, auf dem alle Ingress-Pfade ruhen:
//!
//! - [`guard`] — persistenter Guard-Store (`eventsub_guard_state`), das
//!   Exactly-once-Primitiv (Message-Dedup, Offline-Throttle, Business-Effekte).
//! - [`inbox_store`] / [`inbox_runtime`] — durable Work-Queue
//!   (`twitch_eventsub_processing_inbox`/`_dead_letter`) mit Leased-Worker,
//!   die Empfang von Verarbeitung entkoppelt.
//!
//! Beide Tabellen teilen sich Python und Rust während der Migration
//! (Schema-Vertrag, siehe `docs/02-db-contract.md`); die Semantik ist
//! deshalb 1:1 zum Python-Original — bewusste Abweichungen sind im
//! Plan-Doc `docs/plans/2026-06-09-schritt-4-monitoring.md` dokumentiert.

pub mod guard;
pub mod inbox_runtime;
pub mod inbox_store;

pub use guard::{GuardKind, GuardStore};
pub use inbox_runtime::{
    ClockFn, DeadLetterHook, DeadLetterNotice, HandlerError, InboxHandler, InboxRuntime,
    InboxRuntimeHandle,
};
pub use inbox_store::{DeadLetterEntry, LeasedWork, PendingEntry, ProcessingInboxStore};

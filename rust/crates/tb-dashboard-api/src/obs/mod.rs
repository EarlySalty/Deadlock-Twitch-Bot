//! Das WebSocket-Gateway der eigenen OBS-Docks (Plan Abschnitt 2.3).
//!
//! Zwei Teile:
//! - [`bus`] verteilt: ein `PgListener` je Prozess auf dem Postgres-Kanal
//!   `obs_dock`, dahinter ein `broadcast`-Kanal je `channel_id`.
//! - [`ws`] bedient: Auth vor dem Upgrade, Nachlauf aus `obs_dock_events`,
//!   danach live, mit Heartbeat und Socketgrenzen.
//!
//! Der Schreibpfad (Migration `obs_dock_events`, Hooks-Wrapper und
//! `pg_notify`) gehoert zu Auftrag B und liegt nicht in diesem Crate. Hier
//! wird nur gelesen; das Drahtformat ist `tb_platform_core::PlatformEvent` und
//! wird unveraendert durchgereicht.

pub mod bus;
pub mod ws;

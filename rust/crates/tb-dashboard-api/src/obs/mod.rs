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
//! wird nur gelesen.
//!
//! Das Ereignisformat `tb_platform_core::PlatformEvent` wird unveraendert
//! durchgereicht, aber nicht nackt: der Socket legt eine Huelle mit der
//! `obs_dock_events.id` darum (`{"id":123,"ereignis":{...}}`), sonst koennte
//! ein Dock nach einem Neustart sein `?seit=` gar nicht setzen. Einzelheiten
//! im Kopf von [`ws`].

pub mod bus;
pub mod ws;

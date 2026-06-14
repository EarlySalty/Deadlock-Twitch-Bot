//! tb-highlight — automatischer Highlight-Clipper für aktive Partner-Streamer.
//!
//! Port von `bot/highlight_clipper/`. Die Pipeline lädt Deadlock-Match-Demos,
//! erkennt Highlight-Momente (Multikills/Teamfights/Clutches) über das externe
//! `boon`-Binary (Source-2-Demo-Parser, `tools/boon`), schneidet Clips aus dem
//! Twitch-VOD per ffmpeg und stellt sie bereit. Python ist dabei nur
//! Orchestrator + Output-Parser — das schwere Demo-Parsing steckt im Binary,
//! sodass der Rust-Port dieselben `boon`-Subkommandos via Subprocess aufruft.
//!
//! Slices (bottom-up): [`config`] (Konstanten), [`deadlock_client`]
//! (Match-History/Metadata-API), [`demo_downloader`] (Demo-Download +
//! bz2-Entpacken + lokaler Cache), [`boon`] (Subprocess-Wrapper + Output-Parser
//! des Demo-Parser-Binaries), [`event_detector`] (API-basierte Event-Erkennung
//! + geteilter [`event_detector::HighlightEvent`]-Typ).

pub mod boon;
pub mod config;
pub mod deadlock_client;
pub mod demo_analyzer;
pub mod demo_downloader;
pub mod event_detector;
pub mod highlight_sender;
pub mod state;
pub mod twitch_vod;
pub mod worker;

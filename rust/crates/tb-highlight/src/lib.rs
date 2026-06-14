//! tb-highlight — automatischer Highlight-Clipper für aktive Partner-Streamer.
//!
//! Port von `bot/highlight_clipper/`. Die Pipeline lädt Deadlock-Match-Demos,
//! erkennt Highlight-Momente (Multikills/Teamfights/Clutches) über das externe
//! `boon`-Binary (Source-2-Demo-Parser, `tools/boon`), schneidet Clips aus dem
//! Twitch-VOD per ffmpeg und stellt sie bereit. Python ist dabei nur
//! Orchestrator + Output-Parser — das schwere Demo-Parsing steckt im Binary,
//! sodass der Rust-Port dieselben `boon`-Subkommandos via Subprocess aufruft.
//!
//! Slices (bottom-up): [`deadlock_client`] (Match-History/Metadata-API),
//! [`demo_downloader`] (Demo-Download + bz2-Entpacken + lokaler Cache).

pub mod deadlock_client;
pub mod demo_downloader;

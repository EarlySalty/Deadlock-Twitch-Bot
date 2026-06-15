//! tb-engagement — KI-Chat-Engagement für den Twitch-Chat (Port von
//! `bot/engagement/`).
//!
//! Pro eingehender Chat-Message läuft die [`pipeline`] eine Gate-Kaskade
//! (Settings → Partner → Live → Opt-out → Rhythmus → Pre-Filter → Flood/Burst)
//! und baut bei Durchlass aus ~15 optionalen Kontext-Fragmenten (Persona,
//! Threads, Lurker, Match, Wiki, Patches, Stats, Transkripte, Sentiment …) den
//! System-Prompt für den MiniMax-Call. Antwortet das Modell, geht der Text in
//! den Chat.
//!
//! Aufbau bottom-up in Teil-Slices; Slice 1 (hier): Kern-[`types`] + die reinen
//! Pipeline-Helfer in [`pipeline`] (Pre-Filter + Kostenrechnung).

pub mod channel_background;
pub mod conversation;
pub mod deadlock_patches;
pub mod deadlock_stats;
pub mod deadlock_wiki;
pub mod match_context;
pub mod minimax_chat;
pub mod persona;
pub mod pipeline;
pub mod rhythm;
pub mod soul_store;
pub mod stream_transcripts;
pub mod style_examples;
pub mod types;

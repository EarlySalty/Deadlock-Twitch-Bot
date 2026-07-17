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

pub mod audio_capture;
pub mod auto_off;
pub mod background;
pub mod channel_background;
pub mod claude_chat;
pub mod conversation;
pub mod crew_review;
pub mod crew_review_store;
pub mod deadlock_patches;
pub mod deadlock_stats;
pub mod deadlock_wiki;
pub mod gate;
pub mod global_sentiment;
pub mod irc_message;
pub mod irc_reader;
pub mod lurker_signal;
pub mod match_context;
pub mod minimax_chat;
pub mod persona;
pub mod pipeline;
pub mod rhythm;
pub mod sender_auth;
pub mod shadow_review;
pub mod soul_store;
pub mod stealth_sender;
pub mod stream_state;
pub mod stream_transcripts;
pub mod style_examples;
pub mod threads;
pub mod transcribe;
pub mod types;

pub use crew_review::CrewReviewTrigger;

//! Poll-Loop-Subsystem: Engine ([`engine`]), Ports ([`source`], [`hooks`]),
//! Tracking-Loader ([`tracked`]) und Runtime-Intervall ([`settings`]).

pub mod engine;
pub mod hooks;
pub mod settings;
pub mod source;
pub mod tracked;

pub use engine::{PollConfig, PollEngine};
pub use hooks::{
    AnnounceLiveRequest, AnnounceLiveResult, AnnouncementSink, EndAnnouncementOutcome,
    EndAnnouncementRequest, NoopAnnouncementSink, NoopPollHooks, PollHooks, ScoreRefresh,
    TickReport,
};
pub use settings::{PollIntervalStore, POLL_INTERVAL_DEFAULT_SECONDS};
pub use source::{SourceError, StreamSource};
pub use tracked::{TrackedEntry, TrackedStore};

//! Stream-Session-Subsystem: reine Kennzahlen ([`metrics`]), DB-Zugriff
//! ([`store`]) und Lebenszyklus-Orchestrierung ([`tracker`]).

pub mod metrics;
pub mod store;
pub mod tracker;

pub use metrics::{Aggregates, Dropoff, ViewerSample};
pub use store::{
    FinalizeSource, FinalizeUpdate, NewSession, OpenSession, OrphanCandidate, SessionStore,
    StartOutcome,
};
pub use tracker::{
    FollowerCountSource, FollowerFetch, NoFollowerSource, NoRaidTrackingResolver,
    RaidTrackingResolver, SessionTracker,
};

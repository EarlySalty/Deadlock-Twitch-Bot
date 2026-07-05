//! tb-monitoring — Monitoring-Subsystem des Twitch-Bots (Schritt 4).
//!
//! Slice 4a — Idempotenz-Fundament:
//!
//! - [`guard`] — persistenter Guard-Store (`eventsub_guard_state`), das
//!   Exactly-once-Primitiv (Message-Dedup, Offline-Throttle, Business-Effekte).
//! - [`inbox_store`] / [`inbox_runtime`] — durable Work-Queue
//!   (`twitch_eventsub_processing_inbox`/`_dead_letter`) mit Leased-Worker,
//!   die Empfang von Verarbeitung entkoppelt.
//!
//! Slice 4b — Write-Core des Poll-Pfads:
//!
//! - [`live_state`] — „Wer ist live"-Wahrheit (`twitch_live_state`).
//! - [`sessions`] — Session-Lebenszyklus (`twitch_stream_sessions` + Viewers).
//! - [`stats`] — Time-Series (`twitch_stats_tracked`/`_category`).
//! - [`exp_sessions`] — dünne Hooks des Experimental-Analytics (Fork 2).
//! - [`stream`] — Domänen-Sicht auf einen Helix-Stream + Zeit-Helfer.
//!
//! Die Tabellen teilen sich Python und Rust während der Migration
//! (Schema-Vertrag, siehe `docs/02-db-contract.md`); die Semantik ist
//! deshalb 1:1 zum Python-Original — bewusste Abweichungen sind im
//! Plan-Doc `docs/plans/2026-06-09-schritt-4-monitoring.md` dokumentiert.

pub mod announce;
pub mod chatters_poller;
pub mod dispatch;
pub mod exp_sessions;
pub mod guard;
pub mod handlers;
pub mod inbox_runtime;
pub mod inbox_store;
pub mod irc_lurker;
pub mod live_state;
pub mod observability_retention;
pub mod poller;
pub mod raid_retention;
pub mod scout;
pub mod sessions;
pub mod stats;
pub mod stream;
pub mod subscriptions;
pub mod telemetry;
pub mod webhook_receiver;
pub use webhook_receiver::WebhookReceiver;

pub use announce::{
    AnnouncementSettings, AnnouncementTransport, BrokerAnnouncementSink, ChannelProfileSource,
    LivePingRoleProvider, NoChannelProfile, NoVodPreview, VodPreviewSource,
};
pub use chatters_poller::{
    load_live_roster, record_chatters_for_streamer, BotChatterAuth, ChattersCollector,
    ChattersFetcher, CycleStats, KeyedCooldown, LiveStreamer, SelfHealCooldowns,
    StreamerTokenSource,
};
pub use dispatch::{
    classify_chat_notification, has_registered_handler, ChatNotificationKind, DispatchNotReady,
    DispatchOutcome, EventSubDispatcher, EventSubHooks, NoopEventSubHooks,
};
pub use exp_sessions::{ExpSessionStore, ExpSessionTracker};
pub use guard::{GuardKind, GuardStore};
pub use handlers::MonitoringEventHandler;
pub use inbox_runtime::{
    epoch_clock, ClockFn, DeadLetterHook, DeadLetterNotice, HandlerError, InboxEnqueuer,
    InboxHandler, InboxRuntime, InboxRuntimeHandle,
};
pub use inbox_store::{DeadLetterEntry, LeasedWork, PendingEntry, ProcessingInboxStore};
pub use irc_lurker::{record_presence_ticks, IrcLurkerTracker, TrackMode};
pub use live_state::{
    FinalizeState, LiveStateRow, LiveStateStore, LiveStateUpsert, OfflineSourceState,
    SnapshotEntry, TrackedStreamer,
};
pub use observability_retention::{
    cleanup_observability_events, cleanup_observability_events_before,
    observability_retention_days, OBSERVABILITY_RETENTION_DEFAULT_DAYS,
};
pub use poller::{
    AnnouncementSink, NoopAnnouncementSink, NoopPollHooks, PollConfig, PollEngine, PollHooks,
    PollIntervalStore, ScoreRefresh, StreamSource, TickReport, TrackedEntry, TrackedStore,
};
pub use raid_retention::{compute_raid_retention, RetentionStats};
pub use scout::{build_scout_task, NoopScoutChatSink, ScoutChatSink, ScoutTask};
pub use sessions::{
    FollowerCountSource, FollowerFetch, NewSession, NoFollowerSource, NoRaidTrackingResolver,
    RaidTrackingResolver, SessionStore, SessionTracker, StartOutcome,
};
pub use stats::{StatsSample, StatsStore};
pub use stream::StreamSnapshot;
pub use subscriptions::{
    eventsub_webhook_capacity_values, BroadcasterEventSubTokenProvider, CapacitySnapshotStore,
    EventSubCapacityValues, EventSubUserToken, ModeratorProvisionOutcome, ModeratorProvisioner,
    RemoteSubscription, RevocationSink, SubscriptionConfig, SubscriptionCreateError,
    SubscriptionEnsureReport, SubscriptionFailureCounter, SubscriptionFailureStatus,
    SubscriptionManager, SubscriptionTransport, EVENTSUB_CORE_SUB_TYPES,
};
pub use telemetry::{HypeTrainPhase, TelemetryStore};

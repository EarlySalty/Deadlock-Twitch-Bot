//! Hooks des Poll-Loops zu Nachbar-Subsystemen.
//!
//! Der Engine-Kern bleibt frei von Discord/EventSub/Raid-Wissen:
//! - [`AnnouncementSink`] — Go-Live-/Offline-Postings (Slice 4e).
//! - [`PollHooks`] — EventSub-Subscription bei Go-Live (4d), Partner-Score-
//!   Refreshes und Partner-Lifecycle-Ops (Cutover-Kopplungen, siehe Plan-Doc).
//!
//! Bis zur Verdrahtung laufen die Noop-Implementierungen — der Poll-Loop ist
//! damit ein reiner Write-Core-Treiber ohne Außenwirkung.

use crate::poller::tracked::TrackedEntry;
use crate::stream::StreamSnapshot;

/// Kontext für ein Go-Live-Posting.
#[derive(Debug, Clone)]
pub struct AnnounceLiveRequest {
    pub login: String,
    pub entry: TrackedEntry,
    pub stream: StreamSnapshot,
    pub previous_message_id: Option<String>,
    pub previous_tracking_token: Option<String>,
    pub stream_id: Option<String>,
    pub started_at_iso: Option<String>,
    pub active_session_id: Option<i64>,
}

/// Ergebnis eines erfolgreichen Go-Live-Postings.
#[derive(Debug, Clone)]
pub struct AnnounceLiveResult {
    pub message_id: String,
    pub tracking_token: Option<String>,
    /// Gesendeter Text — wird als `notification_text` an der Session gespeichert.
    pub notification_text: String,
}

/// Kontext für das Beenden eines Postings (Offline-/VOD-Embed-Edit).
#[derive(Debug, Clone)]
pub struct EndAnnouncementRequest {
    pub login: String,
    pub display_name: String,
    pub message_id: String,
    pub previous_tracking_token: Option<String>,
    pub last_title: Option<String>,
    pub last_game: Option<String>,
    pub twitch_user_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndAnnouncementOutcome {
    /// Posting wurde zum Offline-Embed editiert → message_id austragen.
    Updated,
    /// Posting existiert nicht mehr → message_id austragen.
    Gone,
    /// Edit fehlgeschlagen → message_id behalten, nächster Tick versucht erneut.
    Failed,
}

#[async_trait::async_trait]
pub trait AnnouncementSink: Send + Sync {
    /// Ist ein Announcement-Transport konfiguriert und bereit?
    /// (Python `_announcement_transport_ready` — gated should_post.)
    fn ready(&self) -> bool;

    /// Go-Live-Posting senden. `None` = Senden fehlgeschlagen (Retry im
    /// nächsten Tick, der Sink verwaltet sein Retry-Payload selbst).
    async fn announce_live(&self, request: AnnounceLiveRequest) -> Option<AnnounceLiveResult>;

    /// Bestehendes Posting auf „Deadlock beendet" umstellen.
    async fn end_announcement(&self, request: EndAnnouncementRequest) -> EndAnnouncementOutcome;

    /// Streamer ist offline/neu gestartet — Retry-Zustand fürs Posting verwerfen.
    async fn on_stream_not_live(&self, _login: &str) {}
}

/// Sink ohne Transport: `ready() == false`, der Tick schreibt nur die DB.
pub struct NoopAnnouncementSink;

#[async_trait::async_trait]
impl AnnouncementSink for NoopAnnouncementSink {
    fn ready(&self) -> bool {
        false
    }
    async fn announce_live(&self, _request: AnnounceLiveRequest) -> Option<AnnounceLiveResult> {
        None
    }
    async fn end_announcement(&self, _request: EndAnnouncementRequest) -> EndAnnouncementOutcome {
        EndAnnouncementOutcome::Failed
    }
}

/// Ein fälliger Partner-Raid-Score-Refresh (Raid-Subsystem, Phase 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreRefresh {
    pub twitch_user_id: String,
    pub login: String,
    pub trigger: &'static str,
}

/// Zusammenfassung eines Ticks für nachgelagerte Subsysteme.
#[derive(Debug, Clone)]
pub struct TickReport {
    pub score_refreshes: Vec<ScoreRefresh>,
    /// Kategorie-Sample dieses Ticks (Partner-Rekrutierung/Outreach).
    pub category_streams: Vec<StreamSnapshot>,
}

#[async_trait::async_trait]
pub trait PollHooks: Send + Sync {
    /// Verifizierter Partner mit aktivem Raid-Bot ist frisch live gegangen —
    /// 4d registriert hier die `stream.offline`-Subscription.
    async fn on_stream_went_live(&self, _twitch_user_id: &str, _login: &str) {}

    /// Archivierter Partner streamt wieder Deadlock → entarchivieren.
    /// `true` = durchgeführt (der Tick behandelt ihn ab sofort als aktiv).
    async fn on_auto_unarchive(&self, _login: &str) -> bool {
        false
    }

    /// Partner war > N Tage nicht mit Deadlock live → archivieren.
    /// `true` = durchgeführt.
    async fn on_auto_archive(&self, _login: &str) -> bool {
        false
    }

    /// Tick-Abschluss: Score-Refreshes + Kategorie-Sample.
    async fn after_tick(&self, _report: TickReport) {}

    /// Jeder Poll-Tick: Gelegenheit, die EventSub-Capacity-Zeitreihe zu schreiben
    /// (B5-08). Die Drosselung (Sample-Intervall + Retention) liegt im Adapter
    /// bzw. im `SubscriptionManager`; der Engine ruft nur taktgebend auf. Default
    /// no-op (Setups ohne Subscription-Manager schreiben keine Zeitreihe).
    async fn on_capacity_tick(&self) {}
}

/// Hooks ohne Wirkung (bis 4d/4f verdrahten).
pub struct NoopPollHooks;

#[async_trait::async_trait]
impl PollHooks for NoopPollHooks {}

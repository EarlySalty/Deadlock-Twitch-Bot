//! Hooks des Poll-Loops zu Nachbar-Subsystemen.
//!
//! Der Engine-Kern bleibt frei von Discord/EventSub/Raid-Wissen:
//! - [`AnnouncementSink`] — Go-Live-/Offline-Postings (Slice 4e).
//! - [`PollHooks`] — EventSub-Subscription bei Go-Live (4d), Partner-Score-
//!   Refreshes, ReAuth-Reminder und Partner-Lifecycle-Ops
//!   (Cutover-Kopplungen, siehe Plan-Doc).
//!
//! Bis zur Verdrahtung laufen die Noop-Implementierungen — der Poll-Loop ist
//! damit ein reiner Write-Core-Treiber ohne Außenwirkung.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::PgPool;
use tb_chat::{ChatApi, SendOutcome};

use crate::poller::tracked::TrackedEntry;
use crate::stream::StreamSnapshot;

/// Kurzer Crash-/Reconnect-Korridor: In diesem Fenster wird ein beendeter
/// Live-Post wieder auf Live editiert; danach gibt es einen frischen Post.
pub(crate) const ANNOUNCE_REANNOUNCE_COOLDOWN_SECONDS: f64 = 5.0 * 60.0;

pub(crate) fn announcement_reannounce_cooldown_key(login: &str) -> Option<String> {
    let login = login.trim().to_lowercase();
    (!login.is_empty()).then(|| format!("announcement_reannounce:{login}"))
}

/// User-sichtbarer Chat-Text für den Poller-basierten Re-Auth-Reminder.
/// Byte-identisch zum kanonischen EventSub-Pfad (`bin/tb-bot/src/reauth_reminder.rs`).
///
/// ⚠️ NICHT VERDRAHTEN: Der produktive Re-Auth-Reminder läuft über den EventSub-
/// Go-Live-Pfad (`RaidEventSubHooks::on_stream_went_live` → `ReauthReminder`).
/// Diesen Poller-Hook zusätzlich einzuhängen würde pro Go-Live einen ZWEITEN
/// Reminder senden (eigener, getrennter In-Memory-Dedupe) — Doppel-Nachricht
/// ohne Funktionsgewinn. Die Konstante bleibt korrekt befüllt, falls je ein
/// EventSub-loses Setup einen geteilten DB-Dedupe bekommt.
pub const REAUTH_REMINDER_TEXT: &str = "Kurze Erinnerung: Fuer den Raid-/Stats-Bot fehlt noch die neue Twitch-Autorisierung. Bitte im Dashboard einloggen und Twitch neu verbinden. Falls du die DM brauchst: Der Re-Auth-Link wurde dir bereits auf Discord geschickt.";

const REAUTH_FALLBACK_DEDUPE_WINDOW: Duration = Duration::from_secs(300);

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
    /// Re-Announce innerhalb des Flap-Cooldowns: Posting ja, Rollen-Pings nein.
    pub suppress_role_pings: bool,
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
    pub started_at_iso: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndAnnouncementOutcome {
    /// Posting wurde editiert.
    Updated,
    /// Posting existiert nicht mehr.
    Gone,
    /// Edit fehlgeschlagen oder bewusst übersprungen.
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

    /// Bestehendes Live-Posting bei Resume/Preview-Bucket-Wechsel aktualisieren.
    async fn sync_live_announcement(
        &self,
        _request: AnnounceLiveRequest,
    ) -> EndAnnouncementOutcome {
        EndAnnouncementOutcome::Updated
    }

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
    /// Aktiver Partner ist frisch live gegangen — 4d registriert hier die
    /// `stream.offline`-Subscription.
    async fn on_stream_went_live(&self, _twitch_user_id: &str, _login: &str) {}

    /// Go-Live mit aktuellem Stream-Kontext. Default bleibt kompatibel zu
    /// Hooks, die die `stream_id` nicht brauchen.
    async fn on_stream_went_live_with_stream_id(
        &self,
        twitch_user_id: &str,
        login: &str,
        _stream_id: Option<&str>,
    ) {
        self.on_stream_went_live(twitch_user_id, login).await;
    }

    /// Aktiver Partner ist laut Poller offline gegangen — redundanter
    /// Auto-Raid-Trigger zum EventSub-`stream.offline`-Pfad.
    async fn on_stream_offline_raid(&self, _twitch_user_id: &str, _login: Option<&str>) {}

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

/// PollHooks-Decorator für den Go-Live-ReAuth-Reminder.
///
/// WIRING-TODO(P2.60): In `bin/tb-bot/src/main.rs` den bestehenden
/// `SubscriptionPollHooks` mit diesem Decorator umwickeln und den nativen
/// `tb_chat::ChatApi` injizieren. Ohne dieses Composition-Root-Wiring bleibt
/// der Poller-Sendepfad ungenutzt.
pub struct ReauthReminderPollHooks {
    inner: Arc<dyn PollHooks>,
    pool: PgPool,
    chat: Arc<dyn ChatApi>,
    sent: Mutex<HashMap<String, Instant>>,
}

impl ReauthReminderPollHooks {
    pub fn new(inner: Arc<dyn PollHooks>, pool: PgPool, chat: Arc<dyn ChatApi>) -> Self {
        Self {
            inner,
            pool,
            chat,
            sent: Mutex::new(HashMap::new()),
        }
    }

    async fn reminder_key(
        &self,
        twitch_user_id: &str,
        current_stream_id: Option<&str>,
    ) -> Option<ReminderKey> {
        let twitch_user_id = twitch_user_id.trim();
        if twitch_user_id.is_empty() {
            return None;
        }
        let row = sqlx::query!(
            "SELECT ra.needs_reauth, NULLIF(ls.last_stream_id, '') AS \"last_stream_id?\" \
               FROM twitch_raid_auth ra \
          LEFT JOIN twitch_live_state ls ON ls.twitch_user_id = ra.twitch_user_id \
              WHERE ra.twitch_user_id = $1",
            twitch_user_id,
        )
        .fetch_optional(&self.pool)
        .await;

        match row {
            Ok(Some(row)) if row.needs_reauth == Some(true) => {
                let stream_id = current_stream_id
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .or(row.last_stream_id);
                if let Some(stream_id) = stream_id {
                    Some(ReminderKey::Stream(format!("stream:{stream_id}")))
                } else {
                    Some(ReminderKey::Fallback(format!("fallback:{twitch_user_id}")))
                }
            }
            Ok(Some(_)) | Ok(None) => None,
            Err(error) => {
                tracing::debug!(%error, twitch_user_id, "ReAuth-Reminder: needs_reauth-Check fehlgeschlagen");
                None
            }
        }
    }

    fn claim_send(&self, key: ReminderKey) -> bool {
        let Ok(mut sent) = self.sent.lock() else {
            tracing::warn!("ReAuth-Reminder: Dedupe-Lock vergiftet");
            return false;
        };
        let now = Instant::now();
        match key {
            ReminderKey::Stream(key) => match sent.entry(key) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(now);
                    true
                }
                std::collections::hash_map::Entry::Occupied(_) => false,
            },
            ReminderKey::Fallback(key) => {
                let due = sent
                    .get(&key)
                    .map(|last| now.duration_since(*last) >= REAUTH_FALLBACK_DEDUPE_WINDOW)
                    .unwrap_or(true);
                if due {
                    sent.insert(key, now);
                }
                due
            }
        }
    }
    async fn maybe_send_reauth_reminder(
        &self,
        twitch_user_id: &str,
        login: &str,
        stream_id: Option<&str>,
    ) {
        let Some(key) = self.reminder_key(twitch_user_id, stream_id).await else {
            return;
        };
        if !self.claim_send(key) {
            return;
        }
        match self
            .chat
            .send_message(twitch_user_id.trim(), REAUTH_REMINDER_TEXT)
            .await
        {
            Ok(SendOutcome::Sent) => {
                tracing::info!(
                    login = %login.trim().to_lowercase(),
                    twitch_user_id = %twitch_user_id.trim(),
                    "ReAuth-Reminder bei Go-Live in den Chat gesendet"
                );
            }
            Ok(outcome) => {
                tracing::debug!(
                    ?outcome,
                    twitch_user_id = %twitch_user_id.trim(),
                    "ReAuth-Reminder nicht zugestellt"
                );
            }
            Err(error) => {
                tracing::debug!(%error, twitch_user_id = %twitch_user_id.trim(), "ReAuth-Reminder-Send fehlgeschlagen");
            }
        }
    }
}

enum ReminderKey {
    Stream(String),
    Fallback(String),
}

#[async_trait::async_trait]
impl PollHooks for ReauthReminderPollHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.inner.on_stream_went_live(twitch_user_id, login).await;
        self.maybe_send_reauth_reminder(twitch_user_id, login, None)
            .await;
    }

    async fn on_stream_went_live_with_stream_id(
        &self,
        twitch_user_id: &str,
        login: &str,
        stream_id: Option<&str>,
    ) {
        self.inner
            .on_stream_went_live_with_stream_id(twitch_user_id, login, stream_id)
            .await;
        self.maybe_send_reauth_reminder(twitch_user_id, login, stream_id)
            .await;
    }

    async fn on_stream_offline_raid(&self, twitch_user_id: &str, login: Option<&str>) {
        self.inner
            .on_stream_offline_raid(twitch_user_id, login)
            .await;
    }

    async fn on_auto_unarchive(&self, login: &str) -> bool {
        self.inner.on_auto_unarchive(login).await
    }

    async fn on_auto_archive(&self, login: &str) -> bool {
        self.inner.on_auto_archive(login).await
    }

    async fn after_tick(&self, report: TickReport) {
        self.inner.after_tick(report).await;
    }

    async fn on_capacity_tick(&self) {
        self.inner.on_capacity_tick().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{PollHooks, ReauthReminderPollHooks, REAUTH_REMINDER_TEXT};

    use std::str::FromStr;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Utc};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use tb_chat::{BanOutcome, ChatApi, SendOutcome};

    #[derive(Default)]
    struct RecordingChat {
        sent: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl ChatApi for RecordingChat {
        async fn send_message(
            &self,
            broadcaster_id: &str,
            message: &str,
        ) -> Result<SendOutcome, String> {
            self.sent
                .lock()
                .unwrap()
                .push((broadcaster_id.to_string(), message.to_string()));
            Ok(SendOutcome::Sent)
        }

        async fn send_announcement(
            &self,
            _broadcaster_id: &str,
            _message: &str,
            _color: &str,
        ) -> Result<bool, String> {
            Ok(false)
        }

        async fn ban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _reason: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Failed {
                status: 501,
                body: String::new(),
            })
        }

        async fn timeout_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _duration_secs: u32,
            _reason: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Failed {
                status: 501,
                body: String::new(),
            })
        }

        async fn unban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
        ) -> Result<bool, String> {
            Ok(false)
        }

        async fn delete_message(
            &self,
            _broadcaster_id: &str,
            _message_id: &str,
        ) -> Result<bool, String> {
            Ok(false)
        }

        async fn user_created_at(&self, _user_id: &str) -> Result<Option<DateTime<Utc>>, String> {
            Ok(None)
        }

        async fn resolve_user_id(&self, _login: &str) -> Result<Option<String>, String> {
            Ok(None)
        }

        async fn bot_user_id(&self) -> String {
            "bot".to_string()
        }
    }

    async fn pool_in_schema(schema: &str) -> Option<sqlx::PgPool> {
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return None;
        };
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY,
                needs_reauth BOOLEAN NOT NULL DEFAULT FALSE
            )",
            "CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY,
                last_stream_id TEXT
            )",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn reauth_reminder_sendet_einmal_pro_stream_id() {
        let Some(pool) = pool_in_schema("poll_hooks_reauth").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, needs_reauth) VALUES ('42', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, last_stream_id) VALUES ('42', 's-1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let chat = Arc::new(RecordingChat::default());
        let hooks = ReauthReminderPollHooks::new(
            Arc::new(super::NoopPollHooks),
            pool.clone(),
            chat.clone(),
        );
        hooks.on_stream_went_live("42", "drag").await;
        hooks.on_stream_went_live("42", "drag").await;

        sqlx::query(
            "UPDATE twitch_live_state SET last_stream_id = 's-2' WHERE twitch_user_id = '42'",
        )
        .execute(&pool)
        .await
        .unwrap();
        hooks.on_stream_went_live("42", "drag").await;

        let sent = chat.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[0],
            ("42".to_string(), REAUTH_REMINDER_TEXT.to_string())
        );
        assert!(REAUTH_REMINDER_TEXT.contains("Twitch-Autorisierung"));
    }

    #[tokio::test]
    async fn reauth_reminder_nutzt_aktuellen_stream_id_kontext_vor_db_stand() {
        let Some(pool) = pool_in_schema("poll_hooks_reauth_current_stream").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, needs_reauth) VALUES ('42', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, last_stream_id) VALUES ('42', 's-1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let chat = Arc::new(RecordingChat::default());
        let hooks = ReauthReminderPollHooks::new(
            Arc::new(super::NoopPollHooks),
            pool.clone(),
            chat.clone(),
        );
        hooks
            .on_stream_went_live_with_stream_id("42", "drag", Some("s-1"))
            .await;
        hooks
            .on_stream_went_live_with_stream_id("42", "drag", Some("s-1"))
            .await;
        hooks
            .on_stream_went_live_with_stream_id("42", "drag", Some("s-2"))
            .await;

        let sent = chat.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
    }
}

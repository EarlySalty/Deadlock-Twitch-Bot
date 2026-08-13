use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tb_chat::types::{ChatMessageEvent, SendOutcome};
use tb_chat::ChatApi;
use tb_monitoring::{LiveStateStore, WriteStats};
use tb_raid::courtesy::{classify, should_remind, CourtesyClass, CourtesyOutcome, CourtesySummary};
use tb_raid::courtesy_store::{CourtesyEvent, CourtesyStore, ObservationSource};
use tb_raid::{RaidGreetingMonitorPort, RaidGreetingRegistration};

// ponytail: 20 Min Kulanz; Pending ist prozess-lokal, Neustart verwirft es konservativ: lieber kein Whisper als ein falscher Vorwurf.
const GREETING_WINDOW: Duration = Duration::from_secs(20 * 60);

pub trait RaidTargetChatProbe: Send + Sync {
    fn watch(&self, channel: &str);
    fn unwatch(&self, channel: &str);
    /// Schreib-Statistik des Nicks im Kanal seit `since`. `None` = die
    /// Beobachtung ist nicht aussagefähig; daraus darf **nicht** auf Schweigen
    /// geschlossen werden.
    fn write_stats(&self, channel: &str, nick: &str, since: Instant) -> Option<WriteStats>;
}

/// Persistenz der Etikette-Beobachtungen. Als Trait, damit der Monitor nicht
/// direkt an der Datenbank hängt und in Tests ohne Postgres läuft.
#[async_trait::async_trait]
pub trait CourtesyRecorder: Send + Sync {
    /// Bisherige Etikette-Historie des Streamers (für die Whisper-Entscheidung).
    async fn summary(&self, from_broadcaster_id: &str) -> CourtesySummary;
    /// Wann ging zuletzt eine Erinnerung an ihn raus?
    async fn last_whisper_at(&self, from_broadcaster_id: &str) -> Option<DateTime<Utc>>;
    /// Hält die Beobachtung fest.
    async fn record(&self, event: CourtesyEvent);
}

/// Postgres-Implementierung über den [`CourtesyStore`].
pub struct DbCourtesyRecorder {
    store: CourtesyStore,
}

impl DbCourtesyRecorder {
    pub fn new(store: CourtesyStore) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl CourtesyRecorder for DbCourtesyRecorder {
    async fn summary(&self, from_broadcaster_id: &str) -> CourtesySummary {
        match self
            .store
            .summary_for(from_broadcaster_id, Utc::now())
            .await
        {
            Ok(summary) => summary,
            Err(error) => {
                tracing::warn!(%error, "Etikette-Historie nicht ladbar");
                CourtesySummary::default()
            }
        }
    }

    async fn last_whisper_at(&self, from_broadcaster_id: &str) -> Option<DateTime<Utc>> {
        match self.store.last_whisper_at(from_broadcaster_id).await {
            Ok(last) => last,
            Err(error) => {
                tracing::warn!(%error, "Letzten Erinnerungs-Zeitpunkt nicht ladbar");
                // Konservativ: unbekannt heißt hier "lange her", sonst bliebe
                // die Erinnerung bei jedem DB-Schluckauf dauerhaft aus.
                None
            }
        }
    }

    async fn record(&self, event: CourtesyEvent) {
        if let Err(error) = self.store.record(&event).await {
            tracing::warn!(%error, "Etikette-Beobachtung nicht speicherbar");
        }
    }
}

/// Senke für das Raid-Ziel, das Twitch im Quellkanal meldet (`channel.moderate`).
pub trait OutgoingRaidSink: Send + Sync {
    fn raid_retargeted(&self, registration: RaidGreetingRegistration);
}

impl OutgoingRaidSink for RaidGreetingMonitor {
    fn raid_retargeted(&self, registration: RaidGreetingRegistration) {
        RaidGreetingMonitor::raid_retargeted(self, registration);
    }
}

/// Läuft der Stream des Raid-Ziels noch? `None` = unbekannt (fremder Kanal,
/// DB-Fehler) — dann bleibt es beim bisherigen Verhalten.
#[async_trait::async_trait]
pub trait RaidTargetLiveProbe: Send + Sync {
    async fn is_live(&self, login: &str) -> Option<bool>;
}

#[derive(Debug, Clone)]
struct PendingGreeting {
    key: String,
    from_broadcaster_id: String,
    from_broadcaster_login: String,
    to_broadcaster_id: String,
    to_broadcaster_login: String,
    probe_watched: bool,
    probe_started_at: Instant,
    /// Zeitpunkt des Raid-Starts für die gespeicherte Beobachtung.
    started_at: DateTime<Utc>,
    /// Nachrichten des Raiders im Zielchat, die über EventSub hereinkamen.
    /// Das Fenster läuft bis zum Ende durch: für die Einstufung zählt nicht
    /// nur **ob**, sondern **wie viel** geschrieben wurde.
    eventsub_writes: Vec<Instant>,
}

impl PendingGreeting {
    /// Schreib-Statistik aus dem EventSub-Strom.
    fn eventsub_stats(&self) -> WriteStats {
        WriteStats {
            count: self.eventsub_writes.len() as u32,
            first_at: self.eventsub_writes.first().copied(),
            last_at: self.eventsub_writes.last().copied(),
        }
    }
}

/// Nimmt die aussagekräftigere der beiden Beobachtungen.
///
/// EventSub sieht nur Kanäle, in denen der Bot sitzt; die IRC-Beobachtung
/// deckt auch fremde Zielkanäle ab. Beide können lückenhaft sein, aber eine
/// gesehene Nachricht ist immer ein Fakt. Darum gewinnt die höhere Zählung.
fn merge_stats(
    eventsub: WriteStats,
    probe: Option<WriteStats>,
) -> (WriteStats, Option<ObservationSource>) {
    let Some(probe) = probe else {
        return if eventsub.count > 0 {
            (eventsub, Some(ObservationSource::EventSub))
        } else {
            // Keine IRC-Aussage und nichts über EventSub gesehen: das ist
            // keine Stille, sondern schlicht keine Messung.
            (eventsub, None)
        };
    };
    if eventsub.count == 0 {
        return (probe, Some(ObservationSource::IrcProbe));
    }
    if probe.count == 0 {
        return (eventsub, Some(ObservationSource::EventSub));
    }
    let source = Some(ObservationSource::Both);
    if eventsub.count >= probe.count {
        (eventsub, source)
    } else {
        (probe, source)
    }
}

pub struct RaidGreetingMonitor {
    chat: Arc<dyn ChatApi>,
    probe: Option<Arc<dyn RaidTargetChatProbe>>,
    live: Option<Arc<dyn RaidTargetLiveProbe>>,
    courtesy: Option<Arc<dyn CourtesyRecorder>>,
    pending: Arc<Mutex<HashMap<String, PendingGreeting>>>,
    greeting_window: Duration,
}

impl RaidGreetingMonitor {
    pub fn new(chat: Arc<dyn ChatApi>, probe: Option<Arc<dyn RaidTargetChatProbe>>) -> Self {
        Self {
            chat,
            probe,
            live: None,
            courtesy: None,
            pending: Arc::new(Mutex::new(HashMap::new())),
            greeting_window: GREETING_WINDOW,
        }
    }

    /// Endet der Zielstream innerhalb des Fensters (Stream-Ende oder Weiter-Raid),
    /// kann dort niemand mehr begrüßt werden — dann entfällt die Erinnerung.
    pub fn with_live_probe(mut self, live: Arc<dyn RaidTargetLiveProbe>) -> Self {
        self.live = Some(live);
        self
    }

    /// Hält jede Beobachtung fest, damit sie in Score und Matching einfließt
    /// und die Erinnerungen gedrosselt werden können. Ohne Recorder verhält
    /// sich der Monitor wie bisher, nur ohne Gedächtnis.
    pub fn with_courtesy(mut self, courtesy: Arc<dyn CourtesyRecorder>) -> Self {
        self.courtesy = Some(courtesy);
        self
    }

    #[cfg(test)]
    fn with_window(
        chat: Arc<dyn ChatApi>,
        probe: Option<Arc<dyn RaidTargetChatProbe>>,
        greeting_window: Duration,
    ) -> Self {
        Self {
            chat,
            probe,
            live: None,
            courtesy: None,
            pending: Arc::new(Mutex::new(HashMap::new())),
            greeting_window,
        }
    }

    pub fn observe_chat(&self, event: &ChatMessageEvent) {
        let event = event.with_effective_channel();
        let target_id = event.broadcaster_user_id.trim();
        let chatter_id = event.chatter_user_id.trim();
        let chatter_login = event.chatter_user_login.trim().to_lowercase();
        let key = {
            let pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending
                .iter()
                .find(|(_, item)| {
                    item.to_broadcaster_id == target_id
                        && (item.from_broadcaster_id == chatter_id
                            || item.from_broadcaster_login == chatter_login)
                })
                .map(|(key, _)| key.clone())
        };

        let Some(key) = key else {
            return;
        };

        // Bewusst NICHT das Pending auflösen: für die Einstufung zählt, wie
        // viel über das ganze Fenster geschrieben wurde, nicht nur ob
        // überhaupt. Ein kurzes Hallo und eine echte Unterhaltung sind zwei
        // verschiedene Klassen, und das entscheidet sich erst am Fensterende.
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(item) = pending.get_mut(&key) else {
            return;
        };
        item.eventsub_writes.push(Instant::now());
        let count = item.eventsub_writes.len();
        let from = item.from_broadcaster_login.clone();
        let to = item.to_broadcaster_login.clone();
        drop(pending);

        tracing::debug!(
            %from,
            %to,
            nachrichten = count,
            "Raider hat im Zielchat geschrieben"
        );
    }

    /// Twitch meldet über `channel.moderate` das Ziel, das der Raid **wirklich**
    /// erreicht. Weicht es vom geplanten Ziel ab (manueller Raid überschreibt den
    /// Auto-Raid), wird die offene Erinnerung auf den echten Kanal umgezogen.
    /// Ohne offene Erinnerung passiert nichts — rein manuelle Raids bekommen
    /// bewusst keine.
    pub fn raid_retargeted(&self, registration: RaidGreetingRegistration) {
        let from_id = registration.from_broadcaster_id.trim().to_string();
        let from_login = clean_login(&registration.from_broadcaster_login);
        let to_id = registration.to_broadcaster_id.trim().to_string();
        let to_login = clean_login(&registration.to_broadcaster_login);
        if to_id.is_empty() && to_login.is_empty() {
            return;
        }

        let stale: Vec<PendingGreeting> = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let keys: Vec<String> = pending
                .iter()
                .filter(|(_, item)| {
                    let same_source = (!from_id.is_empty() && item.from_broadcaster_id == from_id)
                        || (!from_login.is_empty() && item.from_broadcaster_login == from_login);
                    let same_target = (!to_id.is_empty() && item.to_broadcaster_id == to_id)
                        || (!to_login.is_empty() && item.to_broadcaster_login == to_login);
                    same_source && !same_target
                })
                .map(|(key, _)| key.clone())
                .collect();
            keys.iter().filter_map(|key| pending.remove(key)).collect()
        };

        if stale.is_empty() {
            return;
        }

        for item in &stale {
            if item.probe_watched {
                if let Some(probe) = &self.probe {
                    probe.unwatch(&item.to_broadcaster_login);
                }
            }
            tracing::info!(
                from = %item.from_broadcaster_login,
                geplant = %item.to_broadcaster_login,
                echt = %to_login,
                reason = "raid_target_changed",
                "Raid ging an einen anderen Kanal als geplant, Erinnerung umgezogen"
            );
        }

        // Quellchat-Hinweis bewusst nicht erneut senden — er ist beim Start schon raus.
        if let Some(pending) = self.register(RaidGreetingRegistration {
            from_broadcaster_id: from_id,
            from_broadcaster_login: from_login,
            to_broadcaster_id: to_id,
            to_broadcaster_login: to_login,
        }) {
            self.spawn_deadline(pending.key);
        }
    }

    /// Zieht der Quell-Streamer den Raid per `/unraid` zurück, gab es nie einen
    /// Raid — dann darf auch keine Begrüßungs-Erinnerung mehr kommen. Matcht
    /// per ID oder Login, leere Angaben matchen nie.
    pub fn raid_canceled(&self, from_broadcaster_id: &str, from_broadcaster_login: &str) {
        let from_id = from_broadcaster_id.trim();
        let from_login = clean_login(from_broadcaster_login);
        if from_id.is_empty() && from_login.is_empty() {
            return;
        }

        let removed: Vec<PendingGreeting> = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let keys: Vec<String> = pending
                .iter()
                .filter(|(_, item)| {
                    (!from_id.is_empty() && item.from_broadcaster_id == from_id)
                        || (!from_login.is_empty() && item.from_broadcaster_login == from_login)
                })
                .map(|(key, _)| key.clone())
                .collect();
            keys.iter().filter_map(|key| pending.remove(key)).collect()
        };

        if removed.is_empty() {
            let source = if from_login.is_empty() {
                from_id.to_string()
            } else {
                from_login
            };
            tracing::info!(
                from = %source,
                whisper = false,
                reason = "unraid_ohne_pending_greeting",
                "Unraid ohne offene Raid-Begrüßungs-Erinnerung"
            );
            return;
        }

        for item in removed {
            if item.probe_watched {
                if let Some(probe) = &self.probe {
                    probe.unwatch(&item.to_broadcaster_login);
                }
            }
            tracing::info!(
                from = %item.from_broadcaster_login,
                to = %item.to_broadcaster_login,
                whisper = false,
                reason = "raid_canceled_unraid",
                "Raid zurückgezogen, Begrüßungs-Erinnerung verworfen"
            );
        }
    }

    fn register(&self, registration: RaidGreetingRegistration) -> Option<PendingGreeting> {
        let from_broadcaster_id = registration.from_broadcaster_id.trim().to_string();
        let to_broadcaster_id = registration.to_broadcaster_id.trim().to_string();
        if from_broadcaster_id.is_empty() || to_broadcaster_id.is_empty() {
            return None;
        }
        let from_broadcaster_login = clean_login(&registration.from_broadcaster_login);
        let to_broadcaster_login = clean_login(&registration.to_broadcaster_login);
        let probe_watched = self.probe.is_some();
        let probe_started_at = Instant::now();
        if let Some(probe) = &self.probe {
            probe.watch(&to_broadcaster_login);
        }
        let key = format!("{to_broadcaster_id}:{from_broadcaster_id}");
        let pending = PendingGreeting {
            key: key.clone(),
            from_broadcaster_id,
            from_broadcaster_login,
            to_broadcaster_id,
            to_broadcaster_login,
            probe_watched,
            probe_started_at,
            started_at: Utc::now(),
            eventsub_writes: Vec::new(),
        };
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, pending.clone());
        Some(pending)
    }

    fn send_source_hint(&self, pending: &PendingGreeting) {
        let chat = Arc::clone(&self.chat);
        let from_id = pending.from_broadcaster_id.clone();
        let from_login = pending.from_broadcaster_login.clone();
        let message = source_hint_message(&pending.to_broadcaster_login);
        tokio::spawn(async move {
            match chat.send_message(&from_id, &message).await {
                Ok(SendOutcome::Sent) => tracing::info!(
                    from = %from_login,
                    "Raid-Begrüßungshinweis im Quellchat gesendet"
                ),
                Ok(SendOutcome::Dropped { code, message }) => tracing::debug!(
                    from = %from_login,
                    %code,
                    drop_message = %message,
                    "Raid-Begrüßungshinweis von Twitch verworfen"
                ),
                Ok(SendOutcome::HttpError { status, .. }) => tracing::debug!(
                    from = %from_login,
                    status,
                    "Raid-Begrüßungshinweis: HTTP-Fehler"
                ),
                Err(error) => tracing::debug!(
                    %error,
                    from = %from_login,
                    "Raid-Begrüßungshinweis fehlgeschlagen"
                ),
            }
        });
    }

    fn spawn_deadline(&self, key: String) {
        let pending = Arc::clone(&self.pending);
        let chat = Arc::clone(&self.chat);
        let probe = self.probe.clone();
        let live = self.live.clone();
        let courtesy = self.courtesy.clone();
        let greeting_window = self.greeting_window;
        tokio::spawn(async move {
            tokio::time::sleep(greeting_window).await;
            let item = pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&key);
            let Some(item) = item else {
                return;
            };

            let probe_stats = if item.probe_watched {
                probe.as_ref().and_then(|probe| {
                    probe.write_stats(
                        &item.to_broadcaster_login,
                        &item.from_broadcaster_login,
                        item.probe_started_at,
                    )
                })
            } else {
                None
            };
            if item.probe_watched {
                if let Some(probe) = &probe {
                    probe.unwatch(&item.to_broadcaster_login);
                }
            }

            let (stats, source) = merge_stats(item.eventsub_stats(), probe_stats);

            // Einstufung: erst entscheiden, ob überhaupt messbar, dann in eine
            // der drei Klassen einsortieren. Nicht messbar ist ausdrücklich
            // nicht dasselbe wie geschwiegen.
            let (outcome, unknown_reason) = if stats.count > 0 {
                (
                    CourtesyOutcome::Classified(classify(stats.count, stats.span())),
                    None,
                )
            } else if source.is_none() {
                // Weder EventSub noch IRC hatten eine belastbare Aussage.
                (CourtesyOutcome::Unknown, Some("keine_beobachtung"))
            } else if target_stream_ended(&live, &item.to_broadcaster_login).await {
                // Der Zielstream endete im Fenster oder raidete weiter: der
                // Raider hatte dort niemanden mehr zum Begrüßen.
                (CourtesyOutcome::Unknown, Some("zielstream_beendet"))
            } else {
                (CourtesyOutcome::Classified(CourtesyClass::Silent), None)
            };

            // Erinnerung nur an Leute, die auch sonst nicht schreiben, und
            // dann gedrosselt. Ein Aussetzer eines Schreibers bleibt still.
            let mut whisper_sent = false;
            if let Some(recorder) = &courtesy {
                let history = recorder.summary(&item.from_broadcaster_id).await;
                let days_since = recorder
                    .last_whisper_at(&item.from_broadcaster_id)
                    .await
                    .map(|last| (Utc::now() - last).num_days());
                if should_remind(outcome, history.class, days_since) {
                    whisper_sent = send_reminder(&chat, &item).await;
                } else {
                    tracing::info!(
                        from = %item.from_broadcaster_login,
                        to = %item.to_broadcaster_login,
                        klasse = %outcome.as_str(),
                        historie = history.class.map(CourtesyClass::as_str).unwrap_or("keine"),
                        whisper = false,
                        "Raid-Etikette festgehalten, keine Erinnerung nötig"
                    );
                }
            } else if outcome == CourtesyOutcome::Classified(CourtesyClass::Silent) {
                // Ohne Recorder gibt es keine Historie und keine Drosselung;
                // dann bleibt es beim bisherigen Verhalten.
                whisper_sent = send_reminder(&chat, &item).await;
            }

            tracing::info!(
                from = %item.from_broadcaster_login,
                to = %item.to_broadcaster_login,
                klasse = %outcome.as_str(),
                nachrichten = stats.count,
                spanne_sek = stats.span().as_secs(),
                quelle = source.map(ObservationSource::as_str).unwrap_or("keine"),
                grund = unknown_reason.unwrap_or(""),
                whisper = whisper_sent,
                "Raid-Etikette ausgewertet"
            );

            if let Some(recorder) = &courtesy {
                recorder
                    .record(CourtesyEvent {
                        raid_history_id: None,
                        from_broadcaster_id: item.from_broadcaster_id.clone(),
                        from_broadcaster_login: item.from_broadcaster_login.clone(),
                        to_broadcaster_id: item.to_broadcaster_id.clone(),
                        to_broadcaster_login: item.to_broadcaster_login.clone(),
                        observed_from: item.started_at,
                        outcome,
                        message_count: stats.count as i32,
                        message_span_sec: stats.span().as_secs().min(i32::MAX as u64) as i32,
                        observation_source: source,
                        unknown_reason: unknown_reason.map(str::to_string),
                        whisper_sent,
                    })
                    .await;
            }
        });
    }
}

/// Ob der Zielstream nicht mehr läuft. Unbekannter Status zählt als „läuft
/// noch": ein Ausfall der Live-Abfrage darf keine Beobachtung entwerten.
async fn target_stream_ended(live: &Option<Arc<dyn RaidTargetLiveProbe>>, login: &str) -> bool {
    match live {
        Some(live) => live.is_live(login).await == Some(false),
        None => false,
    }
}

/// Schickt die Erinnerung und meldet, ob sie rausging.
async fn send_reminder(chat: &Arc<dyn ChatApi>, item: &PendingGreeting) -> bool {
    match chat
        .send_whisper(&item.from_broadcaster_id, WHISPER_REMINDER)
        .await
    {
        Ok(true) => {
            tracing::info!(
                from = %item.from_broadcaster_login,
                to = %item.to_broadcaster_login,
                "Raid-Begrüßungs-Erinnerung per Whisper gesendet"
            );
            true
        }
        Ok(false) => {
            tracing::warn!(
                from = %item.from_broadcaster_login,
                to = %item.to_broadcaster_login,
                "Raid-Begrüßungs-Erinnerung per Whisper nicht akzeptiert"
            );
            false
        }
        Err(error) => {
            tracing::warn!(
                %error,
                from = %item.from_broadcaster_login,
                to = %item.to_broadcaster_login,
                "Raid-Begrüßungs-Erinnerung per Whisper fehlgeschlagen"
            );
            false
        }
    }
}

#[async_trait::async_trait]
impl RaidGreetingMonitorPort for RaidGreetingMonitor {
    async fn raid_started(&self, registration: RaidGreetingRegistration) {
        let Some(pending) = self.register(registration) else {
            tracing::info!(
                whisper = false,
                reason = "invalid_raid_registration",
                "Raid-Begrüßungs-Erinnerung nicht geplant"
            );
            return;
        };
        self.send_source_hint(&pending);
        self.spawn_deadline(pending.key);
    }
}

fn clean_login(login: &str) -> String {
    login.trim().trim_start_matches('@').trim().to_lowercase()
}

fn source_hint_message(to_login: &str) -> String {
    let login = clean_login(to_login);
    if login.is_empty() {
        "Die Reise geht weiter. Vergesst nicht kurz hallo und tschüss zu sagen :)".to_string()
    } else {
        // ponytail: kein Satzzeichen direkt hinter dem Mention, Twitch klebt es an den Login
        format!("Die Reise geht an @{login} Vergesst nicht kurz hallo und tschüss zu sagen :)")
    }
}

/// Live-Status aus `twitch_live_state` (vom Scout und den stream.online/offline-
/// Events gepflegt). Unbekannter Kanal oder DB-Fehler → `None`, damit ein
/// Ausfall die Erinnerung nicht stillschweigend abschaltet.
pub struct LiveStateTargetProbe {
    live_state: LiveStateStore,
}

impl LiveStateTargetProbe {
    pub fn new(live_state: LiveStateStore) -> Self {
        Self { live_state }
    }
}

#[async_trait::async_trait]
impl RaidTargetLiveProbe for LiveStateTargetProbe {
    async fn is_live(&self, login: &str) -> Option<bool> {
        let login = clean_login(login);
        if login.is_empty() {
            return None;
        }
        match self
            .live_state
            .source_states_by_logins(std::slice::from_ref(&login))
            .await
        {
            Ok(states) => states
                .get(&login)
                .and_then(|state| state.is_live)
                .map(|flag| flag != 0),
            Err(error) => {
                tracing::warn!(%error, %login, "Live-Status des Raid-Ziels nicht abrufbar");
                None
            }
        }
    }
}

/// Der Whisper geht als DM direkt an den Raider, ein @-Mention wäre doppelt gemoppelt.
const WHISPER_REMINDER: &str = "Hey, denk nach dem Raid dran: Ein kurzes Hallo und Tschüss im Chat gehört zum guten Ton :) Das macht den Raid viel persönlicher, so bleibt man am besten im Kopf und stärkt die Connection!";

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use std::sync::Mutex;
    use tb_chat::api::AnnouncementOutcome;
    use tb_chat::BanOutcome;

    #[derive(Default)]
    struct FakeChatApi {
        messages: Mutex<Vec<(String, String)>>,
        whispers: Mutex<Vec<(String, String)>>,
    }

    struct FakeProbe {
        result: Option<WriteStats>,
        watched: Mutex<Vec<String>>,
        unwatched: Mutex<Vec<String>>,
    }

    impl FakeProbe {
        /// `None` = Beobachtung nicht aussagefähig, `Some(0 Nachrichten)` =
        /// belegtes Schweigen. Der Unterschied entscheidet alles.
        fn new(result: Option<bool>) -> Self {
            let result = result.map(|wrote| {
                if wrote {
                    stats(1, 0)
                } else {
                    WriteStats::default()
                }
            });
            Self {
                result,
                watched: Mutex::new(Vec::new()),
                unwatched: Mutex::new(Vec::new()),
            }
        }

        fn with_stats(result: WriteStats) -> Self {
            Self {
                result: Some(result),
                watched: Mutex::new(Vec::new()),
                unwatched: Mutex::new(Vec::new()),
            }
        }
    }

    /// Baut eine Schreib-Statistik mit `count` Nachrichten über `span_secs`.
    fn stats(count: u32, span_secs: u64) -> WriteStats {
        let first = Instant::now();
        WriteStats {
            count,
            first_at: (count > 0).then_some(first),
            last_at: (count > 0).then(|| first + Duration::from_secs(span_secs)),
        }
    }

    struct FakeLiveProbe {
        live: Option<bool>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeLiveProbe {
        fn new(live: Option<bool>) -> Self {
            Self {
                live,
                asked: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl RaidTargetLiveProbe for FakeLiveProbe {
        async fn is_live(&self, login: &str) -> Option<bool> {
            self.asked.lock().unwrap().push(login.to_string());
            self.live
        }
    }

    impl RaidTargetChatProbe for FakeProbe {
        fn watch(&self, channel: &str) {
            self.watched.lock().unwrap().push(channel.to_string());
        }

        fn unwatch(&self, channel: &str) {
            self.unwatched.lock().unwrap().push(channel.to_string());
        }

        fn write_stats(&self, _channel: &str, _nick: &str, _since: Instant) -> Option<WriteStats> {
            self.result
        }
    }

    #[async_trait::async_trait]
    impl ChatApi for FakeChatApi {
        async fn send_message(
            &self,
            broadcaster_id: &str,
            message: &str,
        ) -> Result<SendOutcome, String> {
            self.messages
                .lock()
                .unwrap()
                .push((broadcaster_id.to_string(), message.to_string()));
            Ok(SendOutcome::Sent)
        }

        async fn send_whisper(&self, to_user_id: &str, message: &str) -> Result<bool, String> {
            self.whispers
                .lock()
                .unwrap()
                .push((to_user_id.to_string(), message.to_string()));
            Ok(true)
        }

        async fn send_announcement(
            &self,
            _broadcaster_id: &str,
            _message: &str,
            _color: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn send_announcement_detailed(
            &self,
            _broadcaster_id: &str,
            _message: &str,
            _color: &str,
        ) -> Result<AnnouncementOutcome, String> {
            Ok(AnnouncementOutcome::accepted())
        }

        async fn ban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _reason: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }

        async fn timeout_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _duration_secs: u32,
            _reason: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }

        async fn unban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn delete_message(
            &self,
            _broadcaster_id: &str,
            _message_id: &str,
        ) -> Result<bool, String> {
            Ok(true)
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

    fn chat_event(text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "to1".into(),
            broadcaster_user_login: "ziel".into(),
            chatter_user_id: "from1".into(),
            chatter_user_login: "raider".into(),
            message_id: "m1".into(),
            message: tb_chat::types::ChatMessageBody {
                text: text.into(),
                fragments: vec![],
            },
            ..Default::default()
        }
    }

    fn registration() -> RaidGreetingRegistration {
        RaidGreetingRegistration {
            from_broadcaster_id: "from1".into(),
            from_broadcaster_login: "Raider".into(),
            to_broadcaster_id: "to1".into(),
            to_broadcaster_login: "Ziel".into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn beliebige_raider_nachricht_erfuellt_pending_ohne_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(20));

        monitor.raid_started(registration()).await;
        monitor.observe_chat(&chat_event("gg wp starker stream"));
        tokio::time::sleep(Duration::from_millis(40)).await;

        assert_eq!(fake.whispers.lock().unwrap().len(), 0);
        let messages = fake.messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, "from1");
        assert!(messages[0].1.contains("@ziel"));
        assert_eq!(probe.unwatched.lock().unwrap().as_slice(), ["ziel"]);
    }

    #[tokio::test(start_paused = true)]
    async fn fremder_chatter_erfuellt_pending_nicht() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor = RaidGreetingMonitor::with_window(chat, Some(probe), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        let mut event = chat_event("gg wp starker stream");
        event.chatter_user_id = "other1".into();
        event.chatter_user_login = "someone_else".into();
        monitor.observe_chat(&event);
        tokio::time::sleep(Duration::from_millis(30)).await;

        let whispers = fake.whispers.lock().unwrap();
        assert_eq!(whispers.len(), 1);
        assert_eq!(whispers[0].0, "from1");
    }

    #[tokio::test(start_paused = true)]
    async fn falscher_zielkanal_erfuellt_pending_nicht() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor = RaidGreetingMonitor::with_window(chat, Some(probe), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        let mut event = chat_event("gg wp starker stream");
        event.broadcaster_user_id = "to2".into();
        monitor.observe_chat(&event);
        tokio::time::sleep(Duration::from_millis(30)).await;

        let whispers = fake.whispers.lock().unwrap();
        assert_eq!(whispers.len(), 1);
        assert_eq!(whispers[0].0, "from1");
    }

    #[tokio::test(start_paused = true)]
    async fn fehlende_begruessung_sendet_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        let whispers = fake.whispers.lock().unwrap();
        assert_eq!(whispers.len(), 1);
        assert_eq!(whispers[0].0, "from1");
        assert!(whispers[0].1.to_lowercase().contains("hallo"));
        assert_eq!(probe.watched.lock().unwrap().as_slice(), ["ziel"]);
        assert_eq!(probe.unwatched.lock().unwrap().as_slice(), ["ziel"]);
    }

    #[tokio::test(start_paused = true)]
    async fn beendeter_zielstream_verhindert_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let live = Arc::new(FakeLiveProbe::new(Some(false)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5))
                .with_live_probe(live.clone());

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(fake.whispers.lock().unwrap().is_empty());
        assert_eq!(live.asked.lock().unwrap().as_slice(), ["ziel"]);
        assert_eq!(probe.unwatched.lock().unwrap().as_slice(), ["ziel"]);
    }

    #[tokio::test(start_paused = true)]
    async fn laufender_zielstream_sendet_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let live = Arc::new(FakeLiveProbe::new(Some(true)));
        let monitor = RaidGreetingMonitor::with_window(chat, Some(probe), Duration::from_millis(5))
            .with_live_probe(live);

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(fake.whispers.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn unbekannter_live_status_sendet_whisper_weiter() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let live = Arc::new(FakeLiveProbe::new(None));
        let monitor = RaidGreetingMonitor::with_window(chat, Some(probe), Duration::from_millis(5))
            .with_live_probe(live);

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(fake.whispers.lock().unwrap().len(), 1);
    }

    fn registration_to(login: &str, id: &str) -> RaidGreetingRegistration {
        RaidGreetingRegistration {
            from_broadcaster_id: "from1".into(),
            from_broadcaster_login: "Raider".into(),
            to_broadcaster_id: id.into(),
            to_broadcaster_login: login.into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retarget_zieht_erinnerung_auf_das_echte_ziel() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        monitor.raid_retargeted(registration_to("Echtes_Ziel", "to2"));
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(
            probe.watched.lock().unwrap().as_slice(),
            ["ziel", "echtes_ziel"]
        );
        // Das alte Ziel wird beim Umzug freigegeben, das neue nach dem Fenster.
        assert_eq!(
            probe.unwatched.lock().unwrap().as_slice(),
            ["ziel", "echtes_ziel"]
        );
        // Der Quellchat-Hinweis darf kein zweites Mal rausgehen.
        assert_eq!(fake.messages.lock().unwrap().len(), 1);
        assert_eq!(fake.whispers.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn retarget_auf_dasselbe_ziel_ist_no_op() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        monitor.raid_retargeted(registration_to("Ziel", "to1"));
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(probe.watched.lock().unwrap().as_slice(), ["ziel"]);
        assert_eq!(fake.whispers.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn retarget_ohne_offene_erinnerung_legt_keine_an() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5));

        monitor.raid_retargeted(registration_to("Echtes_Ziel", "to2"));
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(fake.whispers.lock().unwrap().is_empty());
        assert!(probe.watched.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn unraid_verhindert_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        monitor.raid_canceled("from1", "Raider");
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(fake.whispers.lock().unwrap().is_empty());
        assert_eq!(probe.unwatched.lock().unwrap().as_slice(), ["ziel"]);
    }

    #[tokio::test(start_paused = true)]
    async fn unraid_matcht_auch_nur_per_login() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor = RaidGreetingMonitor::with_window(chat, Some(probe), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        monitor.raid_canceled("", "@Raider");
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(fake.whispers.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn unraid_fremder_quelle_laesst_pending_stehen() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor = RaidGreetingMonitor::with_window(chat, Some(probe), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        monitor.raid_canceled("other1", "fremder");
        tokio::time::sleep(Duration::from_millis(30)).await;

        let whispers = fake.whispers.lock().unwrap();
        assert_eq!(whispers.len(), 1);
        assert_eq!(whispers[0].0, "from1");
    }

    #[tokio::test(start_paused = true)]
    async fn unraid_mit_leeren_argumenten_ist_no_op() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(false)));
        let monitor = RaidGreetingMonitor::with_window(chat, Some(probe), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        monitor.raid_canceled("  ", " @ ");
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(fake.whispers.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn geschriebener_privmsg_verhindert_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(Some(true)));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(fake.whispers.lock().unwrap().is_empty());
        assert_eq!(probe.unwatched.lock().unwrap().as_slice(), ["ziel"]);
    }

    #[tokio::test(start_paused = true)]
    async fn fehlende_probe_verhindert_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let monitor = RaidGreetingMonitor::with_window(chat, None, Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(fake.whispers.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn nicht_aussagefaehige_probe_verhindert_whisper_und_wird_beendet() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let probe = Arc::new(FakeProbe::new(None));
        let monitor =
            RaidGreetingMonitor::with_window(chat, Some(probe.clone()), Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(fake.whispers.lock().unwrap().is_empty());
        assert_eq!(probe.unwatched.lock().unwrap().as_slice(), ["ziel"]);
    }

    #[test]
    fn whisper_reminder_bittet_freundlich_um_hallo_und_tschuess() {
        let lower = WHISPER_REMINDER.to_lowercase();
        assert!(lower.contains("hallo"), "{WHISPER_REMINDER}");
        assert!(lower.contains("tschüss"), "{WHISPER_REMINDER}");
        // DM an den Raider: ein @-Mention wäre doppelt gemoppelt
        assert!(!WHISPER_REMINDER.contains('@'), "{WHISPER_REMINDER}");
    }

    #[test]
    fn source_hint_haengt_kein_satzzeichen_an_den_mention() {
        let message = source_hint_message("@DeusAsta");
        assert!(message.contains("@deusasta"), "{message}");
        let rest = message.split("@deusasta").nth(1).unwrap();
        assert!(
            !rest.starts_with(['.', ',', '!', '?', ':', ';']),
            "Satzzeichen klebt am Mention: {message}"
        );
    }

    // ─── Einstufung in die drei Klassen ──────────────────────────────────────

    /// Recorder-Attrappe: merkt sich die geschriebenen Beobachtungen und
    /// liefert eine vorgegebene Historie zurück.
    struct FakeRecorder {
        history: Option<CourtesyClass>,
        days_since_whisper: Option<i64>,
        recorded: Mutex<Vec<CourtesyEvent>>,
    }

    impl FakeRecorder {
        fn new(history: Option<CourtesyClass>, days_since_whisper: Option<i64>) -> Self {
            Self {
                history,
                days_since_whisper,
                recorded: Mutex::new(Vec::new()),
            }
        }

        fn classes(&self) -> Vec<String> {
            self.recorded
                .lock()
                .unwrap()
                .iter()
                .map(|event| event.outcome.as_str().to_string())
                .collect()
        }
    }

    #[async_trait::async_trait]
    impl CourtesyRecorder for FakeRecorder {
        async fn summary(&self, _from: &str) -> CourtesySummary {
            CourtesySummary {
                class: self.history,
                ..CourtesySummary::default()
            }
        }

        async fn last_whisper_at(&self, _from: &str) -> Option<DateTime<Utc>> {
            self.days_since_whisper
                .map(|days| Utc::now() - chrono::Duration::days(days))
        }

        async fn record(&self, event: CourtesyEvent) {
            self.recorded.lock().unwrap().push(event);
        }
    }

    /// Baut einen Monitor mit Recorder und kurzem Fenster.
    fn monitor_with(
        chat: Arc<dyn ChatApi>,
        probe: FakeProbe,
        recorder: Arc<FakeRecorder>,
    ) -> RaidGreetingMonitor {
        let recorder: Arc<dyn CourtesyRecorder> = recorder;
        RaidGreetingMonitor::with_window(chat, Some(Arc::new(probe)), Duration::from_millis(5))
            .with_courtesy(recorder)
    }

    #[tokio::test(start_paused = true)]
    async fn keine_nachricht_wird_als_silent_festgehalten() {
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(Some(CourtesyClass::Silent), None));
        let monitor = monitor_with(
            fake.clone(),
            FakeProbe::with_stats(WriteStats::default()),
            recorder.clone(),
        );

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(recorder.classes(), vec!["silent"]);
        let recorded = recorder.recorded.lock().unwrap();
        assert_eq!(recorded[0].message_count, 0);
        assert!(recorded[0].whisper_sent, "Dauerschweiger wird erinnert");
    }

    #[tokio::test(start_paused = true)]
    async fn ein_kurzes_hallo_wird_als_greeter_festgehalten() {
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(None, None));
        let monitor = monitor_with(
            fake.clone(),
            FakeProbe::with_stats(WriteStats::default()),
            recorder.clone(),
        );

        monitor.raid_started(registration()).await;
        monitor.observe_chat(&chat_event("hi zusammen"));
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(recorder.classes(), vec!["greeter"]);
        // Wer schreibt, bekommt nie eine Erinnerung.
        assert!(fake.whispers.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn mehrere_nachrichten_werden_als_engaged_festgehalten() {
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(None, None));
        let monitor = monitor_with(
            fake.clone(),
            FakeProbe::with_stats(WriteStats::default()),
            recorder.clone(),
        );

        monitor.raid_started(registration()).await;
        // Das Pending darf nach der ersten Nachricht nicht verschwinden,
        // sonst käme die dritte nie an und es bliebe bei "greeter".
        monitor.observe_chat(&chat_event("hi"));
        monitor.observe_chat(&chat_event("wie laeuft der stream"));
        monitor.observe_chat(&chat_event("ciao, bis bald"));
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(recorder.classes(), vec!["engaged"]);
        let recorded = recorder.recorded.lock().unwrap();
        assert_eq!(recorded[0].message_count, 3);
        assert!(fake.whispers.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn nicht_aussagefaehige_probe_wird_unknown_nicht_silent() {
        // Der wichtigste Fall: ein Ausfall der Beobachtung darf niemanden
        // als Schweiger abstempeln.
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(None, None));
        let monitor = monitor_with(fake.clone(), FakeProbe::new(None), recorder.clone());

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(recorder.classes(), vec!["unknown"]);
        assert!(
            fake.whispers.lock().unwrap().is_empty(),
            "ohne Messung kein Vorwurf"
        );
        let recorded = recorder.recorded.lock().unwrap();
        assert_eq!(
            recorded[0].unknown_reason.as_deref(),
            Some("keine_beobachtung")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn beendeter_zielstream_ohne_nachricht_wird_unknown() {
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(Some(CourtesyClass::Silent), None));
        let live = Arc::new(FakeLiveProbe::new(Some(false)));
        let recorder_dyn: Arc<dyn CourtesyRecorder> = recorder.clone();
        let monitor = RaidGreetingMonitor::with_window(
            fake.clone(),
            Some(Arc::new(FakeProbe::with_stats(WriteStats::default()))),
            Duration::from_millis(5),
        )
        .with_live_probe(live)
        .with_courtesy(recorder_dyn);

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        // Dort war niemand mehr zum Begrüßen — das ist kein Schweigen.
        assert_eq!(recorder.classes(), vec!["unknown"]);
        assert!(fake.whispers.lock().unwrap().is_empty());
    }

    // ─── Drosselung der Erinnerungen ─────────────────────────────────────────

    #[tokio::test(start_paused = true)]
    async fn aussetzer_eines_gruessers_bleibt_ohne_erinnerung() {
        // Der Nutzerfall: wer sonst schreibt und einmal vergisst, wird in Ruhe
        // gelassen. Die Beobachtung wird trotzdem festgehalten.
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(Some(CourtesyClass::Greeter), None));
        let monitor = monitor_with(
            fake.clone(),
            FakeProbe::with_stats(WriteStats::default()),
            recorder.clone(),
        );

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(recorder.classes(), vec!["silent"]);
        assert!(fake.whispers.lock().unwrap().is_empty());
        assert!(!recorder.recorded.lock().unwrap()[0].whisper_sent);
    }

    #[tokio::test(start_paused = true)]
    async fn schweiger_wird_innerhalb_des_cooldowns_nicht_erneut_angeschrieben() {
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(Some(CourtesyClass::Silent), Some(1)));
        let monitor = monitor_with(
            fake.clone(),
            FakeProbe::with_stats(WriteStats::default()),
            recorder.clone(),
        );

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(recorder.classes(), vec!["silent"]);
        assert!(
            fake.whispers.lock().unwrap().is_empty(),
            "gestern erst angeschrieben, kein Dauerfeuer"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn schweiger_wird_nach_abgelaufenem_cooldown_wieder_erinnert() {
        let fake = Arc::new(FakeChatApi::default());
        let recorder = Arc::new(FakeRecorder::new(Some(CourtesyClass::Silent), Some(30)));
        let monitor = monitor_with(
            fake.clone(),
            FakeProbe::with_stats(WriteStats::default()),
            recorder.clone(),
        );

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert_eq!(fake.whispers.lock().unwrap().len(), 1);
    }
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tb_chat::types::{ChatMessageEvent, SendOutcome};
use tb_chat::ChatApi;
use tb_monitoring::LiveStateStore;
use tb_raid::{RaidGreetingMonitorPort, RaidGreetingRegistration};

// ponytail: 20 Min Kulanz; Pending ist prozess-lokal, Neustart verwirft es konservativ: lieber kein Whisper als ein falscher Vorwurf.
const GREETING_WINDOW: Duration = Duration::from_secs(20 * 60);

pub trait RaidTargetChatProbe: Send + Sync {
    fn watch(&self, channel: &str);
    fn unwatch(&self, channel: &str);
    fn has_written(&self, channel: &str, nick: &str, since: Instant) -> Option<bool>;
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
}

pub struct RaidGreetingMonitor {
    chat: Arc<dyn ChatApi>,
    probe: Option<Arc<dyn RaidTargetChatProbe>>,
    live: Option<Arc<dyn RaidTargetLiveProbe>>,
    pending: Arc<Mutex<HashMap<String, PendingGreeting>>>,
    greeting_window: Duration,
}

impl RaidGreetingMonitor {
    pub fn new(chat: Arc<dyn ChatApi>, probe: Option<Arc<dyn RaidTargetChatProbe>>) -> Self {
        Self {
            chat,
            probe,
            live: None,
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

        if let Some(key) = key {
            let removed = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&key);
            if let Some(item) = removed {
                if item.probe_watched {
                    if let Some(probe) = &self.probe {
                        probe.unwatch(&item.to_broadcaster_login);
                    }
                }
                tracing::info!(
                    from = %item.from_broadcaster_login,
                    to = %item.to_broadcaster_login,
                    whisper = false,
                    reason = "eventsub_chat_observed",
                    "Raider hat im Zielchat geschrieben"
                );
            }
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

            let probe_result = if item.probe_watched {
                probe.as_ref().and_then(|probe| {
                    probe.has_written(
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

            match probe_result {
                Some(true) => {
                    tracing::info!(
                        from = %item.from_broadcaster_login,
                        to = %item.to_broadcaster_login,
                        whisper = false,
                        reason = "irc_privmsg_observed",
                        "Raid-Begrüßungs-Erinnerung nicht gesendet"
                    );
                    return;
                }
                Some(false) => tracing::info!(
                    from = %item.from_broadcaster_login,
                    to = %item.to_broadcaster_login,
                    whisper = true,
                    reason = "irc_probe_verified_silent",
                    "Raid-Begrüßungs-Erinnerung wird gesendet"
                ),
                None => {
                    tracing::info!(
                        from = %item.from_broadcaster_login,
                        to = %item.to_broadcaster_login,
                        whisper = false,
                        reason = "irc_probe_unavailable",
                        "Raid-Begrüßungs-Erinnerung nicht gesendet"
                    );
                    return;
                }
            }

            // Der Zielstream kann im Fenster enden oder weiterraiden — dann ist der
            // Raider längst woanders und die Erinnerung wäre ein falscher Vorwurf.
            if let Some(live) = &live {
                let target_live = live.is_live(&item.to_broadcaster_login).await;
                if target_live == Some(false) {
                    tracing::info!(
                        from = %item.from_broadcaster_login,
                        to = %item.to_broadcaster_login,
                        whisper = false,
                        reason = "target_stream_ended",
                        "Raid-Begrüßungs-Erinnerung nicht gesendet"
                    );
                    return;
                }
                if target_live.is_none() {
                    tracing::info!(
                        from = %item.from_broadcaster_login,
                        to = %item.to_broadcaster_login,
                        whisper = true,
                        reason = "target_live_state_unbekannt",
                        "Live-Status des Raid-Ziels unbekannt, Erinnerung geht raus"
                    );
                }
            }

            match chat
                .send_whisper(&item.from_broadcaster_id, WHISPER_REMINDER)
                .await
            {
                Ok(true) => tracing::info!(
                    from = %item.from_broadcaster_login,
                    to = %item.to_broadcaster_login,
                    "Raid-Begrüßungs-Erinnerung per Whisper gesendet"
                ),
                Ok(false) => tracing::warn!(
                    from = %item.from_broadcaster_login,
                    to = %item.to_broadcaster_login,
                    "Raid-Begrüßungs-Erinnerung per Whisper nicht akzeptiert"
                ),
                Err(error) => tracing::warn!(
                    %error,
                    from = %item.from_broadcaster_login,
                    to = %item.to_broadcaster_login,
                    "Raid-Begrüßungs-Erinnerung per Whisper fehlgeschlagen"
                ),
            }
        });
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
        result: Option<bool>,
        watched: Mutex<Vec<String>>,
        unwatched: Mutex<Vec<String>>,
    }

    impl FakeProbe {
        fn new(result: Option<bool>) -> Self {
            Self {
                result,
                watched: Mutex::new(Vec::new()),
                unwatched: Mutex::new(Vec::new()),
            }
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

        fn has_written(&self, _channel: &str, _nick: &str, _since: Instant) -> Option<bool> {
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
}

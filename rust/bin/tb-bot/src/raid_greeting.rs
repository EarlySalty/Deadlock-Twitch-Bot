use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tb_chat::types::{ChatMessageEvent, SendOutcome};
use tb_chat::ChatApi;
use tb_raid::{RaidGreetingMonitorPort, RaidGreetingRegistration};

const GREETING_WINDOW: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
struct PendingGreeting {
    key: String,
    from_broadcaster_id: String,
    from_broadcaster_login: String,
    to_broadcaster_id: String,
    to_broadcaster_login: String,
}

pub struct RaidGreetingMonitor {
    chat: Arc<dyn ChatApi>,
    // ponytail: process-local; persistieren, wenn Begrüßungs-Compliance Restarts überleben soll.
    pending: Arc<Mutex<HashMap<String, PendingGreeting>>>,
    greeting_window: Duration,
}

impl RaidGreetingMonitor {
    pub fn new(chat: Arc<dyn ChatApi>) -> Self {
        Self {
            chat,
            pending: Arc::new(Mutex::new(HashMap::new())),
            greeting_window: GREETING_WINDOW,
        }
    }

    #[cfg(test)]
    fn with_window(chat: Arc<dyn ChatApi>, greeting_window: Duration) -> Self {
        Self {
            chat,
            pending: Arc::new(Mutex::new(HashMap::new())),
            greeting_window,
        }
    }

    pub fn observe_chat(&self, event: &ChatMessageEvent) {
        if !contains_greeting(event.text()) {
            return;
        }

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
                tracing::info!(
                    from = %item.from_broadcaster_login,
                    to = %item.to_broadcaster_login,
                    "Raid-Begrüßung im Zielchat erkannt"
                );
            }
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
        let key = format!("{to_broadcaster_id}:{from_broadcaster_id}");
        let pending = PendingGreeting {
            key: key.clone(),
            from_broadcaster_id,
            from_broadcaster_login,
            to_broadcaster_id,
            to_broadcaster_login,
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

            let message = whisper_reminder_message(&item.from_broadcaster_login);
            match chat.send_whisper(&item.from_broadcaster_id, &message).await {
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
        format!("Die Reise geht an @{login}. Vergesst nicht kurz hallo und tschüss zu sagen :)")
    }
}

fn whisper_reminder_message(from_login: &str) -> String {
    let login = clean_login(from_login);
    if login.is_empty() {
        "Hey, kleiner Reminder: Sag nach einem Raid im Zielchat bitte kurz Hallo. Das wirkt persönlicher und hilft dem Netzwerk.".to_string()
    } else {
        format!("Hey @{login}, kleiner Reminder: Sag nach einem Raid im Zielchat bitte kurz Hallo. Das wirkt persönlicher und hilft dem Netzwerk.")
    }
}

fn contains_greeting(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if lower.contains("guten morgen")
        || lower.contains("guten tag")
        || lower.contains("guten abend")
    {
        return true;
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .any(|word| {
            matches!(
                word,
                "hallo"
                    | "halli"
                    | "hi"
                    | "hey"
                    | "hello"
                    | "moin"
                    | "servus"
                    | "gude"
                    | "huhu"
                    | "tach"
            ) || word.starts_with("grü")
                || word.starts_with("grue")
                || word.starts_with("gruss")
        })
}

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

    #[test]
    fn erkennt_typische_begruessungen() {
        assert!(contains_greeting("Hallo zusammen"));
        assert!(contains_greeting("moin moin"));
        assert!(contains_greeting("guten Abend euch"));
        assert!(contains_greeting("grüße in die Runde"));
        assert!(!contains_greeting("gg danke fuer den raid"));
    }

    #[tokio::test]
    async fn raider_begruessung_erfuellt_pending_ohne_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let monitor = RaidGreetingMonitor::with_window(chat, Duration::from_millis(20));

        monitor.raid_started(registration()).await;
        monitor.observe_chat(&chat_event("Hallo Zielchat"));
        tokio::time::sleep(Duration::from_millis(40)).await;

        assert_eq!(fake.whispers.lock().unwrap().len(), 0);
        let messages = fake.messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, "from1");
        assert!(messages[0].1.contains("@ziel"));
    }

    #[tokio::test]
    async fn fehlende_begruessung_sendet_whisper() {
        let fake = Arc::new(FakeChatApi::default());
        let chat: Arc<dyn ChatApi> = fake.clone();
        let monitor = RaidGreetingMonitor::with_window(chat, Duration::from_millis(5));

        monitor.raid_started(registration()).await;
        tokio::time::sleep(Duration::from_millis(30)).await;

        let whispers = fake.whispers.lock().unwrap();
        assert_eq!(whispers.len(), 1);
        assert_eq!(whispers[0].0, "from1");
        assert!(whispers[0].1.contains("Hallo"));
    }
}

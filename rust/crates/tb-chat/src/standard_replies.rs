//! Zwei feste Chat-Antworten ohne KI: Begrüßung erwidern und die
//! Release-Datum-Frage beantworten.
//!
//! Beide sind bewusst dumm und deterministisch — reine Regex-Treffer, feste
//! Texte, Cooldowns. Der teure Weg (Judge, Invite-Rückfrage) liegt in
//! [`crate::invite_question`] und bleibt davon unberührt.
//!
//! # Vertrag
//!
//! - Begrüßung: nur wenn die Nachricht praktisch *nur* ein Gruß ist, sonst
//!   antwortet der Bot mitten in Gesprächen. Kanalweit, kein Deadlock-Gate.
//! - Release-Frage: nur wenn Deadlock live läuft — der Verweis auf `!invite`
//!   ist sonst wertlos, weil der Befehl selbst live-gegated ist.
//! - Cooldowns: pro Kanal, damit ein Gruß-Sturm nicht zum Bot-Monolog wird.
//! - Commands (`!…`) und Nachrichten mit URL lösen nie aus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use tracing::warn;

use crate::api::ChatApi;
use crate::types::ChatMessageEvent;

/// Cooldown für Grüße pro Kanal.
const GREETING_COOLDOWN: Duration = Duration::from_secs(600);
/// Cooldown für Release-Antworten pro Kanal.
const RELEASE_COOLDOWN: Duration = Duration::from_secs(300);

const GREETING_REPLY: &str = "@{chatter} Hey, willkommen im Chat!";
const RELEASE_REPLY: &str = "@{chatter} Ein offizielles Release-Datum gibt es noch nicht. Wenn du reinschauen willst: tipp !invite, dann bekommst du den Weg zu einer Einladung.";

/// Grußformeln — die Nachricht darf praktisch nur daraus bestehen.
fn greeting_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\W*(hi+|hallo+|hey+|hei|moin|servus|griaß di|grüß dich|gruess dich|yo|hello+|huhu|guten (?:morgen|tag|abend)|na)(?:\s+(?:zusammen|leute|@?\w+))?\W*$",
        )
        .expect("valid greeting regex")
    })
}

/// Frage nach dem Erscheinungsdatum: Zeitwort plus Release-Wort.
fn release_question_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(wann|when|welches datum)\b").expect("valid release question regex")
    })
}

fn release_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(release\w*|erschein\w*|rauskomm\w*|raus|ver(?:ö|oe)ffentlich\w*|launch\w*|full\s*game)\b")
            .expect("valid release word regex")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardReply {
    Greeting,
    ReleaseDate,
}

/// Reine Entscheidung ohne Zustand — Cooldowns prüft der Responder.
pub fn classify_standard_reply(content: &str, is_deadlock_live: bool) -> Option<StandardReply> {
    let raw = content.trim();
    if raw.is_empty() || raw.starts_with('!') || raw.to_lowercase().contains("http") {
        return None;
    }

    if greeting_re().is_match(raw) {
        return Some(StandardReply::Greeting);
    }
    // ponytail: Release-Antwort verweist auf !invite, das nur live existiert.
    if is_deadlock_live && release_question_re().is_match(raw) && release_word_re().is_match(raw) {
        return Some(StandardReply::ReleaseDate);
    }
    None
}

pub struct StandardReplies {
    api: Arc<dyn ChatApi>,
    cooldowns: Mutex<HashMap<(String, StandardReply), Instant>>,
}

impl StandardReplies {
    pub fn new(api: Arc<dyn ChatApi>) -> Self {
        Self {
            api,
            cooldowns: Mutex::new(HashMap::new()),
        }
    }

    /// Antwortet, wenn ein Muster trifft und der Kanal-Cooldown frei ist.
    /// Gibt zurück, ob gesendet wurde.
    pub async fn maybe_respond(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        is_deadlock_live: bool,
    ) -> bool {
        let Some(kind) = classify_standard_reply(event.text(), is_deadlock_live) else {
            return false;
        };
        if !self.cooldown_ok(channel_login, kind) {
            return false;
        }

        let template = match kind {
            StandardReply::Greeting => GREETING_REPLY,
            StandardReply::ReleaseDate => RELEASE_REPLY,
        };
        let message = template.replace("{chatter}", &event.chatter_user_login);

        match self
            .api
            .send_message(&event.broadcaster_user_id, &message)
            .await
        {
            Ok(_) => {
                self.mark_sent(channel_login, kind);
                true
            }
            Err(error) => {
                warn!(channel = channel_login, %error, "Standard-Antwort nicht gesendet");
                false
            }
        }
    }

    fn cooldown_ok(&self, channel_login: &str, kind: StandardReply) -> bool {
        let window = match kind {
            StandardReply::Greeting => GREETING_COOLDOWN,
            StandardReply::ReleaseDate => RELEASE_COOLDOWN,
        };
        let Ok(cooldowns) = self.cooldowns.lock() else {
            return false;
        };
        cooldowns
            .get(&(channel_login.to_string(), kind))
            .is_none_or(|last| last.elapsed() >= window)
    }

    fn mark_sent(&self, channel_login: &str, kind: StandardReply) {
        if let Ok(mut cooldowns) = self.cooldowns.lock() {
            cooldowns.insert((channel_login.to_string(), kind), Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::BanOutcome;
    use crate::types::{ChatMessageBody, SendOutcome};
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};

    struct MockApi {
        sent: Mutex<Vec<String>>,
    }

    impl MockApi {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(vec![]),
            })
        }
        fn messages(&self) -> Vec<String> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, _: &str, message: &str) -> Result<SendOutcome, String> {
            self.sent.lock().unwrap().push(message.to_string());
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }
        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
            Ok(None)
        }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn bot_user_id(&self) -> String {
            "bot123".to_string()
        }
    }

    fn event(text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "ch1".to_string(),
            broadcaster_user_login: "streamer1".to_string(),
            chatter_user_id: "u1".to_string(),
            chatter_user_login: "neuling".to_string(),
            chatter_user_name: "Neuling".to_string(),
            message_id: "m1".to_string(),
            message: ChatMessageBody {
                text: text.to_string(),
                fragments: vec![],
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn gruss_wird_beantwortet_und_dann_vom_cooldown_gebremst() {
        let api = MockApi::new();
        let replies = StandardReplies::new(api.clone());

        assert!(replies.maybe_respond(&event("hallo"), "ch1", false).await);
        assert!(!replies.maybe_respond(&event("moin"), "ch1", false).await);

        let messages = api.messages();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].starts_with("@neuling"));
    }

    #[tokio::test]
    async fn release_antwort_nennt_den_invite_befehl() {
        let api = MockApi::new();
        let replies = StandardReplies::new(api.clone());

        assert!(
            replies
                .maybe_respond(&event("wann kommt das spiel raus"), "ch1", true)
                .await
        );
        assert!(api.messages()[0].contains("!invite"));
    }

    #[tokio::test]
    async fn ohne_treffer_wird_nichts_gesendet() {
        let api = MockApi::new();
        let replies = StandardReplies::new(api.clone());

        assert!(
            !replies
                .maybe_respond(&event("der build ist stark"), "ch1", true)
                .await
        );
        assert!(api.messages().is_empty());
    }

    #[test]
    fn gruesse_werden_erkannt() {
        for raw in ["hallo", "Hi", "moin zusammen", "hey :)", "Guten Abend"] {
            assert_eq!(
                classify_standard_reply(raw, false),
                Some(StandardReply::Greeting),
                "kein Gruß: {raw}"
            );
        }
    }

    #[test]
    fn gruss_mitten_im_satz_loest_nicht_aus() {
        for raw in [
            "hallo wie kommt man an einen invite",
            "hey der Build ist echt stark",
            "na das war ja ein desaster",
        ] {
            assert_eq!(classify_standard_reply(raw, false), None, "falsch: {raw}");
        }
    }

    #[test]
    fn release_frage_nur_wenn_deadlock_live() {
        let raw = "wann kommt das spiel raus";
        assert_eq!(
            classify_standard_reply(raw, true),
            Some(StandardReply::ReleaseDate)
        );
        assert_eq!(classify_standard_reply(raw, false), None);
    }

    #[test]
    fn release_varianten_und_abgrenzung() {
        for raw in [
            "wann erscheint deadlock denn",
            "when is the release",
            "wann kommt das full game raus?",
        ] {
            assert_eq!(
                classify_standard_reply(raw, true),
                Some(StandardReply::ReleaseDate),
                "kein Treffer: {raw}"
            );
        }
        // Kein Zeitwort, kein Release-Wort, Command, URL.
        assert_eq!(classify_standard_reply("das release war stark", true), None);
        assert_eq!(
            classify_standard_reply("wann bist du wieder da", true),
            None
        );
        assert_eq!(classify_standard_reply("!invite", true), None);
        assert_eq!(
            classify_standard_reply("hallo https://example.com", true),
            None
        );
    }
}

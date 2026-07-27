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
use crate::types::{ChatMessageEvent, SendOutcome};

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

/// Eindeutige Release-Wörter: tragen die Frage allein.
fn release_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(release\w*|erschein\w*|rauskomm\w*|ver(?:ö|oe)ffentlich\w*|launch\w*|full\s*game)\b")
            .expect("valid release word regex")
    })
}

/// "raus" allein sagt nichts — "wann kommt der Patch raus" ist keine Frage
/// nach dem Erscheinen des Spiels. Nur mit ausdrücklichem Spielbezug.
fn release_weak_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\braus\b").expect("valid weak release regex"))
}

fn game_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(game|spiel|deadlock)\b").expect("valid game word regex"))
}

/// Themen, die ihr eigenes Erscheinungsdatum haben — nie die Spiel-Antwort.
fn other_subject_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(patch\w*|update\w*|hotfix\w*|season\w*|saison\w*|skin\w*|held|hero\w*|char(?:akter)?\w*|event\w*|turnier\w*|video|stream)\b")
            .expect("valid other subject regex")
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
    if is_deadlock_live && release_question_re().is_match(raw) && !other_subject_re().is_match(raw)
    {
        let eindeutig = release_word_re().is_match(raw);
        let mit_spielbezug = release_weak_word_re().is_match(raw) && game_word_re().is_match(raw);
        if eindeutig || mit_spielbezug {
            return Some(StandardReply::ReleaseDate);
        }
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
        // Prüfen und Belegen in einem Zug: zwischen Prüfung und Senden liegt
        // ein await, in dem sonst ein zweites Chat-Event durchrutschen und
        // dieselbe Antwort ein zweites Mal schicken könnte.
        let Some(previous) = self.reserve(channel_login, kind) else {
            return false;
        };

        let template = match kind {
            StandardReply::Greeting => GREETING_REPLY,
            StandardReply::ReleaseDate => RELEASE_REPLY,
        };
        let message = template.replace("{chatter}", &event.chatter_user_login);

        // Nur eine wirklich zugestellte Nachricht zaehlt: sonst wuerde ein
        // stiller Twitch-Drop (Timeout, Kanaleinstellung) den Cooldown setzen
        // und die nachfolgenden Detektoren ueberspringen.
        match self
            .api
            .send_message(&event.broadcaster_user_id, &message)
            .await
        {
            Ok(SendOutcome::Sent) => true,
            Ok(outcome) => {
                self.release(channel_login, kind, previous);
                warn!(
                    channel = channel_login,
                    ?outcome,
                    "Standard-Antwort von Twitch verworfen"
                );
                false
            }
            Err(error) => {
                self.release(channel_login, kind, previous);
                warn!(channel = channel_login, %error, "Standard-Antwort nicht gesendet");
                false
            }
        }
    }

    /// Belegt den Cooldown, wenn er frei ist. Rückgabe ist der vorherige
    /// Eintrag für ein sauberes Zurückrollen; `None` heißt: gesperrt.
    fn reserve(&self, channel_login: &str, kind: StandardReply) -> Option<Option<Instant>> {
        let window = match kind {
            StandardReply::Greeting => GREETING_COOLDOWN,
            StandardReply::ReleaseDate => RELEASE_COOLDOWN,
        };
        let mut cooldowns = self.cooldowns.lock().ok()?;
        let key = (channel_login.to_string(), kind);
        let previous = cooldowns.get(&key).copied();
        if previous.is_some_and(|last| last.elapsed() < window) {
            return None;
        }
        cooldowns.insert(key, Instant::now());
        Some(previous)
    }

    /// Gibt eine Reservierung zurück, wenn nichts im Chat gelandet ist.
    fn release(&self, channel_login: &str, kind: StandardReply, previous: Option<Instant>) {
        if let Ok(mut cooldowns) = self.cooldowns.lock() {
            let key = (channel_login.to_string(), kind);
            match previous {
                Some(last) => cooldowns.insert(key, last),
                None => cooldowns.remove(&key),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::BanOutcome;
    use crate::types::ChatMessageBody;
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
    async fn verworfene_nachricht_setzt_keinen_cooldown() {
        struct DroppingApi;

        #[async_trait]
        impl ChatApi for DroppingApi {
            async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
                Ok(SendOutcome::Dropped {
                    code: "sender_timedout".to_string(),
                    message: "getimeoutet".to_string(),
                })
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

        let replies = StandardReplies::new(Arc::new(DroppingApi));
        assert!(!replies.maybe_respond(&event("hallo"), "ch1", false).await);
        // Kein Cooldown gesetzt: der naechste Versuch darf es erneut probieren.
        assert!(!replies.maybe_respond(&event("moin"), "ch1", false).await);
    }

    #[tokio::test]
    async fn zwei_gleichzeitige_gruesse_ergeben_eine_antwort() {
        struct SlowApi {
            sent: Mutex<Vec<String>>,
        }

        #[async_trait]
        impl ChatApi for SlowApi {
            async fn send_message(&self, _: &str, message: &str) -> Result<SendOutcome, String> {
                // Fenster, in dem ein zweites Event die Pruefung passieren
                // wuerde, waere der Cooldown erst danach gesetzt.
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
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

        let api = Arc::new(SlowApi {
            sent: Mutex::new(vec![]),
        });
        let replies = StandardReplies::new(api.clone());

        let erstes = event("hallo");
        let zweites = event("moin");
        let (a, b) = tokio::join!(
            replies.maybe_respond(&erstes, "ch1", false),
            replies.maybe_respond(&zweites, "ch1", false)
        );

        assert!(a ^ b, "genau einer der beiden darf senden");
        assert_eq!(api.sent.lock().unwrap().len(), 1);
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
        // Andere Themen mit eigenem Datum bleiben unberuehrt.
        for raw in [
            "wann kommt der patch raus",
            "wann kommt das update fuer das game raus",
            "wann kommt die neue season raus",
            "wann kommt der neue held raus?",
        ] {
            assert_eq!(classify_standard_reply(raw, true), None, "falsch: {raw}");
        }
        // "raus" allein ohne Spielbezug reicht nicht.
        assert_eq!(classify_standard_reply("wann gehst du raus", true), None);

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

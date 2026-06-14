//! Freche Kurz-Antworten auf Danke-Nachrichten im Chat.
//!
//! Port von `_maybe_fun_responses` (bot/chat/bot.py Z. 850–875).
//!
//! # Vertrag
//!
//! - Wird NUR aufgerufen wenn `is_deadlock_live == true` (bot.py Z. 1746 — das Gate
//!   liegt in der Pipeline, nicht hier).
//! - Reagiert auf "danke / thanks / thx / merci / ty" (bot.py Z. 865).
//! - URLs ("http") in der Nachricht deaktivieren den Trigger (bot.py Z. 866).
//! - Cooldown: 90 Sekunden pro Kanal (bot.py Z. 867 `_cooldown_ok(... 90.0)`).
//! - Antworten: zufällig aus zwei festen Strings (bot.py Z. 869–872).
//! - Kein Response wenn `raw.starts_with(prefix)` (Command-Prefix; bot.py Z. 856).
//!   Rust-Seite: Präfix ist immer `!` — wir lehnen Nachrichten ab die mit `!` beginnen.
//! - `_fun_thanks_reply_enabled` Flag: bot.py Z. 190 = false (default), wird zur
//!   Laufzeit gesetzt. In Rust: Flag im Konstruktor übergeben; Default = false.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::api::ChatApi;
use crate::types::ChatMessageEvent;

// ---------------------------------------------------------------------------
// Konstanten — exakt aus bot/chat/bot.py Z. 869–872
// ---------------------------------------------------------------------------

/// Antwort-Texte für den Danke-Trigger (bot.py Z. 869–872).
/// Zufällig gewählt via Modulo-Index.
const THANKS_REPLIES: &[&str] = &[
    "Danke, ich wusste ja, dass ich gut bin. WiltedRose",
    "Oh stop it, you :relaxed:",
];

/// Trigger-Wörter für den Danke-Check (bot.py Z. 865).
const THANKS_TRIGGERS: &[&str] = &["danke", "thanks", "thx", "merci", "ty"];

/// Cooldown pro Kanal in Sekunden (bot.py Z. 867 `_cooldown_ok(... 90.0)`).
const FUN_REPLY_COOLDOWN_SECS: u64 = 90;

// ---------------------------------------------------------------------------
// FunResponses
// ---------------------------------------------------------------------------

/// Zustandshalter für Danke-Antworten.
///
/// # Verwendung
///
/// ```rust,ignore
/// let fun = FunResponses::new(api.clone(), true);
/// fun.maybe_respond(&event, "streamer_login").await;
/// ```
pub struct FunResponses {
    api: Arc<dyn ChatApi>,
    /// Aktivierungs-Flag — `false` unterdrückt alle Antworten.
    /// Entspricht `_fun_thanks_reply_enabled` (bot.py Z. 190).
    enabled: bool,
    /// Cooldown-Tracker: channel_login → letzter Aufruf-Zeitpunkt.
    /// Entspricht `_fun_reply_cd` (bot.py Z. 188).
    cooldowns: Mutex<HashMap<String, Instant>>,
    /// Seed für Pseudo-Zufallsauswahl (monoton hochgezählt, kein rand-Crate nötig).
    counter: Mutex<usize>,
}

impl FunResponses {
    /// Erstellt eine neue Instanz.
    ///
    /// # Parameter
    /// - `api` — Ausgehende Chat-API (für `send_message`).
    /// - `enabled` — Entspricht `_fun_thanks_reply_enabled` (bot.py Z. 190); default=false.
    pub fn new(api: Arc<dyn ChatApi>, enabled: bool) -> Self {
        Self {
            api,
            enabled,
            cooldowns: Mutex::new(HashMap::new()),
            counter: Mutex::new(0),
        }
    }

    /// Prüft ob eine Danke-Antwort ausgelöst werden soll und sendet sie ggf.
    ///
    /// Port von `_maybe_fun_responses` (bot/chat/bot.py Z. 850–875).
    ///
    /// Stille Rückkehr in allen Nicht-Treffer-Fällen (keine Fehler-Propagation,
    /// da in bot.py Z. 1749 per `except: log.debug` behandelt).
    pub async fn maybe_respond(&self, event: &ChatMessageEvent, channel_login: &str) {
        if !self.enabled {
            return;
        }

        let raw = event.text().to_string();
        if raw.is_empty() {
            return;
        }
        // Command-Prefix-Gate (bot.py Z. 856–857): `!` ist der feste Präfix
        if raw.starts_with('!') {
            return;
        }

        let low = raw.to_lowercase();

        // Danke-Trigger (bot.py Z. 865)
        let thanks_hit = THANKS_TRIGGERS.iter().any(|w| low.contains(*w));
        if !thanks_hit {
            return;
        }
        // URL-Gate (bot.py Z. 866)
        if low.contains("http") {
            return;
        }

        // Cooldown-Check (bot.py Z. 867, 90 Sekunden)
        if !self.cooldown_ok(channel_login) {
            return;
        }

        let reply = self.pick_reply();
        // Fehler beim Senden werden still ignoriert (bot.py Z. 874 sendet ohne Fehlerbehandlung)
        let _ = self.api.send_message(&event.broadcaster_user_id, reply).await;
    }

    /// Prüft Cooldown und aktualisiert ihn bei Ok.
    fn cooldown_ok(&self, channel_login: &str) -> bool {
        let mut map = self.cooldowns.lock().expect("FunResponses cooldown lock");
        let now = Instant::now();
        if let Some(&last) = map.get(channel_login) {
            if now.duration_since(last).as_secs() < FUN_REPLY_COOLDOWN_SECS {
                return false;
            }
        }
        map.insert(channel_login.to_string(), now);
        true
    }

    /// Wählt deterministisch-abwechselnd eine Antwort (bot.py Z. 868 `random.choice`).
    fn pick_reply(&self) -> &'static str {
        let mut ctr = self.counter.lock().expect("FunResponses counter lock");
        let idx = *ctr % THANKS_REPLIES.len();
        *ctr = ctr.wrapping_add(1);
        THANKS_REPLIES[idx]
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{BanOutcome, ChatApi};
    use crate::types::{ChatMessageBody, SendOutcome};
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};

    // --- Mock-API ---

    struct MockApi {
        sent: Mutex<Vec<(String, String)>>,
    }

    impl MockApi {
        fn new() -> Arc<Self> {
            Arc::new(Self { sent: Mutex::new(vec![]) })
        }
        fn messages(&self) -> Vec<(String, String)> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, broadcaster_id: &str, message: &str) -> Result<SendOutcome, String> {
            self.sent.lock().unwrap().push((broadcaster_id.to_string(), message.to_string()));
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> { Ok(true) }
        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> { Ok(BanOutcome::Banned) }
        async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<BanOutcome, String> { Ok(BanOutcome::Banned) }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> { Ok(true) }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> { Ok(true) }
        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> { Ok(None) }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> { Ok(None) }
        async fn bot_user_id(&self) -> String { "bot123".to_string() }
    }

    fn make_event(text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "ch1".to_string(),
            broadcaster_user_login: "streamer1".to_string(),
            broadcaster_user_name: "Streamer1".to_string(),
            chatter_user_id: "u1".to_string(),
            chatter_user_login: "user1".to_string(),
            chatter_user_name: "User1".to_string(),
            message_id: "m1".to_string(),
            message: ChatMessageBody { text: text.to_string(), fragments: vec![] },
            badges: vec![],
            color: String::new(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn danke_trigger_sendet_antwort() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), true);
        fun.maybe_respond(&make_event("danke fürs zuschauen"), "ch1").await;
        assert_eq!(api.messages().len(), 1);
    }

    #[tokio::test]
    async fn disabled_sendet_nichts() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), false);
        fun.maybe_respond(&make_event("danke"), "ch1").await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn url_in_nachricht_unterdrückt_antwort() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), true);
        fun.maybe_respond(&make_event("danke https://example.com"), "ch1").await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn command_prefix_unterdrückt_antwort() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), true);
        fun.maybe_respond(&make_event("!danke"), "ch1").await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn kein_trigger_sendet_nichts() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), true);
        fun.maybe_respond(&make_event("schöner stream heute"), "ch1").await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn cooldown_verhindert_zweiten_send() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), true);
        fun.maybe_respond(&make_event("danke"), "ch1").await;
        fun.maybe_respond(&make_event("thanks"), "ch1").await;
        // Cooldown läuft noch → nur 1 Nachricht
        assert_eq!(api.messages().len(), 1);
    }

    #[tokio::test]
    async fn cooldown_unterschiedliche_kanäle_unabhängig() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), true);
        fun.maybe_respond(&make_event("danke"), "ch1").await;
        fun.maybe_respond(&make_event("danke"), "ch2").await;
        // Zwei verschiedene Kanäle → je ein Send
        assert_eq!(api.messages().len(), 2);
    }

    #[tokio::test]
    async fn alle_trigger_wörter_matchen() {
        for trigger in &["danke", "thanks", "thx", "merci", "ty"] {
            let api = MockApi::new();
            let fun = FunResponses::new(api.clone(), true);
            fun.maybe_respond(&make_event(&format!("hey {trigger}")), "ch_x").await;
            assert!(!api.messages().is_empty(), "Trigger '{trigger}' löste keine Antwort aus");
        }
    }

    #[tokio::test]
    async fn reply_texte_sind_aus_konstante() {
        let api = MockApi::new();
        let fun = FunResponses::new(api.clone(), true);
        fun.maybe_respond(&make_event("danke"), "ch1").await;
        let msgs = api.messages();
        let text = &msgs[0].1;
        assert!(
            THANKS_REPLIES.contains(&text.as_str()),
            "Antwort '{text}' ist nicht in THANKS_REPLIES"
        );
    }
}

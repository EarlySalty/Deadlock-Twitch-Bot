//! `ChatApi` — der Port für alle ausgehenden Twitch-Aktionen des Chat-Bots
//! (Senden, Announcements, Bans, Unbans, Message-Delete, User-Lookups).
//!
//! Implementierung: `HelixChatClient` (Helix-Endpoints aus
//! `tb-transport-twitch` + `BotTokenManager` mit dem Python-2-Attempt-Muster:
//! 401 → `force_refresh()` → einmal wiederholen). Die Module (Moderation,
//! Promos, Commands, Scam-Warnung) hängen nur am Trait — testbar mit Mocks.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::types::SendOutcome;

pub use tb_transport_twitch::AnnouncementOutcome;
/// Ban-/Timeout-Ergebnis: kanonisch im Transport definiert.
pub use tb_transport_twitch::BanOutcome;

/// Gemeinsamer Command-Antwortweg. Nur eine bestätigte Zustellung ist Erfolg.
/// Keine Wiederholung mutierender Commands; der Aufrufer entscheidet über Sperren.
pub async fn send_reply(api: &dyn ChatApi, broadcaster_id: &str, text: &str) -> bool {
    let kind = match api.send_message(broadcaster_id, text).await {
        Ok(SendOutcome::Sent) => return true,
        Ok(SendOutcome::Dropped { .. }) => "dropped",
        Ok(SendOutcome::HttpError { .. }) => "http_error",
        Err(_) => "transport_error",
    };
    // Gleichartige Fehler je Kanal höchstens einmal pro Tag und zweimal je
    // rollenden sieben Tagen melden, unterdrückte Wiederholungen mitzählen.
    // Keine Antworttexte, HTTP-Bodies oder Zugangsdaten in diesem Log.
    static FAILURES: OnceLock<Mutex<ReplyFailures>> = OnceLock::new();
    let mut failures = FAILURES
        .get_or_init(|| Mutex::new(ReplyFailures::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(suppressed) = failures.record(broadcaster_id, kind, Instant::now()) {
        tracing::warn!(
            broadcaster_id,
            kind,
            suppressed,
            "Command-Antwort nicht zugestellt"
        );
    }
    false
}

#[derive(Default)]
struct ReplyFailures(HashMap<(String, &'static str), ReplyFailureWindow>);

#[derive(Default)]
struct ReplyFailureWindow {
    warnings: std::collections::VecDeque<Instant>,
    suppressed: u64,
}

impl ReplyFailures {
    fn record(&mut self, broadcaster_id: &str, kind: &'static str, now: Instant) -> Option<u64> {
        const DAY: Duration = Duration::from_secs(24 * 60 * 60);
        const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);
        // Offene Wiederholungszahlen bis zur nächsten Meldung erhalten.
        self.0.retain(|_, window| {
            window.suppressed != 0
                || window
                    .warnings
                    .back()
                    .is_some_and(|at| now.duration_since(*at) < WEEK)
        });
        let window = self.0.entry((broadcaster_id.into(), kind)).or_default();
        while window
            .warnings
            .front()
            .is_some_and(|at| now.duration_since(*at) >= WEEK)
        {
            window.warnings.pop_front();
        }
        if window.warnings.len() >= 2
            || window
                .warnings
                .back()
                .is_some_and(|at| now.duration_since(*at) < DAY)
        {
            window.suppressed = window.suppressed.saturating_add(1);
            return None;
        }
        window.warnings.push_back(now);
        Some(std::mem::take(&mut window.suppressed))
    }
}

/// Port für ausgehende Chat-/Moderations-Aktionen mit dem Bot-Token.
#[async_trait]
pub trait ChatApi: Send + Sync {
    /// `POST /helix/chat/messages` — HTTP 200 kann trotzdem Drop sein
    /// (`is_sent=false` + `drop_reason`), siehe `SendOutcome`.
    async fn send_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String>;

    /// `POST /helix/whispers` — braucht `user:manage:whispers` auf dem
    /// Bot-User-Token.
    async fn send_whisper(&self, _to_user_id: &str, _message: &str) -> Result<bool, String> {
        Err("whisper_not_supported".to_string())
    }

    /// `POST /helix/chat/announcements` (Farbe: blue/green/orange/purple/primary).
    /// Python fällt bei Fehlern auf `send_message` zurück — das macht der
    /// Aufrufer, nicht diese Methode.
    async fn send_announcement(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<bool, String>;

    /// Detailvariante für Announcements: additiv zum alten bool-Vertrag.
    /// Implementierungen ohne Detaildaten fallen über den Default auf `accepted`
    /// ohne Status/Body zurück.
    async fn send_announcement_detailed(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<AnnouncementOutcome, String> {
        self.send_announcement(broadcaster_id, message, color)
            .await
            .map(AnnouncementOutcome::from_bool)
    }

    /// `POST /helix/moderation/bans` ohne Dauer (permanenter Ban).
    async fn ban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        reason: &str,
    ) -> Result<BanOutcome, String>;

    /// `POST /helix/moderation/bans` mit `duration` (Timeout).
    async fn timeout_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        duration_secs: u32,
        reason: &str,
    ) -> Result<BanOutcome, String>;

    /// `DELETE /helix/moderation/bans`.
    async fn unban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
    ) -> Result<bool, String>;

    /// `DELETE /helix/moderation/chat` — einzelne Nachricht löschen.
    async fn delete_message(
        &self,
        broadcaster_id: &str,
        message_id: &str,
    ) -> Result<bool, String>;

    /// `GET /helix/users?id=` → `created_at` (Account-Alter für Spam-/
    /// Scam-Eskalatoren). None = User nicht gefunden.
    async fn user_created_at(
        &self,
        user_id: &str,
    ) -> Result<Option<DateTime<Utc>>, String>;

    /// `GET /helix/users?login=` → user_id.
    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String>;

    /// Bot-User-ID (sender_id/moderator_id für alle Aktionen).
    async fn bot_user_id(&self) -> String;
}

#[cfg(test)]
mod reply_failure_tests {
    use super::*;

    #[test]
    fn regression_sendelog_tages_wochenlimit_und_wiederholungszahl() {
        let mut failures = ReplyFailures::default();
        let start = Instant::now();
        let day = Duration::from_secs(86400);
        assert_eq!(failures.record("own", "dropped", start), Some(0));
        assert_eq!(
            failures.record("own", "dropped", start + Duration::from_secs(61)),
            None
        );
        assert_eq!(
            failures.record("own", "dropped", start + day - Duration::from_secs(1)),
            None
        );
        assert_eq!(failures.record("own", "dropped", start + day), Some(2));
        assert_eq!(failures.record("own", "dropped", start + day * 2), None);
        assert_eq!(
            failures.record("own", "dropped", start + day * 7 - Duration::from_secs(1)),
            None
        );
        assert_eq!(
            failures.record("other", "dropped", start + day * 7),
            Some(0)
        );
        assert_eq!(
            failures.record("own", "http_error", start + day * 7),
            Some(0)
        );
        assert_eq!(failures.record("own", "dropped", start + day * 7), Some(2));
        assert_eq!(failures.record("own", "dropped", start + day * 8), Some(0));
        assert_eq!(failures.record("own", "dropped", start + day * 9), None);
        // Auch lange ruhende Einträge dürfen unterdrückte Fehler nicht vergessen.
        assert_eq!(failures.record("own", "dropped", start + day * 30), Some(1));
    }
}

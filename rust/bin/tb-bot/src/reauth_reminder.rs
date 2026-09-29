//! Go-Live-ReAuth-Chat-Reminder (B11, Go-Live-Followup-Teil).
//!
//! Port von `monitoring.py::_maybe_send_reauth_chat_reminder` plus dem
//! `needs_reauth`-Zweig in `eventsub_mixin.py::_handle_stream_went_live`
//! (Z. 1561–1591): Geht ein Partner live, dessen Token re-authentifiziert
//! werden muss (`twitch_raid_auth.needs_reauth`), bekommt er einmalig pro
//! Stream-Start eine freundliche Erinnerung in seinen Twitch-Chat.
//!
//! Diese Followup-Wirkung fiel unter dem nativen Monitoring-Takeover aus:
//! der Rust-Go-Live-Hook legte nur die stream.offline-Subscription an und
//! schickte keine Re-Auth-Erinnerung mehr — betroffene Streamer verloren
//! ihre Autorisierung still, ohne beim Live-Gehen erinnert zu werden.
//!
//! Bewusst NICHT in dieser Slice: Werbefrei-Pitch (`consume_stream_start_pitch`,
//! braucht den Timeout-Guard) und der event-getriebene Chat-Join (läuft nativ
//! über die Sub-Reconcile-Schleife). Diese bleiben eigene Einheiten.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::PgPool;
use tb_chat::ChatApi;
use tb_transport_twitch::SendOutcome;

/// Dedupe-Fenster pro Broadcaster. Python dedupt primär über die `stream_id`
/// (genau ein Reminder pro Stream-Start) mit 300-s-Fallback-Guard. Der native
/// Pfad hat im Hook keine `stream_id`, dedupt aber bereits message_id-basiert
/// über die durable Processing-Inbox — dieses Zeitfenster schützt nur noch
/// gegen Webhook-Retries/Doppel-Trigger; zwei echte Stream-Starts liegen weiter
/// auseinander und bekommen je einen Reminder (= Python-Verhalten).
const REMINDER_DEDUPE_WINDOW: Duration = Duration::from_secs(300);

/// Exakter Text aus `monitoring.py::_reauth_chat_reminder_text` — bewusst
/// byte-identisch (inkl. der ASCII-Schreibweise „Fuer"), damit die nativ
/// gesendete Chat-Nachricht 1:1 der Python-Variante entspricht (Parität).
const REAUTH_REMINDER_TEXT: &str = "Kurze Erinnerung: Fuer den Raid-/Stats-Bot fehlt noch die neue Twitch-Autorisierung. Bitte im Dashboard einloggen und Twitch neu verbinden. Falls du die DM brauchst: Der Re-Auth-Link wurde dir bereits auf Discord geschickt.";

/// Sendet beim Go-Live einmalig eine Re-Auth-Erinnerung an Partner mit
/// `needs_reauth`. Hält ein In-Memory-Dedupe pro Broadcaster.
pub struct ReauthReminder {
    pool: PgPool,
    chat: Arc<dyn ChatApi>,
    last_sent: Mutex<HashMap<String, Instant>>,
}

impl ReauthReminder {
    pub fn new(pool: PgPool, chat: Arc<dyn ChatApi>) -> Self {
        Self {
            pool,
            chat,
            last_sent: Mutex::new(HashMap::new()),
        }
    }

    /// Variante mit aktuellem Stream-Kontext. Ist eine `stream_id` bekannt,
    /// dedupt der Reminder pro Stream-Start statt pauschal pro 300s-Fenster.
    pub async fn maybe_remind_for_stream(
        &self,
        broadcaster_id: &str,
        login: &str,
        stream_id: Option<&str>,
    ) -> bool {
        let broadcaster_id = broadcaster_id.trim();
        let login = login.trim().to_lowercase();
        if broadcaster_id.is_empty() || login.is_empty() {
            return false;
        }

        // Reminder nur, wenn eine `twitch_raid_auth`-Zeile existiert UND eine
        // Re-Auth verlangt. Fehlende Zeile = nie autorisiert (z. B. reiner
        // `is_monitored_only`-Scout-Kanal) → kein Reminder.
        let row = self.load_needs_reauth(broadcaster_id).await;
        if !should_remind(row) {
            return false;
        }

        // Dedupe-Guard VOR dem Senden setzen (Python: race-condition-Fix gegen
        // gleichzeitige Trigger-Pfade).
        {
            let Ok(mut guard) = self.last_sent.lock() else {
                tracing::warn!("ReAuth-Reminder: Dedupe-Lock vergiftet");
                return false;
            };
            let now = Instant::now();
            if !claim_dedupe(&mut guard, broadcaster_id, stream_id, now) {
                return false;
            }
        }

        match self
            .chat
            .send_message(broadcaster_id, REAUTH_REMINDER_TEXT)
            .await
        {
            Ok(SendOutcome::Sent) => {
                tracing::info!(
                    login = %login,
                    broadcaster_id,
                    "ReAuth-Reminder bei Go-Live in den Chat gesendet"
                );
                true
            }
            Ok(other) => {
                tracing::debug!(
                    login = %login,
                    ?other,
                    "ReAuth-Reminder nicht zugestellt (Drop/HTTP-Fehler)"
                );
                false
            }
            Err(error) => {
                tracing::debug!(%error, login = %login, "ReAuth-Reminder-Send fehlgeschlagen");
                false
            }
        }
    }

    /// Liest `needs_reauth` für den Broadcaster. `Option<Option<bool>>`:
    /// äußeres `None` = keine Zeile, inneres `None` = SQL-NULL. Fehler werden
    /// (wie Python) zu „nicht voll autorisiert" verschluckt → `Some(Some(true))`.
    async fn load_needs_reauth(&self, broadcaster_id: &str) -> Option<Option<bool>> {
        match sqlx::query_scalar::<_, Option<bool>>(
            "SELECT needs_reauth FROM twitch_raid_auth WHERE twitch_user_id = $1",
        )
        .bind(broadcaster_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::debug!(%error, broadcaster_id, "needs_reauth-Check fehlgeschlagen");
                // Python fällt bei Exception auf „nicht fully authed" → wir
                // liefern einen Wert, der genau das ergibt.
                Some(Some(true))
            }
        }
    }
}

/// Python `_is_fully_authed`: Zeile muss existieren UND `needs_reauth == 0`.
/// Boolean-Spalte → fully authed nur bei explizit `false`; NULL, `true` oder
/// fehlende Zeile gelten als „nicht voll autorisiert".
fn classify_fully_authed(row: Option<Option<bool>>) -> bool {
    matches!(row, Some(Some(false)))
}

/// Reminder-Gate: senden nur, wenn eine `twitch_raid_auth`-Zeile EXISTIERT und
/// nicht voll autorisiert ist. Eine fehlende Zeile (`None`) heißt „nie
/// autorisiert" — z. B. ein reiner `is_monitored_only`-Scout-Kanal, der nie
/// etwas mit dem Raid-/Stats-Bot zu tun hatte; für den gibt es nichts zu
/// RE-autorisieren. Im Python lag dieser Schutz in der Aufruf-Topologie
/// (`_handle_stream_went_live`-Gates); beim nativen Port fiel er weg, sodass
/// fremde Streamer beim Go-Live fälschlich die Erinnerung bekamen.
fn should_remind(row: Option<Option<bool>>) -> bool {
    row.is_some() && !classify_fully_authed(row)
}

/// Dedupe-Entscheidung: senden, wenn noch nie gesendet oder das Fenster
/// abgelaufen ist.
fn should_send(last: Option<Instant>, now: Instant, window: Duration) -> bool {
    match last {
        None => true,
        Some(prev) => now.duration_since(prev) >= window,
    }
}

fn claim_dedupe(
    sent: &mut HashMap<String, Instant>,
    broadcaster_id: &str,
    stream_id: Option<&str>,
    now: Instant,
) -> bool {
    let stream_id = stream_id.map(str::trim).filter(|s| !s.is_empty());
    let key = match stream_id {
        Some(stream_id) => format!("stream:{broadcaster_id}:{stream_id}"),
        None => format!("fallback:{broadcaster_id}"),
    };
    if stream_id.is_some() {
        return match sent.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(now);
                true
            }
            std::collections::hash_map::Entry::Occupied(_) => false,
        };
    }
    if !should_send(sent.get(&key).copied(), now, REMINDER_DEDUPE_WINDOW) {
        return false;
    }
    sent.insert(key, now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_authed_nur_bei_explizit_false() {
        // Voll autorisiert: Zeile da, needs_reauth = false.
        assert!(classify_fully_authed(Some(Some(false))));
        // needs_reauth = true → nicht fully authed.
        assert!(!classify_fully_authed(Some(Some(true))));
        // NULL → nicht fully authed (Python `None == 0` ist False).
        assert!(!classify_fully_authed(Some(None)));
        // Keine Zeile → nicht fully authed.
        assert!(!classify_fully_authed(None));
    }

    #[test]
    fn should_remind_nur_bei_bestehender_reauth_zeile() {
        // Regression (sagetheman_): keine Zeile = nie autorisiert (z. B. reiner
        // is_monitored_only-Kanal) → KEIN Reminder. Genau dieser Fall hat den
        // Fehl-Reminder an Fremd-Streamer ausgelöst.
        assert!(!should_remind(None));
        // Zeile da, voll autorisiert (needs_reauth = false) → kein Reminder.
        assert!(!should_remind(Some(Some(false))));
        // Zeile da, needs_reauth = true → Reminder (echter Re-Auth-Fall).
        assert!(should_remind(Some(Some(true))));
        // Zeile da, needs_reauth = NULL → Reminder (Python-Parität: nicht fully authed).
        assert!(should_remind(Some(None)));
    }

    #[test]
    fn dedupe_fenster() {
        let now = Instant::now();
        // Noch nie gesendet → senden.
        assert!(should_send(None, now, REMINDER_DEDUPE_WINDOW));
        // Gerade gesendet → unterdrücken.
        assert!(!should_send(Some(now), now, REMINDER_DEDUPE_WINDOW));
        // Innerhalb des Fensters → unterdrücken.
        assert!(!should_send(
            Some(now - Duration::from_secs(120)),
            now,
            REMINDER_DEDUPE_WINDOW
        ));
        // Genau am Fensterrand → senden.
        assert!(should_send(
            Some(now - Duration::from_secs(300)),
            now,
            REMINDER_DEDUPE_WINDOW
        ));
        // Lange her → senden.
        assert!(should_send(
            Some(now - Duration::from_secs(3600)),
            now,
            REMINDER_DEDUPE_WINDOW
        ));
    }

    #[test]
    fn stream_id_dedupe_erlaubt_neuen_stream_im_fenster() {
        let now = Instant::now();
        let mut sent = HashMap::new();
        assert!(claim_dedupe(&mut sent, "42", Some("s-1"), now));
        assert!(!claim_dedupe(
            &mut sent,
            "42",
            Some("s-1"),
            now + Duration::from_secs(10)
        ));
        assert!(claim_dedupe(
            &mut sent,
            "42",
            Some("s-2"),
            now + Duration::from_secs(10)
        ));
        assert!(claim_dedupe(&mut sent, "42", None, now));
        assert!(!claim_dedupe(
            &mut sent,
            "42",
            None,
            now + Duration::from_secs(10)
        ));
    }

    #[test]
    fn reminder_text_ist_python_paritaet() {
        // Byte-identisch zu monitoring.py::_reauth_chat_reminder_text.
        assert!(REAUTH_REMINDER_TEXT.starts_with("Kurze Erinnerung: Fuer den Raid-/Stats-Bot"));
        assert!(
            REAUTH_REMINDER_TEXT.contains("Bitte im Dashboard einloggen und Twitch neu verbinden.")
        );
        assert!(REAUTH_REMINDER_TEXT.ends_with("auf Discord geschickt."));
        // ASCII-Parität: keine echten Umlaute im gesendeten Text.
        assert!(REAUTH_REMINDER_TEXT.is_ascii());
    }
}

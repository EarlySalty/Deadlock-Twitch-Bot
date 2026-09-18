use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use tb_chat::ChatApi;
use tb_raid::flip_unraid::{
    pending_within_flip_window, FlipOutcome, FlipRepeatTracker, FLIP_PAUSE_DEFAULT_SECS,
    FLIP_REPEAT_WINDOW_DEFAULT_SECS, FLIP_WINDOW_DEFAULT_SECS,
};
use tb_raid::pending_raids::PendingRaidStore;
use tb_raid::token_provider::TokenProvider;
use tb_raid::ManualRaidSuppression;
use tb_transport_twitch::HelixClient;

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(default)
}

const PAUSE_NOTICES: [&str; 4] = [
    "Oh, warst eben ganz kurz weg und schon wieder da 🙂 Die automatischen Raids halte ich hier fürs Erste an, damit dir keiner dazwischenfunkt. Willst du selbst raiden, schick deine Leute einfach mit !raid los.",
    "Dein Stream hat gerade kurz geblinzelt und war direkt wieder da 👀 Die automatischen Raids lasse ich hier deshalb erstmal ruhen. Zum Raiden nimmst du einfach !raid, dann schicke ich deine Crew weiter.",
    "Na, kurz weg und gleich wieder zurück 🙂 Ich nehme die automatischen Raids hier fürs Erste raus, sonst wird das wild. Wenn du jemanden raiden magst, geht das jederzeit mit !raid.",
    "Hoppla, dein Stream war eben nur ganz kurz weg und schon wieder da. Die automatischen Raids halte ich hier erstmal zurück. Magst du selbst raiden, sag einfach !raid, dann kümmere ich mich drum.",
];

pub struct FlipUnraidHandler {
    pending: Arc<Mutex<PendingRaidStore>>,
    suppression: Arc<Mutex<ManualRaidSuppression>>,
    token_provider: Arc<TokenProvider>,
    helix: HelixClient,
    chat: Option<Arc<dyn ChatApi>>,
    tracker: Mutex<FlipRepeatTracker>,
    notice_rotation: AtomicUsize,
    window_secs: f64,
    repeat_window_secs: f64,
    pause_secs: f64,
}

impl FlipUnraidHandler {
    pub fn new(
        pending: Arc<Mutex<PendingRaidStore>>,
        suppression: Arc<Mutex<ManualRaidSuppression>>,
        token_provider: Arc<TokenProvider>,
        helix: HelixClient,
        chat: Option<Arc<dyn ChatApi>>,
    ) -> Self {
        Self {
            pending,
            suppression,
            token_provider,
            helix,
            chat,
            tracker: Mutex::new(FlipRepeatTracker::new()),
            notice_rotation: AtomicUsize::new(0),
            window_secs: env_f64("TB_AUTO_UNRAID_WINDOW_SECS", FLIP_WINDOW_DEFAULT_SECS),
            repeat_window_secs: env_f64(
                "TB_FLIP_REPEAT_WINDOW_SECS",
                FLIP_REPEAT_WINDOW_DEFAULT_SECS,
            ),
            pause_secs: env_f64("TB_FLIP_PAUSE_SECS", FLIP_PAUSE_DEFAULT_SECS),
        }
    }

    pub async fn handle_go_live(&self, source_id: &str, source_login: &str) {
        let source_id = source_id.trim();
        if source_id.is_empty() {
            return;
        }
        let login = source_login.trim().to_lowercase();
        let removed = {
            let mut store = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            store.cancel_from_source(&login)
        };
        if removed.is_empty() {
            return;
        }

        let now = unix_now();
        let flipped = removed.iter().any(|pending| {
            pending_within_flip_window(pending.registered_ts, now, self.window_secs)
        });
        if !flipped {
            tracing::debug!(
                streamer = %login,
                entfernt = removed.len(),
                "Go-Live: nur alte ausgehende Raids aufgeräumt, kein Abbruch nötig"
            );
            return;
        }

        let cancelled = self.cancel_source_raid(source_id, &login).await;
        tracing::info!(
            streamer = %login,
            abgebrochen = cancelled,
            "Go-Live im Countdown erkannt: ausgehenden Raid gestoppt"
        );

        let outcome = self
            .tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .record_flip(source_id, now, self.repeat_window_secs, self.pause_secs);
        match outcome {
            FlipOutcome::First => {}
            FlipOutcome::RepeatEntersPause => {
                self.suppression
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .mark(source_id, self.pause_secs, Some(now));
                tracing::warn!(
                    streamer = %login,
                    pause_secs = self.pause_secs,
                    "Wiederholter Go-Live im Kurzfenster: automatische Raids für diese Quelle angehalten"
                );
                self.send_pause_notice(source_id, &login).await;
            }
            FlipOutcome::RepeatAlreadyPaused => {
                tracing::debug!(
                    streamer = %login,
                    "Wiederholter Go-Live, Pause läuft bereits, keine erneute Nachricht"
                );
            }
        }
    }

    async fn cancel_source_raid(&self, source_id: &str, login: &str) -> bool {
        let token = match self
            .token_provider
            .get_valid_token(source_id, Utc::now())
            .await
        {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(streamer = %login, "Kein gültiger Token für Auto-Unraid");
                return false;
            }
            Err(error) => {
                tracing::error!(%error, streamer = %login, "Token-Lookup für Auto-Unraid fehlgeschlagen");
                return false;
            }
        };
        match self.helix.cancel_raid(source_id, &token).await {
            Ok(Ok(())) => true,
            Ok(Err(api_error)) => {
                tracing::warn!(streamer = %login, %api_error, "Auto-Unraid abgelehnt");
                false
            }
            Err(error) => {
                tracing::warn!(streamer = %login, %error, "Auto-Unraid-Request fehlgeschlagen");
                false
            }
        }
    }

    async fn send_pause_notice(&self, source_id: &str, login: &str) {
        let Some(chat) = &self.chat else {
            return;
        };
        let index = self.notice_rotation.fetch_add(1, Ordering::Relaxed) % PAUSE_NOTICES.len();
        if let Err(error) = chat.send_message(source_id, PAUSE_NOTICES[index]).await {
            tracing::warn!(streamer = %login, %error, "Hinweis zur Raid-Pause nicht sendbar");
        }
    }
}

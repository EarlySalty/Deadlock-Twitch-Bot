use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use tb_chat::ChatApi;
use tb_raid::flip_unraid::{pending_within_flip_window, FlipOutcome, FlipRepeatTracker};
#[cfg(test)]
use tb_raid::flip_unraid::{FLIP_PAUSE_DEFAULT_SECS, FLIP_REPEAT_WINDOW_DEFAULT_SECS, FLIP_WINDOW_DEFAULT_SECS};
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

const PAUSE_NOTICES: [&str; 4] = [
    "Oh, warst eben ganz kurz weg und schon wieder da 🙂 Die automatischen Raids halte ich hier fürs Erste an, damit dir keiner dazwischenfunkt. Willst du selbst raiden, schick deine Leute einfach mit !raid los.",
    "Dein Stream hat gerade kurz geblinzelt und war direkt wieder da 👀 Die automatischen Raids lasse ich hier deshalb erstmal ruhen. Zum Raiden nimmst du einfach !raid, dann schicke ich deine Crew weiter.",
    "Na, kurz weg und gleich wieder zurück 🙂 Ich nehme die automatischen Raids hier fürs Erste raus, sonst wird das wild. Wenn du jemanden raiden magst, geht das jederzeit mit !raid.",
    "Hoppla, dein Stream war eben nur ganz kurz weg und schon wieder da. Die automatischen Raids halte ich hier erstmal zurück. Magst du selbst raiden, sag einfach !raid, dann kümmere ich mich drum.",
];

#[async_trait::async_trait]
pub trait SourceRaidCanceller: Send + Sync {
    async fn cancel(&self, source_id: &str) -> bool;
}

pub struct HelixSourceRaidCanceller {
    token_provider: Arc<TokenProvider>,
    helix: HelixClient,
}

impl HelixSourceRaidCanceller {
    pub fn new(token_provider: Arc<TokenProvider>, helix: HelixClient) -> Self {
        Self {
            token_provider,
            helix,
        }
    }
}

#[async_trait::async_trait]
impl SourceRaidCanceller for HelixSourceRaidCanceller {
    async fn cancel(&self, source_id: &str) -> bool {
        let token = match self
            .token_provider
            .get_valid_token(source_id, Utc::now())
            .await
        {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(source_id, "Kein gültiger Token für Auto-Unraid");
                return false;
            }
            Err(error) => {
                tracing::error!(%error, source_id, "Token-Lookup für Auto-Unraid fehlgeschlagen");
                return false;
            }
        };
        match self.helix.cancel_raid(source_id, &token).await {
            Ok(Ok(())) => true,
            Ok(Err(api_error)) => {
                tracing::warn!(source_id, %api_error, "Auto-Unraid abgelehnt");
                false
            }
            Err(error) => {
                tracing::warn!(source_id, %error, "Auto-Unraid-Request fehlgeschlagen");
                false
            }
        }
    }
}

pub struct FlipUnraidHandler {
    pending: Arc<Mutex<PendingRaidStore>>,
    suppression: Arc<Mutex<ManualRaidSuppression>>,
    canceller: Arc<dyn SourceRaidCanceller>,
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
        canceller: Arc<dyn SourceRaidCanceller>,
        chat: Option<Arc<dyn ChatApi>>,
        config: &tb_config::operations::BotOperations,
    ) -> Self {
        Self {
            pending,
            suppression,
            canceller,
            chat,
            tracker: Mutex::new(FlipRepeatTracker::new()),
            notice_rotation: AtomicUsize::new(0),
            window_secs: config.auto_unraid_window_seconds,
            repeat_window_secs: config.flip_repeat_window_seconds,
            pause_secs: config.flip_pause_seconds,
        }
    }

    pub async fn handle_go_live(&self, source_id: &str, source_login: &str) {
        self.handle_go_live_at(source_id, source_login, unix_now())
            .await;
    }

    async fn handle_go_live_at(&self, source_id: &str, source_login: &str, now: f64) {
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

        let cancelled = self.canceller.cancel(source_id).await;
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
                self.send_pause_notice(source_id).await;
            }
            FlipOutcome::RepeatAlreadyPaused => {
                tracing::debug!(
                    streamer = %login,
                    "Wiederholter Go-Live, Pause läuft bereits, keine erneute Nachricht"
                );
            }
        }
    }

    async fn send_pause_notice(&self, source_id: &str) {
        let Some(chat) = &self.chat else {
            return;
        };
        let index = self.notice_rotation.fetch_add(1, Ordering::Relaxed) % PAUSE_NOTICES.len();
        tb_chat::api::send_reply(chat.as_ref(), source_id, PAUSE_NOTICES[index]).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tb_chat::api::AnnouncementOutcome;
    use tb_chat::types::SendOutcome;
    use tb_chat::BanOutcome;
    use tb_raid::pending_raids::PendingRaid;

    #[derive(Default)]
    struct FakeChat {
        sends: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ChatApi for FakeChat {
        async fn send_message(&self, _b: &str, _m: &str) -> Result<SendOutcome, String> {
            self.sends.fetch_add(1, Ordering::Relaxed);
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(&self, _b: &str, _m: &str, _c: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn send_announcement_detailed(
            &self,
            _b: &str,
            _m: &str,
            _c: &str,
        ) -> Result<AnnouncementOutcome, String> {
            Ok(AnnouncementOutcome::accepted())
        }
        async fn ban_user(&self, _b: &str, _t: &str, _r: &str) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }
        async fn timeout_user(
            &self,
            _b: &str,
            _t: &str,
            _d: u32,
            _r: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }
        async fn unban_user(&self, _b: &str, _t: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _b: &str, _m: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn user_created_at(
            &self,
            _u: &str,
        ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
            Ok(None)
        }
        async fn resolve_user_id(&self, _l: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn bot_user_id(&self) -> String {
            "bot".to_string()
        }
    }

    #[derive(Default)]
    struct FakeCanceller {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl SourceRaidCanceller for FakeCanceller {
        async fn cancel(&self, _source_id: &str) -> bool {
            self.calls.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    fn handler(
        chat: Arc<dyn ChatApi>,
        canceller: Arc<dyn SourceRaidCanceller>,
    ) -> FlipUnraidHandler {
        FlipUnraidHandler {
            pending: Arc::new(Mutex::new(PendingRaidStore::new())),
            suppression: Arc::new(Mutex::new(ManualRaidSuppression::new())),
            canceller,
            chat: Some(chat),
            tracker: Mutex::new(FlipRepeatTracker::new()),
            notice_rotation: AtomicUsize::new(0),
            window_secs: FLIP_WINDOW_DEFAULT_SECS,
            repeat_window_secs: FLIP_REPEAT_WINDOW_DEFAULT_SECS,
            pause_secs: FLIP_PAUSE_DEFAULT_SECS,
        }
    }

    fn insert_fresh_pending(h: &FlipUnraidHandler, from_login: &str, registered_ts: f64) {
        let mut store = h
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut pending = PendingRaid::new(from_login, "target1");
        pending.registered_ts = registered_ts;
        store.store(pending);
    }

    #[tokio::test]
    async fn erster_flip_ohne_nachricht_zweiter_flip_genau_eine() {
        let chat = Arc::new(FakeChat::default());
        let canceller = Arc::new(FakeCanceller::default());
        let chat_dyn: Arc<dyn ChatApi> = chat.clone();
        let canceller_dyn: Arc<dyn SourceRaidCanceller> = canceller.clone();
        let h = handler(chat_dyn, canceller_dyn);

        insert_fresh_pending(&h, "quelle", 1000.0);
        h.handle_go_live_at("src1", "quelle", 1000.0).await;
        assert_eq!(
            chat.sends.load(Ordering::Relaxed),
            0,
            "erster Flip schickt keine Nachricht"
        );

        insert_fresh_pending(&h, "quelle", 1100.0);
        h.handle_go_live_at("src1", "quelle", 1100.0).await;
        assert_eq!(
            chat.sends.load(Ordering::Relaxed),
            1,
            "zweiter Flip im Fenster schickt genau eine Nachricht"
        );
        assert_eq!(
            canceller.calls.load(Ordering::Relaxed),
            2,
            "beide Flips brechen den Raid ab"
        );
    }

    #[tokio::test]
    async fn dritter_flip_waehrend_der_pause_schickt_nichts_mehr() {
        let chat = Arc::new(FakeChat::default());
        let canceller = Arc::new(FakeCanceller::default());
        let chat_dyn: Arc<dyn ChatApi> = chat.clone();
        let canceller_dyn: Arc<dyn SourceRaidCanceller> = canceller.clone();
        let h = handler(chat_dyn, canceller_dyn);

        insert_fresh_pending(&h, "quelle", 1000.0);
        h.handle_go_live_at("src1", "quelle", 1000.0).await;
        insert_fresh_pending(&h, "quelle", 1100.0);
        h.handle_go_live_at("src1", "quelle", 1100.0).await;
        insert_fresh_pending(&h, "quelle", 1200.0);
        h.handle_go_live_at("src1", "quelle", 1200.0).await;

        assert_eq!(
            chat.sends.load(Ordering::Relaxed),
            1,
            "die Pause meldet sich nur beim Zustandswechsel, nicht erneut"
        );
    }

    #[tokio::test]
    async fn ohne_pending_kein_abbruch_und_keine_nachricht() {
        let chat = Arc::new(FakeChat::default());
        let canceller = Arc::new(FakeCanceller::default());
        let chat_dyn: Arc<dyn ChatApi> = chat.clone();
        let canceller_dyn: Arc<dyn SourceRaidCanceller> = canceller.clone();
        let h = handler(chat_dyn, canceller_dyn);

        h.handle_go_live_at("src1", "quelle", 1000.0).await;
        assert_eq!(canceller.calls.load(Ordering::Relaxed), 0);
        assert_eq!(chat.sends.load(Ordering::Relaxed), 0);
    }
}

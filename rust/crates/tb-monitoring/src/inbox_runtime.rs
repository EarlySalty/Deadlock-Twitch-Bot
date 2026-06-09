//! Hintergrund-Worker der Processing-Inbox: leased fällige Aufträge,
//! ruft den fachlichen Handler, retryt mit exponentiellem Backoff
//! (1 s · 2^n, Cap 60 s) und dead-lettert nach 5 Versuchen.
//!
//! Bewusste Abweichungen vom Python-Original (siehe Plan-Doc Schritt 4):
//! Store-Fehler im Lease-Pfad töten den Worker nicht (Python-Task stirbt
//! dort still), und kaputtes Payload-JSON läuft sauber in den
//! Retry-/Dead-Letter-Pfad statt in einen NameError im Hook.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;

use crate::inbox_store::{LeasedWork, ProcessingInboxStore};

pub const INBOX_BATCH_SIZE: i64 = 20;
pub const INBOX_LEASE_SECONDS: f64 = 30.0;
pub const INBOX_IDLE_WAIT: Duration = Duration::from_secs(5);
pub const INBOX_RETRY_BASE_SECONDS: f64 = 1.0;
pub const INBOX_RETRY_MAX_SECONDS: f64 = 60.0;
pub const INBOX_MAX_ATTEMPTS: i32 = 5;

/// Fehler aus der fachlichen Verarbeitung — führt zu Retry bzw. Dead-Letter.
pub type HandlerError = Box<dyn std::error::Error + Send + Sync>;

/// Fachliche Verarbeitung eines Auftrags. Muss idempotent sein — die Inbox
/// garantiert at-least-once, Exactly-once liefert erst der Guard-Store.
#[async_trait::async_trait]
pub trait InboxHandler: Send + Sync {
    async fn handle(
        &self,
        work_type: &str,
        payload: &serde_json::Value,
    ) -> Result<(), HandlerError>;
}

/// Benachrichtigung nach einem Dead-Letter (z. B. für Admin-Alarme).
#[derive(Debug, Clone)]
pub struct DeadLetterNotice {
    pub work_id: String,
    pub work_type: String,
    pub message_id: Option<String>,
    /// Geparster Payload; leeres Objekt, wenn das JSON selbst kaputt war.
    pub payload: serde_json::Value,
    pub attempt_count: i32,
    pub last_error: String,
}

#[async_trait::async_trait]
pub trait DeadLetterHook: Send + Sync {
    async fn on_dead_letter(&self, notice: DeadLetterNotice);
}

/// Uhr in Epoch-Sekunden — injizierbar für deterministische Tests.
pub type ClockFn = Arc<dyn Fn() -> f64 + Send + Sync>;

/// System-Uhr in Epoch-Sekunden — Standard-Implementierung für [`ClockFn`].
pub fn epoch_clock() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Konfigurierter, noch nicht gestarteter Worker.
pub struct InboxRuntime {
    store: ProcessingInboxStore,
    handler: Arc<dyn InboxHandler>,
    dead_letter_hook: Option<Arc<dyn DeadLetterHook>>,
    clock: ClockFn,
}

impl InboxRuntime {
    pub fn new(store: ProcessingInboxStore, handler: Arc<dyn InboxHandler>) -> Self {
        Self {
            store,
            handler,
            dead_letter_hook: None,
            clock: Arc::new(epoch_clock),
        }
    }

    pub fn with_dead_letter_hook(mut self, hook: Arc<dyn DeadLetterHook>) -> Self {
        self.dead_letter_hook = Some(hook);
        self
    }

    /// Testbarkeit: injizierbare Uhr (Epoch-Sekunden).
    pub fn with_clock(mut self, clock: ClockFn) -> Self {
        self.clock = clock;
        self
    }

    /// Startet den Worker als Tokio-Task und gibt das Steuer-Handle zurück.
    pub fn start(self) -> InboxRuntimeHandle {
        let wakeup = Arc::new(Notify::new());
        let (stop_tx, stop_rx) = watch::channel(false);
        let store = self.store.clone();
        let clock = self.clock.clone();
        let worker_wakeup = wakeup.clone();
        let task = tokio::spawn(async move {
            self.run(worker_wakeup, stop_rx).await;
        });
        InboxRuntimeHandle {
            store,
            clock,
            wakeup,
            stop: stop_tx,
            task,
        }
    }

    async fn run(self, wakeup: Arc<Notify>, mut stop: watch::Receiver<bool>) {
        while !*stop.borrow() {
            let processed = self.process_due_batch(&stop).await;
            if processed {
                continue;
            }
            tokio::select! {
                _ = wakeup.notified() => {}
                _ = tokio::time::sleep(INBOX_IDLE_WAIT) => {}
                changed = stop.changed() => {
                    // Sender weg (Handle gedroppt ohne shutdown): beenden statt
                    // busy-loopen — stoppen kann uns ohnehin niemand mehr.
                    if changed.is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// Verarbeitet einen Schwung fälliger Aufträge. `true` = es gab Arbeit
    /// (sofort weiterpollen), `false` = leer/Fehler (idle warten).
    async fn process_due_batch(&self, stop: &watch::Receiver<bool>) -> bool {
        let now = (self.clock)();
        let leased = match self
            .store
            .lease_due(now, INBOX_LEASE_SECONDS, INBOX_BATCH_SIZE)
            .await
        {
            Ok(leased) => leased,
            Err(error) => {
                tracing::error!(%error, "Inbox-Lease fehlgeschlagen — idle und erneut versuchen");
                return false;
            }
        };
        if leased.is_empty() {
            return false;
        }
        for work in leased {
            if *stop.borrow() {
                return true;
            }
            self.process_one(work).await;
        }
        true
    }

    async fn process_one(&self, work: LeasedWork) {
        let parsed = parse_payload(&work.payload_json);
        let result = match &parsed {
            Ok(payload) => self.handler.handle(&work.work_type, payload).await,
            Err(parse_error) => {
                Err(format!("invalid eventsub processing payload: {parse_error}").into())
            }
        };
        match result {
            Ok(()) => {
                if let Err(error) = self.store.mark_delivered(&work.work_id).await {
                    // Auftrag bleibt liegen und läuft nach Lease-Ablauf erneut —
                    // at-least-once, Handler sind idempotent (Guard-Store).
                    tracing::error!(%error, work_id = %work.work_id, "mark_delivered fehlgeschlagen");
                }
            }
            Err(handler_error) => {
                self.handle_failure(work, parsed.ok(), handler_error.to_string())
                    .await;
            }
        }
    }

    async fn handle_failure(
        &self,
        work: LeasedWork,
        payload: Option<serde_json::Value>,
        error_message: String,
    ) {
        let next_attempt_count = work.attempt_count + 1;
        if next_attempt_count >= INBOX_MAX_ATTEMPTS {
            let dead_lettered_at = (self.clock)();
            if let Err(error) = self
                .store
                .mark_dead_letter(&work, next_attempt_count, &error_message, dead_lettered_at)
                .await
            {
                tracing::error!(%error, work_id = %work.work_id, "mark_dead_letter fehlgeschlagen");
                return;
            }
            tracing::error!(
                work_type = %work.work_type,
                work_id = %work.work_id,
                message_id = work.message_id.as_deref().unwrap_or("n/a"),
                attempts = next_attempt_count,
                error = %error_message,
                "Inbox-Auftrag dead-lettered"
            );
            if let Some(hook) = &self.dead_letter_hook {
                hook.on_dead_letter(DeadLetterNotice {
                    work_id: work.work_id,
                    work_type: work.work_type,
                    message_id: work.message_id,
                    payload: payload.unwrap_or_else(|| serde_json::json!({})),
                    attempt_count: next_attempt_count,
                    last_error: error_message,
                })
                .await;
            }
            return;
        }
        let next_attempt_at = (self.clock)() + retry_delay_seconds(next_attempt_count);
        if let Err(error) = self
            .store
            .mark_retry(
                &work.work_id,
                next_attempt_count,
                &error_message,
                next_attempt_at,
            )
            .await
        {
            tracing::error!(%error, work_id = %work.work_id, "mark_retry fehlgeschlagen");
            return;
        }
        tracing::warn!(
            work_type = %work.work_type,
            work_id = %work.work_id,
            attempt = next_attempt_count,
            max_attempts = INBOX_MAX_ATTEMPTS,
            error = %error_message,
            "Inbox-Auftrag wird erneut versucht"
        );
    }
}

/// Steuer-Handle des laufenden Workers.
pub struct InboxRuntimeHandle {
    store: ProcessingInboxStore,
    clock: ClockFn,
    wakeup: Arc<Notify>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl InboxRuntimeHandle {
    /// Legt einen Auftrag an und weckt den Worker sofort.
    pub async fn enqueue(
        &self,
        work_type: &str,
        payload: &serde_json::Value,
        message_id: Option<&str>,
    ) -> Result<String, sqlx::Error> {
        let work_id = self
            .store
            .enqueue(work_type, payload, message_id, (self.clock)())
            .await?;
        self.wakeup.notify_one();
        Ok(work_id)
    }

    /// Leichtgewichtiger, klonbarer Enqueue-Zugang (z. B. für den Dispatcher),
    /// ohne das Steuer-Handle (shutdown) herauszugeben.
    pub fn enqueuer(&self) -> InboxEnqueuer {
        InboxEnqueuer {
            store: self.store.clone(),
            clock: self.clock.clone(),
            wakeup: self.wakeup.clone(),
        }
    }

    /// Stoppt den Worker geordnet; ein laufender Auftrag wird noch beendet.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        self.wakeup.notify_one();
        let _ = self.task.await;
    }
}

/// Enqueue-only-Sicht auf die laufende Inbox (weckt den Worker mit).
#[derive(Clone)]
pub struct InboxEnqueuer {
    store: ProcessingInboxStore,
    clock: ClockFn,
    wakeup: Arc<Notify>,
}

impl InboxEnqueuer {
    pub async fn enqueue(
        &self,
        work_type: &str,
        payload: &serde_json::Value,
        message_id: Option<&str>,
    ) -> Result<String, sqlx::Error> {
        let work_id = self
            .store
            .enqueue(work_type, payload, message_id, (self.clock)())
            .await?;
        self.wakeup.notify_one();
        Ok(work_id)
    }
}

fn parse_payload(payload_json: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value = serde_json::from_str(payload_json).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("payload ist kein JSON-Objekt".to_string());
    }
    Ok(value)
}

fn retry_delay_seconds(attempts: i32) -> f64 {
    let scaled = INBOX_RETRY_BASE_SECONDS * 2f64.powi((attempts - 1).max(0));
    scaled.min(INBOX_RETRY_MAX_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::retry_delay_seconds;

    #[test]
    fn backoff_verdoppelt_und_kappt_bei_60s() {
        assert_eq!(retry_delay_seconds(1), 1.0);
        assert_eq!(retry_delay_seconds(2), 2.0);
        assert_eq!(retry_delay_seconds(3), 4.0);
        assert_eq!(retry_delay_seconds(7), 60.0);
        assert_eq!(retry_delay_seconds(0), 1.0);
    }
}

use std::sync::Arc;
use tokio::time::{sleep, Duration};

use super::service::ClipFetchService;

/// Standard-Intervall: 6 Stunden (wie Python).
const DEFAULT_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Initiale Verzögerung nach Prozessstart: 60 Sekunden.
const INITIAL_DELAY: Duration = Duration::from_secs(60);

/// Periodischer Hintergrund-Task für den Clip-Fetcher.
///
/// In Python (`ClipFetcher.__init__`) bedingungslos gestartet (always-on, 6h).
/// [`start`](Self::start) startet den Loop nach ausdrücklicher Freigabe
/// durch die typisierte Bot-Konfiguration in der Composition-Root.
pub struct ClipFetchTask {
    service: Arc<ClipFetchService>,
    interval: Duration,
    initial_delay: Duration,
}

impl ClipFetchTask {
    pub fn new(service: Arc<ClipFetchService>) -> Self {
        Self {
            service,
            interval: DEFAULT_INTERVAL,
            initial_delay: INITIAL_DELAY,
        }
    }

    /// Startet den Task bedingungslos (1:1 zu Pythons always-on-ClipFetcher).
    pub fn start(self) {
        tracing::info!(
            "clip_fetch: Task startet (Intervall={}s, InitialDelay={}s)",
            self.interval.as_secs(),
            self.initial_delay.as_secs(),
        );
        spawn_logged("clip_fetch", self.run());
    }

    async fn run(self) {
        sleep(self.initial_delay).await;

        loop {
            self.service.fetch_all_active_partners().await;
            sleep(self.interval).await;
        }
    }
}

fn spawn_logged(task: &'static str, future: impl std::future::Future<Output = ()> + Send + 'static) {
    let handle = tokio::spawn(future);
    tokio::spawn(async move {
        if let Err(error) = handle.await {
            tracing::error!(task, %error, "Social-Media-Clip-Task fehlerhaft beendet");
        }
    });
}

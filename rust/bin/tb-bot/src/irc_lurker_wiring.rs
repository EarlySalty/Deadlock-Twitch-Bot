//! Composition-Root fuer den anonymen IRC-Lurker Presence-Harvester.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tb_monitoring::{IrcLurkerTracker, TrackMode};

const SYNC_INTERVAL: Duration = Duration::from_secs(60);
// 600ms/Track = ~16 JOINs/10s, sicher unter Twitchs 20-JOINs/10s-Limit.
const TRACK_STAGGER: Duration = Duration::from_millis(600);
const MAX_TRACKED_CHANNELS: usize = 250;

pub fn spawn_irc_lurker(pool: PgPool) {
    if std::env::var("TB_IRC_LURKER_ENABLED").as_deref() != Ok("1") {
        tracing::info!("IRC-Lurker (anon Presence) deaktiviert (TB_IRC_LURKER_ENABLED!=1)");
        return;
    }

    tracing::info!("IRC-Lurker anon Presence-Harvester aktiv");

    let tracker = Arc::new(IrcLurkerTracker::new(
        pool.clone(),
        String::new(),
        String::new(),
        None,
    ));

    let runner = Arc::clone(&tracker);
    tokio::spawn(async move {
        runner.run().await;
    });

    tokio::spawn(sync_loop(pool, tracker));
}

async fn sync_loop(pool: PgPool, tracker: Arc<IrcLurkerTracker>) {
    let mut tracked = HashSet::new();
    let mut tick = tokio::time::interval(SYNC_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tick.tick().await;
        match load_live_channels(&pool).await {
            Ok(live_channels) => {
                sync_tracked_channels(tracker.as_ref(), &mut tracked, live_channels).await;
            }
            Err(error) => {
                tracing::warn!(%error, "IRC-Lurker Live-Roster konnte nicht geladen werden");
            }
        }
    }
}

async fn load_live_channels(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let channels: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT LOWER(streamer_login) \
         FROM twitch_live_state \
         WHERE is_live = 1 \
           AND active_session_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(normalize_channels(channels))
}

async fn sync_tracked_channels<T: ChannelTracker + ?Sized>(
    tracker: &T,
    tracked: &mut HashSet<String>,
    live_channels: Vec<String>,
) {
    let live_count = live_channels.len();
    let plan = plan_channel_sync(&live_channels, tracked, MAX_TRACKED_CHANNELS);
    if plan.skipped_over_limit > 0 {
        tracing::warn!(
            live_channels = live_count,
            max_channels = MAX_TRACKED_CHANNELS,
            skipped_channels = plan.skipped_over_limit,
            "IRC-Lurker Live-Roster ueber Limit; ueberschuessige Kanaele ausgelassen"
        );
    }

    let added_channels = plan.to_track.len();
    let removed_channels = plan.to_untrack.len();
    let skipped_channels = plan.skipped_over_limit;
    let tracked_channels = plan.desired.len();

    apply_channel_sync(tracker, &plan, TRACK_STAGGER).await;
    *tracked = plan.desired;

    tracing::info!(
        live_channels = live_count,
        tracked_channels,
        added_channels,
        removed_channels,
        skipped_channels,
        "IRC-Lurker Tracking-Sync abgeschlossen"
    );
}

trait ChannelTracker: Send + Sync {
    fn track_channel(&self, channel: &str, mode: TrackMode);
    fn untrack_channel(&self, channel: &str);
}

impl ChannelTracker for IrcLurkerTracker {
    fn track_channel(&self, channel: &str, mode: TrackMode) {
        IrcLurkerTracker::track_channel(self, channel, mode);
    }

    fn untrack_channel(&self, channel: &str) {
        IrcLurkerTracker::untrack_channel(self, channel);
    }
}

async fn apply_channel_sync<T: ChannelTracker + ?Sized>(
    tracker: &T,
    plan: &ChannelSyncPlan,
    track_delay: Duration,
) {
    for channel in &plan.to_untrack {
        tracker.untrack_channel(channel);
    }

    for channel in &plan.to_track {
        tracker.track_channel(channel, TrackMode::Category);
        if !track_delay.is_zero() {
            tokio::time::sleep(track_delay).await;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelSyncPlan {
    desired: HashSet<String>,
    to_track: Vec<String>,
    to_untrack: Vec<String>,
    skipped_over_limit: usize,
}

fn plan_channel_sync(
    live_channels: &[String],
    tracked: &HashSet<String>,
    max_channels: usize,
) -> ChannelSyncPlan {
    let mut desired_ordered = normalize_channels(live_channels.iter().cloned());
    let skipped_over_limit = desired_ordered.len().saturating_sub(max_channels);
    if desired_ordered.len() > max_channels {
        desired_ordered.truncate(max_channels);
    }

    let desired: HashSet<String> = desired_ordered.iter().cloned().collect();
    let to_track = desired_ordered
        .iter()
        .filter(|channel| !tracked.contains(*channel))
        .cloned()
        .collect();
    let mut to_untrack: Vec<String> = tracked.difference(&desired).cloned().collect();
    to_untrack.sort();

    ChannelSyncPlan {
        desired,
        to_track,
        to_untrack,
        skipped_over_limit,
    }
}

fn normalize_channels(channels: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut normalized: Vec<String> = channels
        .into_iter()
        .filter_map(|channel| normalize_channel(&channel))
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_channel(channel: &str) -> Option<String> {
    let login = channel
        .trim()
        .trim_start_matches('#')
        .trim()
        .to_lowercase();
    (!login.is_empty()).then_some(login)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct SpyTracker {
        calls: Mutex<Vec<String>>,
    }

    impl ChannelTracker for SpyTracker {
        fn track_channel(&self, channel: &str, mode: TrackMode) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("track:{channel}:{mode:?}"));
        }

        fn untrack_channel(&self, channel: &str) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("untrack:{channel}"));
        }
    }

    #[tokio::test]
    async fn channel_diff_ruft_track_und_untrack_korrekt_auf() {
        let tracked = HashSet::from(["stay".to_string(), "old".to_string()]);
        let live = vec![
            "Stay".to_string(),
            "#new".to_string(),
            "new".to_string(),
            " ".to_string(),
        ];
        let plan = plan_channel_sync(&live, &tracked, MAX_TRACKED_CHANNELS);
        assert_eq!(plan.to_track, vec!["new".to_string()]);
        assert_eq!(plan.to_untrack, vec!["old".to_string()]);
        assert_eq!(plan.skipped_over_limit, 0);

        let spy = SpyTracker::default();
        apply_channel_sync(&spy, &plan, Duration::ZERO).await;

        assert_eq!(
            spy.calls.lock().unwrap().as_slice(),
            [
                "untrack:old".to_string(),
                "track:new:Category".to_string()
            ]
        );
    }
}

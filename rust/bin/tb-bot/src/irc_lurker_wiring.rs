//! Composition-Root fuer den anonymen IRC-Lurker Presence-Harvester.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::PgPool;
use sqlx::Row;
use tb_monitoring::{IrcLurkerTracker, TrackMode, WriteStats};

use crate::raid_greeting::RaidTargetChatProbe;
use crate::task_supervisor::TaskSupervisor;

const SYNC_INTERVAL: Duration = Duration::from_secs(60);
// 600ms/Track = ~16 JOINs/10s, sicher unter Twitchs 20-JOINs/10s-Limit.
const TRACK_STAGGER: Duration = Duration::from_millis(600);
const MAX_TRACKED_CHANNELS: usize = 250;

pub fn build_irc_lurker(pool: PgPool) -> Option<Arc<IrcLurkerTracker>> {
    if std::env::var("TB_IRC_LURKER_ENABLED").as_deref() != Ok("1") {
        tracing::info!("IRC-Lurker (anon Presence) deaktiviert (TB_IRC_LURKER_ENABLED!=1)");
        return None;
    }

    tracing::info!("IRC-Lurker anon Presence-Harvester aktiv");
    Some(Arc::new(IrcLurkerTracker::new(
        pool,
        String::new(),
        String::new(),
        None,
    )))
}

pub fn spawn_irc_lurker(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    tracker: Option<Arc<IrcLurkerTracker>>,
) {
    let Some(tracker) = tracker else {
        return;
    };

    let runner = Arc::clone(&tracker);
    supervisor.spawn("irc_lurker_runner", async move {
        runner.run().await;
    });

    supervisor.spawn("irc_lurker_sync", sync_loop(pool, tracker));
}

impl RaidTargetChatProbe for IrcLurkerTracker {
    fn watch(&self, channel: &str) {
        self.watch_raid_channel(channel);
    }

    fn unwatch(&self, channel: &str) {
        self.unwatch_raid_channel(channel);
    }

    fn write_stats(&self, channel: &str, nick: &str, since: Instant) -> Option<WriteStats> {
        self.write_stats_since(channel, nick, since)
    }
}

async fn sync_loop(pool: PgPool, tracker: Arc<IrcLurkerTracker>) {
    let mut tracked = HashMap::new();
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

async fn load_live_channels(pool: &PgPool) -> Result<Vec<TrackedChannel>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT LOWER(ls.streamer_login) AS channel_login, \
                COALESCE(MAX(CASE \
                    WHEN COALESCE(ps.is_partner_active, 0) <> 0 \
                      OR EXISTS ( \
                          SELECT 1 FROM twitch_streamers s \
                          WHERE ((NULLIF(ls.twitch_user_id, '') IS NOT NULL \
                                  AND s.twitch_user_id = ls.twitch_user_id) \
                              OR LOWER(s.twitch_login) = LOWER(ls.streamer_login)) \
                            AND NOT EXISTS ( \
                                SELECT 1 FROM twitch_partners p \
                                WHERE p.twitch_user_id = s.twitch_user_id \
                                   OR LOWER(p.twitch_login) = LOWER(s.twitch_login) \
                            ) \
                      ) \
                    THEN 1 ELSE 0 END), 0)::int AS partner_like \
         FROM twitch_live_state ls \
         LEFT JOIN twitch_streamers_partner_state ps \
                ON ((NULLIF(ls.twitch_user_id, '') IS NOT NULL \
                     AND ps.twitch_user_id = ls.twitch_user_id) \
                    OR LOWER(ps.twitch_login) = LOWER(ls.streamer_login)) \
         WHERE ls.is_live = 1 \
           AND ls.active_session_id IS NOT NULL \
         GROUP BY LOWER(ls.streamer_login)",
    )
    .fetch_all(pool)
    .await?;

    let mut channels = Vec::with_capacity(rows.len());
    for row in rows {
        let login: String = row.try_get("channel_login")?;
        let partner_like: i32 = row.try_get("partner_like")?;
        if let Some(channel) = normalize_channel(&login) {
            channels.push(TrackedChannel {
                login: channel,
                mode: if partner_like != 0 {
                    TrackMode::Partner
                } else {
                    TrackMode::Category
                },
            });
        }
    }

    Ok(normalize_tracked_channels(channels))
}

async fn sync_tracked_channels<T: ChannelTracker + ?Sized>(
    tracker: &T,
    tracked: &mut HashMap<String, TrackMode>,
    live_channels: Vec<TrackedChannel>,
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
    *tracked = plan.desired.clone();

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
        tracker.track_channel(&channel.login, channel.mode);
        if !track_delay.is_zero() {
            tokio::time::sleep(track_delay).await;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelSyncPlan {
    desired: HashMap<String, TrackMode>,
    to_track: Vec<TrackedChannel>,
    to_untrack: Vec<String>,
    skipped_over_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackedChannel {
    login: String,
    mode: TrackMode,
}

fn plan_channel_sync(
    live_channels: &[TrackedChannel],
    tracked: &HashMap<String, TrackMode>,
    max_channels: usize,
) -> ChannelSyncPlan {
    let mut desired_ordered = normalize_tracked_channels(live_channels.iter().cloned());
    let skipped_over_limit = desired_ordered.len().saturating_sub(max_channels);
    if desired_ordered.len() > max_channels {
        desired_ordered.truncate(max_channels);
    }

    let desired: HashMap<String, TrackMode> = desired_ordered
        .iter()
        .map(|channel| (channel.login.clone(), channel.mode))
        .collect();
    let to_track = desired_ordered
        .iter()
        .filter(|channel| tracked.get(&channel.login).copied() != Some(channel.mode))
        .cloned()
        .collect();
    let mut to_untrack: Vec<String> = tracked
        .keys()
        .filter(|channel| !desired.contains_key(*channel))
        .cloned()
        .collect();
    to_untrack.sort();

    ChannelSyncPlan {
        desired,
        to_track,
        to_untrack,
        skipped_over_limit,
    }
}

fn normalize_tracked_channels(
    channels: impl IntoIterator<Item = TrackedChannel>,
) -> Vec<TrackedChannel> {
    let mut by_login: HashMap<String, TrackMode> = HashMap::new();
    for channel in channels {
        let Some(login) = normalize_channel(&channel.login) else {
            continue;
        };
        by_login
            .entry(login)
            .and_modify(|mode| {
                if channel.mode == TrackMode::Partner {
                    *mode = TrackMode::Partner;
                }
            })
            .or_insert(channel.mode);
    }
    let mut normalized: Vec<TrackedChannel> = by_login
        .into_iter()
        .map(|(login, mode)| TrackedChannel { login, mode })
        .collect();
    normalized.sort_by(|a, b| a.login.cmp(&b.login));
    normalized
}

fn normalize_channel(channel: &str) -> Option<String> {
    let login = channel.trim().trim_start_matches('#').trim().to_lowercase();
    (!login.is_empty()).then_some(login)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
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
        let tracked = HashMap::from([
            ("stay".to_string(), TrackMode::Partner),
            ("old".to_string(), TrackMode::Partner),
            ("flip".to_string(), TrackMode::Category),
        ]);
        let live = vec![
            TrackedChannel {
                login: "Stay".to_string(),
                mode: TrackMode::Partner,
            },
            TrackedChannel {
                login: "#new".to_string(),
                mode: TrackMode::Category,
            },
            TrackedChannel {
                login: "new".to_string(),
                mode: TrackMode::Partner,
            },
            TrackedChannel {
                login: "flip".to_string(),
                mode: TrackMode::Partner,
            },
            TrackedChannel {
                login: " ".to_string(),
                mode: TrackMode::Category,
            },
        ];
        let plan = plan_channel_sync(&live, &tracked, MAX_TRACKED_CHANNELS);
        assert_eq!(
            plan.to_track,
            vec![
                TrackedChannel {
                    login: "flip".to_string(),
                    mode: TrackMode::Partner,
                },
                TrackedChannel {
                    login: "new".to_string(),
                    mode: TrackMode::Partner,
                },
            ]
        );
        assert_eq!(plan.to_untrack, vec!["old".to_string()]);
        assert_eq!(plan.skipped_over_limit, 0);

        let spy = SpyTracker::default();
        apply_channel_sync(&spy, &plan, Duration::ZERO).await;

        assert_eq!(
            spy.calls.lock().unwrap().as_slice(),
            [
                "untrack:old".to_string(),
                "track:flip:Partner".to_string(),
                "track:new:Partner".to_string()
            ]
        );
    }

    async fn setup_db(schema_suffix: &str) -> Option<PgPool> {
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return None;
        };
        let schema = format!("tb_bot_irc_lurker_{schema_suffix}_{}", std::process::id());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_live_state (
                twitch_user_id TEXT NOT NULL,
                streamer_login TEXT NOT NULL,
                is_live INTEGER DEFAULT 0,
                active_session_id BIGINT
            )",
            "CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT,
                twitch_user_id TEXT,
                is_partner_active INTEGER,
                is_monitored_only INTEGER
            )",
            "CREATE TABLE twitch_streamers (
                twitch_login TEXT,
                twitch_user_id TEXT
            )",
            "CREATE TABLE twitch_partners (
                twitch_login TEXT,
                twitch_user_id TEXT
            )",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn live_monitored_only_ohne_partner_zeile_wird_partner_tracking() {
        let Some(pool) = setup_db("monitored_only").await else {
            return;
        };

        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('p1', 'partner_live', 1, 10),
                    ('s1', 'scout_live', 1, 11),
                    ('c1', 'category_live', 1, 12)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state
                (twitch_login, twitch_user_id, is_partner_active, is_monitored_only)
             VALUES ('partner_live', 'p1', 1, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
             VALUES ('scout_live', 's1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let channels = load_live_channels(&pool).await.unwrap();
        let mode_for = |login: &str| {
            channels
                .iter()
                .find(|channel| channel.login == login)
                .map(|channel| channel.mode)
        };

        assert_eq!(mode_for("partner_live"), Some(TrackMode::Partner));
        assert_eq!(mode_for("scout_live"), Some(TrackMode::Partner));
        assert_eq!(mode_for("category_live"), Some(TrackMode::Category));
    }
}

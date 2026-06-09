//! Hermetische Tests des Poll-Loops (Slice 4c): Transitions über zwei Ticks,
//! Hook-/Refresh-Verhalten und Stats-Kadenz — mit Stub-StreamSource statt
//! Helix, Noop-Announcements und Recording-Hooks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use sqlx::PgPool;
use tb_monitoring::poller::{NoopAnnouncementSink, PollHooks, TickReport};
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::{
    ExpSessionStore, ExpSessionTracker, GuardStore, LiveStateStore, NoFollowerSource, PollConfig,
    PollEngine, PollIntervalStore, SessionTracker, StatsStore, StreamSnapshot, StreamSource,
    TrackedStore,
};

mod support;

macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
}

/// Programmierbare Stream-Quelle: Login-Streams pro Tick setzbar.
struct StubSource {
    streams: Mutex<Vec<StreamSnapshot>>,
    category: Mutex<Vec<StreamSnapshot>>,
}

impl StubSource {
    fn new() -> Self {
        Self {
            streams: Mutex::new(Vec::new()),
            category: Mutex::new(Vec::new()),
        }
    }
    fn set_streams(&self, streams: Vec<StreamSnapshot>) {
        *self.streams.lock().unwrap() = streams;
    }
}

#[async_trait::async_trait]
impl StreamSource for StubSource {
    async fn streams_by_logins(
        &self,
        _logins: &[String],
        _language: Option<&str>,
    ) -> Result<Vec<StreamSnapshot>, tb_monitoring::poller::SourceError> {
        Ok(self.streams.lock().unwrap().clone())
    }
    async fn streams_by_category(
        &self,
        _category_id: &str,
        _language: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<StreamSnapshot>, tb_monitoring::poller::SourceError> {
        Ok(self.category.lock().unwrap().clone())
    }
    async fn category_id(
        &self,
        _game_name: &str,
    ) -> Result<Option<String>, tb_monitoring::poller::SourceError> {
        Ok(Some("g1".to_string()))
    }
}

/// Zeichnet Hook-Aufrufe auf.
struct RecordingHooks {
    went_live: AtomicU64,
    reports: Mutex<Vec<TickReport>>,
}

impl RecordingHooks {
    fn new() -> Self {
        Self {
            went_live: AtomicU64::new(0),
            reports: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl PollHooks for RecordingHooks {
    async fn on_stream_went_live(&self, _twitch_user_id: &str, _login: &str) {
        self.went_live.fetch_add(1, Ordering::SeqCst);
    }
    async fn after_tick(&self, report: TickReport) {
        self.reports.lock().unwrap().push(report);
    }
}

fn engine_with(pool: &PgPool, source: Arc<StubSource>, hooks: Arc<RecordingHooks>) -> PollEngine {
    let tracker = Arc::new(SessionTracker::new(
        SessionStore::new(pool.clone()),
        LiveStateStore::new(pool.clone()),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        Arc::new(NoFollowerSource),
        "Deadlock",
    ));
    PollEngine::new(
        source,
        TrackedStore::new(pool.clone()),
        LiveStateStore::new(pool.clone()),
        SessionStore::new(pool.clone()),
        tracker,
        StatsStore::new(pool.clone()),
        GuardStore::new(pool.clone()),
        Arc::new(NoopAnnouncementSink),
        hooks,
        PollIntervalStore::new(pool.clone()),
        PollConfig::default(),
    )
}

fn live_stream(login: &str, user_id: &str, stream_id: &str, viewers: i32) -> StreamSnapshot {
    StreamSnapshot {
        id: Some(stream_id.to_string()),
        user_login: login.to_string(),
        user_name: login.to_uppercase(),
        title: "Ranked".to_string(),
        game_name: "Deadlock".to_string(),
        language: "de".to_string(),
        viewer_count: viewers,
        is_mature: false,
        tags: vec![format!("uid-{user_id}")],
        started_at: Some((Utc::now() - Duration::minutes(5)).to_rfc3339()),
        thumbnail_url: None,
    }
}

#[tokio::test]
async fn tick_transitions_online_dann_offline() {
    let pool = pool_or_skip!("t4c_transitions");
    // Verifizierter Partner mit aktivem Raid-Bot.
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state
            (twitch_login, twitch_user_id, is_partner_active, is_partner)
         VALUES ('drag', '42', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
         VALUES ('42', 'drag', 'active', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let source = Arc::new(StubSource::new());
    let hooks = Arc::new(RecordingHooks::new());
    let engine = engine_with(&pool, source.clone(), hooks.clone());

    // Tick 1: drag ist live.
    source.set_streams(vec![live_stream("drag", "42", "s-1", 10)]);
    engine.tick().await;

    #[derive(sqlx::FromRow)]
    struct StateRow {
        is_live: Option<i32>,
        had_deadlock_in_session: Option<i32>,
        active_session_id: Option<i64>,
        last_game: Option<String>,
    }
    let state: StateRow = sqlx::query_as(
        "SELECT is_live, had_deadlock_in_session, active_session_id, last_game
           FROM twitch_live_state WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state.is_live, Some(1));
    assert_eq!(state.had_deadlock_in_session, Some(1));
    assert!(state.active_session_id.is_some(), "Session verknüpft");
    assert_eq!(state.last_game.as_deref(), Some("Deadlock"));

    assert_eq!(hooks.went_live.load(Ordering::SeqCst), 1, "Go-Live-Hook");
    {
        let reports = hooks.reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].score_refreshes.len(), 1);
        assert_eq!(reports[0].score_refreshes[0].trigger, "poll_stream_online");
    }

    // Stats-Kadenz (log_every_n = 1): tracked-Stats + Session-Sample.
    let stats: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_stats_tracked")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stats, 1);
    let samples: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_viewers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(samples, 1);

    // Tick 2: drag ist offline → Session finalisiert, Refresh offline.
    source.set_streams(vec![]);
    engine.tick().await;

    let state: StateRow = sqlx::query_as(
        "SELECT is_live, had_deadlock_in_session, active_session_id, last_game
           FROM twitch_live_state WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state.is_live, Some(0));
    assert_eq!(state.had_deadlock_in_session, Some(0), "offline → false");
    assert_eq!(state.active_session_id, None);

    let (ended, notes): (Option<chrono::DateTime<Utc>>, Option<String>) =
        sqlx::query_as("SELECT ended_at, notes FROM twitch_stream_sessions LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ended.is_some(), "Session abgeschlossen");
    assert_eq!(notes.as_deref(), Some("offline"));

    {
        let reports = hooks.reports.lock().unwrap();
        assert_eq!(reports[1].score_refreshes.len(), 1);
        assert_eq!(reports[1].score_refreshes[0].trigger, "poll_stream_offline");
    }
    assert_eq!(
        hooks.went_live.load(Ordering::SeqCst),
        1,
        "kein zweiter Go-Live"
    );
}

#[tokio::test]
async fn tick_stream_restart_erzeugt_restart_refresh() {
    let pool = pool_or_skip!("t4c_restart");
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state
            (twitch_login, twitch_user_id, is_partner_active, is_partner)
         VALUES ('drag', '42', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let source = Arc::new(StubSource::new());
    let hooks = Arc::new(RecordingHooks::new());
    let engine = engine_with(&pool, source.clone(), hooks.clone());

    source.set_streams(vec![live_stream("drag", "42", "s-1", 10)]);
    engine.tick().await;
    // Neuer Broadcast (andere stream_id) → alte Session zu, Restart-Refresh.
    source.set_streams(vec![live_stream("drag", "42", "s-2", 12)]);
    engine.tick().await;

    let restarted: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_stream_sessions WHERE notes = 'restarted'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(restarted, 1);
    let open: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_stream_sessions WHERE ended_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(open, 1);

    let reports = hooks.reports.lock().unwrap();
    let triggers: Vec<&str> = reports[1]
        .score_refreshes
        .iter()
        .map(|r| r.trigger)
        .collect();
    assert!(triggers.contains(&"poll_stream_restarted"), "{triggers:?}");
}

#[tokio::test]
async fn poll_intervall_aus_settings_mit_clamp() {
    let pool = pool_or_skip!("t4c_interval");
    let store = PollIntervalStore::new(pool.clone());
    assert_eq!(store.current_seconds().await, 15, "Default ohne Eintrag");

    sqlx::query(
        "INSERT INTO twitch_global_settings (setting_key, setting_value)
         VALUES ('poll_interval_seconds', '60')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(store.current_seconds().await, 60);

    sqlx::query(
        "UPDATE twitch_global_settings SET setting_value = '9999'
          WHERE setting_key = 'poll_interval_seconds'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(store.current_seconds().await, 15, "ungültig → Default");
}

#[tokio::test]
async fn monitored_only_kanal_wird_getrackt_aber_nicht_als_partner() {
    let pool = pool_or_skip!("t4c_monitored_only");
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only)
         VALUES ('lurker', '77', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let tracked_store = TrackedStore::new(pool.clone());
    let (tracked, partner_logins) = tracked_store.load().await.unwrap();
    assert_eq!(tracked.len(), 1);
    assert_eq!(tracked[0].login, "lurker");
    assert!(!tracked[0].is_verified);
    assert!(partner_logins.is_empty());

    // Live-State wird trotzdem geschrieben (Monitoring-only-Tracking).
    let source = Arc::new(StubSource::new());
    let hooks = Arc::new(RecordingHooks::new());
    let engine = engine_with(&pool, source.clone(), hooks.clone());
    source.set_streams(vec![live_stream("lurker", "77", "s-9", 3)]);
    engine.tick().await;

    let is_live: Option<i32> =
        sqlx::query_scalar("SELECT is_live FROM twitch_live_state WHERE twitch_user_id = '77'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(is_live, Some(1));
    // Kein Go-Live-Hook, keine Score-Refreshes (nicht verifiziert).
    assert_eq!(hooks.went_live.load(Ordering::SeqCst), 0);
    assert!(hooks.reports.lock().unwrap()[0].score_refreshes.is_empty());
}

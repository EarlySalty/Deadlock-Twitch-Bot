//! Hermetische Tests für den Write-Core (Slice 4b) gegen den
//! Wegwerf-Container (`TB_TEST_DATABASE_URL`). Schema pro Test, DDL nach dem
//! **prod-verifizierten** Stand (2026-06-22): Sessions mit timestamptz/boolean/
//! bigint, Live-State mit TEXT-Timestamps, exp_* mit TEXT-Timestamps + REAL.

use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use sqlx::PgPool;
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::{
    ExpSessionStore, ExpSessionTracker, FollowerCountSource, FollowerFetch, LiveStateStore,
    LiveStateUpsert, NewSession, NoFollowerSource, SessionTracker, StartOutcome, StatsSample,
    StatsStore, StreamSnapshot, TrackedStreamer,
};

mod support;

/// Schema-Pool oder lauter Skip (DDL + Isolation siehe `tests/support/mod.rs`).
macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
}

/// Follower-Stub: liefert die Werte der Reihe nach (ensure → start, finalize → end).
struct SeqFollowers {
    values: Mutex<Vec<Option<i32>>>,
}

#[async_trait::async_trait]
impl FollowerCountSource for SeqFollowers {
    async fn follower_total(&self, _user_id: Option<&str>, _login: &str) -> FollowerFetch {
        let mut values = self.values.lock().unwrap();
        let total = if values.is_empty() {
            None
        } else {
            values.remove(0)
        };
        FollowerFetch {
            total,
            http_status: total.map(|_| 200),
            error_code: None,
        }
    }
}

fn tracker_with(pool: &PgPool, followers: Arc<dyn FollowerCountSource>) -> SessionTracker {
    SessionTracker::new(
        SessionStore::new(pool.clone()),
        LiveStateStore::new(pool.clone()),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        followers,
        "Deadlock",
    )
}

fn deadlock_stream(stream_id: &str, login: &str, viewers: i32) -> StreamSnapshot {
    StreamSnapshot {
        id: Some(stream_id.to_string()),
        user_login: login.to_string(),
        user_id: "0".to_string(),
        user_name: login.to_uppercase(),
        title: "Ranked Grind".to_string(),
        game_name: "Deadlock".to_string(),
        language: "de".to_string(),
        viewer_count: viewers,
        is_mature: false,
        tags: vec!["DE".to_string()],
        started_at: None,
        thumbnail_url: None,
        profile_image_url: None,
    }
}

// ── Live-State ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn live_state_upsert_drift_cleanup_und_snapshot() {
    let pool = pool_or_skip!("t4b_live_state");
    let store = LiveStateStore::new(pool.clone());

    let row = |user_id: &str, login: &str, live: i32| LiveStateUpsert {
        twitch_user_id: user_id.to_string(),
        streamer_login: login.to_string(),
        is_live: live,
        last_seen_at: "2026-06-09T18:00:00+00:00".to_string(),
        last_title: Some("Titel".to_string()),
        last_game: Some("Deadlock".to_string()),
        last_viewer_count: 12,
        last_discord_message_id: None,
        last_tracking_token: None,
        last_stream_id: Some("s1".to_string()),
        last_started_at: Some("2026-06-09T17:00:00+00:00".to_string()),
        had_deadlock_in_session: 1,
        active_session_id: None,
        last_deadlock_seen_at: None,
    };

    store.persist(&[row("111", "drag", 1)]).await.unwrap();
    // user_id-Drift: gleicher Login unter neuer ID → alte Row verschwindet.
    store.persist(&[row("222", "drag", 1)]).await.unwrap();
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT twitch_user_id FROM twitch_live_state ORDER BY twitch_user_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ids, vec!["222".to_string()]);

    // Leere user_id wird übersprungen, gültige Rows daneben geschrieben.
    store
        .persist(&[row("", "kaputt", 1), row("333", "zwei", 0)])
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_live_state")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    // Snapshot: Partner-Flag über LATERAL, Fallback für Logins ohne Row.
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
         VALUES ('222', 'drag', 'active', 1), ('999', 'neu', 'active', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let tracked = vec![
        TrackedStreamer {
            login: "Drag".to_string(),
            twitch_user_id: Some("222".to_string()),
        },
        TrackedStreamer {
            login: "neu".to_string(),
            twitch_user_id: Some("999".to_string()),
        },
        TrackedStreamer {
            login: "unbekannt".to_string(),
            twitch_user_id: None,
        },
    ];
    let snapshot = store.load_snapshot(&tracked).await.unwrap();
    assert_eq!(snapshot.len(), 2);
    let drag = &snapshot["drag"];
    assert_eq!(drag.partner_raid_bot_enabled, 1);
    assert!(drag.state.is_some());
    let neu = &snapshot["neu"];
    assert!(neu.state.is_none(), "Fallback-Eintrag ohne Live-State-Row");
    assert_eq!(neu.twitch_user_id.as_deref(), Some("999"));
}

#[tokio::test]
async fn live_state_stale_sweep_setzt_alte_live_rows_offline() {
    let pool = pool_or_skip!("t4b_live_state_stale_sweep");
    let store = LiveStateStore::new(pool.clone());
    let stale_seen = (Utc::now() - Duration::minutes(45)).to_rfc3339();
    let fresh_seen = (Utc::now() - Duration::minutes(5)).to_rfc3339();

    sqlx::query(
        "INSERT INTO twitch_live_state
            (twitch_user_id, streamer_login, is_live, last_seen_at, active_session_id)
         VALUES ('old', 'oldlogin', 1, $1, 11),
                ('fresh', 'freshlogin', 1, $2, 22)",
    )
    .bind(stale_seen)
    .bind(fresh_seen)
    .execute(&pool)
    .await
    .unwrap();

    let healed = store.sweep_stale_live(30 * 60).await.unwrap();
    assert_eq!(healed, 1);

    let rows: Vec<(String, i32, Option<i64>)> = sqlx::query_as(
        "SELECT twitch_user_id, is_live, active_session_id
           FROM twitch_live_state
          ORDER BY twitch_user_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("fresh".to_string(), 1, Some(22)),
            ("old".to_string(), 0, None),
        ]
    );
}

// ── Session-Lifecycle ─────────────────────────────────────────────────────────

#[tokio::test]
async fn session_start_schreibt_boolean_session_flags() {
    let pool = pool_or_skip!("t4b_session_bool_flags");
    let store = SessionStore::new(pool.clone());

    let new = NewSession {
        streamer_login: "drag".to_string(),
        stream_id: Some("s-bool".to_string()),
        started_at: Utc::now(),
        viewer_count: 7,
        followers_start: None,
        title: "Ranked".to_string(),
        language: "de".to_string(),
        is_mature: true,
        tags: String::new(),
        game_name: Some("Deadlock".to_string()),
        had_deadlock: true,
    };

    let outcome = store
        .start_session(&new)
        .await
        .expect("start_session muss gegen BOOLEAN-Session-Flags schreiben");

    let flags: (bool, bool) = sqlx::query_as(
        "SELECT is_mature, had_deadlock_in_session
           FROM twitch_stream_sessions WHERE id = $1",
    )
    .bind(outcome.session_id())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(flags, (true, true));
}

#[tokio::test]
async fn session_lifecycle_start_sample_finalize() {
    let pool = pool_or_skip!("t4b_lifecycle");
    let followers = Arc::new(SeqFollowers {
        values: Mutex::new(vec![Some(10), Some(25)]),
    });
    let tracker = tracker_with(&pool, followers);

    // Live-State-Row, damit start_session active_session_id setzen kann.
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_game, had_deadlock_in_session)
         VALUES ('42', 'drag', 'Deadlock', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let started = Utc::now() - Duration::minutes(10);
    let mut stream = deadlock_stream("s-100", "drag", 20);
    stream.started_at = Some(started.to_rfc3339());

    let session_id = tracker
        .ensure_session("drag", &stream, None, Some("42"), Utc::now())
        .await
        .expect("session angelegt");

    let active: Option<i64> = sqlx::query_scalar(
        "SELECT active_session_id FROM twitch_live_state WHERE streamer_login = 'drag'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active, Some(session_id));

    // Zwei Samples: 20 → 30 Zuschauer.
    tracker.record_sample("drag", &stream, Utc::now()).await;
    stream.viewer_count = 30;
    tracker
        .record_sample("drag", &stream, Utc::now() + Duration::seconds(1))
        .await;

    // Chatters: 3 unique, 2 first-time → returning = 1 (Fix für SUM(boolean)).
    for (chatter, first) in [("a", true), ("b", true), ("c", false)] {
        sqlx::query(
            "INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login, is_first_time_streamer)
             VALUES ($1, 'drag', $2, $3)",
        )
        .bind(session_id)
        .bind(chatter)
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    }

    let ended_at = Utc::now() + Duration::seconds(2);
    assert!(tracker.finalize("drag", "done", None, Some(ended_at)).await);

    #[derive(sqlx::FromRow)]
    struct FinalizedRow {
        // P2.38: ended_at ist TEXT — als ::text lesen.
        ended_at: Option<String>,
        end_viewers: i32,
        peak_viewers: i32,
        avg_viewers: f64,
        samples: i32,
        unique_chatters: i32,
        first_time_chatters: i32,
        follower_delta: Option<i32>,
        notes: String,
        had_deadlock_in_session: bool,
    }
    let row: FinalizedRow = sqlx::query_as(
        "SELECT ended_at::text AS ended_at, end_viewers, peak_viewers, avg_viewers, samples,
                unique_chatters, first_time_chatters, follower_delta, notes,
                had_deadlock_in_session
           FROM twitch_stream_sessions WHERE id = $1",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(row.ended_at.is_some(), "ended_at gesetzt");
    assert_eq!(row.end_viewers, 30, "end_viewers vom letzten Sample");
    assert_eq!(row.peak_viewers, 30, "peak");
    assert!((row.avg_viewers - 25.0).abs() < 1e-9, "avg über Samples");
    assert_eq!(row.samples, 2, "samples");
    assert_eq!(
        (row.unique_chatters, row.first_time_chatters),
        (3, 2),
        "Chatter-Zählung via FILTER"
    );
    assert_eq!(row.follower_delta, Some(15), "follower_delta 25-10");
    assert_eq!(row.notes, "done");
    assert!(
        row.had_deadlock_in_session,
        "had_deadlock aus Live-State/Game"
    );

    let active: Option<i64> = sqlx::query_scalar(
        "SELECT active_session_id FROM twitch_live_state WHERE streamer_login = 'drag'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active, None, "Live-State-Verknüpfung gelöst");

    // exp-Spiegel: Session angelegt + finalisiert.
    let exp: (String, Option<String>, i32) = sqlx::query_as(
        "SELECT streamer, ended_at, samples FROM exp_sessions WHERE stream_id = 's-100'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(exp.0, "drag");
    assert!(exp.1.is_some(), "exp ended_at gesetzt");
    assert_eq!(exp.2, 2, "exp Samples mitgeführt");
}

#[tokio::test]
async fn adoptierte_session_reaktiviert_exp_snapshots() {
    let pool = pool_or_skip!("t4b_adopt_exp");
    let started = Utc::now() - Duration::minutes(20);
    sqlx::query(
        "INSERT INTO twitch_stream_sessions
            (streamer_login, stream_id, started_at, start_viewers, game_name)
         VALUES ('drag', 's-adopt', $1, 12, NULL)",
    )
    .bind(started.to_rfc3339())
    .execute(&pool)
    .await
    .unwrap();

    let tracker = tracker_with(&pool, Arc::new(NoFollowerSource));
    let mut stream = deadlock_stream("s-adopt", "drag", 12);
    stream.started_at = Some(started.to_rfc3339());

    tracker
        .ensure_session("drag", &stream, None, None, Utc::now())
        .await
        .expect("offene Session adoptiert");
    tracker.record_sample("drag", &stream, Utc::now()).await;

    let (samples, avg_viewers, peak_viewers): (i32, f32, i32) = sqlx::query_as(
        "SELECT samples, avg_viewers, peak_viewers
           FROM exp_sessions WHERE stream_id = 's-adopt'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let snapshots: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM exp_snapshots sn
           JOIN exp_sessions es ON es.id = sn.exp_session_id
          WHERE es.stream_id = 's-adopt'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(snapshots, 1, "Adopt-Pfad schreibt wieder Snapshots");
    assert_eq!(samples, 1);
    assert_eq!(avg_viewers, 12.0);
    assert_eq!(peak_viewers, 12);
}

#[tokio::test]
async fn session_doppel_start_wird_db_seitig_verhindert() {
    let pool = pool_or_skip!("t4b_double_start");
    let store = SessionStore::new(pool.clone());

    let new = NewSession {
        streamer_login: "drag".to_string(),
        stream_id: Some("s-1".to_string()),
        started_at: Utc::now(),
        viewer_count: 5,
        followers_start: None,
        title: String::new(),
        language: String::new(),
        is_mature: false,
        tags: String::new(),
        game_name: None,
        had_deadlock: false,
    };
    let first = store.start_session(&new).await.unwrap();
    let second = store.start_session(&new).await.unwrap();
    assert!(matches!(first, StartOutcome::Created(_)));
    assert_eq!(second, StartOutcome::AlreadyOpen(first.session_id()));
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_stream_sessions WHERE streamer_login = 'drag' AND ended_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open, 1, "kein Doppel-Insert trotz fehlendem Cache");
}

#[tokio::test]
async fn session_restart_finalisiert_alte_session() {
    let pool = pool_or_skip!("t4b_restart");
    let tracker = tracker_with(&pool, Arc::new(NoFollowerSource));

    let first = tracker
        .ensure_session(
            "drag",
            &deadlock_stream("s-1", "drag", 5),
            None,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let second = tracker
        .ensure_session(
            "drag",
            &deadlock_stream("s-2", "drag", 7),
            None,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    assert_ne!(first, second, "neue Session nach Stream-Neustart");

    let (old_ended, old_notes): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT ended_at::text, notes FROM twitch_stream_sessions WHERE id = $1")
            .bind(first)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(old_ended.is_some());
    assert_eq!(old_notes.as_deref(), Some("restarted"));
}

#[tokio::test]
async fn orphan_cleanup_schliesst_scout_und_stale_sessions() {
    let pool = pool_or_skip!("t4b_orphans");
    let tracker = tracker_with(&pool, Arc::new(NoFollowerSource));

    // Scout-Session: 0 Samples, > 24 h offen.
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, samples)
         VALUES ('alt', (NOW() - INTERVAL '25 hours')::text, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Stale Session: Samples vorhanden, letzter Viewer-Eintrag 2 h alt.
    let stale_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, samples)
         VALUES ('stale', (NOW() - INTERVAL '5 hours')::text, 3) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_session_viewers (session_id, ts_utc, minutes_from_start, viewer_count)
         VALUES ($1, NOW() - INTERVAL '2 hours', 180, 9)",
    )
    .bind(stale_id)
    .execute(&pool)
    .await
    .unwrap();
    // Frische offene Session bleibt unangetastet.
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, samples)
         VALUES ('frisch', (NOW() - INTERVAL '10 minutes')::text, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(tracker.cleanup_orphans().await, 2);
    let still_open: Vec<String> = sqlx::query_scalar(
        "SELECT streamer_login FROM twitch_stream_sessions WHERE ended_at IS NULL",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(still_open, vec!["frisch".to_string()]);
    let stale_notes: Option<String> =
        sqlx::query_scalar("SELECT notes FROM twitch_stream_sessions WHERE id = $1")
            .bind(stale_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(stale_notes.unwrap().contains("stale session"));
}

// ── Stats ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn stats_batch_inserts() {
    let pool = pool_or_skip!("t4b_stats");
    let store = StatsStore::new(pool.clone());
    let ts = Utc::now();
    let sample = |login: &str, partner: bool| StatsSample {
        streamer: login.to_string(),
        viewer_count: 7,
        is_partner: partner,
        game_name: Some("Deadlock".to_string()),
        stream_title: None,
        tags: Some(r#"["DE"]"#.to_string()),
        language: Some("de".to_string()),
    };
    store
        .log_tracked(ts, &[sample("a", true), sample("b", false)])
        .await
        .unwrap();
    store.log_category(ts, &[sample("c", false)]).await.unwrap();

    let tracked: Vec<(String, bool)> =
        sqlx::query_as("SELECT streamer, is_partner FROM twitch_stats_tracked ORDER BY streamer")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        tracked,
        vec![("a".to_string(), true), ("b".to_string(), false)]
    );
    let category: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_stats_category")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(category, 1);
}

// ── exp-Hooks ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn exp_hooks_idempotent_und_vollstaendig() {
    let pool = pool_or_skip!("t4b_exp");
    let exp = ExpSessionTracker::new(ExpSessionStore::new(pool.clone()));
    let stream = deadlock_stream("s-9", "drag", 11);
    let t0 = Utc::now() - Duration::minutes(3);

    exp.on_session_start("drag", &stream, t0).await;
    exp.on_session_start("drag", &stream, t0).await; // idempotent über stream_id
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exp_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    exp.on_session_sample("drag", &stream, Utc::now()).await;
    let (samples, peak): (i32, i32) =
        sqlx::query_as("SELECT samples, peak_viewers FROM exp_sessions WHERE stream_id = 's-9'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((samples, peak), (1, 11));

    exp.on_game_transition("drag", "Deadlock", "Just Chatting", 8, Utc::now())
        .await;
    let transitions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exp_game_transitions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(transitions, 1);

    exp.on_session_finalize("drag", Some(4), Utc::now()).await;
    let (ended_at, delta, duration): (Option<String>, Option<i32>, Option<f32>) = sqlx::query_as(
        "SELECT ended_at, follower_delta, duration_min FROM exp_sessions WHERE stream_id = 's-9'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ended_at.is_some());
    assert_eq!(delta, Some(4));
    assert!(duration.unwrap() >= 3.0, "duration_min aus started_at");
}

#[tokio::test]
async fn offline_source_state_liest_restzustand_nach_offline() {
    let pool = pool_or_skip!("t6_live_remnant");
    let store = LiveStateStore::new(pool.clone());
    sqlx::query(
        "INSERT INTO twitch_live_state
            (twitch_user_id, streamer_login, is_live, last_game, last_viewer_count,
             last_started_at, had_deadlock_in_session, last_deadlock_seen_at)
         VALUES ('42', 'drag', 1, 'Deadlock', 33,
                 '2026-06-10T17:00:00+00:00', 1, '2026-06-10T18:30:00+00:00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Offline setzen — die Session-Restfelder bleiben bewusst stehen.
    store
        .apply_stream_offline("42", "2026-06-10T19:00:00+00:00")
        .await
        .unwrap();

    let state = store.offline_source_state("42").await.unwrap().unwrap();
    assert_eq!(state.last_game.as_deref(), Some("Deadlock"));
    assert_eq!(state.last_viewer_count, Some(33));
    assert_eq!(
        state.last_started_at.as_deref(),
        Some("2026-06-10T17:00:00+00:00")
    );
    assert_eq!(state.had_deadlock_in_session, Some(1));
    assert_eq!(
        state.last_deadlock_seen_at.as_deref(),
        Some("2026-06-10T18:30:00+00:00")
    );

    assert!(store.offline_source_state("99").await.unwrap().is_none());
}

#[tokio::test]
async fn source_states_by_logins_liefert_login_map() {
    let pool = pool_or_skip!("t6_live_loginmap");
    let store = LiveStateStore::new(pool.clone());
    sqlx::query(
        "INSERT INTO twitch_live_state
            (twitch_user_id, streamer_login, last_game, had_deadlock_in_session, last_deadlock_seen_at)
         VALUES ('1', 'alpha', 'Deadlock', 1, '2026-06-10T18:00:00+00:00'),
                ('2', 'beta', 'Just Chatting', 0, NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let map = store
        .source_states_by_logins(&["alpha".to_string(), "beta".to_string(), "fehlt".to_string()])
        .await
        .unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!(map["alpha"].had_deadlock_in_session, Some(1));
    assert_eq!(map["beta"].last_game.as_deref(), Some("Just Chatting"));
    assert!(!map.contains_key("fehlt"));

    assert!(store.source_states_by_logins(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn first_message_setzt_confirmed_first_ever_auf_session_chatter() {
    let pool = pool_or_skip!("t4b_first_msg");
    let store = tb_monitoring::TelemetryStore::new(pool.clone());

    // Offene Session + ein Session-Chatter (noch nicht confirmed).
    let session_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at)
         VALUES ('drag', NOW()::text) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login)
         VALUES ($1, 'drag', 'viewer1')",
    )
    .bind(session_id)
    .execute(&pool)
    .await
    .unwrap();

    // first_message-Event (Login wird klein geschrieben → matcht 'viewer1').
    let event = serde_json::json!({
        "chatter_user_login": "Viewer1",
        "chatter_user_id": "42",
        "message_id": "m1",
        "message": {"text": "hallo"}
    });
    store
        .store_first_message_event("bid", "drag", &event, Utc::now())
        .await
        .expect("first_message gespeichert");

    let cnt: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_first_message_events WHERE chatter_login = 'viewer1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cnt, 1, "first_message_events-Zeile erwartet");

    let confirmed: bool = sqlx::query_scalar(
        "SELECT confirmed_first_ever FROM twitch_session_chatters
          WHERE session_id = $1 AND chatter_login = 'viewer1'",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        confirmed,
        "confirmed_first_ever muss nach first_message TRUE sein"
    );

    // Ohne offene Session: kein Update, aber auch kein Fehler (Subquery → NULL).
    sqlx::query("UPDATE twitch_stream_sessions SET ended_at = NOW()::text WHERE id = $1")
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();
    let event2 = serde_json::json!({
        "chatter_user_login": "viewer2", "chatter_user_id": "43",
        "message_id": "m2", "message": {"text": "hi"}
    });
    store
        .store_first_message_event("bid", "drag", &event2, Utc::now())
        .await
        .expect("first_message ohne offene Session darf nicht fehlschlagen");
}

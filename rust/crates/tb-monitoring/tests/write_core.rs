//! Hermetische Tests für den Write-Core (Slice 4b) gegen den
//! Wegwerf-Container (`TB_TEST_DATABASE_URL`). Schema pro Test, DDL nach dem
//! **prod-verifizierten** Stand (2026-06-09): Sessions mit timestamptz/boolean/
//! bigint, Live-State mit TEXT-Timestamps, exp_* mit TEXT-Timestamps + REAL.

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::{
    ExpSessionStore, ExpSessionTracker, FollowerCountSource, LiveStateStore, LiveStateUpsert,
    NewSession, NoFollowerSource, SessionTracker, StartOutcome, StatsSample, StatsStore,
    StreamSnapshot, TrackedStreamer,
};

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

macro_rules! skip_without_db {
    () => {
        match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!(
                    "SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`"
                );
                return;
            }
        }
    };
}

async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .expect("admin connect");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(dsn)
        .expect("dsn parse")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect");

    for ddl in [
        "CREATE TABLE twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            last_stream_id TEXT, last_started_at TEXT, last_title TEXT, last_game_id TEXT,
            last_discord_message_id TEXT, last_notified_at TEXT,
            is_live INTEGER DEFAULT 0, last_seen_at TEXT, last_game TEXT,
            last_viewer_count INTEGER DEFAULT 0, last_tracking_token TEXT,
            active_session_id BIGINT, had_deadlock_in_session INTEGER DEFAULT 0,
            last_deadlock_seen_at TEXT
        )",
        "CREATE TABLE twitch_partners (
            id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL, status TEXT NOT NULL,
            raid_bot_enabled INTEGER DEFAULT 0
        )",
        "CREATE TABLE twitch_stream_sessions (
            id BIGSERIAL PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            stream_id TEXT,
            started_at TIMESTAMPTZ NOT NULL,
            ended_at TIMESTAMPTZ,
            duration_seconds INTEGER DEFAULT 0,
            start_viewers INTEGER DEFAULT 0, peak_viewers INTEGER DEFAULT 0,
            end_viewers INTEGER DEFAULT 0,
            avg_viewers DOUBLE PRECISION DEFAULT 0, samples INTEGER DEFAULT 0,
            retention_5m DOUBLE PRECISION, retention_10m DOUBLE PRECISION,
            retention_20m DOUBLE PRECISION,
            dropoff_pct DOUBLE PRECISION, dropoff_label TEXT,
            unique_chatters INTEGER DEFAULT 0, first_time_chatters INTEGER DEFAULT 0,
            returning_chatters INTEGER DEFAULT 0,
            followers_start INTEGER, followers_end INTEGER, follower_delta INTEGER,
            stream_title TEXT, notification_text TEXT, language TEXT,
            is_mature BOOLEAN DEFAULT FALSE, tags TEXT,
            had_deadlock_in_session BOOLEAN DEFAULT FALSE,
            game_name TEXT, notes TEXT
        )",
        "CREATE TABLE twitch_session_viewers (
            session_id BIGINT NOT NULL, ts_utc TIMESTAMPTZ NOT NULL,
            minutes_from_start INTEGER, viewer_count INTEGER NOT NULL,
            PRIMARY KEY (session_id, ts_utc)
        )",
        "CREATE TABLE twitch_session_chatters (
            session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL,
            chatter_login TEXT NOT NULL, is_first_time_streamer BOOLEAN DEFAULT FALSE
        )",
        "CREATE TABLE twitch_stats_tracked (
            ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
            is_partner BOOLEAN, game_name TEXT, stream_title TEXT, tags TEXT
        )",
        "CREATE TABLE twitch_stats_category (
            ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
            is_partner BOOLEAN, game_name TEXT, stream_title TEXT, tags TEXT
        )",
        "CREATE TABLE exp_sessions (
            id BIGSERIAL PRIMARY KEY, streamer TEXT NOT NULL, stream_id TEXT,
            started_at TEXT NOT NULL, ended_at TEXT, game_name TEXT, stream_title TEXT,
            peak_viewers INTEGER DEFAULT 0, avg_viewers REAL DEFAULT 0,
            samples INTEGER DEFAULT 0, follower_delta INTEGER, duration_min REAL
        )",
        "CREATE UNIQUE INDEX idx_exp_sessions_stream_id ON exp_sessions(stream_id)
            WHERE stream_id IS NOT NULL",
        "CREATE TABLE exp_snapshots (
            id BIGSERIAL PRIMARY KEY, exp_session_id BIGINT NOT NULL, ts_utc TEXT NOT NULL,
            viewer_count INTEGER, minutes_from_start REAL
        )",
        "CREATE UNIQUE INDEX idx_exp_snapshots_session_ts
            ON exp_snapshots(exp_session_id, ts_utc)",
        "CREATE TABLE exp_game_transitions (
            id BIGSERIAL PRIMARY KEY, exp_session_id BIGINT NOT NULL, streamer TEXT NOT NULL,
            ts_utc TEXT NOT NULL, from_game TEXT, to_game TEXT, viewer_count INTEGER
        )",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    pool
}

/// Follower-Stub: liefert die Werte der Reihe nach (ensure → start, finalize → end).
struct SeqFollowers {
    values: Mutex<Vec<Option<i32>>>,
}

#[async_trait::async_trait]
impl FollowerCountSource for SeqFollowers {
    async fn follower_total(&self, _user_id: Option<&str>, _login: &str) -> Option<i32> {
        let mut values = self.values.lock().unwrap();
        if values.is_empty() {
            None
        } else {
            values.remove(0)
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
        title: "Ranked Grind".to_string(),
        game_name: "Deadlock".to_string(),
        language: "de".to_string(),
        viewer_count: viewers,
        is_mature: false,
        tags: vec!["DE".to_string()],
        started_at: None,
    }
}

// ── Live-State ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn live_state_upsert_drift_cleanup_und_snapshot() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4b_live_state").await;
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

// ── Session-Lifecycle ─────────────────────────────────────────────────────────

#[tokio::test]
async fn session_lifecycle_start_sample_finalize() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4b_lifecycle").await;
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
        ended_at: Option<chrono::DateTime<Utc>>,
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
        "SELECT ended_at, end_viewers, peak_viewers, avg_viewers, samples,
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
async fn session_doppel_start_wird_db_seitig_verhindert() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4b_double_start").await;
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
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4b_restart").await;
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

    let (old_ended, old_notes): (Option<chrono::DateTime<Utc>>, Option<String>) =
        sqlx::query_as("SELECT ended_at, notes FROM twitch_stream_sessions WHERE id = $1")
            .bind(first)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(old_ended.is_some());
    assert_eq!(old_notes.as_deref(), Some("restarted"));
}

#[tokio::test]
async fn orphan_cleanup_schliesst_scout_und_stale_sessions() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4b_orphans").await;
    let tracker = tracker_with(&pool, Arc::new(NoFollowerSource));

    // Scout-Session: 0 Samples, > 24 h offen.
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, samples)
         VALUES ('alt', NOW() - INTERVAL '25 hours', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Stale Session: Samples vorhanden, letzter Viewer-Eintrag 2 h alt.
    let stale_id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, samples)
         VALUES ('stale', NOW() - INTERVAL '5 hours', 3) RETURNING id",
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
         VALUES ('frisch', NOW() - INTERVAL '10 minutes', 0)",
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
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4b_stats").await;
    let store = StatsStore::new(pool.clone());
    let ts = Utc::now();
    let sample = |login: &str, partner: bool| StatsSample {
        streamer: login.to_string(),
        viewer_count: 7,
        is_partner: partner,
        game_name: Some("Deadlock".to_string()),
        stream_title: None,
        tags: Some(r#"["DE"]"#.to_string()),
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
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4b_exp").await;
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

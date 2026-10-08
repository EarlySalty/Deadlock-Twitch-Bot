mod metadata;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{Connection, PgConnection, PgPool};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tb_analytics::category;
use tb_monitoring::anonymous_chat::{command_channel, AnonymousChat, ReadEvent, ReadStats};
use tb_transport_twitch::HelixClient;
use tokio::{
    sync::{mpsc, watch, Mutex, RwLock},
    task::JoinSet,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Roster = Arc<RwLock<HashMap<String, (String, String)>>>;
#[derive(Default)]
struct Counters {
    stored: AtomicU64,
    invalid: AtomicU64,
    storage_dropped: AtomicU64,
    raw_paused: AtomicBool,
    budget_paused: AtomicBool,
    disk_paused: AtomicBool,
    details: Mutex<Value>,
}

pub async fn run(pool: PgPool, helix: Arc<HelixClient>, mut stop: watch::Receiver<bool>) {
    let started_at = Utc::now();
    let identity = json!({"runtime":"tb-bot","process_id":std::process::id(),
        "process_started_at":started_at,"lease_id":format!("{}-{}",std::process::id(),started_at.timestamp_micros())});
    let counters = Arc::new(Counters::default());
    while !*stop.borrow() {
        let outcome = tokio::select! {
            biased;
            _ = stopped(&mut stop) => return,
            result = acquire_lease(&pool, &identity) => result,
        };
        match outcome {
            Ok(mut leader) => {
                tracing::info!(
                    runtime = "tb-bot",
                    "category native database lease acquired"
                );
                if let Err(error) = run_active(
                    &pool,
                    helix.clone(),
                    &mut leader,
                    stop.clone(),
                    &identity,
                    counters.clone(),
                )
                .await
                {
                    tracing::error!(%error, runtime = "tb-bot", "category runtime stopped; retry pending");
                }
                if leader.ping().await.is_ok() {
                    if let Err(error) = sqlx::query("UPDATE category_collector_status SET details=details || '{\"native_lease_active\":false,\"lease_state\":\"inactive\"}'::jsonb WHERE singleton AND details->>'lease_id'=$1")
                        .bind(identity["lease_id"].as_str()).execute(&mut leader).await {
                        tracing::warn!(%error, "category lease status shutdown failed");
                    }
                    if let Err(error) = publish_presence(&mut leader, &identity, "inactive").await {
                        tracing::warn!(%error, "category native presence shutdown failed");
                    }
                }
                if let Err(error) = leader.close().await {
                    tracing::warn!(%error, "category database lease close failed");
                }
            }
            Err(error) => tracing::warn!(%error, "category lease unavailable; bot continues"),
        }
        tokio::select! {
            biased;
            _ = stopped(&mut stop) => return,
            _ = tokio::time::sleep(Duration::from_secs(5)) => {},
        }
    }
}

async fn stopped(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow_and_update() || stop.changed().await.is_err() {
            return;
        }
    }
}

async fn acquire_lease(pool: &PgPool, identity: &Value) -> Result<PgConnection, Error> {
    category::collector_config(pool).await?;
    let mut leader = pool.acquire().await?.detach();
    let mut waiting = false;
    loop {
        let elected = try_lease(&mut leader, identity).await?;
        if elected {
            return Ok(leader);
        }
        if !waiting {
            tracing::info!(
                runtime = "tb-bot",
                "category lease held by existing collector; waiting for takeover"
            );
            waiting = true;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn try_lease(leader: &mut PgConnection, identity: &Value) -> Result<bool, Error> {
    let elected: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(782363991806)")
        .fetch_one(&mut *leader)
        .await?;
    publish_presence(leader, identity, if elected { "active" } else { "waiting" }).await?;
    Ok(elected)
}

async fn publish_presence(
    leader: &mut PgConnection,
    identity: &Value,
    state: &str,
) -> Result<(), Error> {
    let mut details = identity.clone();
    details["lease_state"] = json!(state);
    let mut tx = leader.begin().await?;
    sqlx::query("INSERT INTO category_native_processes(process_id,heartbeat_at,details) VALUES($1,now(),$2)
        ON CONFLICT(process_id) DO UPDATE SET heartbeat_at=excluded.heartbeat_at,details=excluded.details")
        .bind(identity["process_id"].as_i64()).bind(&details).execute(&mut *tx).await?;
    if state == "active" {
        sqlx::query("INSERT INTO category_native_runtime(singleton,heartbeat_at,details) VALUES(true,now(),$1)
            ON CONFLICT(singleton) DO UPDATE SET heartbeat_at=excluded.heartbeat_at,details=excluded.details")
            .bind(&details).execute(&mut *tx).await?;
    } else if state == "inactive" {
        sqlx::query(
            "UPDATE category_native_runtime SET heartbeat_at=now(),details=$1
            WHERE singleton AND details->>'lease_id'=$2",
        )
        .bind(&details)
        .bind(identity["lease_id"].as_str())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn monitor_lease(leader: &mut PgConnection, identity: &Value) -> Result<(), Error> {
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        leader.ping().await?;
        publish_presence(leader, identity, "active").await?;
    }
}

async fn run_active(
    pool: &PgPool,
    helix: Arc<HelixClient>,
    leader: &mut PgConnection,
    mut stop: watch::Receiver<bool>,
    identity: &Value,
    counters: Arc<Counters>,
) -> Result<(), Error> {
    let game_id = helix
        .search_category_id("Deadlock")
        .await?
        .ok_or("Deadlock category unavailable")?;
    let roster: Roster = Arc::new(RwLock::new(HashMap::new()));
    let settings = category::collector_config(pool).await?;
    if counters.details.lock().await.is_null() {
        restore_storage_latches(pool, &counters).await?;
    }
    counters.stored.store(0, Ordering::Relaxed);
    counters.invalid.store(0, Ordering::Relaxed);
    counters.storage_dropped.store(0, Ordering::Relaxed);
    *counters.details.lock().await = json!({"game_id":game_id,"mode":"anonymous_read_only","desired_channels":0,
        "poll_seconds":settings.poll_seconds,"retention_days":null,"preserve_raw_data":true,
        "raw_budget_bytes":settings.raw_budget_bytes,"media_enabled":settings.media_enabled,
        "language_detector":category::DETECTOR,"discovery_state":"starting","runtime":"tb-bot",
        "process_id":identity["process_id"],"process_started_at":identity["process_started_at"],"lease_id":identity["lease_id"],
        "collector_started_at":Utc::now(),"counters_scope":"collector_run",
        "native_lease_active":true,"lease_state":"active"});
    check_storage(pool, &counters, 0, true).await?;
    let (chat, events) = AnonymousChat::start(20_000);
    let chat = Arc::new(chat);
    let mut tasks: JoinSet<Result<(), Error>> = JoinSet::new();
    tasks.spawn(discover(
        pool.clone(),
        helix.clone(),
        game_id.clone(),
        chat.clone(),
        roster.clone(),
        counters.clone(),
    ));
    tasks.spawn(maintenance(pool.clone(), counters.clone()));
    tasks.spawn(heartbeat(
        pool.clone(),
        chat.stats.clone(),
        counters.clone(),
    ));
    tasks.spawn(metadata::run(
        pool.clone(),
        helix,
        counters.clone(),
        game_id,
    ));
    let mut writer = tokio::spawn(write_chat(pool.clone(), events, roster, counters));
    tracing::info!(
        runtime = "tb-bot",
        "category collector started: anonymous chat, public Helix reads, no outbound actions"
    );
    let (outcome, writer_finished, lease_lost): (Result<(), Error>, bool, bool) = tokio::select! {
        biased;
        _ = stopped(&mut stop) => (Ok(()), false, false),
        result = tasks.join_next() => (Err(format!("collector worker exited: {result:?}").into()), false, false),
        result = &mut writer => (Err(format!("chat storage worker exited: {result:?}").into()), true, false),
        result = monitor_lease(leader, identity) => (result, false, true),
    };
    tasks.shutdown().await;
    drop(chat);
    if !writer_finished {
        if lease_lost {
            writer.abort();
            let _ = writer.await;
        } else {
            writer.await??;
        }
    }
    outcome
}

async fn discover(
    pool: PgPool,
    helix: Arc<HelixClient>,
    game_id: String,
    chat: Arc<AnonymousChat>,
    roster: Roster,
    counters: Arc<Counters>,
) -> Result<(), Error> {
    let mut timer = tokio::time::interval(Duration::from_secs(60));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_success: Option<tokio::time::Instant> = None;
    loop {
        timer.tick().await;
        let settings = category::collector_config(&pool).await?;
        let seconds = settings.poll_seconds as u64;
        counters.details.lock().await["poll_seconds"] = json!(settings.poll_seconds);
        if timer.period().as_secs() != seconds {
            timer = tokio::time::interval_at(
                tokio::time::Instant::now() + Duration::from_secs(seconds),
                Duration::from_secs(seconds),
            );
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        }
        if !settings.enabled || counters.disk_paused.load(Ordering::Relaxed) {
            roster.write().await.clear();
            chat.set_channels(&[]);
            let mut status = counters.details.lock().await;
            status["discovery_state"] = json!(if settings.enabled {
                "storage_paused"
            } else {
                "disabled"
            });
            status["desired_channels"] = json!(0);
            continue;
        }
        let at = Utc::now();
        let result = tokio::time::timeout(
            Duration::from_secs(seconds.saturating_sub(5)),
            helix.get_all_streams_by_category(&game_id),
        )
        .await;
        match result {
            Ok(Ok(streams)) => {
                category::store_snapshot(&pool, at, &streams, seconds as i32).await?;
                let next: HashMap<_, _> = streams
                    .iter()
                    .filter(|s| tb_monitoring::anonymous_chat::valid_channel(&s.user_login))
                    .map(|s| {
                        (
                            s.user_login.to_ascii_lowercase(),
                            (s.user_id.clone(), s.language.clone()),
                        )
                    })
                    .collect();
                let channels: Vec<_> = next.keys().cloned().collect();
                *roster.write().await = next;
                chat.set_channels(&channels);
                last_success = Some(tokio::time::Instant::now());
                let mut status = counters.details.lock().await;
                status["last_discovery"] = json!(Utc::now());
                status["last_discovery_snapshot_at"] = json!(at);
                status["discovery_state"] = json!("ok");
                status["desired_channels"] = json!(channels.len());
                status["last_discovery_error"] = Value::Null;
                tracing::info!(
                    streams = streams.len(),
                    viewers = streams.iter().map(|s| s.viewer_count).sum::<i64>(),
                    "category snapshot committed"
                );
            }
            failed => {
                let stale = last_success
                    .is_none_or(|last| last.elapsed() > Duration::from_secs(seconds * 2));
                if stale {
                    roster.write().await.clear();
                    chat.set_channels(&[]);
                }
                let mut status = counters.details.lock().await;
                status["discovery_state"] = json!(if stale {
                    "stale_chat_stopped"
                } else {
                    "retrying"
                });
                status["last_discovery_error"] = json!(format!("{failed:?}"));
                if stale {
                    status["desired_channels"] = json!(0);
                }
                tracing::warn!(
                    stale,
                    "category discovery failed; no partial snapshot was stored"
                );
            }
        }
    }
}

async fn write_chat(
    pool: PgPool,
    mut events: mpsc::Receiver<ReadEvent>,
    roster: Roster,
    counters: Arc<Counters>,
) -> Result<(), Error> {
    let mut batch = Vec::with_capacity(500);
    let mut timer = tokio::time::interval(Duration::from_secs(2));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            event=events.recv()=>{
                let Some(event)=event else { flush(&pool,&mut batch,&counters).await?; return Ok(()); };
                let room=roster.read().await.get(&event.channel).cloned();
                let Some((room_id,language))=room else { counters.invalid.fetch_add(1,Ordering::Relaxed); continue; };
                let verb=command_channel(&event.line).map(|(verb,_)|verb).unwrap_or("");
                if matches!(verb,"CLEARMSG"|"CLEARCHAT") {
                    flush(&pool,&mut batch,&counters).await?;
                    category::delete_chat(&pool,&event.line,&room_id,event.received_at).await?;
                } else if counters.raw_paused.load(Ordering::Relaxed) {
                    counters.storage_dropped.fetch_add(1,Ordering::Relaxed);
                } else if let Some(raw)=category::raw_message(&event.line,event.received_at,&room_id,&language) {
                    batch.push(raw);
                    if batch.len()>=500 { flush(&pool,&mut batch,&counters).await?; }
                } else { counters.invalid.fetch_add(1,Ordering::Relaxed); }
            }
            _=timer.tick()=>flush(&pool,&mut batch,&counters).await?,
        }
    }
}
async fn flush(
    pool: &PgPool,
    batch: &mut Vec<category::RawMessage>,
    counters: &Counters,
) -> Result<(), Error> {
    if batch.is_empty() {
        return Ok(());
    }
    let mut attempt = 0;
    loop {
        match category::store_messages(pool, batch).await {
            Ok(count) => {
                counters.stored.fetch_add(count as u64, Ordering::Relaxed);
                batch.clear();
                return Ok(());
            }
            Err(error) if attempt < 3 => {
                attempt += 1;
                tracing::warn!(%error,attempt,"chat batch retry; pending rows retained");
                tokio::time::sleep(Duration::from_secs(attempt)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn maintenance(pool: PgPool, counters: Arc<Counters>) -> Result<(), Error> {
    let mut timer = tokio::time::interval(Duration::from_secs(30));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut iteration = 1u64;
    loop {
        timer.tick().await;
        check_storage(&pool, &counters, iteration, true).await?;
        if !counters.disk_paused.load(Ordering::Relaxed) {
            category::flush_rollups(&pool, 2000).await?;
        }
        iteration += 1;
    }
}

async fn restore_storage_latches(pool: &PgPool, counters: &Counters) -> Result<(), Error> {
    let previous: Option<(bool, bool)> = sqlx::query_as("SELECT COALESCE((details->>'budget_paused')::boolean,FALSE),
        COALESCE((details->>'disk_paused')::boolean,FALSE) FROM category_collector_status WHERE singleton")
        .fetch_optional(pool).await?;
    let (budget, disk) = previous.unwrap_or((false, false));
    counters.budget_paused.store(budget, Ordering::Relaxed);
    counters.disk_paused.store(disk, Ordering::Relaxed);
    Ok(())
}

async fn check_storage(
    pool: &PgPool,
    counters: &Counters,
    iteration: u64,
    hysteresis: bool,
) -> Result<(), Error> {
    let settings = category::collector_config(pool).await?;
    let bytes = category::raw_storage_bytes(pool).await?;
    let free = nix::sys::statvfs::statvfs("/var/lib/postgresql")
        .ok()
        .map(|disk| disk.blocks_available().saturating_mul(disk.fragment_size()));
    let previous = (
        counters.budget_paused.load(Ordering::Relaxed),
        counters.disk_paused.load(Ordering::Relaxed),
    );
    let was_raw_paused = counters.raw_paused.load(Ordering::Relaxed);
    let (raw_paused, disk_paused, budget_paused, warning) =
        storage_state(bytes, &settings, free, previous, hysteresis);
    if partitions_due(previous.1, disk_paused, iteration) {
        sqlx::query("SELECT category_prepare_partitions()")
            .execute(pool)
            .await?;
    }
    counters.raw_paused.store(raw_paused, Ordering::Relaxed);
    counters
        .budget_paused
        .store(budget_paused, Ordering::Relaxed);
    counters.disk_paused.store(disk_paused, Ordering::Relaxed);
    if previous != (budget_paused, disk_paused) || was_raw_paused != raw_paused {
        tracing::info!(
            raw_paused,
            disk_paused,
            "category storage state changed; archive unchanged"
        );
    }
    let mut status = counters.details.lock().await;
    status["raw_bytes"] = json!(bytes);
    status["raw_budget_bytes"] = json!(settings.raw_budget_bytes);
    status["free_disk_bytes"] = json!(free);
    status["min_free_bytes"] = json!(settings.min_free_bytes);
    status["storage_warning"] = json!(warning);
    status["storage_checked_at"] = json!(Utc::now());
    status["raw_paused"] = json!(raw_paused);
    status["budget_paused"] = json!(budget_paused);
    status["disk_paused"] = json!(disk_paused);
    status["retention_days"] = Value::Null;
    status["preserve_raw_data"] = json!(true);
    status["poll_seconds"] = json!(settings.poll_seconds);
    status["media_enabled"] = json!(settings.media_enabled);
    Ok(())
}

fn partitions_due(was_disk_paused: bool, disk_paused: bool, iteration: u64) -> bool {
    !disk_paused && (was_disk_paused || iteration.is_multiple_of(20))
}

fn storage_state(
    bytes: i64,
    settings: &category::CollectorConfig,
    free: Option<u64>,
    previous: (bool, bool),
    hysteresis: bool,
) -> (bool, bool, bool, bool) {
    let reserve = settings.min_free_bytes as u64;
    let resume_reserve = reserve.saturating_add((reserve / 5).max(1024 * 1024 * 1024));
    let disk_paused = free.is_none_or(|available| {
        available
            < if hysteresis && previous.1 {
                resume_reserve
            } else {
                reserve
            }
    });
    let budget_paused = bytes >= settings.raw_budget_bytes
        || (hysteresis
            && previous.0
            && bytes > settings.raw_budget_bytes - settings.raw_budget_bytes / 10);
    let raw_paused = !settings.enabled || disk_paused || budget_paused;
    let warning = disk_paused
        || bytes as f64 >= settings.raw_budget_bytes as f64 * 0.8
        || free.is_some_and(|available| {
            available < (settings.min_free_bytes as u64).saturating_mul(2)
        });
    (raw_paused, disk_paused, budget_paused, warning)
}
async fn heartbeat(
    pool: PgPool,
    stats: Arc<ReadStats>,
    counters: Arc<Counters>,
) -> Result<(), Error> {
    let mut timer = tokio::time::interval(Duration::from_secs(15));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        let mut status = counters.details.lock().await.clone();
        status["connected_shards"] = json!(stats.connected_shards.load(Ordering::Relaxed));
        status["confirmed_channels"] = json!(stats.confirmed_channels.load(Ordering::Relaxed));
        status["received_messages"] = json!(stats.received_messages.load(Ordering::Relaxed));
        status["queue_drops"] = json!(stats.dropped_events.load(Ordering::Relaxed));
        status["irc_reconnects"] = json!(stats.reconnects.load(Ordering::Relaxed));
        status["stored_since_collector_start"] = json!(counters.stored.load(Ordering::Relaxed));
        status["invalid_events"] = json!(counters.invalid.load(Ordering::Relaxed));
        status["storage_drops"] = json!(counters.storage_dropped.load(Ordering::Relaxed));
        status["counters_started_note"] =
            json!("counters reset on collector retry; persistent totals are in category tables");
        let raw: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT min(sent_at) FROM category_chat_messages")
                .fetch_one(&pool)
                .await?;
        status["oldest_raw_message"] = json!(raw);
        sqlx::query("INSERT INTO category_collector_status(singleton,heartbeat_at,details) VALUES(true,now(),$1::jsonb)
            ON CONFLICT(singleton) DO UPDATE SET heartbeat_at=excluded.heartbeat_at,details=excluded.details")
            .bind(status.to_string()).execute(&pool).await?;
    }
}

#[cfg(test)]
#[path = "../../../test-support/database.rs"]
mod test_database;

#[cfg(test)]
mod storage_tests {
    use super::*;

    fn settings() -> category::CollectorConfig {
        category::CollectorConfig {
            enabled: true,
            poll_seconds: 60,
            raw_budget_bytes: 1000,
            min_free_bytes: 100,
            media_enabled: true,
        }
    }

    #[test]
    fn storage_limits_pause_ingestion_without_requesting_deletion() {
        let cfg = settings();
        assert_eq!(
            storage_state(0, &cfg, Some(1000), (false, false), false),
            (false, false, false, false)
        );
        assert_eq!(
            storage_state(800, &cfg, Some(1000), (false, false), false),
            (false, false, false, true)
        );
        assert_eq!(
            storage_state(1000, &cfg, Some(1000), (false, false), false),
            (true, false, true, true)
        );
        assert_eq!(
            storage_state(0, &cfg, Some(99), (false, false), false),
            (true, true, false, true)
        );
        assert_eq!(
            storage_state(0, &cfg, None, (false, false), false),
            (true, true, false, true)
        );
    }

    #[test]
    fn resuming_after_midnight_prepares_partitions_before_the_periodic_tick() {
        assert!(partitions_due(true, false, 1));
        assert!(partitions_due(false, false, 20));
        assert!(!partitions_due(false, false, 1));
        assert!(!partitions_due(true, true, 20));
    }

    #[test]
    fn pauses_resume_only_after_the_recovery_threshold() {
        let mut cfg = settings();
        assert!(storage_state(901, &cfg, Some(10_000_000_000), (true, false), true).0);
        assert!(!storage_state(900, &cfg, Some(10_000_000_000), (true, false), true).0);
        let gib = 1024 * 1024 * 1024;
        cfg.min_free_bytes = 10 * gib;
        assert!(storage_state(0, &cfg, Some(11 * gib as u64), (true, true), true).1);
        assert!(!storage_state(0, &cfg, Some(12 * gib as u64), (true, true), true).1);
        assert!(!storage_state(0, &cfg, Some(11 * gib as u64), (false, false), true).1);
    }

    #[test]
    fn disk_and_disable_do_not_latch_the_budget() {
        let mut cfg = settings();
        let free = Some(10_000_000_000);
        cfg.enabled = false;
        let disabled = storage_state(950, &cfg, free, (false, false), true);
        assert_eq!(disabled, (true, false, false, true));
        cfg.enabled = true;
        assert!(!storage_state(950, &cfg, free, (disabled.2, disabled.1), true).0);
        let disk = storage_state(950, &cfg, Some(0), (false, false), true);
        assert_eq!(disk, (true, true, false, true));
        assert!(!storage_state(950, &cfg, free, (disk.2, disk.1), true).0);
        let budget = storage_state(1000, &cfg, free, (false, false), true);
        assert!(storage_state(950, &cfg, free, (budget.2, budget.1), true).0);
        cfg.enabled = false;
        let disabled_budget = storage_state(950, &cfg, free, (budget.2, budget.1), true);
        cfg.enabled = true;
        assert!(
            storage_state(
                950,
                &cfg,
                free,
                (disabled_budget.2, disabled_budget.1),
                true
            )
            .0
        );
        assert!(
            !storage_state(
                900,
                &cfg,
                free,
                (disabled_budget.2, disabled_budget.1),
                true
            )
            .0
        );
    }

    #[tokio::test]
    async fn waiting_processes_preserve_owner_and_empty_measurements_prove_cutover() {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;
        let dsn =
            test_database::database_url().expect("isolated Postgres test configuration required");
        let options = PgConnectOptions::from_str(&dsn).unwrap();
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        let schema = format!(
            "native_{}_{}",
            std::process::id(),
            Utc::now().timestamp_subsec_nanos()
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.options([("search_path", schema.as_str())]))
            .await
            .unwrap();
        sqlx::raw_sql(
            include_str!("../../../migrations/20260918123000_category_collector.sql")
                .split("CREATE OR REPLACE FUNCTION category_prepare_partitions()")
                .next()
                .unwrap(),
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(
            include_str!("../../../migrations/20261008160000_category_native_bot.sql")
                .split("CREATE OR REPLACE FUNCTION category_prepare_partitions()")
                .next()
                .unwrap(),
        )
        .execute(&pool)
        .await
        .unwrap();
        let first = json!({"runtime":"tb-bot","process_id":101,"lease_id":"101-1000","process_started_at":"2026-10-08T12:00:00Z"});
        let second = json!({"runtime":"tb-bot","process_id":102,"lease_id":"102-2000","process_started_at":"2026-10-08T12:01:00Z"});
        let mut owner = pool.acquire().await.unwrap().detach();
        let mut waiter = pool.acquire().await.unwrap().detach();
        assert!(try_lease(&mut owner, &first).await.unwrap());
        assert!(!try_lease(&mut waiter, &second).await.unwrap());
        let active: Value =
            sqlx::query_scalar("SELECT details FROM category_native_runtime WHERE singleton")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(active["lease_id"], first["lease_id"]);
        let waiting: Value = sqlx::query_scalar(
            "SELECT details FROM category_native_processes WHERE process_id=102",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(waiting["lease_id"], second["lease_id"]);
        assert_eq!(waiting["lease_state"], "waiting");
        publish_presence(&mut owner, &first, "active")
            .await
            .unwrap();
        assert!(!try_lease(&mut waiter, &second).await.unwrap());
        publish_presence(&mut owner, &first, "inactive")
            .await
            .unwrap();
        owner.close().await.unwrap();
        assert!(try_lease(&mut waiter, &second).await.unwrap());
        let at = DateTime::parse_from_rfc3339("2026-10-08T12:02:00Z")
            .unwrap()
            .with_timezone(&Utc);
        category::store_snapshot(&pool, at, &[], 60).await.unwrap();
        let mut status = second.clone();
        status["native_lease_active"] = json!(true);
        status["last_discovery_snapshot_at"] = json!(at);
        let mut proof_tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE category_native_runtime SET heartbeat_at=now()")
            .execute(&mut *proof_tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO category_collector_status(details) VALUES($1)")
            .bind(&status)
            .execute(&mut *proof_tx)
            .await
            .unwrap();
        let wrapper = include_str!("../../../../ops/systemd/deploy-twitch-release");
        let proof = wrapper
            .split("SELECT EXISTS(SELECT FROM category_collector_status s JOIN")
            .nth(1)
            .unwrap()
            .split("\nSQL")
            .next()
            .unwrap();
        let proof = format!("SELECT EXISTS(SELECT FROM category_collector_status s JOIN{proof}")
            .replace(":'pid'", "$1")
            .replace(":'lease_id'", "$2")
            .replace(":'cutoff'", "$3");
        let proved: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(proof.as_str()))
            .bind("102")
            .bind("102-2000")
            .bind(at - chrono::Duration::seconds(1))
            .fetch_one(&mut *proof_tx)
            .await
            .unwrap();
        assert!(proved);
        let stream_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM category_stream_snapshots")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stream_rows, 0);
        let wrong_owner: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(proof.as_str()))
            .bind("101")
            .bind("101-1000")
            .bind(at - chrono::Duration::seconds(1))
            .fetch_one(&mut *proof_tx)
            .await
            .unwrap();
        assert!(!wrong_owner);
        proof_tx.commit().await.unwrap();
        let counters = Counters::default();
        for (details, expected) in [
            (
                json!({"raw_paused":true,"disk_paused":false}),
                (false, false),
            ),
            (json!({"raw_paused":true,"disk_paused":true}), (false, true)),
            (
                json!({"budget_paused":true,"disk_paused":false}),
                (true, false),
            ),
        ] {
            sqlx::query("UPDATE category_collector_status SET details=$1")
                .bind(details)
                .execute(&pool)
                .await
                .unwrap();
            restore_storage_latches(&pool, &counters).await.unwrap();
            let restored = (
                counters.budget_paused.load(Ordering::Relaxed),
                counters.disk_paused.load(Ordering::Relaxed),
            );
            assert_eq!(restored, expected);
            assert_eq!(
                storage_state(950, &settings(), Some(10_000_000_000), restored, true).0,
                expected.0
            );
        }
        waiter.close().await.unwrap();
        pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }

    #[test]
    fn disabled_collection_does_not_accept_inflight_chat() {
        let mut cfg = settings();
        cfg.enabled = false;
        let (raw_paused, disk_paused, _, _) =
            storage_state(0, &cfg, Some(1000), (false, false), false);
        assert!(raw_paused);
        assert!(!disk_paused);
    }
}

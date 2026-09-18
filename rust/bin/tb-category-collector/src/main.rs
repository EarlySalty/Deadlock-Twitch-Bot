include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

mod config;
mod metadata;

use std::{collections::HashMap, sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}}, time::Duration};
use chrono::{DateTime,Utc};
use serde_json::{json,Value};
use sqlx::{Connection,PgConnection,PgPool,Row,postgres::PgPoolOptions};
use tb_analytics::category;
use tb_monitoring::anonymous_chat::{AnonymousChat,ReadEvent,ReadStats,command_channel};
use tb_transport_twitch::{HelixClient,HelixConfig};
use tokio::{sync::{Mutex,RwLock,mpsc},task::JoinSet};
use config::Config;

type Error=Box<dyn std::error::Error+Send+Sync>;
type Roster=Arc<RwLock<HashMap<String,(String,String)>>>;
#[derive(Default)]
struct Counters {
    stored:AtomicU64, invalid:AtomicU64, storage_dropped:AtomicU64,
    raw_paused:AtomicBool, details:Mutex<Value>,
}

#[tokio::main(worker_threads=2)]
async fn main()->Result<(),Error> {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).with_target(false).init();
    let args:Vec<_>=std::env::args().collect();
    if args.len()!=3 || args[1]!="--config" { return Err("usage: tb-category-collector --config /absolute/config.json".into()); }
    let config=Arc::new(Config::load(std::path::Path::new(&args[2]))?);
    let credentials=config.credentials().await?;
    let helix=Arc::new(HelixClient::new(HelixConfig::new(credentials.client_id.clone(),credentials.client_secret.clone()))?);
    drop(credentials);
    let pool=PgPoolOptions::new().max_connections(5).acquire_timeout(Duration::from_secs(10))
        .after_connect(|connection,_| Box::pin(async move {
            sqlx::query("SET timezone='UTC';").execute(&mut *connection).await?;
            sqlx::query("SET statement_timeout='30s';").execute(&mut *connection).await?;
            Ok(())
        })).connect(&config.database_url).await?;
    let mut leader=PgConnection::connect(&config.database_url).await?;
    let elected:bool=sqlx::query_scalar("SELECT pg_try_advisory_lock(782363991806)").fetch_one(&mut leader).await?;
    if !elected { return Err("another category collector already owns the database lease".into()); }
    // Migrations are operator-owned. Missing schema fails startup, never creates
    // unrelated tables or upgrades the bot's grants.
    sqlx::query("SELECT category_prepare_partitions()").execute(&pool).await?;
    let game_id=helix.search_category_id("Deadlock").await?.ok_or("Deadlock category unavailable")?;
    let roster:Roster=Arc::new(RwLock::new(HashMap::new()));
    let counters=Arc::new(Counters::default());
    *counters.details.lock().await=json!({"game_id":game_id,"mode":"anonymous_read_only","desired_channels":0,
        "poll_seconds":config.poll_seconds,"retention_days":config.retention_days,"raw_budget_bytes":config.raw_budget_bytes,
        "media_enabled":config.media_enabled,"language_detector":category::DETECTOR,"discovery_state":"starting"});
    let (chat,events)=AnonymousChat::start(20_000);
    let chat=Arc::new(chat);
    let mut tasks:JoinSet<Result<(),Error>>=JoinSet::new();
    tasks.spawn(discover(pool.clone(),helix.clone(),game_id.clone(),chat.clone(),roster.clone(),counters.clone(),config.poll_seconds));
    tasks.spawn(maintenance(pool.clone(),counters.clone(),config.clone()));
    tasks.spawn(heartbeat(pool.clone(),chat.stats.clone(),counters.clone()));
    tasks.spawn(metadata::run(pool.clone(),helix,config.clone(),game_id));
    tasks.spawn(async move {
        loop { tokio::time::sleep(Duration::from_secs(10)).await; leader.ping().await?; }
        #[allow(unreachable_code)] Ok(())
    });
    let mut writer=tokio::spawn(write_chat(pool,events,roster,counters));
    tracing::info!("category collector started: anonymous chat, public Helix reads, no outbound actions");
    let outcome:Result<(),Error>=tokio::select! {
        _=shutdown_signal()=>Ok(()),
        result=tasks.join_next()=>Err(format!("collector worker exited: {result:?}").into()),
        result=&mut writer=>Err(format!("chat storage worker exited: {result:?}").into()),
    };
    tasks.shutdown().await;
    drop(chat);
    if !writer.is_finished() {
        match tokio::time::timeout(Duration::from_secs(20),&mut writer).await {
            Ok(result)=>{ result??; },
            Err(_)=>{ writer.abort(); return Err("chat drain exceeded shutdown deadline".into()); }
        }
    }
    outcome
}

async fn shutdown_signal() {
    let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=term.recv()=>{} }
}

async fn discover(pool:PgPool,helix:Arc<HelixClient>,game_id:String,chat:Arc<AnonymousChat>,roster:Roster,counters:Arc<Counters>,seconds:u64)->Result<(),Error> {
    let mut timer=tokio::time::interval(Duration::from_secs(seconds));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_success:Option<tokio::time::Instant>=None;
    loop {
        timer.tick().await;
        let at=Utc::now();
        let result=tokio::time::timeout(Duration::from_secs(seconds.saturating_sub(5)),helix.get_all_streams_by_category(&game_id)).await;
        match result {
            Ok(Ok(streams))=>{
                category::store_snapshot(&pool,at,&streams,seconds as i32).await?;
                let next:HashMap<_,_>=streams.iter().filter(|s|tb_monitoring::anonymous_chat::valid_channel(&s.user_login))
                    .map(|s|(s.user_login.to_ascii_lowercase(),(s.user_id.clone(),s.language.clone()))).collect();
                let channels:Vec<_>=next.keys().cloned().collect();
                *roster.write().await=next;
                chat.set_channels(&channels);
                last_success=Some(tokio::time::Instant::now());
                let mut status=counters.details.lock().await;
                status["last_discovery"]=json!(Utc::now()); status["discovery_state"]=json!("ok");
                status["desired_channels"]=json!(channels.len()); status["last_discovery_error"]=Value::Null;
                tracing::info!(streams=streams.len(),viewers=streams.iter().map(|s|s.viewer_count).sum::<i64>(),"category snapshot committed");
            }
            failed=>{
                let stale=last_success.is_none_or(|last|last.elapsed()>Duration::from_secs(seconds*2));
                if stale { roster.write().await.clear(); chat.set_channels(&[]); }
                let mut status=counters.details.lock().await;
                status["discovery_state"]=json!(if stale {"stale_chat_stopped"} else {"retrying"});
                status["last_discovery_error"]=json!(format!("{failed:?}"));
                if stale {status["desired_channels"]=json!(0);}
                tracing::warn!(stale,"category discovery failed; no partial snapshot was stored");
            }
        }
    }
}

async fn write_chat(pool:PgPool,mut events:mpsc::Receiver<ReadEvent>,roster:Roster,counters:Arc<Counters>)->Result<(),Error> {
    let mut batch=Vec::with_capacity(500);
    let mut timer=tokio::time::interval(Duration::from_secs(2));
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
                    category::delete_chat(&pool,&event.line,&room_id).await?;
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
async fn flush(pool:&PgPool,batch:&mut Vec<category::RawMessage>,counters:&Counters)->Result<(),Error> {
    if batch.is_empty(){ return Ok(()); }
    let mut attempt=0;
    loop {
        match category::store_messages(pool,batch).await {
            Ok(count)=>{ counters.stored.fetch_add(count as u64,Ordering::Relaxed); batch.clear(); return Ok(()); }
            Err(error) if attempt<3=>{ attempt+=1; tracing::warn!(%error,attempt,"chat batch retry; pending rows retained"); tokio::time::sleep(Duration::from_secs(attempt)).await; }
            Err(error)=>return Err(error.into()),
        }
    }
}

async fn maintenance(pool:PgPool,counters:Arc<Counters>,config:Arc<Config>)->Result<(),Error> {
    let mut timer=tokio::time::interval(Duration::from_secs(30));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut iteration=0u64;
    loop {
        timer.tick().await;
        category::flush_rollups(&pool,2000).await?;
        if iteration%20==0 {
            sqlx::query("SELECT category_prepare_partitions()").execute(&pool).await?;
            let row=sqlx::query("SELECT * FROM category_prune_partitions($1,$2)").bind(config.retention_days).bind(config.raw_budget_bytes).fetch_one(&pool).await?;
            let bytes:i64=row.try_get("raw_bytes")?;
            let pressure:bool=row.try_get("pressure")?;
            let expired=category::trim_expired_rows(&pool,config.retention_days).await?;
            // A day alone may exceed the budget. Pause rather than fill the host;
            // drops are explicit, never reported as complete collection.
            counters.raw_paused.store(bytes>=config.raw_budget_bytes,Ordering::Relaxed);
            let mut status=counters.details.lock().await;
            status["raw_bytes"]=json!(bytes); status["retention_pressure"]=json!(pressure);
            status["retention_checked_at"]=json!(Utc::now()); status["expired_rows_removed"]=json!(expired);
            status["partitions_dropped"]=json!(row.try_get::<i32,_>("dropped")?);
            status["raw_paused"]=json!(bytes>=config.raw_budget_bytes);
        }
        iteration+=1;
    }
}
async fn heartbeat(pool:PgPool,stats:Arc<ReadStats>,counters:Arc<Counters>)->Result<(),Error> {
    let mut timer=tokio::time::interval(Duration::from_secs(15));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        let mut status=counters.details.lock().await.clone();
        status["connected_shards"]=json!(stats.connected_shards.load(Ordering::Relaxed));
        status["confirmed_channels"]=json!(stats.confirmed_channels.load(Ordering::Relaxed));
        status["received_messages"]=json!(stats.received_messages.load(Ordering::Relaxed));
        status["queue_drops"]=json!(stats.dropped_events.load(Ordering::Relaxed));
        status["irc_reconnects"]=json!(stats.reconnects.load(Ordering::Relaxed));
        status["stored_this_process"]=json!(counters.stored.load(Ordering::Relaxed));
        status["invalid_events"]=json!(counters.invalid.load(Ordering::Relaxed));
        status["storage_drops"]=json!(counters.storage_dropped.load(Ordering::Relaxed));
        status["process_started_note"]=json!("counters reset on restart; persistent totals are in category tables");
        let raw:Option<DateTime<Utc>>=sqlx::query_scalar("SELECT min(sent_at) FROM category_chat_messages").fetch_one(&pool).await?;
        status["oldest_raw_message"]=json!(raw);
        sqlx::query("INSERT INTO category_collector_status(singleton,heartbeat_at,details) VALUES(true,now(),$1::jsonb)
            ON CONFLICT(singleton) DO UPDATE SET heartbeat_at=excluded.heartbeat_at,details=excluded.details")
            .bind(status.to_string()).execute(&pool).await?;
    }
}

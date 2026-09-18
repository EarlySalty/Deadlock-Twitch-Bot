//! Independent collector runtime. Only the existing anonymous IRC transport
//! and read-only Helix methods are wired here; no ChatApi, bot token, moderation
//! actions, engagement pipeline, video/audio reader or speech processing.
use crate::{
    config, sprache,
    store::{self, ChatZeile},
};
use chrono::{DateTime, TimeZone, Utc};
use sqlx::{pool::PoolConnection, PgPool, Postgres};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tb_engagement::irc_message::{parse_privmsg, parse_tags};
use tb_monitoring::anon_chat::{AnonChatConfig, AnonChatHandle, PrivmsgSink, TokioTaskSpawner};
use tb_transport_twitch::HelixClient;
use tokio::{sync::mpsc, task::JoinSet};

type Roster = Arc<RwLock<HashMap<String, String>>>;
#[derive(Default)]
struct Counters {
    rejected: AtomicU64,
    duplicates: AtomicU64,
}
#[derive(Debug)]
enum Event {
    Message(ChatZeile),
    Redact {
        room: String,
        message: Option<String>,
        chatter: Option<String>,
        at: DateTime<Utc>,
    },
}
struct Sink {
    queue: mpsc::Sender<Event>,
    roster: Roster,
    counters: Arc<Counters>,
}
#[async_trait::async_trait]
impl PrivmsgSink for Sink {
    async fn handle_privmsg(&self, line: String) {
        let event = parse_event(&line, &self.roster);
        match event {
            Some(event) => {
                if self.queue.try_send(event).is_err() {
                    self.counters.rejected.fetch_add(1, Ordering::Relaxed);
                }
            }
            None => {
                self.counters.rejected.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
fn time(tags: &HashMap<String, String>) -> DateTime<Utc> {
    tags.get("tmi-sent-ts")
        .and_then(|s| s.parse().ok())
        .and_then(|n| Utc.timestamp_millis_opt(n).single())
        .unwrap_or_else(Utc::now)
}
fn tag(tags: &HashMap<String, String>, key: &str) -> Option<String> {
    tags.get(key).filter(|s| !s.is_empty()).cloned()
}
fn parse_event(line: &str, roster: &Roster) -> Option<Event> {
    if let Some(p) = parse_privmsg(line) {
        let room = roster
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&p.channel.to_ascii_lowercase())?
            .clone();
        if tag(&p.tags, "room-id").as_deref() != Some(&room)
            || p.text.is_empty()
            || p.text.len() > 16_384
        {
            return None;
        }
        let user = tag(&p.tags, "user-id")?;
        let sent = time(&p.tags);
        if sent > Utc::now() + chrono::Duration::minutes(5) {
            return None;
        }
        let detected = sprache::erkenne(&p.text, p.tags.get("emotes").map(String::as_str));
        let id = tag(&p.tags, "id").unwrap_or_else(|| {
            let input = format!("{room}\0{user}\0{}\0{}", sent.timestamp_millis(), p.text);
            let hash = input.bytes().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
            });
            format!("fallback-{hash:016x}")
        });
        let emotes = p.tags.get("emotes").map_or(0, |s| {
            s.split('/')
                .filter_map(|part| part.split_once(':'))
                .flat_map(|(_, positions)| positions.split(','))
                .filter(|pos| {
                    pos.split_once('-').is_some_and(|(a, b)| {
                        a.parse::<usize>().is_ok() && b.parse::<usize>().is_ok()
                    })
                })
                .count() as i32
        });
        return Some(Event::Message(ChatZeile {
            room_user_id: room,
            message_id: id,
            source_message_id: tag(&p.tags, "source-id"),
            source_room_id: tag(&p.tags, "source-room-id"),
            sent_at: sent,
            chatter_user_id: user,
            chatter_login: p.login,
            text_len: p.text.chars().count() as i32,
            message_text: p.text,
            detected_lang: detected.lang.unwrap_or_else(|| "und".into()),
            lang_confidence: detected.confidence,
            lang_method: detected.method.into(),
            emote_count: emotes,
            irc_tags: serde_json::json!(p.tags),
        }));
    }
    let (tags, rest) = if let Some(rest) = line.strip_prefix('@') {
        let (tags, rest) = rest.split_once(' ')?;
        (parse_tags(tags), rest)
    } else {
        (HashMap::new(), line)
    };
    let rest = if rest.starts_with(':') {
        rest.split_once(' ')?.1
    } else {
        rest
    };
    let mut fields = rest.splitn(3, ' ');
    let command = fields.next()?;
    if !matches!(command, "CLEARMSG" | "CLEARCHAT") {
        return None;
    }
    let login = fields.next()?.strip_prefix('#')?.to_ascii_lowercase();
    let room = roster
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&login)?
        .clone();
    if tag(&tags, "room-id").is_some_and(|id| id != room) {
        return None;
    }
    let message = if command == "CLEARMSG" {
        Some(tag(&tags, "target-msg-id")?)
    } else {
        None
    };
    Some(Event::Redact {
        room,
        message,
        chatter: tag(&tags, "target-user-id"),
        at: time(&tags),
    })
}

pub struct SammlerDienst {
    pool: PgPool,
    helix: HelixClient,
    _leader: PoolConnection<Postgres>,
}
impl SammlerDienst {
    pub async fn new(pool: PgPool, helix: HelixClient) -> Result<Self, sqlx::Error> {
        let mut leader = pool.acquire().await?;
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(827445012)")
            .fetch_one(&mut *leader)
            .await?;
        if !locked {
            return Err(sqlx::Error::Protocol(
                "another category collector holds the singleton lock".into(),
            ));
        }
        config::lade(&pool).await?;
        sqlx::query("UPDATE category_polls SET status='failed',completed_at=now(),error_detail='collector restarted during poll' WHERE status='running'")
            .execute(&pool).await?;
        Ok(Self {
            pool,
            helix,
            _leader: leader,
        })
    }
    /// Optional bounded duration is for an explicit smoke run, not a second
    /// collector configuration surface. All collection policy remains in PG.
    pub async fn run(self, duration: Option<Duration>) -> Result<(), String> {
        let baseline = store::counter_baseline(&self.pool)
            .await
            .map_err(|_| "collector counters could not be loaded")?;
        let roster: Roster = Default::default();
        let counters = Arc::new(Counters::default());
        let (tx, rx) = mpsc::channel(2_048);
        let nick_base = 70_000_000 + (std::process::id() as u64 % 1_000_000) * 100;
        let chat = AnonChatHandle::start(
            Arc::new(Sink {
                queue: tx,
                roster: roster.clone(),
                counters: counters.clone(),
            }),
            AnonChatConfig {
                nick_base,
                message_queue_capacity: 512,
                ..Default::default()
            },
            Arc::new(TokioTaskSpawner),
        );
        let mut writer = tokio::spawn(writer(self.pool.clone(), rx, counters.clone()));
        let mut tasks = JoinSet::new();
        tasks.spawn(discovery(
            self.pool.clone(),
            self.helix.clone(),
            roster,
            chat.clone(),
        ));
        tasks.spawn(maintenance(self.pool.clone()));
        tasks.spawn(crate::metadata::run(self.pool.clone(), self.helix.clone()));
        tasks.spawn(status_loop(
            self.pool.clone(),
            chat.clone(),
            counters.clone(),
            baseline.clone(),
        ));
        let stop_after = async {
            match duration {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending::<()>().await,
            }
        };
        let mut result = tokio::select! {
            _=shutdown()=>Ok(()),
            _=stop_after=>Ok(()),
            _=tasks.join_next()=>Err("critical collector worker ended; restart required".into()),
            _=&mut writer=>Err("collector storage worker ended; restart required".into()),
        };
        chat.stop().await;
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        // Closing the anonymous coordinator drains the bounded dispatcher,
        // closes its Sink, then lets the DB writer commit the final batch.
        if !writer.is_finished() {
            match tokio::time::timeout(Duration::from_secs(15), &mut writer).await {
                Ok(Ok(Ok(()))) => {}
                _ => {
                    writer.abort();
                    tracing::error!("collector final writer flush failed");
                    result = Err("collector final writer flush failed".into());
                }
            }
        }
        if store::status(
            &self.pool,
            &chat.stats(),
            &baseline,
            counters.rejected.load(Ordering::Acquire),
            counters.duplicates.load(Ordering::Acquire),
        )
        .await
        .is_err()
        {
            result = Err("collector final status could not be saved".into());
        }
        result
    }
}
async fn shutdown() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut terminate) => tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}},
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}
async fn discovery(
    pool: PgPool,
    helix: HelixClient,
    roster: Roster,
    chat: AnonChatHandle,
) -> Result<(), sqlx::Error> {
    let mut validated: Option<(String, DateTime<Utc>)> = None;
    let mut last_good = Utc::now();
    loop {
        let started = tokio::time::Instant::now();
        let mut cfg = config::lade(&pool).await?;
        if !cfg.enabled {
            roster.write().unwrap_or_else(|e| e.into_inner()).clear();
            chat.set_channels(vec![]).await;
            tokio::time::sleep(cfg.interval()).await;
            continue;
        }
        if validated.as_ref().is_none_or(|(id, at)| {
            id != &cfg.deadlock_game_id || Utc::now() - *at >= chrono::Duration::hours(1)
        }) {
            match helix.resolve_category_id_exact("Deadlock").await {
                Ok(Some(id)) => {
                    if id != cfg.deadlock_game_id {
                        sqlx::query("UPDATE category_collector_config SET deadlock_game_id=$1,updated_at=now() WHERE id=1")
                            .bind(&id).execute(&pool).await?;
                    }
                    cfg.deadlock_game_id = id.clone();
                    validated = Some((id, Utc::now()));
                }
                _ => {
                    store::error(&pool, "exact Deadlock category resolution failed").await?;
                    roster.write().unwrap_or_else(|e| e.into_inner()).clear();
                    chat.set_channels(vec![]).await;
                    tokio::time::sleep(cfg.interval()).await;
                    continue;
                }
            }
        }
        let at = Utc::now();
        let poll = store::start_poll(&pool, at).await?;
        let fetch = helix
            .get_streams_by_category_full(&cfg.deadlock_game_id, 10_000)
            .await;
        if fetch.complete {
            store::complete_poll(&pool, poll, at, &fetch.streams, fetch.pages_fetched).await?;
            let next: HashMap<_, _> = if cfg.chat_enabled {
                fetch
                    .streams
                    .iter()
                    .map(|s| (s.user_login.to_ascii_lowercase(), s.user_id.clone()))
                    .collect()
            } else {
                HashMap::new()
            };
            let logins = next.keys().cloned().collect();
            *roster.write().unwrap_or_else(|e| e.into_inner()) = next;
            chat.set_channels(logins).await;
            last_good = Utc::now();
            tracing::info!(
                poll,
                streams = fetch.streams.len(),
                viewers = fetch.streams.iter().map(|s| s.viewer_count).sum::<i64>(),
                pages = fetch.pages_fetched,
                "category snapshot committed"
            );
        } else {
            store::fail_poll(
                &pool,
                poll,
                fetch.pages_fetched,
                fetch.truncated_by_cap,
                fetch.error.as_deref().unwrap_or("category cap reached"),
            )
            .await?;
            if Utc::now() - last_good >= chrono::Duration::seconds(cfg.roster_decay_seconds as i64)
            {
                roster.write().unwrap_or_else(|e| e.into_inner()).clear();
                chat.set_channels(vec![]).await;
            }
        }
        tokio::time::sleep_until(started + cfg.interval()).await;
    }
}
async fn writer(
    pool: PgPool,
    mut rx: mpsc::Receiver<Event>,
    counters: Arc<Counters>,
) -> Result<(), sqlx::Error> {
    let mut batch = Vec::with_capacity(500);
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            item=rx.recv()=>match item {
                Some(Event::Message(row))=>{batch.push(row);if batch.len()>=500 {flush(&pool,&mut batch,&counters).await?;}},
                Some(Event::Redact {room,message,chatter,at})=>{
                    // Preserve stream order: insert previous messages before
                    // applying the deletion, never maintain a second queue.
                    flush(&pool,&mut batch,&counters).await?;
                    store::redact(&pool,&room,message.as_deref(),chatter.as_deref(),at).await?;
                },
                None=>{flush(&pool,&mut batch,&counters).await?;return Ok(());},
            },
            _=tick.tick()=>flush(&pool,&mut batch,&counters).await?,
        }
    }
}
async fn flush(
    pool: &PgPool,
    batch: &mut Vec<ChatZeile>,
    counters: &Counters,
) -> Result<(), sqlx::Error> {
    if batch.is_empty() {
        return Ok(());
    }
    let (_, duplicates, expired) = store::insert_chat(pool, batch).await?;
    counters
        .duplicates
        .fetch_add(duplicates as u64, Ordering::Relaxed);
    counters
        .rejected
        .fetch_add(expired as u64, Ordering::Relaxed);
    batch.clear();
    Ok(())
}
async fn maintenance(pool: PgPool) -> Result<(), sqlx::Error> {
    loop {
        // Configuration validation remains mandatory even while paused.
        config::lade(&pool).await?;
        for _ in 0..120 {
            if !store::rollup_one(&pool).await? {
                break;
            }
        }
        for _ in 0..20 {
            if store::retention_batch(&pool).await? == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
async fn status_loop(
    pool: PgPool,
    chat: AnonChatHandle,
    counters: Arc<Counters>,
    baseline: store::CounterBaseline,
) -> Result<(), sqlx::Error> {
    loop {
        store::status(
            &pool,
            &chat.stats(),
            &baseline,
            counters.rejected.load(Ordering::Acquire),
            counters.duplicates.load(Ordering::Acquire),
        )
        .await?;
        tokio::time::sleep(Duration::from_secs(15)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn roster() -> Roster {
        Arc::new(RwLock::new(HashMap::from([(
            "channel".into(),
            "99".into(),
        )])))
    }
    #[test]
    fn original_and_shared_chat_metadata_preserved_without_bot_filter() {
        let line=format!("@room-id=99;user-id=42;id=original;source-id=origin;source-room-id=100;tmi-sent-ts={} :nightbot!n@n PRIVMSG #channel :Hello this is a public message",Utc::now().timestamp_millis());
        let Some(Event::Message(row)) = parse_event(&line, &roster()) else {
            panic!("message discarded");
        };
        assert_eq!(row.chatter_login, "nightbot");
        assert_eq!(row.source_room_id.as_deref(), Some("100"));
        assert_eq!(row.message_text, "Hello this is a public message");
    }
    #[test]
    fn server_prefix_does_not_hide_moderation_event() {
        let Some(Event::Redact { message, .. }) = parse_event(
            "@room-id=99;target-msg-id=abc :tmi.twitch.tv CLEARMSG #channel :removed",
            &roster(),
        ) else {
            panic!("event lost");
        };
        assert_eq!(message.as_deref(), Some("abc"));
    }
    #[test]
    fn unexpected_room_and_unknown_channel_are_not_collected() {
        assert!(parse_event(
            "@room-id=88;user-id=42 :u!u@u PRIVMSG #channel :hello",
            &roster()
        )
        .is_none());
        assert!(parse_event(
            "@room-id=99;user-id=42 :u!u@u PRIVMSG #other :hello",
            &roster()
        )
        .is_none());
    }
}

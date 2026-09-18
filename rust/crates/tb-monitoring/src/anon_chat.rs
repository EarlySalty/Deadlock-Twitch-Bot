//! Gemeinsamer anonymer Twitch-IRC-Transport für Scout und Kategoriesammler.
//!
//! Es gibt keinen Token-Parameter, keinen PASS und keine Chat-/Moderations-API.
//! Der zentrale Writer akzeptiert ausschließlich NICK justinfan, CAP, JOIN,
//! PART, PING und PONG. Nachrichten und Löschereignisse werden nur empfangen.
//! Die Kanalzuordnung bleibt stabil und überschreitet niemals 100 je Shard.
//! Netzwerk-Reader und JOIN-Drossel laufen gleichzeitig; begrenzte Queues
//! machen Verluste sichtbar statt unendlich viele Tasks anzulegen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

const MAX_IRC_LINE_BYTES: usize = 65_536;
const RATE_WINDOW: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct AnonChatConfig {
    pub irc_host: String,
    pub irc_port: u16,
    pub nick_base: u64,
    pub max_channels_per_connection: usize,
    pub join_stagger: Duration,
    pub connect_backoff: Duration,
    pub connect_timeout: Duration,
    pub read_idle_before_ping: Duration,
    pub read_deadline_after_ping: Duration,
    pub write_timeout: Duration,
    pub global_joins_per_10s: u32,
    pub global_connects_per_10s: u32,
    /// Begrenzte Coordinator-Queue. Pro Shard wird nur der letzte Sollstand gehalten.
    pub command_queue_capacity: usize,
    pub message_queue_capacity: usize,
}

impl Default for AnonChatConfig {
    fn default() -> Self {
        Self {
            irc_host: "irc.chat.twitch.tv".into(),
            irc_port: 6667,
            nick_base: 13_371_338,
            max_channels_per_connection: 100,
            join_stagger: Duration::from_millis(600),
            connect_backoff: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            read_idle_before_ping: Duration::from_secs(240),
            read_deadline_after_ping: Duration::from_secs(60),
            write_timeout: Duration::from_secs(5),
            global_joins_per_10s: 15,
            global_connects_per_10s: 5,
            command_queue_capacity: 64,
            message_queue_capacity: 4096,
        }
    }
}

impl AnonChatConfig {
    fn bounded(mut self) -> Self {
        self.max_channels_per_connection = self.max_channels_per_connection.clamp(1, 100);
        self.global_joins_per_10s = self.global_joins_per_10s.clamp(1, 20);
        self.global_connects_per_10s = self.global_connects_per_10s.clamp(1, 5);
        self.command_queue_capacity = self.command_queue_capacity.clamp(1, 4096);
        self.message_queue_capacity = self.message_queue_capacity.clamp(1, 65_536);
        self.nick_base = self.nick_base.min(9_000_000_000);
        for duration in [
            &mut self.join_stagger,
            &mut self.connect_backoff,
            &mut self.connect_timeout,
            &mut self.read_idle_before_ping,
            &mut self.read_deadline_after_ping,
            &mut self.write_timeout,
        ] {
            *duration = (*duration).max(Duration::from_millis(1));
        }
        self
    }
}

/// Historischer Name: liefert PRIVMSG sowie CLEARMSG/CLEARCHAT in Reihenfolge.
/// Der Dispatcher wartet auf genau einen Sink-Aufruf; niemals ein Task je Zeile.
#[async_trait::async_trait]
pub trait PrivmsgSink: Send + Sync + 'static {
    async fn handle_privmsg(&self, line: String);
}

pub trait TaskSpawner: Send + Sync + 'static {
    fn spawn_task(&self, name: &'static str, future: Pin<Box<dyn Future<Output = ()> + Send>>);
}

pub struct TokioTaskSpawner;
impl TaskSpawner for TokioTaskSpawner {
    fn spawn_task(&self, _name: &'static str, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        tokio::spawn(future);
    }
}

#[derive(Debug, Default)]
pub struct AnonChatStats {
    pub privmsgs_dispatched: AtomicU64,
    pub privmsgs_dropped: AtomicU64,
    pub control_events_dispatched: AtomicU64,
    pub control_events_dropped: AtomicU64,
    pub commands_dropped: AtomicU64,
    pub joins_written: AtomicU64,
    pub parts_written: AtomicU64,
    pub invalid_logins_rejected: AtomicU64,
    pub reconnects: AtomicU64,
    pub pongs_sent: AtomicU64,
    pub pings_sent: AtomicU64,
    pub framing_errors: AtomicU64,
    pub connected_shards: AtomicUsize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AnonChatStatsSnapshot {
    pub privmsgs_dispatched: u64,
    pub privmsgs_dropped: u64,
    pub control_events_dispatched: u64,
    pub control_events_dropped: u64,
    pub commands_dropped: u64,
    pub joins_written: u64,
    pub parts_written: u64,
    pub invalid_logins_rejected: u64,
    pub reconnects: u64,
    pub pongs_sent: u64,
    pub pings_sent: u64,
    pub framing_errors: u64,
    pub shards_running: usize,
    pub connected_shards: usize,
    /// Gewünschte Kanäle, kein behaupteter JOIN-ACK-/Vollständigkeitsnachweis.
    pub channels_monitored: usize,
}

pub fn validiere_login(raw: &str) -> Option<String> {
    if raw.contains(['\r', '\n']) {
        return None;
    }
    let login = raw
        .trim()
        .strip_prefix('#')
        .unwrap_or(raw.trim())
        .to_ascii_lowercase();
    if login.is_empty()
        || login.len() > 25
        || !login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        None
    } else {
        Some(login)
    }
}

/// Bisherige Zuordnungen bleiben erhalten. Nur neue Kanäle belegen freie Plätze.
fn assign_channels(
    desired: &HashSet<String>,
    capacity: usize,
    assignment: &mut HashMap<String, usize>,
    existing_shards: usize,
) -> Vec<HashSet<String>> {
    let capacity = capacity.clamp(1, 100);
    assignment.retain(|login, _| desired.contains(login));
    let count = existing_shards.max(assignment.values().max().map_or(0, |index| index + 1));
    let mut result = vec![HashSet::new(); count];
    for (login, &index) in assignment.iter() {
        result[index].insert(login.clone());
    }
    let mut added: Vec<_> = desired
        .iter()
        .filter(|login| !assignment.contains_key(*login))
        .cloned()
        .collect();
    added.sort_unstable();
    for login in added {
        let index = result
            .iter()
            .position(|shard| shard.len() < capacity)
            .unwrap_or_else(|| {
                result.push(HashSet::new());
                result.len() - 1
            });
        result[index].insert(login.clone());
        assignment.insert(login, index);
    }
    result
}

#[derive(Debug)]
struct Throttle {
    limit: usize,
    window: std::sync::Mutex<VecDeque<Instant>>,
}

impl Throttle {
    fn new(limit: u32) -> Self {
        Self {
            limit: limit.max(1) as usize,
            window: Default::default(),
        }
    }

    /// Keine await-Stelle nach der Reservierung: abgebrochene Warte-Futures
    /// halten keine Sperre und überschreiten niemals das gemeinsame Budget.
    async fn slot(&self) {
        loop {
            let wait = {
                let mut window = self
                    .window
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let now = Instant::now();
                while window
                    .front()
                    .is_some_and(|at| now.duration_since(*at) >= RATE_WINDOW)
                {
                    window.pop_front();
                }
                if window.len() < self.limit {
                    window.push_back(now);
                    return;
                }
                window.front().map_or(Duration::from_millis(1), |at| {
                    RATE_WINDOW
                        .saturating_sub(now.duration_since(*at))
                        .max(Duration::from_millis(1))
                })
            };
            tokio::time::sleep(wait).await;
        }
    }
}

enum RosterCommand {
    Set(Vec<String>),
    Join(Vec<String>),
    Part(Vec<String>),
    Stop,
}

#[derive(Clone)]
pub struct AnonChatHandle {
    tx: mpsc::Sender<RosterCommand>,
    stats: Arc<AnonChatStats>,
    monitored: Arc<AtomicUsize>,
    shards_running: Arc<AtomicUsize>,
    stopped_tx: Arc<watch::Sender<bool>>,
}

impl AnonChatHandle {
    pub fn start(
        sink: Arc<dyn PrivmsgSink>,
        config: AnonChatConfig,
        spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        let config = config.bounded();
        let stats = Arc::new(AnonChatStats::default());
        let monitored = Arc::new(AtomicUsize::new(0));
        let shards_running = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel(config.command_queue_capacity);
        let (stopped_tx, stopped_rx) = watch::channel(false);
        let (msg_tx, mut msg_rx) = mpsc::channel::<String>(config.message_queue_capacity);
        let dispatch_stats = Arc::clone(&stats);
        spawner.spawn_task(
            "anon_chat_dispatcher",
            Box::pin(async move {
                while let Some(line) = msg_rx.recv().await {
                    if irc_command(&line).0 == "PRIVMSG" {
                        dispatch_stats
                            .privmsgs_dispatched
                            .fetch_add(1, Ordering::Relaxed);
                    } else {
                        dispatch_stats
                            .control_events_dispatched
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    sink.handle_privmsg(line).await;
                }
            }),
        );
        let context = CoordinatorContext {
            rx,
            config: config.clone(),
            stats: Arc::clone(&stats),
            msg_tx,
            spawner: Arc::clone(&spawner),
            join_throttle: Arc::new(Throttle::new(config.global_joins_per_10s)),
            connect_throttle: Arc::new(Throttle::new(config.global_connects_per_10s)),
            monitored: Arc::clone(&monitored),
            shards_running: Arc::clone(&shards_running),
            stopped_rx,
        };
        spawner.spawn_task("anon_chat_coordinator", Box::pin(run_coordinator(context)));
        Self {
            tx,
            stats,
            monitored,
            shards_running,
            stopped_tx: Arc::new(stopped_tx),
        }
    }

    async fn send(&self, command: RosterCommand) {
        if self.tx.send(command).await.is_err() {
            self.stats.commands_dropped.fetch_add(1, Ordering::Relaxed);
            tracing::error!("Anonymer Chat-Coordinator ist beendet");
        }
    }
    pub async fn set_channels(&self, logins: Vec<String>) {
        self.send(RosterCommand::Set(logins)).await;
    }
    pub async fn join_channels(&self, logins: Vec<String>) {
        self.send(RosterCommand::Join(logins)).await;
    }
    pub async fn part_channels(&self, logins: Vec<String>) {
        self.send(RosterCommand::Part(logins)).await;
    }
    pub async fn stop(&self) {
        let _ = self.stopped_tx.send(true);
        let _ = self.tx.send(RosterCommand::Stop).await;
    }
    pub fn stats(&self) -> AnonChatStatsSnapshot {
        let s = &self.stats;
        AnonChatStatsSnapshot {
            privmsgs_dispatched: s.privmsgs_dispatched.load(Ordering::Relaxed),
            privmsgs_dropped: s.privmsgs_dropped.load(Ordering::Relaxed),
            control_events_dispatched: s.control_events_dispatched.load(Ordering::Relaxed),
            control_events_dropped: s.control_events_dropped.load(Ordering::Relaxed),
            commands_dropped: s.commands_dropped.load(Ordering::Relaxed),
            joins_written: s.joins_written.load(Ordering::Relaxed),
            parts_written: s.parts_written.load(Ordering::Relaxed),
            invalid_logins_rejected: s.invalid_logins_rejected.load(Ordering::Relaxed),
            reconnects: s.reconnects.load(Ordering::Relaxed),
            pongs_sent: s.pongs_sent.load(Ordering::Relaxed),
            pings_sent: s.pings_sent.load(Ordering::Relaxed),
            framing_errors: s.framing_errors.load(Ordering::Relaxed),
            shards_running: self.shards_running.load(Ordering::Relaxed),
            connected_shards: s.connected_shards.load(Ordering::Relaxed),
            channels_monitored: self.monitored.load(Ordering::Relaxed),
        }
    }
}

struct CoordinatorContext {
    rx: mpsc::Receiver<RosterCommand>,
    config: AnonChatConfig,
    stats: Arc<AnonChatStats>,
    msg_tx: mpsc::Sender<String>,
    spawner: Arc<dyn TaskSpawner>,
    join_throttle: Arc<Throttle>,
    connect_throttle: Arc<Throttle>,
    monitored: Arc<AtomicUsize>,
    shards_running: Arc<AtomicUsize>,
    stopped_rx: watch::Receiver<bool>,
}

async fn run_coordinator(mut ctx: CoordinatorContext) {
    let mut desired = HashSet::new();
    let mut assignment = HashMap::new();
    let mut shard_senders: Vec<watch::Sender<HashSet<String>>> = Vec::new();
    loop {
        let command = tokio::select! {
            _ = ctx.stopped_rx.changed() => break,
            command = ctx.rx.recv() => command,
        };
        let (logins, operation) = match command {
            None | Some(RosterCommand::Stop) => break,
            Some(RosterCommand::Set(logins)) => {
                desired.clear();
                (logins, true)
            }
            Some(RosterCommand::Join(logins)) => (logins, true),
            Some(RosterCommand::Part(logins)) => (logins, false),
        };
        for raw in logins {
            if let Some(login) = validiere_login(&raw) {
                if operation {
                    desired.insert(login);
                } else {
                    desired.remove(&login);
                }
            } else {
                ctx.stats
                    .invalid_logins_rejected
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        ctx.monitored.store(desired.len(), Ordering::Relaxed);
        let assignments = assign_channels(
            &desired,
            ctx.config.max_channels_per_connection,
            &mut assignment,
            shard_senders.len(),
        );
        while shard_senders.len() < assignments.len() {
            let index = shard_senders.len();
            let (tx, rx) = watch::channel(HashSet::new());
            let shard = ShardContext {
                nick: format!(
                    "justinfan{:08}",
                    ctx.config.nick_base.saturating_add(index as u64)
                ),
                rx,
                config: ctx.config.clone(),
                stats: Arc::clone(&ctx.stats),
                msg_tx: ctx.msg_tx.clone(),
                join_throttle: Arc::clone(&ctx.join_throttle),
                connect_throttle: Arc::clone(&ctx.connect_throttle),
                shards_running: Arc::clone(&ctx.shards_running),
                stopped_rx: ctx.stopped_rx.clone(),
            };
            ctx.spawner
                .spawn_task("anon_chat_shard", Box::pin(run_shard(shard)));
            shard_senders.push(tx);
        }
        for (sender, next) in shard_senders.iter().zip(assignments) {
            // Watch coalesces updates without losing the latest roster to a full queue.
            sender.send_replace(next);
        }
    }
}

struct ShardContext {
    nick: String,
    rx: watch::Receiver<HashSet<String>>,
    config: AnonChatConfig,
    stats: Arc<AnonChatStats>,
    msg_tx: mpsc::Sender<String>,
    join_throttle: Arc<Throttle>,
    connect_throttle: Arc<Throttle>,
    shards_running: Arc<AtomicUsize>,
    stopped_rx: watch::Receiver<bool>,
}

struct CountGuard(Arc<AtomicUsize>);
impl CountGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}
impl Drop for CountGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

async fn run_shard(mut ctx: ShardContext) {
    let _running = CountGuard::new(Arc::clone(&ctx.shards_running));
    let mut failed_connects: u32 = 0;
    loop {
        if *ctx.stopped_rx.borrow() {
            break;
        }
        let desired = ctx.rx.borrow_and_update().clone();
        if desired.is_empty() {
            tokio::select! {
                _ = ctx.stopped_rx.changed() => break,
                result = ctx.rx.changed() => if result.is_err() { break; },
            }
            continue;
        }
        tokio::select! {
            _ = ctx.stopped_rx.changed() => break,
            result = ctx.rx.changed() => { if result.is_err() { break; } continue; },
            _ = ctx.connect_throttle.slot() => {},
        }
        let connection = tokio::select! {
            _ = ctx.stopped_rx.changed() => break,
            connection = connect(&ctx.config, &ctx.nick) => connection,
        };
        if let Some((reader, writer)) = connection {
            failed_connects = 0;
            ctx.stats.connected_shards.fetch_add(1, Ordering::Relaxed);
            serve(&mut ctx, reader, writer).await;
            ctx.stats.connected_shards.fetch_sub(1, Ordering::Relaxed);
            if *ctx.stopped_rx.borrow() {
                break;
            }
            if !ctx.rx.borrow().is_empty() {
                ctx.stats.reconnects.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            failed_connects = failed_connects.saturating_add(1);
            tracing::warn!("Anonymer Chat: Verbindung/Handshake fehlgeschlagen");
        }
        let multiplier = 1u32 << failed_connects.saturating_sub(1).min(4);
        let backoff = ctx
            .config
            .connect_backoff
            .saturating_mul(multiplier)
            .min(Duration::from_secs(300));
        tokio::select! {
            _ = ctx.stopped_rx.changed() => break,
            result = ctx.rx.changed() => if result.is_err() { break; },
            _ = tokio::time::sleep(backoff) => {},
        }
    }
}

fn roster_diff(
    desired: &HashSet<String>,
    joined: &HashSet<String>,
) -> (VecDeque<String>, VecDeque<String>) {
    let mut joins: Vec<_> = desired.difference(joined).cloned().collect();
    let mut parts: Vec<_> = joined.difference(desired).cloned().collect();
    joins.sort_unstable();
    parts.sort_unstable();
    (joins.into(), parts.into())
}

async fn serve(
    ctx: &mut ShardContext,
    mut reader: BufReader<OwnedReadHalf>,
    mut writer: OwnedWriteHalf,
) {
    let mut desired = ctx.rx.borrow_and_update().clone();
    let mut joined = HashSet::new();
    let (mut joins, mut parts) = roster_diff(&desired, &joined);
    let mut buffer = Vec::with_capacity(1024);
    let mut next_join = Instant::now();
    let mut last_received = Instant::now();
    let mut ping_deadline: Option<Instant> = None;
    loop {
        if desired.is_empty() && joined.is_empty() {
            return;
        }
        let read_deadline =
            ping_deadline.unwrap_or(last_received + ctx.config.read_idle_before_ping);
        tokio::select! {
            _ = ctx.stopped_rx.changed() => return,
            result = ctx.rx.changed() => {
                if result.is_err() { return; }
                desired = ctx.rx.borrow_and_update().clone();
                (joins, parts) = roster_diff(&desired, &joined);
            },
            _ = tokio::time::sleep_until(read_deadline.into()) => {
                if ping_deadline.is_some() { return; }
                if !write_control(&mut writer, "PING :tmi.twitch.tv\r\n", ctx.config.write_timeout).await { return; }
                ctx.stats.pings_sent.fetch_add(1, Ordering::Relaxed);
                ping_deadline = Some(Instant::now() + ctx.config.read_deadline_after_ping);
            },
            _ = async {}, if !parts.is_empty() => {
                if let Some(login) = parts.pop_front() {
                    if !write_control(&mut writer, &format!("PART #{login}\r\n"), ctx.config.write_timeout).await { return; }
                    joined.remove(&login);
                    ctx.stats.parts_written.fetch_add(1, Ordering::Relaxed);
                }
            },
            _ = async {
                tokio::time::sleep_until(next_join.into()).await;
                ctx.join_throttle.slot().await;
            }, if !joins.is_empty() && parts.is_empty() => {
                if let Some(login) = joins.pop_front() {
                    if !write_control(&mut writer, &format!("JOIN #{login}\r\n"), ctx.config.write_timeout).await { return; }
                    joined.insert(login);
                    ctx.stats.joins_written.fetch_add(1, Ordering::Relaxed);
                }
                next_join = Instant::now() + ctx.config.join_stagger;
            },
            result = read_bounded_line(&mut reader, &mut buffer) => {
                let line = match result {
                    Ok(Some(line)) => line,
                    Ok(None) => return,
                    Err(_) => { ctx.stats.framing_errors.fetch_add(1, Ordering::Relaxed); return; },
                };
                last_received = Instant::now();
                ping_deadline = None;
                let (command, payload) = irc_command(&line);
                match command {
                    "PING" => {
                        if !write_control(&mut writer, &format!("PONG {payload}\r\n"), ctx.config.write_timeout).await { return; }
                        ctx.stats.pongs_sent.fetch_add(1, Ordering::Relaxed);
                    },
                    "RECONNECT" => return,
                    "PRIVMSG" | "CLEARMSG" | "CLEARCHAT" => {
                        let is_message = command == "PRIVMSG";
                        if ctx.msg_tx.try_send(line).is_err() {
                            if is_message { ctx.stats.privmsgs_dropped.fetch_add(1, Ordering::Relaxed); }
                            else { ctx.stats.control_events_dropped.fetch_add(1, Ordering::Relaxed); }
                        }
                    },
                    _ => {},
                }
            },
        }
    }
}

/// Cancel-safe: read_until bewahrt Teilzeilen im übergebenen Puffer.
/// Take begrenzt den Speicher bereits während des Lesens, nicht erst danach.
async fn read_bounded_line(
    reader: &mut BufReader<OwnedReadHalf>,
    buffer: &mut Vec<u8>,
) -> io::Result<Option<String>> {
    let remaining = MAX_IRC_LINE_BYTES.saturating_sub(buffer.len());
    if remaining == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IRC line too long",
        ));
    }
    let count = (&mut *reader)
        .take(remaining as u64)
        .read_until(b'\n', buffer)
        .await?;
    if count == 0 && buffer.is_empty() {
        return Ok(None);
    }
    if !buffer.ends_with(b"\n") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Incomplete IRC line",
        ));
    }
    let mut line = String::from_utf8(std::mem::take(buffer))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid IRC UTF-8"))?;
    line.pop();
    if line.ends_with('\r') {
        line.pop();
    }
    if line.contains(['\r', '\n']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid IRC framing",
        ));
    }
    Ok(Some(line))
}

/// Nur Kontrollkommando bestimmen. PRIVMSG-Inhalt parst weiterhin der bestehende Fachparser.
fn irc_command(line: &str) -> (&str, &str) {
    let mut rest = line;
    if rest.starts_with('@') {
        rest = rest.split_once(' ').map_or("", |(_, tail)| tail);
    }
    if rest.starts_with(':') {
        rest = rest.split_once(' ').map_or("", |(_, tail)| tail);
    }
    rest.split_once(' ').unwrap_or((rest, ""))
}

fn control_is_allowed(line: &str) -> bool {
    let Some(body) = line.strip_suffix("\r\n") else {
        return false;
    };
    if body.contains(['\r', '\n']) {
        return false;
    }
    let (command, rest) = body.split_once(' ').unwrap_or((body, ""));
    match command {
        "NICK" => rest.strip_prefix("justinfan").is_some_and(|digits| {
            !digits.is_empty() && digits.len() <= 17 && digits.bytes().all(|b| b.is_ascii_digit())
        }),
        "CAP" => rest == "REQ :twitch.tv/tags twitch.tv/commands",
        "JOIN" | "PART" => rest
            .strip_prefix('#')
            .is_some_and(|login| validiere_login(login).as_deref() == Some(login)),
        "PING" | "PONG" => rest.starts_with(':') && rest.len() <= 512,
        _ => false,
    }
}

async fn write_control(writer: &mut OwnedWriteHalf, line: &str, timeout: Duration) -> bool {
    if !control_is_allowed(line) {
        tracing::error!("Anonymer IRC-Writer hat unerlaubtes Kommando blockiert");
        return false;
    }
    matches!(
        tokio::time::timeout(timeout, async {
            writer.write_all(line.as_bytes()).await?;
            writer.flush().await
        })
        .await,
        Ok(Ok(()))
    )
}

async fn connect(
    config: &AnonChatConfig,
    nick: &str,
) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
    tokio::time::timeout(config.connect_timeout, async {
        let stream = TcpStream::connect((config.irc_host.as_str(), config.irc_port))
            .await
            .ok()?;
        let _ = stream.set_nodelay(true);
        let (read, mut write) = stream.into_split();
        for command in [
            format!("NICK {nick}\r\n"),
            "CAP REQ :twitch.tv/tags twitch.tv/commands\r\n".to_string(),
        ] {
            if !write_control(&mut write, &command, config.write_timeout).await {
                return None;
            }
        }
        let mut reader = BufReader::new(read);
        let mut buffer = Vec::new();
        loop {
            let line = read_bounded_line(&mut reader, &mut buffer).await.ok()??;
            let (command, payload) = irc_command(&line);
            match command {
                "001" => return Some((reader, write)),
                "PING" => {
                    if !write_control(
                        &mut write,
                        &format!("PONG {payload}\r\n"),
                        config.write_timeout,
                    )
                    .await
                    {
                        return None;
                    }
                }
                "ERROR" | "RECONNECT" => return None,
                _ => {}
            }
        }
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[derive(Default)]
    struct Capture(std::sync::Mutex<Vec<String>>);
    #[async_trait::async_trait]
    impl PrivmsgSink for Capture {
        async fn handle_privmsg(&self, line: String) {
            self.0.lock().unwrap().push(line);
        }
    }

    #[test]
    fn login_guard_and_control_allowlist_block_send_and_authentication() {
        assert_eq!(
            validiere_login(" #Some_User ").as_deref(),
            Some("some_user")
        );
        for login in [
            "",
            "chan\r\nPRIVMSG #other :bad",
            "chan\n",
            "chan other",
            "##chan",
            "äuser",
        ] {
            assert_eq!(validiere_login(login), None, "{login:?}");
        }
        for command in [
            "PASS oauth:secret\r\n",
            "NICK botaccount\r\n",
            "PRIVMSG #chan :hi\r\n",
            "WHISPER user :hi\r\n",
            "JOIN #a,#b\r\n",
            "JOIN #chan\r\nPRIVMSG #chan :hi\r\n",
        ] {
            assert!(!control_is_allowed(command), "{command:?}");
        }
        for command in [
            "NICK justinfan12345678\r\n",
            "CAP REQ :twitch.tv/tags twitch.tv/commands\r\n",
            "JOIN #channel\r\n",
            "PART #channel\r\n",
            "PONG :tmi.twitch.tv\r\n",
        ] {
            assert!(control_is_allowed(command), "{command:?}");
        }
    }

    #[test]
    fn capacity_is_hard_and_surviving_channels_never_move() {
        let mut assignment = HashMap::new();
        let mut desired: HashSet<_> = (0..200).map(|n| format!("channel_{n:03}")).collect();
        let first = assign_channels(&desired, 100, &mut assignment, 0);
        assert_eq!(
            first.iter().map(HashSet::len).collect::<Vec<_>>(),
            [100, 100]
        );
        let before = assignment.clone();
        desired.extend((200..301).map(|n| format!("channel_{n:03}")));
        let next = assign_channels(&desired, 100, &mut assignment, first.len());
        assert_eq!(next.iter().map(HashSet::len).sum::<usize>(), 301);
        assert!(next.iter().all(|shard| shard.len() <= 100));
        for (login, shard) in &before {
            assert_eq!(assignment.get(login), Some(shard));
        }
        desired.remove("channel_001");
        desired.insert("replacement".into());
        let after = assignment.clone();
        let last = assign_channels(&desired, 100, &mut assignment, next.len());
        assert!(last.iter().all(|shard| shard.len() <= 100));
        for (login, shard) in &after {
            if desired.contains(login) {
                assert_eq!(assignment.get(login), Some(shard));
            }
        }
    }

    #[test]
    fn configuration_has_no_zero_queue_or_unbounded_join_budget() {
        let config = AnonChatConfig {
            global_joins_per_10s: 0,
            global_connects_per_10s: 999,
            max_channels_per_connection: 1000,
            message_queue_capacity: 0,
            ..Default::default()
        }
        .bounded();
        assert_eq!(config.global_joins_per_10s, 1);
        assert_eq!(config.global_connects_per_10s, 5);
        assert_eq!(config.max_channels_per_connection, 100);
        assert_eq!(config.message_queue_capacity, 1);
    }

    #[test]
    fn control_words_inside_messages_are_not_server_commands() {
        assert_eq!(
            irc_command("@id=x :u!u@u PRIVMSG #chan :please RECONNECT").0,
            "PRIVMSG"
        );
        assert_eq!(irc_command(":tmi.twitch.tv RECONNECT").0, "RECONNECT");
        assert_eq!(
            irc_command("@target-msg-id=x :tmi.twitch.tv CLEARMSG #chan :text").0,
            "CLEARMSG"
        );
    }

    async fn fixture() -> (TcpListener, AnonChatConfig) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = AnonChatConfig {
            irc_host: "127.0.0.1".into(),
            irc_port: listener.local_addr().unwrap().port(),
            join_stagger: Duration::from_millis(1),
            connect_backoff: Duration::from_millis(5),
            ..Default::default()
        };
        (listener, config)
    }

    async fn welcome(
        socket: TcpStream,
    ) -> (tokio::io::Lines<BufReader<OwnedReadHalf>>, OwnedWriteHalf) {
        let (read, mut write) = socket.into_split();
        let mut lines = BufReader::new(read).lines();
        let nick = lines.next_line().await.unwrap().unwrap();
        let cap = lines.next_line().await.unwrap().unwrap();
        assert!(control_is_allowed(&format!("{nick}\r\n")));
        assert!(control_is_allowed(&format!("{cap}\r\n")));
        write
            .write_all(b":tmi.twitch.tv 001 justinfan :Welcome\r\n")
            .await
            .unwrap();
        (lines, write)
    }

    #[tokio::test]
    async fn ping_and_fragmented_message_survive_exhausted_join_budget_and_roster_update() {
        let (listener, mut config) = fixture().await;
        config.global_joins_per_10s = 1;
        let capture = Arc::new(Capture::default());
        let handle = AnonChatHandle::start(capture.clone(), config, Arc::new(TokioTaskSpawner));
        handle
            .set_channels(vec!["channel_a".into(), "channel_b".into()])
            .await;
        let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let (mut lines, mut writer) = welcome(socket).await;
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "JOIN #channel_a");
        writer
            .write_all(b"@room-id=1;user-id=2;id=x :u!u@u PRIVMSG #channel_a :hel")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        handle
            .set_channels(vec![
                "channel_a".into(),
                "channel_b".into(),
                "channel_c".into(),
            ])
            .await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        writer.write_all(b"lo world\r\nPING :tmi.twitch.tv\r\n@target-msg-id=x :tmi.twitch.tv CLEARMSG #channel_a :hello world\r\n@target-user-id=2 :tmi.twitch.tv CLEARCHAT #channel_a :u\r\n").await.unwrap();
        let pong = tokio::time::timeout(Duration::from_millis(500), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(pong, "PONG :tmi.twitch.tv");
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if capture.0.lock().unwrap().len() == 3 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let events = capture.0.lock().unwrap().clone();
        assert!(events[0].ends_with(":hello world"));
        assert_eq!(irc_command(&events[1]).0, "CLEARMSG");
        assert_eq!(irc_command(&events[2]).0, "CLEARCHAT");
        assert_eq!(handle.stats().joins_written, 1);
        assert_eq!(handle.stats().privmsgs_dropped, 0);
        handle.stop().await;
    }

    #[tokio::test]
    async fn repeated_roster_updates_do_not_keep_a_silent_connection_alive() {
        let (listener, mut config) = fixture().await;
        config.read_idle_before_ping = Duration::from_millis(50);
        config.read_deadline_after_ping = Duration::from_millis(50);
        let handle = AnonChatHandle::start(
            Arc::new(Capture::default()),
            config,
            Arc::new(TokioTaskSpawner),
        );
        handle.set_channels(vec!["channel_a".into()]).await;
        let (socket, _) = listener.accept().await.unwrap();
        let (mut lines, _writer) = welcome(socket).await;
        let updater = handle.clone();
        let updates = tokio::spawn(async move {
            for _ in 0..50 {
                updater.set_channels(vec!["channel_a".into()]).await;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        let ping = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let line = lines.next_line().await.unwrap().unwrap();
                if line.starts_with("PING ") {
                    break line;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(ping, "PING :tmi.twitch.tv");
        let (_second, _) = tokio::time::timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        assert!(handle.stats().reconnects >= 1);
        handle.stop().await;
        updates.abort();
    }

    #[tokio::test]
    async fn oversized_line_is_bounded_before_all_input_is_allocated() {
        let (listener, _) = fixture().await;
        let connecting = tokio::spawn(TcpStream::connect(listener.local_addr().unwrap()));
        let (server, _) = listener.accept().await.unwrap();
        let mut client = connecting.await.unwrap().unwrap();
        let writer = tokio::spawn(async move {
            client
                .write_all(&vec![b'a'; MAX_IRC_LINE_BYTES + 1024])
                .await
        });
        let (read, _write) = server.into_split();
        let mut reader = BufReader::new(read);
        let mut buffer = Vec::new();
        assert!(read_bounded_line(&mut reader, &mut buffer).await.is_err());
        assert!(buffer.len() <= MAX_IRC_LINE_BYTES);
        writer.abort();
    }
}

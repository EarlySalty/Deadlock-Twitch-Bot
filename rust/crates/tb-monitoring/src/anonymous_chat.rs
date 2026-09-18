//! Shared anonymous IRC read transport, extracted from tb-bot/scout_chat.
//! No token, ChatApi, arbitrary write method, or moderation hook is accepted.
//! Membership is incremental; existing channels retain their shard assignment.

use std::{collections::BTreeSet, sync::{Arc, atomic::{AtomicU64, Ordering}}, time::Duration};
use chrono::{DateTime, Utc};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpStream, tcp::{OwnedReadHalf, OwnedWriteHalf}}, sync::{mpsc, watch, Mutex}, task::JoinSet, time::Instant};

const JOIN_SPACING: Duration = Duration::from_millis(600);
const BACKOFF: Duration = Duration::from_secs(30);
pub const MAX_CHANNELS_PER_SHARD: usize = 100;

#[derive(Default)]
pub struct ReadStats {
    pub connected_shards: AtomicU64,
    pub confirmed_channels: AtomicU64,
    pub received_messages: AtomicU64,
    pub dropped_events: AtomicU64,
    pub reconnects: AtomicU64,
}

#[derive(Debug)]
pub struct ReadEvent {
    pub received_at: DateTime<Utc>,
    pub channel: String,
    pub line: String,
}

pub struct AnonymousChat {
    roster: watch::Sender<Vec<String>>,
    pub stats: Arc<ReadStats>,
}

impl AnonymousChat {
    pub fn start(buffer: usize) -> (Self, mpsc::Receiver<ReadEvent>) {
        let (roster, rx) = watch::channel(Vec::new());
        let (events, received) = mpsc::channel(buffer.clamp(100, 50_000));
        let stats = Arc::new(ReadStats::default());
        tokio::spawn(coordinate(rx, events, stats.clone()));
        (Self { roster, stats }, received)
    }

    pub fn set_channels(&self, channels: &[String]) {
        self.roster.send_replace(normalize_channels(channels));
    }

    pub fn join_channels(&self, channels: &[String]) {
        self.roster.send_modify(|roster| {
            roster.extend(normalize_channels(channels));
            roster.sort_unstable();
            roster.dedup();
        });
    }

    pub fn part_channels(&self, channels: &[String]) {
        let removed = normalize_channels(channels);
        self.roster.send_modify(|roster| roster.retain(|c| !removed.contains(c)));
    }
}

pub fn valid_channel(channel: &str) -> bool {
    !channel.is_empty() && channel.len() <= 25
        && channel.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

pub fn normalize_channels(channels: &[String]) -> Vec<String> {
    let mut result: Vec<_> = channels.iter().map(|c| c.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|c| valid_channel(c)).collect();
    result.sort_unstable();
    result.dedup();
    result
}

/// Retain assignments before filling holes: adding an early-sorting login must
/// not PART/JOIN the rest of the category.
pub fn stable_shards(previous: &[Vec<String>], wanted: &[String]) -> Vec<Vec<String>> {
    let mut remaining: BTreeSet<_> = normalize_channels(wanted).into_iter().collect();
    let mut shards: Vec<Vec<String>> = previous.iter().map(|shard| shard.iter()
        .filter(|c| remaining.remove(*c)).cloned().collect()).collect();
    for channel in remaining {
        if let Some(shard) = shards.iter_mut().find(|s| s.len() < MAX_CHANNELS_PER_SHARD) {
            shard.push(channel);
        } else {
            shards.push(vec![channel]);
        }
    }
    // Keep internal empty shards to retain indices; trailing empty connections close.
    while shards.last().is_some_and(Vec::is_empty) { shards.pop(); }
    shards
}

pub fn anonymous_nick(shard: usize) -> String {
    // A process-specific identity avoids collision with the existing scout.
    format!("justinfan{}{:04}", std::process::id(), shard)
}

async fn coordinate(mut roster: watch::Receiver<Vec<String>>, events: mpsc::Sender<ReadEvent>, stats: Arc<ReadStats>) {
    let limiter = Arc::new(Mutex::new(Instant::now()));
    let mut tasks = JoinSet::new();
    let mut assignments = Vec::new();
    let mut senders: Vec<watch::Sender<Vec<String>>> = Vec::new();
    loop {
        tokio::select! {
            changed = roster.changed() => {
                if changed.is_err() { break; }
                assignments = stable_shards(&assignments, &roster.borrow_and_update());
                // Reuse empty connections rather than growing tasks on every roster update.
                while senders.len() < assignments.len() {
                    let index = senders.len();
                    let (tx, rx) = watch::channel(Vec::new());
                    tasks.spawn(run_shard(index, rx, events.clone(), stats.clone(), limiter.clone()));
                    senders.push(tx);
                }
                for (index, sender) in senders.iter().enumerate() {
                    sender.send_replace(assignments.get(index).cloned().unwrap_or_default());
                }
            }
            completed = tasks.join_next(), if !tasks.is_empty() => {
                // A terminated shard is not silently treated as coverage.
                tracing::error!(?completed, "anonymous IRC worker exited; coordinator stopping");
                break;
            }
            _ = events.closed() => break,
        }
    }
    tasks.abort_all();
}

async fn run_shard(index: usize, mut roster: watch::Receiver<Vec<String>>, events: mpsc::Sender<ReadEvent>, stats: Arc<ReadStats>, limiter: Arc<Mutex<Instant>>) {
    let mut attempted = false;
    loop {
        while roster.borrow().is_empty() {
            if roster.changed().await.is_err() { return; }
        }
        if attempted { stats.reconnects.fetch_add(1, Ordering::Relaxed); }
        attempted = true;
        if let Some((reader, writer)) = connect(&anonymous_nick(index)).await {
            serve(reader, writer, &mut roster, &events, &stats, &limiter).await;
        }
        // All disconnect paths back off, including a server closing immediately.
        let deadline = tokio::time::sleep(BACKOFF);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                changed = roster.changed() => if changed.is_err() { return; },
                _ = events.closed() => return,
            }
        }
    }
}

fn handshake(nick: &str) -> Option<[String; 2]> {
    if !nick.starts_with("justinfan") || !nick[9..].bytes().all(|b| b.is_ascii_digit()) || nick.len() <= 9 { return None; }
    Some([format!("NICK {nick}\r\n"), "CAP REQ :twitch.tv/tags twitch.tv/commands\r\n".into()])
}

async fn connect(nick: &str) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
    let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(("irc.chat.twitch.tv", 6667))).await.ok()?.ok()?;
    let (read, mut write) = stream.into_split();
    for command in handshake(nick)? {
        tokio::time::timeout(Duration::from_secs(5), write.write_all(command.as_bytes())).await.ok()?.ok()?;
    }
    let mut reader = BufReader::new(read);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut line = String::new();
        let n = tokio::time::timeout_at(deadline, reader.read_line(&mut line)).await.ok()?.ok()?;
        if n == 0 || line.len() > 16_384 { return None; }
        if line.starts_with(":tmi.twitch.tv 001") { return Some((reader, write)); }
        if line.starts_with("PING ") && !pong(&mut write, line.trim_end()).await { return None; }
    }
}

struct ConnectedGauge<'a> { stats: &'a ReadStats, confirmed: BTreeSet<String> }
impl Drop for ConnectedGauge<'_> {
    fn drop(&mut self) {
        self.stats.connected_shards.fetch_sub(1, Ordering::Relaxed);
        self.stats.confirmed_channels.fetch_sub(self.confirmed.len() as u64, Ordering::Relaxed);
    }
}

async fn join_slot(limiter: &Mutex<Instant>) {
    let mut next = limiter.lock().await;
    tokio::time::sleep_until(*next).await;
    *next = Instant::now() + JOIN_SPACING;
}

async fn serve(reader: BufReader<OwnedReadHalf>, mut writer: OwnedWriteHalf, roster: &mut watch::Receiver<Vec<String>>, events: &mpsc::Sender<ReadEvent>, stats: &ReadStats, limiter: &Mutex<Instant>) {
    stats.connected_shards.fetch_add(1, Ordering::Relaxed);
    let mut gauge = ConnectedGauge { stats, confirmed: BTreeSet::new() };
    let mut desired: BTreeSet<String> = roster.borrow_and_update().iter().cloned().collect();
    let mut joined = BTreeSet::new();
    let mut lines = reader.lines(); // next_line is cancellation-safe inside select.
    loop {
        let pending = desired.difference(&joined).next().cloned();
        tokio::select! {
            line = tokio::time::timeout(Duration::from_secs(360), lines.next_line()) => {
                let Ok(Ok(Some(line))) = line else { return; };
                if line.len() > 16_384 { return; }
                if line.starts_with("PING ") {
                    if !pong(&mut writer, &line).await { return; }
                    continue;
                }
                let Some((verb, channel)) = command_channel(&line) else { continue; };
                if verb == "RECONNECT" { return; }
                if !desired.contains(channel) || !joined.contains(channel) { continue; }
                if verb == "ROOMSTATE" && gauge.confirmed.insert(channel.to_owned()) {
                    stats.confirmed_channels.fetch_add(1, Ordering::Relaxed);
                }
                if matches!(verb, "PRIVMSG" | "CLEARMSG" | "CLEARCHAT") {
                    if verb == "PRIVMSG" { stats.received_messages.fetch_add(1, Ordering::Relaxed); }
                    let event = ReadEvent { received_at: Utc::now(), channel: channel.to_owned(), line };
                    if let Err(error) = events.try_send(event) {
                        if matches!(error, mpsc::error::TrySendError::Closed(_)) { return; }
                        stats.dropped_events.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            changed = roster.changed() => {
                if changed.is_err() { return; }
                desired = roster.borrow_and_update().iter().cloned().collect();
                let removed: Vec<_> = joined.difference(&desired).cloned().collect();
                for channel in removed {
                    if !membership(&mut writer, false, &channel).await { return; }
                    joined.remove(&channel);
                    if gauge.confirmed.remove(&channel) { stats.confirmed_channels.fetch_sub(1, Ordering::Relaxed); }
                }
                if desired.is_empty() { return; }
            }
            _ = join_slot(limiter), if pending.is_some() => {
                let channel = pending.expect("pending join");
                if !membership(&mut writer, true, &channel).await { return; }
                joined.insert(channel);
            }
            _ = events.closed() => return,
        }
    }
}

pub fn command_channel(line: &str) -> Option<(&str, &str)> {
    let rest = if line.starts_with('@') { line.split_once(' ')?.1 } else { line };
    let rest = if rest.starts_with(':') { rest.split_once(' ')?.1 } else { rest };
    let mut words = rest.split_whitespace();
    let verb = words.next()?;
    if verb == "RECONNECT" { return Some((verb, "")); }
    let channel = words.next()?.strip_prefix('#')?;
    valid_channel(channel).then_some((verb, channel))
}

async fn membership(writer: &mut OwnedWriteHalf, join: bool, channel: &str) -> bool {
    if !valid_channel(channel) { return false; }
    let verb = if join { "JOIN" } else { "PART" };
    tokio::time::timeout(Duration::from_secs(5), writer.write_all(format!("{verb} #{channel}\r\n").as_bytes())).await.is_ok_and(|r| r.is_ok())
}

async fn pong(writer: &mut OwnedWriteHalf, ping: &str) -> bool {
    let Some(payload) = ping.strip_prefix("PING ") else { return false; };
    if payload.contains(['\r', '\n']) { return false; }
    tokio::time::timeout(Duration::from_secs(5), writer.write_all(format!("PONG {payload}\r\n").as_bytes())).await.is_ok_and(|r| r.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_anonymous_handshake_and_no_command_injection() {
        assert!(handshake("real_bot").is_none());
        assert!(handshake("justinfan1\r\nPRIVMSG #x :bad").is_none());
        let commands = handshake(&anonymous_nick(1)).unwrap().join("");
        for forbidden in ["PASS", "oauth:", "PRIVMSG", "WHISPER"] { assert!(!commands.contains(forbidden)); }
        assert!(normalize_channels(&["#Good_name".into(), "bad\r\nJOIN #evil".into(), "élise".into()]) == ["good_name"]);
    }
    #[test]
    fn category_churn_preserves_shards_and_has_no_loss() {
        let channels: Vec<_> = (0..301).map(|i| format!("u{i:04}")).collect();
        let before = stable_shards(&[], &channels);
        assert_eq!(before.len(), 4);
        let mut wanted = channels.clone();
        wanted.retain(|c| c != "u0001");
        wanted.push("aaaa".into());
        let after = stable_shards(&before, &wanted);
        assert_eq!(before[1..], after[1..]);
        let flat: BTreeSet<_> = after.iter().flatten().cloned().collect();
        assert_eq!(flat, wanted.into_iter().collect());
        assert!(after.iter().all(|s| s.len() <= 100));
    }
    #[test]
    fn read_events_include_deletions_but_not_fake_commands_in_text() {
        assert_eq!(command_channel("@room-id=1 :tmi.twitch.tv CLEARCHAT #test :user"), Some(("CLEARCHAT", "test")));
        assert_eq!(command_channel(":x!x@x PRIVMSG #test :RECONNECT #evil"), Some(("PRIVMSG", "test")));
    }
    #[tokio::test]
    async fn socket_transport_only_sends_membership_and_pong() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (read, write) = client.into_split();
        let (server_read, mut server_write) = server.into_split();
        let mut commands = BufReader::new(server_read).lines();
        let (roster, mut desired) = watch::channel(vec!["safe".into()]);
        let (events, mut received) = mpsc::channel(10);
        let stats = Arc::new(ReadStats::default());
        let task_stats = stats.clone();
        let task = tokio::spawn(async move {
            serve(BufReader::new(read), write, &mut desired, &events, &task_stats, &Mutex::new(Instant::now())).await;
        });
        assert_eq!(commands.next_line().await.unwrap().as_deref(), Some("JOIN #safe"));
        server_write.write_all(b"@room-id=1 :tmi.twitch.tv ROOMSTATE #safe\r\n@room-id=1;user-id=2;id=test :user!u@u PRIVMSG #safe :please send a message\r\nPING :tmi.twitch.tv\r\n").await.unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), received.recv()).await.unwrap().unwrap();
        assert_eq!(event.channel, "safe");
        assert_eq!(commands.next_line().await.unwrap().as_deref(), Some("PONG :tmi.twitch.tv"));
        assert_eq!(stats.confirmed_channels.load(Ordering::Relaxed), 1);
        roster.send_replace(Vec::new());
        assert_eq!(commands.next_line().await.unwrap().as_deref(), Some("PART #safe"));
        task.await.unwrap();
        assert_eq!(stats.connected_shards.load(Ordering::Relaxed), 0);
        assert_eq!(stats.confirmed_channels.load(Ordering::Relaxed), 0);
        assert_eq!(commands.next_line().await.unwrap(), None);
    }
}

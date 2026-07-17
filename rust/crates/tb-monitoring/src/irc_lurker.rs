//! Experimenteller IRC-Lurker-Tracker (Port von `bot/chat/irc_lurker_tracker.py`).
//!
//! Zweite, **ergänzende** Presence-Quelle neben dem primären Helix
//! `Get Chatters`: eine separate IRC-Verbindung liest JOIN/PART/NAMES und
//! spiegelt die beobachteten Chatter in `twitch_session_chatters` (für die
//! aktive Session des Kanals). Bewusst **default-AUS** und im Binary per
//! `TB_IRC_LURKER_ENABLED=1` aktivierbar.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Known-Chat-Bots (chat_bots.py Z. 8–19). Lokale Kopie wie in den anderen Crates.
const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

fn is_known_chat_bot(login: &str) -> bool {
    KNOWN_CHAT_BOTS.contains(&login)
}

// ---- IRC-Zeilen-Parser (pur, ohne Socket testbar) --------------------------

/// `:nick!user@host JOIN #channel` → `(nick, channel)`.
pub fn parse_join(line: &str) -> Option<(String, String)> {
    parse_membership(line, "JOIN")
}

/// `:nick!user@host PART #channel` → `(nick, channel)`.
pub fn parse_part(line: &str) -> Option<(String, String)> {
    parse_membership(line, "PART")
}

/// `:nick!user@host PRIVMSG #channel :text` → `(nick, channel)`.
pub fn parse_privmsg(line: &str) -> Option<(String, String)> {
    parse_membership(line, "PRIVMSG")
}

fn parse_membership(line: &str, verb: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix(':')?;
    let (nick, after) = rest.split_once('!')?;
    if nick.is_empty() || !nick.chars().all(is_irc_word) {
        return None;
    }
    let marker = format!(" {verb} #");
    let idx = after.find(&marker)?;
    let channel: String = after[idx + marker.len()..]
        .chars()
        .take_while(|c| is_irc_word(*c))
        .collect();
    if channel.is_empty() {
        return None;
    }
    Some((nick.to_string(), channel))
}

/// NAMES-Reply `:server 353 nick = #channel :n1 n2 n3` → `(channel, [nicks])`.
pub fn parse_names(line: &str) -> Option<(String, Vec<String>)> {
    let after = &line[line.find(" 353 ")? + 5..];
    let chan_and_nicks = &after[after.find(" = #")? + 4..];
    let (channel_raw, nicks_str) = chan_and_nicks.split_once(" :")?;
    let channel: String = channel_raw
        .chars()
        .take_while(|c| is_irc_word(*c))
        .collect();
    if channel.is_empty() {
        return None;
    }
    let nicks = nicks_str.split_whitespace().map(str::to_string).collect();
    Some((channel, nicks))
}

fn is_irc_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn normalize_channel(channel: &str) -> String {
    channel
        .trim()
        .to_lowercase()
        .trim_start_matches('#')
        .to_string()
}

// ---- DB-Layer --------------------------------------------------------------

/// Aktive Session-ID eines live Kanals (`twitch_live_state`), sonst `None`.
async fn resolve_active_session(pool: &PgPool, channel: &str) -> Result<Option<i64>, sqlx::Error> {
    Ok(sqlx::query_scalar!(
        "SELECT active_session_id FROM twitch_live_state \
         WHERE LOWER(streamer_login) = $1 AND is_live = 1",
        channel,
    )
    .fetch_optional(pool)
    .await?
    .flatten())
}

const INSERT_CHATTER: &str = "INSERT INTO twitch_session_chatters \
    (session_id, streamer_login, chatter_login, chatter_id, first_message_at, \
     messages, is_first_time_streamer, seen_via_chatters_api, last_seen_at) \
    VALUES ($1, $2, $3, NULL, $4, 0, FALSE, TRUE, $4)";

/// JOIN-Event: aktualisiert `last_seen_at` (Upsert; Python `_update_chatter_seen`).
/// No-op für bekannte Bots oder ohne aktive Session.
pub async fn upsert_chatter_seen(pool: &PgPool, channel: &str, nick: &str, now: DateTime<Utc>) {
    let nick = nick.to_lowercase();
    if is_known_chat_bot(&nick) {
        return;
    }
    let session_id = match resolve_active_session(pool, channel).await {
        Ok(Some(session_id)) => session_id,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(
                %error,
                channel,
                nick = %nick,
                "IRC-Lurker: aktive Session-Query fehlgeschlagen"
            );
            return;
        }
    };
    // dyn: wiederverwendeter INSERT-Grundkörper mit variierendem ON-CONFLICT-Zweig.
    match sqlx::query(&format!(
        "{INSERT_CHATTER} ON CONFLICT (session_id, chatter_login) \
         DO UPDATE SET last_seen_at = EXCLUDED.last_seen_at"
    ))
    .bind(session_id)
    .bind(channel)
    .bind(&nick)
    .bind(now)
    .execute(pool)
    .await
    {
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                %error,
                channel,
                nick = %nick,
                session_id,
                "IRC-Lurker: JOIN-Upsert fehlgeschlagen"
            );
        }
    }
}

/// NAMES-Liste: spiegelt alle Chatter der Session (Python `_on_names_list`).
/// Bekannte Bots gefiltert; vorhandene → `last_seen_at`-Update, neue → Insert.
/// Liefert `(inserts, updates)`. No-op ohne aktive Session.
pub async fn upsert_names_batch(
    pool: &PgPool,
    channel: &str,
    nicks: &[String],
    now: DateTime<Utc>,
) -> (usize, usize) {
    let session_id = match resolve_active_session(pool, channel).await {
        Ok(Some(session_id)) => session_id,
        Ok(None) => return (0, 0),
        Err(error) => {
            tracing::warn!(
                %error,
                channel,
                nick_count = nicks.len(),
                "IRC-Lurker: aktive Session-Query fuer NAMES fehlgeschlagen"
            );
            return (0, 0);
        }
    };
    let existing: HashSet<String> = match sqlx::query_scalar!(
        "SELECT chatter_login FROM twitch_session_chatters WHERE session_id = $1",
        session_id,
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows.into_iter().collect(),
        Err(error) => {
            tracing::warn!(
                %error,
                channel,
                session_id,
                "IRC-Lurker: NAMES-existing-SELECT fehlgeschlagen"
            );
            return (0, 0);
        }
    };

    let mut inserts = 0;
    let mut updates = 0;
    for nick in nicks {
        let nick = nick.to_lowercase();
        if is_known_chat_bot(&nick) {
            continue;
        }
        if existing.contains(&nick) {
            match sqlx::query!(
                "UPDATE twitch_session_chatters SET last_seen_at = $1 \
                 WHERE session_id = $2 AND chatter_login = $3",
                now,
                session_id,
                &nick,
            )
            .execute(pool)
            .await
            {
                Ok(_) => {
                    updates += 1;
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        channel,
                        nick = %nick,
                        session_id,
                        "IRC-Lurker: NAMES-Update fehlgeschlagen"
                    );
                }
            }
        } else {
            // dyn: gleicher INSERT-Grundkörper wie JOIN-Pfad, aber DO NOTHING statt UPDATE.
            match sqlx::query(&format!(
                "{INSERT_CHATTER} ON CONFLICT (session_id, chatter_login) DO NOTHING"
            ))
            .bind(session_id)
            .bind(channel)
            .bind(&nick)
            .bind(now)
            .execute(pool)
            .await
            {
                Ok(_) => {
                    inserts += 1;
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        channel,
                        nick = %nick,
                        session_id,
                        "IRC-Lurker: NAMES-Insert fehlgeschlagen"
                    );
                }
            }
        }
    }
    (inserts, updates)
}

/// Schreibt pro Poll-Tick einen Presence-Tick je Chatter
/// (Python `bot/analytics/mixin.py:1647-1658`). Liefert die Roh-Zeitreihe für
/// Anwesenheits-/Watchtime-Analysen (`viewer_timeline`).
///
/// `ON CONFLICT (session_id, viewer_login, tick_at) DO NOTHING` → idempotent,
/// falls derselbe Tick (gleicher `tick_at`) doppelt verarbeitet wird. `tick_at`
/// ist `TIMESTAMPTZ` (clean-SQL — kein TEXT-Timestamp). Liefert die Anzahl
/// tatsächlich neu eingefügter Zeilen.
///
/// WIRING-TODO(P1.23): Aus dem produktiven Chatters-Poll-Tick aufrufen — pro
/// laufender Session einmal je 30s-Tick mit den aktuellen Chatter-Logins
/// (Bot-Filter erfolgt bereits upstream beim Befüllen von
/// `twitch_session_chatters`). In `bin/tb-bot` neben den 300s-Jobs verdrahten.
pub async fn record_presence_ticks(
    pool: &PgPool,
    session_id: i64,
    streamer_login: &str,
    viewer_logins: &[String],
    tick_at: DateTime<Utc>,
) -> u64 {
    if viewer_logins.is_empty() {
        return 0;
    }
    let streamer = streamer_login.to_lowercase();
    let mut inserted = 0u64;
    for viewer in viewer_logins {
        let viewer = viewer.to_lowercase();
        let res = sqlx::query!(
            "INSERT INTO twitch_viewer_presence_ticks \
             (session_id, streamer_login, viewer_login, tick_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (session_id, viewer_login, tick_at) DO NOTHING",
            session_id,
            &streamer,
            &viewer,
            tick_at,
        )
        .execute(pool)
        .await;
        if let Ok(done) = res {
            inserted += done.rows_affected();
        }
    }
    inserted
}

// ---- Async-Tracker (Verbindungs-Loop + dynamisches Channel-Tracking) -------
//
// HINWEIS zum An/Aus-Zustand: In Python ist `experimental_irc_lurker_enabled`
// hartcodiert `False` (runtime_bootstrap.py) und wird NIE auf True gesetzt.
// Der Rust-Port wird in `tb-bot` weiterhin default-aus gehalten und nur per
// `TB_IRC_LURKER_ENABLED=1` als anonymer justinfan-Presence-Sammler gespawnt.

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;
const NAMES_POLL_SECONDS: u64 = 120;
const CONNECT_BACKOFF_SECONDS: u64 = 30;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// 600ms/JOIN = max ~16 JOINs/10s, sicher unter Twitchs Anonym-/User-Limit von
// 20 JOINs pro 10s (Ueberschreitung => Verbindungsabbruch + Reconnect-Schleife).
const IRC_JOIN_STAGGER: Duration = Duration::from_millis(600);

/// Klassifizierung einer getrackten Quelle (nur Metadaten; die Datensammlung
/// läuft für alle Kanäle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMode {
    Partner,
    Category,
}

/// Laufzeit-Kommando an den Verbindungs-Loop (JOIN/PART eines Kanals).
enum Cmd {
    Join(String),
    Part(String),
    Stop,
}

struct RaidWatch {
    refs: usize,
    owns_tracking: bool,
}

/// Experimenteller IRC-Lurker-Tracker. Erst nach `run()` aktiv; `track_channel`
/// kann jederzeit (auch vor `run`) Kanäle setzen.
pub struct IrcLurkerTracker {
    pool: PgPool,
    /// Vom Aufrufer übergeben (API-Parität zu Python `IRCLurkerTracker`), aber
    /// vom Twitch-IRC-Protokoll ungenutzt — Login läuft über PASS/NICK.
    #[allow(dead_code)]
    client_id: String,
    access_token: String,
    nick: String,
    authenticated: bool,
    channels: Arc<Mutex<HashSet<String>>>,
    partner_channels: Arc<Mutex<HashSet<String>>>,
    category_channels: Arc<Mutex<HashSet<String>>>,
    chatters: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    writers: Arc<Mutex<HashMap<String, HashMap<String, Instant>>>>,
    raid_watches: Arc<Mutex<HashMap<String, RaidWatch>>>,
    connected_since: Arc<Mutex<Option<Instant>>>,
    ready_channels: Arc<Mutex<HashSet<String>>>,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    cmd_rx: Mutex<Option<mpsc::UnboundedReceiver<Cmd>>>,
    stop_requested: Arc<AtomicBool>,
}

impl IrcLurkerTracker {
    /// `nick`/`access_token` leer → anonymer `justinfan`-Login.
    pub fn new(
        pool: PgPool,
        client_id: String,
        access_token: String,
        nick: Option<String>,
    ) -> Self {
        let nick_norm = nick
            .map(|n| n.trim().to_lowercase())
            .filter(|n| !n.is_empty());
        let access_token = access_token.trim().to_string();
        let authenticated = !access_token.is_empty() && nick_norm.is_some();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        Self {
            pool,
            client_id,
            access_token,
            nick: nick_norm.unwrap_or_else(|| "justinfan12345".to_string()),
            authenticated,
            channels: Arc::new(Mutex::new(HashSet::new())),
            partner_channels: Arc::new(Mutex::new(HashSet::new())),
            category_channels: Arc::new(Mutex::new(HashSet::new())),
            chatters: Arc::new(Mutex::new(HashMap::new())),
            writers: Arc::new(Mutex::new(HashMap::new())),
            raid_watches: Arc::new(Mutex::new(HashMap::new())),
            connected_since: Arc::new(Mutex::new(None)),
            ready_channels: Arc::new(Mutex::new(HashSet::new())),
            cmd_tx,
            cmd_rx: Mutex::new(Some(cmd_rx)),
            stop_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Fügt einen Kanal zur Verfolgung hinzu (Python `track_channel`). Bei
    /// laufender Verbindung wird er sofort gejoint.
    pub fn track_channel(&self, channel: &str, mode: TrackMode) {
        let channel = channel
            .trim()
            .to_lowercase()
            .trim_start_matches('#')
            .to_string();
        if channel.is_empty() {
            return;
        }
        lock_or_recover(&self.channels, "channels").insert(channel.clone());
        {
            let mut partner = lock_or_recover(&self.partner_channels, "partner_channels");
            let mut category = lock_or_recover(&self.category_channels, "category_channels");
            match mode {
                TrackMode::Partner => {
                    partner.insert(channel.clone());
                    category.remove(&channel);
                }
                TrackMode::Category => {
                    partner.remove(&channel);
                    category.insert(channel.clone());
                    lock_or_recover(&self.chatters, "chatters").remove(&channel);
                }
            }
        }
        let _ = self.cmd_tx.send(Cmd::Join(channel));
    }

    /// Entfernt einen Kanal (Python `untrack_channel`).
    pub fn untrack_channel(&self, channel: &str) {
        let channel = channel
            .trim()
            .to_lowercase()
            .trim_start_matches('#')
            .to_string();
        if !lock_or_recover(&self.channels, "channels").remove(&channel) {
            return;
        }
        lock_or_recover(&self.partner_channels, "partner_channels").remove(&channel);
        lock_or_recover(&self.category_channels, "category_channels").remove(&channel);
        lock_or_recover(&self.chatters, "chatters").remove(&channel);
        lock_or_recover(&self.writers, "writers").remove(&channel);
        lock_or_recover(&self.ready_channels, "ready_channels").remove(&channel);
        let _ = self.cmd_tx.send(Cmd::Part(channel));
    }

    /// Aktuell beobachtete Chatter eines Kanals (Python `get_chatters`).
    pub fn get_chatters(&self, channel: &str) -> HashSet<String> {
        let channel = channel
            .trim()
            .to_lowercase()
            .trim_start_matches('#')
            .to_string();
        self.chatters
            .lock()
            .unwrap_or_else(|poisoned| {
                tracing::warn!(
                    lock = "chatters",
                    "IRC-Lurker: Mutex war poisoned, nutze letzten Zustand weiter"
                );
                poisoned.into_inner()
            })
            .get(&channel)
            .cloned()
            .unwrap_or_default()
    }

    pub fn watch_raid_channel(&self, channel: &str) {
        let channel = normalize_channel(channel);
        if channel.is_empty() {
            return;
        }
        let mut watches = lock_or_recover(&self.raid_watches, "raid_watches");
        if let Some(watch) = watches.get_mut(&channel) {
            watch.refs += 1;
            return;
        }
        let owns_tracking = !lock_or_recover(&self.channels, "channels").contains(&channel);
        watches.insert(
            channel.clone(),
            RaidWatch {
                refs: 1,
                owns_tracking,
            },
        );
        drop(watches);
        if owns_tracking {
            self.track_channel(&channel, TrackMode::Category);
        }
    }

    pub fn unwatch_raid_channel(&self, channel: &str) {
        let channel = normalize_channel(channel);
        let owns_tracking = {
            let mut watches = lock_or_recover(&self.raid_watches, "raid_watches");
            let Some(watch) = watches.get_mut(&channel) else {
                return;
            };
            if watch.refs > 1 {
                watch.refs -= 1;
                return;
            }
            watches
                .remove(&channel)
                .is_some_and(|watch| watch.owns_tracking)
        };
        // Raid-Beobachtung beendet: Schreiber-Puffer dieses Ziels freigeben (auch
        // wenn der Kanal als Partner weiter getrackt bleibt und nicht gepartet wird).
        lock_or_recover(&self.writers, "writers").remove(&channel);
        if owns_tracking
            && !lock_or_recover(&self.partner_channels, "partner_channels").contains(&channel)
        {
            self.untrack_channel(&channel);
        }
    }

    /// Ob der Nick seit Beginn des Trackings im Kanal geschrieben hat.
    pub fn has_written(&self, channel: &str, nick: &str) -> bool {
        let channel = channel
            .trim()
            .to_lowercase()
            .trim_start_matches('#')
            .to_string();
        let nick = nick.trim().to_lowercase();
        lock_or_recover(&self.writers, "writers")
            .get(&channel)
            .is_some_and(|writers| writers.contains_key(&nick))
    }

    pub fn has_written_since(&self, channel: &str, nick: &str, since: Instant) -> Option<bool> {
        let channel = normalize_channel(channel);
        let connected = lock_or_recover(&self.connected_since, "connected_since")
            .is_some_and(|connected_at| connected_at <= since);
        if !connected || !lock_or_recover(&self.ready_channels, "ready_channels").contains(&channel)
        {
            return None;
        }
        let nick = nick.trim().to_lowercase();
        Some(
            lock_or_recover(&self.writers, "writers")
                .get(&channel)
                .and_then(|writers| writers.get(&nick))
                .is_some_and(|written_at| *written_at >= since),
        )
    }

    fn record_privmsg(&self, msg: &str) -> bool {
        let Some((nick, channel)) = parse_privmsg(msg) else {
            return false;
        };
        let channel = channel.to_lowercase();
        if !lock_or_recover(&self.channels, "channels").contains(&channel) {
            return false;
        }
        lock_or_recover(&self.ready_channels, "ready_channels").insert(channel.clone());
        // ponytail: Schreiber nur für aktive Raid-Ziele puffern; sonst wüchse die
        // writers-Map für dauerhaft getrackte Kanäle je Schreiber unbegrenzt (24/7-Prozess).
        if lock_or_recover(&self.raid_watches, "raid_watches").contains_key(&channel) {
            lock_or_recover(&self.writers, "writers")
                .entry(channel)
                .or_default()
                .insert(nick.to_lowercase(), Instant::now());
        }
        true
    }

    /// Fordert einen sauberen Stopp des Verbindungs-Loops an.
    ///
    /// Python `IRCLurkerTracker.stop()` cancelt Connection-/Read-/Poll-Tasks und
    /// trennt die Verbindung. Der Rust-Tracker besitzt nur einen Loop; dieses
    /// Signal beendet ihn beim nächsten Select-/Backoff-Punkt.
    pub fn stop(&self) {
        self.stop_requested.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(Cmd::Stop);
    }

    /// Verbindungs-Loop mit Auto-Reconnect. Läuft bis zum Programmende.
    pub async fn run(&self) {
        let Some(mut rx) = lock_or_recover(&self.cmd_rx, "cmd_rx").take() else {
            tracing::warn!("IRC-Lurker: run() bereits aktiv");
            return;
        };
        while !self.stop_requested.load(Ordering::SeqCst) {
            match self.connect().await {
                Some((reader, writer)) => {
                    *lock_or_recover(&self.connected_since, "connected_since") =
                        Some(Instant::now());
                    lock_or_recover(&self.ready_channels, "ready_channels").clear();
                    self.serve(reader, writer, &mut rx).await;
                    *lock_or_recover(&self.connected_since, "connected_since") = None;
                    lock_or_recover(&self.ready_channels, "ready_channels").clear();
                }
                None => {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(CONNECT_BACKOFF_SECONDS)) => {}
                        cmd = rx.recv() => {
                            if self.handle_disconnected_cmd(cmd) {
                                break;
                            }
                        }
                    }
                }
            }
        }
        tracing::info!("IRC-Lurker: Stop-Signal verarbeitet, Runner beendet");
    }

    async fn connect(&self) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
        let stream = match TcpStream::connect((IRC_HOST, IRC_PORT)).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "IRC-Lurker: Connect fehlgeschlagen");
                return None;
            }
        };
        let (rd, mut wr) = stream.into_split();
        for command in self.handshake_commands() {
            if let Err(error) = wr.write_all(command.as_bytes()).await {
                tracing::warn!(%error, "IRC-Lurker: Handshake-Write fehlgeschlagen");
                return None;
            }
        }
        if let Err(error) = wr.flush().await {
            tracing::warn!(%error, "IRC-Lurker: Handshake-Flush fehlgeschlagen");
            return None;
        }

        let mut reader = BufReader::new(rd);
        let mut line = String::new();
        loop {
            line.clear();
            let n = match tokio::time::timeout(CONNECT_TIMEOUT, reader.read_line(&mut line)).await {
                Ok(Ok(n)) => n,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "IRC-Lurker: Handshake-Read fehlgeschlagen");
                    return None;
                }
                Err(_) => {
                    tracing::warn!("IRC-Lurker: Handshake-Timeout");
                    return None;
                }
            };
            if n == 0 {
                return None;
            }
            let msg = line.trim_end();
            if msg.starts_with(":tmi.twitch.tv 001") {
                tracing::info!(authenticated = self.authenticated, nick = %self.nick, "IRC-Lurker verbunden");
                return Some((reader, wr));
            }
            if msg.starts_with("PING") {
                pong(&mut wr, msg).await;
            }
        }
    }

    async fn serve(
        &self,
        mut reader: BufReader<OwnedReadHalf>,
        mut writer: OwnedWriteHalf,
        rx: &mut mpsc::UnboundedReceiver<Cmd>,
    ) {
        let channels: Vec<String> = lock_or_recover(&self.channels, "channels")
            .iter()
            .cloned()
            .collect();
        for ch in channels {
            write_irc_line(&mut writer, &format!("JOIN #{ch}\r\n"), "JOIN", Some(&ch)).await;
            tokio::time::sleep(IRC_JOIN_STAGGER).await;
        }
        let mut poll = tokio::time::interval(Duration::from_secs(NAMES_POLL_SECONDS));
        poll.tick().await; // erster Tick sofort — überspringen.

        let mut line = String::new();
        loop {
            if self.stop_requested.load(Ordering::SeqCst) {
                break;
            }
            line.clear();
            tokio::select! {
                res = reader.read_line(&mut line) => {
                    match res {
                        Ok(0) => break,
                        Err(error) => {
                            tracing::warn!(%error, "IRC-Lurker: Read fehlgeschlagen");
                            break;
                        }
                        Ok(_) => self.handle_line(line.trim_end(), &mut writer).await,
                    }
                }
                _ = poll.tick() => {
                    let chans: Vec<String> = lock_or_recover(&self.channels, "channels").iter().cloned().collect();
                    for ch in chans {
                        write_irc_line(&mut writer, &format!("NAMES #{ch}\r\n"), "NAMES", Some(&ch)).await;
                        tokio::time::sleep(IRC_JOIN_STAGGER).await;
                    }
                }
                Some(cmd) = rx.recv() => match cmd {
                    Cmd::Join(ch) => { write_irc_line(&mut writer, &format!("JOIN #{ch}\r\n"), "JOIN", Some(&ch)).await; }
                    Cmd::Part(ch) => { write_irc_line(&mut writer, &format!("PART #{ch}\r\n"), "PART", Some(&ch)).await; }
                    Cmd::Stop => {
                        self.stop_requested.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }
    }

    async fn handle_line(&self, msg: &str, writer: &mut OwnedWriteHalf) {
        if msg.is_empty() {
            return;
        }
        if msg.starts_with("PING") {
            pong(writer, msg).await;
            return;
        }
        let now = Utc::now();
        if let Some((nick, channel)) = parse_join(msg) {
            let (channel, nick) = (channel.to_lowercase(), nick.to_lowercase());
            lock_or_recover(&self.ready_channels, "ready_channels").insert(channel.clone());
            lock_or_recover(&self.chatters, "chatters")
                .entry(channel.clone())
                .or_default()
                .insert(nick.clone());
            upsert_chatter_seen(&self.pool, &channel, &nick, now).await;
        } else if let Some((nick, channel)) = parse_part(msg) {
            let (channel, nick) = (channel.to_lowercase(), nick.to_lowercase());
            if let Some(set) = lock_or_recover(&self.chatters, "chatters").get_mut(&channel) {
                set.remove(&nick);
            }
        } else if let Some((channel, nicks)) = parse_names(msg) {
            let channel = channel.to_lowercase();
            lock_or_recover(&self.ready_channels, "ready_channels").insert(channel.clone());
            let nicks_lower: Vec<String> = nicks.iter().map(|n| n.to_lowercase()).collect();
            // NAMES nur für Partner-Kanäle im Speicher halten (RAM-Schonung).
            if lock_or_recover(&self.partner_channels, "partner_channels").contains(&channel) {
                lock_or_recover(&self.chatters, "chatters")
                    .insert(channel.clone(), nicks_lower.iter().cloned().collect());
            } else {
                lock_or_recover(&self.chatters, "chatters").remove(&channel);
            }
            upsert_names_batch(&self.pool, &channel, &nicks_lower, now).await;
        } else {
            self.record_privmsg(msg);
        }
    }

    fn handle_disconnected_cmd(&self, cmd: Option<Cmd>) -> bool {
        match cmd {
            Some(Cmd::Stop) | None => {
                self.stop_requested.store(true, Ordering::SeqCst);
                true
            }
            Some(Cmd::Join(_)) | Some(Cmd::Part(_)) => false,
        }
    }

    fn handshake_commands(&self) -> Vec<String> {
        let mut commands = Vec::with_capacity(3);
        if self.authenticated {
            let clean = self.access_token.replace("oauth:", "");
            commands.push(format!("PASS oauth:{clean}\r\n"));
        }
        commands.push(format!("NICK {}\r\n", self.nick));
        commands.push("CAP REQ :twitch.tv/membership twitch.tv/commands\r\n".to_string());
        commands
    }
}

fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, name: &'static str) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        tracing::warn!(
            lock = name,
            "IRC-Lurker: Mutex war poisoned, nutze letzten Zustand weiter"
        );
        poisoned.into_inner()
    })
}

/// Antwortet auf einen IRC-PING.
async fn pong(writer: &mut OwnedWriteHalf, ping: &str) {
    let reply = ping.replacen("PING", "PONG", 1);
    if let Err(error) = writer.write_all(reply.as_bytes()).await {
        tracing::warn!(%error, "IRC-Lurker: PONG-Write fehlgeschlagen");
    }
    if let Err(error) = writer.write_all(b"\r\n").await {
        tracing::warn!(%error, "IRC-Lurker: PONG-Newline fehlgeschlagen");
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, "IRC-Lurker: PONG-Flush fehlgeschlagen");
    }
}

async fn write_irc_line(
    writer: &mut OwnedWriteHalf,
    line: &str,
    action: &'static str,
    channel: Option<&str>,
) {
    if let Err(error) = writer.write_all(line.as_bytes()).await {
        tracing::warn!(
            %error,
            action,
            channel = channel.unwrap_or(""),
            "IRC-Lurker: Write fehlgeschlagen"
        );
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(
            %error,
            action,
            channel = channel.unwrap_or(""),
            "IRC-Lurker: Flush fehlgeschlagen"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn join_part_parse() {
        let (n, c) = parse_join(":viewer!viewer@viewer.tmi.twitch.tv JOIN #nani").unwrap();
        assert_eq!(n, "viewer");
        assert_eq!(c, "nani");
        let (n, c) = parse_part(":lurker!x@y PART #drag").unwrap();
        assert_eq!(n, "lurker");
        assert_eq!(c, "drag");
        // Kein JOIN/PART → None.
        assert!(parse_join(":tmi.twitch.tv 001 nick :welcome").is_none());
        assert!(parse_part(":viewer!x@y JOIN #nani").is_none());
    }

    #[test]
    fn names_parse() {
        let line = ":tmi.twitch.tv 353 mybot = #nani :alice bob carol";
        let (c, nicks) = parse_names(line).unwrap();
        assert_eq!(c, "nani");
        assert_eq!(nicks, vec!["alice", "bob", "carol"]);
        // Kein 353 → None.
        assert!(parse_names(":tmi.twitch.tv 366 nick #nani :End of NAMES").is_none());
    }

    #[test]
    fn privmsg_parse() {
        let (nick, channel) =
            parse_privmsg(":Raider!raider@raider.tmi.twitch.tv PRIVMSG #Ziel :Hallo!").unwrap();
        assert_eq!(nick, "Raider");
        assert_eq!(channel, "Ziel");
        assert!(parse_privmsg(":viewer!x@y JOIN #ziel").is_none());
    }

    #[tokio::test]
    async fn privmsg_markiert_writer_ohne_presence_zu_veraendern() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        let tracker = IrcLurkerTracker::new(pool, String::new(), String::new(), None);
        tracker.watch_raid_channel("ziel");

        assert!(!tracker.has_written("ziel", "raider"));
        assert!(tracker.record_privmsg(":Raider!raider@raider.tmi.twitch.tv PRIVMSG #Ziel :Hallo!"));
        assert!(tracker.has_written(" #ZIEL ", "RAIDER"));
        assert!(tracker.get_chatters("ziel").is_empty());
    }

    #[tokio::test]
    async fn raid_probe_braucht_ununterbrochene_bestaetigte_verbindung() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        let tracker = IrcLurkerTracker::new(pool, String::new(), String::new(), None);
        let connected_at = std::time::Instant::now();
        *tracker.connected_since.lock().unwrap() = Some(connected_at);
        tracker.watch_raid_channel("ziel");
        let raid_started_at = std::time::Instant::now();

        assert_eq!(
            tracker.has_written_since("ziel", "raider", raid_started_at),
            None
        );
        tracker.ready_channels.lock().unwrap().insert("ziel".into());

        assert!(tracker.record_privmsg(":Raider!raider@raider.tmi.twitch.tv PRIVMSG #Ziel :Hallo!"));
        assert_eq!(
            tracker.has_written_since("ziel", "raider", raid_started_at),
            Some(true)
        );
        assert_eq!(
            tracker.has_written_since("ziel", "anderer", raid_started_at),
            Some(false)
        );

        *tracker.connected_since.lock().unwrap() = Some(std::time::Instant::now());
        assert_eq!(
            tracker.has_written_since("ziel", "raider", raid_started_at),
            None
        );
    }

    #[tokio::test]
    async fn raid_unwatch_erhaelt_partner_tracking() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        let tracker = IrcLurkerTracker::new(pool, String::new(), String::new(), None);
        tracker.track_channel("partner", TrackMode::Partner);

        tracker.watch_raid_channel("partner");
        tracker.unwatch_raid_channel("partner");

        assert!(tracker.channels.lock().unwrap().contains("partner"));
        assert!(tracker.partner_channels.lock().unwrap().contains("partner"));
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
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
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, is_live INTEGER DEFAULT 0, active_session_id BIGINT)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, \
             chatter_login TEXT, chatter_id TEXT, first_message_at TIMESTAMPTZ, messages INTEGER, \
             is_first_time_streamer BOOLEAN, seen_via_chatters_api BOOLEAN, last_seen_at TIMESTAMPTZ, \
             PRIMARY KEY (session_id, chatter_login))",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_viewer_presence_ticks (\
             session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL, \
             viewer_login TEXT NOT NULL, tick_at TIMESTAMPTZ NOT NULL, \
             PRIMARY KEY (session_id, viewer_login, tick_at))",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn names_batch_und_join_upsert() {
        let Some(pool) = make_pool("t_irc_lurker").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id) VALUES ('1', 'nani', 1, 42)")
            .execute(&pool).await.unwrap();
        let now = Utc::now();

        // NAMES: 3 echte Chatter + 1 Bot (gefiltert) → 3 Inserts.
        let nicks: Vec<String> = ["Alice", "Bob", "Carol", "Nightbot"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (ins, upd) = upsert_names_batch(&pool, "nani", &nicks, now).await;
        assert_eq!((ins, upd), (3, 0));
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 3); // Bot nicht eingefügt
        let sva: bool = sqlx::query_scalar(
            "SELECT seen_via_chatters_api FROM twitch_session_chatters WHERE chatter_login='alice'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(sva); // boolean TRUE

        // Zweiter NAMES-Lauf: alle vorhanden → 3 Updates, 0 Inserts.
        let later = now + chrono::Duration::seconds(60);
        let (ins2, upd2) = upsert_names_batch(&pool, "nani", &nicks, later).await;
        assert_eq!((ins2, upd2), (0, 3));

        // JOIN eines neuen Chatters → Upsert (Insert).
        upsert_chatter_seen(&pool, "nani", "Dave", later).await;
        let n2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n2, 4);

        // JOIN eines Bots → ignoriert.
        upsert_chatter_seen(&pool, "nani", "StreamElements", later).await;
        let n3: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n3, 4);
    }

    #[tokio::test]
    async fn presence_ticks_pro_tick_und_idempotent() {
        // P1.23: pro Poll-Tick eine Presence-Tick-Row je aktivem Chatter,
        // ON CONFLICT idempotent beim Re-Tick (gleicher tick_at).
        let Some(pool) = make_pool("t_presence_ticks").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id) VALUES ('1', 'nani', 1, 42)")
            .execute(&pool).await.unwrap();
        let tick = Utc::now();
        let chatters = vec!["Alice".to_string(), "Bob".to_string()];

        // Erster Tick: 2 aktive Chatter → 2 Rows.
        let n = record_presence_ticks(&pool, 42, "nani", &chatters, tick).await;
        assert_eq!(n, 2);
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_viewer_presence_ticks WHERE session_id = 42",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 2);
        // Logins normalisiert (lowercase).
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_viewer_presence_ticks WHERE viewer_login = 'alice'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists, 1);

        // Re-Tick mit identischem tick_at → idempotent (0 neue Rows).
        let n2 = record_presence_ticks(&pool, 42, "nani", &chatters, tick).await;
        assert_eq!(n2, 0);
        let rows2: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_viewer_presence_ticks WHERE session_id = 42",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows2, 2);

        // Nächster Tick (anderer tick_at) → neue Rows.
        let later = tick + chrono::Duration::seconds(30);
        let n3 = record_presence_ticks(&pool, 42, "nani", &chatters, later).await;
        assert_eq!(n3, 2);
    }

    #[tokio::test]
    async fn track_untrack_und_auth_flag() {
        let Some(pool) = make_pool("t_irc_lurker_track").await else {
            return;
        };
        // Authentifiziert: Token + Nick → kein justinfan.
        let auth = IrcLurkerTracker::new(
            pool.clone(),
            "cid".into(),
            "tok".into(),
            Some("MyBot".into()),
        );
        assert!(auth.authenticated);
        assert_eq!(auth.nick, "mybot");
        // Anonym: leerer Token → justinfan, nicht authentifiziert.
        let t = IrcLurkerTracker::new(pool, "cid".into(), String::new(), None);
        assert!(!t.authenticated);
        assert_eq!(t.nick, "justinfan12345");
        assert_eq!(
            t.handshake_commands(),
            vec![
                "NICK justinfan12345\r\n".to_string(),
                "CAP REQ :twitch.tv/membership twitch.tv/commands\r\n".to_string()
            ]
        );

        assert_eq!(
            auth.handshake_commands(),
            vec![
                "PASS oauth:tok\r\n".to_string(),
                "NICK mybot\r\n".to_string(),
                "CAP REQ :twitch.tv/membership twitch.tv/commands\r\n".to_string()
            ]
        );

        t.track_channel("#Nani", TrackMode::Partner);
        t.track_channel("someCat", TrackMode::Category);
        assert!(t.channels.lock().unwrap().contains("nani"));
        assert!(t.partner_channels.lock().unwrap().contains("nani"));
        assert!(!t.category_channels.lock().unwrap().contains("nani"));
        assert!(t.channels.lock().unwrap().contains("somecat"));
        assert!(!t.partner_channels.lock().unwrap().contains("somecat")); // category ≠ partner
        assert!(t.category_channels.lock().unwrap().contains("somecat"));

        t.chatters.lock().unwrap().insert(
            "nani".to_string(),
            ["alice".to_string(), "bob".to_string()]
                .into_iter()
                .collect(),
        );
        let chatters = t.get_chatters(" #NANI ");
        assert!(chatters.contains("alice"));
        assert!(chatters.contains("bob"));

        t.untrack_channel("nani");
        assert!(!t.channels.lock().unwrap().contains("nani"));
        assert!(!t.partner_channels.lock().unwrap().contains("nani"));
        assert!(!t.category_channels.lock().unwrap().contains("nani"));

        t.stop();
        assert!(t.stop_requested.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn keine_aktive_session_ist_noop() {
        let Some(pool) = make_pool("t_irc_lurker_nosession").await else {
            return;
        };
        // Kanal offline → keine active_session_id.
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live) VALUES ('1', 'nani', 0)")
            .execute(&pool).await.unwrap();
        let nicks = vec!["alice".to_string()];
        assert_eq!(
            upsert_names_batch(&pool, "nani", &nicks, Utc::now()).await,
            (0, 0)
        );
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn names_batch_bricht_bei_existing_select_fehler_ab() {
        let Some(pool) = make_pool("t_irc_lurker_existing_error").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id) VALUES ('1', 'nani', 1, 42)")
            .execute(&pool).await.unwrap();
        sqlx::query("DROP TABLE twitch_session_chatters")
            .execute(&pool)
            .await
            .unwrap();

        let nicks = vec!["alice".to_string()];
        assert_eq!(
            upsert_names_batch(&pool, "nani", &nicks, Utc::now()).await,
            (0, 0)
        );
    }
}

//! Experimenteller IRC-Lurker-Tracker (Port von `bot/chat/irc_lurker_tracker.py`).
//!
//! Zweite, **ergänzende** Presence-Quelle neben dem primären Helix
//! `Get Chatters`: eine separate IRC-Verbindung liest JOIN/PART/NAMES und
//! spiegelt die beobachteten Chatter in `twitch_session_chatters` (für die
//! aktive Session des Kanals). Bewusst ein Experiment, **default-AUS**
//! (`TWITCH_EXPERIMENTAL_IRC_LURKER_CHANNELS`).
//!
//! Diese Slice (26a) enthält die ohne Socket testbaren Kern-Teile: die
//! IRC-Zeilen-Parser und die DB-Upserts. Der async-Verbindungs-Loop +
//! Channel-Tracking + Wiring folgen separat.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

// ---- DB-Layer --------------------------------------------------------------

/// Aktive Session-ID eines live Kanals (`twitch_live_state`), sonst `None`.
async fn resolve_active_session(pool: &PgPool, channel: &str) -> Option<i32> {
    sqlx::query_scalar::<_, Option<i32>>(
        "SELECT active_session_id FROM twitch_live_state \
         WHERE LOWER(streamer_login) = $1 AND is_live = 1",
    )
    .bind(channel)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
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
    let Some(session_id) = resolve_active_session(pool, channel).await else {
        return;
    };
    let _ = sqlx::query(&format!(
        "{INSERT_CHATTER} ON CONFLICT (session_id, chatter_login) \
         DO UPDATE SET last_seen_at = EXCLUDED.last_seen_at"
    ))
    .bind(session_id)
    .bind(channel)
    .bind(&nick)
    .bind(now)
    .execute(pool)
    .await;
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
    let Some(session_id) = resolve_active_session(pool, channel).await else {
        return (0, 0);
    };
    let existing: HashSet<String> = sqlx::query_scalar::<_, String>(
        "SELECT chatter_login FROM twitch_session_chatters WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect();

    let mut inserts = 0;
    let mut updates = 0;
    for nick in nicks {
        let nick = nick.to_lowercase();
        if is_known_chat_bot(&nick) {
            continue;
        }
        if existing.contains(&nick) {
            let _ = sqlx::query(
                "UPDATE twitch_session_chatters SET last_seen_at = $1 \
                 WHERE session_id = $2 AND chatter_login = $3",
            )
            .bind(now)
            .bind(session_id)
            .bind(&nick)
            .execute(pool)
            .await;
            updates += 1;
        } else {
            let _ = sqlx::query(&format!(
                "{INSERT_CHATTER} ON CONFLICT (session_id, chatter_login) DO NOTHING"
            ))
            .bind(session_id)
            .bind(channel)
            .bind(&nick)
            .bind(now)
            .execute(pool)
            .await;
            inserts += 1;
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
        let res = sqlx::query(
            "INSERT INTO twitch_viewer_presence_ticks \
             (session_id, streamer_login, viewer_login, tick_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (session_id, viewer_login, tick_at) DO NOTHING",
        )
        .bind(session_id)
        .bind(&streamer)
        .bind(&viewer)
        .bind(tick_at)
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
// hartcodiert `False` (runtime_bootstrap.py) und wird NIE auf True gesetzt — der
// Tracker startet dort nie. Dieser Port baut das Feature vollständig (runnable),
// wird in tb-bot aber bewusst NICHT gespawnt → 1:1-Parität (dauerhaft aus). Zum
// Aktivieren müsste ein Aufrufer eine Instanz bauen, `track_channel` füttern und
// `run()` spawnen (Bot-User-Token + `user:read:chat`-Scope vorausgesetzt).

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;
const NAMES_POLL_SECONDS: u64 = 120;
const CONNECT_BACKOFF_SECONDS: u64 = 30;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

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
    chatters: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    cmd_rx: Mutex<Option<mpsc::UnboundedReceiver<Cmd>>>,
}

impl IrcLurkerTracker {
    /// `nick`/`access_token` leer → anonymer `justinfan`-Login (nur lokale Tests);
    /// produktiv: Bot-Login + User-Token (mirror `IRCLurkerTracker.__init__`).
    pub fn new(
        pool: PgPool,
        client_id: String,
        access_token: String,
        nick: Option<String>,
    ) -> Self {
        let nick_norm = nick
            .map(|n| n.trim().to_lowercase())
            .filter(|n| !n.is_empty());
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
            chatters: Arc::new(Mutex::new(HashMap::new())),
            cmd_tx,
            cmd_rx: Mutex::new(Some(cmd_rx)),
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
        self.channels.lock().unwrap().insert(channel.clone());
        {
            let mut partner = self.partner_channels.lock().unwrap();
            match mode {
                TrackMode::Partner => {
                    partner.insert(channel.clone());
                }
                TrackMode::Category => {
                    partner.remove(&channel);
                    self.chatters.lock().unwrap().remove(&channel);
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
        if !self.channels.lock().unwrap().remove(&channel) {
            return;
        }
        self.partner_channels.lock().unwrap().remove(&channel);
        self.chatters.lock().unwrap().remove(&channel);
        let _ = self.cmd_tx.send(Cmd::Part(channel));
    }

    /// Aktuell beobachtete Chatter eines Kanals (Python `get_chatters`).
    pub fn get_chatters(&self, channel: &str) -> HashSet<String> {
        let channel = channel.trim().to_lowercase();
        self.chatters
            .lock()
            .unwrap()
            .get(&channel)
            .cloned()
            .unwrap_or_default()
    }

    /// Verbindungs-Loop mit Auto-Reconnect. Läuft bis zum Programmende.
    pub async fn run(&self) {
        let Some(mut rx) = self.cmd_rx.lock().unwrap().take() else {
            tracing::warn!("IRC-Lurker: run() bereits aktiv");
            return;
        };
        loop {
            match self.connect().await {
                Some((reader, writer)) => self.serve(reader, writer, &mut rx).await,
                None => tokio::time::sleep(Duration::from_secs(CONNECT_BACKOFF_SECONDS)).await,
            }
        }
    }

    async fn connect(&self) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
        let stream = TcpStream::connect((IRC_HOST, IRC_PORT)).await.ok()?;
        let (rd, mut wr) = stream.into_split();
        if self.authenticated {
            let clean = self.access_token.replace("oauth:", "");
            wr.write_all(format!("PASS oauth:{clean}\r\n").as_bytes())
                .await
                .ok()?;
        }
        wr.write_all(format!("NICK {}\r\n", self.nick).as_bytes())
            .await
            .ok()?;
        wr.write_all(b"CAP REQ :twitch.tv/membership twitch.tv/commands\r\n")
            .await
            .ok()?;
        wr.flush().await.ok()?;

        let mut reader = BufReader::new(rd);
        let mut line = String::new();
        loop {
            line.clear();
            let n = match tokio::time::timeout(CONNECT_TIMEOUT, reader.read_line(&mut line)).await {
                Ok(Ok(n)) => n,
                _ => return None,
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
        let channels: Vec<String> = self.channels.lock().unwrap().iter().cloned().collect();
        for ch in channels {
            let _ = writer.write_all(format!("JOIN #{ch}\r\n").as_bytes()).await;
        }
        let _ = writer.flush().await;
        let mut poll = tokio::time::interval(Duration::from_secs(NAMES_POLL_SECONDS));
        poll.tick().await; // erster Tick sofort — überspringen.

        let mut line = String::new();
        loop {
            line.clear();
            tokio::select! {
                res = reader.read_line(&mut line) => {
                    match res {
                        Ok(0) | Err(_) => break,
                        Ok(_) => self.handle_line(line.trim_end(), &mut writer).await,
                    }
                }
                _ = poll.tick() => {
                    let chans: Vec<String> = self.channels.lock().unwrap().iter().cloned().collect();
                    for ch in chans {
                        let _ = writer.write_all(format!("NAMES #{ch}\r\n").as_bytes()).await;
                    }
                    let _ = writer.flush().await;
                }
                Some(cmd) = rx.recv() => match cmd {
                    Cmd::Join(ch) => { let _ = writer.write_all(format!("JOIN #{ch}\r\n").as_bytes()).await; let _ = writer.flush().await; }
                    Cmd::Part(ch) => { let _ = writer.write_all(format!("PART #{ch}\r\n").as_bytes()).await; let _ = writer.flush().await; }
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
            self.chatters
                .lock()
                .unwrap()
                .entry(channel.clone())
                .or_default()
                .insert(nick.clone());
            upsert_chatter_seen(&self.pool, &channel, &nick, now).await;
        } else if let Some((nick, channel)) = parse_part(msg) {
            let (channel, nick) = (channel.to_lowercase(), nick.to_lowercase());
            if let Some(set) = self.chatters.lock().unwrap().get_mut(&channel) {
                set.remove(&nick);
            }
        } else if let Some((channel, nicks)) = parse_names(msg) {
            let channel = channel.to_lowercase();
            let nicks_lower: Vec<String> = nicks.iter().map(|n| n.to_lowercase()).collect();
            // NAMES nur für Partner-Kanäle im Speicher halten (RAM-Schonung).
            if self.partner_channels.lock().unwrap().contains(&channel) {
                self.chatters
                    .lock()
                    .unwrap()
                    .insert(channel.clone(), nicks_lower.iter().cloned().collect());
            } else {
                self.chatters.lock().unwrap().remove(&channel);
            }
            upsert_names_batch(&self.pool, &channel, &nicks_lower, now).await;
        }
    }
}

/// Antwortet auf einen IRC-PING.
async fn pong(writer: &mut OwnedWriteHalf, ping: &str) {
    let reply = ping.replacen("PING", "PONG", 1);
    let _ = writer.write_all(reply.as_bytes()).await;
    let _ = writer.write_all(b"\r\n").await;
    let _ = writer.flush().await;
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
        sqlx::query("CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, is_live INTEGER DEFAULT 0, active_session_id INTEGER)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_session_chatters (session_id INTEGER, streamer_login TEXT, \
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

        t.track_channel("#Nani", TrackMode::Partner);
        t.track_channel("someCat", TrackMode::Category);
        assert!(t.channels.lock().unwrap().contains("nani"));
        assert!(t.partner_channels.lock().unwrap().contains("nani"));
        assert!(t.channels.lock().unwrap().contains("somecat"));
        assert!(!t.partner_channels.lock().unwrap().contains("somecat")); // category ≠ partner

        t.untrack_channel("nani");
        assert!(!t.channels.lock().unwrap().contains("nani"));
        assert!(!t.partner_channels.lock().unwrap().contains("nani"));
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
}

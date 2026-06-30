//! Engagement-Chat-Quelle via Twitch-IRC (Port von
//! `bot/engagement/irc_reader.py`).
//!
//! Für Kanäle, die dem Bot kein `channel:bot` per EventSub freigegeben haben
//! (einwilligende Streamer ohne Partner-Onboarding), gibt es keinen
//! EventSub-`channel.chat.message`-Stream. Dieser Reader joint solche Kanäle
//! stattdessen **anonym über IRC** (`justinfan`), liest die Chat-Nachrichten und
//! routet sie in dieselbe [`EngagementPipeline`] wie der EventSub-Pfad.
//!
//! Trennung der Transporte:
//! - **Lesen**: anonymes IRC (`irc.chat.twitch.tv:6667`, CAP `tags`+`commands`).
//! - **Schreiben**: Helix über den Smoke-Account ([`StealthSender`]).
//!
//! Kanalquelle: `twitch_engagement_settings` mit `enabled = TRUE AND irc_read =
//! TRUE`. Die Kanal-Menge ist disjunkt zum EventSub-Pfad → kein Doppel-Processing.
//!
//! Statt Pythons drei asyncio-Tasks (Connection/Read/Refresh mit geteiltem
//! `self.writer`) läuft hier **ein** Task mit `tokio::select!` über Read vs.
//! Refresh-Tick — der Task besitzt die Stream-Hälften exklusiv (kein Mutex).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use crate::pipeline::EngagementPipeline;
use crate::sender_auth::SENDER_LOGIN;
use crate::stealth_sender::StealthSender;
use crate::types::IncomingMessage;

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;
const ANON_NICK: &str = "justinfan13371337";
const CHANNEL_REFRESH_SECONDS: u64 = 300;
const CONNECT_BACKOFF_SECONDS: u64 = 30;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Known-Chat-Bots (chat_bots.py Z. 8–19 `KNOWN_CHAT_BOTS`). Lokale Kopie wie in
/// tb-chat/tb-dashboard-api — der Reader hängt nicht an der Chat-Crate.
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

/// Fügt die `irc_read`-Spalte lazy hinzu (kein Eingriff in den Settings-Flow).
async fn ensure_schema(pool: &PgPool) {
    let _ = sqlx::query!(
        "ALTER TABLE twitch_engagement_settings \
         ADD COLUMN IF NOT EXISTS irc_read BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(pool)
    .await;
}

/// Aktive Kanäle mit `irc_read = TRUE` (kleingeschrieben).
async fn load_irc_channels(pool: &PgPool) -> HashSet<String> {
    ensure_schema(pool).await;
    sqlx::query_scalar!(
        r#"SELECT channel_login AS "channel_login!"
           FROM twitch_engagement_settings
           WHERE enabled = TRUE AND irc_read = TRUE"#
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|c| c.trim().to_lowercase())
    .filter(|c| !c.is_empty())
    .collect()
}

/// Parst die IRCv3-Tags (`key=value;key2=value2`).
fn parse_tags(raw: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for kv in raw.split(';') {
        if let Some((key, value)) = kv.split_once('=') {
            out.insert(key.to_string(), value.to_string());
        }
    }
    out
}

/// Eine geparste PRIVMSG-Zeile: Tags + Absender + Kanal + Text.
struct ParsedPrivmsg {
    tags: HashMap<String, String>,
    login: String,
    channel: String,
    text: String,
}

/// Zerlegt eine IRC-Zeile in eine PRIVMSG, falls es eine ist (sonst `None`).
/// Format mit optionalem Tag-Präfix:
/// `@tags :nick!user@host PRIVMSG #channel :text`.
fn parse_privmsg(line: &str) -> Option<ParsedPrivmsg> {
    let (tags, rest) = if let Some(stripped) = line.strip_prefix('@') {
        let (tag_part, rest) = stripped.split_once(' ')?;
        (parse_tags(tag_part), rest)
    } else {
        (HashMap::new(), line)
    };

    // :nick!user@host PRIVMSG #channel :text
    let rest = rest.strip_prefix(':')?;
    let (prefix, after) = rest.split_once(' ')?;
    let login = prefix.split('!').next()?.to_string();
    let after = after.strip_prefix("PRIVMSG #")?;
    let (channel, text) = after.split_once(' ')?;
    let text = text.strip_prefix(':')?;
    Some(ParsedPrivmsg {
        tags,
        login,
        channel: channel.to_string(),
        text: text.to_string(),
    })
}

/// Baut aus einer geparsten PRIVMSG die [`IncomingMessage`] + Broadcaster-ID
/// (room-id) — oder `None`, wenn übersprungen werden soll (leer, eigener
/// Account, bekannter Bot, fehlende room-id/user-id). Pure (ohne I/O testbar).
fn build_incoming(parsed: &ParsedPrivmsg, self_login: &str) -> Option<(IncomingMessage, String)> {
    let login = parsed.login.trim().to_lowercase();
    let channel = parsed.channel.trim().to_lowercase();
    let text = parsed.text.trim().to_string();
    if login.is_empty() || channel.is_empty() || text.is_empty() {
        return None;
    }
    // Eigene Nachrichten und bekannte Bots ignorieren (keine Selbst-Antwort-Loops).
    if login == self_login || is_known_chat_bot(&login) {
        return None;
    }
    let room_id = parsed.tags.get("room-id").map(|s| s.trim()).unwrap_or("").to_string();
    let user_id = parsed.tags.get("user-id").map(|s| s.trim()).unwrap_or("").to_string();
    if room_id.is_empty() || user_id.is_empty() {
        return None;
    }
    let message_id = parsed
        .tags
        .get("id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some((
        IncomingMessage {
            channel_login: channel,
            twitch_user_id: user_id,
            twitch_login: login,
            content: text,
            message_id,
        },
        room_id,
    ))
}

/// Anonymer IRC-Reader, der Chat in die Engagement-Pipeline speist.
pub struct EngagementIrcReader {
    pool: PgPool,
    pipeline: Arc<EngagementPipeline>,
    stealth: Option<Arc<StealthSender>>,
    self_login: String,
}

impl EngagementIrcReader {
    pub fn new(
        pool: PgPool,
        pipeline: Arc<EngagementPipeline>,
        stealth: Option<Arc<StealthSender>>,
    ) -> Self {
        Self {
            pool,
            pipeline,
            stealth,
            self_login: SENDER_LOGIN.to_lowercase(),
        }
    }

    /// Startet den Reader. No-op (kehrt sofort zurück), wenn keine
    /// `irc_read`-Kanäle konfiguriert sind (Python: „Reader bleibt aus").
    /// Reconnectet bei Verbindungsabbruch dauerhaft.
    pub async fn run(self) {
        let mut channels = load_irc_channels(&self.pool).await;
        if channels.is_empty() {
            tracing::info!("Engagement-IRC: keine irc_read-Kanäle konfiguriert, Reader bleibt aus");
            return;
        }
        tracing::info!(channels = ?sorted(&channels), "Engagement-IRC-Reader gestartet");
        loop {
            match self.connect().await {
                Some((reader, writer)) => self.serve(reader, writer, &mut channels).await,
                None => tokio::time::sleep(Duration::from_secs(CONNECT_BACKOFF_SECONDS)).await,
            }
        }
    }

    /// Baut die anonyme IRC-Verbindung auf (NICK + CAP) und wartet auf `001`.
    async fn connect(&self) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
        let stream = TcpStream::connect((IRC_HOST, IRC_PORT)).await.ok()?;
        let (rd, mut wr) = stream.into_split();
        wr.write_all(format!("NICK {ANON_NICK}\r\n").as_bytes()).await.ok()?;
        wr.write_all(b"CAP REQ :twitch.tv/tags twitch.tv/commands\r\n").await.ok()?;
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
                tracing::info!("Engagement-IRC verbunden (anonym)");
                return Some((reader, wr));
            }
            if msg.starts_with("PING") {
                pong(&mut wr, msg).await;
            }
        }
    }

    /// Liest bis zum Verbindungsabbruch + refresht periodisch die Kanal-Joins.
    async fn serve(
        &self,
        mut reader: BufReader<OwnedReadHalf>,
        mut writer: OwnedWriteHalf,
        channels: &mut HashSet<String>,
    ) {
        for ch in channels.iter() {
            join(&mut writer, ch).await;
        }
        let mut refresh = tokio::time::interval(Duration::from_secs(CHANNEL_REFRESH_SECONDS));
        refresh.tick().await; // erster Tick feuert sofort — überspringen.

        let mut line = String::new();
        loop {
            line.clear();
            tokio::select! {
                res = reader.read_line(&mut line) => {
                    match res {
                        Ok(0) | Err(_) => break, // Abbruch → äußerer Reconnect.
                        Ok(_) => self.handle_line(line.trim_end(), &mut writer).await,
                    }
                }
                _ = refresh.tick() => {
                    let latest = load_irc_channels(&self.pool).await;
                    for ch in latest.difference(channels) {
                        join(&mut writer, ch).await;
                    }
                    *channels = latest;
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
        let Some(parsed) = parse_privmsg(msg) else {
            return;
        };
        let Some((incoming, room_id)) = build_incoming(&parsed, &self.self_login) else {
            return;
        };
        // Pipeline + Send in eigener Task — MiniMax-Latenz blockiert den Read-Loop nicht.
        let pipeline = Arc::clone(&self.pipeline);
        let stealth = self.stealth.clone();
        let channel = incoming.channel_login.clone();
        tokio::spawn(async move {
            let result = pipeline.handle(&incoming).await;
            let Some(text) = result.response_text else {
                return;
            };
            match &stealth {
                Some(sender) if sender.send(&room_id, &text).await.is_none() => {
                    tracing::info!(channel = %channel, "Engagement-IRC: kein Sende-Account, Antwort verworfen");
                }
                Some(_) => {}
                None => tracing::debug!(channel = %channel, "Engagement-IRC: Stealth-Sender nicht verfügbar"),
            }
        });
    }
}

/// Antwortet auf einen IRC-PING (`PING ...` → `PONG ...`).
async fn pong(writer: &mut OwnedWriteHalf, ping: &str) {
    let reply = ping.replacen("PING", "PONG", 1);
    let _ = writer.write_all(reply.as_bytes()).await;
    let _ = writer.write_all(b"\r\n").await;
    let _ = writer.flush().await;
}

/// Joint einen Kanal (`JOIN #channel`).
async fn join(writer: &mut OwnedWriteHalf, channel: &str) {
    let _ = writer.write_all(format!("JOIN #{channel}\r\n").as_bytes()).await;
    let _ = writer.flush().await;
}

/// Sortierte Kanal-Liste fürs Log (deterministisch).
fn sorted(channels: &HashSet<String>) -> Vec<&String> {
    let mut v: Vec<&String> = channels.iter().collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_parse() {
        let t = parse_tags("room-id=123;user-id=456;id=abc;badge=");
        assert_eq!(t.get("room-id").unwrap(), "123");
        assert_eq!(t.get("user-id").unwrap(), "456");
        assert_eq!(t.get("id").unwrap(), "abc");
        assert_eq!(t.get("badge").unwrap(), "");
    }

    #[test]
    fn privmsg_mit_tags() {
        let line = "@room-id=99;user-id=42;id=m1 :viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #nani :lohnt sich haze?";
        let p = parse_privmsg(line).unwrap();
        assert_eq!(p.login, "viewer");
        assert_eq!(p.channel, "nani");
        assert_eq!(p.text, "lohnt sich haze?");
        assert_eq!(p.tags.get("room-id").unwrap(), "99");
    }

    #[test]
    fn privmsg_ohne_tags() {
        let line = ":viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #nani :hallo welt";
        let p = parse_privmsg(line).unwrap();
        assert_eq!(p.login, "viewer");
        assert_eq!(p.channel, "nani");
        assert_eq!(p.text, "hallo welt");
        assert!(p.tags.is_empty());
    }

    #[test]
    fn nicht_privmsg_ist_none() {
        assert!(parse_privmsg("PING :tmi.twitch.tv").is_none());
        assert!(parse_privmsg(":tmi.twitch.tv 001 justinfan :welcome").is_none());
        assert!(parse_privmsg("@badge=1 :x!x@x JOIN #nani").is_none());
    }

    #[test]
    fn text_mit_doppelpunkt_bleibt_ganz() {
        let line = ":v!v@v PRIVMSG #c :check http://x.y :cool";
        let p = parse_privmsg(line).unwrap();
        assert_eq!(p.text, "check http://x.y :cool");
    }

    #[test]
    fn incoming_aus_privmsg() {
        let line = "@room-id=99;user-id=42;id=m1 :viewer!v@v PRIVMSG #Nani :lohnt sich haze";
        let p = parse_privmsg(line).unwrap();
        let (im, room) = build_incoming(&p, "iamspyingthroughtyourcam").unwrap();
        assert_eq!(room, "99");
        assert_eq!(im.channel_login, "nani"); // kleingeschrieben
        assert_eq!(im.twitch_user_id, "42");
        assert_eq!(im.twitch_login, "viewer");
        assert_eq!(im.content, "lohnt sich haze");
        assert_eq!(im.message_id.as_deref(), Some("m1"));
    }

    #[test]
    fn skip_eigener_account() {
        let line = "@room-id=9;user-id=1 :iamspyingthroughtyourcam!s@s PRIVMSG #nani :test nachricht";
        let p = parse_privmsg(line).unwrap();
        assert!(build_incoming(&p, "iamspyingthroughtyourcam").is_none());
    }

    #[test]
    fn skip_bekannter_bot() {
        let line = "@room-id=9;user-id=1 :nightbot!n@n PRIVMSG #nani :!command output";
        let p = parse_privmsg(line).unwrap();
        assert!(build_incoming(&p, "iamspyingthroughtyourcam").is_none());
    }

    #[test]
    fn skip_ohne_room_oder_user_id() {
        // Fehlende room-id.
        let line = "@user-id=1 :viewer!v@v PRIVMSG #nani :hallo welt";
        let p = parse_privmsg(line).unwrap();
        assert!(build_incoming(&p, "iamspyingthroughtyourcam").is_none());
        // Fehlende user-id.
        let line2 = "@room-id=9 :viewer!v@v PRIVMSG #nani :hallo welt";
        let p2 = parse_privmsg(line2).unwrap();
        assert!(build_incoming(&p2, "iamspyingthroughtyourcam").is_none());
    }

    #[test]
    fn skip_leerer_text() {
        let line = "@room-id=9;user-id=1 :viewer!v@v PRIVMSG #nani :   ";
        let p = parse_privmsg(line).unwrap();
        assert!(build_incoming(&p, "iamspyingthroughtyourcam").is_none());
    }
}

//! Anonymer IRC-Reader für den Reaktions-Lernmodus.
//!
//! Löst das Henne-Ei-Problem von „lerne dort, wo ich auftauche": Um zu sehen,
//! dass der Owner in einem Kanal schreibt, muss man den Kanal schon mitlesen.
//! Der EventSub-Pfad hilft dabei nicht, denn er deckt nur Kanäle ab, die dem
//! Bot `channel:bot` erteilt haben (Partner) — und gelernt werden soll gerade
//! in fremden Kanälen.
//!
//! Also: alle live Deadlock-Kanäle anonym (`justinfan`) mitlesen und abwarten.
//! Der Scout hält `twitch_live_state` aktuell, und anonymes IRC-Zuhören kostet
//! nichts außer einer TCP-Verbindung. Geschrieben wird hier nichts: dieser
//! Reader hat keinen Sende-Transport und keinen Draht zur Antwort-Pipeline.
//!
//! Gespeichert wird trotzdem nur wenig — [`ReactionLearning::observe`] wirft
//! alles weg, was weder vom Owner kommt noch in einem lern-heißen Kanal steht.
//! Aus 30 mitgelesenen Kanälen wird so nur der eine, in dem er gerade sitzt.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use crate::irc_message::parse_privmsg;
use crate::reaction_learning::ReactionLearning;

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;
/// Eigener Nick, damit die Verbindung nicht mit der des Engagement-Readers
/// verwechselt wird.
const ANON_NICK: &str = "justinfan47110815";
const CHANNEL_REFRESH_SECONDS: u64 = 120;
const CONNECT_BACKOFF_SECONDS: u64 = 30;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Obergrenze der gejointen Kanäle — Schutz vor einer entgleisten Kanalliste.
const MAX_CHANNELS: usize = 60;

/// Live Deadlock-Kanäle plus alle, die gerade lern-heiß sind.
///
/// Die heißen Kanäle bleiben bewusst drin, auch wenn der Streamer kurz auf
/// „Just Chatting" wechselt: mitten in einer laufenden Sitzung den Chat zu
/// verlieren würde genau die Nachrichten kosten, um die es geht.
async fn load_learn_channels(pool: &PgPool, learn: &ReactionLearning) -> HashSet<String> {
    let mut channels: HashSet<String> = sqlx::query_scalar!(
        r#"SELECT streamer_login AS "streamer_login!"
           FROM twitch_live_state
           WHERE COALESCE(is_live, 0) <> 0 AND LOWER(TRIM(COALESCE(last_game, ''))) = 'deadlock'"#
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|c| c.trim().to_lowercase())
    .filter(|c| !c.is_empty())
    .take(MAX_CHANNELS)
    .collect();
    channels.extend(learn.hot_channels());
    channels
}

/// Anonymer Mitleser, der ausschließlich den Lernmodus füttert.
pub struct LearnIrcReader {
    pool: PgPool,
    learn: Arc<ReactionLearning>,
}

impl LearnIrcReader {
    pub fn new(pool: PgPool, learn: Arc<ReactionLearning>) -> Self {
        Self { pool, learn }
    }

    /// Läuft dauerhaft und reconnectet bei Abbruch. Ohne live Deadlock-Kanäle
    /// wartet er, statt sich zu beenden.
    pub async fn run(self) {
        tracing::info!("Lern-IRC-Reader gestartet (anonym, nur Beobachtung)");
        let mut channels: HashSet<String> = HashSet::new();
        loop {
            if channels.is_empty() {
                channels = load_learn_channels(&self.pool, &self.learn).await;
            }
            if channels.is_empty() {
                tokio::time::sleep(Duration::from_secs(CHANNEL_REFRESH_SECONDS)).await;
                continue;
            }
            match self.connect().await {
                Some((reader, writer)) => self.serve(reader, writer, &mut channels).await,
                None => tokio::time::sleep(Duration::from_secs(CONNECT_BACKOFF_SECONDS)).await,
            }
        }
    }

    async fn connect(&self) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
        let stream = match TcpStream::connect((IRC_HOST, IRC_PORT)).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "Lern-IRC: Connect fehlgeschlagen");
                return None;
            }
        };
        let (rd, mut wr) = stream.into_split();
        if let Err(error) = wr.write_all(format!("NICK {ANON_NICK}\r\n").as_bytes()).await {
            tracing::warn!(%error, "Lern-IRC: NICK-Handshake fehlgeschlagen");
            return None;
        }
        if let Err(error) = wr.write_all(b"CAP REQ :twitch.tv/tags twitch.tv/commands\r\n").await {
            tracing::warn!(%error, "Lern-IRC: CAP-Handshake fehlgeschlagen");
            return None;
        }
        if let Err(error) = wr.flush().await {
            tracing::warn!(%error, "Lern-IRC: Handshake-Flush fehlgeschlagen");
            return None;
        }

        let mut reader = BufReader::new(rd);
        let mut line = String::new();
        loop {
            line.clear();
            let n = match tokio::time::timeout(CONNECT_TIMEOUT, reader.read_line(&mut line)).await {
                Ok(Ok(n)) => n,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "Lern-IRC: Handshake-Read fehlgeschlagen");
                    return None;
                }
                Err(_) => {
                    tracing::warn!("Lern-IRC: Handshake-Timeout");
                    return None;
                }
            };
            if n == 0 {
                return None;
            }
            let msg = line.trim_end();
            if msg.starts_with(":tmi.twitch.tv 001") {
                tracing::info!("Lern-IRC verbunden (anonym)");
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
        channels: &mut HashSet<String>,
    ) {
        for ch in channels.iter() {
            join(&mut writer, ch).await;
        }
        let mut refresh = tokio::time::interval(Duration::from_secs(CHANNEL_REFRESH_SECONDS));
        refresh.tick().await; // erster Tick feuert sofort

        let mut line = String::new();
        loop {
            line.clear();
            tokio::select! {
                res = reader.read_line(&mut line) => {
                    match res {
                        Ok(0) => break,
                        Err(error) => {
                            tracing::warn!(%error, "Lern-IRC: Read fehlgeschlagen");
                            break;
                        }
                        Ok(_) => self.handle_line(line.trim_end(), &mut writer).await,
                    }
                }
                _ = refresh.tick() => {
                    let latest = load_learn_channels(&self.pool, &self.learn).await;
                    let (to_join, to_part) = channel_delta(channels, &latest);
                    for ch in &to_join {
                        join(&mut writer, ch).await;
                    }
                    for ch in &to_part {
                        part(&mut writer, ch).await;
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
        let message_id = parsed.tags.get("id").map(String::as_str);
        self.learn
            .observe(&parsed.channel, None, &parsed.login, &parsed.text, message_id)
            .await;
    }
}

async fn pong(writer: &mut OwnedWriteHalf, ping: &str) {
    let reply = ping.replacen("PING", "PONG", 1);
    if let Err(error) = writer.write_all(reply.as_bytes()).await {
        tracing::warn!(%error, "Lern-IRC: PONG-Write fehlgeschlagen");
    }
    if let Err(error) = writer.write_all(b"\r\n").await {
        tracing::warn!(%error, "Lern-IRC: PONG-Newline fehlgeschlagen");
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, "Lern-IRC: PONG-Flush fehlgeschlagen");
    }
}

/// Welche Kanäle beim Refresh dazukommen und welche wegfallen.
fn channel_delta(current: &HashSet<String>, latest: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let to_join = latest.difference(current).cloned().collect();
    let to_part = current.difference(latest).cloned().collect();
    (to_join, to_part)
}

async fn join(writer: &mut OwnedWriteHalf, channel: &str) {
    if let Err(error) = writer.write_all(format!("JOIN #{channel}\r\n").as_bytes()).await {
        tracing::warn!(%error, channel, "Lern-IRC: JOIN-Write fehlgeschlagen");
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, channel, "Lern-IRC: JOIN-Flush fehlgeschlagen");
    }
}

async fn part(writer: &mut OwnedWriteHalf, channel: &str) {
    if let Err(error) = writer.write_all(format!("PART #{channel}\r\n").as_bytes()).await {
        tracing::warn!(%error, channel, "Lern-IRC: PART-Write fehlgeschlagen");
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, channel, "Lern-IRC: PART-Flush fehlgeschlagen");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn delta_joint_neue_und_partet_alte() {
        let current: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let latest: HashSet<String> = ["b".to_string(), "c".to_string()].into_iter().collect();
        let (join, part) = channel_delta(&current, &latest);
        assert_eq!(join, vec!["c".to_string()]);
        assert_eq!(part, vec!["a".to_string()]);
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_live_state (\
             twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, \
             is_live INTEGER DEFAULT 0, last_game TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_learn_channels (\
             channel_login TEXT PRIMARY KEY, channel_user_id TEXT, \
             first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
             last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
             message_count BIGINT NOT NULL DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn nur_live_deadlock_kanaele_werden_gejoint() {
        let Some(pool) = make_pool("t_eng_learn_irc").await else { return };
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES \
             ('1','LiveDeadlock',1,'Deadlock'), \
             ('2','live_anderes',1,'Dota 2'), \
             ('3','offline_deadlock',0,'Deadlock'), \
             ('4','ohne_spiel',1,NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        let channels = load_learn_channels(&pool, &learn).await;
        assert_eq!(channels, ["livedeadlock".to_string()].into_iter().collect());
    }

    #[tokio::test]
    async fn heisser_kanal_bleibt_auch_ohne_deadlock_dabei() {
        let Some(pool) = make_pool("t_eng_learn_irc_hot").await else { return };
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) \
             VALUES ('1','pausiert',1,'Just Chatting')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        // Owner schreibt dort → Kanal ist heiß, obwohl kein Deadlock läuft.
        learn.observe("pausiert", None, "owner", "kurz afk", None).await;
        let channels = load_learn_channels(&pool, &learn).await;
        assert!(channels.contains("pausiert"));
    }
}

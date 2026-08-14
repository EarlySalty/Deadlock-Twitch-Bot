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
//! - **Schreiben**: absichtlich nicht möglich; der anonyme Reader besitzt
//!   keinen Twitch-Schreib-Transport.
//!
//! Kanalquelle: `twitch_engagement_settings` mit `enabled = TRUE AND irc_read =
//! TRUE`. Die Kanal-Menge ist disjunkt zum EventSub-Pfad → kein Doppel-Processing.
//!
//! Statt Pythons drei asyncio-Tasks (Connection/Read/Refresh mit geteiltem
//! `self.writer`) läuft hier **ein** Task mit `tokio::select!` über Read vs.
//! Refresh-Tick — der Task besitzt die Stream-Hälften exklusiv (kein Mutex).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use crate::irc_message::{build_incoming, parse_privmsg};
use crate::pipeline::EngagementPipeline;
use crate::sender_auth::SENDER_LOGIN;

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;
const ANON_NICK: &str = "justinfan13371337";
const CHANNEL_REFRESH_SECONDS: u64 = 300;
const CONNECT_BACKOFF_SECONDS: u64 = 30;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Fügt die `irc_read`-Spalte lazy hinzu (kein Eingriff in den Settings-Flow).
async fn ensure_schema(pool: &PgPool) {
    let _ = sqlx::query!(
        "ALTER TABLE twitch_engagement_settings \
         ADD COLUMN IF NOT EXISTS irc_read BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(pool)
    .await;
}

/// Lädt Kanäle, bis mindestens einer da ist. Ohne dieses Warten wäre ein
/// Botstart ohne `irc_read`-Kanal endgültig: der Reader beendete sich, und ein
/// später freigeschalteter Kanal (Smalltalk-Loop) würde nie gejoint.
async fn wait_for_channels<F, Fut>(interval: Duration, mut load: F) -> HashSet<String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = HashSet<String>>,
{
    let mut logged = false;
    loop {
        let channels = load().await;
        if !channels.is_empty() {
            return channels;
        }
        if !logged {
            tracing::info!(
                event = "engagement_irc.waiting_for_channels",
                "Engagement-IRC: noch keine irc_read-Kanäle, Reader wartet"
            );
            logged = true;
        }
        tokio::time::sleep(interval).await;
    }
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

/// Anonymer IRC-Reader, der Chat in die Engagement-Pipeline speist.
pub struct EngagementIrcReader {
    pool: PgPool,
    pipeline: Arc<EngagementPipeline>,
    self_login: String,
}

impl EngagementIrcReader {
    pub fn new(pool: PgPool, pipeline: Arc<EngagementPipeline>) -> Self {
        Self {
            pool,
            pipeline,
            self_login: SENDER_LOGIN.to_lowercase(),
        }
    }

    /// Startet den Reader. Sind noch keine `irc_read`-Kanäle konfiguriert,
    /// wartet er darauf, statt sich zu beenden: der Smalltalk-Loop schaltet
    /// seinen Kanal erst zur Laufzeit frei, und ein beendeter Reader würde ihn
    /// nie joinen. Reconnectet bei Verbindungsabbruch dauerhaft.
    pub async fn run(self) {
        let pool = self.pool.clone();
        let mut channels = wait_for_channels(Duration::from_secs(CHANNEL_REFRESH_SECONDS), || {
            let pool = pool.clone();
            async move { load_irc_channels(&pool).await }
        })
        .await;
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
        let stream = match TcpStream::connect((IRC_HOST, IRC_PORT)).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "Engagement-IRC: Connect fehlgeschlagen");
                return None;
            }
        };
        let (rd, mut wr) = stream.into_split();
        if let Err(error) = wr
            .write_all(format!("NICK {ANON_NICK}\r\n").as_bytes())
            .await
        {
            tracing::warn!(%error, "Engagement-IRC: NICK-Handshake fehlgeschlagen");
            return None;
        }
        if let Err(error) = wr
            .write_all(b"CAP REQ :twitch.tv/tags twitch.tv/commands\r\n")
            .await
        {
            tracing::warn!(%error, "Engagement-IRC: CAP-Handshake fehlgeschlagen");
            return None;
        }
        if let Err(error) = wr.flush().await {
            tracing::warn!(%error, "Engagement-IRC: Handshake-Flush fehlgeschlagen");
            return None;
        }

        let mut reader = BufReader::new(rd);
        let mut line = String::new();
        loop {
            line.clear();
            let n = match tokio::time::timeout(CONNECT_TIMEOUT, reader.read_line(&mut line)).await {
                Ok(Ok(n)) => n,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "Engagement-IRC: Handshake-Read fehlgeschlagen");
                    return None;
                }
                Err(_) => {
                    tracing::warn!("Engagement-IRC: Handshake-Timeout");
                    return None;
                }
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
                        Ok(0) => break, // Abbruch → äußerer Reconnect.
                        Err(error) => {
                            tracing::warn!(%error, "Engagement-IRC: Read fehlgeschlagen");
                            break;
                        }
                        Ok(_) => self.handle_line(line.trim_end(), &mut writer).await,
                    }
                }
                _ = refresh.tick() => {
                    let latest = load_irc_channels(&self.pool).await;
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
        let Some(incoming) = build_incoming(&parsed, &self.self_login) else {
            return;
        };
        // Lurker bleibt Lurker: Der anonyme Reader kann die Pipeline speisen,
        // besitzt aber absichtlich keinen Schreib-Transport.
        let pipeline = Arc::clone(&self.pipeline);
        let channel = incoming.channel_login.clone();
        match tokio::spawn(async move { pipeline.handle(&incoming).await }).await {
            Ok(_) => {}
            Err(error) => {
                tracing::error!(channel = %channel, %error, "Engagement-IRC: Pipeline-Task fehlgeschlagen");
            }
        }
    }
}

/// Antwortet auf einen IRC-PING (`PING ...` → `PONG ...`).
async fn pong(writer: &mut OwnedWriteHalf, ping: &str) {
    let reply = ping.replacen("PING", "PONG", 1);
    if let Err(error) = writer.write_all(reply.as_bytes()).await {
        tracing::warn!(%error, "Engagement-IRC: PONG-Write fehlgeschlagen");
    }
    if let Err(error) = writer.write_all(b"\r\n").await {
        tracing::warn!(%error, "Engagement-IRC: PONG-Newline fehlgeschlagen");
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, "Engagement-IRC: PONG-Flush fehlgeschlagen");
    }
}

/// Joint einen Kanal (`JOIN #channel`).
/// Welche Kanäle beim Refresh dazukommen und welche wegfallen.
///
/// Der Wegfall ist der wichtige Teil: der Smalltalk-Loop nimmt `irc_read` am
/// Sitzungsende wieder zurueck. Ohne PART bliebe der Reader im fremden Kanal
/// und laese dort weiter mit, obwohl keine Sitzung mehr laeuft, und bei
/// stuendlicher Rotation summierten sich die Joins.
fn channel_delta(
    current: &HashSet<String>,
    latest: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let to_join = latest.difference(current).cloned().collect();
    let to_part = current.difference(latest).cloned().collect();
    (to_join, to_part)
}

/// Verlässt einen Kanal (`PART #channel`).
async fn part(writer: &mut OwnedWriteHalf, channel: &str) {
    if let Err(error) = writer
        .write_all(format!("PART #{channel}\r\n").as_bytes())
        .await
    {
        tracing::warn!(%error, channel, "Engagement-IRC: PART-Write fehlgeschlagen");
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, channel, "Engagement-IRC: PART-Flush fehlgeschlagen");
    }
}

async fn join(writer: &mut OwnedWriteHalf, channel: &str) {
    if let Err(error) = writer
        .write_all(format!("JOIN #{channel}\r\n").as_bytes())
        .await
    {
        tracing::warn!(%error, channel, "Engagement-IRC: JOIN-Write fehlgeschlagen");
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, channel, "Engagement-IRC: JOIN-Flush fehlgeschlagen");
    }
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
    use crate::irc_message::parse_tags;

    /// Der Smalltalk-Loop schaltet `irc_read` erst zur Laufzeit ein. Kehrte
    /// der Reader bei anfangs leerer Kanalmenge zurueck, wuerde der Testkanal
    /// nie gejoint: die Sitzung liefe ohne Chat-Input und meldete "keine
    /// Nachrichten". Diese Stille saehe wie ein Ergebnis aus, waere aber
    /// keins. Deshalb wartet der Reader, statt sich zu beenden.
    #[tokio::test]
    async fn wartet_auf_spaeter_aktivierte_kanaele_statt_sich_zu_beenden() {
        let runde = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let zaehler = runde.clone();

        let kanaele = wait_for_channels(Duration::from_millis(1), move || {
            let zaehler = zaehler.clone();
            async move {
                // Erst ab der dritten Abfrage liefert die DB einen Kanal,
                // so wie wenn der Loop ihn mitten im Betrieb aktiviert.
                if zaehler.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2 {
                    HashSet::new()
                } else {
                    HashSet::from(["fremder_kanal".to_owned()])
                }
            }
        })
        .await;

        assert_eq!(kanaele, HashSet::from(["fremder_kanal".to_owned()]));
        assert!(
            runde.load(std::sync::atomic::Ordering::SeqCst) >= 3,
            "der Reader muss erneut nachsehen, statt beim ersten leeren Ergebnis aufzugeben"
        );
    }

    /// Am Sitzungsende faellt der Testkanal aus `irc_read` heraus. Wird er
    /// dann nicht verlassen, liest der Bot in einem fremden Kanal weiter mit,
    /// ohne dass dort eine Sitzung laeuft.
    #[test]
    fn refresh_verlaesst_abgeschaltete_kanaele() {
        let aktuell = HashSet::from(["partner".to_owned(), "testkanal".to_owned()]);
        let danach = HashSet::from(["partner".to_owned(), "neuer_test".to_owned()]);

        let (to_join, to_part) = channel_delta(&aktuell, &danach);

        assert_eq!(to_join, vec!["neuer_test".to_owned()]);
        assert_eq!(
            to_part,
            vec!["testkanal".to_owned()],
            "der abgeschaltete Testkanal muss verlassen werden"
        );
    }

    #[test]
    fn refresh_ohne_aenderung_macht_nichts() {
        let gleich = HashSet::from(["partner".to_owned()]);

        let (to_join, to_part) = channel_delta(&gleich, &gleich);

        assert!(to_join.is_empty() && to_part.is_empty());
    }

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
        let im = build_incoming(&p, "iamspyingthroughtyourcam").unwrap();
        assert_eq!(im.channel_login, "nani"); // kleingeschrieben
        assert_eq!(im.twitch_user_id, "42");
        assert_eq!(im.twitch_login, "viewer");
        assert_eq!(im.content, "lohnt sich haze");
        assert_eq!(im.message_id.as_deref(), Some("m1"));
    }

    #[test]
    fn skip_eigener_account() {
        let line =
            "@room-id=9;user-id=1 :iamspyingthroughtyourcam!s@s PRIVMSG #nani :test nachricht";
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

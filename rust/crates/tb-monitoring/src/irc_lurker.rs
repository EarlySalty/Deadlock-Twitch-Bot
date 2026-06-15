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

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

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
    let channel: String = after[idx + marker.len()..].chars().take_while(|c| is_irc_word(*c)).collect();
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
    let channel: String = channel_raw.chars().take_while(|c| is_irc_word(*c)).collect();
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
async fn resolve_active_session(pool: &PgPool, channel: &str) -> Option<i64> {
    let row: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT active_session_id FROM twitch_live_state \
         WHERE LOWER(streamer_login) = $1 AND is_live = 1",
    )
    .bind(channel)
    .fetch_optional(pool)
    .await
    .ok()?;
    row.and_then(|(s,)| s)
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
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, is_live INTEGER DEFAULT 0, active_session_id BIGINT)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, \
             chatter_login TEXT, chatter_id TEXT, first_message_at TIMESTAMPTZ, messages INTEGER, \
             is_first_time_streamer BOOLEAN, seen_via_chatters_api BOOLEAN, last_seen_at TIMESTAMPTZ, \
             PRIMARY KEY (session_id, chatter_login))",
        )
        .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn names_batch_und_join_upsert() {
        let Some(pool) = make_pool("t_irc_lurker").await else { return };
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id) VALUES ('1', 'nani', 1, 42)")
            .execute(&pool).await.unwrap();
        let now = Utc::now();

        // NAMES: 3 echte Chatter + 1 Bot (gefiltert) → 3 Inserts.
        let nicks: Vec<String> = ["Alice", "Bob", "Carol", "Nightbot"].iter().map(|s| s.to_string()).collect();
        let (ins, upd) = upsert_names_batch(&pool, "nani", &nicks, now).await;
        assert_eq!((ins, upd), (3, 0));
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 3); // Bot nicht eingefügt
        let sva: bool = sqlx::query_scalar("SELECT seen_via_chatters_api FROM twitch_session_chatters WHERE chatter_login='alice'").fetch_one(&pool).await.unwrap();
        assert!(sva); // boolean TRUE

        // Zweiter NAMES-Lauf: alle vorhanden → 3 Updates, 0 Inserts.
        let later = now + chrono::Duration::seconds(60);
        let (ins2, upd2) = upsert_names_batch(&pool, "nani", &nicks, later).await;
        assert_eq!((ins2, upd2), (0, 3));

        // JOIN eines neuen Chatters → Upsert (Insert).
        upsert_chatter_seen(&pool, "nani", "Dave", later).await;
        let n2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters").fetch_one(&pool).await.unwrap();
        assert_eq!(n2, 4);

        // JOIN eines Bots → ignoriert.
        upsert_chatter_seen(&pool, "nani", "StreamElements", later).await;
        let n3: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters").fetch_one(&pool).await.unwrap();
        assert_eq!(n3, 4);
    }

    #[tokio::test]
    async fn keine_aktive_session_ist_noop() {
        let Some(pool) = make_pool("t_irc_lurker_nosession").await else { return };
        // Kanal offline → keine active_session_id.
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live) VALUES ('1', 'nani', 0)")
            .execute(&pool).await.unwrap();
        let nicks = vec!["alice".to_string()];
        assert_eq!(upsert_names_batch(&pool, "nani", &nicks, Utc::now()).await, (0, 0));
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
    }
}

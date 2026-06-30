//! `@`-Mention-Netzwerk (`/twitch/api/v2/chat-social-graph`).
//!
//! Port von `bot/analytics/chat_social_graph_loader.py:load_chat_social_graph_payload`.
//! Aus den Chat-Nachrichten mit `@` werden Mentions geparst → Hubs (meist
//! erwähnt/erwähnend), Top-Paare (wer erwähnt wen) und eine Verteilung, plus der
//! geteilte [`crate::raw_chat_status`]-Block.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::raw_chat_status::{build_raw_chat_status, Scope};

/// Bekannte Chat-Bots (deckungsgleich mit `bot/core/chat_bots.py`).
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

/// Python `_MENTION_RE = r"(?<!\w)@([A-Za-z0-9_]{3,25})\b"`. Rusts `regex` kennt kein
/// Lookbehind; `(?:^|\W)` ist das Unicode-treue Äquivalent zu `(?<!\w)` (regex-`\W`/`\b`
/// sind wie Python3 Unicode-aware).
static MENTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\W)@([A-Za-z0-9_]{3,25})\b").unwrap());

/// Lädt das Mention-Netzwerk (Python `load_chat_social_graph_payload`).
pub async fn load_chat_social_graph_payload(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let cutoff: DateTime<Utc> = Utc::now() - Duration::days(days);
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query!(
        r#"
        SELECT m.chatter_login, m.content
        FROM twitch_chat_messages m
        JOIN twitch_stream_sessions s ON s.id = m.session_id
        WHERE LOWER(s.streamer_login) = $1
          AND m.message_ts >= $2
          AND m.content LIKE '%@%'
          AND (m.chatter_login IS NULL OR m.chatter_login = '' OR LOWER(m.chatter_login) <> ALL($3))
        "#,
        streamer,
        cutoff,
        &bots
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.chatter_login, row.content))
    .collect();

    let mut mention_sent: HashMap<String, i64> = HashMap::new();
    let mut mention_received: HashMap<String, i64> = HashMap::new();
    let mut pair_counts: HashMap<(String, String), i64> = HashMap::new();
    let mut total_mentions: i64 = 0;
    let mut mentioners: HashSet<String> = HashSet::new();
    let mut mentioned: HashSet<String> = HashSet::new();

    for (login, content) in &rows {
        let sender = login.as_deref().unwrap_or("").to_lowercase();
        let content = content.as_deref().unwrap_or("");
        for cap in MENTION_RE.captures_iter(content) {
            let target = cap[1].to_lowercase();
            if target == sender {
                continue; // keine Selbst-Mentions
            }
            total_mentions += 1;
            mentioners.insert(sender.clone());
            mentioned.insert(target.clone());
            *mention_sent.entry(sender.clone()).or_insert(0) += 1;
            *mention_received.entry(target.clone()).or_insert(0) += 1;
            *pair_counts
                .entry((sender.clone(), target.clone()))
                .or_insert(0) += 1;
        }
    }

    // Hubs: alle Nutzer (gesendet ∪ empfangen), Score = sent + received, Top 20.
    let mut all_users: HashSet<&String> = mention_sent.keys().collect();
    all_users.extend(mention_received.keys());
    let mut hubs: Vec<Value> = all_users
        .iter()
        .map(|u| {
            let sent = *mention_sent.get(*u).unwrap_or(&0);
            let received = *mention_received.get(*u).unwrap_or(&0);
            json!({ "login": u, "mentionsSent": sent, "mentionsReceived": received, "score": sent + received })
        })
        .collect();
    hubs.sort_by(|a, b| {
        b["score"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["score"].as_i64().unwrap_or(0))
    });
    hubs.truncate(20);

    // Top-Paare nach Häufigkeit, Top 20.
    let mut pairs: Vec<Value> = pair_counts
        .iter()
        .map(|((from, to), c)| json!({ "from": from, "to": to, "count": c }))
        .collect();
    pairs.sort_by(|a, b| {
        b["count"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["count"].as_i64().unwrap_or(0))
    });
    pairs.truncate(20);

    // Verteilung über die Empfangs-Häufigkeit.
    let recv: Vec<i64> = mention_received.values().copied().collect();
    let mentioned_once = recv.iter().filter(|&&c| c == 1).count() as i64;
    let mentioned_2to5 = recv.iter().filter(|&&c| (2..=5).contains(&c)).count() as i64;
    let mentioned_5plus = recv.iter().filter(|&&c| c > 5).count() as i64;

    let raw_chat_status = build_raw_chat_status(pool, streamer, Scope::Since(cutoff)).await?;

    Ok(json!({
        "totalMentions": total_mentions,
        "uniqueMentioners": mentioners.len(),
        "uniqueMentioned": mentioned.len(),
        "hubs": hubs,
        "topPairs": pairs,
        "mentionDistribution": {
            "mentionedOnce": mentioned_once,
            "mentioned2to5": mentioned_2to5,
            "mentioned5plus": mentioned_5plus,
        },
        "rawChatStatus": raw_chat_status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_regex_paritaet() {
        let grab = |s: &str| -> Vec<String> {
            MENTION_RE
                .captures_iter(s)
                .map(|c| c[1].to_lowercase())
                .collect()
        };
        // Standard + Wortgrenze + Mindestlänge 3.
        assert_eq!(grab("hi @Nani und @bob"), vec!["nani", "bob"]);
        assert_eq!(grab("@ab zu kurz"), Vec::<String>::new()); // <3 Zeichen
                                                               // @ direkt nach Wortzeichen → kein Mention (Lookbehind-Ersatz).
        assert_eq!(grab("mail@example"), Vec::<String>::new());
        // @@doppel: erstes @ scheitert (kein Wortzeichen folgt), zweites greift.
        assert_eq!(grab("@@abc"), vec!["abc"]);
        // direkt verkettet: nur das erste zählt (zweites @ folgt Wortzeichen).
        assert_eq!(grab("@abc@def"), vec!["abc"]);
    }

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

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
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, messages INTEGER DEFAULT 0)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, content TEXT, message_ts TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT)").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn mention_netzwerk() {
        let Some(pool) = make_pool("t_csg").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani',NOW()-INTERVAL '1 day')").execute(&pool).await.unwrap();
        // alice→bob (×2), alice→carol, bob→alice; carol erwähnt sich selbst (ignoriert);
        // nightbot ist Bot (gefiltert); Nachricht ohne @ wird gar nicht geladen.
        let msgs = [
            ("alice", "hey @bob und @carol"),
            ("alice", "@bob nochmal"),
            ("bob", "danke @alice"),
            ("carol", "@carol selbstgespraech"),
            ("nightbot", "@alice bot-spam"),
            ("dave", "kein mention hier"),
        ];
        for (login, content) in msgs {
            sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (1,'nani',$1,$2,NOW()-INTERVAL '2 hours')")
                .bind(login).bind(content).execute(&pool).await.unwrap();
        }
        let v = load_chat_social_graph_payload(&pool, "nani", 30)
            .await
            .unwrap();
        // Mentions: alice→bob, alice→carol, alice→bob, bob→alice = 4 (carol-selbst + bot raus).
        assert_eq!(v["totalMentions"], 4);
        assert_eq!(v["uniqueMentioners"], 2); // alice, bob
        assert_eq!(v["uniqueMentioned"], 3); // bob, carol, alice
                                             // Top-Paar: alice→bob mit count 2.
        assert_eq!(v["topPairs"][0]["from"], "alice");
        assert_eq!(v["topPairs"][0]["to"], "bob");
        assert_eq!(v["topPairs"][0]["count"], 2);
        // Hub mit höchstem Score: alice (sent 3, empfängt 1 von bob) → score 4.
        assert_eq!(v["hubs"][0]["login"], "alice");
        assert_eq!(v["hubs"][0]["mentionsSent"], 3);
        assert_eq!(v["hubs"][0]["mentionsReceived"], 1);
        assert_eq!(v["hubs"][0]["score"], 4);
        // Verteilung: bob empfängt 2 (→2to5), carol 1, alice 1 (→once=2).
        assert_eq!(v["mentionDistribution"]["mentionedOnce"], 2);
        assert_eq!(v["mentionDistribution"]["mentioned2to5"], 1);
        assert_eq!(v["rawChatStatus"]["available"], true);
    }
}

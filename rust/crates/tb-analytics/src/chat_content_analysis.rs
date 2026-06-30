//! Chat-Content-Analyse (`/twitch/api/v2/chat-content-analysis`).
//!
//! Port von `bot/analytics/api_chat_deep.py:_load_chat_content_analysis_payload_sync`
//! + die Keyword-Heuristiken. Hero-/Topic-Erkennung, Sentiment-Scoring,
//! Nachrichten-Klassifikation (reaction/greeting/social/smalltalk/community).
//!
//! Die Keyword-Daten liegen exakt aus der Python-Quelle generiert in
//! [`crate::chat_content_lexicon`]. Diese Datei enthält die (selbst geschriebene)
//! Logik. **Teil 1: die pure Detection-Schicht.** Loader + Handler folgen als Teil 2.

use std::collections::HashMap;
use std::sync::LazyLock;

use chrono::{DateTime, Duration, Timelike, Utc};
use regex::Regex;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::chat_content_lexicon::*;
use crate::raw_chat_status::{build_raw_chat_status, Scope};

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

/// topicBreakdown-Schlüssel in fester Reihenfolge (Python-Dict-Init-Reihenfolge).
const TOPIC_BREAKDOWN_KEYS: &[&str] = &[
    "heroes",
    "builds",
    "ranked",
    "meta",
    "gameplay",
    "backseat",
    "commands",
    "social",
    "smalltalk",
    "greeting",
    "community",
    "reaction",
    "other",
];

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Python `_WORD_RE = r"[a-z0-9äöüß_+#']+"` (content ist bereits kleingeschrieben).
static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9äöüß_+#']+").unwrap());

/// Tokenisiert kleingeschriebenen Chat-Text (Python `_tokenize_words`).
pub fn tokenize_words(content_lower: &str) -> Vec<&str> {
    WORD_RE
        .find_iter(content_lower)
        .map(|m| m.as_str())
        .collect()
}

/// Erwähnte Hero-Keys, dedupliziert, in ALIAS_TO_HERO-Reihenfolge (Python `_detect_heroes`).
pub fn detect_heroes(content_lower: &str) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for (alias, hero) in ALIAS_TO_HERO {
        if content_lower.contains(alias) && !found.contains(hero) {
            found.push(hero);
        }
    }
    found
}

/// Getroffene Topic-Kategorien (Python `_detect_topics`).
pub fn detect_topics(content_lower: &str) -> Vec<&'static str> {
    let mut topics: Vec<&'static str> = Vec::new();
    for (topic, keywords) in TOPIC_KEYWORDS {
        if keywords.iter().any(|kw| content_lower.contains(kw)) {
            topics.push(topic);
        }
    }
    topics
}

fn any_contains(haystacks: &[&str], needle: &str) -> bool {
    haystacks.iter().any(|h| needle.contains(h))
}

fn is_alpha_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_lowercase() || matches!(ch, 'ä' | 'ö' | 'ü' | 'ß'))
}

fn count_alpha_words(words: &[&str]) -> usize {
    words.iter().filter(|t| is_alpha_word(t)).count()
}

/// Kurze Emote-/Hype-Nachricht (Python `_is_reaction_message`).
pub fn is_reaction_message(content_lower: &str, words: &[&str]) -> bool {
    let stripped = content_lower.trim();
    if matches!(stripped, "?" | "??" | "!" | "!!") {
        return true;
    }
    if REACTION_PHRASES.iter().any(|p| content_lower.contains(p)) {
        return true;
    }
    // Reine Emoji-/Symbol-Nachrichten (keine alnum-Tokens) sind pure Reaction.
    if words.is_empty() && !stripped.is_empty() {
        return true;
    }
    words.iter().any(|&t| {
        REACTION_TOKENS.contains(&t)
            || EMOTE_PREFIXES.iter().any(|p| t.starts_with(p))
            || EMOTE_SUFFIXES.iter().any(|s| t.ends_with(s))
            || t.starts_with("xd")
            || t.starts_with("haha")
    })
}

/// Bot-/Chat-Command (Python `_is_command_message`).
pub fn is_command_message(content_lower: &str) -> bool {
    content_lower.trim_start().starts_with('!')
}

/// Begrüßung/Verabschiedung (Python `_is_greeting_message`).
pub fn is_greeting_message(content_lower: &str, words: &[&str]) -> bool {
    if GREETING_PHRASES.iter().any(|p| content_lower.contains(p)) {
        return true;
    }
    words.iter().any(|&t| GREETING_TOKENS.contains(&t))
}

/// Social-/Channel-/Meta-Chat (Python `_is_social_message`).
pub fn is_social_message(content_lower: &str) -> bool {
    any_contains(SOCIAL_MARKERS, content_lower)
}

/// Kurze Bestätigungen / leichtes Geplänkel (Python `_is_smalltalk_message`).
pub fn is_smalltalk_message(_content_lower: &str, words: &[&str]) -> bool {
    if words.len() <= 4 && words.iter().any(|&t| SMALLTALK_TOKENS.contains(&t)) {
        return true;
    }
    let alpha = count_alpha_words(words);
    (1..=2).contains(&alpha)
}

/// Community-/Stream-Chat ohne Game-Topic (Python `_looks_like_community_message`).
pub fn looks_like_community_message(content_lower: &str, words: &[&str]) -> bool {
    let alpha = count_alpha_words(words);
    if alpha >= 4 {
        return true;
    }
    if content_lower.contains('?') && alpha >= 2 {
        return true;
    }
    false
}

/// Sentiment: +1 positiv, -1 negativ, 0 neutral (Python `_score_sentiment`).
pub fn score_sentiment(content_lower: &str) -> i32 {
    if content_lower.trim().is_empty() {
        return 0;
    }
    let mut pos = 0i32;
    let mut neg = 0i32;

    // 1) Multi-Wort-Phrasen (Substring).
    for phrase in POSITIVE_PHRASES {
        if content_lower.contains(phrase) {
            pos += 1;
        }
    }
    for phrase in NEGATIVE_PHRASES {
        if content_lower.contains(phrase) {
            neg += 1;
        }
    }

    // 2) Tokenisieren via Whitespace (Python str.split()).
    for token in content_lower.split_whitespace() {
        // 3) Kurze, mehrdeutige Tokens nur als isoliertes Wort.
        if SHORT_POSITIVE.contains(&token) {
            pos += 1;
            continue;
        }
        if SHORT_NEGATIVE.contains(&token) {
            neg += 1;
            continue;
        }
        // 4) Reguläre Wörter (sehr kurze überspringen).
        if token.chars().count() < 2 {
            continue;
        }
        if POSITIVE_WORDS.contains(&token) {
            pos += 1;
        } else if NEGATIVE_WORDS.contains(&token) {
            neg += 1;
        }
    }

    // 5) Mehrheit entscheidet.
    if pos > neg {
        1
    } else if neg > pos {
        -1
    } else {
        0
    }
}

/// Lädt die Chat-Content-Analyse (Python `_load_chat_content_analysis_payload_sync`).
pub async fn load_chat_content_analysis_payload(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let cutoff: DateTime<Utc> = Utc::now() - Duration::days(days);
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    let rows: Vec<(DateTime<Utc>, String, Option<String>)> = sqlx::query!(
        r#"
        SELECT m.message_ts AS "message_ts!",
               m.content AS "content!",
               m.chatter_login
        FROM twitch_chat_messages m
        JOIN twitch_stream_sessions s ON s.id = m.session_id
        WHERE LOWER(s.streamer_login) = $1
          AND m.message_ts >= $2
          AND m.content IS NOT NULL
          AND m.content != ''
          AND (m.chatter_login IS NULL OR m.chatter_login = '' OR LOWER(m.chatter_login) <> ALL($3))
        ORDER BY m.message_ts
        "#,
        streamer,
        cutoff,
        &bots
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.message_ts, row.content, row.chatter_login))
    .collect();

    let mut hero_counts: HashMap<&'static str, i64> = HashMap::new();
    let mut hero_order: Vec<&'static str> = Vec::new(); // First-Seen (Query ist ORDER BY ts → deterministisch)
    let mut topic_counts: HashMap<&'static str, i64> =
        TOPIC_BREAKDOWN_KEYS.iter().map(|k| (*k, 0i64)).collect();
    let mut sentiment_buckets: HashMap<String, (i64, i64, i64)> = HashMap::new();
    let mut total_positive = 0i64;
    let mut total_negative = 0i64;
    let mut backseat_count = 0i64;
    let mut backseat_examples: Vec<String> = Vec::new();
    let mut depth_reaction = 0i64;
    let mut depth_short = 0i64;
    let mut depth_discussion = 0i64;
    let mut total_words = 0i64;

    for (ts, content, _login) in &rows {
        let content_lower = content.to_lowercase();

        let heroes = detect_heroes(&content_lower);
        for h in &heroes {
            if !hero_counts.contains_key(h) {
                hero_order.push(h);
            }
            *hero_counts.entry(h).or_insert(0) += 1;
        }

        let topics = detect_topics(&content_lower);
        let mut matched_any = false;
        if !heroes.is_empty() {
            *topic_counts.get_mut("heroes").unwrap() += 1;
            matched_any = true;
        }
        for t in &topics {
            *topic_counts.get_mut(t).unwrap() += 1;
            matched_any = true;
        }

        let is_backseat = BACKSEAT_PHRASES.iter().any(|p| content_lower.contains(p));
        if is_backseat {
            *topic_counts.get_mut("backseat").unwrap() += 1;
            matched_any = true;
            backseat_count += 1;
            if backseat_examples.len() < 10 {
                let ex = if content.chars().count() > 80 {
                    let s: String = content.chars().take(80).collect();
                    format!("{s}...")
                } else {
                    content.clone()
                };
                backseat_examples.push(ex);
            }
        }

        if !matched_any {
            let tokens = tokenize_words(&content_lower);
            if is_reaction_message(&content_lower, &tokens) {
                *topic_counts.get_mut("reaction").unwrap() += 1;
                matched_any = true;
            } else if is_greeting_message(&content_lower, &tokens) {
                *topic_counts.get_mut("greeting").unwrap() += 1;
                matched_any = true;
            } else if is_command_message(&content_lower) {
                *topic_counts.get_mut("commands").unwrap() += 1;
                matched_any = true;
            } else if is_social_message(&content_lower) {
                *topic_counts.get_mut("social").unwrap() += 1;
                matched_any = true;
            } else if is_smalltalk_message(&content_lower, &tokens) {
                *topic_counts.get_mut("smalltalk").unwrap() += 1;
                matched_any = true;
            } else if looks_like_community_message(&content_lower, &tokens) {
                *topic_counts.get_mut("community").unwrap() += 1;
                matched_any = true;
            }
        }
        if !matched_any {
            *topic_counts.get_mut("other").unwrap() += 1;
        }

        let word_count = content.split_whitespace().count() as i64;
        total_words += word_count;
        if word_count <= 3 {
            depth_reaction += 1;
        } else if word_count <= 10 {
            depth_short += 1;
        } else {
            depth_discussion += 1;
        }

        let score = score_sentiment(&content_lower);
        let bucket_min = ts.minute() - (ts.minute() % 15);
        let bucket_key = format!(
            "{}T{:02}:{:02}",
            ts.format("%Y-%m-%d"),
            ts.hour(),
            bucket_min
        );
        let entry = sentiment_buckets.entry(bucket_key).or_insert((0, 0, 0));
        if score > 0 {
            entry.0 += 1;
            total_positive += 1;
        } else if score < 0 {
            entry.1 += 1;
            total_negative += 1;
        } else {
            entry.2 += 1;
        }
    }

    // heroMentions: First-Seen-Reihenfolge, stabil nach count absteigend, Top 25.
    let total_hero_mentions: i64 = hero_counts.values().sum();
    let mut hero_mentions: Vec<Value> = hero_order
        .iter()
        .map(|h| {
            let count = hero_counts[h];
            let pct = if total_hero_mentions != 0 {
                json!(round1(count as f64 / total_hero_mentions as f64 * 100.0))
            } else {
                json!(0)
            };
            json!({ "hero": h, "count": count, "pct": pct })
        })
        .collect();
    hero_mentions.sort_by(|a, b| {
        b["count"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["count"].as_i64().unwrap_or(0))
    });
    hero_mentions.truncate(25);

    // sentimentTimeline: nach Bucket-Key sortiert.
    let mut bucket_vec: Vec<(&String, &(i64, i64, i64))> = sentiment_buckets.iter().collect();
    bucket_vec.sort_by(|a, b| a.0.cmp(b.0));
    let sentiment_timeline: Vec<Value> = bucket_vec
        .iter()
        .map(|(bucket, (pos, neg, _neu))| {
            let score = round2((pos - neg) as f64 / (pos + neg).max(1) as f64);
            json!({ "bucket": bucket, "positive": pos, "negative": neg, "score": score })
        })
        .collect();

    let total_analyzed = rows.len() as i64;
    let scored_total = total_positive + total_negative;
    let overall_score_f = if scored_total > 0 {
        round2((total_positive - total_negative) as f64 / scored_total.max(1) as f64)
    } else {
        0.0
    };
    let overall_score_val = if scored_total > 0 {
        json!(overall_score_f)
    } else {
        json!(0)
    };

    let trend = if sentiment_timeline.len() >= 4 {
        let mid = sentiment_timeline.len() / 2;
        let avg = |slice: &[Value]| -> f64 {
            if slice.is_empty() {
                0.0
            } else {
                slice
                    .iter()
                    .map(|s| s["score"].as_f64().unwrap_or(0.0))
                    .sum::<f64>()
                    / slice.len() as f64
            }
        };
        let first_avg = avg(&sentiment_timeline[..mid]);
        let second_avg = avg(&sentiment_timeline[mid..]);
        if second_avg > first_avg + 0.1 {
            "rising"
        } else if first_avg > second_avg + 0.1 {
            "falling"
        } else {
            "stable"
        }
    } else {
        "insufficient_data"
    };

    let label = if overall_score_f > 0.2 {
        "positiv"
    } else if overall_score_f < -0.2 {
        "negativ"
    } else {
        "neutral"
    };
    let depth_total = depth_reaction + depth_short + depth_discussion;
    let depth_pct = |v: i64| round1(v as f64 / depth_total.max(1) as f64 * 100.0);
    let backseat_pct = round1(backseat_count as f64 / total_analyzed.max(1) as f64 * 100.0);
    let avg_word_count = round1(total_words as f64 / total_analyzed.max(1) as f64);

    let raw_chat_status = build_raw_chat_status(pool, streamer, Scope::Since(cutoff)).await?;

    let mut topic_breakdown = serde_json::Map::new();
    for key in TOPIC_BREAKDOWN_KEYS {
        topic_breakdown.insert(key.to_string(), json!(topic_counts[key]));
    }

    Ok(json!({
        "heroMentions": hero_mentions,
        "topicBreakdown": Value::Object(topic_breakdown),
        "sentimentTimeline": sentiment_timeline,
        "overallSentiment": {
            "score": overall_score_val,
            "label": label,
            "trend": trend,
            "totalAnalyzed": total_analyzed,
            "positiveCount": total_positive,
            "negativeCount": total_negative,
        },
        "backseat": {
            "count": backseat_count,
            "pct": backseat_pct,
            "examples": backseat_examples,
        },
        "engagementDepth": {
            "reaction": depth_reaction,
            "reactionPct": depth_pct(depth_reaction),
            "short": depth_short,
            "shortPct": depth_pct(depth_short),
            "discussion": depth_discussion,
            "discussionPct": depth_pct(depth_discussion),
            "total": depth_total,
            "avgWordCount": avg_word_count,
        },
        "rawChatStatus": raw_chat_status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize() {
        assert_eq!(
            tokenize_words("hey! <3 lol c++"),
            vec!["hey", "3", "lol", "c++"]
        );
        assert_eq!(
            tokenize_words("schön übel ärgerlich"),
            vec!["schön", "übel", "ärgerlich"]
        );
    }

    #[test]
    fn heroes_und_topics() {
        // "talon" → grey_talon (steht in ALIAS-Reihenfolge vor haze).
        assert_eq!(
            detect_heroes("nice talon und haze play"),
            vec!["grey_talon", "haze"]
        );
        assert_eq!(detect_heroes("kein hero hier"), Vec::<&str>::new());
        assert_eq!(detect_topics("the meta is broken, pls nerf"), vec!["meta"]);
        assert_eq!(detect_topics("guter build mit item"), vec!["builds"]);
    }

    #[test]
    fn sentiment() {
        assert_eq!(score_sentiment("gg nice clutch"), 1); // gg(short)+nice+clutch
        assert_eq!(score_sentiment("trash garbage"), -1);
        assert_eq!(score_sentiment("hello world"), 0);
        assert_eq!(score_sentiment("lets go that was so good"), 1); // 2 Phrasen
        assert_eq!(score_sentiment(""), 0);
        assert_eq!(score_sentiment("w"), 1); // SHORT_POSITIVE isoliert
        assert_eq!(score_sentiment("ff"), -1); // SHORT_NEGATIVE isoliert
    }

    #[test]
    fn klassifikation() {
        assert!(is_reaction_message("kekw", &tokenize_words("kekw")));
        assert!(is_reaction_message("??", &tokenize_words("??")));
        assert!(is_reaction_message("xddd", &tokenize_words("xddd"))); // startswith xd
        assert!(is_command_message("!uptime"));
        assert!(!is_command_message("kein command"));
        assert!(is_greeting_message(
            "moin zusammen",
            &tokenize_words("moin zusammen")
        ));
        assert!(is_social_message("schau auf meinem discord"));
        assert!(is_smalltalk_message("ja", &tokenize_words("ja")));
        assert!(looks_like_community_message(
            "warum macht ihr das alle",
            &tokenize_words("warum macht ihr das alle")
        ));
        assert!(looks_like_community_message(
            "was geht?",
            &tokenize_words("was geht?")
        ));
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
    async fn loader_aggregiert() {
        let Some(pool) = make_pool("t_cca").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani',NOW()-INTERVAL '1 day')").execute(&pool).await.unwrap();
        let msgs = [
            "haze ist so stark gg",       // hero=haze, sentiment+ (stark/gg)
            "nice talon play",            // hero=grey_talon, sentiment+ (nice)
            "you should just buy spirit", // backseat (you should/just buy) + topic builds(spirit)
            "moin zusammen",              // greeting
            "trash game so boring",       // sentiment- (trash + phrase 'so boring')
        ];
        for m in msgs {
            sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (1,'nani','viewer',$1,NOW()-INTERVAL '2 hours')")
                .bind(m).execute(&pool).await.unwrap();
        }
        let v = load_chat_content_analysis_payload(&pool, "nani", 30)
            .await
            .unwrap();
        // Hero-Mentions: haze + grey_talon je 1.
        let heroes: Vec<&str> = v["heroMentions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["hero"].as_str().unwrap())
            .collect();
        assert!(heroes.contains(&"haze") && heroes.contains(&"grey_talon"));
        // Backseat erkannt (1×).
        assert_eq!(v["backseat"]["count"], 1);
        assert_eq!(v["backseat"]["examples"].as_array().unwrap().len(), 1);
        // topicBreakdown vollständig (13 Keys) + heroes-Topic >=2.
        assert_eq!(v["topicBreakdown"].as_object().unwrap().len(), 13);
        assert_eq!(v["topicBreakdown"]["heroes"], 2);
        assert_eq!(v["topicBreakdown"]["greeting"], 1);
        // Sentiment: 2 positiv (haze/talon-Msgs), 1 negativ → overall positiv-ish.
        assert_eq!(v["overallSentiment"]["totalAnalyzed"], 5);
        assert_eq!(v["overallSentiment"]["positiveCount"], 2);
        assert_eq!(v["overallSentiment"]["negativeCount"], 1);
        assert_eq!(v["rawChatStatus"]["available"], true);
        assert_eq!(v["engagementDepth"]["total"], 5);
    }
}

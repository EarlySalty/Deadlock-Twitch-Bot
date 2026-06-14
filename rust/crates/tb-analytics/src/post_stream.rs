//! Post-Stream-Analyse (B11-Post-Stream-Trigger). Port von
//! `bot/analytics/api_post_stream.py` + `post_stream/report_builder.py`.
//!
//! GROSSES Subsystem, Bottom-up portiert. Slice 1 (diese Datei zu Beginn): die
//! fundamentale Datenquelle `load_session_chat_data` — Session-Metadaten +
//! Chat-Nachrichten einer abgeschlossenen Session. Sie speist später die
//! KI-Wortgruppen und den Report-Builder. Weitere Slices: Wortgruppen-AI,
//! Report-Snapshot-Builder, Report-AI, Trigger-Wiring in on_stream_offline.
//!
//! LATENTER PYTHON-BUG (bewusst 1:1 portiert): `extract_json_object` sucht — wie
//! Python `_extract_json_object` — `{}` VOR `[]`. Für eine Wortgruppen-Antwort
//! (ein JSON-Array `[{...}]`) liefert es daher das erste innere Objekt statt des
//! Arrays; der nachgelagerte `startswith("[")`-Check schlägt fehl → Wortgruppen
//! bleiben in Python effektiv leer. Für den Report (ein Objekt) ist die Funktion
//! korrekt. Nicht „gefixt", da Nani-Direktive = An/Aus-Zustand 1:1 übernehmen;
//! falls gewünscht, wäre ein separater Array-Extraktor für den Wortgruppen-Pfad
//! der saubere Build-to-intent-Fix.

use regex::Regex;
use sqlx::PgPool;

/// Session-Metadaten einer abgeschlossenen Stream-Session
/// (Python `_load_session_chat_data` → `session`).
#[derive(Debug, Clone)]
pub struct PostStreamSession {
    pub streamer_login: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_seconds: i64,
    pub avg_viewers: f64,
    pub peak_viewers: i64,
    pub followers_delta: i64,
}

/// Geladene Chat-/Session-Daten für die Post-Stream-Analyse
/// (Python `_load_session_chat_data`-Rückgabe).
#[derive(Debug, Clone)]
pub struct SessionChatData {
    pub session: PostStreamSession,
    /// Nicht-Command-Nachrichten der Session (≤ 1500, nach Zeit sortiert).
    pub messages: Vec<String>,
    /// Dauer in Minuten, mindestens 1 (Python `duration_min`).
    pub duration_min: i64,
    pub unique_chatters: i64,
}

/// Lädt Session-Metadaten + Chat-Nachrichten einer Session (Python
/// `_load_session_chat_data`). `None`, wenn die Session nicht existiert.
pub async fn load_session_chat_data(pool: &PgPool, session_id: i64) -> Option<SessionChatData> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<f64>,
            Option<i64>,
            Option<i64>,
        ),
    >(
        "SELECT s.streamer_login, \
                s.started_at::text, \
                s.ended_at::text, \
                s.duration_seconds::int8, \
                COALESCE(s.avg_viewers, 0)::float8, \
                COALESCE(s.peak_viewers, 0)::int8, \
                COALESCE(s.follower_delta, 0)::int8 \
         FROM twitch_stream_sessions s \
         WHERE s.id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;

    let (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, followers_delta) =
        row;
    let duration_seconds = duration_seconds.unwrap_or(0);

    let messages: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT content FROM twitch_chat_messages \
         WHERE session_id = $1 \
           AND is_command = FALSE \
           AND content IS NOT NULL \
           AND length(content) > 1 \
         ORDER BY message_ts \
         LIMIT 1500",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|c| c.trim().to_string())
    .filter(|c| !c.is_empty())
    .collect();

    let unique_chatters: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT chatter_login) FROM twitch_chat_messages \
         WHERE session_id = $1 AND chatter_login IS NOT NULL",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    // Python: max(1, duration_seconds // 60).
    let duration_min = (duration_seconds / 60).max(1);

    Some(SessionChatData {
        session: PostStreamSession {
            streamer_login,
            started_at,
            ended_at,
            duration_seconds,
            avg_viewers: avg_viewers.unwrap_or(0.0),
            peak_viewers: peak_viewers.unwrap_or(0),
            followers_delta: followers_delta.unwrap_or(0),
        },
        messages,
        duration_min,
        unique_chatters,
    })
}

// ---------------------------------------------------------------------------
// KI-Wortgruppen — pure Logik (Python `_generate_word_groups` + JSON-/Prompt-Helfer)
// ---------------------------------------------------------------------------

/// Eine thematische Chat-Wortgruppe (Python-Normalisierung in `_generate_word_groups`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordGroup {
    pub group_name: String,
    pub keywords: Vec<String>,
    pub message_count: i64,
}

/// Prompt-Template für die Wortgruppen-Analyse (Python `_WORD_GROUP_PROMPT_TEMPLATE`,
/// ASCII-Umschreibung wie in der Quelle). `{n}`/`{sample}` werden ersetzt.
const WORD_GROUP_PROMPT_TEMPLATE: &str = r#"Du analysierst den Twitch-Chat eines Gaming-Streams. Es wurden {n} Chat-Nachrichten erfasst.

Erkenne 5-10 thematische Wortgruppen (z.B. Lob, Kritik, Emote-Spam, Hero-Bezuege, Gameplay-Feedback, Fragen, Negativitaet, Hype-Momente, Community-Inside-Jokes).

Fuer jede Gruppe:
- group_name: kurzer deutscher Name (2-3 Woerter max)
- keywords: haeufigste Woerter/Phrasen dieser Gruppe (max. 15, Kleinbuchstaben)
- message_count: geschaetzte Anzahl Nachrichten dieser Gruppe

Chat-Nachrichten (Stichprobe):
{sample}

Antworte NUR als JSON-Array ohne weitere Erklaerungen:
[{"group_name": "...", "keywords": ["..."], "message_count": 0}]"#;

/// Entfernt `<think>…</think>`-Blöcke (Python `_THINK_BLOCK_RE`, DOTALL+IGNORECASE).
fn strip_think_blocks(text: &str) -> String {
    Regex::new(r"(?si)<think>.*?</think>")
        .unwrap()
        .replace_all(text, "")
        .into_owned()
}

/// Extrahiert den ersten vollständigen JSON-Block (`{…}` oder `[…]`) aus einem
/// Modell-Output — string-/escape-aware, nach `<think>`-Strip
/// (Python `_extract_json_object`).
pub fn extract_json_object(text: &str) -> Option<String> {
    let text = strip_think_blocks(text);
    for (start_char, end_char) in [('{', '}'), ('[', ']')] {
        let Some(start) = text.find(start_char) else {
            continue;
        };
        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escape_next = false;
        for (i, ch) in text[start..].char_indices() {
            if escape_next {
                escape_next = false;
                continue;
            }
            if ch == '\\' && in_string {
                escape_next = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
                continue;
            }
            if in_string {
                continue;
            }
            if ch == start_char {
                depth += 1;
            } else if ch == end_char {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + ch.len_utf8()].to_string());
                }
            }
        }
    }
    None
}

/// `serde_json::from_str` mit Trailing-Comma-Reparatur (Python `_loads_ai_json`).
pub fn loads_ai_json(extracted: &str) -> Option<serde_json::Value> {
    if let Ok(value) = serde_json::from_str(extracted) {
        return Some(value);
    }
    let repaired = Regex::new(r",\s*([}\]])")
        .unwrap()
        .replace_all(extracted, "$1");
    serde_json::from_str(&repaired).ok()
}

/// Normalisiert eine Nachricht fürs Prompt (Whitespace zusammenfassen, auf
/// `limit` Zeichen kürzen mit „…"-Suffix) — Python `_clean_prompt_message`.
pub fn clean_prompt_message(message: &str, limit: usize) -> String {
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        return normalized;
    }
    let truncated: String = normalized.chars().take(limit.saturating_sub(3)).collect();
    format!("{}...", truncated.trim_end())
}

/// Stichprobe der Nachrichten (Python `messages[::step][:300]`, `step=max(1,len/300)`).
pub fn sample_messages(messages: &[String]) -> Vec<&str> {
    let step = (messages.len() / 300).max(1);
    messages
        .iter()
        .step_by(step)
        .take(300)
        .map(String::as_str)
        .collect()
}

/// Baut das Wortgruppen-Prompt (Python `_WORD_GROUP_PROMPT_TEMPLATE.format`).
pub fn build_word_group_prompt(total_count: usize, messages: &[String]) -> String {
    let sample_text = sample_messages(messages)
        .iter()
        .map(|m| format!("- {}", clean_prompt_message(m, 240)))
        .collect::<Vec<_>>()
        .join("\n");
    WORD_GROUP_PROMPT_TEMPLATE
        .replace("{n}", &total_count.to_string())
        .replace("{sample}", &sample_text)
}

fn keyword_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.trim().to_lowercase(),
        other => other.to_string().trim().to_lowercase(),
    }
}

/// Normalisiert die KI-JSON-Antwort zu Wortgruppen (Python-Normalisierung in
/// `_generate_word_groups`): `group_name` (≤80, nicht-leer), `keywords` (≤15,
/// lowercased, nicht-leer), `message_count` (≥0), maximal 10 Gruppen.
pub fn normalize_word_groups(value: &serde_json::Value) -> Vec<WordGroup> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<WordGroup> = Vec::new();
    for group in arr {
        let Some(obj) = group.as_object() else {
            continue;
        };
        let group_name = obj
            .get("group_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if group_name.is_empty() {
            continue;
        }
        let group_name: String = group_name.chars().take(80).collect();
        let keywords: Vec<String> = obj
            .get("keywords")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .map(keyword_to_string)
                    .filter(|k| !k.is_empty())
                    .take(15)
                    .collect()
            })
            .unwrap_or_default();
        let message_count = obj
            .get("message_count")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
            })
            .unwrap_or(0)
            .max(0);
        out.push(WordGroup {
            group_name,
            keywords,
            message_count,
        });
        if out.len() >= 10 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[test]
    fn extract_json_object_faithful_objekt_zuerst_und_think() {
        // FAITHFUL zu Python `_extract_json_object`: `{}` wird ZUERST gesucht,
        // daher liefert ein Array `[{...}]` das erste innere OBJEKT (das `}` im
        // String wird korrekt übersprungen). Konsequenz: die Wortgruppen-
        // Extraktion (prüft danach `startswith("[")`) ist in Python latent
        // gebrochen — bewusst 1:1 portiert (siehe Modul-Doku).
        let raw = "bla [{\"group_name\": \"a }not end\", \"keywords\": []}] danke";
        assert_eq!(
            extract_json_object(raw).as_deref(),
            Some("{\"group_name\": \"a }not end\", \"keywords\": []}")
        );
        // <think>-Block wird entfernt; danach das Objekt.
        let think = "<think>egal {nope}</think> {\"x\": 1}";
        assert_eq!(extract_json_object(think).as_deref(), Some("{\"x\": 1}"));
        // Kein JSON → None.
        assert!(extract_json_object("nur text").is_none());
    }

    #[test]
    fn loads_ai_json_repariert_trailing_comma() {
        assert!(loads_ai_json("{\"a\": 1,}").is_some());
        let v = loads_ai_json("[1, 2, 3,]").unwrap();
        assert_eq!(v.as_array().unwrap().len(), 3);
    }

    #[test]
    fn clean_prompt_message_kuerzt() {
        assert_eq!(clean_prompt_message("hallo    welt", 240), "hallo welt");
        let long = "a".repeat(300);
        let cleaned = clean_prompt_message(&long, 240);
        assert_eq!(cleaned.chars().count(), 240);
        assert!(cleaned.ends_with("..."));
    }

    #[test]
    fn sample_messages_step() {
        // 600 Nachrichten → step 2 → 300 Samples.
        let msgs: Vec<String> = (0..600).map(|i| i.to_string()).collect();
        let sample = sample_messages(&msgs);
        assert_eq!(sample.len(), 300);
        assert_eq!(sample[0], "0");
        assert_eq!(sample[1], "2");
    }

    #[test]
    fn normalize_word_groups_limits_und_lowercase() {
        let json = serde_json::json!([
            {"group_name": "  Lob  ", "keywords": ["GG", "Nice", "", "POG"], "message_count": 5},
            {"group_name": "", "keywords": ["x"], "message_count": 1},
            {"group_name": "Kritik", "keywords": [], "message_count": "3"}
        ]);
        let groups = normalize_word_groups(&json);
        assert_eq!(groups.len(), 2); // leerer group_name gefiltert
        assert_eq!(groups[0].group_name, "Lob");
        assert_eq!(groups[0].keywords, vec!["gg", "nice", "pog"]);
        assert_eq!(groups[0].message_count, 5);
        // message_count als String "3" → 3.
        assert_eq!(groups[1].message_count, 3);
    }

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (id BIGINT PRIMARY KEY, streamer_login TEXT, \
             started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, \
             avg_viewers DOUBLE PRECISION, peak_viewers INTEGER, follower_delta INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_chat_messages (session_id BIGINT, content TEXT, is_command BOOLEAN, \
             message_ts TIMESTAMPTZ, chatter_login TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn laedt_session_metadaten_und_gefilterte_messages() {
        let Some(pool) = pool_or_skip("t6e_post_stream").await else { return };
        sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (id, streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, follower_delta) \
             VALUES (1,'streamer','2026-06-10T18:00:00+00','2026-06-10T20:00:00+00',7200,12.5,40,7)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_chat_messages (session_id, content, is_command, message_ts, chatter_login) VALUES \
             (1,'hallo zusammen', FALSE, '2026-06-10T18:05:00+00','a'), \
             (1,'!ping', TRUE, '2026-06-10T18:06:00+00','b'), \
             (1,'x', FALSE, '2026-06-10T18:07:00+00','c'), \
             (1,'gutes spiel', FALSE, '2026-06-10T18:08:00+00','a'), \
             (1,NULL, FALSE, '2026-06-10T18:09:00+00','d')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let data = load_session_chat_data(&pool, 1).await.expect("session vorhanden");
        assert_eq!(data.session.streamer_login, "streamer");
        assert_eq!(data.session.duration_seconds, 7200);
        assert_eq!(data.session.avg_viewers, 12.5);
        assert_eq!(data.session.peak_viewers, 40);
        assert_eq!(data.session.followers_delta, 7);
        assert_eq!(data.duration_min, 120); // 7200/60
        // Command (!ping), zu kurz (x) und NULL gefiltert → nur 2 echte Nachrichten.
        assert_eq!(data.messages, vec!["hallo zusammen".to_string(), "gutes spiel".to_string()]);
        // 4 distinkte Chatter (a,b,c,d).
        assert_eq!(data.unique_chatters, 4);

        // Unbekannte Session → None.
        assert!(load_session_chat_data(&pool, 999).await.is_none());
    }
}

//! Datenschicht + reine Helfer für `/twitch/api/v2/chat-deep-minimax`.
//!
//! Port von `bot/analytics/api_chat_deep.py:_api_v2_chat_minimax_deep`.
//! Holt die Chat-Nachrichten einer Session (Bot-gefiltert, max. 1000), baut den
//! deutschen Analyse-Prompt und extrahiert das JSON-Objekt aus der MiniMax-
//! Antwort. Der eigentliche MiniMax-Call lebt im Dashboard-Handler.

use sqlx::PgPool;

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

/// Holt bis zu 1000 nicht-leere, nicht-Bot-Chat-Nachrichten einer Session,
/// chronologisch. `session_id_raw` wird wie in Python als Roh-String gebunden
/// und per `::bigint` gecastet (mirror Pythons untypisierte Literal-Coercion).
pub async fn fetch_session_messages(
    pool: &PgPool,
    session_id_raw: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();
    let rows = sqlx::query_scalar!(
        r#"
        SELECT m.content AS "content!"
        FROM twitch_chat_messages m
        WHERE m.session_id = $1::text::bigint
          AND m.content IS NOT NULL
          AND m.content != ''
          AND (m.chatter_login IS NULL OR m.chatter_login = '' OR LOWER(m.chatter_login) <> ALL($2))
        ORDER BY m.message_ts
        LIMIT 1000
        "#,
        session_id_raw,
        &bots
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Statischer Prompt-Kopf (1:1 Python, escaped `{{`/`}}` → literale Klammern).
const DEEP_PROMPT_PREFIX: &str = r#"Du bist ein Twitch-Analytics-Experte. Analysiere die folgende Liste von Chat-Nachrichten eines Deadlock-Streams.

Deine Aufgabe:
1. Kategorisiere die Nachrichten in diese Typen (gib Counts zurück):
   - Greeting (Begrüßung/Abschied)
   - Question (Fragen zum Spiel/Streamer)
   - Reaction (Emotes, Lachen, 'lol', 'gg')
   - Hype (Hype-Momente, Raids, 'pog')
   - Game-Related (Strategie, Helden, Meta)
   - Feedback (Lob/Kritik am Stream)
   - Technical (Ton/Bild-Probleme)
   - Social (Discord/Social Media)
   - Other (Rest)

2. Bewerte die "Chat-Tiefe" (Chat Depth) insgesamt (0-100) und gib eine kurze Begründung.

3. Identifiziere die Top 3 Themen.

Antworte NUR als JSON:
{
  "category_counts": {"Greeting": 0, "Question": 0, "Reaction": 0, "Hype": 0, "Game-Related": 0, "Feedback": 0, "Technical": 0, "Social": 0, "Other": 0},
  "chat_depth_score": 0,
  "chat_depth_explanation": "...",
  "top_topics": ["...", "...", "..."]
}

Hier sind die Nachrichten:
"#;

/// Baut den Analyse-Prompt. Die Nachrichten werden als JSON-Array angehängt —
/// byte-gleich zu `json.dumps(messages[:1000], ensure_ascii=False)`: jede
/// Nachricht einzeln escaped (Non-ASCII bleibt erhalten), mit `, ` verbunden.
pub fn build_deep_prompt(messages: &[String]) -> String {
    let items: Vec<String> = messages
        .iter()
        .take(1000)
        .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "\"\"".to_string()))
        .collect();
    format!("{DEEP_PROMPT_PREFIX}[{}]\n", items.join(", "))
}

/// Extrahiert das JSON-Objekt aus der LLM-Antwort: vom ersten `{` bis zum
/// letzten `}` (1:1 `content.find/rfind`). Kein `{`/`}` → ganzer Text;
/// `}` vor `{` → leer (wie Pythons `content[start:end+1]` mit start > end).
pub fn extract_json_object(content: &str) -> &str {
    match (content.find('{'), content.rfind('}')) {
        (Some(s), Some(e)) if e >= s => &content[s..=e],
        (Some(_), Some(_)) => "",
        _ => content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn build_deep_prompt_json_array() {
        let p = build_deep_prompt(&["hallo".to_string(), "grüße".to_string()]);
        // Non-ASCII bleibt erhalten (ensure_ascii=False), `, `-Separator.
        assert!(p.ends_with("Hier sind die Nachrichten:\n[\"hallo\", \"grüße\"]\n"));
        assert!(p.starts_with("Du bist ein Twitch-Analytics-Experte."));
    }

    #[test]
    fn extract_json_object_faelle() {
        assert_eq!(
            extract_json_object("vortext {\"a\": 1} nachtext"),
            "{\"a\": 1}"
        );
        assert_eq!(extract_json_object("kein json"), "kein json");
        assert_eq!(extract_json_object("}{"), ""); // end < start
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
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, chatter_login TEXT, content TEXT, message_ts TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn fetch_session_messages_filtert() {
        let Some(pool) = make_pool("t_deep_fetch").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_chat_messages (session_id, chatter_login, content, message_ts) VALUES \
            (5, 'alice', 'erste',  NOW()-INTERVAL '3 min'), \
            (5, 'nightbot', 'bot-spam', NOW()-INTERVAL '2 min'), \
            (5, 'bob', '', NOW()-INTERVAL '1 min'), \
            (5, 'carol', 'zweite', NOW()), \
            (9, 'dave', 'andere session', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Roh-String "5" → ::bigint; Bot + leere + fremde Session raus, chronologisch.
        let msgs = fetch_session_messages(&pool, "5").await.unwrap();
        assert_eq!(msgs, vec!["erste".to_string(), "zweite".to_string()]);
    }
}

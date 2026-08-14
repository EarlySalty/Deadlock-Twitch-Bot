//! Globaler Community-Sentiment für den Engagement-Layer (Port von
//! `bot/engagement/global_sentiment.py`).
//!
//! Ein Background-Job wirft die letzten Chat-Nachrichten ALLER Channels zusammen
//! und destilliert per MiniMax ein kompaktes Stimmungsbild („wie fühlt sich
//! Deadlock gerade an"). Persistiert in `twitch_engagement_global_sentiment`;
//! die Pipeline liest nur die neueste frische Zeile als ambientes Bauchgefühl.
//! Halluzinations-sicher: nur echte Nachrichten, nie als Statistik vorlesen.

use sqlx::PgPool;

use crate::llm_chat::{strip_think, EngagementLlmClient};

const POOL_LIMIT: i64 = 250;
const POOL_MAX_AGE_HOURS: i32 = 336; // 14 Tage Backstop
const MIN_MSGS_TO_BUILD: usize = 8;
/// Älteres Sentiment gilt als nicht mehr „aktuell" (kein Fragment).
pub const FRESH_MAX_AGE_HOURS: i32 = 12;
const BUILD_MAX_TOKENS: i64 = 4000;
const KEEP_ROWS: i64 = 50;

const SYS: &str = "Du bist ein nüchterner Analyst. Gib nur die verlangte Zusammenfassung, \
kein Vorwort, keine Meta.";

/// Build-Prompt aus den gepoolten Nachrichten (Python `_build_user_prompt`).
fn build_user_prompt(lines: &[String]) -> String {
    let block = lines
        .iter()
        .map(|m| format!("- {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Hier echte Twitch-Chat-Nachrichten aus mehreren Deadlock-Streams (zusammengeworfen). \
         Destillier in 3-6 knappen Stichpunkten, wie sich Deadlock GERADE anfühlt — Stimmung, \
         Meta, Patch-Reaktionen, welche Helden/Items auffallen, was nervt, was gefeiert wird. \
         Nutze NUR was in den Nachrichten steht, erfinde NICHTS. Ist die Datenlage für einen \
         Punkt zu dünn, lass ihn weg. Sachlich, interne Stimmungs-Notiz.\n\nNachrichten:\n{block}"
    )
}

/// Prompt-Fragment aus dem Sentiment-Text (Python `build_sentiment_fragment`).
pub fn build_sentiment_fragment(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    format!(
        "So fühlt sich Deadlock in der Community gerade an — das ist DEIN eigenes Bauchgefühl \
         zur aktuellen Lage (Meta, Patches, was nervt/gefeiert wird). Zieh daraus Meinung und \
         Vibe, aber lies es NIEMALS als Liste oder Statistik vor und sag nie, woher du das hast. \
         Lass nur beiläufig einfließen, was gerade zum Gespräch passt:\n{text}"
    )
}

/// Globaler-Sentiment-Provider.
pub struct GlobalSentiment {
    pool: PgPool,
}

impl GlobalSentiment {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn load_pooled(&self) -> Vec<String> {
        sqlx::query_scalar!(
            r#"SELECT content AS "content!" FROM twitch_engagement_conversation
             WHERE role = 'user' AND ts > NOW() - make_interval(hours => $1)
             ORDER BY ts DESC LIMIT $2"#,
            POOL_MAX_AGE_HOURS,
            POOL_LIMIT
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            let t = c.trim().to_string();
            (t.chars().count() > 3).then_some(t)
        })
        .collect()
    }

    async fn store(&self, text: &str, msg_count: i64, model: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO twitch_engagement_global_sentiment (sentiment_text, msg_count, model) \
             VALUES ($1, $2::int8, $3)",
            text,
            msg_count,
            model
        )
        .execute(&self.pool)
        .await?;
        sqlx::query!(
            "DELETE FROM twitch_engagement_global_sentiment \
             WHERE id NOT IN (\
               SELECT id FROM twitch_engagement_global_sentiment \
               ORDER BY built_at DESC LIMIT $1)",
            KEEP_ROWS
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_latest(&self, max_age_hours: i32) -> Option<String> {
        sqlx::query_scalar!(
            r#"SELECT sentiment_text AS "sentiment_text!" FROM twitch_engagement_global_sentiment
             WHERE built_at > NOW() - make_interval(hours => $1)
             ORDER BY built_at DESC LIMIT 1"#,
            max_age_hours
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    }

    /// Neuestes (frisches) Sentiment als System-Prompt-Fragment, sonst "".
    pub async fn get_sentiment_fragment(&self, max_age_hours: i32) -> String {
        match self.load_latest(max_age_hours).await {
            Some(text) => build_sentiment_fragment(&text),
            None => String::new(),
        }
    }

    /// Pool über alle Channels → MiniMax-Destillation → persistieren
    /// (Hintergrund-Job). Liefert den Text oder None.
    pub async fn rebuild_global_sentiment(
        &self,
        minimax: &EngagementLlmClient,
    ) -> Option<String> {
        let lines = self.load_pooled().await;
        if lines.len() < MIN_MSGS_TO_BUILD {
            tracing::info!(
                msgs = lines.len(),
                "GlobalSentiment: zu wenig Material, skip"
            );
            return None;
        }
        let raw = minimax
            .raw_completion(SYS, &build_user_prompt(&lines), BUILD_MAX_TOKENS, 0.4)
            .await
            .ok()?;
        let stripped = strip_think(&raw);
        let text = stripped.trim();
        if text.is_empty() {
            return None;
        }
        self.store(text, lines.len() as i64, minimax.model())
            .await
            .ok()?;
        tracing::info!(
            msgs = lines.len(),
            chars = text.chars().count(),
            "GlobalSentiment: neu gebaut"
        );
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn fragment_und_prompt() {
        assert_eq!(build_sentiment_fragment(""), "");
        let frag = build_sentiment_fragment("- meta fühlt sich stark an");
        assert!(frag.contains("DEIN eigenes Bauchgefühl"));
        assert!(frag.contains("- meta fühlt sich stark an"));
        let prompt = build_user_prompt(&["haze ist op".to_string()]);
        assert!(prompt.contains("- haze ist op"));
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
        sqlx::query(
            "CREATE TABLE twitch_engagement_conversation (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, content TEXT, \
             ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_global_sentiment (\
             id BIGSERIAL PRIMARY KEY, sentiment_text TEXT NOT NULL, msg_count INT NOT NULL DEFAULT 0, \
             model TEXT, built_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn rebuild_und_fragment_e2e() {
        let Some(pool) = make_pool("t_eng_sentiment").await else {
            return;
        };
        // 8 User-Msgs über mehrere Channels.
        let mut q = String::from(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, content) VALUES ",
        );
        let vals: Vec<String> = (0..8)
            .map(|i| format!("('ch{i}','user','nachricht ueber meta {i}')"))
            .collect();
        q.push_str(&vals.join(","));
        sqlx::query(&q).execute(&pool).await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "<think>x</think>- meta ist grad spicy\n- haze gefeiert"}}]
            })))
            .mount(&server)
            .await;
        let minimax = EngagementLlmClient::new(
            Some("k".to_string()),
            Some(server.uri()),
            Some("MiniMax-M3".to_string()),
            None,
        );

        let gs = GlobalSentiment::new(pool.clone());
        let text = gs.rebuild_global_sentiment(&minimax).await;
        assert_eq!(
            text.as_deref(),
            Some("- meta ist grad spicy\n- haze gefeiert")
        ); // <think> raus
           // Frisches Fragment.
        let frag = gs.get_sentiment_fragment(FRESH_MAX_AGE_HOURS).await;
        assert!(frag.contains("- meta ist grad spicy"));
    }

    #[tokio::test]
    async fn rebuild_zu_wenig_none() {
        let Some(pool) = make_pool("t_eng_sentiment_few").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_conversation (channel_login, role, content) VALUES ('a','user','eine lange nachricht')")
            .execute(&pool).await.unwrap();
        let minimax = EngagementLlmClient::new(
            Some("k".to_string()),
            Some("http://127.0.0.1:1".to_string()),
            Some("m".to_string()),
            None,
        );
        assert_eq!(
            GlobalSentiment::new(pool)
                .rebuild_global_sentiment(&minimax)
                .await,
            None
        );
    }
}

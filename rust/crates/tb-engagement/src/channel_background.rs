//! Per-Streamer-Background für den Engagement-Layer (Port von
//! `bot/engagement/channel_background.py`).
//!
//! Ein Reflexions-Job destilliert pro Channel aus dessen Chats ein kurzes Profil
//! (gespielte Helden, Spielstil, Vibe, Running-Gags) — soziales Kontext-Wissen,
//! KEINE harten Spielfakten. Persistiert in `twitch_engagement_channel_profile`;
//! die Pipeline injiziert nur das Profil des gerade behandelten Channels. Im
//! Prompt als Kontext markiert („nie auswendig aufsagen").

use sqlx::PgPool;

use crate::minimax_chat::{strip_think, EngagementMinimaxClient};

const POOL_LIMIT: i64 = 200;
const MIN_MSGS: usize = 15;
const BUILD_MAX_TOKENS: i64 = 3000;
const PROFILE_MAX_CHARS: usize = 800;

const SYS: &str = "Du bist ein nüchterner Beobachter. Gib nur die verlangte Zusammenfassung, \
kein Vorwort, keine Meta.";

/// Build-Prompt für das Channel-Profil (Python `_build_prompt`).
fn build_profile_prompt(streamer: &str, lines: &[String]) -> String {
    let block = lines.iter().map(|m| format!("- {m}")).collect::<Vec<_>>().join("\n");
    format!(
        "Hier echte Chat-Nachrichten aus dem Twitch-Channel von {streamer} (ein \
         Deadlock-Streamer). Fass in 2-4 knappen Stichpunkten zusammen, was man über DIESEN \
         Streamer und seine Community erkennt: welche Helden er offenbar spielt oder mag, \
         Spielstil falls ablesbar, Running-Gags, wiederkehrende Themen, der allgemeine Vibe. \
         NUR was sich aus den Nachrichten ablesen lässt, NICHTS erfinden, keine harten \
         Spielfakten behaupten. Ist die Datenlage für einen Punkt zu dünn, lass ihn weg. \
         Sachlich.\n\nNachrichten:\n{block}"
    )
}

/// Prompt-Fragment aus dem gespeicherten Profil (Python `get_channel_profile_fragment`).
fn build_profile_fragment(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    format!(
        "Das weißt du über diesen Channel und seinen Streamer (Kontext, damit du dich natürlich \
         einfügst — niemals auswendig aufsagen, nur einfließen lassen wo's passt):\n{text}"
    )
}

/// Channel-Profil-Provider.
pub struct ChannelBackground {
    pool: PgPool,
}

impl ChannelBackground {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn channel_msgs(&self, channel_login: &str, limit: i64) -> Vec<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT content FROM twitch_engagement_conversation \
             WHERE channel_login = $1 AND role = 'user' \
             ORDER BY ts DESC LIMIT $2",
        )
        .bind(channel_login)
        .bind(limit)
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

    async fn channels_with_data(&self, min_msgs: i64) -> Vec<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT channel_login FROM twitch_engagement_conversation \
             WHERE role = 'user' \
             GROUP BY channel_login HAVING count(*) >= $1",
        )
        .bind(min_msgs)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
    }

    async fn upsert(&self, channel_login: &str, text: &str, count: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_engagement_channel_profile \
             (channel_login, profile_text, msg_count, updated_at) VALUES ($1, $2, $3, NOW()) \
             ON CONFLICT (channel_login) DO UPDATE \
               SET profile_text = EXCLUDED.profile_text, \
                   msg_count = EXCLUDED.msg_count, \
                   updated_at = NOW()",
        )
        .bind(channel_login)
        .bind(text)
        .bind(count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load(&self, channel_login: &str) -> Option<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT profile_text FROM twitch_engagement_channel_profile WHERE channel_login = $1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    }

    /// Gespeichertes Channel-Profil als Prompt-Fragment; "" wenn keins da.
    pub async fn get_channel_profile_fragment(&self, channel_login: &str) -> String {
        match self.load(channel_login).await {
            Some(text) => build_profile_fragment(&text),
            None => String::new(),
        }
    }

    /// Baut das Profil eines Channels neu (Hintergrund-Job). Liefert das Profil
    /// oder None (zu wenig Daten / Modellfehler / leer).
    pub async fn rebuild_channel_profile(
        &self,
        channel_login: &str,
        minimax: &EngagementMinimaxClient,
    ) -> Option<String> {
        let lines = self.channel_msgs(channel_login, POOL_LIMIT).await;
        if lines.len() < MIN_MSGS {
            return None;
        }
        let raw = minimax
            .raw_completion(SYS, &build_profile_prompt(channel_login, &lines), BUILD_MAX_TOKENS, 0.4)
            .await
            .ok()?;
        let stripped = strip_think(&raw);
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            return None;
        }
        let text: String = trimmed.chars().take(PROFILE_MAX_CHARS).collect();
        self.upsert(channel_login, &text, lines.len() as i64).await.ok()?;
        tracing::info!(channel = channel_login, msgs = lines.len(), "ChannelBackground: Profil aktualisiert");
        Some(text)
    }

    /// Baut die Profile aller Channels mit genug Daten neu; liefert die Anzahl.
    pub async fn rebuild_all_channel_profiles(&self, minimax: &EngagementMinimaxClient) -> i64 {
        let channels = self.channels_with_data(MIN_MSGS as i64).await;
        let mut n = 0;
        for ch in channels {
            if self.rebuild_channel_profile(&ch, minimax).await.is_some() {
                n += 1;
            }
        }
        n
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
        assert_eq!(build_profile_fragment(""), "");
        let frag = build_profile_fragment("spielt viel Haze, chiller Vibe");
        assert!(frag.contains("niemals auswendig aufsagen"));
        assert!(frag.contains("spielt viel Haze, chiller Vibe"));

        let prompt = build_profile_prompt("nani", &["zeile eins".to_string(), "zeile zwei".to_string()]);
        assert!(prompt.contains("Channel von nani"));
        assert!(prompt.contains("- zeile eins"));
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
            "CREATE TABLE twitch_engagement_conversation (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, content TEXT, \
             ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_channel_profile (\
             channel_login TEXT PRIMARY KEY, profile_text TEXT NOT NULL, \
             msg_count INT NOT NULL DEFAULT 0, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn rebuild_und_fragment_e2e() {
        let Some(pool) = make_pool("t_eng_chbg").await else { return };
        // 15 User-Msgs (jeweils > 3 Zeichen).
        let mut q = String::from("INSERT INTO twitch_engagement_conversation (channel_login, role, content) VALUES ");
        let vals: Vec<String> = (0..15).map(|i| format!("('nani','user','nachricht nummer {i}')")).collect();
        q.push_str(&vals.join(","));
        sqlx::query(&q).execute(&pool).await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "<think>x</think>- spielt Haze\n- chiller vibe"}}]
            })))
            .mount(&server)
            .await;
        let minimax = EngagementMinimaxClient::new(
            Some("k".to_string()),
            Some(server.uri()),
            Some("m".to_string()),
            None,
        );

        let bg = ChannelBackground::new(pool.clone());
        let profile = bg.rebuild_channel_profile("nani", &minimax).await;
        assert_eq!(profile.as_deref(), Some("- spielt Haze\n- chiller vibe")); // <think> raus
        // Persistiert → Fragment.
        let frag = bg.get_channel_profile_fragment("nani").await;
        assert!(frag.contains("- spielt Haze"));
        // channels_with_data findet nani (15 >= 15).
        assert_eq!(bg.channels_with_data(15).await, vec!["nani".to_string()]);
    }

    #[tokio::test]
    async fn rebuild_zu_wenig_msgs_none() {
        let Some(pool) = make_pool("t_eng_chbg_few").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_conversation (channel_login, role, content) VALUES ('nani','user','nur eine lange nachricht')")
            .execute(&pool).await.unwrap();
        let minimax = EngagementMinimaxClient::new(
            Some("k".to_string()),
            Some("http://127.0.0.1:1".to_string()),
            Some("m".to_string()),
            None,
        );
        assert_eq!(ChannelBackground::new(pool).rebuild_channel_profile("nani", &minimax).await, None);
    }
}

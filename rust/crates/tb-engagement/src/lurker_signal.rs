//! Stammgast-Lurker-Erkennung mit Themen-Ankern (Port von
//! `bot/engagement/lurker_signal.py`).
//!
//! Stammgast = ≥N User-Turns in den letzten 30 Tagen. Lurker = Stammgast, der in
//! den letzten 10 Min NICHT gepostet hat. Pro Lurker bis zu 2 offene Threads als
//! Themen-Anker — damit der Bot subtle Themen in deren Richtung legen kann, ohne
//! sie je direkt zu adressieren. 30s in-memory gecacht.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::PgPool;

use crate::threads::{Thread, Threads};

const CACHE_TTL: Duration = Duration::from_secs(30);

/// Ein still mitlesender Stammgast samt seiner offenen Themen-Fäden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LurkerHint {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub top_threads: Vec<Thread>,
}

/// Prompt-Hint aus den Lurkern (reiner Port von `lurker_hint_to_prompt_fragment`).
pub fn lurker_hint_to_prompt_fragment(hints: &[LurkerHint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "Folgende Stammgäste sind möglicherweise gerade still im Chat. \
         Wenn das laufende Thema natürlich an einen ihrer Interessen-Fäden andockt, \
         kannst du subtle Themen-Anker in diese Richtung legen — \
         NIEMALS direkt adressieren (kein 'Hey X' / kein '@X'):"
            .to_string(),
    ];
    for h in hints {
        if h.top_threads.is_empty() {
            lines.push(format!("  - {} (kein konkreter Faden bekannt)", h.twitch_login));
        } else {
            let summaries = h
                .top_threads
                .iter()
                .take(2)
                .map(|t| t.summary.clone())
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("  - {}: {summaries}", h.twitch_login));
        }
    }
    lines.join("\n")
}

/// Lurker-Provider mit 30s-Cache.
pub struct LurkerSignal {
    pool: PgPool,
    cache: Mutex<HashMap<String, (Instant, Vec<LurkerHint>)>>,
}

impl LurkerSignal {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, cache: Mutex::new(HashMap::new()) }
    }

    /// Stammgäste, die aktuell still sind (conversation-buffer-basiert), je mit
    /// bis zu 2 offenen Threads. 30s gecacht.
    pub async fn known_regulars_currently_lurking(
        &self,
        channel_login: &str,
        min_messages: i64,
        days: i32,
        recent_minutes: i32,
        limit: i64,
    ) -> Vec<LurkerHint> {
        {
            let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((at, hints)) = cache.get(channel_login) {
                if at.elapsed() < CACHE_TTL {
                    return hints.clone();
                }
            }
        }

        let rows = sqlx::query!(
            r#"WITH regulars AS (
               SELECT twitch_user_id, MAX(twitch_login) AS twitch_login, COUNT(*) AS msg_count
               FROM twitch_engagement_conversation
               WHERE channel_login = $1 AND role = 'user' AND twitch_user_id IS NOT NULL
                 AND ts > NOW() - make_interval(days => $2)
               GROUP BY twitch_user_id HAVING COUNT(*) >= $3)
             SELECT r.twitch_user_id AS "twitch_user_id!", r.twitch_login FROM regulars r
             WHERE NOT EXISTS (
               SELECT 1 FROM twitch_engagement_conversation c
               WHERE c.channel_login = $1 AND c.role = 'user'
                 AND c.twitch_user_id = r.twitch_user_id
                 AND c.ts > NOW() - make_interval(mins => $4))
             ORDER BY r.msg_count DESC LIMIT $5"#,
            channel_login,
            days,
            min_messages,
            recent_minutes,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let threads = Threads::new(self.pool.clone());
        let mut hints = Vec::new();
        for row in rows {
            let user_id = row.twitch_user_id;
            let top_threads = threads.load_open_threads_for_user(&user_id, channel_login, 2).await;
            hints.push(LurkerHint {
                twitch_user_id: user_id,
                twitch_login: row.twitch_login.unwrap_or_default(),
                top_threads,
            });
        }

        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            cache.insert(channel_login.to_string(), (Instant::now(), hints.clone()));
        }
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn hint(login: &str, summaries: &[&str]) -> LurkerHint {
        LurkerHint {
            twitch_user_id: "u".into(),
            twitch_login: login.into(),
            top_threads: summaries
                .iter()
                .map(|s| Thread {
                    id: 0,
                    twitch_user_id: "u".into(),
                    twitch_login: login.into(),
                    channel_login: None,
                    thread_type: "recurring_interest".into(),
                    summary: (*s).into(),
                    due_at: None,
                    status: "open".into(),
                    last_referenced_at: None,
                })
                .collect(),
        }
    }

    #[test]
    fn fragment_mit_und_ohne_faden() {
        assert_eq!(lurker_hint_to_prompt_fragment(&[]), "");
        let frag = lurker_hint_to_prompt_fragment(&[
            hint("alice", &["mag Haze", "OP bald"]),
            hint("bob", &[]),
        ]);
        assert!(frag.contains("NIEMALS direkt adressieren"));
        assert!(frag.contains("- alice: mag Haze; OP bald"));
        assert!(frag.contains("- bob (kein konkreter Faden bekannt)"));
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
             id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, twitch_user_id TEXT, \
             twitch_login TEXT, content TEXT, ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_user_threads (\
             id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL, twitch_login TEXT NOT NULL, \
             channel_login TEXT, thread_type TEXT NOT NULL, summary TEXT NOT NULL, \
             due_at TIMESTAMPTZ, status TEXT NOT NULL DEFAULT 'open', source_message_id TEXT, \
             last_referenced_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn erkennt_stillen_stammgast() {
        let Some(pool) = make_pool("t_eng_lurker").await else { return };
        // Stammgast 'alice': 3 Msgs, alle > 10 Min alt (still).
        // 'bob': hat gerade gepostet (kein Lurker).
        // 'carol': nur 1 Msg (kein Stammgast).
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, twitch_user_id, twitch_login, content, ts) VALUES \
             ('nani','user','a','alice','m1', NOW() - INTERVAL '20 minutes'), \
             ('nani','user','a','alice','m2', NOW() - INTERVAL '25 minutes'), \
             ('nani','user','a','alice','m3', NOW() - INTERVAL '30 minutes'), \
             ('nani','user','b','bob','m1', NOW() - INTERVAL '20 minutes'), \
             ('nani','user','b','bob','m2', NOW() - INTERVAL '25 minutes'), \
             ('nani','user','b','bob','m3', NOW() - INTERVAL '1 minute'), \
             ('nani','user','c','carol','m1', NOW() - INTERVAL '20 minutes')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // alice hat einen offenen Thread.
        sqlx::query(
            "INSERT INTO twitch_user_threads (twitch_user_id, twitch_login, channel_login, thread_type, summary, status) \
             VALUES ('a','alice','nani','recurring_interest','mag Haze','open')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let ls = LurkerSignal::new(pool);
        let hints = ls.known_regulars_currently_lurking("nani", 3, 30, 10, 5).await;
        // Nur alice: Stammgast (3 >= 3) + still (kein Post < 10min). bob postete grad, carol zu wenig.
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].twitch_login, "alice");
        assert_eq!(hints[0].top_threads.len(), 1);
        assert_eq!(hints[0].top_threads[0].summary, "mag Haze");
    }
}

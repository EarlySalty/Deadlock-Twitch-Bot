//! Konversations-Fäden mit Lifecycle (Port von `bot/engagement/threads.py`).
//!
//! Lifecycle: open → follow_up_due (Cron flippt due_at) → awaiting_response
//! (Bot fragt) → closed (Auto-Close). Persistiert in `twitch_user_threads`. Die
//! Pipeline lädt offene Threads pro Sender und gibt sie als „niemals
//! auspacken"-Hint weiter.
//!
//! Slice 15a (hier): Lese-/Lifecycle-Teil. Der MiniMax-Thread-Extractor
//! (`extract_threads`) folgt in 15b.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Ein Konversations-Faden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: i64,
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub channel_login: Option<String>,
    pub thread_type: String,
    pub summary: String,
    pub due_at: Option<DateTime<Utc>>,
    pub status: String,
    pub last_referenced_at: Option<DateTime<Utc>>,
}

/// Ergebnis von [`Threads::auto_close_stale`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CloseCounts {
    pub open_to_due: u64,
    pub awaiting_to_closed: u64,
    pub open_to_closed: u64,
}

/// Baut den Prompt-Hint aus den offenen Threads eines Users (reiner Port von
/// `threads_to_prompt_fragment`).
pub fn threads_to_prompt_fragment(user_login: &str, threads: &[Thread]) -> String {
    if threads.is_empty() {
        return String::new();
    }
    let mut lines = vec![format!(
        "Was du über {user_login} (aus früheren Gesprächen) weisst — \
         nur einsetzen wenn das Gespräch NATÜRLICH darauf führt, NIEMALS auspacken:"
    )];
    for t in threads {
        let marker = if t.status == "follow_up_due" {
            "↪ Follow-up wäre passend (wenn die Gelegenheit kommt)"
        } else {
            "•"
        };
        lines.push(format!("  {marker} ({}) {}", t.thread_type, t.summary));
    }
    lines.join("\n")
}

/// Thread-Provider.
pub struct Threads {
    pool: PgPool,
}

impl Threads {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Offene Threads eines Users im Channel (oder channel-übergreifend), die
    /// nicht in den letzten 30 Min referenziert wurden. follow_up_due zuerst.
    pub async fn load_open_threads_for_user(
        &self,
        user_id: &str,
        channel_login: &str,
        limit: i64,
    ) -> Vec<Thread> {
        if user_id.is_empty() {
            return Vec::new();
        }
        type Row = (
            i64,
            String,
            String,
            Option<String>,
            String,
            String,
            Option<DateTime<Utc>>,
            String,
            Option<DateTime<Utc>>,
        );
        let rows = sqlx::query_as::<_, Row>(
            "SELECT id, twitch_user_id, twitch_login, channel_login, thread_type, summary, \
                    due_at, status, last_referenced_at \
             FROM twitch_user_threads \
             WHERE twitch_user_id = $1 AND (channel_login = $2 OR channel_login IS NULL) \
               AND status IN ('open', 'follow_up_due') \
               AND (last_referenced_at IS NULL \
                    OR last_referenced_at < NOW() - INTERVAL '30 minutes') \
             ORDER BY CASE WHEN status = 'follow_up_due' THEN 0 ELSE 1 END, \
                      COALESCE(due_at, created_at) ASC \
             LIMIT $3",
        )
        .bind(user_id)
        .bind(channel_login)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(
                |(id, twitch_user_id, twitch_login, channel_login, thread_type, summary, due_at, status, last_referenced_at)| {
                    Thread {
                        id,
                        twitch_user_id,
                        twitch_login,
                        channel_login,
                        thread_type,
                        summary,
                        due_at,
                        status,
                        last_referenced_at,
                    }
                },
            )
            .collect()
    }

    /// Markiert Threads als referenziert (`last_referenced_at = NOW()`); ein
    /// `follow_up_due`-Thread wird dabei zu `awaiting_response`.
    pub async fn mark_referenced(&self, thread_ids: &[i64]) -> Result<(), sqlx::Error> {
        if thread_ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE twitch_user_threads \
                SET last_referenced_at = NOW(), \
                    status = CASE WHEN status = 'follow_up_due' THEN 'awaiting_response' \
                                  ELSE status END, \
                    updated_at = NOW() \
              WHERE id = ANY($1)",
        )
        .bind(thread_ids)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lifecycle-Cron: open+fällig → follow_up_due, awaiting_response >7d →
    /// closed, open >30d → closed. Liefert die Zähler.
    pub async fn auto_close_stale(&self) -> CloseCounts {
        let run = |sql: &'static str| async move {
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .map(|r| r.rows_affected())
                .unwrap_or(0)
        };
        let open_to_due = run(
            "UPDATE twitch_user_threads SET status='follow_up_due', updated_at=NOW() \
             WHERE status='open' AND due_at IS NOT NULL AND due_at <= NOW()",
        )
        .await;
        let awaiting_to_closed = run(
            "UPDATE twitch_user_threads SET status='closed', updated_at=NOW() \
             WHERE status='awaiting_response' AND updated_at < NOW() - INTERVAL '7 days'",
        )
        .await;
        let open_to_closed = run(
            "UPDATE twitch_user_threads SET status='closed', updated_at=NOW() \
             WHERE status='open' AND updated_at < NOW() - INTERVAL '30 days'",
        )
        .await;
        CloseCounts { open_to_due, awaiting_to_closed, open_to_closed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn thread(status: &str, ttype: &str, summary: &str) -> Thread {
        Thread {
            id: 0,
            twitch_user_id: "u".into(),
            twitch_login: "user".into(),
            channel_login: Some("nani".into()),
            thread_type: ttype.into(),
            summary: summary.into(),
            due_at: None,
            status: status.into(),
            last_referenced_at: None,
        }
    }

    #[test]
    fn fragment_marker() {
        assert_eq!(threads_to_prompt_fragment("user", &[]), "");
        let frag = threads_to_prompt_fragment(
            "user",
            &[
                thread("follow_up_due", "upcoming_event", "OP morgen"),
                thread("open", "recurring_interest", "mag Haze"),
            ],
        );
        assert!(frag.contains("NIEMALS auspacken"));
        assert!(frag.contains("↪ Follow-up wäre passend"));
        assert!(frag.contains("(upcoming_event) OP morgen"));
        assert!(frag.contains("• (recurring_interest) mag Haze"));
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
    async fn load_filtert_und_ordnet() {
        let Some(pool) = make_pool("t_eng_threads").await else { return };
        sqlx::query(
            "INSERT INTO twitch_user_threads (twitch_user_id, twitch_login, channel_login, thread_type, summary, status, last_referenced_at) VALUES \
             ('u','user','nani','recurring_interest','offen', 'open', NULL), \
             ('u','user','nani','upcoming_event','fällig', 'follow_up_due', NULL), \
             ('u','user','nani','life_status','geschlossen', 'closed', NULL), \
             ('u','user','nani','recent_experience','grad referenziert', 'open', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        let t = Threads::new(pool.clone());
        let open = t.load_open_threads_for_user("u", "nani", 5).await;
        // closed + grad-referenziert raus → 2 übrig, follow_up_due zuerst.
        assert_eq!(open.len(), 2);
        assert_eq!(open[0].status, "follow_up_due");
        assert_eq!(open[1].status, "open");
        // leerer user_id → leer
        assert!(t.load_open_threads_for_user("", "nani", 5).await.is_empty());
    }

    #[tokio::test]
    async fn mark_referenced_flippt_due() {
        let Some(pool) = make_pool("t_eng_threads_mark").await else { return };
        sqlx::query(
            "INSERT INTO twitch_user_threads (id, twitch_user_id, twitch_login, thread_type, summary, status) \
             VALUES (1,'u','user','upcoming_event','x','follow_up_due'), (2,'u','user','life_status','y','open')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let t = Threads::new(pool.clone());
        t.mark_referenced(&[1, 2]).await.unwrap();
        let status: Vec<(i64, String)> = sqlx::query_as("SELECT id, status FROM twitch_user_threads ORDER BY id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(status[0], (1, "awaiting_response".to_string())); // follow_up_due → awaiting
        assert_eq!(status[1], (2, "open".to_string())); // open bleibt open
    }

    #[tokio::test]
    async fn auto_close_lifecycle() {
        let Some(pool) = make_pool("t_eng_threads_close").await else { return };
        sqlx::query(
            "INSERT INTO twitch_user_threads (twitch_user_id, twitch_login, thread_type, summary, status, due_at, updated_at) VALUES \
             ('u','user','upcoming_event','fällig','open', NOW() - INTERVAL '1 hour', NOW()), \
             ('u','user','life_status','altes awaiting','awaiting_response', NULL, NOW() - INTERVAL '8 days'), \
             ('u','user','recurring_interest','altes open','open', NULL, NOW() - INTERVAL '31 days')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let counts = Threads::new(pool).auto_close_stale().await;
        assert_eq!(counts.open_to_due, 1);
        assert_eq!(counts.awaiting_to_closed, 1);
        assert_eq!(counts.open_to_closed, 1);
    }
}

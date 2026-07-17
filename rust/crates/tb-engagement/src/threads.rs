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
use serde_json::Value;
use sqlx::PgPool;

use crate::minimax_chat::{ChatMessage, EngagementMinimaxClient};

const EXTRACTOR_SYSTEM_PROMPT: &str =
    "Du bist ein Konversations-Analyst für einen Twitch-Chat. Lies die folgenden \
Chat-Nachrichten und identifiziere Konversations-Fäden, die für einen späteren \
Follow-up wertvoll sein könnten — Dinge mit echtem zwischenmenschlichem Wert: \
anstehende Ereignisse (OP, Reise, Prüfung), kürzliche Erlebnisse die \
nachgefragt werden könnten, oder klare Dauerinteressen (Lieblings-Hero, Hobby).\n\
\n\
Antworte AUSSCHLIESSLICH als JSON-Array (kein Markdown, kein Vortext). Jeder \
Eintrag hat die Felder: twitch_user_id, twitch_login, thread_type \
(\"upcoming_event\"|\"recent_experience\"|\"recurring_interest\"|\"life_status\"), \
summary (knapp, max 80 Zeichen), due_at_iso (YYYY-MM-DD, optional, nur wenn ein \
konkretes Datum genannt wurde).\n\
\n\
Wenn nichts mit echtem Wert identifizierbar ist, antworte mit []. Erfinde nichts.";

const THREAD_TYPES: &[&str] = &[
    "upcoming_event",
    "recent_experience",
    "recurring_interest",
    "life_status",
];

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
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", twitch_user_id AS "twitch_user_id!",
                    twitch_login AS "twitch_login!", channel_login,
                    thread_type AS "thread_type!", summary AS "summary!",
                    due_at, status AS "status!", last_referenced_at
             FROM twitch_user_threads
             WHERE twitch_user_id = $1 AND (channel_login = $2 OR channel_login IS NULL)
               AND status IN ('open', 'follow_up_due')
               AND (last_referenced_at IS NULL
                    OR last_referenced_at < NOW() - INTERVAL '30 minutes')
             ORDER BY CASE WHEN status = 'follow_up_due' THEN 0 ELSE 1 END,
                      COALESCE(due_at, created_at) ASC
             LIMIT $3"#,
            user_id,
            channel_login,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(|r| Thread {
                id: r.id,
                twitch_user_id: r.twitch_user_id,
                twitch_login: r.twitch_login,
                channel_login: r.channel_login,
                thread_type: r.thread_type,
                summary: r.summary,
                due_at: r.due_at,
                status: r.status,
                last_referenced_at: r.last_referenced_at,
            })
            .collect()
    }

    /// Markiert Threads als referenziert (`last_referenced_at = NOW()`); ein
    /// `follow_up_due`-Thread wird dabei zu `awaiting_response`.
    pub async fn mark_referenced(&self, thread_ids: &[i64]) -> Result<(), sqlx::Error> {
        if thread_ids.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            "UPDATE twitch_user_threads \
                SET last_referenced_at = NOW(), \
                    status = CASE WHEN status = 'follow_up_due' THEN 'awaiting_response' \
                                  ELSE status END, \
                    updated_at = NOW() \
              WHERE id = ANY($1)",
            thread_ids
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lifecycle-Cron: open+fällig → follow_up_due, awaiting_response >7d →
    /// closed, open >30d → closed. Liefert die Zähler.
    pub async fn auto_close_stale(&self) -> CloseCounts {
        let run = |sql: &'static str| async move {
            // dyn: gemeinsamer Helper für drei statische Lifecycle-Statements.
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
        CloseCounts {
            open_to_due,
            awaiting_to_closed,
            open_to_closed,
        }
    }

    async fn load_recent_user_turns(
        &self,
        channel_login: &str,
        hours: i32,
        limit: i64,
    ) -> Vec<(String, Option<String>, String, DateTime<Utc>)> {
        sqlx::query!(
            r#"SELECT twitch_user_id AS "twitch_user_id!", twitch_login,
                    content AS "content!", ts AS "ts!"
             FROM twitch_engagement_conversation
             WHERE channel_login = $1 AND role = 'user' AND twitch_user_id IS NOT NULL
               AND ts > NOW() - make_interval(hours => $2)
             ORDER BY ts DESC LIMIT $3"#,
            channel_login,
            hours,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.twitch_user_id, r.twitch_login, r.content, r.ts))
        .collect()
    }

    /// Insert nur, wenn kein offener Thread mit gleichem (user, channel, type,
    /// LOWER(summary)) existiert. `true` = neu eingefügt.
    async fn upsert_thread(
        &self,
        uid: &str,
        login: &str,
        channel: &str,
        ttype: &str,
        summary: &str,
        due_at: Option<DateTime<Utc>>,
    ) -> Result<bool, sqlx::Error> {
        let existing = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM twitch_user_threads
             WHERE twitch_user_id = $1 AND COALESCE(channel_login, '') = $2
               AND thread_type = $3 AND LOWER(summary) = LOWER($4)
               AND status IN ('open', 'follow_up_due', 'awaiting_response') LIMIT 1"#,
            uid,
            channel,
            ttype,
            summary
        )
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_some() {
            return Ok(false);
        }
        sqlx::query!(
            "INSERT INTO twitch_user_threads \
             (twitch_user_id, twitch_login, channel_login, thread_type, summary, due_at, \
              status, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, 'open', NOW(), NOW())",
            uid,
            login,
            channel,
            ttype,
            summary,
            due_at
        )
        .execute(&self.pool)
        .await?;
        Ok(true)
    }

    /// Thread-Extractor (Hintergrund-Job): jüngste User-Turns → MiniMax-JSON →
    /// upsert. Liefert die Anzahl neu eingefügter Threads.
    pub async fn extract_threads(
        &self,
        channel_login: &str,
        minimax: &EngagementMinimaxClient,
        hours: i32,
        limit: i64,
    ) -> i64 {
        let rows = self
            .load_recent_user_turns(channel_login, hours, limit)
            .await;
        if rows.is_empty() {
            return 0;
        }
        let lines: Vec<String> = rows
            .iter()
            .rev()
            .map(|(uid, login, content, ts)| {
                format!(
                    "[{}] ({uid}|{}): {content}",
                    ts.format("%Y-%m-%dT%H:%M:%S"),
                    login.as_deref().unwrap_or_default()
                )
            })
            .collect();
        let user_prompt = format!(
            "Channel: {channel_login}\nZeitfenster: letzte {hours} Stunden\n\n{}\n",
            lines.join("\n")
        );

        let response = match minimax
            .generate(
                EXTRACTOR_SYSTEM_PROMPT,
                &[ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt,
                    name: None,
                }],
                800,
                480,
            )
            .await
        {
            Ok(r) => r,
            Err(error) => {
                tracing::warn!(
                    %error,
                    channel = %channel_login,
                    "Thread-Extractor: MiniMax-Aufruf fehlgeschlagen"
                );
                return 0;
            }
        };
        let Some(text) = response.text else {
            tracing::warn!(channel = %channel_login, "Thread-Extractor: MiniMax ohne Text");
            return 0;
        };
        let cleaned = strip_codeblock(&text);
        let items = match serde_json::from_str::<Value>(&cleaned) {
            Ok(items) => items,
            Err(error) => {
                tracing::warn!(
                    %error,
                    channel = %channel_login,
                    "Thread-Extractor: JSON nicht lesbar"
                );
                return 0;
            }
        };
        let Some(arr) = items.as_array() else {
            tracing::warn!(channel = %channel_login, "Thread-Extractor: JSON ist kein Array");
            return 0;
        };

        let mut inserted = 0;
        for item in arr {
            let uid = item
                .get("twitch_user_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let login = item
                .get("twitch_login")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let ttype = item
                .get("thread_type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let summary = item
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if uid.is_empty() || login.is_empty() || ttype.is_empty() || summary.is_empty() {
                continue;
            }
            if !THREAD_TYPES.contains(&ttype) {
                continue;
            }
            let due_at = item
                .get("due_at_iso")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .and_then(parse_iso_date);
            let summary_trunc: String = summary.chars().take(80).collect();
            match self
                .upsert_thread(uid, login, channel_login, ttype, &summary_trunc, due_at)
                .await
            {
                Ok(true) => inserted += 1,
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    %error,
                    channel = %channel_login,
                    twitch_user_id = %uid,
                    twitch_login = %login,
                    "Thread-Extractor: Upsert fehlgeschlagen"
                ),
            }
        }
        inserted
    }
}

/// Entfernt ```/```json-Codeblock-Hüllen (Python `_strip_codeblock`).
fn strip_codeblock(text: &str) -> String {
    let cleaned = text.trim();
    if !cleaned.starts_with("```") {
        return cleaned.to_string();
    }
    let cleaned = cleaned.trim_start_matches('`');
    let cleaned = if cleaned.to_lowercase().starts_with("json") {
        &cleaned[4..]
    } else {
        cleaned
    };
    let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned);
    cleaned.trim().to_string()
}

/// Parst `due_at_iso` (YYYY-MM-DD oder ISO-Datetime) zu UTC; sonst None.
///
/// **Timezone-Angleichung (bewusste Divergenz zu Python):** Pythons
/// `datetime.fromisoformat("YYYY-MM-DD")` liefert ein *naives* Datetime ohne
/// `tzinfo`. Beim Binden gegen die `TIMESTAMPTZ`-Spalte `due_at` interpretiert
/// psycopg dieses naive Mitternacht in der **Session-Zeitzone der DB-Verbindung**
/// (typischerweise Europe/Berlin) — `"2024-01-15"` landet also als
/// `2024-01-14T23:00:00Z` (Winter), eine Stunde zu früh im `due_at <= NOW()`-
/// Cron-Vergleich. Das ist ein latenter, locale-abhängiger Migrationsbug. Hier
/// wird stattdessen jede zeitzonenlose Eingabe **explizit als UTC** verankert
/// (date-only → UTC-Mitternacht, naive Datetime → UTC) — deterministisch,
/// unabhängig von der DB-Session-TZ.
fn parse_iso_date(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.and_utc());
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
        sqlx::query(
            "CREATE TABLE twitch_engagement_conversation (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, twitch_user_id TEXT, \
             twitch_login TEXT, content TEXT, ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[test]
    fn strip_codeblock_und_parse_date() {
        assert_eq!(strip_codeblock("```json\n[1,2]\n```"), "[1,2]");
        assert_eq!(strip_codeblock("```[1]```"), "[1]");
        assert_eq!(strip_codeblock("[3]"), "[3]"); // ohne Fence unverändert
        assert!(parse_iso_date("2024-01-15").is_some());
        assert!(parse_iso_date("kein datum").is_none());
    }

    /// Lockt die Timezone-Angleichung: zeitzonenlose Eingaben werden EXPLIZIT als
    /// UTC verankert (nicht als Session-TZ-naive wie Python). Verhindert eine
    /// Regression auf das locale-abhängige `due_at`-Verhalten.
    #[test]
    fn parse_iso_date_verankert_naive_eingaben_als_utc() {
        use chrono::TimeZone;
        // Date-only → UTC-Mitternacht (nicht Berlin-Mitternacht = 23:00Z).
        assert_eq!(
            parse_iso_date("2024-01-15"),
            Some(Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).unwrap())
        );
        // Naive Datetime (kein Offset) → als UTC interpretiert.
        assert_eq!(
            parse_iso_date("2024-01-15T08:30:00"),
            Some(Utc.with_ymd_and_hms(2024, 1, 15, 8, 30, 0).unwrap())
        );
        // Expliziter Offset bleibt korrekt nach UTC umgerechnet.
        assert_eq!(
            parse_iso_date("2024-01-15T08:30:00+02:00"),
            Some(Utc.with_ymd_and_hms(2024, 1, 15, 6, 30, 0).unwrap())
        );
    }

    #[tokio::test]
    async fn extract_threads_und_dedup() {
        let Some(pool) = make_pool("t_eng_threads_extract").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, twitch_user_id, twitch_login, content) \
             VALUES ('nani','user','u1','user','ich hab morgen OP')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let server = MockServer::start().await;
        // Modell liefert ein JSON-Array.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content":
                    "[{\"twitch_user_id\":\"u1\",\"twitch_login\":\"user\",\"thread_type\":\"upcoming_event\",\"summary\":\"OP morgen\"}]"
                }}]
            })))
            .mount(&server)
            .await;
        let minimax = EngagementMinimaxClient::new(
            Some("k".to_string()),
            Some(server.uri()),
            Some("m".to_string()),
            None,
        );

        let t = Threads::new(pool.clone());
        let n = t.extract_threads("nani", &minimax, 6, 80).await;
        assert_eq!(n, 1);
        // Zweiter Lauf: gleicher Thread existiert offen → Dedup, 0 neu.
        let n2 = t.extract_threads("nani", &minimax, 6, 80).await;
        assert_eq!(n2, 0);
        // Der Thread ist offen und ladbar.
        let open = t.load_open_threads_for_user("u1", "nani", 5).await;
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].summary, "OP morgen");
    }

    #[tokio::test]
    async fn load_filtert_und_ordnet() {
        let Some(pool) = make_pool("t_eng_threads").await else {
            return;
        };
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
        let Some(pool) = make_pool("t_eng_threads_mark").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_user_threads (id, twitch_user_id, twitch_login, thread_type, summary, status) \
             VALUES (1,'u','user','upcoming_event','x','follow_up_due'), (2,'u','user','life_status','y','open')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let t = Threads::new(pool.clone());
        t.mark_referenced(&[1, 2]).await.unwrap();
        let status: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, status FROM twitch_user_threads ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(status[0], (1, "awaiting_response".to_string())); // follow_up_due → awaiting
        assert_eq!(status[1], (2, "open".to_string())); // open bleibt open
    }

    #[tokio::test]
    async fn auto_close_lifecycle() {
        let Some(pool) = make_pool("t_eng_threads_close").await else {
            return;
        };
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

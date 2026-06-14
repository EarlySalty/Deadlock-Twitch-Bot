//! Hintergrundjobs des Title-Generators (Port von
//! `bot/title_generator/knowledge_job.py` + `insight_job.py`).
//!
//! - `knowledge_job`: baut nächtlich die Cross-Streamer-Knowledge-Base aus
//!   überdurchschnittlich performenden Titeln der letzten 7 Tage.
//! - `insight_job` (Slice 8c): wöchentliche KI-Insights pro Partner.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use regex::Regex;
use sqlx::PgPool;

use crate::title_db;

/// Score-Schwelle, ab der ein Titel in die Knowledge-Base aufgenommen wird
/// (Python `SCORE_THRESHOLD`).
const SCORE_THRESHOLD: f64 = 1.2;

/// Stopwords für die Keyword-Extraktion (Python `_extract_keywords`).
const STOPWORDS: [&str; 9] = [
    "game", "stream", "live", "heute", "jetzt", "playing", "with", "ranked", "grind",
];

/// Eine geladene Session-Zeile fürs Scoring (Python `_fetch_recent_sessions`).
struct RecentSession {
    streamer_login: String,
    title: String,
    avg_viewers: f64,
    followers_start: i64,
}

/// Größenklasse des Streamers (Python `_classify_size`).
fn classify_size(avg_viewers: f64) -> &'static str {
    if avg_viewers < 100.0 {
        "small"
    } else if avg_viewers < 500.0 {
        "medium"
    } else {
        "large"
    }
}

/// Keywords aus einem Titel (Python `_extract_keywords`): Wörter ≥4 Buchstaben
/// (inkl. Umlaute), lowercased, ohne Stopwords, max. 8.
fn extract_keywords(title: &str) -> Vec<String> {
    let re = Regex::new(r"[a-zA-ZäöüÄÖÜß]{4,}").unwrap();
    let lower = title.to_lowercase();
    re.find_iter(&lower)
        .map(|m| m.as_str().to_string())
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .take(8)
        .collect()
}

/// Lädt Sessions der letzten `days` Tage mit Titel + Viewer/Follower-Stats
/// (Python `_fetch_recent_sessions`).
async fn fetch_recent_sessions(pool: &PgPool, days: i64) -> Vec<RecentSession> {
    let cutoff = Utc::now() - chrono::Duration::days(days);
    sqlx::query_as::<_, (String, String, f64, i64)>(
        "SELECT streamer_login, stream_title, avg_viewers::float8, followers_start::int8 \
         FROM twitch_stream_sessions \
         WHERE started_at >= $1 \
           AND streamer_login IS NOT NULL AND streamer_login != '' \
           AND stream_title IS NOT NULL AND stream_title != '' \
           AND avg_viewers IS NOT NULL \
           AND followers_start IS NOT NULL AND followers_start > 0",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(streamer_login, title, avg_viewers, followers_start)| RecentSession {
        streamer_login: streamer_login.trim().to_lowercase(),
        title,
        avg_viewers,
        followers_start,
    })
    .collect()
}

/// Löst die `twitch_user_id` zu einem Login auf (Python
/// `_resolve_streamer_id_for_login`): kein Treffer → `None`.
async fn resolve_streamer_id(pool: &PgPool, login: &str) -> Option<String> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = $1 LIMIT 1",
    )
    .bind(login.trim().to_lowercase())
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Nächtlicher Knowledge-Job (Python `run_knowledge_job`): scort die Titel der
/// letzten 7 Tage relativ zur eigenen Performance + Engagement, gewichtet mit der
/// History-Tiefe, und upserted überdurchschnittliche Titel in die Knowledge-Base.
pub async fn run_knowledge_job(pool: &PgPool) {
    let sessions = fetch_recent_sessions(pool, 7).await;
    tracing::info!(sessions = sessions.len(), "title_generator: knowledge job start");

    let mut by_streamer: HashMap<String, Vec<RecentSession>> = HashMap::new();
    for s in sessions {
        by_streamer.entry(s.streamer_login.clone()).or_default().push(s);
    }

    let mut inserted = 0u32;
    for (login, streamer_sessions) in by_streamer {
        let Some(streamer_id) = resolve_streamer_id(pool, &login).await else {
            continue;
        };

        let own_avg = title_db::get_streamer_avg_viewers(pool, &streamer_id).await;
        let session_count = title_db::get_streamer_session_count(pool, &streamer_id).await;
        let history_weight = (session_count as f64 / 20.0).min(1.0);

        for sess in &streamer_sessions {
            if own_avg <= 0.0 {
                continue;
            }
            let relative_perf = sess.avg_viewers / own_avg;
            let engagement_rate = sess.avg_viewers / sess.followers_start as f64;
            let normalized_score =
                (0.5 * relative_perf + 0.5 * engagement_rate * 100.0) * history_weight;
            if normalized_score < SCORE_THRESHOLD {
                continue;
            }
            let title = sess.title.trim();
            let len = title.chars().count();
            if len < 10 || len > 140 {
                continue;
            }
            let keywords = extract_keywords(title);
            let streamer_size = classify_size(own_avg);
            let source: String = streamer_id.chars().take(8).collect::<String>() + "...";
            if title_db::upsert_knowledge_entry(
                pool,
                title,
                &keywords,
                relative_perf,
                engagement_rate,
                history_weight,
                normalized_score,
                streamer_size,
                &source,
            )
            .await
            .is_ok()
            {
                inserted += 1;
            }
        }
    }
    tracing::info!(inserted, "title_generator: knowledge job done");
}

/// Periodischer Job: nächtlich `run_knowledge_job` (Python
/// `schedule_nightly_knowledge_job`). Als tokio-Task starten (läuft endlos).
pub async fn schedule_nightly_knowledge_job(pool: PgPool, start_delay_s: u64) {
    if start_delay_s > 0 {
        tokio::time::sleep(Duration::from_secs(start_delay_s)).await;
    }
    loop {
        run_knowledge_job(&pool).await;
        tokio::time::sleep(Duration::from_secs(86400)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn classify_size_schwellen() {
        assert_eq!(classify_size(50.0), "small");
        assert_eq!(classify_size(99.9), "small");
        assert_eq!(classify_size(100.0), "medium");
        assert_eq!(classify_size(499.0), "medium");
        assert_eq!(classify_size(500.0), "large");
    }

    #[test]
    fn extract_keywords_filter_und_limit() {
        // "mit" (3 Buchstaben) raus; ranked/grind/heute Stopwords raus.
        let kw = extract_keywords("Deadlock Ranked Grind mit heute Spielen");
        assert_eq!(kw, vec!["deadlock".to_string(), "spielen".to_string()]);
        // Max 8 Keywords.
        let many = extract_keywords("alpha beta gamma delta epsilon zeta theta kappa lambda omega");
        assert_eq!(many.len(), 8);
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(4).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_streamers (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_stream_sessions (streamer_login TEXT, stream_title TEXT, \
             avg_viewers DOUBLE PRECISION, followers_start INTEGER, started_at TIMESTAMPTZ)",
            "CREATE TABLE title_generator_knowledge (\
             id SERIAL PRIMARY KEY, title TEXT NOT NULL, keywords TEXT[] DEFAULT '{}', \
             game_context TEXT NOT NULL DEFAULT 'deadlock', relative_perf FLOAT NOT NULL, \
             engagement_rate FLOAT NOT NULL, history_weight FLOAT NOT NULL DEFAULT 1.0, \
             normalized_score FLOAT NOT NULL, \
             streamer_size TEXT CHECK (streamer_size IN ('small','medium','large')), \
             source_streamer TEXT, \
             quality_tier SMALLINT NOT NULL DEFAULT 1 CHECK (quality_tier IN (1,2,3)), \
             UNIQUE (title, game_context))",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn knowledge_job_scort_und_upserted() {
        let Some(pool) = make_pool("t6e_title_knowledge_job").await else { return };
        sqlx::query("INSERT INTO twitch_streamers (twitch_user_id, twitch_login) VALUES ('900','foo')")
            .execute(&pool).await.unwrap();
        // 3 Sessions in den letzten 7 Tagen (relativ zu now() = 2026-06-14).
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, stream_title, avg_viewers, followers_start, started_at) VALUES \
             ('foo','Deadlock Ranked Grind Session',50.0,100,NOW() - INTERVAL '2 days'), \
             ('foo','Chill Deadlock Gameplay heute',40.0,100,NOW() - INTERVAL '3 days'), \
             ('foo','x',30.0,100,NOW() - INTERVAL '4 days')",
        )
        .execute(&pool).await.unwrap();

        run_knowledge_job(&pool).await;

        // own_avg=AVG(50,40,30)=40, count=3→hist=0.15.
        // s1: rel=1.25, eng=0.5 → (0.625+25)*0.15=3.84 ≥1.2 ✓; s2: (0.5+20)*0.15=3.075 ✓;
        // s3 'x' (len 1) gefiltert.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*)::int8 FROM title_generator_knowledge")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 2);

        let (score, size, source, kw): (f64, String, String, Vec<String>) = sqlx::query_as(
            "SELECT normalized_score::float8, streamer_size, source_streamer, keywords \
             FROM title_generator_knowledge WHERE title='Deadlock Ranked Grind Session'",
        )
        .fetch_one(&pool).await.unwrap();
        assert!(score > 3.0); // ≈3.84
        assert_eq!(size, "small"); // own_avg 40 < 100
        assert_eq!(source, "900..."); // streamer_id[:8] + "..."
        assert_eq!(kw, vec!["deadlock".to_string(), "session".to_string()]); // ranked/grind raus

        // Idempotent: erneut → GREATEST, weiterhin 2 Zeilen.
        run_knowledge_job(&pool).await;
        let count2: i64 = sqlx::query_scalar("SELECT COUNT(*)::int8 FROM title_generator_knowledge")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count2, 2);
    }
}

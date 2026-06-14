//! Datenzugriff für den `!title`-Generator (B11). Port der Lese-Funktionen aus
//! `bot/title_generator/title_db.py`, die der `!title`-Chat-Command braucht.
//!
//! Die Schreib-Seite (upsert_knowledge_entry / insert_insight) gehört zu den
//! Knowledge-/Insight-Jobs und wird mit deren Port ergänzt — hier bewusst nur
//! die vom Command genutzten Reads, um keinen toten Code anzulegen.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Eine Stream-Session aus der Titel-Historie
/// (Python `get_streamer_title_history`).
#[derive(Debug, Clone)]
pub struct TitleHistoryItem {
    pub title: String,
    pub avg_viewers: Option<f64>,
    pub peak_viewers: Option<i64>,
    pub followers_start: Option<i64>,
    pub started_at: Option<DateTime<Utc>>,
}

/// Ein kuratierter Knowledge-Titel (Python `get_top_knowledge_titles`).
#[derive(Debug, Clone)]
pub struct KnowledgeTitle {
    pub title: String,
    pub normalized_score: Option<f64>,
    pub keywords: Vec<String>,
    pub quality_tier: Option<i32>,
}

/// Löst den `twitch_login` zu einer `twitch_user_id` auf (Python
/// `_resolve_streamer_login_for_user_id`): leerer/fehlender Treffer → `None`.
async fn resolve_streamer_login(pool: &PgPool, streamer_id: &str) -> Option<String> {
    let login: Option<String> = sqlx::query_scalar(
        "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = $1 LIMIT 1",
    )
    .bind(streamer_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    login
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
}

/// Letzte Stream-Sessions mit Titel + Viewer-Statistik des Streamers
/// (Python `get_streamer_title_history`). Kein aufgelöster Login oder
/// Query-Fehler → leere Liste (defensiv; der Command degradiert ohnehin).
pub async fn get_streamer_title_history(
    pool: &PgPool,
    streamer_id: &str,
    limit: i64,
) -> Vec<TitleHistoryItem> {
    let Some(login) = resolve_streamer_login(pool, streamer_id).await else {
        return Vec::new();
    };
    let rows = sqlx::query_as::<_, (String, Option<f64>, Option<i64>, Option<i64>, Option<DateTime<Utc>>)>(
        "SELECT \
             s.stream_title, \
             s.avg_viewers::float8, \
             s.peak_viewers::int8, \
             s.followers_start::int8, \
             s.started_at \
         FROM twitch_stream_sessions s \
         WHERE LOWER(s.streamer_login) = $1 \
           AND s.stream_title IS NOT NULL \
           AND s.stream_title != '' \
         ORDER BY s.started_at DESC \
         LIMIT $2",
    )
    .bind(&login)
    .bind(limit)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|(title, avg_viewers, peak_viewers, followers_start, started_at)| TitleHistoryItem {
                title,
                avg_viewers,
                peak_viewers,
                followers_start,
                started_at,
            })
            .collect(),
        Err(error) => {
            tracing::debug!(%error, streamer_id, "get_streamer_title_history fehlgeschlagen");
            Vec::new()
        }
    }
}

/// Durchschnittliche Viewer-Zahl des Streamers über alle Sessions
/// (Python `get_streamer_avg_viewers`). Kein Login / Fehler / NULL → 0.0.
pub async fn get_streamer_avg_viewers(pool: &PgPool, streamer_id: &str) -> f64 {
    let Some(login) = resolve_streamer_login(pool, streamer_id).await else {
        return 0.0;
    };
    sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(avg_viewers)::float8 FROM twitch_stream_sessions \
         WHERE LOWER(streamer_login) = $1 AND avg_viewers IS NOT NULL",
    )
    .bind(&login)
    .fetch_one(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(0.0)
}

/// Top-Knowledge-Titel nach `normalized_score` (Python
/// `get_top_knowledge_titles`), gefiltert auf `game_context = 'deadlock'`.
/// Query-Fehler (z. B. fehlende Tabelle) → leere Liste.
pub async fn get_top_knowledge_titles(pool: &PgPool, limit: i64) -> Vec<KnowledgeTitle> {
    let rows = sqlx::query_as::<_, (String, Option<f64>, Option<Vec<String>>, Option<i32>)>(
        "SELECT title, normalized_score::float8, keywords, quality_tier::int4 \
         FROM title_generator_knowledge \
         WHERE game_context = 'deadlock' \
         ORDER BY normalized_score DESC \
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|(title, normalized_score, keywords, quality_tier)| KnowledgeTitle {
                title,
                normalized_score,
                keywords: keywords.unwrap_or_default(),
                quality_tier,
            })
            .collect(),
        Err(error) => {
            tracing::debug!(%error, "get_top_knowledge_titles fehlgeschlagen");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new().max_connections(1).connect(dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_streamers (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_stream_sessions (streamer_login TEXT, stream_title TEXT, \
             avg_viewers DOUBLE PRECISION, peak_viewers INTEGER, followers_start INTEGER, \
             started_at TIMESTAMPTZ)",
            "CREATE TABLE title_generator_knowledge (title TEXT, normalized_score DOUBLE PRECISION, \
             keywords TEXT[], quality_tier INTEGER, game_context TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn history_und_avg_ueber_login_aufgeloest() {
        let pool = pool_or_skip!("t6e_title_db");
        sqlx::query("INSERT INTO twitch_streamers (twitch_user_id, twitch_login) VALUES ('900','StreamerX')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (streamer_login, stream_title, avg_viewers, peak_viewers, followers_start, started_at) VALUES \
             ('streamerx','Ranked Grind',10.0,20,100,'2026-06-10T18:00:00+00:00'), \
             ('streamerx','Chill Stream',30.0,40,110,'2026-06-12T18:00:00+00:00'), \
             ('streamerx','',5.0,5,90,'2026-06-13T18:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let history = get_streamer_title_history(&pool, "900", 30).await;
        // Leerer Titel wird ausgefiltert; DESC nach started_at.
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].title, "Chill Stream");
        assert_eq!(history[0].avg_viewers, Some(30.0));
        assert_eq!(history[0].peak_viewers, Some(40));
        assert_eq!(history[1].title, "Ranked Grind");

        // AVG über alle drei nicht-NULL avg_viewers (10,30,5) = 15.
        let avg = get_streamer_avg_viewers(&pool, "900").await;
        assert!((avg - 15.0).abs() < 1e-9, "AVG(10,30,5)=15, war {avg}");

        // Unbekannte streamer_id → leer / 0.
        assert!(get_streamer_title_history(&pool, "404", 30).await.is_empty());
        assert_eq!(get_streamer_avg_viewers(&pool, "404").await, 0.0);
    }

    #[tokio::test]
    async fn knowledge_titel_nach_score_und_deadlock_gefiltert() {
        let pool = pool_or_skip!("t6e_title_knowledge");
        sqlx::query(
            "INSERT INTO title_generator_knowledge (title, normalized_score, keywords, quality_tier, game_context) VALUES \
             ('Top Titel',2.5,ARRAY['ranked','grind'],3,'deadlock'), \
             ('Mittel',1.2,ARRAY['chill'],1,'deadlock'), \
             ('Falsches Game',9.9,ARRAY['x'],3,'valorant')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let titles = get_top_knowledge_titles(&pool, 30).await;
        // Nur deadlock, nach Score DESC.
        assert_eq!(titles.len(), 2);
        assert_eq!(titles[0].title, "Top Titel");
        assert_eq!(
            titles[0].keywords,
            vec!["ranked".to_string(), "grind".to_string()]
        );
        assert_eq!(titles[0].quality_tier, Some(3));
        assert_eq!(titles[1].title, "Mittel");
    }
}

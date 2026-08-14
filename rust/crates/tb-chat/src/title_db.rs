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
    let login: Option<String> = sqlx::query_scalar!(
        "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = $1 LIMIT 1",
        streamer_id,
    )
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
    let rows = sqlx::query!(
        "SELECT \
             s.stream_title AS \"stream_title!\", \
             s.avg_viewers::float8 AS avg_viewers, \
             s.peak_viewers::int8 AS peak_viewers, \
             s.followers_start::int8 AS followers_start, \
             s.started_at AS \"started_at?\" \
         FROM twitch_stream_sessions s \
         WHERE LOWER(s.streamer_login) = $1 \
           AND s.stream_title IS NOT NULL \
           AND s.stream_title != '' \
         ORDER BY s.started_at DESC \
         LIMIT $2",
        &login,
        limit,
    )
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|row| TitleHistoryItem {
                title: row.stream_title,
                avg_viewers: row.avg_viewers,
                peak_viewers: row.peak_viewers,
                followers_start: row.followers_start,
                started_at: row.started_at,
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
    sqlx::query_scalar!(
        "SELECT AVG(avg_viewers)::float8 AS avg_viewers FROM twitch_stream_sessions \
         WHERE LOWER(streamer_login) = $1 AND avg_viewers IS NOT NULL",
        &login,
    )
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
    let rows = sqlx::query!(
        "SELECT title AS \"title!\", \
                normalized_score::float8 AS \"normalized_score?\", \
                keywords, \
                quality_tier::int4 AS \"quality_tier?\" \
         FROM title_generator_knowledge \
         WHERE game_context = 'deadlock' \
         ORDER BY normalized_score DESC \
         LIMIT $1",
        limit,
    )
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|row| KnowledgeTitle {
                title: row.title,
                normalized_score: row.normalized_score,
                keywords: row.keywords.unwrap_or_default(),
                quality_tier: row.quality_tier,
            })
            .collect(),
        Err(error) => {
            tracing::debug!(%error, "get_top_knowledge_titles fehlgeschlagen");
            Vec::new()
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Schreib-Seite: gespeist von den Hintergrundjobs (knowledge_job/insight_job).
// ───────────────────────────────────────────────────────────────────────────

/// Anzahl aufgezeichneter Sessions eines Streamers (Python
/// `get_streamer_session_count`). Kein aufgelöster Login → 0.
pub async fn get_streamer_session_count(pool: &PgPool, streamer_id: &str) -> i64 {
    let Some(login) = resolve_streamer_login(pool, streamer_id).await else {
        return 0;
    };
    sqlx::query_scalar!(
        "SELECT COUNT(*)::int8 AS \"count!\" FROM twitch_stream_sessions WHERE LOWER(streamer_login) = $1",
        &login,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0)
}

/// Knowledge-Eintrag upserten (Python `upsert_knowledge_entry`): bei Konflikt
/// (title, game_context) bleibt der höhere Score (`GREATEST`), und `quality_tier`
/// richtet sich nach dem EINGEHENDEN Score (>2.0→3, >1.5→2, sonst 1). Bei einem
/// frischen INSERT bleibt `quality_tier` auf dem Default 1 (Python-Verhalten:
/// nur der UPDATE-Pfad setzt den Tier).
#[allow(clippy::too_many_arguments)]
pub async fn upsert_knowledge_entry(
    pool: &PgPool,
    title: &str,
    keywords: &[String],
    relative_perf: f64,
    engagement_rate: f64,
    history_weight: f64,
    normalized_score: f64,
    streamer_size: &str,
    source_streamer: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO title_generator_knowledge \
         (title, keywords, relative_perf, engagement_rate, history_weight, \
          normalized_score, streamer_size, source_streamer) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (title, game_context) \
         DO UPDATE SET \
             normalized_score = GREATEST(title_generator_knowledge.normalized_score, EXCLUDED.normalized_score), \
             quality_tier = CASE WHEN EXCLUDED.normalized_score > 2.0 THEN 3 \
                                 WHEN EXCLUDED.normalized_score > 1.5 THEN 2 \
                                 ELSE 1 END",
        title,
        keywords,
        relative_perf,
        engagement_rate,
        history_weight,
        normalized_score,
        streamer_size,
        source_streamer,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Wöchentlichen Insight-Datensatz speichern (Python `insert_insight`).
/// `raw_response` als JSON-Text → `::jsonb` (= Python `json.dumps`).
#[allow(clippy::too_many_arguments)]
pub async fn insert_insight(
    pool: &PgPool,
    streamer_id: &str,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
    strengths: &str,
    weaknesses: &str,
    patterns: &str,
    recommendations: &str,
    raw_response: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    let raw = serde_json::to_string(raw_response).unwrap_or_else(|_| "{}".to_string());
    sqlx::query!(
        "INSERT INTO title_generator_insights \
         (streamer_id, period_start, period_end, strengths, weaknesses, \
          patterns, recommendations, raw_response) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::text::jsonb)",
        streamer_id,
        period_start,
        period_end,
        strengths,
        weaknesses,
        patterns,
        recommendations,
        raw,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Neuesten wöchentlichen Insight-Datensatz laden.
pub async fn get_latest_insight(
    pool: &PgPool,
    streamer_id: &str,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT strengths, weaknesses, patterns, recommendations, generated_at AS \"generated_at?\" \
         FROM title_generator_insights \
         WHERE streamer_id = $1 \
         ORDER BY generated_at DESC LIMIT 1",
        streamer_id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| {
        serde_json::json!({
            "strengths": row.strengths.unwrap_or_default(),
            "weaknesses": row.weaknesses.unwrap_or_default(),
            "patterns": row.patterns.unwrap_or_default(),
            "recommendations": row.recommendations.unwrap_or_default(),
            "generated_at": row.generated_at,
        })
    }))
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
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
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
        assert!(get_streamer_title_history(&pool, "404", 30)
            .await
            .is_empty());
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

    #[tokio::test]
    async fn session_count_ueber_login_aufgeloest() {
        let pool = pool_or_skip!("t6e_title_session_count");
        sqlx::query("INSERT INTO twitch_streamers (twitch_user_id, twitch_login) VALUES ('900','StreamerX')")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, stream_title, avg_viewers, peak_viewers, followers_start, started_at) VALUES \
             ('streamerx','a',10.0,20,100,'2026-06-10T18:00:00+00'), \
             ('streamerx','b',20.0,30,100,'2026-06-11T18:00:00+00'), \
             ('streamerx','c',30.0,40,100,'2026-06-12T18:00:00+00')",
        )
        .execute(&pool).await.unwrap();
        assert_eq!(get_streamer_session_count(&pool, "900").await, 3);
        assert_eq!(get_streamer_session_count(&pool, "404").await, 0); // unbekannte ID
    }

    #[tokio::test]
    async fn latest_insight_liefert_neuesten_datensatz() {
        let pool = pool_or_skip!("t6e_title_latest_insight");
        sqlx::query(
            "CREATE TABLE title_generator_insights (streamer_id TEXT, strengths TEXT, \
             weaknesses TEXT, patterns TEXT, recommendations TEXT, generated_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO title_generator_insights VALUES \
             ('900','alt','w','p','r','2026-06-01T00:00:00+00'), \
             ('900','neu','w2','p2','r2','2026-06-10T00:00:00+00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let insight = get_latest_insight(&pool, "900").await.unwrap().unwrap();
        assert_eq!(insight["strengths"], "neu");
        assert_eq!(insight["recommendations"], "r2");
    }

    #[tokio::test]
    async fn upsert_knowledge_greatest_und_tier() {
        let pool = pool_or_skip!("t6e_title_knowledge_upsert");
        // Volle Tabelle (Minimal-Variante aus pool_in_schema hat nicht alle Spalten/UNIQUE).
        sqlx::query("DROP TABLE title_generator_knowledge")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE title_generator_knowledge (\
             id SERIAL PRIMARY KEY, title TEXT NOT NULL, keywords TEXT[] DEFAULT '{}', \
             game_context TEXT NOT NULL DEFAULT 'deadlock', relative_perf FLOAT NOT NULL, \
             engagement_rate FLOAT NOT NULL, history_weight FLOAT NOT NULL DEFAULT 1.0, \
             normalized_score FLOAT NOT NULL, \
             streamer_size TEXT CHECK (streamer_size IN ('small','medium','large')), \
             source_streamer TEXT, \
             quality_tier SMALLINT NOT NULL DEFAULT 1 CHECK (quality_tier IN (1,2,3)), \
             UNIQUE (title, game_context))",
        )
        .execute(&pool)
        .await
        .unwrap();
        let kw = vec!["ranked".to_string(), "grind".to_string()];

        // 1. Frischer INSERT: Score 1.5, quality_tier bleibt Default 1.
        upsert_knowledge_entry(&pool, "X", &kw, 1.2, 0.05, 1.0, 1.5, "small", "s1")
            .await
            .unwrap();
        let (score, tier): (f64, i32) = sqlx::query_as(
            "SELECT normalized_score::float8, quality_tier::int4 FROM title_generator_knowledge WHERE title='X'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(score, 1.5);
        assert_eq!(tier, 1); // INSERT-Pfad setzt keinen Tier

        // 2. Konflikt mit niedrigerem Score 1.3 → GREATEST behält 1.5, Tier = CASE(1.3) = 1.
        upsert_knowledge_entry(&pool, "X", &kw, 1.0, 0.04, 1.0, 1.3, "small", "s2")
            .await
            .unwrap();
        let (score2, tier2): (f64, i32) = sqlx::query_as(
            "SELECT normalized_score::float8, quality_tier::int4 FROM title_generator_knowledge WHERE title='X'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(score2, 1.5); // GREATEST(1.5, 1.3)
        assert_eq!(tier2, 1);

        // 3. Konflikt mit höherem Score 2.5 → Score 2.5, Tier = CASE(2.5) = 3.
        upsert_knowledge_entry(&pool, "X", &kw, 2.0, 0.1, 1.0, 2.5, "large", "s3")
            .await
            .unwrap();
        let (score3, tier3): (f64, i32) = sqlx::query_as(
            "SELECT normalized_score::float8, quality_tier::int4 FROM title_generator_knowledge WHERE title='X'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(score3, 2.5);
        assert_eq!(tier3, 3);

        // Keine Duplikate (UNIQUE title, game_context).
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*)::int8 FROM title_generator_knowledge")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn insert_insight_persistiert() {
        let pool = pool_or_skip!("t6e_title_insight");
        sqlx::query(
            "CREATE TABLE title_generator_insights (\
             id SERIAL PRIMARY KEY, streamer_id TEXT NOT NULL, generated_at TIMESTAMPTZ DEFAULT NOW(), \
             period_start TIMESTAMPTZ NOT NULL, period_end TIMESTAMPTZ NOT NULL, \
             strengths TEXT, weaknesses TEXT, patterns TEXT, recommendations TEXT, raw_response JSONB)",
        )
        .execute(&pool).await.unwrap();
        let start = chrono::DateTime::parse_from_rfc3339("2026-05-17T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end = chrono::DateTime::parse_from_rfc3339("2026-06-14T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let raw = serde_json::json!({"k": "v"});
        insert_insight(
            &pool,
            "900",
            start,
            end,
            "stark",
            "schwach",
            "muster",
            "empfehlung",
            &raw,
        )
        .await
        .unwrap();
        let (sid, strengths, raw_k): (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT streamer_id, strengths, raw_response->>'k' FROM title_generator_insights WHERE streamer_id='900'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(sid, "900");
        assert_eq!(strengths.as_deref(), Some("stark"));
        assert_eq!(raw_k.as_deref(), Some("v")); // JSONB korrekt gespeichert
    }
}

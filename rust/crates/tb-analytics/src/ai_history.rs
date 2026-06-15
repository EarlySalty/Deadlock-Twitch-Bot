//! AI-Analyse-Historie (`GET /twitch/api/v2/ai/history`).
//!
//! Port von `bot/analytics/api_ai.py:_api_v2_ai_history` (reiner DB-Read; der
//! `_cleanup_ai_chat_state`-Aufruf betrifft nur den In-Memory-State von ai/chat
//! und ist für die Historie ohne Wirkung → ausgelassen).

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

fn emit_iso(dt: DateTime<Utc>) -> String {
    if dt.timestamp_subsec_nanos() == 0 {
        dt.to_rfc3339_opts(SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(SecondsFormat::Micros, false)
    }
}

/// Stellt die `ai_analyses`-Tabelle sicher (Python `_ensure_ai_table`, idempotent).
async fn ensure_ai_table(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ai_analyses ( \
            id BIGSERIAL PRIMARY KEY, \
            streamer TEXT NOT NULL, \
            days INTEGER NOT NULL, \
            model TEXT NOT NULL, \
            generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
            data_snapshot JSONB NOT NULL, \
            points JSONB NOT NULL )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ai_analyses_streamer_ts ON ai_analyses (streamer, generated_at DESC)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Persistiert eine KI-Analyse (Python: `INSERT INTO ai_analyses … RETURNING id`,
/// best-effort). `model_name` ist der echte Modellname (`claude-opus-4-6` /
/// `MiniMax-M3`), NICHT die `opus`/`minimax`-Kennung. Bei jedem Fehler `None`
/// (mirror Pythons `try/except` mit `log.warning` — blockiert die Antwort nie).
#[allow(clippy::too_many_arguments)]
pub async fn save_analysis(
    pool: &PgPool,
    streamer: &str,
    days: i64,
    model_name: &str,
    generated_at: DateTime<Utc>,
    data_snapshot: &Value,
    points: &Value,
) -> Option<i64> {
    if ensure_ai_table(pool).await.is_err() {
        return None;
    }
    // JSONB normalisiert Whitespace → Input-Serialisierung egal.
    let snapshot_text = data_snapshot.to_string();
    let points_text = points.to_string();
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO ai_analyses (streamer, days, model, generated_at, data_snapshot, points) \
         VALUES ($1, $2, $3, $4, $5::jsonb, $6::jsonb) RETURNING id",
    )
    .bind(streamer)
    .bind(days)
    .bind(model_name)
    .bind(generated_at)
    .bind(snapshot_text)
    .bind(points_text)
    .fetch_one(pool)
    .await
    .ok()
}

fn count_priority(points: &Value, priority: &str) -> i64 {
    points
        .as_array()
        .map(|arr| arr.iter().filter(|p| p.get("priority").and_then(Value::as_str) == Some(priority)).count() as i64)
        .unwrap_or(0)
}

/// Lädt die letzten AI-Analysen eines Streamers (neueste zuerst).
pub async fn load_ai_history(pool: &PgPool, streamer: &str, limit: i64) -> Result<Value, sqlx::Error> {
    ensure_ai_table(pool).await?;

    let rows: Vec<(i64, String, i32, String, DateTime<Utc>, Value, Value)> = sqlx::query_as(
        "SELECT id::bigint, streamer, days, model, generated_at, data_snapshot, points \
           FROM ai_analyses WHERE streamer = $1 ORDER BY generated_at DESC LIMIT $2",
    )
    .bind(streamer)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let result: Vec<Value> = rows
        .into_iter()
        .map(|(id, streamer, days, model, generated_at, snap, points)| {
            // model_alias: "claude" im Namen → opus, sonst minimax (Python AI_MODEL_*).
            let model_alias = if model.contains("claude") { "opus" } else { "minimax" };
            json!({
                "id": id,
                "streamer": streamer,
                "days": days,
                "model": model_alias,
                "generatedAt": emit_iso(generated_at),
                "dataSnapshot": snap,
                "points": points,
                "kritischCount": count_priority(&points, "kritisch"),
                "hochCount": count_priority(&points, "hoch"),
                "mittelCount": count_priority(&points, "mittel"),
            })
        })
        .collect();

    Ok(Value::Array(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        Some(PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap())
    }

    #[tokio::test]
    async fn ensure_table_und_leer() {
        let Some(pool) = make_pool("t_aih_empty").await else { return };
        // Tabelle existiert nicht → ensure legt sie an → leeres Array.
        let v = load_ai_history(&pool, "nani", 20).await.unwrap();
        assert_eq!(v, json!([]));
    }

    #[tokio::test]
    async fn historie_mit_counts() {
        let Some(pool) = make_pool("t_aih").await else { return };
        ensure_ai_table(&pool).await.unwrap();
        sqlx::query("INSERT INTO ai_analyses (streamer, days, model, generated_at, data_snapshot, points) VALUES \
            ('nani', 30, 'claude-opus', NOW()-INTERVAL '1 hour', '{\"x\":1}'::jsonb, '[{\"priority\":\"kritisch\"},{\"priority\":\"hoch\"},{\"priority\":\"kritisch\"}]'::jsonb), \
            ('nani', 7, 'minimax-m3', NOW(), '{}'::jsonb, '[{\"priority\":\"mittel\"}]'::jsonb), \
            ('other', 30, 'claude', NOW(), '{}'::jsonb, '[]'::jsonb)")
            .execute(&pool).await.unwrap();

        let v = load_ai_history(&pool, "nani", 20).await.unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2); // nur nani
        // neueste zuerst: minimax-Eintrag (NOW) vor claude (NOW-1h).
        assert_eq!(arr[0]["model"], "minimax");
        assert_eq!(arr[0]["mittelCount"], 1);
        assert_eq!(arr[1]["model"], "opus"); // claude → opus
        assert_eq!(arr[1]["kritischCount"], 2);
        assert_eq!(arr[1]["hochCount"], 1);
        assert_eq!(arr[1]["dataSnapshot"]["x"], 1);
    }

    #[tokio::test]
    async fn save_analysis_persistiert() {
        let Some(pool) = make_pool("t_aih_save").await else { return };
        let id = save_analysis(
            &pool,
            "nani",
            30,
            "claude-opus-4-6",
            Utc::now(),
            &json!({"k": "v"}),
            &json!([{"priority": "kritisch"}]),
        )
        .await;
        assert!(id.is_some());
        // Über load_ai_history gegenlesen: model claude → opus-Alias, snapshot da.
        let v = load_ai_history(&pool, "nani", 5).await.unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["model"], "opus");
        assert_eq!(arr[0]["dataSnapshot"]["k"], "v");
        assert_eq!(arr[0]["kritischCount"], 1);
    }

    #[tokio::test]
    async fn limit_greift() {
        let Some(pool) = make_pool("t_aih_limit").await else { return };
        ensure_ai_table(&pool).await.unwrap();
        for i in 0..5 {
            sqlx::query("INSERT INTO ai_analyses (streamer, days, model, generated_at, data_snapshot, points) VALUES ('nani',30,'minimax', NOW()-($1 || ' minutes')::interval, '{}'::jsonb, '[]'::jsonb)")
                .bind(i.to_string()).execute(&pool).await.unwrap();
        }
        let v = load_ai_history(&pool, "nani", 3).await.unwrap();
        assert_eq!(v.as_array().unwrap().len(), 3);
    }
}

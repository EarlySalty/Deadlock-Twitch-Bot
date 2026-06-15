//! DB-Reads der Pipeline-Gates + Decision-Logging (Port der `_sync_*`-Helfer aus
//! `bot/engagement/pipeline.py`).
//!
//! Slice 18a: die Datenzugriffe, die der Orchestrator (18b) braucht —
//! Engagement-Settings, Opt-out, operativer-Partner-Gate und das
//! Decision-Log in `twitch_engagement_log`.

use sqlx::PgPool;

use crate::types::{Decision, EngagementSettings, HandleResult};

/// Lädt die Engagement-Settings eines Channels (Python `_sync_load_settings`).
pub async fn load_settings(pool: &PgPool, channel_login: &str) -> Option<EngagementSettings> {
    let row: Option<(String, bool, Option<String>, Option<String>, Option<Vec<String>>)> =
        sqlx::query_as(
            "SELECT channel_login, enabled, steam_id, persona_override, tabu_topics \
             FROM twitch_engagement_settings WHERE channel_login = $1",
        )
        .bind(channel_login)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    row.map(|(channel_login, enabled, steam_id, persona_override, tabu_topics)| EngagementSettings {
        channel_login,
        enabled,
        steam_id,
        persona_override,
        tabu_topics: tabu_topics.unwrap_or_default(),
    })
}

/// Hat sich der User vom Engagement abgemeldet? (Python `_sync_is_opted_out`).
pub async fn is_opted_out(pool: &PgPool, twitch_user_id: &str) -> bool {
    sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM twitch_user_engagement_optout WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

/// True nur für operativ aktive Partner-Channels (Python
/// `is_operational_partner_channel`): `is_partner_active` aus
/// `twitch_streamers_partner_state` über `LOWER(twitch_login)`.
pub async fn is_operational_partner(pool: &PgPool, channel_login: &str) -> bool {
    let norm = channel_login.trim().to_lowercase();
    let norm = norm.trim_start_matches('#');
    if norm.is_empty() {
        return false;
    }
    let active: Option<i32> = sqlx::query_scalar(
        "SELECT is_partner_active::int FROM twitch_streamers_partner_state \
         WHERE LOWER(twitch_login) = $1",
    )
    .bind(norm)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    active.unwrap_or(0) != 0
}

/// Schreibt eine Engagement-Entscheidung ins Log (Python `_sync_log_decision`).
#[allow(clippy::too_many_arguments)]
pub async fn log_decision(
    pool: &PgPool,
    channel_login: &str,
    triggered_by_msg_id: Option<&str>,
    result: &HandleResult,
    cost_usd: Option<f64>,
) {
    let _ = sqlx::query(
        "INSERT INTO twitch_engagement_log \
         (channel_login, triggered_by_msg_id, decision, response_text, referenced_thread_ids, \
          model, prompt_tokens, completion_tokens, cost_usd_estimate, latency_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(channel_login)
    .bind(triggered_by_msg_id)
    .bind(result.decision.as_str())
    .bind(result.response_text.as_deref())
    .bind(result.referenced_thread_ids.as_deref())
    .bind(result.model.as_deref().unwrap_or(""))
    .bind(result.prompt_tokens.map(|t| t as i32))
    .bind(result.completion_tokens.map(|t| t as i32))
    .bind(cost_usd)
    .bind(result.latency_ms.map(|t| t as i32))
    .execute(pool)
    .await;
}

/// Bequemer Helfer: Decision-Wert (für Tests/Aufrufer).
pub fn disabled() -> HandleResult {
    HandleResult::new(Decision::Disabled)
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
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_settings (\
             channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT FALSE, \
             steam_id TEXT, persona_override TEXT, tabu_topics TEXT[])",
        )
        .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_user_engagement_optout (twitch_user_id TEXT PRIMARY KEY, opted_out_at TIMESTAMPTZ DEFAULT NOW())")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT, is_partner_active INTEGER)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_log (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, triggered_by_msg_id TEXT, \
             decision TEXT NOT NULL, response_text TEXT, referenced_thread_ids BIGINT[], \
             model TEXT NOT NULL, prompt_tokens INT, completion_tokens INT, \
             cost_usd_estimate DOUBLE PRECISION, latency_ms INT, \
             created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn settings_optout_partner() {
        let Some(pool) = make_pool("t_eng_gate").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, steam_id, tabu_topics) VALUES ('nani', TRUE, '123', ARRAY['politik','religion'])")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_user_engagement_optout (twitch_user_id) VALUES ('u_out')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('Nani', 1), ('passiv', 0)").execute(&pool).await.unwrap();

        let s = load_settings(&pool, "nani").await.unwrap();
        assert!(s.enabled);
        assert_eq!(s.steam_id.as_deref(), Some("123"));
        assert_eq!(s.tabu_topics, vec!["politik".to_string(), "religion".to_string()]);
        assert!(load_settings(&pool, "unbekannt").await.is_none());

        assert!(is_opted_out(&pool, "u_out").await);
        assert!(!is_opted_out(&pool, "u_in").await);

        // Partner-Gate: LOWER(twitch_login) match, # gestrippt.
        assert!(is_operational_partner(&pool, "#Nani").await);
        assert!(!is_operational_partner(&pool, "passiv").await);
        assert!(!is_operational_partner(&pool, "unbekannt").await);
    }

    #[tokio::test]
    async fn log_decision_schreibt() {
        let Some(pool) = make_pool("t_eng_gate_log").await else { return };
        let result = HandleResult {
            decision: Decision::Spoke,
            response_text: Some("antwort".to_string()),
            model: Some("MiniMax-M3".to_string()),
            prompt_tokens: Some(42),
            completion_tokens: Some(7),
            latency_ms: Some(120),
            referenced_thread_ids: Some(vec![1, 2]),
        };
        log_decision(&pool, "nani", Some("m1"), &result, Some(0.003)).await;
        let row: (String, String, Option<String>, Vec<i64>, i32) = sqlx::query_as(
            "SELECT channel_login, decision, response_text, referenced_thread_ids, completion_tokens \
             FROM twitch_engagement_log LIMIT 1",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(row.1, "spoke");
        assert_eq!(row.2.as_deref(), Some("antwort"));
        assert_eq!(row.3, vec![1, 2]);
        assert_eq!(row.4, 7);
    }
}

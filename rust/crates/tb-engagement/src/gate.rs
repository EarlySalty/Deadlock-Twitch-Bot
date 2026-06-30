//! DB-Reads der Pipeline-Gates + Decision-Logging (Port der `_sync_*`-Helfer aus
//! `bot/engagement/pipeline.py`).
//!
//! Slice 18a: die Datenzugriffe, die der Orchestrator (18b) braucht —
//! Engagement-Settings, Opt-out, operativer-Partner-Gate und das
//! Decision-Log in `twitch_engagement_log`.

use sqlx::PgPool;

use crate::types::{Decision, EngagementSettings, HandleResult, OutputMode};

/// Lädt die Engagement-Settings eines Channels (Python `_sync_load_settings`,
/// erweitert um die additive Spalte `output_mode`).
///
/// `output_mode` kommt aus der Migration mit `NOT NULL DEFAULT 'off'`; ein
/// fehlender/unbekannter Wert fällt über [`OutputMode::from_db`] fail-safe auf
/// `off` zurück (kein Output im Zweifel).
pub async fn load_settings(pool: &PgPool, channel_login: &str) -> Option<EngagementSettings> {
    let row = sqlx::query!(
        r#"SELECT channel_login AS "channel_login!", enabled AS "enabled!",
                  steam_id, persona_override, tabu_topics, output_mode AS "output_mode?"
             FROM twitch_engagement_settings WHERE channel_login = $1"#,
        channel_login
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    row.map(|r| EngagementSettings {
        channel_login: r.channel_login,
        enabled: r.enabled,
        steam_id: r.steam_id,
        persona_override: r.persona_override,
        tabu_topics: r.tabu_topics.unwrap_or_default(),
        output_mode: OutputMode::from_db(r.output_mode.as_deref().unwrap_or("off")),
    })
}

/// Hat sich der User vom Engagement abgemeldet? (Python `_sync_is_opted_out`).
pub async fn is_opted_out(pool: &PgPool, twitch_user_id: &str) -> bool {
    match sqlx::query_scalar!(
        r#"SELECT 1 AS "opted_out!" FROM twitch_user_engagement_optout WHERE twitch_user_id = $1"#,
        twitch_user_id
    )
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row.is_some(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                twitch_user_id,
                "Engagement: opt-out-Check fehlgeschlagen - fail-safe als opted-out behandelt"
            );
            true
        }
    }
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
    match sqlx::query_scalar!(
        r#"SELECT is_partner_active::int AS "is_partner_active!"
           FROM twitch_streamers_partner_state
           WHERE LOWER(twitch_login) = $1"#,
        norm
    )
    .fetch_optional(pool)
    .await
    {
        Ok(active) => active.unwrap_or(0) != 0,
        Err(e) => {
            tracing::warn!(
                error = %e,
                channel = norm,
                "Engagement: operational partner DB-Fehler - fail-closed (false)"
            );
            false
        }
    }
}

/// Schreibt eine Engagement-Entscheidung ins Log (Python `_sync_log_decision`).
///
/// Der in die `response_text`-Spalte geschriebene Text ist im `live`-Modus
/// [`HandleResult::response_text`] (der gesendete Text) und im `shadow`-Modus
/// [`HandleResult::shadow_text`] (der erzeugte, aber nicht gesendete Text). So
/// findet das Discord-Review-Ticket den gestagten Shadow-Output direkt im Log.
#[allow(clippy::too_many_arguments)]
pub async fn log_decision(
    pool: &PgPool,
    channel_login: &str,
    triggered_by_msg_id: Option<&str>,
    result: &HandleResult,
    cost_usd: Option<f64>,
) {
    let logged_text = result.response_text.as_deref().or(result.shadow_text.as_deref());
    let _ = sqlx::query!(
        "INSERT INTO twitch_engagement_log \
         (channel_login, triggered_by_msg_id, decision, response_text, referenced_thread_ids, \
          model, prompt_tokens, completion_tokens, cost_usd_estimate, latency_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, ($9::double precision)::numeric, $10)",
        channel_login,
        triggered_by_msg_id,
        result.decision.as_str(),
        logged_text,
        result.referenced_thread_ids.as_deref(),
        result.model.as_deref().unwrap_or(""),
        result.prompt_tokens.map(|t| t as i32),
        result.completion_tokens.map(|t| t as i32),
        cost_usd,
        result.latency_ms.map(|t| t as i32)
    )
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

    async fn closed_pool() -> PgPool {
        let pool = PgPoolOptions::new().max_connections(1).connect_lazy_with(PgConnectOptions::new());
        pool.close().await;
        pool
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
            "CREATE TABLE twitch_engagement_settings (\
             channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT FALSE, \
             steam_id TEXT, persona_override TEXT, tabu_topics TEXT[], \
             output_mode TEXT NOT NULL DEFAULT 'off')",
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
        // output_mode kommt aus dem Spalten-Default → off (kein Output ohne Toggle).
        assert_eq!(s.output_mode, OutputMode::Off);
        assert!(load_settings(&pool, "unbekannt").await.is_none());

        // Explizit gesetzter Modus wird gelesen.
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('livech', TRUE, 'live'), ('shadowch', TRUE, 'shadow')")
            .execute(&pool).await.unwrap();
        assert_eq!(load_settings(&pool, "livech").await.unwrap().output_mode, OutputMode::Live);
        assert_eq!(load_settings(&pool, "shadowch").await.unwrap().output_mode, OutputMode::Shadow);

        assert!(is_opted_out(&pool, "u_out").await);
        assert!(!is_opted_out(&pool, "u_in").await);

        // Partner-Gate: LOWER(twitch_login) match, # gestrippt.
        assert!(is_operational_partner(&pool, "#Nani").await);
        assert!(!is_operational_partner(&pool, "passiv").await);
        assert!(!is_operational_partner(&pool, "unbekannt").await);
    }

    #[tokio::test]
    async fn optout_db_error_fail_safe_true() {
        let pool = closed_pool().await;
        assert!(is_opted_out(&pool, "u_out").await);
    }

    #[tokio::test]
    async fn operational_partner_db_error_fail_closed_false() {
        let pool = closed_pool().await;
        assert!(!is_operational_partner(&pool, "nani").await);
    }

    #[tokio::test]
    async fn log_decision_schreibt() {
        let Some(pool) = make_pool("t_eng_gate_log").await else { return };
        let result = HandleResult {
            decision: Decision::Spoke,
            response_text: Some("antwort".to_string()),
            shadow_text: None,
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

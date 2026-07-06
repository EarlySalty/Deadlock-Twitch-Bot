//! Manuelles Lernen für Twitch-Spam-Alerts.
//!
//! Discord-Buttons bestätigen ein bereits gemeldetes `sus_spam` entweder als
//! echtes Spam-Muster oder als harmloses Safe-Muster. Gespeichert wird direkt
//! in den vorhandenen Auto-Learning-Tabellen.

use axum::{extract::State, response::IntoResponse, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_http_core::{ApiError, AuthLevel};

const MAX_PATTERN_CHARS: usize = 200;
const MAX_SOURCE_CHARS: usize = 500;
const MAX_REASON_CHARS: usize = 200;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpamLearningRequest {
    pub verdict: String,
    pub pattern: String,
    #[serde(default)]
    pub pattern_type: Option<String>,
    #[serde(default)]
    pub source_message: Option<String>,
    #[serde(default)]
    pub source_channel: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct SpamLearningResponse {
    pub ok: bool,
    pub verdict: String,
    pub pattern: String,
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.trim().chars().take(max).collect()
}

fn normalize_pattern(raw: &str) -> Result<String, ApiError> {
    let pattern = truncate_chars(raw, MAX_PATTERN_CHARS).to_lowercase();
    if pattern.chars().count() < 4 {
        return Err(ApiError::bad_request(
            "pattern must have at least 4 characters",
        ));
    }
    Ok(pattern)
}

fn normalize_pattern_type(raw: Option<String>) -> String {
    match raw.as_deref().map(str::trim) {
        Some("phrase") => "phrase".to_string(),
        _ => "fragment".to_string(),
    }
}

fn normalize_optional(raw: Option<String>, max: usize) -> Option<String> {
    let text = truncate_chars(raw.as_deref().unwrap_or(""), max);
    (!text.is_empty()).then_some(text)
}

/// `POST /internal/twitch/v1/spam-learning`
pub async fn learn_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<SpamLearningRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let verdict = body.verdict.trim().to_lowercase();
    let pattern = normalize_pattern(&body.pattern)?;
    let pattern_type = normalize_pattern_type(body.pattern_type);
    let source_message = normalize_optional(body.source_message, MAX_SOURCE_CHARS);
    let source_channel = normalize_optional(body.source_channel, 100)
        .map(|s| s.trim_start_matches(['#', '@']).to_lowercase());
    let reason = normalize_optional(body.reason, MAX_REASON_CHARS);
    let now = Utc::now();

    match verdict.as_str() {
        "spam" => {
            sqlx::query(
                r#"
                INSERT INTO twitch_auto_learned_spam_patterns
                    (pattern, pattern_type, source_message, source_channel, minimax_reasoning, created_at)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (pattern) DO UPDATE SET
                    hit_count = twitch_auto_learned_spam_patterns.hit_count + 1
                "#,
            )
            .bind(&pattern)
            .bind(&pattern_type)
            .bind(&source_message)
            .bind(&source_channel)
            .bind(&reason)
            .bind(now)
            .execute(&pool)
            .await
            .map_err(|e| {
                tracing::error!("spam-learning spam DB-Fehler: {e}");
                ApiError::internal()
            })?;
        }
        "safe" => {
            sqlx::query(
                r#"
                INSERT INTO twitch_auto_learned_safe_patterns
                    (pattern, source_message, source_channel, minimax_reasoning, created_at)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (pattern) DO UPDATE SET
                    hit_count = twitch_auto_learned_safe_patterns.hit_count + 1
                "#,
            )
            .bind(&pattern)
            .bind(&source_message)
            .bind(&source_channel)
            .bind(&reason)
            .bind(now)
            .execute(&pool)
            .await
            .map_err(|e| {
                tracing::error!("spam-learning safe DB-Fehler: {e}");
                ApiError::internal()
            })?;
        }
        _ => return Err(ApiError::bad_request("verdict must be spam or safe")),
    }

    Ok(Json(SpamLearningResponse {
        ok: true,
        verdict,
        pattern,
    }))
}

#[cfg(test)]
mod tests {
    use super::{normalize_pattern, normalize_pattern_type};

    #[test]
    fn pattern_wird_getrimmt_gekappt_und_kleingeschrieben() {
        assert_eq!(
            normalize_pattern("  StreamBoo.COM  ").unwrap(),
            "streamboo.com"
        );
    }

    #[test]
    fn zu_kurzes_pattern_wird_abgelehnt() {
        assert!(normalize_pattern("abc").is_err());
    }

    #[test]
    fn pattern_type_ist_nur_phrase_oder_fragment() {
        assert_eq!(normalize_pattern_type(Some("phrase".into())), "phrase");
        assert_eq!(normalize_pattern_type(Some("x".into())), "fragment");
        assert_eq!(normalize_pattern_type(None), "fragment");
    }
}

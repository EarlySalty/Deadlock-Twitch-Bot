//! Korrektur-Endpoints für den Spam-Judge.
//!
//! Der Judge (tb-chat) lernt Spam-Muster selbst; die Discord-Buttons
//! korrigieren ihn nur noch:
//! - `POST …/spam-learning` („Als Spam korrigieren" bei Harmlos-Urteil):
//!   lernt ein Spam-Muster. Das Distinktivitäts-Gate aus tb-chat gilt auch hier.
//! - `POST …/spam-learning/safe` („Als harmlos korrigieren" ohne Spam-Row):
//!   schreibt das manuelle Safe-Pattern und ein `clean`-Audit.
//! - `POST …/spam-learning/correct` („Als harmlos korrigieren" bei
//!   Spam-Urteil): übernimmt die Originalnachricht der Spam-Row als Safe-Pattern,
//!   auditert die Korrektur und entfernt die falsche Spam-Row atomar.

use axum::{Json, extract::State, response::IntoResponse};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_http_core::{ApiError, AuthLevel};

const MAX_PATTERN_CHARS: usize = 200;
const MAX_SOURCE_CHARS: usize = 500;
const MAX_SAFE_MESSAGE_CHARS: usize = 500;
const MAX_REASON_CHARS: usize = 200;
const MANUAL_SAFE_SOURCE_CHANNEL: &str = "discord-correction";

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
    /// false, wenn das Distinktivitäts-Gate das Muster abgelehnt hat
    /// (generisches Vokabular) — Request ok, aber nichts gespeichert.
    pub learned: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpamSafeFeedbackRequest {
    #[serde(default)]
    pub verdict: Option<String>,
    pub pattern: String,
    #[serde(default)]
    pub source_channel: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct SpamSafeFeedbackResponse {
    pub ok: bool,
    pub recorded: bool,
    pub pattern: String,
}

#[derive(Deserialize)]
pub struct SpamCorrectRequest {
    /// Bisher nur "spam" — Feld existiert für Vorwärtskompatibilität.
    pub table: String,
    pub id: i64,
}

#[derive(Serialize)]
pub struct SpamCorrectResponse {
    pub ok: bool,
    pub deleted: bool,
    pub pattern: Option<String>,
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

struct NormalizedSafeFeedback {
    pattern: String,
    source_message: String,
    source_channel: String,
    reason: String,
}

fn normalize_safe_feedback(
    raw_pattern: &str,
    raw_source_channel: Option<String>,
    raw_reason: Option<String>,
) -> Result<NormalizedSafeFeedback, ApiError> {
    let source_message = truncate_chars(raw_pattern, MAX_SAFE_MESSAGE_CHARS);
    let pattern = tb_chat::spam_filter::normalize_exact_spam_message(&source_message);
    if pattern.chars().count() < 4 {
        return Err(ApiError::bad_request(
            "pattern must have at least 4 characters",
        ));
    }
    let source_channel = normalize_optional(raw_source_channel, 100)
        .map(|value| value.trim_start_matches(['#', '@']).trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "discord-correction".to_string());
    let reason = normalize_optional(raw_reason, MAX_REASON_CHARS)
        .unwrap_or_else(|| "Manuelle Harmlos-Korrektur (Discord)".to_string());
    Ok(NormalizedSafeFeedback {
        source_message,
        pattern,
        source_channel,
        reason,
    })
}

async fn insert_safe_pattern(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    feedback: &NormalizedSafeFeedback,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_auto_learned_safe_patterns
            (pattern, source_message, source_channel, minimax_reasoning)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (pattern) DO UPDATE SET
            source_message = EXCLUDED.source_message,
            source_channel = EXCLUDED.source_channel,
            minimax_reasoning = EXCLUDED.minimax_reasoning,
            hit_count = twitch_auto_learned_safe_patterns.hit_count + 1",
    )
    .bind(&feedback.pattern)
    .bind(&feedback.source_message)
    .bind(MANUAL_SAFE_SOURCE_CHANNEL)
    .bind(&feedback.reason)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

async fn insert_clean_review(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    channel_login: &str,
    feedback: &NormalizedSafeFeedback,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_spam_review_decisions \
         (channel_login, chatter_login, chatter_id, source_message, verdict, confidence, reason) \
         VALUES ($1, $2, $3, $4, 'clean', NULL, $5)",
    )
    .bind(channel_login)
    .bind("discord-moderator")
    .bind(Option::<String>::None)
    .bind(&feedback.source_message)
    .bind(&feedback.reason)
    .execute(&mut **tx)
    .await
    .map(|_| ())
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

    if verdict != "spam" {
        // Der Spam-Endpunkt lernt ausschließlich AI-/Mod-Spam. Manuelles
        // Safe-Lernen läuft getrennt über /safe bzw. /correct.
        return Err(ApiError::bad_request("verdict must be spam"));
    }

    if !tb_chat::spam_filter::is_distinctive_spam_pattern_vom_menschen(&pattern) {
        tracing::warn!(
            pattern = %pattern,
            "spam-learning: Muster abgelehnt (Distinktivitäts-Gate, nur generisches Vokabular)"
        );
        return Ok(Json(SpamLearningResponse {
            ok: true,
            verdict,
            pattern,
            learned: false,
        }));
    }

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

    Ok(Json(SpamLearningResponse {
        ok: true,
        verdict,
        pattern,
        learned: true,
    }))
}

/// `POST /internal/twitch/v1/spam-learning/safe`
///
/// Speichert menschliches Review-Feedback als aktives, exaktes Safe-Pattern
/// und zusätzlich als `clean`-Audit.
pub async fn safe_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<SpamSafeFeedbackRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    if body
        .verdict
        .as_deref()
        .is_some_and(|verdict| !verdict.trim().eq_ignore_ascii_case("clean"))
    {
        return Err(ApiError::bad_request("verdict must be clean"));
    }

    let feedback = normalize_safe_feedback(&body.pattern, body.source_channel, body.reason)?;
    let mut tx = pool.begin().await.map_err(|error| {
        tracing::error!(%error, "spam-learning safe feedback Transaktion fehlgeschlagen");
        ApiError::internal()
    })?;
    insert_safe_pattern(&mut tx, &feedback)
        .await
        .map_err(|error| {
            tracing::error!(%error, "spam-learning safe feedback DB-Fehler");
            ApiError::internal()
        })?;
    insert_clean_review(&mut tx, &feedback.source_channel, &feedback)
        .await
        .map_err(|error| {
            tracing::error!(%error, "spam-learning safe feedback DB-Fehler");
            ApiError::internal()
        })?;
    tx.commit().await.map_err(|error| {
        tracing::error!(%error, "spam-learning safe feedback Commit fehlgeschlagen");
        ApiError::internal()
    })?;

    tracing::info!(
        pattern = %feedback.pattern,
        source_channel = %feedback.source_channel,
        "spam-learning: menschliches Harmlos-Feedback gespeichert"
    );
    Ok(Json(SpamSafeFeedbackResponse {
        ok: true,
        recorded: true,
        pattern: feedback.pattern,
    }))
}

/// `POST /internal/twitch/v1/spam-learning/correct`
///
/// „Als harmlos korrigieren": übernimmt die Originalnachricht eines vom Judge
/// gelernten Spam-Musters als exaktes Safe-Pattern und entfernt die Spam-Row
/// anhand ihrer ID (steht in der custom_id des Discord-Buttons).
///
/// ponytail: Der SpamFilter-Cache lädt alle 120s neu — bis dahin kann das
/// gelöschte Muster noch matchen. Bewusst akzeptiert: Korrekturen kommen
/// ohnehin Minuten nach der Aktion; ein Invalidation-Kanal in den Filter
/// lohnt erst, wenn das Fenster real stört.
pub async fn correct_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<SpamCorrectRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    if body.table != "spam" {
        return Err(ApiError::bad_request("table must be spam"));
    }

    let mut tx = pool.begin().await.map_err(|e| {
        tracing::error!("spam-learning correct Transaktion fehlgeschlagen: {e}");
        ApiError::internal()
    })?;
    let source: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT pattern, source_message, source_channel
         FROM twitch_auto_learned_spam_patterns
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(body.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!("spam-learning correct DB-Lese-Fehler: {e}");
        ApiError::internal()
    })?;

    let Some((spam_pattern, source_message, source_channel)) = source else {
        return Err(ApiError::not_found());
    };
    let original = source_message
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| spam_pattern.clone());
    let feedback = normalize_safe_feedback(
        &original,
        Some(MANUAL_SAFE_SOURCE_CHANNEL.to_string()),
        Some("Manuelle Harmlos-Korrektur (Discord)".to_string()),
    )?;
    let review_channel = source_channel
        .filter(|channel| !channel.trim().is_empty())
        .unwrap_or_else(|| MANUAL_SAFE_SOURCE_CHANNEL.to_string());
    insert_safe_pattern(&mut tx, &feedback).await.map_err(|e| {
        tracing::error!("spam-learning correct Safe-Write fehlgeschlagen: {e}");
        ApiError::internal()
    })?;
    insert_clean_review(&mut tx, &review_channel, &feedback)
        .await
        .map_err(|e| {
            tracing::error!("spam-learning correct Audit-Write fehlgeschlagen: {e}");
            ApiError::internal()
        })?;
    let deleted: Option<String> = sqlx::query_scalar(
        "DELETE FROM twitch_auto_learned_spam_patterns WHERE id = $1 RETURNING pattern",
    )
    .bind(body.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!("spam-learning correct DB-Delete-Fehler: {e}");
        ApiError::internal()
    })?;
    tx.commit().await.map_err(|e| {
        tracing::error!("spam-learning correct Commit fehlgeschlagen: {e}");
        ApiError::internal()
    })?;

    match deleted {
        Some(pattern) => {
            tracing::info!(
                id = body.id,
                pattern = %pattern,
                "spam-learning: AI-Spam-Entscheidung per Korrektur-Button als Safe gelernt"
            );
            Ok(Json(SpamCorrectResponse {
                ok: true,
                deleted: true,
                pattern: Some(pattern),
            }))
        }
        None => Err(ApiError::not_found()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SpamCorrectRequest, SpamSafeFeedbackRequest, correct_handler, normalize_pattern,
        normalize_pattern_type, normalize_safe_feedback, safe_handler,
    };
    use axum::{Json, extract::State, response::IntoResponse};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

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

    #[test]
    fn safe_feedback_normalisiert_quelle_und_grund() {
        let feedback = normalize_safe_feedback(
            "  Aha, so sammelt man Viewer  ",
            Some(" #DeuSta ".to_string()),
            None,
        )
        .expect("feedback");
        assert_eq!(feedback.pattern, "aha, so sammelt man viewer");
        assert_eq!(feedback.source_channel, "deusta");
        assert_eq!(feedback.source_message, "Aha, so sammelt man Viewer");
        assert_eq!(feedback.reason, "Manuelle Harmlos-Korrektur (Discord)");
    }

    #[tokio::test]
    async fn safe_handler_persistiert_clean_feedback() {
        let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let schema = format!("spam_safe_feedback_{}", std::process::id());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("DB-Verbindung");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("Schema löschen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("Schema anlegen");
        admin.close().await;

        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(
                PgConnectOptions::from_str(&dsn)
                    .expect("DSN")
                    .options([("search_path", schema.as_str())]),
            )
            .await
            .expect("Testpool");
        sqlx::query(
            "CREATE TABLE twitch_spam_review_decisions (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                chatter_login TEXT NOT NULL,
                chatter_id TEXT,
                source_message TEXT NOT NULL,
                verdict TEXT NOT NULL,
                confidence REAL,
                reason TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&pool)
        .await
        .expect("Review-Tabelle");
        sqlx::query(
            "CREATE TABLE twitch_auto_learned_safe_patterns (
                pattern TEXT PRIMARY KEY,
                source_message TEXT,
                source_channel TEXT,
                minimax_reasoning TEXT,
                hit_count INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&pool)
        .await
        .expect("Safe-Tabelle");

        let response = safe_handler(
            tb_http_core::AuthLevel::Admin,
            State(pool.clone()),
            Json(SpamSafeFeedbackRequest {
                verdict: Some("clean".to_string()),
                pattern: "Aha, so sammelt man Viewer".to_string(),
                source_channel: Some("#DeuSta".to_string()),
                reason: None,
            }),
        )
        .await
        .expect("Safe-Feedback");
        assert_eq!(
            response.into_response().status(),
            axum::http::StatusCode::OK
        );

        let row: (String, String, Option<String>, String, String, String) = sqlx::query_as(
            "SELECT channel_login, chatter_login, chatter_id, source_message, verdict, reason
             FROM twitch_spam_review_decisions",
        )
        .fetch_one(&pool)
        .await
        .expect("Feedback-Zeile");
        assert_eq!(row.0, "deusta");
        assert_eq!(row.1, "discord-moderator");
        assert_eq!(row.2, None);
        assert_eq!(row.3, "aha, so sammelt man viewer");
        assert_eq!(row.4, "clean");
        assert_eq!(row.5, "Manuelle Harmlos-Korrektur (Discord)");

        let safe_row: (String, String, String, String, i32) = sqlx::query_as(
            "SELECT pattern, source_message, source_channel, minimax_reasoning, hit_count
             FROM twitch_auto_learned_safe_patterns",
        )
        .fetch_one(&pool)
        .await
        .expect("Safe-Pattern-Zeile");
        assert_eq!(safe_row.0, "aha, so sammelt man viewer");
        assert_eq!(safe_row.1, "Aha, so sammelt man Viewer");
        assert_eq!(safe_row.2, "discord-correction");
        assert_eq!(safe_row.3, "Manuelle Harmlos-Korrektur (Discord)");
        assert_eq!(safe_row.4, 0);
    }

    #[tokio::test]
    async fn correct_handler_lernt_original_als_safe_und_entfernt_spam() {
        let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let schema = format!("spam_safe_correction_{}", std::process::id());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("DB-Verbindung");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("Schema löschen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("Schema anlegen");
        admin.close().await;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(
                PgConnectOptions::from_str(&dsn)
                    .expect("DSN")
                    .options([("search_path", schema.as_str())]),
            )
            .await
            .expect("Testpool");

        sqlx::query(
            "CREATE TABLE twitch_auto_learned_spam_patterns (
                pattern TEXT PRIMARY KEY,
                pattern_type TEXT NOT NULL DEFAULT 'fragment',
                source_message TEXT,
                source_channel TEXT,
                minimax_reasoning TEXT,
                hit_count INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                id BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE
            )",
        )
        .execute(&pool)
        .await
        .expect("Spam-Tabelle");
        sqlx::query(
            "CREATE TABLE twitch_auto_learned_safe_patterns (
                pattern TEXT PRIMARY KEY,
                source_message TEXT,
                source_channel TEXT,
                minimax_reasoning TEXT,
                hit_count INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&pool)
        .await
        .expect("Safe-Tabelle");
        sqlx::query(
            "CREATE TABLE twitch_spam_review_decisions (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                chatter_login TEXT NOT NULL,
                chatter_id TEXT,
                source_message TEXT NOT NULL,
                verdict TEXT NOT NULL,
                confidence REAL,
                reason TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&pool)
        .await
        .expect("Review-Tabelle");
        sqlx::query(
            "INSERT INTO twitch_auto_learned_spam_patterns
             (pattern, source_message, source_channel, minimax_reasoning)
             VALUES ('eballo.com', '8 viewer brd wild', 'deusta', 'Judge')",
        )
        .execute(&pool)
        .await
        .expect("Spam-Zeile");
        let id: i64 = sqlx::query_scalar(
            "SELECT id FROM twitch_auto_learned_spam_patterns WHERE pattern = 'eballo.com'",
        )
        .fetch_one(&pool)
        .await
        .expect("Spam-ID");

        correct_handler(
            tb_http_core::AuthLevel::Admin,
            State(pool.clone()),
            Json(SpamCorrectRequest {
                table: "spam".to_string(),
                id,
            }),
        )
        .await
        .expect("Korrektur");

        let safe: (String, String, String) = sqlx::query_as(
            "SELECT pattern, source_message, source_channel
             FROM twitch_auto_learned_safe_patterns",
        )
        .fetch_one(&pool)
        .await
        .expect("Safe-Pattern");
        assert_eq!(
            safe,
            (
                "8 viewer brd wild".into(),
                "8 viewer brd wild".into(),
                "discord-correction".into()
            )
        );
        let spam_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_auto_learned_spam_patterns")
                .fetch_one(&pool)
                .await
                .expect("Spam-Anzahl");
        assert_eq!(spam_count, 0);
    }
}

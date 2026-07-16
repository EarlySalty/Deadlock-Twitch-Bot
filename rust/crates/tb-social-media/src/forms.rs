use serde::Deserialize;
use sqlx::PgPool;

use crate::settings::{forms_submit_enabled, get_forms_contact_email};

const USER_AGENT: &str = "Deadlock-Twitch-Bot/1.0";
const VINDICTA_ECLIPSE_URL: &str = "https://docs.google.com/forms/d/e/1FAIpQLSe6Q0nHYVQSSBaAhSyvOeBzI97f0OB3wIJpYuwZ3ZqRsxmH3Q/formResponse";
const DEADLOCK_HIGH_URL: &str = "https://docs.google.com/forms/d/e/1FAIpQLSeVOlCAmjIVr-GPyoq1D0kp5YjUKDF8U9JglWw-5LsaClV05A/formResponse";
const DEADLOCK_PIRATE_URL: &str = "https://docs.google.com/forms/d/e/1FAIpQLSdiyrvA_1vLFJf2CribM3fSi4ww-5oRd5IPOUFeTwVwURsUVQ/formResponse";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormKey {
    VindictaEclipse,
    DeadlockHigh,
    DeadlockPirate,
}

impl FormKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VindictaEclipse => "vindicta_eclipse",
            Self::DeadlockHigh => "deadlock_high",
            Self::DeadlockPirate => "deadlock_pirate",
        }
    }

    const fn url(self) -> &'static str {
        match self {
            Self::VindictaEclipse => VINDICTA_ECLIPSE_URL,
            Self::DeadlockHigh => DEADLOCK_HIGH_URL,
            Self::DeadlockPirate => DEADLOCK_PIRATE_URL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipFormInput {
    pub clip_url: String,
    pub credit: String,
    pub hero: Option<String>,
    pub ai_description: String,
    pub clip_type: Option<String>,
    pub contact_email: String,
}

pub fn build_payload(form: FormKey, input: &ClipFormInput) -> Vec<(String, String)> {
    let field = |entry: &str, value: &str| (entry.to_string(), value.to_string());

    match form {
        FormKey::VindictaEclipse => vec![
            field("entry.1290281111", &input.clip_url),
            field("entry.1554938691", &input.credit),
            field(
                "entry.1264171160",
                "Yes and Vindicta Eclipse has permission to use it",
            ),
            field("entry.1478183538", &input.ai_description),
            field("entry.1585060458", &input.contact_email),
        ],
        FormKey::DeadlockHigh => {
            let clip_type = input
                .clip_type
                .as_deref()
                .filter(|value| matches!(*value, "FUNNY" | "EPIC" | "CLUTCH" | "FAIL"))
                .unwrap_or("EPIC");
            vec![
                field("entry.652511119", &input.contact_email),
                field("entry.1933051763", &input.credit),
                field("entry.284507193", input.hero.as_deref().unwrap_or_default()),
                field("entry.1930240104", &input.ai_description),
                field("entry.1338784444", &input.clip_url),
                field("entry.1950414210", clip_type),
                field(
                    "entry.1123024881",
                    "Yes, and you have my permission to use it on Deadlock HIGH",
                ),
            ]
        }
        FormKey::DeadlockPirate => vec![
            field("entry.1101495589", &input.clip_url),
            field("entry.1736002995", "Emissary / Archon / Oracle"),
            field("entry.1701310762", "0:00"),
            field("entry.292357452", input.hero.as_deref().unwrap_or_default()),
            field("entry.344865364", &input.ai_description),
            field("entry.1690403409", &input.credit),
        ],
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FormsError {
    #[error("Google-Forms-Anfrage fehlgeschlagen: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Google Forms antwortete mit HTTP {0}")]
    HttpStatus(u16),
}

pub async fn submit(
    client: &reqwest::Client,
    form: FormKey,
    input: &ClipFormInput,
) -> Result<u16, FormsError> {
    submit_to(client, form.url(), form, input).await
}

async fn submit_to(
    client: &reqwest::Client,
    url: &str,
    form: FormKey,
    input: &ClipFormInput,
) -> Result<u16, FormsError> {
    let mut payload = build_payload(form, input);
    payload.extend([
        ("fvv".to_string(), "1".to_string()),
        ("pageHistory".to_string(), "0".to_string()),
        ("submit".to_string(), "Submit".to_string()),
    ]);

    let response = match client
        .post(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .form(&payload)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(
                clip_reference = %input.clip_url,
                form_key = form.as_str(),
                http_status = ?Option::<u16>::None,
                outcome = "fail",
                %error,
                "Google-Forms-Submit fehlgeschlagen"
            );
            return Err(FormsError::Request(error));
        }
    };

    let status = response.status();
    let ok = status.is_success() || status.is_redirection();
    if ok {
        tracing::info!(
            clip_reference = %input.clip_url,
            form_key = form.as_str(),
            http_status = status.as_u16(),
            outcome = "ok",
            "Google-Forms-Submit abgeschlossen"
        );
        Ok(status.as_u16())
    } else {
        tracing::error!(
            clip_reference = %input.clip_url,
            form_key = form.as_str(),
            http_status = status.as_u16(),
            outcome = "fail",
            "Google-Forms-Submit abgelehnt"
        );
        Err(FormsError::HttpStatus(status.as_u16()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionAttempt {
    Pending,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormSubmissionOutcome {
    Submitted(u16),
    Failed(Option<u16>),
    SkippedDisabled,
    SkippedDuplicate,
}

pub async fn persist_submission_attempt(
    pool: &PgPool,
    clip_id: i32,
    form: FormKey,
) -> Result<SubmissionAttempt, sqlx::Error> {
    // ponytail: 'pending' blockt Concurrent-Doppel-POST; ein crash-hinterlassenes stale 'pending' bleibt blockiert bis es auf 'failed' geht — Stale-Timeout nachruesten falls Submits haengen.
    let pending = sqlx::query_scalar::<_, i32>(
        "INSERT INTO twitch_clip_form_submissions (clip_id, form_key, status) \
         VALUES ($1, $2, 'pending') \
         ON CONFLICT (clip_id, form_key) DO UPDATE \
         SET status = 'pending', http_status = NULL, error = NULL, submitted_at = NULL \
         WHERE twitch_clip_form_submissions.status = 'failed' \
         RETURNING id",
    )
    .bind(clip_id)
    .bind(form.as_str())
    .fetch_optional(pool)
    .await?
    .is_some();
    let outcome = if pending {
        SubmissionAttempt::Pending
    } else {
        SubmissionAttempt::Skipped
    };
    tracing::info!(
        clip_id,
        form_key = form.as_str(),
        outcome = ?outcome,
        "Google-Forms-Submit-Versuch reserviert"
    );
    Ok(outcome)
}

/// Reserviert und verarbeitet genau einen externen Formular-Submit.
pub async fn submit_clip_form(
    pool: &PgPool,
    client: &reqwest::Client,
    clip_id: i32,
    form: FormKey,
    resolved_title: Option<&str>,
) -> Result<FormSubmissionOutcome, sqlx::Error> {
    submit_clip_form_to(pool, client, clip_id, form, resolved_title, form.url()).await
}

async fn finish_submission(
    pool: &PgPool,
    clip_id: i32,
    form: FormKey,
    status: &str,
    http_status: Option<u16>,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE twitch_clip_form_submissions \
         SET status = $1, http_status = $2, submitted_at = CURRENT_TIMESTAMP, error = $3 \
         WHERE clip_id = $4 AND form_key = $5",
    )
    .bind(status)
    .bind(http_status.map(i32::from))
    .bind(error)
    .bind(clip_id)
    .bind(form.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

async fn submit_clip_form_to(
    pool: &PgPool,
    client: &reqwest::Client,
    clip_id: i32,
    form: FormKey,
    resolved_title: Option<&str>,
    url: &str,
) -> Result<FormSubmissionOutcome, sqlx::Error> {
    // Flag-Check VOR Reservierung: sonst vergiftet eine Disabled-Aktivierung die spätere Idempotenz-Zeile.
    if !forms_submit_enabled(pool).await {
        tracing::info!(
            clip_id,
            form_key = form.as_str(),
            http_status = ?Option::<u16>::None,
            outcome = "skipped_disabled",
            "Formular-Submit übersprungen"
        );
        return Ok(FormSubmissionOutcome::SkippedDisabled);
    }
    if persist_submission_attempt(pool, clip_id, form).await? == SubmissionAttempt::Skipped {
        tracing::info!(
            clip_id,
            form_key = form.as_str(),
            http_status = ?Option::<u16>::None,
            outcome = "skipped_duplicate",
            "Formular-Submit übersprungen"
        );
        return Ok(FormSubmissionOutcome::SkippedDuplicate);
    }

    let clip = sqlx::query_as::<_, (Option<String>, String, Option<String>)>(
        "SELECT clip_url, streamer_login, clip_title \
         FROM twitch_clips_social_media WHERE id = $1",
    )
    .bind(clip_id)
    .fetch_optional(pool)
    .await?;
    let Some((clip_url, streamer_login, clip_title)) = clip else {
        let error = "clip_not_found";
        finish_submission(pool, clip_id, form, "failed", None, Some(error)).await?;
        tracing::error!(
            clip_id,
            form_key = form.as_str(),
            http_status = ?Option::<u16>::None,
            outcome = "failed",
            error,
            "Formular-Submit fehlgeschlagen"
        );
        return Ok(FormSubmissionOutcome::Failed(None));
    };
    let input = ClipFormInput {
        clip_url: clip_url.unwrap_or_default(),
        credit: streamer_login,
        hero: None,
        ai_description: resolved_title
            .map(str::to_owned)
            .or(clip_title)
            .unwrap_or_default(),
        clip_type: None,
        contact_email: get_forms_contact_email(pool).await,
    };

    match submit_to(client, url, form, &input).await {
        Ok(http_status) => {
            finish_submission(pool, clip_id, form, "submitted", Some(http_status), None).await?;
            tracing::info!(
                clip_id,
                form_key = form.as_str(),
                http_status,
                outcome = "submitted",
                "Formular-Submit abgeschlossen"
            );
            Ok(FormSubmissionOutcome::Submitted(http_status))
        }
        Err(error) => {
            let http_status = match &error {
                FormsError::HttpStatus(status) => Some(*status),
                FormsError::Request(_) => None,
            };
            let error_message = error.to_string();
            finish_submission(
                pool,
                clip_id,
                form,
                "failed",
                http_status,
                Some(&error_message),
            )
            .await?;
            tracing::error!(
                clip_id,
                form_key = form.as_str(),
                ?http_status,
                outcome = "failed",
                error = %error_message,
                "Formular-Submit fehlgeschlagen"
            );
            Ok(FormSubmissionOutcome::Failed(http_status))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, str::FromStr};

    use sqlx::{
        postgres::{PgConnectOptions, PgPoolOptions},
        PgPool,
    };
    use wiremock::{
        matchers::{header, method},
        Mock, MockServer, ResponseTemplate,
    };

    use super::*;

    fn input() -> ClipFormInput {
        ClipFormInput {
            clip_url: "https://clips.twitch.tv/example".to_string(),
            credit: "streamer_login".to_string(),
            hero: Some("Vindicta".to_string()),
            ai_description: "A precise airshot".to_string(),
            clip_type: None,
            contact_email: "configured@example.invalid".to_string(),
        }
    }

    fn fields(form: FormKey, input: &ClipFormInput) -> HashMap<String, String> {
        build_payload(form, input).into_iter().collect()
    }

    #[test]
    fn builds_vindicta_eclipse_payload() {
        let fields = fields(FormKey::VindictaEclipse, &input());

        assert_eq!(fields.len(), 5);
        assert_eq!(
            fields["entry.1290281111"],
            "https://clips.twitch.tv/example"
        );
        assert_eq!(fields["entry.1554938691"], "streamer_login");
        assert_eq!(
            fields["entry.1264171160"],
            "Yes and Vindicta Eclipse has permission to use it"
        );
        assert_eq!(fields["entry.1478183538"], "A precise airshot");
        assert_eq!(fields["entry.1585060458"], "configured@example.invalid");
    }

    #[test]
    fn form_key_deserializes_api_values() {
        assert_eq!(
            serde_json::from_str::<FormKey>("\"deadlock_high\"").unwrap(),
            FormKey::DeadlockHigh
        );
    }

    #[test]
    fn builds_deadlock_high_payload_with_default_clip_type() {
        let mut input = input();
        input.hero = None;
        let fields = fields(FormKey::DeadlockHigh, &input);

        assert_eq!(fields.len(), 7);
        assert_eq!(fields["entry.652511119"], "configured@example.invalid");
        assert_eq!(fields["entry.1933051763"], "streamer_login");
        assert_eq!(fields["entry.284507193"], "");
        assert_eq!(fields["entry.1930240104"], "A precise airshot");
        assert_eq!(
            fields["entry.1338784444"],
            "https://clips.twitch.tv/example"
        );
        assert_eq!(fields["entry.1950414210"], "EPIC");
        assert_eq!(
            fields["entry.1123024881"],
            "Yes, and you have my permission to use it on Deadlock HIGH"
        );
    }

    #[test]
    fn replaces_unsupported_deadlock_high_clip_type_with_epic() {
        let mut input = input();
        input.clip_type = Some("OTHER".to_string());

        let fields = fields(FormKey::DeadlockHigh, &input);

        assert_eq!(fields["entry.1950414210"], "EPIC");
    }

    #[test]
    fn builds_deadlock_pirate_loose_fill() {
        let fields = fields(FormKey::DeadlockPirate, &input());

        assert_eq!(fields.len(), 6);
        assert_eq!(
            fields["entry.1101495589"],
            "https://clips.twitch.tv/example"
        );
        assert_eq!(fields["entry.1736002995"], "Emissary / Archon / Oracle");
        assert_eq!(fields["entry.1701310762"], "0:00");
        assert_eq!(fields["entry.292357452"], "Vindicta");
        assert_eq!(fields["entry.344865364"], "A precise airshot");
        assert_eq!(fields["entry.1690403409"], "streamer_login");
    }

    #[tokio::test]
    async fn submit_posts_required_fields_and_accepts_redirect() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("user-agent", USER_AGENT))
            .respond_with(ResponseTemplate::new(302))
            .mount(&server)
            .await;

        let status = submit_to(
            &reqwest::Client::new(),
            &server.uri(),
            FormKey::VindictaEclipse,
            &input(),
        )
        .await
        .unwrap();

        assert_eq!(status, 302);
        let requests = server.received_requests().await.unwrap();
        let fields: HashMap<String, String> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();
        assert_eq!(fields["fvv"], "1");
        assert_eq!(fields["pageHistory"], "0");
        assert_eq!(fields["submit"], "Submit");
        assert_eq!(
            fields["entry.1290281111"],
            "https://clips.twitch.tv/example"
        );
    }

    #[tokio::test]
    async fn submit_rejects_non_success_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let error = submit_to(
            &reqwest::Client::new(),
            &server.uri(),
            FormKey::DeadlockHigh,
            &input(),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, FormsError::HttpStatus(500)));
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

        let options = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(options)
                .await
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn submitted_attempt_is_skipped_and_failed_attempt_is_retried() {
        let Some(pool) = make_pool("t_sm_forms").await else {
            return;
        };
        sqlx::query(
            "CREATE TABLE twitch_clip_form_submissions (\
             id SERIAL PRIMARY KEY, clip_id INTEGER NOT NULL, form_key TEXT NOT NULL, \
             status TEXT NOT NULL DEFAULT 'pending', http_status INTEGER, error TEXT, \
             submitted_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
             UNIQUE (clip_id, form_key))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let first = persist_submission_attempt(&pool, 42, FormKey::DeadlockHigh)
            .await
            .unwrap();
        let in_flight = persist_submission_attempt(&pool, 42, FormKey::DeadlockHigh)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_clip_form_submissions SET status = 'submitted' WHERE clip_id = 42",
        )
        .execute(&pool)
        .await
        .unwrap();
        let second = persist_submission_attempt(&pool, 42, FormKey::DeadlockHigh)
            .await
            .unwrap();

        assert_eq!(first, SubmissionAttempt::Pending);
        assert_eq!(in_flight, SubmissionAttempt::Skipped);
        assert_eq!(second, SubmissionAttempt::Skipped);
        sqlx::query(
            "UPDATE twitch_clip_form_submissions \
             SET status = 'failed', http_status = 500, error = 'rejected', submitted_at = now() \
             WHERE clip_id = 42",
        )
        .execute(&pool)
        .await
        .unwrap();
        let retry = persist_submission_attempt(&pool, 42, FormKey::DeadlockHigh)
            .await
            .unwrap();

        assert_eq!(retry, SubmissionAttempt::Pending);
        let row: (String, String) = sqlx::query_as(
            "SELECT form_key, status FROM twitch_clip_form_submissions WHERE clip_id = 42",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let retry_metadata_cleared: bool = sqlx::query_scalar(
            "SELECT http_status IS NULL AND error IS NULL AND submitted_at IS NULL \
             FROM twitch_clip_form_submissions WHERE clip_id = 42",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, ("deadlock_high".to_string(), "pending".to_string()));
        assert!(retry_metadata_cleared);
    }

    async fn setup_submission_pool(schema: &str) -> Option<PgPool> {
        let pool = make_pool(schema).await?;
        for ddl in [
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB)",
            "CREATE TABLE twitch_clips_social_media (id INTEGER PRIMARY KEY, clip_url TEXT, streamer_login TEXT NOT NULL, clip_title TEXT)",
            "CREATE TABLE twitch_clip_form_submissions (id SERIAL PRIMARY KEY, clip_id INTEGER NOT NULL, form_key TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', http_status INTEGER, error TEXT, submitted_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), UNIQUE (clip_id, form_key))",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO twitch_clips_social_media (id, clip_url, streamer_login, clip_title) VALUES (42, 'https://clips.twitch.tv/example', 'streamer_login', 'DB title')")
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn disabled_submit_reserves_nothing_and_makes_no_request() {
        let Some(pool) = setup_submission_pool("t_sm_forms_disabled").await else {
            return;
        };
        let server = MockServer::start().await;

        let outcome = submit_clip_form_to(
            &pool,
            &reqwest::Client::new(),
            42,
            FormKey::DeadlockHigh,
            Some("Resolved title"),
            &server.uri(),
        )
        .await
        .unwrap();

        assert_eq!(outcome, FormSubmissionOutcome::SkippedDisabled);
        assert!(server.received_requests().await.unwrap().is_empty());
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clip_form_submissions WHERE clip_id = 42",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn enabled_submit_posts_to_mock_and_persists_success() {
        let Some(pool) = setup_submission_pool("t_sm_forms_enabled").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_settings (key, value) VALUES ('forms_submit_enabled', 'true'::jsonb), ('forms_contact_email', '\"forms@example.invalid\"'::jsonb)")
            .execute(&pool)
            .await
            .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let outcome = submit_clip_form_to(
            &pool,
            &reqwest::Client::new(),
            42,
            FormKey::DeadlockHigh,
            Some("Resolved title"),
            &server.uri(),
        )
        .await
        .unwrap();

        assert_eq!(outcome, FormSubmissionOutcome::Submitted(201));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let fields: HashMap<String, String> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();
        assert_eq!(fields["entry.652511119"], "forms@example.invalid");
        assert_eq!(fields["entry.1933051763"], "streamer_login");
        assert_eq!(
            fields["entry.1338784444"],
            "https://clips.twitch.tv/example"
        );
        assert_eq!(fields["entry.284507193"], "");
        assert_eq!(fields["entry.1930240104"], "Resolved title");
        assert_eq!(fields["entry.1950414210"], "EPIC");

        let row: (String, Option<i32>, bool, Option<String>) = sqlx::query_as(
            "SELECT status, http_status, submitted_at IS NOT NULL, error FROM twitch_clip_form_submissions WHERE clip_id = 42",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, ("submitted".to_string(), Some(201), true, None));
    }

    #[tokio::test]
    async fn failed_submit_persists_http_status_and_error() {
        let Some(pool) = setup_submission_pool("t_sm_forms_failed").await else {
            return;
        };
        sqlx::query("INSERT INTO social_media_settings (key, value) VALUES ('forms_submit_enabled', 'true'::jsonb)")
            .execute(&pool)
            .await
            .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let outcome = submit_clip_form_to(
            &pool,
            &reqwest::Client::new(),
            42,
            FormKey::VindictaEclipse,
            Some("Resolved title"),
            &server.uri(),
        )
        .await
        .unwrap();

        assert_eq!(outcome, FormSubmissionOutcome::Failed(Some(500)));
        let row: (String, Option<i32>, bool, bool) = sqlx::query_as(
            "SELECT status, http_status, submitted_at IS NOT NULL, error IS NOT NULL FROM twitch_clip_form_submissions WHERE clip_id = 42",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, ("failed".to_string(), Some(500), true, true));
    }
}

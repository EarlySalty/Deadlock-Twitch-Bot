use std::time::Instant;

use sqlx::PgPool;
use tb_llm::video_observer::{self, ObserverError, VideoObserverConfig};

use super::{source::SourceIdentity, EditorRuntime};

pub(super) const PROMPT_VERSION: &str = "clip-observer-v1";

const PROMPT: &str = "You are watching a Deadlock (Valve's hero shooter) Twitch clip that a viewer or streamer clipped. Watch the whole video with audio.\n\n1. Timeline: describe what happens second by second (mm:ss), including the streamer's own hero, HP, kills, deaths, death cam, parries, stuns, voice lines and chat-worthy reactions.\n2. Why was this clipped? Name the one moment or arc that makes it worth sharing. Remember that the highlight can happen after the streamer dies (death cam, teammates fighting on) and that reactions and voice lines are often the point.\n3. Cut or keep: say whether the clip should be kept in full or trimmed. Only suggest trimming clear dead time that adds nothing to the setup, the highlight or the reaction. If any cut would hurt the setup or payoff, say keep in full.\n4. If trimming makes sense, give exact start and end timestamps to keep.\n\nAnswer in English. Be precise about timestamps and say when you are unsure.";

pub(super) struct ObservationOwner<'a> {
    pub pool: &'a PgPool,
    pub twitch_user_id: &'a str,
    pub job_namespace: String,
}

pub(super) async fn watch(
    owner: ObservationOwner<'_>,
    source: &SourceIdentity,
    runtime: &EditorRuntime,
) -> Option<String> {
    let config = runtime.limits.video_observer.as_ref()?;
    match stored_answer(owner.pool, source, config).await {
        Ok(Some(answer)) => return Some(answer),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(job = %owner.job_namespace, %error, "stored video observation unreadable")
        }
    }
    let started = Instant::now();
    let result = observe(owner.pool, config, source, runtime).await;
    let elapsed_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    if let Err(error) = record(&owner, source, config, &result, elapsed_ms).await {
        tracing::warn!(job = %owner.job_namespace, %error, "video observation not stored");
    }
    match result {
        Ok(answer) => Some(answer),
        Err(error) => {
            tracing::warn!(job = %owner.job_namespace, %error, "video observation unavailable");
            None
        }
    }
}

pub(super) async fn observe(
    pool: &PgPool,
    config: &VideoObserverConfig,
    source: &SourceIdentity,
    runtime: &EditorRuntime,
) -> Result<String, ObserverError> {
    let path = super::source::authorized_path(
        &runtime.limits.source_root,
        &source.path,
        runtime.limits.resources.max_source_bytes,
    )
    .map_err(|_| ObserverError::Source)?;
    let prompt = format!("{PROMPT}\n\nClip title: {}", source.title);
    let passkey = super::browser_passkey::load(pool).await?;
    video_observer::observe(config, &path, &prompt, &passkey).await
}

async fn stored_answer(
    pool: &PgPool,
    source: &SourceIdentity,
    config: &VideoObserverConfig,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT answer FROM public.social_ai_video_observations \
         WHERE source_sha256 = $1 AND observer = $2 AND prompt_version = $3 AND outcome = 'answered' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&source.sha256)
    .bind(config.label())
    .bind(PROMPT_VERSION)
    .fetch_optional(pool)
    .await
}

async fn record(
    owner: &ObservationOwner<'_>,
    source: &SourceIdentity,
    config: &VideoObserverConfig,
    result: &Result<String, ObserverError>,
    elapsed_ms: i64,
) -> Result<(), sqlx::Error> {
    let twitch_user_id: i64 = owner
        .twitch_user_id
        .parse()
        .map_err(|_| sqlx::Error::Protocol("invalid owner".into()))?;
    let (outcome, answer, failure) = match result {
        Ok(answer) => ("answered", Some(answer.as_str()), None),
        Err(error) => ("failed", None, Some(error.to_string())),
    };
    sqlx::query(
        "INSERT INTO public.social_ai_video_observations \
         (twitch_user_id, job_namespace, twitch_clip_id, source_sha256, source_bytes, observer, prompt_version, outcome, failure_code, answer, elapsed_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(twitch_user_id)
    .bind(&owner.job_namespace)
    .bind(&source.twitch_clip_id)
    .bind(&source.sha256)
    .bind(i64::try_from(source.bytes).unwrap_or(i64::MAX))
    .bind(config.label())
    .bind(PROMPT_VERSION)
    .bind(outcome)
    .bind(failure)
    .bind(answer)
    .bind(elapsed_ms)
    .execute(owner.pool)
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::postgres;

    fn config() -> VideoObserverConfig {
        VideoObserverConfig {
            steel_endpoint: "http://127.0.0.1:3000".into(),
            devtools_endpoint: "http://127.0.0.1:9223".into(),
            chat_url: "https://gemini.google.com/app".into(),
            model_item: "4 Argon".into(),
            thinking_menu: "Thinking level".into(),
            thinking_item: "High".into(),
            active_label: "Argon High".into(),
            max_video_bytes: 1024,
            max_answer_bytes: 24_000,
            step_timeout_ms: 45_000,
            upload_timeout_ms: 180_000,
            response_timeout_ms: 420_000,
            total_deadline_ms: 720_000,
        }
    }

    #[tokio::test]
    async fn observations_are_stored_with_their_clip() {
        let db = postgres::TestPostgres::start().await;
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20261024090000_social_ai_video_observations.sql"
        ))
        .execute(&db.pool)
        .await
        .expect("observation migration");
        let source = SourceIdentity {
            twitch_clip_id: "ParryParty".into(),
            path: PathBuf::from("clip.mp4"),
            sha256: "a".repeat(64),
            bytes: 2048,
            login: "earlysalty".into(),
            title: "Parry Party".into(),
        };
        let owner = ObservationOwner {
            pool: &db.pool,
            twitch_user_id: "303",
            job_namespace: "manual:7".into(),
        };
        record(
            &owner,
            &source,
            &config(),
            &Ok("keep in full".into()),
            54_000,
        )
        .await
        .expect("answered row");
        record(
            &owner,
            &source,
            &config(),
            &Err(ObserverError::NotLoggedIn),
            900,
        )
        .await
        .expect("failed row");
        assert_eq!(
            stored_answer(&db.pool, &source, &config())
                .await
                .expect("stored answer")
                .as_deref(),
            Some("keep in full")
        );
        let rows: Vec<(String, Option<String>, Option<String>, String)> = sqlx::query_as(
            "SELECT outcome, answer, failure_code, observer FROM public.social_ai_video_observations WHERE twitch_clip_id = 'ParryParty' ORDER BY id",
        )
        .fetch_all(&db.pool)
        .await
        .expect("rows");
        assert_eq!(
            rows,
            vec![
                (
                    "answered".into(),
                    Some("keep in full".into()),
                    None,
                    "gemini-web:Argon High".into()
                ),
                (
                    "failed".into(),
                    None,
                    Some("not_logged_in".into()),
                    "gemini-web:Argon High".into()
                ),
            ]
        );
    }
}

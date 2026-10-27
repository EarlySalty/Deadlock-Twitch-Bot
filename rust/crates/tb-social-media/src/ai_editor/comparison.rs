use std::time::{Duration, Instant};

use sqlx::{PgPool, Row};
use tb_llm::video_observer::{ObserverError, ObserverVariant, VideoObserverConfig};

use super::{observation, source, EditorRuntime};

const IDLE: Duration = Duration::from_secs(15);
const RATE_LIMIT_PAUSE: Duration = Duration::from_secs(900);

struct Trial {
    id: i64,
    clip_db_id: i64,
    owner: String,
    variant: ObserverVariant,
}

struct TrialOutcome {
    result: Result<String, String>,
    sha256: Option<String>,
    elapsed_ms: i64,
}

pub async fn run_observer_comparisons(pool: PgPool, runtime: EditorRuntime) {
    let Some(base) = runtime.limits.video_observer.clone() else {
        return;
    };
    let variants = runtime.limits.observer_variants.clone();
    if variants.is_empty() {
        return;
    }
    if let Err(error) = recover(&pool).await {
        tracing::warn!(%error, "observer comparison recovery failed");
    }
    loop {
        match step(&pool, &runtime, &base, &variants).await {
            Ok(Some(pause)) => tokio::time::sleep(pause).await,
            Ok(None) => tokio::time::sleep(IDLE).await,
            Err(error) => {
                tracing::warn!(%error, "observer comparison step failed");
                tokio::time::sleep(IDLE).await;
            }
        }
    }
}

async fn step(
    pool: &PgPool,
    runtime: &EditorRuntime,
    base: &VideoObserverConfig,
    variants: &[ObserverVariant],
) -> Result<Option<Duration>, sqlx::Error> {
    if expand(pool, base, variants).await? {
        return Ok(Some(Duration::ZERO));
    }
    let Some(trial) = claim(pool).await? else {
        return Ok(None);
    };
    let outcome = execute(pool, runtime, base, &trial).await;
    let limited = outcome.result.as_ref().err().map(String::as_str)
        == Some(&ObserverError::RateLimited.to_string());
    finish(pool, trial.id, &outcome).await?;
    Ok(Some(if limited {
        RATE_LIMIT_PAUSE
    } else {
        Duration::ZERO
    }))
}

async fn recover(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE public.social_ai_observer_trials SET status = 'queued', started_at = NULL \
         WHERE status = 'running'",
    )
    .execute(pool)
    .await
    .map(|_| ())
}

async fn expand(
    pool: &PgPool,
    base: &VideoObserverConfig,
    variants: &[ObserverVariant],
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM public.social_ai_observer_comparisons WHERE status = 'queued' \
         ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(id) = id else {
        return Ok(false);
    };
    for (position, variant) in variants.iter().enumerate() {
        sqlx::query(
            "INSERT INTO public.social_ai_observer_trials \
             (comparison_id, position, observer, model_item, thinking_item, active_label, prompt_version) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (comparison_id, observer) DO NOTHING",
        )
        .bind(id)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .bind(base.with_variant(variant).label())
        .bind(&variant.model_item)
        .bind(&variant.thinking_item)
        .bind(&variant.active_label)
        .bind(observation::PROMPT_VERSION)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE public.social_ai_observer_comparisons SET status = 'expanded' WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

async fn claim(pool: &PgPool) -> Result<Option<Trial>, sqlx::Error> {
    let row = sqlx::query(
        "WITH next AS ( \
             SELECT t.id FROM public.social_ai_observer_trials t \
             WHERE t.status = 'queued' ORDER BY t.comparison_id, t.position \
             LIMIT 1 FOR UPDATE SKIP LOCKED) \
         UPDATE public.social_ai_observer_trials t SET status = 'running', started_at = NOW() \
         FROM next, public.social_ai_observer_comparisons c \
         WHERE t.id = next.id AND c.id = t.comparison_id \
         RETURNING t.id, c.clip_db_id, c.twitch_user_id, t.model_item, t.thinking_item, t.active_label",
    )
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(Trial {
            id: row.try_get("id")?,
            clip_db_id: row.try_get("clip_db_id")?,
            owner: row.try_get::<i64, _>("twitch_user_id")?.to_string(),
            variant: ObserverVariant {
                model_item: row.try_get("model_item")?,
                thinking_item: row.try_get("thinking_item")?,
                active_label: row.try_get("active_label")?,
            },
        })
    })
    .transpose()
}

async fn execute(
    pool: &PgPool,
    runtime: &EditorRuntime,
    base: &VideoObserverConfig,
    trial: &Trial,
) -> TrialOutcome {
    let started = Instant::now();
    let identity = match identify(pool, runtime, trial).await {
        Ok(identity) => identity,
        Err(()) => {
            return TrialOutcome {
                result: Err(ObserverError::Source.to_string()),
                sha256: None,
                elapsed_ms: elapsed(started),
            }
        }
    };
    let config = base.with_variant(&trial.variant);
    let result = observation::observe(pool, &config, &identity, runtime)
        .await
        .map_err(|error| error.to_string());
    TrialOutcome {
        result,
        sha256: Some(identity.sha256),
        elapsed_ms: elapsed(started),
    }
}

async fn identify(
    pool: &PgPool,
    runtime: &EditorRuntime,
    trial: &Trial,
) -> Result<source::SourceIdentity, ()> {
    let record = source::load_source(pool, runtime, &trial.owner, trial.clip_db_id, None)
        .await
        .map_err(|_| ())?;
    source::SourceIdentity::capture(&record, &runtime.limits.resources)
        .await
        .map_err(|_| ())
}

fn elapsed(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

async fn finish(pool: &PgPool, id: i64, outcome: &TrialOutcome) -> Result<(), sqlx::Error> {
    let (status, answer, failure) = match &outcome.result {
        Ok(answer) => ("answered", Some(answer.as_str()), None),
        Err(code) => ("failed", None, Some(code.as_str())),
    };
    sqlx::query(
        "UPDATE public.social_ai_observer_trials SET status = $2, answer = $3, failure_code = $4, \
         source_sha256 = $5, elapsed_ms = $6, finished_at = NOW() WHERE id = $1 AND status = 'running'",
    )
    .bind(id)
    .bind(status)
    .bind(answer)
    .bind(failure)
    .bind(&outcome.sha256)
    .bind(outcome.elapsed_ms)
    .execute(pool)
    .await
    .map(|_| ())
}

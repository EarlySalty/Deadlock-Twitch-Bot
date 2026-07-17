use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::crew_review::{
    ClaimedModelInputs, ExpiredDiscordGroup, NewReviewEvent, ReviewCycle, ReviewEvent,
    ReviewEventKind, ReviewSession, RickyChatInput, RICKY_TWITCH_USER_ID,
};

const MAX_CONTENT_CHARS: usize = 1_200;
const MAX_ERROR_CLASS_CHARS: usize = 64;
const DEDUPE_INDEX: &str = "twitch_crew_review_events_ricky_source_uidx";
type SessionRow = (Uuid, String, String, DateTime<Utc>, DateTime<Utc>);
const EVENT_COLUMNS: &str = "e.id, e.review_session_id, e.channel_login,
    e.subject_twitch_user_id, e.event_kind, e.source_message_id, e.occurred_at,
    e.content, e.metadata, e.provider, e.model, e.confidence,
    e.discord_message_id, e.discord_deleted_at, e.last_delete_error,
    e.tombstoned_at, e.created_at, e.expires_at";
const SESSION_ACTIVITY_CTE: &str = "WITH session_activity AS (
    SELECT review_session_id,
           MIN(channel_login) AS channel_login,
           MIN(subject_twitch_user_id) AS subject_twitch_user_id,
           MIN(occurred_at) FILTER (WHERE event_kind = 'session_started') AS started_at,
           MAX(occurred_at) FILTER (
               WHERE event_kind = 'ricky_message'
                  OR (event_kind = 'streamer_transcript'
                      AND metadata->'subject_mentioned' = 'true'::jsonb)
                  OR (event_kind = 'ai_decision'
                      AND metadata->'topic_active' = 'true'::jsonb)
           ) AS last_activity_at,
           BOOL_OR(event_kind = 'session_started') AS has_started,
           BOOL_OR(event_kind = 'session_ended') AS has_ended
      FROM twitch_crew_review_events
     GROUP BY review_session_id
)";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("crew review database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("crew review metadata must be an object with a UUID cycle_id")]
    InvalidMetadata,
    #[error("unknown crew review event kind: {0}")]
    InvalidEventKind(String),
    #[error("crew review insert returned no row")]
    MissingInsertedEvent,
    #[error("crew review session does not exist")]
    MissingSession,
    #[error("crew review event does not match its stored session identity")]
    SessionMismatch,
    #[error("crew review event targets a stale session")]
    StaleSession,
    #[error("crew review claim is missing, stale, expired, or incomplete")]
    InvalidClaim,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscordCard {
    pub event_ids: Vec<i64>,
    pub message_id: String,
}

#[derive(Clone)]
pub struct CrewReviewStore {
    pool: PgPool,
}

impl CrewReviewStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record_trigger(&self, input: &RickyChatInput) -> Result<Option<Uuid>, StoreError> {
        if input.subject_twitch_user_id != RICKY_TWITCH_USER_ID {
            return Ok(None);
        }

        let mut transaction = self.pool.begin().await?;
        lock_channel(&mut transaction, &input.channel_login).await?;

        let session_sql = format!(
            "{SESSION_ACTIVITY_CTE}
             SELECT review_session_id
               FROM session_activity
              WHERE channel_login = $1
                AND review_session_id = (
                    SELECT latest.review_session_id
                      FROM twitch_crew_review_events latest
                     WHERE latest.channel_login = $1
                       AND latest.event_kind = 'session_started'
                     ORDER BY latest.id DESC
                     LIMIT 1
                )
                AND has_started
                AND NOT has_ended
                AND last_activity_at > $2 - INTERVAL '10 minutes'
              ORDER BY last_activity_at DESC
              LIMIT 1"
        );
        let session_id: Option<Uuid> = sqlx::query_scalar(&session_sql)
            .bind(&input.channel_login)
            .bind(input.occurred_at)
            .fetch_optional(&mut *transaction)
            .await?;

        let cycle_id = Uuid::new_v4();
        let metadata = json!({"cycle_id": cycle_id.to_string()});
        let session_id = match session_id {
            Some(session_id) => session_id,
            None => {
                let session_id = Uuid::new_v4();
                let started = NewReviewEvent {
                    session_id,
                    channel_login: input.channel_login.clone(),
                    subject_twitch_user_id: input.subject_twitch_user_id.clone(),
                    event_kind: ReviewEventKind::SessionStarted,
                    source_message_id: None,
                    occurred_at: input.occurred_at,
                    content: None,
                    metadata: metadata.clone(),
                    provider: None,
                    model: None,
                    confidence: None,
                };
                insert_event_chunks(&mut transaction, &started).await?;
                session_id
            }
        };

        let message = NewReviewEvent {
            session_id,
            channel_login: input.channel_login.clone(),
            subject_twitch_user_id: input.subject_twitch_user_id.clone(),
            event_kind: ReviewEventKind::RickyMessage,
            source_message_id: input.source_message_id.clone(),
            occurred_at: input.occurred_at,
            content: Some(input.content.clone()),
            metadata,
            provider: None,
            model: None,
            confidence: None,
        };

        match insert_event_chunks(&mut transaction, &message).await {
            Ok(_) => {
                transaction.commit().await?;
                Ok(Some(cycle_id))
            }
            Err(StoreError::Database(error)) if is_duplicate_trigger(&error) => {
                transaction.rollback().await?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn close_channel_session(
        &self,
        channel_login: &str,
        reason: &str,
        occurred_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let mut transaction = self.pool.begin().await?;
        lock_channel(&mut transaction, channel_login).await?;
        let session: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT latest.review_session_id, latest.subject_twitch_user_id
               FROM twitch_crew_review_events latest
              WHERE latest.id = (
                    SELECT started.id
                      FROM twitch_crew_review_events started
                     WHERE started.channel_login = $1
                       AND started.event_kind = 'session_started'
                     ORDER BY started.id DESC
                     LIMIT 1
              )
                AND NOT EXISTS (
                    SELECT 1
                      FROM twitch_crew_review_events ended
                     WHERE ended.review_session_id = latest.review_session_id
                       AND ended.event_kind = 'session_ended'
                )",
        )
        .bind(channel_login)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((session_id, subject_twitch_user_id)) = session else {
            transaction.commit().await?;
            return Ok(false);
        };
        insert_event_chunks(
            &mut transaction,
            &session_ended_event(
                session_id,
                channel_login,
                subject_twitch_user_id,
                reason,
                occurred_at,
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn close_all_open_sessions(
        &self,
        reason: &str,
        occurred_at: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let channels: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT started.channel_login
               FROM twitch_crew_review_events started
              WHERE started.event_kind = 'session_started'
                AND NOT EXISTS (
                    SELECT 1
                      FROM twitch_crew_review_events ended
                     WHERE ended.review_session_id = started.review_session_id
                       AND ended.event_kind = 'session_ended'
                )
              ORDER BY started.channel_login",
        )
        .fetch_all(&mut *transaction)
        .await?;
        for channel in &channels {
            lock_channel(&mut transaction, channel).await?;
        }
        let sessions: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT open.review_session_id, open.channel_login,
                    open.subject_twitch_user_id
               FROM (
                    SELECT DISTINCT ON (started.review_session_id)
                           started.review_session_id, started.channel_login,
                           started.subject_twitch_user_id, started.id
                      FROM twitch_crew_review_events started
                     WHERE started.event_kind = 'session_started'
                       AND started.channel_login = ANY($1::text[])
                       AND NOT EXISTS (
                           SELECT 1
                             FROM twitch_crew_review_events ended
                            WHERE ended.review_session_id = started.review_session_id
                              AND ended.event_kind = 'session_ended'
                       )
                     ORDER BY started.review_session_id, started.id
               ) open
              ORDER BY open.channel_login, open.review_session_id",
        )
        .bind(&channels)
        .fetch_all(&mut *transaction)
        .await?;
        for (session_id, channel_login, subject_twitch_user_id) in &sessions {
            insert_event_chunks(
                &mut transaction,
                &session_ended_event(
                    *session_id,
                    channel_login,
                    subject_twitch_user_id.clone(),
                    reason,
                    occurred_at,
                ),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(sessions.len() as u64)
    }

    pub async fn close_inactive_sessions(
        &self,
        reason: &str,
        occurred_at: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let channels_sql = format!(
            "{SESSION_ACTIVITY_CTE}
             SELECT DISTINCT channel_login
               FROM session_activity
              WHERE has_started
                AND NOT has_ended
                AND last_activity_at <= $1 - INTERVAL '10 minutes'
              ORDER BY channel_login"
        );
        let channels: Vec<String> = sqlx::query_scalar(&channels_sql)
            .bind(occurred_at)
            .fetch_all(&mut *transaction)
            .await?;
        for channel in &channels {
            lock_channel(&mut transaction, channel).await?;
        }

        let sessions_sql = format!(
            "{SESSION_ACTIVITY_CTE}
             SELECT review_session_id, channel_login, subject_twitch_user_id
               FROM session_activity
              WHERE has_started
                AND NOT has_ended
                AND channel_login = ANY($2::text[])
                AND last_activity_at <= $1 - INTERVAL '10 minutes'
              ORDER BY channel_login, review_session_id"
        );
        let sessions: Vec<(Uuid, String, String)> = sqlx::query_as(&sessions_sql)
            .bind(occurred_at)
            .bind(&channels)
            .fetch_all(&mut *transaction)
            .await?;
        for (session_id, channel_login, subject_twitch_user_id) in &sessions {
            insert_event_chunks(
                &mut transaction,
                &session_ended_event(
                    *session_id,
                    channel_login,
                    subject_twitch_user_id.clone(),
                    reason,
                    occurred_at,
                ),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(sessions.len() as u64)
    }

    pub async fn append_event(&self, event: NewReviewEvent) -> Result<i64, StoreError> {
        let cycle_id = cycle_id(&event.metadata)?;
        let mut transaction = self.pool.begin().await?;
        lock_event_session(&mut transaction, &event).await?;
        lock_cycle(&mut transaction, event.session_id, cycle_id).await?;
        if changes_session_state(&event) {
            reject_stale_session(&mut transaction, &event).await?;
        }
        reject_sealed_discord_cycle(&mut transaction, &event, cycle_id).await?;
        if matches!(
            event.event_kind,
            ReviewEventKind::AiDecision | ReviewEventKind::ProviderError
        ) && sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                  FROM twitch_crew_review_events
                 WHERE review_session_id = $1
                   AND metadata->>'cycle_id' = $2
                   AND event_kind IN ('ricky_message', 'streamer_transcript')
            )",
        )
        .bind(event.session_id)
        .bind(cycle_id.to_string())
        .fetch_one(&mut *transaction)
        .await?
        {
            return Err(StoreError::InvalidClaim);
        }
        let first_id = insert_event_chunks(&mut transaction, &event).await?;
        transaction.commit().await?;
        Ok(first_id)
    }

    pub async fn append_claimed_model_event(
        &self,
        claim_id: Uuid,
        event: NewReviewEvent,
    ) -> Result<i64, StoreError> {
        if !matches!(
            event.event_kind,
            ReviewEventKind::AiDecision | ReviewEventKind::AiDraft | ReviewEventKind::ProviderError
        ) {
            return Err(StoreError::InvalidClaim);
        }
        let cycle_id = cycle_id(&event.metadata)?;
        let mut transaction = self.pool.begin().await?;
        lock_event_session(&mut transaction, &event).await?;
        lock_cycle(&mut transaction, event.session_id, cycle_id).await?;
        reject_sealed_discord_cycle(&mut transaction, &event, cycle_id).await?;
        let claim_checks: Vec<bool> = sqlx::query_scalar(
            "SELECT COALESCE(
                       model_claim_id = $3
                       AND model_claim_until > clock_timestamp()
                       AND expires_at > clock_timestamp(),
                       FALSE
                   )
               FROM twitch_crew_review_events
              WHERE review_session_id = $1
                AND metadata->>'cycle_id' = $2
                AND event_kind IN ('ricky_message', 'streamer_transcript')
              FOR UPDATE",
        )
        .bind(event.session_id)
        .bind(cycle_id.to_string())
        .bind(claim_id)
        .fetch_all(&mut *transaction)
        .await?;
        if claim_checks.is_empty() || claim_checks.iter().any(|valid| !valid) {
            return Err(StoreError::InvalidClaim);
        }

        let first_id = insert_event_chunks(&mut transaction, &event).await?;
        if matches!(
            event.event_kind,
            ReviewEventKind::AiDecision | ReviewEventKind::ProviderError
        ) {
            let cleared = sqlx::query(
                "UPDATE twitch_crew_review_events
                    SET model_claim_id = NULL,
                        model_claim_until = NULL
                  WHERE review_session_id = $1
                    AND metadata->>'cycle_id' = $2
                    AND model_claim_id = $3
                    AND model_claim_until > clock_timestamp()
                    AND expires_at > clock_timestamp()",
            )
            .bind(event.session_id)
            .bind(cycle_id.to_string())
            .bind(claim_id)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if cleared != claim_checks.len() as u64 {
                return Err(StoreError::InvalidClaim);
            }
        }
        transaction.commit().await?;
        Ok(first_id)
    }

    pub async fn complete_claimed_model_cycle(
        &self,
        claim_id: Uuid,
        draft: Option<NewReviewEvent>,
        terminal: NewReviewEvent,
    ) -> Result<(), StoreError> {
        if !matches!(
            terminal.event_kind,
            ReviewEventKind::AiDecision | ReviewEventKind::ProviderError
        ) {
            return Err(StoreError::InvalidClaim);
        }
        let terminal_cycle_id = cycle_id(&terminal.metadata)?;
        if let Some(draft) = &draft {
            if draft.event_kind != ReviewEventKind::AiDraft
                || draft.session_id != terminal.session_id
                || draft.channel_login != terminal.channel_login
                || draft.subject_twitch_user_id != terminal.subject_twitch_user_id
                || cycle_id(&draft.metadata)? != terminal_cycle_id
            {
                return Err(StoreError::InvalidClaim);
            }
        }

        let mut transaction = self.pool.begin().await?;
        lock_event_session(&mut transaction, &terminal).await?;
        lock_cycle(&mut transaction, terminal.session_id, terminal_cycle_id).await?;
        reject_sealed_discord_cycle(&mut transaction, &terminal, terminal_cycle_id).await?;
        let claim_checks: Vec<bool> = sqlx::query_scalar(
            "SELECT COALESCE(
                       model_claim_id = $3
                       AND model_claim_until > clock_timestamp()
                       AND expires_at > clock_timestamp(),
                       FALSE
                   )
               FROM twitch_crew_review_events
              WHERE review_session_id = $1
                AND metadata->>'cycle_id' = $2
                AND event_kind IN ('ricky_message', 'streamer_transcript')
              FOR UPDATE",
        )
        .bind(terminal.session_id)
        .bind(terminal_cycle_id.to_string())
        .bind(claim_id)
        .fetch_all(&mut *transaction)
        .await?;
        if claim_checks.is_empty() || claim_checks.iter().any(|valid| !valid) {
            return Err(StoreError::InvalidClaim);
        }
        if let Some(draft) = &draft {
            insert_event_chunks(&mut transaction, draft).await?;
        }
        insert_event_chunks(&mut transaction, &terminal).await?;
        let cleared = sqlx::query(
            "UPDATE twitch_crew_review_events
                SET model_claim_id = NULL,
                    model_claim_until = NULL
              WHERE review_session_id = $1
                AND metadata->>'cycle_id' = $2
                AND model_claim_id = $3
                AND model_claim_until > clock_timestamp()
                AND expires_at > clock_timestamp()",
        )
        .bind(terminal.session_id)
        .bind(terminal_cycle_id.to_string())
        .bind(claim_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if cleared != claim_checks.len() as u64 {
            return Err(StoreError::InvalidClaim);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn active_sessions(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<ReviewSession>, StoreError> {
        let sql = format!(
            "{SESSION_ACTIVITY_CTE}
             SELECT review_session_id,
                    channel_login,
                    subject_twitch_user_id,
                    started_at,
                    last_activity_at
               FROM session_activity
              WHERE has_started
                AND NOT has_ended
                AND review_session_id = (
                    SELECT latest.review_session_id
                      FROM twitch_crew_review_events latest
                     WHERE latest.channel_login = session_activity.channel_login
                       AND latest.event_kind = 'session_started'
                     ORDER BY latest.id DESC
                     LIMIT 1
                )
                AND last_activity_at > $1 - INTERVAL '10 minutes'
              ORDER BY last_activity_at, review_session_id"
        );
        let rows: Vec<SessionRow> = sqlx::query_as(&sql).bind(now).fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    session_id,
                    channel_login,
                    subject_twitch_user_id,
                    started_at,
                    last_activity_at,
                )| {
                    ReviewSession {
                        session_id,
                        channel_login,
                        subject_twitch_user_id,
                        started_at,
                        last_activity_at,
                    }
                },
            )
            .collect())
    }

    pub async fn session_events(&self, session_id: Uuid) -> Result<Vec<ReviewEvent>, StoreError> {
        let sql = format!(
            "SELECT {EVENT_COLUMNS}
               FROM twitch_crew_review_events e
              WHERE e.review_session_id = $1
              ORDER BY e.occurred_at, e.id"
        );
        event_rows(
            sqlx::query_as(&sql)
                .bind(session_id)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn pending_model_inputs(
        &self,
        session_id: Uuid,
    ) -> Result<Option<ClaimedModelInputs>, StoreError> {
        let claim_id = Uuid::new_v4();
        let mut transaction = self.pool.begin().await?;
        let cycle_ids: Vec<String> = sqlx::query_scalar(
            "WITH terminal_cycles AS MATERIALIZED (
                SELECT DISTINCT completed.review_session_id,
                       completed.metadata->>'cycle_id' AS cycle_id
                  FROM twitch_crew_review_events completed
                 WHERE completed.review_session_id = $1
                   AND completed.event_kind IN ('ai_decision', 'provider_error')
                   AND completed.metadata ? 'cycle_id'
                   AND jsonb_typeof(completed.metadata->'cycle_id') = 'string'
                   AND NULLIF(btrim(completed.metadata->>'cycle_id'), '') IS NOT NULL
            )
             SELECT pending.metadata->>'cycle_id'
               FROM twitch_crew_review_events pending
               LEFT JOIN terminal_cycles terminal
                 ON terminal.review_session_id = pending.review_session_id
                AND terminal.cycle_id = pending.metadata->>'cycle_id'
              WHERE pending.review_session_id = $1
                AND pending.event_kind IN ('ricky_message', 'streamer_transcript')
                AND pending.metadata ? 'cycle_id'
                AND terminal.cycle_id IS NULL
              GROUP BY pending.review_session_id, pending.metadata->>'cycle_id'
             HAVING BOOL_AND(
                    pending.expires_at > clock_timestamp() + INTERVAL '5 minutes'
                )
                AND BOOL_AND(
                    pending.model_claim_until IS NULL
                    OR pending.model_claim_until <= clock_timestamp()
                )
              ORDER BY MIN(pending.occurred_at), MIN(pending.id),
                       pending.metadata->>'cycle_id'",
        )
        .bind(session_id)
        .fetch_all(&mut *transaction)
        .await?;
        if cycle_ids.is_empty() {
            transaction.commit().await?;
            return Ok(None);
        }
        let mut lock_ids = cycle_ids.clone();
        lock_ids.sort_unstable();
        for cycle_id in lock_ids {
            lock_cycle(
                &mut transaction,
                session_id,
                Uuid::parse_str(&cycle_id).map_err(|_| StoreError::InvalidMetadata)?,
            )
            .await?;
        }
        let claimed_ids: Vec<i64> = sqlx::query_scalar(
            "WITH eligible_cycles AS (
                WITH terminal_cycles AS MATERIALIZED (
                    SELECT DISTINCT completed.review_session_id,
                           completed.metadata->>'cycle_id' AS cycle_id
                      FROM twitch_crew_review_events completed
                     WHERE completed.review_session_id = $1
                       AND completed.event_kind IN ('ai_decision', 'provider_error')
                       AND completed.metadata ? 'cycle_id'
                       AND jsonb_typeof(completed.metadata->'cycle_id') = 'string'
                       AND NULLIF(btrim(completed.metadata->>'cycle_id'), '') IS NOT NULL
                )
                SELECT pending.review_session_id,
                       pending.metadata->>'cycle_id' AS cycle_id
                  FROM twitch_crew_review_events pending
                  LEFT JOIN terminal_cycles terminal
                    ON terminal.review_session_id = pending.review_session_id
                   AND terminal.cycle_id = pending.metadata->>'cycle_id'
                 WHERE pending.review_session_id = $1
                   AND pending.event_kind IN ('ricky_message', 'streamer_transcript')
                   AND pending.metadata ? 'cycle_id'
                   AND pending.metadata->>'cycle_id' = ANY($2::text[])
                   AND terminal.cycle_id IS NULL
                 GROUP BY pending.review_session_id, pending.metadata->>'cycle_id'
                HAVING BOOL_AND(
                       pending.expires_at > clock_timestamp() + INTERVAL '5 minutes'
                   )
                   AND BOOL_AND(
                       pending.model_claim_until IS NULL
                       OR pending.model_claim_until <= clock_timestamp()
                   )
            ), candidates AS (
                SELECT event.id
                  FROM twitch_crew_review_events event
                  JOIN eligible_cycles eligible
                    ON eligible.review_session_id = event.review_session_id
                   AND eligible.cycle_id = event.metadata->>'cycle_id'
                 WHERE event.event_kind IN ('ricky_message', 'streamer_transcript')
            )
            UPDATE twitch_crew_review_events claimed
               SET model_claim_id = $3,
                   model_claim_until = clock_timestamp() + INTERVAL '5 minutes'
              FROM candidates
             WHERE claimed.id = candidates.id
            RETURNING claimed.id",
        )
        .bind(session_id)
        .bind(&cycle_ids)
        .bind(claim_id)
        .fetch_all(&mut *transaction)
        .await?;
        if claimed_ids.is_empty() {
            transaction.commit().await?;
            return Ok(None);
        }
        let sql = format!(
            "SELECT {EVENT_COLUMNS}
               FROM twitch_crew_review_events e
              WHERE e.model_claim_id = $1
              ORDER BY e.occurred_at, e.id"
        );
        let events = event_rows(
            sqlx::query_as(&sql)
                .bind(claim_id)
                .fetch_all(&mut *transaction)
                .await?,
        )?;
        let claim_until: DateTime<Utc> = sqlx::query_scalar(
            "SELECT MIN(model_claim_until)
               FROM twitch_crew_review_events
              WHERE model_claim_id = $1",
        )
        .bind(claim_id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Some(ClaimedModelInputs {
            claim_id,
            claim_until,
            events,
        }))
    }

    pub async fn pending_discord_cycles(&self, limit: i64) -> Result<Vec<ReviewCycle>, StoreError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let claim_id = Uuid::new_v4();
        let mut transaction = self.pool.begin().await?;
        let cycle_keys: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT pending.review_session_id, pending.metadata->>'cycle_id'
               FROM twitch_crew_review_events pending
              WHERE pending.metadata ? 'cycle_id'
              GROUP BY pending.review_session_id, pending.metadata->>'cycle_id'
             HAVING BOOL_AND(pending.discord_message_id IS NULL)
                AND BOOL_AND(
                    pending.expires_at > clock_timestamp() + INTERVAL '5 minutes'
                )
                AND BOOL_AND(
                    pending.discord_claim_until IS NULL
                    OR pending.discord_claim_until <= clock_timestamp()
                )
                AND BOOL_OR(pending.event_kind IN (
                    'ai_decision', 'provider_error', 'session_ended'
                ))
              ORDER BY MIN(pending.occurred_at), MIN(pending.id),
                       pending.review_session_id, pending.metadata->>'cycle_id'
              LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&mut *transaction)
        .await?;
        if cycle_keys.is_empty() {
            transaction.commit().await?;
            return Ok(Vec::new());
        }
        let mut lock_ids = cycle_keys.clone();
        lock_ids.sort_unstable();
        for (session_id, cycle_id) in lock_ids {
            lock_cycle(
                &mut transaction,
                session_id,
                Uuid::parse_str(&cycle_id).map_err(|_| StoreError::InvalidMetadata)?,
            )
            .await?;
        }
        let (session_ids, cycle_ids): (Vec<Uuid>, Vec<String>) = cycle_keys.into_iter().unzip();
        let claimed_ids: Vec<i64> = sqlx::query_scalar(
            "WITH selected_cycles AS (
                SELECT *
                  FROM UNNEST($1::uuid[], $2::text[])
                       AS selected(review_session_id, cycle_id)
            ), eligible_cycles AS (
                SELECT pending.review_session_id,
                       pending.metadata->>'cycle_id' AS cycle_id
                  FROM twitch_crew_review_events pending
                  JOIN selected_cycles selected
                    ON selected.review_session_id = pending.review_session_id
                   AND selected.cycle_id = pending.metadata->>'cycle_id'
                 WHERE pending.metadata ? 'cycle_id'
                 GROUP BY pending.review_session_id, pending.metadata->>'cycle_id'
                HAVING BOOL_AND(pending.discord_message_id IS NULL)
                   AND BOOL_AND(
                       pending.expires_at > clock_timestamp() + INTERVAL '5 minutes'
                   )
                   AND BOOL_AND(
                       pending.discord_claim_until IS NULL
                       OR pending.discord_claim_until <= clock_timestamp()
                   )
                   AND BOOL_OR(pending.event_kind IN (
                       'ai_decision', 'provider_error', 'session_ended'
                   ))
            ), candidates AS (
                SELECT event.id
                  FROM twitch_crew_review_events event
                  JOIN eligible_cycles eligible
                    ON eligible.review_session_id = event.review_session_id
                   AND eligible.cycle_id = event.metadata->>'cycle_id'
            )
            UPDATE twitch_crew_review_events claimed
               SET discord_claim_id = $3,
                   discord_claim_until = clock_timestamp() + INTERVAL '5 minutes'
              FROM candidates
             WHERE claimed.id = candidates.id
            RETURNING claimed.id",
        )
        .bind(&session_ids)
        .bind(&cycle_ids)
        .bind(claim_id)
        .fetch_all(&mut *transaction)
        .await?;
        if claimed_ids.is_empty() {
            transaction.commit().await?;
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {EVENT_COLUMNS}
              FROM twitch_crew_review_events e
              WHERE e.discord_claim_id = $1
              ORDER BY
                  MIN(e.occurred_at) OVER (
                      PARTITION BY e.review_session_id, e.metadata->>'cycle_id'
                  ),
                  MIN(e.id) OVER (
                      PARTITION BY e.review_session_id, e.metadata->>'cycle_id'
                  ),
                  e.review_session_id,
                  e.metadata->>'cycle_id',
                  e.occurred_at,
                  e.id"
        );
        let events = event_rows(
            sqlx::query_as(&sql)
                .bind(claim_id)
                .fetch_all(&mut *transaction)
                .await?,
        )?;
        let claim_until: DateTime<Utc> = sqlx::query_scalar(
            "SELECT MIN(discord_claim_until)
               FROM twitch_crew_review_events
              WHERE discord_claim_id = $1",
        )
        .bind(claim_id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let mut cycles: Vec<ReviewCycle> = Vec::new();
        for event in events {
            let cycle_id = cycle_id(&event.metadata)?;
            if cycles
                .last()
                .map(|cycle| (cycle.session_id, cycle.cycle_id))
                != Some((event.session_id, cycle_id))
            {
                cycles.push(ReviewCycle {
                    cycle_id,
                    session_id: event.session_id,
                    channel_login: event.channel_login.clone(),
                    claim_id,
                    claim_until,
                    events: Vec::new(),
                });
            }
            let Some(cycle) = cycles.last_mut() else {
                return Err(StoreError::InvalidMetadata);
            };
            cycle.events.push(event);
        }
        Ok(cycles)
    }

    pub async fn mark_discord_sent(
        &self,
        event_ids: &[i64],
        claim_id: Uuid,
        message_id: &str,
    ) -> Result<(), StoreError> {
        if event_ids.is_empty() || message_id.trim().is_empty() {
            return Err(StoreError::InvalidClaim);
        }
        let requested_ids: HashSet<i64> = event_ids.iter().copied().collect();
        if requested_ids.len() != event_ids.len() {
            return Err(StoreError::InvalidClaim);
        }
        let mut transaction = self.pool.begin().await?;
        let requested_rows: Vec<(i64, Uuid, Option<String>)> = sqlx::query_as(
            "SELECT id, review_session_id, metadata->>'cycle_id'
               FROM twitch_crew_review_events
              WHERE id = ANY($1::bigint[])
                AND discord_claim_id = $2
                AND discord_claim_until > clock_timestamp()
                AND expires_at > clock_timestamp()
                AND discord_message_id IS NULL
              FOR UPDATE",
        )
        .bind(event_ids)
        .bind(claim_id)
        .fetch_all(&mut *transaction)
        .await?;
        if requested_rows.len() != requested_ids.len() {
            return Err(StoreError::InvalidClaim);
        }
        let mut cycle_keys: Vec<(Uuid, String)> = requested_rows
            .into_iter()
            .map(|(_, session_id, cycle_id)| {
                cycle_id
                    .map(|cycle_id| (session_id, cycle_id))
                    .ok_or(StoreError::InvalidClaim)
            })
            .collect::<Result<HashSet<_>, _>>()?
            .into_iter()
            .collect();
        cycle_keys.sort_unstable();
        let (session_ids, cycle_ids): (Vec<Uuid>, Vec<String>) = cycle_keys.into_iter().unzip();
        let claimed_ids: HashSet<i64> = sqlx::query_scalar(
            "WITH requested_cycles AS (
                SELECT *
                  FROM UNNEST($2::uuid[], $3::text[])
                       AS requested(review_session_id, cycle_id)
            )
            SELECT event.id
              FROM twitch_crew_review_events event
              JOIN requested_cycles requested
                ON requested.review_session_id = event.review_session_id
               AND requested.cycle_id = event.metadata->>'cycle_id'
             WHERE event.discord_claim_id = $1
               AND event.discord_message_id IS NULL
             FOR UPDATE",
        )
        .bind(claim_id)
        .bind(&session_ids)
        .bind(&cycle_ids)
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .collect();
        if claimed_ids != requested_ids {
            return Err(StoreError::InvalidClaim);
        }
        let updated = sqlx::query(
            "UPDATE twitch_crew_review_events
                SET discord_message_id = $1,
                    discord_deleted_at = NULL,
                    last_delete_error = NULL,
                    discord_claim_id = NULL,
                    discord_claim_until = NULL
              WHERE id = ANY($2::bigint[])
                AND discord_claim_id = $3
                AND discord_claim_until > clock_timestamp()
                AND expires_at > clock_timestamp()
                AND discord_message_id IS NULL",
        )
        .bind(message_id)
        .bind(event_ids)
        .bind(claim_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != requested_ids.len() as u64 {
            transaction.rollback().await?;
            return Err(StoreError::InvalidClaim);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn mark_discord_cards_sent(
        &self,
        cards: &[DiscordCard],
        claim_id: Uuid,
    ) -> Result<(), StoreError> {
        if cards.is_empty()
            || cards
                .iter()
                .any(|card| card.event_ids.is_empty() || card.message_id.trim().is_empty())
        {
            return Err(StoreError::InvalidClaim);
        }
        let event_ids: Vec<i64> = cards
            .iter()
            .flat_map(|card| card.event_ids.iter().copied())
            .collect();
        let requested_ids: HashSet<i64> = event_ids.iter().copied().collect();
        if requested_ids.len() != event_ids.len() {
            return Err(StoreError::InvalidClaim);
        }

        let mut transaction = self.pool.begin().await?;
        let requested_rows: Vec<(i64, Uuid, Option<String>)> = sqlx::query_as(
            "SELECT id, review_session_id, metadata->>'cycle_id'
               FROM twitch_crew_review_events
              WHERE id = ANY($1::bigint[])
                AND discord_claim_id = $2
                AND discord_claim_until > clock_timestamp()
                AND expires_at > clock_timestamp()
                AND discord_message_id IS NULL
              FOR UPDATE",
        )
        .bind(&event_ids)
        .bind(claim_id)
        .fetch_all(&mut *transaction)
        .await?;
        if requested_rows.len() != requested_ids.len() {
            return Err(StoreError::InvalidClaim);
        }
        let cycle_keys: HashSet<(Uuid, String)> = requested_rows
            .into_iter()
            .map(|(_, session_id, cycle_id)| {
                cycle_id
                    .map(|cycle_id| (session_id, cycle_id))
                    .ok_or(StoreError::InvalidClaim)
            })
            .collect::<Result<_, _>>()?;
        if cycle_keys.len() != 1 {
            return Err(StoreError::InvalidClaim);
        }
        let Some((session_id, cycle_id)) = cycle_keys.into_iter().next() else {
            return Err(StoreError::InvalidClaim);
        };
        let claimed_rows: Vec<(i64, bool)> = sqlx::query_as(
            "SELECT id,
                    COALESCE(
                        discord_claim_id = $3
                        AND discord_claim_until > clock_timestamp()
                        AND expires_at > clock_timestamp(),
                        FALSE
                    )
               FROM twitch_crew_review_events
              WHERE review_session_id = $1
                AND metadata->>'cycle_id' = $2
                AND discord_message_id IS NULL
              FOR UPDATE",
        )
        .bind(session_id)
        .bind(&cycle_id)
        .bind(claim_id)
        .fetch_all(&mut *transaction)
        .await?;
        let claimed_ids: HashSet<i64> = claimed_rows.iter().map(|(id, _)| *id).collect();
        if claimed_rows.iter().any(|(_, valid)| !valid) || claimed_ids != requested_ids {
            return Err(StoreError::InvalidClaim);
        }

        for card in cards {
            let updated = sqlx::query(
                "UPDATE twitch_crew_review_events
                    SET discord_message_id = $1,
                        discord_deleted_at = NULL,
                        last_delete_error = NULL,
                        discord_claim_id = NULL,
                        discord_claim_until = NULL
                  WHERE id = ANY($2::bigint[])
                    AND discord_claim_id = $3
                    AND discord_claim_until > clock_timestamp()
                    AND expires_at > clock_timestamp()
                    AND discord_message_id IS NULL",
            )
            .bind(&card.message_id)
            .bind(&card.event_ids)
            .bind(claim_id)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if updated != card.event_ids.len() as u64 {
                return Err(StoreError::InvalidClaim);
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn expired_discord_groups(
        &self,
        _now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ExpiredDiscordGroup>, StoreError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let rows: Vec<(String, Vec<i64>)> = sqlx::query_as(
            "SELECT discord_message_id, ARRAY_AGG(id ORDER BY id)
               FROM twitch_crew_review_events
              WHERE discord_message_id IS NOT NULL
              GROUP BY discord_message_id
             HAVING BOOL_AND(expires_at <= NOW())
                AND BOOL_AND(model_claim_until IS NULL OR model_claim_until <= NOW())
                AND BOOL_AND(discord_claim_until IS NULL OR discord_claim_until <= NOW())
              ORDER BY MIN(expires_at), discord_message_id
              LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(discord_message_id, event_ids)| ExpiredDiscordGroup {
                discord_message_id,
                event_ids,
            })
            .collect())
    }

    pub async fn tombstone_group(
        &self,
        message_id: &str,
        error_class: &str,
    ) -> Result<(), StoreError> {
        let error_class = bounded_error_class(error_class);
        sqlx::query(
            "UPDATE twitch_crew_review_events event
                SET content = NULL,
                    metadata = jsonb_build_object(
                        'error_class', $2::text,
                        'tombstoned_at', NOW()
                    ),
                    provider = NULL,
                    model = NULL,
                    confidence = NULL,
                    last_delete_error = $2,
                    tombstoned_at = NOW()
              WHERE event.discord_message_id = $1
                AND (event.model_claim_until IS NULL OR event.model_claim_until <= NOW())
                AND (event.discord_claim_until IS NULL OR event.discord_claim_until <= NOW())
                AND NOT EXISTS (
                    SELECT 1
                      FROM twitch_crew_review_events fresh
                     WHERE fresh.discord_message_id = event.discord_message_id
                       AND fresh.expires_at > NOW()
                )",
        )
        .bind(message_id)
        .bind(error_class)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_expired_group(&self, message_id: &str) -> Result<u64, StoreError> {
        Ok(sqlx::query(
            "DELETE FROM twitch_crew_review_events expired
              WHERE expired.discord_message_id = $1
                AND expired.expires_at <= NOW()
                AND (
                    expired.model_claim_until IS NULL
                    OR expired.model_claim_until <= NOW()
                )
                AND (
                    expired.discord_claim_until IS NULL
                    OR expired.discord_claim_until <= NOW()
                )
                AND NOT EXISTS (
                    SELECT 1
                      FROM twitch_crew_review_events fresh
                     WHERE fresh.discord_message_id = expired.discord_message_id
                       AND fresh.expires_at > NOW()
                )",
        )
        .bind(message_id)
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    pub async fn delete_expired_unposted(&self, _now: DateTime<Utc>) -> Result<u64, StoreError> {
        Ok(sqlx::query(
            "DELETE FROM twitch_crew_review_events
              WHERE discord_message_id IS NULL
                AND expires_at <= NOW()
                AND (model_claim_until IS NULL OR model_claim_until <= NOW())
                AND (discord_claim_until IS NULL OR discord_claim_until <= NOW())",
        )
        .execute(&self.pool)
        .await?
        .rows_affected())
    }
}

#[derive(sqlx::FromRow)]
struct ReviewEventRow {
    id: i64,
    review_session_id: Uuid,
    channel_login: String,
    subject_twitch_user_id: String,
    event_kind: String,
    source_message_id: Option<String>,
    occurred_at: DateTime<Utc>,
    content: Option<String>,
    metadata: Value,
    provider: Option<String>,
    model: Option<String>,
    confidence: Option<f64>,
    discord_message_id: Option<String>,
    discord_deleted_at: Option<DateTime<Utc>>,
    last_delete_error: Option<String>,
    tombstoned_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl TryFrom<ReviewEventRow> for ReviewEvent {
    type Error = StoreError;

    fn try_from(row: ReviewEventRow) -> Result<Self, Self::Error> {
        let event_kind = ReviewEventKind::from_db(&row.event_kind)
            .ok_or_else(|| StoreError::InvalidEventKind(row.event_kind.clone()))?;
        Ok(Self {
            id: row.id,
            session_id: row.review_session_id,
            channel_login: row.channel_login,
            subject_twitch_user_id: row.subject_twitch_user_id,
            event_kind,
            source_message_id: row.source_message_id,
            occurred_at: row.occurred_at,
            content: row.content,
            metadata: row.metadata,
            provider: row.provider,
            model: row.model,
            confidence: row.confidence,
            discord_message_id: row.discord_message_id,
            discord_deleted_at: row.discord_deleted_at,
            last_delete_error: row.last_delete_error,
            tombstoned_at: row.tombstoned_at,
            created_at: row.created_at,
            expires_at: row.expires_at,
        })
    }
}

fn event_rows(rows: Vec<ReviewEventRow>) -> Result<Vec<ReviewEvent>, StoreError> {
    rows.into_iter().map(ReviewEvent::try_from).collect()
}

fn session_ended_event(
    session_id: Uuid,
    channel_login: &str,
    subject_twitch_user_id: String,
    reason: &str,
    occurred_at: DateTime<Utc>,
) -> NewReviewEvent {
    NewReviewEvent {
        session_id,
        channel_login: channel_login.to_owned(),
        subject_twitch_user_id,
        event_kind: ReviewEventKind::SessionEnded,
        source_message_id: None,
        occurred_at,
        content: None,
        metadata: json!({"cycle_id": Uuid::new_v4().to_string(), "reason": reason}),
        provider: None,
        model: None,
        confidence: None,
    }
}

async fn lock_channel(
    transaction: &mut Transaction<'_, Postgres>,
    channel_login: &str,
) -> Result<(), StoreError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(channel_login)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn lock_cycle(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
    cycle_id: Uuid,
) -> Result<(), StoreError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1 || ':' || $2, 1))")
        .bind(session_id.to_string())
        .bind(cycle_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn lock_event_session(
    transaction: &mut Transaction<'_, Postgres>,
    event: &NewReviewEvent,
) -> Result<(), StoreError> {
    let identity: Option<(String, String)> = sqlx::query_as(
        "SELECT channel_login, subject_twitch_user_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
          ORDER BY id
          LIMIT 1",
    )
    .bind(event.session_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((channel_login, subject_twitch_user_id)) = identity else {
        return Err(StoreError::MissingSession);
    };
    lock_channel(transaction, &channel_login).await?;
    if event.channel_login != channel_login
        || event.subject_twitch_user_id != subject_twitch_user_id
    {
        return Err(StoreError::SessionMismatch);
    }
    let identity_is_consistent: bool = sqlx::query_scalar(
        "SELECT BOOL_AND(channel_login = $2 AND subject_twitch_user_id = $3)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1",
    )
    .bind(event.session_id)
    .bind(&channel_login)
    .bind(&subject_twitch_user_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !identity_is_consistent {
        return Err(StoreError::SessionMismatch);
    }
    Ok(())
}

fn changes_session_state(event: &NewReviewEvent) -> bool {
    match event.event_kind {
        ReviewEventKind::SessionStarted
        | ReviewEventKind::RickyMessage
        | ReviewEventKind::SessionEnded => true,
        ReviewEventKind::StreamerTranscript => {
            event.metadata.get("subject_mentioned") == Some(&Value::Bool(true))
        }
        ReviewEventKind::AiDecision => {
            event.metadata.get("topic_active") == Some(&Value::Bool(true))
        }
        ReviewEventKind::AiDraft | ReviewEventKind::ProviderError => false,
    }
}

async fn reject_stale_session(
    transaction: &mut Transaction<'_, Postgres>,
    event: &NewReviewEvent,
) -> Result<(), StoreError> {
    let current_session: Option<Uuid> = sqlx::query_scalar(
        "SELECT latest.review_session_id
           FROM (
               SELECT review_session_id
                 FROM twitch_crew_review_events
                WHERE channel_login = $1
                  AND event_kind = 'session_started'
                ORDER BY id DESC
                LIMIT 1
           ) latest
          WHERE NOT EXISTS (
                SELECT 1
                  FROM twitch_crew_review_events ended
                 WHERE ended.review_session_id = latest.review_session_id
                   AND ended.event_kind = 'session_ended'
          )",
    )
    .bind(&event.channel_login)
    .fetch_optional(&mut **transaction)
    .await?;
    if current_session != Some(event.session_id) {
        return Err(StoreError::StaleSession);
    }
    Ok(())
}

async fn reject_sealed_discord_cycle(
    transaction: &mut Transaction<'_, Postgres>,
    event: &NewReviewEvent,
    cycle_id: Uuid,
) -> Result<(), StoreError> {
    let sealed: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1
              FROM twitch_crew_review_events
             WHERE review_session_id = $1
               AND metadata->>'cycle_id' = $2
               AND (
                   discord_message_id IS NOT NULL
                   OR (
                       discord_claim_id IS NOT NULL
                       AND discord_claim_until > clock_timestamp()
                   )
               )
        )",
    )
    .bind(event.session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    if sealed {
        return Err(StoreError::InvalidClaim);
    }
    Ok(())
}

async fn insert_event_chunks(
    transaction: &mut Transaction<'_, Postgres>,
    event: &NewReviewEvent,
) -> Result<i64, StoreError> {
    if matches!(
        event.event_kind,
        ReviewEventKind::AiDecision | ReviewEventKind::ProviderError
    ) && event
        .content
        .as_deref()
        .is_some_and(|content| content.chars().count() > MAX_CONTENT_CHARS)
    {
        return Err(StoreError::InvalidClaim);
    }
    validate_metadata(&event.metadata)?;
    let chunks = content_chunks(event.content.as_deref());
    let chunk_count = chunks.len();
    let mut first_id = None;

    for (chunk_index, content) in chunks.into_iter().enumerate() {
        let mut metadata = event.metadata.clone();
        if chunk_count > 1 {
            let Some(object) = metadata.as_object_mut() else {
                return Err(StoreError::InvalidMetadata);
            };
            object.insert("chunk_index".to_owned(), json!(chunk_index));
            object.insert("chunk_count".to_owned(), json!(chunk_count));
        }
        let source_message_id = if chunk_index == 0 {
            event.source_message_id.as_deref()
        } else {
            None
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_crew_review_events (
                review_session_id, channel_login, subject_twitch_user_id,
                event_kind, source_message_id, occurred_at, content, metadata,
                provider, model, confidence, expires_at
             ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                $6 + INTERVAL '6 months'
             )
             RETURNING id",
        )
        .bind(event.session_id)
        .bind(&event.channel_login)
        .bind(&event.subject_twitch_user_id)
        .bind(event.event_kind.as_str())
        .bind(source_message_id)
        .bind(event.occurred_at)
        .bind(content)
        .bind(metadata)
        .bind(&event.provider)
        .bind(&event.model)
        .bind(event.confidence)
        .fetch_one(&mut **transaction)
        .await?;
        first_id.get_or_insert(id);
    }

    first_id.ok_or(StoreError::MissingInsertedEvent)
}

fn validate_metadata(metadata: &Value) -> Result<(), StoreError> {
    let Some(object) = metadata.as_object() else {
        return Err(StoreError::InvalidMetadata);
    };
    let Some(cycle_id) = object.get("cycle_id").and_then(Value::as_str) else {
        return Err(StoreError::InvalidMetadata);
    };
    Uuid::parse_str(cycle_id)
        .map(|_| ())
        .map_err(|_| StoreError::InvalidMetadata)
}

fn cycle_id(metadata: &Value) -> Result<Uuid, StoreError> {
    metadata
        .get("cycle_id")
        .and_then(Value::as_str)
        .ok_or(StoreError::InvalidMetadata)
        .and_then(|value| Uuid::parse_str(value).map_err(|_| StoreError::InvalidMetadata))
}

fn content_chunks(content: Option<&str>) -> Vec<Option<String>> {
    let Some(content) = content else {
        return vec![None];
    };
    if content.chars().count() <= MAX_CONTENT_CHARS {
        return vec![Some(content.to_owned())];
    }

    let mut remaining = content.trim();
    let mut chunks = Vec::new();
    while remaining.chars().count() > MAX_CONTENT_CHARS {
        let hard_end = remaining
            .char_indices()
            .nth(MAX_CONTENT_CHARS)
            .map_or(remaining.len(), |(index, _)| index);
        let split_at = remaining[..hard_end]
            .char_indices()
            .rev()
            .find_map(|(index, character)| character.is_whitespace().then_some(index))
            .filter(|index| *index > 0)
            .unwrap_or(hard_end);
        chunks.push(Some(remaining[..split_at].trim_end().to_owned()));
        remaining = remaining[split_at..].trim_start();
    }
    if !remaining.is_empty() {
        chunks.push(Some(remaining.to_owned()));
    }
    chunks
}

fn bounded_error_class(error_class: &str) -> String {
    let bounded: String = error_class
        .trim()
        .chars()
        .take(MAX_ERROR_CLASS_CHARS)
        .collect();
    if bounded.is_empty() {
        "unknown".to_owned()
    } else {
        bounded
    }
}

fn is_duplicate_trigger(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database_error| database_error.constraint() == Some(DEDUPE_INDEX))
}

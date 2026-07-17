use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::crew_review::{
    ExpiredDiscordGroup, NewReviewEvent, ReviewCycle, ReviewEvent, ReviewEventKind, ReviewSession,
    RickyChatInput, RICKY_TWITCH_USER_ID,
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
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&input.channel_login)
            .execute(&mut *transaction)
            .await?;

        let session_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT review_session_id
               FROM twitch_crew_review_events
              WHERE channel_login = $1
              GROUP BY review_session_id
             HAVING BOOL_OR(event_kind = 'session_started')
                AND NOT BOOL_OR(event_kind = 'session_ended')
                AND MAX(occurred_at) FILTER (WHERE event_kind = 'ricky_message')
                    >= $2 - INTERVAL '10 minutes'
              ORDER BY MAX(occurred_at) FILTER (WHERE event_kind = 'ricky_message') DESC
              LIMIT 1",
        )
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

    pub async fn append_event(&self, event: NewReviewEvent) -> Result<i64, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let first_id = insert_event_chunks(&mut transaction, &event).await?;
        transaction.commit().await?;
        Ok(first_id)
    }

    pub async fn active_sessions(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<ReviewSession>, StoreError> {
        let rows: Vec<SessionRow> = sqlx::query_as(
            "SELECT review_session_id,
                        MIN(channel_login),
                        MIN(subject_twitch_user_id),
                        MIN(occurred_at) FILTER (WHERE event_kind = 'session_started'),
                        MAX(occurred_at) FILTER (WHERE event_kind = 'ricky_message')
                   FROM twitch_crew_review_events
                  GROUP BY review_session_id
                 HAVING BOOL_OR(event_kind = 'session_started')
                    AND NOT BOOL_OR(event_kind = 'session_ended')
                    AND MAX(occurred_at) FILTER (WHERE event_kind = 'ricky_message')
                        >= $1 - INTERVAL '10 minutes'
                  ORDER BY MAX(occurred_at) FILTER (WHERE event_kind = 'ricky_message'),
                           review_session_id",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;

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

    pub async fn pending_model_inputs(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ReviewEvent>, StoreError> {
        let sql = format!(
            "SELECT {EVENT_COLUMNS}
               FROM twitch_crew_review_events e
              WHERE e.review_session_id = $1
                AND e.event_kind IN ('ricky_message', 'streamer_transcript')
                AND e.metadata ? 'cycle_id'
                AND NOT EXISTS (
                    SELECT 1
                      FROM twitch_crew_review_events completed
                     WHERE completed.metadata->>'cycle_id' = e.metadata->>'cycle_id'
                       AND completed.event_kind IN ('ai_decision', 'provider_error')
                )
              ORDER BY e.occurred_at, e.id"
        );
        event_rows(
            sqlx::query_as(&sql)
                .bind(session_id)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn pending_discord_cycles(&self, limit: i64) -> Result<Vec<ReviewCycle>, StoreError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let sql = format!(
            "WITH pending_cycles AS (
                SELECT pending.metadata->>'cycle_id' AS cycle_id,
                       MIN(pending.occurred_at) AS first_at,
                       MIN(pending.id) AS first_id
                  FROM twitch_crew_review_events pending
                 WHERE pending.discord_message_id IS NULL
                   AND pending.metadata ? 'cycle_id'
                   AND EXISTS (
                       SELECT 1
                         FROM twitch_crew_review_events terminal
                        WHERE terminal.metadata->>'cycle_id' = pending.metadata->>'cycle_id'
                          AND terminal.event_kind IN (
                              'ai_decision', 'provider_error', 'session_ended'
                          )
                   )
                 GROUP BY pending.metadata->>'cycle_id'
                 ORDER BY MIN(pending.occurred_at), MIN(pending.id)
                 LIMIT $1
            )
            SELECT {EVENT_COLUMNS}
              FROM twitch_crew_review_events e
              JOIN pending_cycles pending
                ON pending.cycle_id = e.metadata->>'cycle_id'
             WHERE e.discord_message_id IS NULL
             ORDER BY pending.first_at, pending.first_id, e.occurred_at, e.id"
        );
        let events = event_rows(
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?,
        )?;
        let mut cycles: Vec<ReviewCycle> = Vec::new();
        for event in events {
            let cycle_id = cycle_id(&event.metadata)?;
            if cycles.last().map(|cycle| cycle.cycle_id) != Some(cycle_id) {
                cycles.push(ReviewCycle {
                    cycle_id,
                    session_id: event.session_id,
                    channel_login: event.channel_login.clone(),
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
        message_id: &str,
    ) -> Result<(), StoreError> {
        if event_ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE twitch_crew_review_events
                SET discord_message_id = $1,
                    discord_deleted_at = NULL,
                    last_delete_error = NULL
              WHERE id = ANY($2::bigint[])",
        )
        .bind(message_id)
        .bind(event_ids)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn expired_discord_groups(
        &self,
        now: DateTime<Utc>,
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
             HAVING BOOL_AND(expires_at <= $1)
              ORDER BY MIN(expires_at), discord_message_id
              LIMIT $2",
        )
        .bind(now)
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

    pub async fn delete_expired_unposted(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        Ok(sqlx::query(
            "DELETE FROM twitch_crew_review_events
              WHERE discord_message_id IS NULL
                AND expires_at <= $1",
        )
        .bind(now)
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

async fn insert_event_chunks(
    transaction: &mut Transaction<'_, Postgres>,
    event: &NewReviewEvent,
) -> Result<i64, StoreError> {
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

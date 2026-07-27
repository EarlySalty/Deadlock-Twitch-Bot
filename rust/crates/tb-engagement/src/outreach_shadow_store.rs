use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::outreach_shadow::{
    NewOutreachEvent, OutreachDecision, OutreachModelInput, OutreachOutcome, OutreachSession,
    OutreachStage, SessionEvidence, TimestampedText,
};

const SESSION_LIMIT: Duration = Duration::minutes(45);
const CHANNEL_COOLDOWN: Duration = Duration::hours(24);
const PROCESSOR_CLAIM_TTL: Duration = Duration::minutes(5);
const DISCORD_CLAIM_TTL: Duration = Duration::minutes(2);
const DISCORD_RETRY_DELAY: Duration = Duration::seconds(30);
const DISCORD_DELETE_RETRY_DELAY: Duration = Duration::hours(1);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database")]
    Database(#[from] sqlx::Error),
    #[error("invalid_data")]
    InvalidData,
}

#[derive(Clone)]
pub struct OutreachShadowStore {
    pool: PgPool,
}

#[derive(Clone, Debug)]
pub struct ClaimedOutreachEvent {
    pub claim_id: Uuid,
    pub event: OutreachEvent,
}

#[derive(Clone, Debug)]
pub struct ClaimedOutreachSession {
    pub claim_id: Uuid,
    pub cycle_id: Uuid,
    pub session: OutreachSession,
}

#[derive(Clone, Debug)]
pub struct ExpiredDiscordEvent {
    pub id: i64,
    pub message_id: String,
}

#[derive(Clone, Debug)]
pub struct OutreachEvent {
    pub id: i64,
    pub session: OutreachSession,
    pub cycle_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub outcome: OutreachOutcome,
    pub transcript: Option<String>,
    pub decision: Option<OutreachDecision>,
    pub static_recruitment_text: Option<String>,
    pub error_class: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LoadedOutreachContext {
    pub input: OutreachModelInput,
    pub evidence: SessionEvidence,
    pub raid_count: i64,
}

#[derive(Clone, Debug)]
struct OutreachCandidate {
    channel_login: String,
    streamer_user_id: String,
    is_live: bool,
    game: Option<String>,
    cooldown_until: Option<DateTime<Utc>>,
    partner_table: bool,
    partner_state: bool,
    last_observed_at: Option<DateTime<Utc>>,
    detected_at: String,
}

#[derive(sqlx::FromRow)]
struct CandidateRow {
    channel_login: String,
    streamer_user_id: String,
    is_live: bool,
    game: Option<String>,
    cooldown_until: Option<String>,
    partner_table: bool,
    partner_state: bool,
    last_observed_at: Option<DateTime<Utc>>,
    detected_at: String,
}

impl OutreachShadowStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn start_next_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<OutreachSession>, StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_global(&mut tx).await?;
        sqlx::query("LOCK TABLE twitch_partners IN SHARE MODE")
            .execute(&mut *tx)
            .await?;
        let open: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM twitch_outreach_shadow_sessions WHERE ended_at IS NULL
            )",
        )
        .fetch_one(&mut *tx)
        .await?;
        if open {
            tx.commit().await?;
            return Ok(None);
        }

        let rows = sqlx::query_as::<_, CandidateRow>(
            "SELECT LOWER(BTRIM(o.streamer_login)) AS channel_login,
                    COALESCE(
                        NULLIF(BTRIM(o.streamer_user_id), ''),
                        ls.twitch_user_id
                    ) AS streamer_user_id,
                    COALESCE(ls.is_live, 0) <> 0 AS is_live,
                    ls.last_game AS game,
                    NULLIF(BTRIM(o.cooldown_until), '') AS cooldown_until,
                    EXISTS (
                        SELECT 1
                        FROM twitch_partners p
                        WHERE p.status = 'active'
                          AND (
                            p.twitch_user_id = COALESCE(
                                NULLIF(BTRIM(o.streamer_user_id), ''),
                                ls.twitch_user_id
                            )
                            OR LOWER(p.twitch_login) = LOWER(o.streamer_login)
                          )
                    ) AS partner_table,
                    EXISTS (
                        SELECT 1
                        FROM twitch_streamers_partner_state ps
                        WHERE COALESCE(ps.is_partner_active, 0) <> 0
                          AND (
                            ps.twitch_user_id = COALESCE(
                                NULLIF(BTRIM(o.streamer_user_id), ''),
                                ls.twitch_user_id
                            )
                            OR LOWER(ps.twitch_login) = LOWER(o.streamer_login)
                          )
                    ) AS partner_state,
                    (
                        SELECT MAX(s.started_at)
                        FROM twitch_outreach_shadow_sessions s
                        WHERE LOWER(s.channel_login) = LOWER(o.streamer_login)
                    ) AS last_observed_at,
                    o.detected_at
             FROM twitch_partner_outreach o
             JOIN twitch_live_state ls
               ON LOWER(ls.streamer_login) = LOWER(o.streamer_login)
             WHERE COALESCE(
                    NULLIF(BTRIM(o.streamer_user_id), ''),
                    NULLIF(BTRIM(ls.twitch_user_id), '')
               ) IS NOT NULL
             FOR UPDATE OF o SKIP LOCKED
             ",
        )
        .fetch_all(&mut *tx)
        .await?;

        let candidates = rows
            .into_iter()
            .map(|row| OutreachCandidate {
                channel_login: row.channel_login,
                streamer_user_id: row.streamer_user_id,
                is_live: row.is_live,
                game: row.game,
                cooldown_until: row.cooldown_until.map(|value| {
                    DateTime::parse_from_rfc3339(&value)
                        .map(|parsed| parsed.with_timezone(&Utc))
                        .unwrap_or(now + Duration::days(36_500))
                }),
                partner_table: row.partner_table,
                partner_state: row.partner_state,
                last_observed_at: row.last_observed_at,
                detected_at: row.detected_at,
            })
            .collect();
        let Some(candidate) = choose_candidate(false, candidates, now) else {
            tx.commit().await?;
            return Ok(None);
        };
        let session = OutreachSession {
            id: Uuid::new_v4(),
            channel_login: candidate.channel_login,
            streamer_user_id: candidate.streamer_user_id,
            started_at: now,
            stage: OutreachStage::Watch,
        };
        sqlx::query(
            "INSERT INTO twitch_outreach_shadow_sessions
                (id, channel_login, streamer_user_id, started_at, stage)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(session.id)
        .bind(&session.channel_login)
        .bind(&session.streamer_user_id)
        .bind(session.started_at)
        .bind(session.stage.as_str())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(session))
    }

    pub async fn active_session(&self) -> Result<Option<OutreachSession>, StoreError> {
        sqlx::query_as::<_, (Uuid, String, String, DateTime<Utc>, String)>(
            "SELECT id, channel_login, streamer_user_id, started_at, stage
             FROM twitch_outreach_shadow_sessions
             WHERE ended_at IS NULL
             ORDER BY started_at
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .map(session_from_row)
        .transpose()
    }

    pub async fn claim_active_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimedOutreachSession>, StoreError> {
        let claim_id = Uuid::new_v4();
        let new_cycle_id = Uuid::new_v4();
        let row = sqlx::query_as::<_, (Uuid, String, String, DateTime<Utc>, String, Uuid)>(
            "UPDATE twitch_outreach_shadow_sessions s
             SET processor_claim_id = $1,
                 processor_claim_until = $2,
                 current_cycle_id = COALESCE(s.current_cycle_id, $3)
             WHERE s.id = (
                 SELECT candidate.id
                 FROM twitch_outreach_shadow_sessions candidate
                 WHERE candidate.ended_at IS NULL
                   AND (
                       candidate.processor_claim_until IS NULL
                       OR candidate.processor_claim_until <= $4
                   )
                   AND EXISTS (
                       SELECT 1
                       FROM twitch_live_state ls
                       WHERE ls.twitch_user_id = candidate.streamer_user_id
                         AND COALESCE(ls.is_live, 0) = 1
                         AND LOWER(COALESCE(ls.last_game, '')) = 'deadlock'
                   )
                   AND NOT EXISTS (
                       SELECT 1
                       FROM twitch_partners p
                       WHERE p.status = 'active'
                         AND (
                             p.twitch_user_id = candidate.streamer_user_id
                             OR LOWER(p.twitch_login) = LOWER(candidate.channel_login)
                         )
                   )
                   AND NOT EXISTS (
                       SELECT 1
                       FROM twitch_streamers_partner_state ps
                       WHERE COALESCE(ps.is_partner_active, 0) <> 0
                         AND (
                             ps.twitch_user_id = candidate.streamer_user_id
                             OR LOWER(ps.twitch_login) = LOWER(candidate.channel_login)
                         )
                   )
                 ORDER BY candidate.started_at
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING s.id, s.channel_login, s.streamer_user_id, s.started_at,
                       s.stage, s.current_cycle_id",
        )
        .bind(claim_id)
        .bind(now + PROCESSOR_CLAIM_TTL)
        .bind(new_cycle_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        row.map(
            |(id, channel_login, streamer_user_id, started_at, stage, cycle_id)| {
                Ok(ClaimedOutreachSession {
                    claim_id,
                    cycle_id,
                    session: OutreachSession {
                        id,
                        channel_login,
                        streamer_user_id,
                        started_at,
                        stage: parse_stage(&stage)?,
                    },
                })
            },
        )
        .transpose()
    }

    pub async fn release_processor_claim(
        &self,
        session_id: Uuid,
        claim_id: Uuid,
        complete_cycle: bool,
    ) -> Result<(), StoreError> {
        let updated = sqlx::query(
            "UPDATE twitch_outreach_shadow_sessions
             SET processor_claim_id = NULL,
                 processor_claim_until = NULL,
                 current_cycle_id = CASE WHEN $1 THEN NULL ELSE current_cycle_id END
             WHERE id = $2 AND processor_claim_id = $3",
        )
        .bind(complete_cycle)
        .bind(session_id)
        .bind(claim_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 1 {
            Ok(())
        } else {
            Err(StoreError::InvalidData)
        }
    }

    pub async fn close_ineligible_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<&'static str>, StoreError> {
        let Some(session) = self.active_session().await? else {
            return Ok(None);
        };
        let (live_deadlock, partner): (bool, bool) = sqlx::query_as(
            "SELECT EXISTS (
                SELECT 1
                FROM twitch_live_state
                WHERE twitch_user_id = $1
                  AND COALESCE(is_live, 0) = 1
                  AND LOWER(COALESCE(last_game, '')) = 'deadlock'
            ),
            EXISTS (
                SELECT 1
                FROM twitch_partners p
                WHERE p.status = 'active'
                  AND (
                      p.twitch_user_id = $1
                      OR LOWER(p.twitch_login) = LOWER($2)
                  )
            ) OR EXISTS (
                SELECT 1
                FROM twitch_streamers_partner_state ps
                WHERE COALESCE(ps.is_partner_active, 0) <> 0
                  AND (
                      ps.twitch_user_id = $1
                      OR LOWER(ps.twitch_login) = LOWER($2)
                  )
            )",
        )
        .bind(&session.streamer_user_id)
        .bind(&session.channel_login)
        .fetch_one(&self.pool)
        .await?;
        let reason = if now - session.started_at >= SESSION_LIMIT {
            Some("session_timeout")
        } else if partner {
            Some("became_partner")
        } else if !live_deadlock {
            Some("stream_ended")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.close_session(session.id, reason, now).await?;
        }
        Ok(reason)
    }

    pub async fn close_all_open_sessions(
        &self,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_global(&mut tx).await?;
        let logins = sqlx::query_scalar::<_, String>(
            "UPDATE twitch_outreach_shadow_sessions
             SET ended_at = $1, end_reason = $2,
                 processor_claim_id = NULL, processor_claim_until = NULL
             WHERE ended_at IS NULL
             RETURNING channel_login",
        )
        .bind(now)
        .bind(reason)
        .fetch_all(&mut *tx)
        .await?;
        apply_cooldown(&mut tx, &logins, now).await?;
        tx.commit().await?;
        Ok(logins.len() as u64)
    }

    pub async fn close_active_session(
        &self,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        if let Some(session) = self.active_session().await? {
            self.close_session(session.id, reason, now).await?;
        }
        Ok(())
    }

    async fn close_session(
        &self,
        session_id: Uuid,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_global(&mut tx).await?;
        let login = sqlx::query_scalar::<_, String>(
            "UPDATE twitch_outreach_shadow_sessions
             SET ended_at = $1, end_reason = $2,
                 processor_claim_id = NULL, processor_claim_until = NULL
             WHERE id = $3 AND ended_at IS NULL
             RETURNING channel_login",
        )
        .bind(now)
        .bind(reason)
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(login) = login {
            apply_cooldown(&mut tx, &[login], now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn record_cycle(&self, event: &NewOutreachEvent) -> Result<bool, StoreError> {
        let decision = event
            .decision
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| StoreError::InvalidData)?;
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            "INSERT INTO twitch_outreach_shadow_events
                (session_id, cycle_id, channel_login, occurred_at, outcome, stage,
                 transcript, decision, static_recruitment_text, error_class, provider, model)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (cycle_id) DO NOTHING",
        )
        .bind(event.session_id)
        .bind(event.cycle_id)
        .bind(&event.channel_login)
        .bind(event.occurred_at)
        .bind(event.outcome.as_str())
        .bind(event.stage.as_str())
        .bind(event.transcript.as_deref())
        .bind(decision)
        .bind(event.static_recruitment_text.as_deref())
        .bind(event.error_class.as_deref())
        .bind(event.provider.as_deref())
        .bind(event.model.as_deref())
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 1 {
            if let Some(decision) = &event.decision {
                sqlx::query(
                    "UPDATE twitch_outreach_shadow_sessions
                     SET stage = $1
                     WHERE id = $2 AND ended_at IS NULL",
                )
                .bind(decision.stage.as_str())
                .bind(event.session_id)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            Ok(true)
        } else {
            tx.commit().await?;
            Ok(false)
        }
    }

    pub async fn load_context(
        &self,
        session: &OutreachSession,
        current_transcript: &str,
        now: DateTime<Utc>,
    ) -> Result<LoadedOutreachContext, StoreError> {
        let rows = sqlx::query_as::<_, (DateTime<Utc>, Option<String>, Option<Value>)>(
            "SELECT occurred_at, transcript, decision
             FROM twitch_outreach_shadow_events
             WHERE session_id = $1
             ORDER BY occurred_at, id",
        )
        .bind(session.id)
        .fetch_all(&self.pool)
        .await?;
        let mut timestamped_transcripts = Vec::new();
        let mut previous_hooks = Vec::new();
        let mut last_qualify_at = None;
        let mut answered_qualify = false;
        let mut previous_offer = false;
        for (occurred_at, transcript, decision) in rows {
            if let Some(text) = transcript.filter(|text| !text.trim().is_empty()) {
                if last_qualify_at.is_some_and(|qualify_at| occurred_at > qualify_at)
                    && looks_like_qualify_answer(&text)
                {
                    answered_qualify = true;
                }
                timestamped_transcripts.push(TimestampedText {
                    text,
                    occurred_at,
                    author: None,
                });
            }
            if let Some(value) = decision {
                let parsed = serde_json::from_value::<OutreachDecision>(value)
                    .map_err(|_| StoreError::InvalidData)?;
                if parsed
                    .hooks
                    .iter()
                    .any(|hook| hook.kind == crate::outreach_shadow::HookKind::Qualify)
                {
                    last_qualify_at = Some(occurred_at);
                }
                previous_offer |= parsed
                    .hooks
                    .iter()
                    .any(|hook| hook.kind == crate::outreach_shadow::HookKind::Offer);
                previous_hooks.extend(parsed.hooks);
            }
        }
        let current_transcript = current_transcript.trim();
        if !current_transcript.is_empty() {
            if last_qualify_at.is_some() && looks_like_qualify_answer(current_transcript) {
                answered_qualify = true;
            }
            timestamped_transcripts.push(TimestampedText {
                text: current_transcript.to_owned(),
                occurred_at: now,
                author: None,
            });
        }

        let chat_rows = sqlx::query_as::<_, (DateTime<Utc>, String, Option<String>)>(
            "SELECT message_ts, content, chatter_login
             FROM twitch_chat_messages
             WHERE LOWER(streamer_login) = LOWER($1)
               AND message_ts >= $2
               AND message_ts <= $3
               AND NULLIF(BTRIM(content), '') IS NOT NULL
             ORDER BY message_ts
             LIMIT 250",
        )
        .bind(&session.channel_login)
        .bind(session.started_at)
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        let mut timestamped_chat = Vec::new();
        for (message_ts, content, author) in chat_rows {
            timestamped_chat.push(TimestampedText {
                text: content,
                occurred_at: message_ts,
                author,
            });
        }
        let (game, viewer_count) = sqlx::query_as::<_, (Option<String>, Option<i32>)>(
            "SELECT last_game, last_viewer_count
             FROM twitch_live_state
             WHERE twitch_user_id = $1
             LIMIT 1",
        )
        .bind(&session.streamer_user_id)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or((None, None));
        let raid_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM twitch_raid_history
             WHERE to_broadcaster_id = $1
               AND COALESCE(success, FALSE) IS TRUE",
        )
        .bind(&session.streamer_user_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(LoadedOutreachContext {
            evidence: SessionEvidence {
                transcripts: timestamped_transcripts.clone(),
                chat_messages: timestamped_chat.clone(),
                has_answered_qualify: answered_qualify,
                has_previous_offer: previous_offer,
                session_started_at: session.started_at,
                now,
                streamer_login: session.channel_login.clone(),
            },
            input: OutreachModelInput {
                streamer_transcripts: timestamped_transcripts,
                chat_messages: timestamped_chat,
                previous_hooks,
                channel_state: json!({
                    "partner": false,
                    "game": game,
                    "viewer_count": viewer_count,
                    "raid_count": raid_count,
                    "session_id": session.id.to_string(),
                    "channel_login": session.channel_login,
                }),
            },
            raid_count,
        })
    }

    pub async fn claim_discord_events(
        &self,
        limit: i64,
        now: DateTime<Utc>,
    ) -> Result<Vec<ClaimedOutreachEvent>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let claim_id = Uuid::new_v4();
        let ids = sqlx::query_scalar::<_, i64>(
            "SELECT id
             FROM twitch_outreach_shadow_events
             WHERE discord_message_id IS NULL
               AND expires_at > $1
               AND discord_attempts < 3
               AND discord_next_attempt_at <= $1
               AND (discord_claim_until IS NULL OR discord_claim_until <= $1)
             ORDER BY id
             FOR UPDATE SKIP LOCKED
             LIMIT $2",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        if ids.is_empty() {
            tx.commit().await?;
            return Ok(Vec::new());
        }
        sqlx::query(
            "UPDATE twitch_outreach_shadow_events
             SET discord_claim_id = $1, discord_claim_until = $2
             WHERE id = ANY($3)",
        )
        .bind(claim_id)
        .bind(now + DISCORD_CLAIM_TTL)
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT e.id, e.session_id, e.cycle_id, e.channel_login,
                    e.occurred_at, e.outcome, e.transcript, e.decision,
                    e.static_recruitment_text, e.error_class, e.provider, e.model,
                    s.streamer_user_id, s.started_at, e.stage
             FROM twitch_outreach_shadow_events e
             JOIN twitch_outreach_shadow_sessions s ON s.id = e.session_id
             WHERE e.id = ANY($1)
             ORDER BY e.id",
        )
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        rows.into_iter()
            .map(|row| {
                Ok(ClaimedOutreachEvent {
                    claim_id,
                    event: row.try_into()?,
                })
            })
            .collect()
    }

    pub async fn mark_discord_sent(
        &self,
        event_id: i64,
        claim_id: Uuid,
        message_id: &str,
    ) -> Result<(), StoreError> {
        let updated = sqlx::query(
            "UPDATE twitch_outreach_shadow_events
             SET discord_message_id = $1,
                 discord_claim_id = NULL,
                 discord_claim_until = NULL,
                 discord_last_error = NULL
             WHERE id = $2 AND discord_claim_id = $3",
        )
        .bind(message_id)
        .bind(event_id)
        .bind(claim_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 1 {
            Ok(())
        } else {
            Err(StoreError::InvalidData)
        }
    }

    pub async fn mark_discord_failed(
        &self,
        event_id: i64,
        claim_id: Uuid,
        error_class: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE twitch_outreach_shadow_events
             SET discord_attempts = discord_attempts + 1,
                 discord_next_attempt_at = $1,
                 discord_claim_id = NULL,
                 discord_claim_until = NULL,
                 discord_last_error = LEFT($2, 64)
             WHERE id = $3 AND discord_claim_id = $4",
        )
        .bind(now + DISCORD_RETRY_DELAY)
        .bind(error_class)
        .bind(event_id)
        .bind(claim_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn expired_discord_events(
        &self,
        limit: i64,
        now: DateTime<Utc>,
    ) -> Result<Vec<ExpiredDiscordEvent>, StoreError> {
        let rows = sqlx::query_as::<_, (i64, String)>(
            "SELECT id, discord_message_id
             FROM twitch_outreach_shadow_events
             WHERE expires_at <= $1
               AND discord_message_id IS NOT NULL
               AND discord_delete_attempts < 3
               AND discord_delete_next_attempt_at <= $1
             ORDER BY expires_at, id
             LIMIT $2",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, message_id)| ExpiredDiscordEvent { id, message_id })
            .collect())
    }

    pub async fn delete_expired_event(
        &self,
        event_id: i64,
        message_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM twitch_outreach_shadow_events
             WHERE id = $1
               AND discord_message_id = $2
               AND expires_at <= $3",
        )
        .bind(event_id)
        .bind(message_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        delete_orphan_sessions(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_discord_delete_failed(
        &self,
        event_id: i64,
        error_class: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE twitch_outreach_shadow_events
             SET discord_delete_attempts = discord_delete_attempts + 1,
                 discord_delete_next_attempt_at = $1,
                 discord_last_delete_error = LEFT($2, 64),
                 transcript = CASE
                     WHEN discord_delete_attempts + 1 >= 3 THEN NULL
                     ELSE transcript
                 END,
                 decision = CASE
                     WHEN discord_delete_attempts + 1 >= 3 THEN NULL
                     ELSE decision
                 END,
                 static_recruitment_text = CASE
                     WHEN discord_delete_attempts + 1 >= 3 THEN NULL
                     ELSE static_recruitment_text
                 END,
                 content_tombstoned_at = CASE
                     WHEN discord_delete_attempts + 1 >= 3 THEN $4
                     ELSE content_tombstoned_at
                 END
             WHERE id = $3
               AND expires_at <= $4
               AND discord_message_id IS NOT NULL",
        )
        .bind(now + DISCORD_DELETE_RETRY_DELAY)
        .bind(error_class)
        .bind(event_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_expired_unposted(&self, now: DateTime<Utc>) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM twitch_outreach_shadow_events
             WHERE expires_at <= $1
               AND discord_message_id IS NULL
               AND (discord_claim_until IS NULL OR discord_claim_until <= $1)",
        )
        .bind(now)
        .execute(&mut *tx)
        .await?;
        delete_orphan_sessions(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn delete_orphan_sessions(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM twitch_outreach_shadow_sessions s
         WHERE s.ended_at IS NOT NULL
           AND NOT EXISTS (
               SELECT 1
               FROM twitch_outreach_shadow_events e
               WHERE e.session_id = s.id
           )",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn choose_candidate(
    active_session: bool,
    mut candidates: Vec<OutreachCandidate>,
    now: DateTime<Utc>,
) -> Option<OutreachCandidate> {
    if active_session {
        return None;
    }
    candidates.retain(|candidate| {
        candidate.is_live
            && candidate
                .game
                .as_deref()
                .is_some_and(|game| game.eq_ignore_ascii_case("deadlock"))
            && !candidate.partner_table
            && !candidate.partner_state
            && candidate
                .cooldown_until
                .is_none_or(|cooldown_until| cooldown_until <= now)
    });
    candidates.sort_by(|left, right| {
        left.last_observed_at
            .cmp(&right.last_observed_at)
            .then_with(|| left.detected_at.cmp(&right.detected_at))
            .then_with(|| left.channel_login.cmp(&right.channel_login))
    });
    candidates.into_iter().next()
}

fn looks_like_qualify_answer(text: &str) -> bool {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .any(|word| {
            matches!(
                word,
                "ja" | "jo"
                    | "jap"
                    | "ne"
                    | "nee"
                    | "nein"
                    | "öfter"
                    | "regelmäßig"
                    | "regelmässig"
                    | "täglich"
                    | "manchmal"
                    | "selten"
                    | "wochenende"
                    | "stream"
                    | "streame"
                    | "streamen"
            )
        })
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: i64,
    session_id: Uuid,
    cycle_id: Uuid,
    channel_login: String,
    occurred_at: DateTime<Utc>,
    outcome: String,
    transcript: Option<String>,
    decision: Option<Value>,
    static_recruitment_text: Option<String>,
    error_class: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    streamer_user_id: String,
    started_at: DateTime<Utc>,
    stage: String,
}

impl TryFrom<EventRow> for OutreachEvent {
    type Error = StoreError;

    fn try_from(row: EventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            session: OutreachSession {
                id: row.session_id,
                channel_login: row.channel_login,
                streamer_user_id: row.streamer_user_id,
                started_at: row.started_at,
                stage: parse_stage(&row.stage)?,
            },
            cycle_id: row.cycle_id,
            occurred_at: row.occurred_at,
            outcome: parse_outcome(&row.outcome)?,
            transcript: row.transcript,
            decision: row
                .decision
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| StoreError::InvalidData)?,
            static_recruitment_text: row.static_recruitment_text,
            error_class: row.error_class,
            provider: row.provider,
            model: row.model,
        })
    }
}

fn session_from_row(
    row: (Uuid, String, String, DateTime<Utc>, String),
) -> Result<OutreachSession, StoreError> {
    Ok(OutreachSession {
        id: row.0,
        channel_login: row.1,
        streamer_user_id: row.2,
        started_at: row.3,
        stage: parse_stage(&row.4)?,
    })
}

fn parse_stage(value: &str) -> Result<OutreachStage, StoreError> {
    match value {
        "watch" => Ok(OutreachStage::Watch),
        "smalltalk" => Ok(OutreachStage::Smalltalk),
        "qualify" => Ok(OutreachStage::Qualify),
        "offer" => Ok(OutreachStage::Offer),
        _ => Err(StoreError::InvalidData),
    }
}

fn parse_outcome(value: &str) -> Result<OutreachOutcome, StoreError> {
    match value {
        "hook" => Ok(OutreachOutcome::Hook),
        "silent" => Ok(OutreachOutcome::Silent),
        "parser_error" => Ok(OutreachOutcome::ParserError),
        "timeout" => Ok(OutreachOutcome::Timeout),
        "provider_error" => Ok(OutreachOutcome::ProviderError),
        "whisper_error" => Ok(OutreachOutcome::WhisperError),
        _ => Err(StoreError::InvalidData),
    }
}

async fn lock_global(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended('tb-engagement:outreach-shadow-global', 0)
        )",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn apply_cooldown(
    tx: &mut Transaction<'_, Postgres>,
    logins: &[String],
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    if logins.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE twitch_partner_outreach
         SET cooldown_until = $1
         WHERE LOWER(streamer_login) = ANY($2)",
    )
    .bind((now + CHANNEL_COOLDOWN).to_rfc3339())
    .bind(
        logins
            .iter()
            .map(|login| login.to_lowercase())
            .collect::<Vec<_>>(),
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::*;

    #[test]
    fn store_typ_ist_konstruierbar() {
        let _ = std::any::TypeId::of::<OutreachShadowStore>();
    }

    #[test]
    fn aktive_sitzung_blockiert_jede_weitere_auswahl() {
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();

        assert!(choose_candidate(true, vec![candidate("frei")], now).is_none());
    }

    #[test]
    fn partner_cooldown_offline_und_falsches_spiel_sind_nicht_waehlbar() {
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
        let mut table_partner = candidate("table_partner");
        table_partner.partner_table = true;
        let mut state_partner = candidate("state_partner");
        state_partner.partner_state = true;
        let mut cooldown = candidate("cooldown");
        cooldown.cooldown_until = Some(now + Duration::hours(1));
        let mut offline = candidate("offline");
        offline.is_live = false;
        let mut wrong_game = candidate("wrong_game");
        wrong_game.game = Some("Counter-Strike 2".to_owned());

        let selected = choose_candidate(
            false,
            vec![
                table_partner,
                state_partner,
                cooldown,
                offline,
                wrong_game,
                candidate("frei"),
            ],
            now,
        )
        .expect("freier Kandidat");

        assert_eq!(selected.channel_login, "frei");
    }

    #[test]
    fn am_laengsten_nicht_beobachteter_kandidat_gewinnt() {
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
        let mut recently_seen = candidate("neu");
        recently_seen.last_observed_at = Some(now - Duration::hours(1));
        let never_seen = candidate("nie");

        let selected =
            choose_candidate(false, vec![recently_seen, never_seen], now).expect("Kandidat");

        assert_eq!(selected.channel_login, "nie");
    }

    #[test]
    fn qualify_braucht_eine_erkennbare_streaming_antwort() {
        assert!(looks_like_qualify_answer(
            "ja ich stream eigentlich jeden tag deadlock"
        ));
        assert!(looks_like_qualify_answer("ne nur manchmal am wochenende"));
        assert!(!looks_like_qualify_answer("das war ein guter fight"));
    }

    fn candidate(login: &str) -> OutreachCandidate {
        OutreachCandidate {
            channel_login: login.to_owned(),
            streamer_user_id: login.to_owned(),
            is_live: true,
            game: Some("Deadlock".to_owned()),
            cooldown_until: None,
            partner_table: false,
            partner_state: false,
            last_observed_at: None,
            detected_at: "2026-07-27T18:00:00Z".to_owned(),
        }
    }
}

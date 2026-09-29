use std::{
    collections::HashSet,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tb_chat::{
    ChatApi, SendOutcome, api::SourceOnlyPreSendCheck, promos::OutboundSuppressionCheck,
};
use tb_http_core::ApiError;
use tb_transport_twitch::{HelixClient, HelixStream};

const EVENT_TTL_SECONDS: i64 = 120;
const SNAPSHOT_TTL_SECONDS: i64 = 120;
const MESSAGE_PREFIX: &str = "Neuer Deadlock-Patch ist da 🔥 Die Änderungen auf Deutsch: ";
const ARTICLE_PREFIX: &str = "https://deutsche-deadlock-community.de/patchnotes/patch-";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchEvent {
    pub event_id: String,
    pub source_url: String,
    pub detected_at: DateTime<Utc>,
    pub article_url: String,
    pub message: String,
}

impl PatchEvent {
    pub fn from_article(
        id: i64,
        article_url: String,
        source_url: String,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, PatchProcessError> {
        if id <= 0 || article_url != format!("{ARTICLE_PREFIX}{id}/") {
            return Err(PatchProcessError::Invalid("invalid article URL"));
        }
        let event_id = hex::encode(Sha256::digest(article_url.as_bytes()));
        let message = format!("{MESSAGE_PREFIX}{article_url}");
        Ok(Self {
            event_id,
            source_url,
            detected_at: observed_at,
            article_url,
            message,
        })
    }

    fn validate(&self, now: DateTime<Utc>) -> Result<bool, PatchProcessError> {
        if self.event_id != hex::encode(Sha256::digest(self.article_url.as_bytes())) {
            return Err(PatchProcessError::Invalid("invalid patch identity"));
        }
        let article = url::Url::parse(&self.article_url)
            .map_err(|_| PatchProcessError::Invalid("invalid article URL"))?;
        let id = self
            .article_url
            .strip_prefix(ARTICLE_PREFIX)
            .and_then(|path| path.strip_suffix('/'))
            .and_then(|digits| digits.parse::<i64>().ok())
            .filter(|id| *id > 0);
        if article.scheme() != "https"
            || article.host_str() != Some("deutsche-deadlock-community.de")
            || article.port().is_some()
            || !article.username().is_empty()
            || article.password().is_some()
            || article.query().is_some()
            || article.fragment().is_some()
            || !id.is_some_and(|id| self.article_url == format!("{ARTICLE_PREFIX}{id}/"))
        {
            return Err(PatchProcessError::Invalid("invalid article URL"));
        }
        let source = url::Url::parse(&self.source_url)
            .map_err(|_| PatchProcessError::Invalid("invalid patch source"))?;
        if source.scheme() != "https"
            || !matches!(
                source.host_str(),
                Some("forums.playdeadlock.com" | "steamcommunity.com" | "store.steampowered.com")
            )
            || !source.username().is_empty()
            || source.password().is_some()
            || source.port().is_some_and(|port| port != 443)
            || self.source_url.len() > 512
            || self.source_url.chars().any(char::is_control)
        {
            return Err(PatchProcessError::Invalid("invalid patch source"));
        }
        if self.message != format!("{MESSAGE_PREFIX}{}", self.article_url)
            || self.message.chars().count() > 450
            || self.message.chars().any(char::is_control)
        {
            return Err(PatchProcessError::Invalid("invalid patch announcement"));
        }
        if self.detected_at > now {
            return Err(PatchProcessError::Invalid("future patch event"));
        }
        Ok(fresh(self.detected_at, now, EVENT_TTL_SECONDS))
    }
}

fn fresh(at: DateTime<Utc>, now: DateTime<Utc>, ttl: i64) -> bool {
    at <= now && now.signed_duration_since(at) <= chrono::Duration::seconds(ttl)
}

type DeadlineClock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

fn source_only_deadline_guard(
    detected_at: DateTime<Utc>,
    deadline_clock: DeadlineClock,
) -> SourceOnlyPreSendCheck {
    Box::new(move || {
        if deadline_clock() >= detected_at + chrono::Duration::seconds(EVENT_TTL_SECONDS) {
            Err("event_expired_before_post")
        } else {
            Ok(())
        }
    })
}

#[derive(Debug)]
pub enum PatchProcessError {
    Invalid(&'static str),
    Unavailable,
    Database { sqlstate: Option<String> },
}

impl fmt::Display for PatchProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "{message}"),
            Self::Unavailable => f.write_str("patch transport unavailable"),
            Self::Database { .. } => f.write_str("patch persistence unavailable"),
        }
    }
}

impl Error for PatchProcessError {}

impl From<PatchProcessError> for ApiError {
    fn from(error: PatchProcessError) -> Self {
        match error {
            PatchProcessError::Invalid(message) => ApiError::bad_request(message),
            PatchProcessError::Unavailable | PatchProcessError::Database { .. } => {
                ApiError::unavailable()
            }
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", content = "deliveries", rename_all = "snake_case")]
pub enum PatchProcessOutcome {
    Processed(Vec<(String, i64)>),
    SkippedExpired,
    DuplicateSource,
}

#[derive(Debug, sqlx::FromRow)]
struct Candidate {
    twitch_user_id: String,
    last_stream_id: String,
}

#[derive(Clone, Debug)]
struct DeliveryResult {
    status: &'static str,
    drop_code: Option<String>,
    http_status: Option<i16>,
    uncertainty_reason: Option<&'static str>,
}

impl DeliveryResult {
    fn status(status: &'static str) -> Self {
        Self {
            status,
            drop_code: None,
            http_status: None,
            uncertainty_reason: None,
        }
    }
}

fn http_error_result(status: u16, body: &str) -> DeliveryResult {
    let uncertainty_reason = if (200..300).contains(&status) {
        match body {
            "response_body_unreadable" => "response_body_unreadable",
            "response_result_missing" => "response_result_missing",
            _ => "unexpected_success_status",
        }
    } else {
        "http_error"
    };
    DeliveryResult {
        status: "uncertain",
        drop_code: None,
        http_status: Some(status as i16),
        uncertainty_reason: Some(uncertainty_reason),
    }
}

fn local_pre_post_rejection_reason(reason: &str) -> Option<&'static str> {
    match reason {
        "channel_policy_denied" => Some("channel_policy_denied"),
        "source_only_chat_muted" => Some("source_only_chat_muted"),
        _ => None,
    }
}

fn delivery_result_from_send_outcome(outcome: SendOutcome) -> DeliveryResult {
    match outcome {
        SendOutcome::Sent => DeliveryResult::status("sent"),
        SendOutcome::Dropped { code, .. } => {
            if let Some(reason) = local_pre_post_rejection_reason(&code) {
                skip_reason_for_result(reason)
            } else {
                DeliveryResult {
                    status: "dropped",
                    drop_code: Some(redact_drop_code(&code)),
                    http_status: None,
                    uncertainty_reason: None,
                }
            }
        }
        SendOutcome::HttpError { status, body } => http_error_result(status, &body),
    }
}

#[async_trait::async_trait]
trait Transport: Send + Sync {
    async fn streams(&self, ids: &[String]) -> Result<Vec<HelixStream>, PatchProcessError>;
    async fn can_send(
        &self,
        id: &str,
        stream_id: &str,
    ) -> Result<Option<&'static str>, PatchProcessError>;
    async fn send(
        &self,
        id: &str,
        stream_id: &str,
        message: &str,
        detected_at: DateTime<Utc>,
    ) -> Result<DeliveryResult, PatchProcessError>;
}

struct LiveTransport<'a> {
    receiver: &'a PatchReceiver,
}

impl LiveTransport<'_> {
    async fn authorized_login(
        &self,
        id: &str,
        stream_id: &str,
    ) -> Result<Option<String>, PatchProcessError> {
        authorized_login(&self.receiver.pool, id, stream_id).await
    }
}

async fn authorized_login(
    pool: &PgPool,
    id: &str,
    stream_id: &str,
) -> Result<Option<String>, PatchProcessError> {
    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT p.twitch_login, l.last_seen_at \
         FROM twitch_streamers_partner_state p \
         JOIN twitch_live_state l ON l.twitch_user_id = p.twitch_user_id \
         WHERE p.twitch_user_id = $1 AND l.last_stream_id = $2 \
           AND l.is_live = 1 AND l.last_game = 'Deadlock' \
           AND p.is_partner_active = 1 \
           AND COALESCE(p.manual_partner_opt_out, 0) = 0 \
           AND EXISTS (SELECT 1 FROM twitch_raid_auth ra \
                       WHERE ra.twitch_user_id = p.twitch_user_id \
                         AND ra.needs_reauth IS FALSE \
                         AND 'channel:bot' = ANY(regexp_split_to_array( \
                             COALESCE(ra.scopes, ''), '[[:space:],]+'))) \
         LIMIT 1",
    )
    .bind(id)
    .bind(stream_id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)?;
    Ok(row.and_then(|(login, last_seen)| {
        last_seen
            .as_deref()
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .filter(|at| fresh(at.with_timezone(&Utc), Utc::now(), SNAPSHOT_TTL_SECONDS))
            .map(|_| login)
    }))
}

#[async_trait::async_trait]
impl Transport for LiveTransport<'_> {
    async fn streams(&self, ids: &[String]) -> Result<Vec<HelixStream>, PatchProcessError> {
        self.receiver
            .helix
            .get_streams_by_user_ids(ids, None)
            .await
            .map_err(|_| PatchProcessError::Unavailable)
    }

    async fn can_send(
        &self,
        id: &str,
        stream_id: &str,
    ) -> Result<Option<&'static str>, PatchProcessError> {
        let Some(login) = self.authorized_login(id, stream_id).await? else {
            return Ok(Some("authorization_unavailable"));
        };
        if self
            .receiver
            .suppression
            .is_muted_checked(&login)
            .await
            .map_err(|_| {
                tracing::error!(
                    broadcaster_id = id,
                    "Patch announcement suppression check failed, skipping send"
                );
                PatchProcessError::Unavailable
            })?
        {
            return Ok(Some("channel_suppressed"));
        }
        Ok(None)
    }

    async fn send(
        &self,
        id: &str,
        stream_id: &str,
        message: &str,
        detected_at: DateTime<Utc>,
    ) -> Result<DeliveryResult, PatchProcessError> {
        if let Some(reason) = self.can_send(id, stream_id).await? {
            return Ok(skip_reason_for_result(reason));
        }
        if !fresh(detected_at, Utc::now(), EVENT_TTL_SECONDS) {
            return Ok(DeliveryResult {
                status: "skipped",
                drop_code: None,
                http_status: None,
                uncertainty_reason: Some("event_expired_before_post"),
            });
        }
        Ok(
            match self
                .receiver
                .chat
                .send_source_only_message_guarded(
                    id,
                    message,
                    source_only_deadline_guard(
                        detected_at,
                        Arc::clone(&self.receiver.deadline_clock),
                    ),
                )
                .await
            {
                Ok(SendOutcome::Dropped { code, .. }) if code == "event_expired_before_post" => {
                    DeliveryResult {
                        status: "skipped",
                        drop_code: None,
                        http_status: None,
                        uncertainty_reason: Some("event_expired_before_post"),
                    }
                }
                Ok(outcome) => delivery_result_from_send_outcome(outcome),
                Err(error) => match local_pre_post_rejection_reason(&error) {
                    Some(reason) => skip_reason_for_result(reason),
                    None => DeliveryResult {
                        status: "uncertain",
                        drop_code: None,
                        http_status: None,
                        uncertainty_reason: Some(send_error_reason(&error)),
                    },
                },
            },
        )
    }
}

fn redact_drop_code(code: &str) -> String {
    if !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        code.to_owned()
    } else {
        "redacted".to_owned()
    }
}

fn send_error_reason(error: &str) -> &'static str {
    if let Some(reason) = error.strip_prefix("source_only_chat_outcome_unknown:") {
        match reason.trim() {
            "source_only_chat_body_unreadable" => "response_body_unreadable",
            "source_only_chat_result_missing" => "response_result_missing",
            "source_only_chat_unexpected_success_status" => "unexpected_success_status",
            _ => "ambiguous_http_outcome",
        }
    } else if error.starts_with("source_only_chat_transport_failed:") {
        "transport_error"
    } else {
        "send_error"
    }
}

fn candidate_skip_reason(
    candidate: &Candidate,
    streams: &[HelixStream],
    event: &PatchEvent,
) -> Result<(), &'static str> {
    let Some(stream) = streams
        .iter()
        .find(|stream| stream.user_id == candidate.twitch_user_id)
    else {
        return Err("stream_not_live");
    };
    if stream.id != candidate.last_stream_id {
        return Err("stream_changed");
    }
    if stream.game_name != "Deadlock" || stream.game_id.is_empty() {
        return Err("game_changed");
    }
    let Ok(started_at) = DateTime::parse_from_rfc3339(&stream.started_at) else {
        return Err("stream_start_invalid");
    };
    if started_at > event.detected_at {
        return Err("stream_started_after_event");
    }
    Ok(())
}

fn skip_reason_for_result(reason: &'static str) -> DeliveryResult {
    DeliveryResult {
        status: "skipped",
        drop_code: None,
        http_status: None,
        uncertainty_reason: Some(reason),
    }
}

fn database_error(error: sqlx::Error) -> PatchProcessError {
    let sqlstate = error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned());
    tracing::error!(error = %error, sqlstate = ?sqlstate, "Patch announcement persistence failed");
    PatchProcessError::Database { sqlstate }
}

fn log_database_context(error: &PatchProcessError, event_id: &str, broadcaster_id: &str) {
    if let PatchProcessError::Database { sqlstate } = error {
        tracing::error!(
            event_id,
            broadcaster_id,
            sqlstate = ?sqlstate,
            "Patch announcement delivery database operation failed"
        );
    }
}

fn database_error_for(
    error: sqlx::Error,
    event_id: &str,
    broadcaster_id: &str,
) -> PatchProcessError {
    let error = database_error(error);
    log_database_context(&error, event_id, broadcaster_id);
    error
}

pub struct PatchReceiver {
    pool: PgPool,
    helix: HelixClient,
    chat: Arc<dyn ChatApi>,
    suppression: Arc<dyn OutboundSuppressionCheck>,
    deadline_clock: DeadlineClock,
}

impl PatchReceiver {
    pub fn new(
        pool: PgPool,
        helix: HelixClient,
        chat: Arc<dyn ChatApi>,
        suppression: Arc<dyn OutboundSuppressionCheck>,
    ) -> Self {
        Self {
            pool,
            helix,
            chat,
            suppression,
            deadline_clock: Arc::new(Utc::now),
        }
    }

    #[cfg(test)]
    fn with_deadline_clock(mut self, deadline_clock: DeadlineClock) -> Self {
        self.deadline_clock = deadline_clock;
        self
    }

    pub async fn process(
        &self,
        event: &PatchEvent,
    ) -> Result<PatchProcessOutcome, PatchProcessError> {
        process_with_timeout(
            &self.pool,
            &LiveTransport { receiver: self },
            event,
            Duration::from_secs(EVENT_TTL_SECONDS as u64),
        )
        .await
    }
}

async fn process_with_timeout(
    pool: &PgPool,
    transport: &dyn Transport,
    event: &PatchEvent,
    timeout: Duration,
) -> Result<PatchProcessOutcome, PatchProcessError> {
    let attempted = Arc::new(Mutex::new(HashSet::new()));
    let result = tokio::time::timeout(
        timeout,
        process_inner(pool, transport, event, Arc::clone(&attempted)),
    )
    .await;
    let result = match result {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!(
                event_id = %event.event_id,
                uncertainty_reason = "receiver_timeout",
                "Patch announcement event processing timed out"
            );
            match tokio::time::timeout(
                Duration::from_secs(5),
                mark_timed_out_attempts(pool, &event.event_id, &attempted),
            )
            .await
            {
                Ok(result) => result?,
                Err(_) => {
                    tracing::error!(
                        event_id = %event.event_id,
                        uncertainty_reason = "receiver_timeout_cleanup_timed_out",
                        "Patch announcement timeout cleanup did not finish"
                    );
                    return Err(PatchProcessError::Unavailable);
                }
            }
            Err(PatchProcessError::Unavailable)
        }
    };
    if let Err(PatchProcessError::Database { sqlstate }) = &result {
        tracing::error!(
            event_id = %event.event_id,
            sqlstate = ?sqlstate,
            "Patch announcement event database operation failed"
        );
    }
    result
}

async fn mark_timed_out_attempts(
    pool: &PgPool,
    event_id: &str,
    attempted: &Mutex<HashSet<(String, String)>>,
) -> Result<(), PatchProcessError> {
    let attempts: Vec<_> = attempted
        .lock()
        .map_err(|_| PatchProcessError::Unavailable)?
        .iter()
        .cloned()
        .collect();
    for (broadcaster_id, attempt_token) in attempts {
        tracing::warn!(
            event_id,
            broadcaster_id,
            outcome = "possible_delivery",
            uncertainty_reason = "receiver_timeout_cleanup_pending",
            "Patch announcement attempt may have been delivered before timing out"
        );
        let updated = sqlx::query(
            "UPDATE twitch_patch_announcement_deliveries \
             SET status='uncertain', uncertainty_reason='receiver_timeout_after_attempt' \
             WHERE event_id=$1 AND broadcaster_id=$2 AND status='attempted' \
               AND uncertainty_reason=$3",
        )
        .bind(event_id)
        .bind(&broadcaster_id)
        .bind(&attempt_token)
        .execute(pool)
        .await
        .map_err(|error| database_error_for(error, event_id, &broadcaster_id))?;
        if updated.rows_affected() == 1 {
            tracing::warn!(
                event_id,
                broadcaster_id,
                outcome = "uncertain",
                uncertainty_reason = "receiver_timeout_after_attempt",
                "Patch announcement attempt was recorded as uncertain after timeout"
            );
        }
    }
    Ok(())
}

async fn skip_pending_deliveries(
    pool: &PgPool,
    event_id: &str,
    reason: &'static str,
) -> Result<(), PatchProcessError> {
    let skipped = sqlx::query_scalar::<_, String>(
        "UPDATE twitch_patch_announcement_deliveries \
         SET status='skipped', uncertainty_reason=$2 \
         WHERE event_id=$1 AND status='pending' RETURNING broadcaster_id",
    )
    .bind(event_id)
    .bind(reason)
    .fetch_all(pool)
    .await
    .map_err(|error| database_error_for(error, event_id, "batch"))?;
    for broadcaster_id in skipped {
        tracing::info!(
            event_id,
            broadcaster_id,
            outcome = "skipped",
            uncertainty_reason = reason,
            "Patch announcement recipient skipped"
        );
    }
    Ok(())
}

#[cfg(test)]
async fn approve_observation(pool: &PgPool, event: &PatchEvent) {
    let patch_id = event
        .article_url
        .strip_prefix(ARTICLE_PREFIX)
        .and_then(|value| value.strip_suffix('/'))
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap();
    sqlx::query(
        "UPDATE twitch_patch_feed_observations SET observed_at=$2 \
         WHERE patch_id=$1 AND status='pending'",
    )
    .bind(patch_id)
    .bind(event.detected_at)
    .execute(pool)
    .await
    .unwrap();
}

#[cfg(test)]
async fn process(
    pool: &PgPool,
    transport: &dyn Transport,
    event: &PatchEvent,
) -> Result<PatchProcessOutcome, PatchProcessError> {
    approve_observation(pool, event).await;
    process_inner(pool, transport, event, Arc::new(Mutex::new(HashSet::new()))).await
}

async fn process_inner(
    pool: &PgPool,
    transport: &dyn Transport,
    event: &PatchEvent,
    attempted: Arc<Mutex<HashSet<(String, String)>>>,
) -> Result<PatchProcessOutcome, PatchProcessError> {
    let patch_id = event
        .article_url
        .strip_prefix(ARTICLE_PREFIX)
        .and_then(|value| value.strip_suffix('/'))
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|id| *id > 0)
        .ok_or(PatchProcessError::Invalid("invalid article URL"))?;
    let observation = sqlx::query_as::<_, (String, DateTime<Utc>)>(
        "SELECT status, observed_at FROM twitch_patch_feed_observations WHERE patch_id=$1",
    )
    .bind(patch_id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)?;
    let Some((observation_status, observed_at)) = observation else {
        return Err(PatchProcessError::Invalid(
            "patch event lacks an approved observation",
        ));
    };
    if observation_status != "pending"
        || observed_at.timestamp_micros() != event.detected_at.timestamp_micros()
    {
        return Err(PatchProcessError::Invalid(
            "patch event differs from its approved observation",
        ));
    }
    event.validate(Utc::now())?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let inserted = sqlx::query(
        "INSERT INTO twitch_patch_announcements (event_id, article_url, source_url, detected_at, message) \
         VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",
    )
    .bind(&event.event_id)
    .bind(&event.article_url)
    .bind(&event.source_url)
    .bind(event.detected_at)
    .bind(&event.message)
    .execute(&mut *tx)
    .await
    .map_err(database_error)?
    .rows_affected() == 1;
    if inserted {
        let patch_id = event
            .article_url
            .strip_prefix(ARTICLE_PREFIX)
            .and_then(|value| value.strip_suffix('/'))
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or(PatchProcessError::Invalid("invalid article URL"))?;
        let candidates = sqlx::query_as::<_, Candidate>(
            "SELECT broadcaster_id AS twitch_user_id, stream_id AS last_stream_id \
             FROM twitch_patch_announcement_recipients WHERE patch_id=$1 \
             ORDER BY broadcaster_id",
        )
        .bind(patch_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(database_error)?;
        if !event.validate(Utc::now())? {
            for candidate in &candidates {
                sqlx::query(
                    "INSERT INTO twitch_patch_announcement_deliveries \
                     (event_id, broadcaster_id, stream_id, status, uncertainty_reason) \
                     VALUES ($1,$2,$3,'skipped','event_expired_before_snapshot')",
                )
                .bind(&event.event_id)
                .bind(&candidate.twitch_user_id)
                .bind(&candidate.last_stream_id)
                .execute(&mut *tx)
                .await
                .map_err(|error| {
                    database_error_for(error, &event.event_id, &candidate.twitch_user_id)
                })?;
                tracing::info!(
                    event_id = %event.event_id,
                    broadcaster_id = %candidate.twitch_user_id,
                    outcome = "skipped",
                    uncertainty_reason = "event_expired_before_snapshot",
                    "Patch announcement recipient skipped"
                );
            }
            tx.commit().await.map_err(database_error)?;
            return Ok(PatchProcessOutcome::SkippedExpired);
        }
        let ids: Vec<_> = candidates
            .iter()
            .map(|candidate| candidate.twitch_user_id.clone())
            .collect();
        let streams = if ids.is_empty() {
            vec![]
        } else {
            transport.streams(&ids).await?
        };
        for candidate in &candidates {
            let reason = match candidate_skip_reason(candidate, &streams, event) {
                Err(reason) => Some(reason),
                Ok(()) => transport
                    .can_send(&candidate.twitch_user_id, &candidate.last_stream_id)
                    .await
                    .inspect_err(|error| {
                        log_database_context(error, &event.event_id, &candidate.twitch_user_id);
                    })?,
            };
            let status = if reason.is_some() {
                "skipped"
            } else {
                "pending"
            };
            sqlx::query(
                "INSERT INTO twitch_patch_announcement_deliveries \
                 (event_id, broadcaster_id, stream_id, status, uncertainty_reason) \
                 VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(&event.event_id)
            .bind(&candidate.twitch_user_id)
            .bind(&candidate.last_stream_id)
            .bind(status)
            .bind(reason)
            .execute(&mut *tx)
            .await
            .map_err(|error| {
                database_error_for(error, &event.event_id, &candidate.twitch_user_id)
            })?;
            if let Some(reason) = reason {
                tracing::info!(
                    event_id = %event.event_id,
                    broadcaster_id = %candidate.twitch_user_id,
                    outcome = "skipped",
                    uncertainty_reason = reason,
                    "Patch announcement recipient excluded"
                );
            }
        }
        tracing::info!(
            event_id = %event.event_id,
            recipients = candidates.len(),
            "Patch announcement snapshot saved"
        );
    }
    tx.commit().await.map_err(database_error)?;
    let saved = sqlx::query_as::<_, (String, String, DateTime<Utc>, String)>(
        "SELECT article_url, source_url, detected_at, message \
         FROM twitch_patch_announcements WHERE event_id=$1",
    )
    .bind(&event.event_id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)?;
    let Some((article, source, detected_at, message)) = saved else {
        return Ok(PatchProcessOutcome::DuplicateSource);
    };
    if article != event.article_url
        || source != event.source_url
        || detected_at.timestamp_micros() != event.detected_at.timestamp_micros()
        || message != event.message
    {
        return Err(PatchProcessError::Invalid(
            "patch event payload differs from saved event",
        ));
    }
    let pending = sqlx::query_as::<_, (String, String)>(
        "SELECT broadcaster_id, stream_id FROM twitch_patch_announcement_deliveries \
         WHERE event_id=$1 AND status='pending' ORDER BY broadcaster_id",
    )
    .bind(&event.event_id)
    .fetch_all(pool)
    .await
    .map_err(database_error)?;
    for (id, stream_id) in pending {
        if !fresh(detected_at, Utc::now(), EVENT_TTL_SECONDS) {
            skip_pending_deliveries(pool, &event.event_id, "event_expired_before_claim").await?;
            return Ok(PatchProcessOutcome::SkippedExpired);
        }
        let live = transport.streams(std::slice::from_ref(&id)).await?;
        let live_stream = live.iter().find(|stream| stream.user_id == id);
        let stream_reason = match live_stream {
            None => Some("stream_not_live"),
            Some(stream) if stream.id != stream_id => Some("stream_changed"),
            Some(stream) if stream.game_name != "Deadlock" || stream.game_id.is_empty() => {
                Some("game_changed")
            }
            Some(_) => None,
        };
        let suppression_reason = if stream_reason.is_none() {
            transport
                .can_send(&id, &stream_id)
                .await
                .inspect_err(|error| {
                    log_database_context(error, &event.event_id, &id);
                })?
        } else {
            None
        };
        let skip_reason = stream_reason.or(suppression_reason);
        let status = if skip_reason.is_none() {
            "attempted"
        } else {
            "skipped"
        };
        let attempt_token = format!("receiver_attempt:{}", uuid::Uuid::new_v4());
        if status == "attempted" {
            attempted
                .lock()
                .map_err(|_| PatchProcessError::Unavailable)?
                .insert((id.clone(), attempt_token.clone()));
        }
        let claimed = sqlx::query(
            "UPDATE twitch_patch_announcement_deliveries SET status=$3, attempted_at=CASE WHEN $3='attempted' THEN now() ELSE attempted_at END, uncertainty_reason=CASE WHEN $3='attempted' THEN $4 ELSE $5 END \
             WHERE event_id=$1 AND broadcaster_id=$2 AND status='pending'",
        )
        .bind(&event.event_id)
        .bind(&id)
        .bind(status)
        .bind(&attempt_token)
        .bind(skip_reason)
        .execute(pool)
        .await
        .map_err(|error| database_error_for(error, &event.event_id, &id))?
        .rows_affected()
            == 1;
        if claimed && status == "skipped" {
            tracing::info!(
                event_id = %event.event_id,
                broadcaster_id = %id,
                outcome = "skipped",
                uncertainty_reason = skip_reason,
                "Patch announcement recipient skipped"
            );
        }
        if claimed && status == "attempted" {
            let outcome = match transport.send(&id, &stream_id, &message, detected_at).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    log_database_context(&error, &event.event_id, &id);
                    return Err(error);
                }
            };
            tracing::info!(
                event_id=%event.event_id,
                broadcaster_id=%id,
                outcome=outcome.status,
                drop_code=?outcome.drop_code,
                http_status=?outcome.http_status,
                uncertainty_reason=?outcome.uncertainty_reason,
                "Patch announcement delivery"
            );
            sqlx::query(
                "UPDATE twitch_patch_announcement_deliveries \
                 SET status=$3, drop_code=$4, http_status=$5, uncertainty_reason=$6 \
                 WHERE event_id=$1 AND broadcaster_id=$2 AND status='attempted' \
                   AND uncertainty_reason=$7",
            )
            .bind(&event.event_id)
            .bind(&id)
            .bind(outcome.status)
            .bind(outcome.drop_code.as_deref())
            .bind(outcome.http_status)
            .bind(outcome.uncertainty_reason)
            .bind(&attempt_token)
            .execute(pool)
            .await
            .map_err(|error| database_error_for(error, &event.event_id, &id))?;
        }
    }
    let counts = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, count(*) FROM twitch_patch_announcement_deliveries \
         WHERE event_id=$1 GROUP BY status",
    )
    .bind(&event.event_id)
    .fetch_all(pool)
    .await
    .map_err(database_error)?;
    Ok(PatchProcessOutcome::Processed(counts))
}

#[cfg(test)]
#[path = "patch_announcement_tests.rs"]
mod integration_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_event() -> PatchEvent {
        PatchEvent::from_article(
            286,
            "https://deutsche-deadlock-community.de/patchnotes/patch-286/".into(),
            "https://forums.playdeadlock.com/posts/123/".into(),
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn validates_only_fresh_signals_and_exact_safe_chat() {
        let mut event = patch_event();
        assert!(event.validate(Utc::now()).unwrap());
        event.detected_at -= chrono::Duration::seconds(121);
        assert!(!event.validate(Utc::now()).unwrap());
        event.detected_at = Utc::now() + chrono::Duration::seconds(10);
        assert!(event.validate(Utc::now()).is_err());
        let mut event = patch_event();
        event.message = format!("@everyone {}", event.article_url);
        assert!(event.validate(Utc::now()).is_err());
    }

    #[test]
    fn allows_sending_until_the_full_event_ttl() {
        let detected_at = DateTime::from_timestamp(1_000, 0).unwrap();
        assert!(fresh(
            detected_at,
            detected_at + chrono::Duration::seconds(119),
            EVENT_TTL_SECONDS
        ));
        assert!(fresh(
            detected_at,
            detected_at + chrono::Duration::seconds(120),
            EVENT_TTL_SECONDS
        ));
        assert!(!fresh(
            detected_at,
            detected_at + chrono::Duration::seconds(121),
            EVENT_TTL_SECONDS
        ));
        assert!(!fresh(
            detected_at,
            detected_at - chrono::Duration::seconds(1),
            EVENT_TTL_SECONDS
        ));
    }

    #[test]
    fn rejects_lookalike_urls_credentials_and_extra_fields() {
        for source in [
            "https://forums.playdeadlock.com.evil.test/posts/1/",
            "https://token@forums.playdeadlock.com/posts/1/",
            "http://forums.playdeadlock.com/posts/1/",
        ] {
            let mut event = patch_event();
            event.source_url = source.into();
            assert!(event.validate(Utc::now()).is_err());
        }
        for article in [
            "https://deutsche-deadlock-community.de.evil.test/patchnotes/patch-286/",
            "https://token@deutsche-deadlock-community.de/patchnotes/patch-286/",
            "http://deutsche-deadlock-community.de/patchnotes/patch-286/",
            "https://deutsche-deadlock-community.de:443/patchnotes/patch-286/",
            "https://deutsche-deadlock-community.de/patchnotes/patch-0286/",
            "https://deutsche-deadlock-community.de/patchnotes/patch-286/?q=1",
            "https://deutsche-deadlock-community.de/patchnotes/patch-286/#more",
            "https://deutsche-deadlock-community.de/patchnotes/patch-abc/",
        ] {
            let mut event = patch_event();
            event.article_url = article.into();
            event.message = format!("{MESSAGE_PREFIX}{article}");
            event.event_id = hex::encode(Sha256::digest(article.as_bytes()));
            assert!(event.validate(Utc::now()).is_err(), "{article}");
        }
        let mut value = serde_json::to_value(patch_event()).unwrap();
        value["patchnotes"] = serde_json::json!("not forwarded");
        assert!(serde_json::from_value::<PatchEvent>(value).is_err());
    }

    #[test]
    fn only_current_deadlock_stream_with_matching_stable_identity() {
        let event = patch_event();
        let candidate = Candidate {
            twitch_user_id: "42".into(),
            last_stream_id: "s1".into(),
        };
        let stream = HelixStream {
            id: "s1".into(),
            user_id: "42".into(),
            game_id: "g1".into(),
            game_name: "Deadlock".into(),
            started_at: (event.detected_at - chrono::Duration::hours(1)).to_rfc3339(),
            ..Default::default()
        };
        assert!(candidate_skip_reason(&candidate, std::slice::from_ref(&stream), &event).is_ok());
        for wrong in [
            HelixStream {
                user_id: "99".into(),
                ..stream.clone()
            },
            HelixStream {
                game_name: "Other game".into(),
                ..stream.clone()
            },
            HelixStream {
                id: "s2".into(),
                ..stream.clone()
            },
            HelixStream {
                started_at: (event.detected_at + chrono::Duration::seconds(1)).to_rfc3339(),
                ..stream.clone()
            },
        ] {
            assert!(candidate_skip_reason(&candidate, &[wrong], &event).is_err());
        }
        assert_eq!(
            send_error_reason("source_only_chat_outcome_unknown: source_only_chat_body_unreadable"),
            "response_body_unreadable"
        );
        assert_eq!(
            send_error_reason("source_only_chat_outcome_unknown: source_only_chat_result_missing"),
            "response_result_missing"
        );
        assert_eq!(
            send_error_reason(
                "source_only_chat_outcome_unknown: source_only_chat_unexpected_success_status"
            ),
            "unexpected_success_status"
        );
        assert_eq!(
            send_error_reason("source_only_chat_outcome_unknown: token=secret"),
            "ambiguous_http_outcome"
        );
        assert_eq!(redact_drop_code("sender_timedout"), "sender_timedout");
        assert_eq!(redact_drop_code("token:secret"), "redacted");
        assert_eq!(
            send_error_reason("source_only_chat_outcome_unknown: xyz"),
            "ambiguous_http_outcome"
        );
    }

    #[test]
    fn known_local_pre_post_rejections_are_skipped_without_guessing() {
        let policy_result = local_pre_post_rejection_reason("channel_policy_denied")
            .map(skip_reason_for_result)
            .unwrap();
        assert_eq!(policy_result.status, "skipped");
        assert_eq!(
            policy_result.uncertainty_reason,
            Some("channel_policy_denied")
        );
        assert_eq!(policy_result.drop_code, None);

        let muted_result = delivery_result_from_send_outcome(SendOutcome::Dropped {
            code: "source_only_chat_muted".into(),
            message: String::new(),
        });
        assert_eq!(muted_result.status, "skipped");
        assert_eq!(
            muted_result.uncertainty_reason,
            Some("source_only_chat_muted")
        );
        assert_eq!(muted_result.drop_code, None);

        let real_transport_error = send_error_reason("source_only_chat_transport_failed: redacted");
        assert_eq!(real_transport_error, "transport_error");
        let lookalike = delivery_result_from_send_outcome(SendOutcome::Dropped {
            code: "source_only_chat_muted_with_suffix".into(),
            message: String::new(),
        });
        assert_eq!(lookalike.status, "dropped");
        assert_eq!(lookalike.uncertainty_reason, None);
    }
}

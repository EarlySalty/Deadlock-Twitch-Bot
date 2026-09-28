use std::{
    collections::HashSet,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{extract::Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tb_chat::{promos::OutboundSuppressionCheck, ChatApi, SendOutcome};
use tb_http_core::{ApiError, AuthLevel};
use tb_transport_twitch::{HelixClient, HelixStream};

const EVENT_TTL_SECONDS: i64 = 120;
const SNAPSHOT_TTL_SECONDS: i64 = 120;
const SOURCE_ONLY_SEND_RESERVE_SECONDS: i64 = 45;
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

fn source_only_send_window_open(detected_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    fresh(
        detected_at,
        now,
        EVENT_TTL_SECONDS - SOURCE_ONLY_SEND_RESERVE_SECONDS,
    )
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

fn eligible(candidate: &Candidate, stream: &HelixStream, event: &PatchEvent) -> bool {
    candidate.twitch_user_id == stream.user_id
        && candidate.last_stream_id == stream.id
        && !stream.id.is_empty()
        && stream.game_name == "Deadlock"
        && !stream.game_id.is_empty()
        && DateTime::parse_from_rfc3339(&stream.started_at).is_ok_and(|at| at <= event.detected_at)
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

#[async_trait::async_trait]
trait Transport: Send + Sync {
    async fn streams(&self, ids: &[String]) -> Result<Vec<HelixStream>, PatchProcessError>;
    async fn can_send(&self, id: &str, stream_id: &str) -> Result<bool, PatchProcessError>;
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

    async fn can_send(&self, id: &str, stream_id: &str) -> Result<bool, PatchProcessError> {
        let Some(login) = self.authorized_login(id, stream_id).await? else {
            return Ok(false);
        };
        Ok(!self.receiver.suppression.is_muted(&login).await)
    }

    async fn send(
        &self,
        id: &str,
        stream_id: &str,
        message: &str,
        detected_at: DateTime<Utc>,
    ) -> Result<DeliveryResult, PatchProcessError> {
        if !self.can_send(id, stream_id).await? {
            return Ok(DeliveryResult::status("skipped"));
        }
        if !source_only_send_window_open(detected_at, Utc::now()) {
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
                .send_source_only_message(id, message)
                .await
            {
                Ok(SendOutcome::Sent) => DeliveryResult::status("sent"),
                Ok(SendOutcome::Dropped { code, .. }) => DeliveryResult {
                    status: "dropped",
                    drop_code: Some(redact_drop_code(&code)),
                    http_status: None,
                    uncertainty_reason: None,
                },
                Ok(SendOutcome::HttpError { status, .. }) => DeliveryResult {
                    status: "uncertain",
                    drop_code: None,
                    http_status: Some(status as i16),
                    uncertainty_reason: Some("http_error"),
                },
                Err(error) => DeliveryResult {
                    status: "uncertain",
                    drop_code: None,
                    http_status: None,
                    uncertainty_reason: Some(send_error_reason(&error)),
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

fn database_error(error: sqlx::Error) -> PatchProcessError {
    let sqlstate = error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned());
    tracing::error!(sqlstate = ?sqlstate, "Patch announcement persistence failed");
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
        }
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

#[derive(Clone)]
pub struct PatchReceiverExt(pub Option<Arc<PatchReceiver>>);

#[cfg(test)]
async fn process(
    pool: &PgPool,
    transport: &dyn Transport,
    event: &PatchEvent,
) -> Result<PatchProcessOutcome, PatchProcessError> {
    process_inner(pool, transport, event, Arc::new(Mutex::new(HashSet::new()))).await
}

async fn process_inner(
    pool: &PgPool,
    transport: &dyn Transport,
    event: &PatchEvent,
    attempted: Arc<Mutex<HashSet<(String, String)>>>,
) -> Result<PatchProcessOutcome, PatchProcessError> {
    if !event.validate(Utc::now())? {
        sqlx::query(
            "UPDATE twitch_patch_announcement_deliveries SET status='skipped' \
             WHERE event_id=$1 AND status='pending'",
        )
        .bind(&event.event_id)
        .execute(pool)
        .await
        .map_err(database_error)?;
        return Ok(PatchProcessOutcome::SkippedExpired);
    }
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
        let ids: Vec<_> = candidates
            .iter()
            .map(|candidate| candidate.twitch_user_id.clone())
            .collect();
        let streams = if ids.is_empty() {
            vec![]
        } else {
            transport.streams(&ids).await?
        };
        if !event.validate(Utc::now())? {
            return Ok(PatchProcessOutcome::SkippedExpired);
        }
        let mut selected = HashSet::new();
        for candidate in &candidates {
            for stream in &streams {
                let current_recipient =
                    eligible(candidate, stream, event) && !selected.contains(&stream.user_id);
                let can_send = if current_recipient {
                    transport
                        .can_send(&stream.user_id, &stream.id)
                        .await
                        .inspect_err(|error| {
                            log_database_context(error, &event.event_id, &stream.user_id);
                        })?
                } else {
                    false
                };
                if current_recipient && can_send {
                    selected.insert(stream.user_id.clone());
                    sqlx::query(
                        "INSERT INTO twitch_patch_announcement_deliveries (event_id, broadcaster_id, stream_id) \
                         VALUES ($1,$2,$3)",
                    )
                    .bind(&event.event_id)
                    .bind(&stream.user_id)
                    .bind(&stream.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| database_error_for(error, &event.event_id, &stream.user_id))?;
                }
            }
        }
        tracing::info!(event_id = %event.event_id, recipients = selected.len(), "Patch announcement snapshot saved");
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
            sqlx::query(
                "UPDATE twitch_patch_announcement_deliveries SET status='skipped' \
                 WHERE event_id=$1 AND status='pending'",
            )
            .bind(&event.event_id)
            .execute(pool)
            .await
            .map_err(database_error)?;
            return Ok(PatchProcessOutcome::SkippedExpired);
        }
        let live = transport.streams(std::slice::from_ref(&id)).await?;
        let still_live = live.iter().any(|stream| {
            stream.user_id == id
                && stream.id == stream_id
                && stream.game_name == "Deadlock"
                && !stream.game_id.is_empty()
        });
        let can_send = if still_live {
            transport
                .can_send(&id, &stream_id)
                .await
                .inspect_err(|error| {
                    log_database_context(error, &event.event_id, &id);
                })?
        } else {
            false
        };
        let send_budget = source_only_send_window_open(detected_at, Utc::now());
        let status = if can_send && send_budget {
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
            "UPDATE twitch_patch_announcement_deliveries SET status=$3, attempted_at=CASE WHEN $3='attempted' THEN now() ELSE attempted_at END, uncertainty_reason=CASE WHEN $3='attempted' THEN $4 ELSE NULL END \
             WHERE event_id=$1 AND broadcaster_id=$2 AND status='pending'",
        )
        .bind(&event.event_id)
        .bind(&id)
        .bind(status)
        .bind(&attempt_token)
        .execute(pool)
        .await
        .map_err(|error| database_error_for(error, &event.event_id, &id))?
        .rows_affected()
            == 1;
        if claimed && can_send && send_budget {
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

pub async fn handler(
    auth: AuthLevel,
    receiver: Option<Extension<PatchReceiverExt>>,
    Json(event): Json<PatchEvent>,
) -> Result<Json<PatchProcessOutcome>, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let receiver = receiver
        .and_then(|Extension(ext)| ext.0)
        .ok_or_else(ApiError::unavailable)?;
    receiver.process(&event).await.map(Json).map_err(Into::into)
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
    fn reserves_a_bounded_source_only_send_window_before_event_expiry() {
        let detected_at = DateTime::from_timestamp(1_000, 0).unwrap();
        assert!(source_only_send_window_open(
            detected_at,
            detected_at + chrono::Duration::seconds(75)
        ));
        assert!(!source_only_send_window_open(
            detected_at,
            detected_at + chrono::Duration::seconds(76)
        ));
        assert!(!source_only_send_window_open(
            detected_at,
            detected_at - chrono::Duration::seconds(1)
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
        assert!(eligible(&candidate, &stream, &event));
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
            assert!(!eligible(&candidate, &wrong, &event));
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
}

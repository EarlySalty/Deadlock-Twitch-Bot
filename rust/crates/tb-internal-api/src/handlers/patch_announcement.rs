use std::{collections::HashSet, error::Error, fmt, sync::Arc, time::Duration};

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

#[derive(Debug)]
pub enum PatchProcessError {
    Invalid(&'static str),
    Unavailable,
    Database,
}

impl fmt::Display for PatchProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "{message}"),
            Self::Unavailable => f.write_str("patch transport unavailable"),
            Self::Database => f.write_str("patch persistence unavailable"),
        }
    }
}

impl Error for PatchProcessError {}

impl From<PatchProcessError> for ApiError {
    fn from(error: PatchProcessError) -> Self {
        match error {
            PatchProcessError::Invalid(message) => ApiError::bad_request(message),
            PatchProcessError::Unavailable | PatchProcessError::Database => ApiError::unavailable(),
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
    last_stream_id: Option<String>,
    last_seen_at: Option<String>,
}

fn eligible(
    candidate: &Candidate,
    stream: &HelixStream,
    event: &PatchEvent,
    now: DateTime<Utc>,
) -> bool {
    candidate.twitch_user_id == stream.user_id
        && !stream.id.is_empty()
        && candidate.last_stream_id.as_deref() == Some(stream.id.as_str())
        && stream.game_name == "Deadlock"
        && !stream.game_id.is_empty()
        && DateTime::parse_from_rfc3339(&stream.started_at).is_ok_and(|at| at <= event.detected_at)
        && candidate
            .last_seen_at
            .as_deref()
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .is_some_and(|at| fresh(at.with_timezone(&Utc), now, SNAPSHOT_TTL_SECONDS))
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
    ) -> Result<&'static str, PatchProcessError>;
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
    ) -> Result<&'static str, PatchProcessError> {
        if !self.can_send(id, stream_id).await? {
            return Ok("skipped");
        }
        Ok(
            match self
                .receiver
                .chat
                .send_source_only_message(id, message)
                .await
            {
                Ok(SendOutcome::Sent) => "sent",
                Ok(SendOutcome::Dropped { .. }) => "dropped",
                Ok(SendOutcome::HttpError { .. }) | Err(_) => "uncertain",
            },
        )
    }
}

fn database_error(_: sqlx::Error) -> PatchProcessError {
    tracing::error!("Patch announcement persistence failed");
    PatchProcessError::Database
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
        tokio::time::timeout(
            Duration::from_secs(EVENT_TTL_SECONDS as u64),
            process(&self.pool, &LiveTransport { receiver: self }, event),
        )
        .await
        .map_err(|_| PatchProcessError::Unavailable)?
    }
}

#[derive(Clone)]
pub struct PatchReceiverExt(pub Option<Arc<PatchReceiver>>);

async fn process(
    pool: &PgPool,
    transport: &dyn Transport,
    event: &PatchEvent,
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
        let candidates = sqlx::query_as::<_, Candidate>(
            "SELECT DISTINCT p.twitch_user_id, l.last_stream_id, l.last_seen_at \
             FROM twitch_streamers_partner_state p \
             JOIN twitch_live_state l ON l.twitch_user_id = p.twitch_user_id \
             WHERE p.is_partner_active = 1 AND COALESCE(p.manual_partner_opt_out, 0) = 0 \
               AND l.is_live = 1 AND l.last_game = 'Deadlock' \
               AND COALESCE(p.twitch_user_id, '') <> '' \
               AND EXISTS (SELECT 1 FROM twitch_raid_auth ra \
                           WHERE ra.twitch_user_id = p.twitch_user_id \
                             AND ra.needs_reauth IS FALSE \
                             AND 'channel:bot' = ANY(regexp_split_to_array( \
                                 COALESCE(ra.scopes, ''), '[[:space:],]+')))",
        )
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
        let now = Utc::now();
        if !event.validate(now)? {
            return Ok(PatchProcessOutcome::SkippedExpired);
        }
        let mut selected = HashSet::new();
        for candidate in &candidates {
            for stream in &streams {
                if eligible(candidate, stream, event, now)
                    && !selected.contains(&stream.user_id)
                    && transport.can_send(&stream.user_id, &stream.id).await?
                {
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
                    .map_err(database_error)?;
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
        let can_send = still_live && transport.can_send(&id, &stream_id).await?;
        let status = if can_send { "attempted" } else { "skipped" };
        let claimed = sqlx::query(
            "UPDATE twitch_patch_announcement_deliveries SET status=$3, attempted_at=now() \
             WHERE event_id=$1 AND broadcaster_id=$2 AND status='pending'",
        )
        .bind(&event.event_id)
        .bind(&id)
        .bind(status)
        .execute(pool)
        .await
        .map_err(database_error)?
        .rows_affected()
            == 1;
        if claimed && can_send && fresh(detected_at, Utc::now(), EVENT_TTL_SECONDS) {
            let outcome = transport.send(&id, &stream_id, &message).await?;
            sqlx::query(
                "UPDATE twitch_patch_announcement_deliveries SET status=$3 \
                 WHERE event_id=$1 AND broadcaster_id=$2 AND status='attempted'",
            )
            .bind(&event.event_id)
            .bind(&id)
            .bind(outcome)
            .execute(pool)
            .await
            .map_err(database_error)?;
            tracing::info!(event_id=%event.event_id, broadcaster_id=%id, outcome, "Patch announcement delivery");
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
        let now = Utc::now();
        let candidate = Candidate {
            twitch_user_id: "42".into(),
            last_stream_id: Some("s1".into()),
            last_seen_at: Some(now.to_rfc3339()),
        };
        let stream = HelixStream {
            id: "s1".into(),
            user_id: "42".into(),
            game_id: "g1".into(),
            game_name: "Deadlock".into(),
            started_at: (event.detected_at - chrono::Duration::hours(1)).to_rfc3339(),
            ..Default::default()
        };
        assert!(eligible(&candidate, &stream, &event, now));
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
            assert!(!eligible(&candidate, &wrong, &event, now));
        }
        let stale = Candidate {
            last_seen_at: Some((now - chrono::Duration::seconds(121)).to_rfc3339()),
            ..candidate
        };
        assert!(!eligible(&stale, &stream, &event, now));
    }
}

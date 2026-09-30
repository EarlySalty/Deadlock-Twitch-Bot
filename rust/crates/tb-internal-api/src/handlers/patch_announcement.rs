//! A new patch signal freezes the currently live Deadlock partner set once.
//! Receipts survive restarts. An ambiguous Twitch send is NEVER retried.

use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{extract::Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_chat::{ChatApi, SendOutcome};
use tb_http_core::{ApiError, AuthLevel};
use tb_transport_twitch::{HelixClient, HelixStream};

const EVENT_TTL_SECONDS: i64 = 120;
const SNAPSHOT_TTL_SECONDS: i64 = 120;

type GuardedChat = (
    Arc<dyn ChatApi>,
    Arc<dyn tb_chat::promos::OutboundSuppressionCheck>,
);

#[derive(Clone)]
pub struct PatchChatExt(pub Option<GuardedChat>);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchEvent {
    pub event_id: String,
    pub source_url: String,
    pub detected_at: DateTime<Utc>,
    pub discord_url: String,
    pub message: String,
}

impl PatchEvent {
    fn validate(&self, now: DateTime<Utc>) -> Result<(), ApiError> {
        if self.event_id.len() != 64
            || !self
                .event_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !fresh(self.detected_at, now, EVENT_TTL_SECONDS)
        {
            return Err(ApiError::bad_request("invalid or expired patch event"));
        }
        let source = url::Url::parse(&self.source_url)
            .map_err(|_| ApiError::bad_request("invalid patch source"))?;
        if source.scheme() != "https"
            || !matches!(
                source.host_str(),
                Some("forums.playdeadlock.com" | "steamcommunity.com" | "store.steampowered.com")
            )
            || !source.username().is_empty()
            || source.password().is_some()
            || source.port_or_known_default() != Some(443)
            || self.source_url.len() > 512
        {
            return Err(ApiError::bad_request("invalid patch source"));
        }
        let discord = url::Url::parse(&self.discord_url)
            .map_err(|_| ApiError::bad_request("invalid Discord channel"))?;
        let parts: Vec<_> = discord.path().split('/').collect();
        if discord.scheme() != "https"
            || discord.host_str() != Some("discord.com")
            || !discord.username().is_empty()
            || discord.password().is_some()
            || discord.port_or_known_default() != Some(443)
            || discord.query().is_some()
            || discord.fragment().is_some()
            || parts.len() != 4
            || parts[1] != "channels"
            || !parts[2..]
                .iter()
                .all(|p| p.parse::<u64>().is_ok_and(|n| n > 0))
        {
            return Err(ApiError::bad_request("invalid Discord channel"));
        }
        if self.message.chars().count() > 450
            || self.message.trim().is_empty()
            || self.message.chars().any(char::is_control)
            || !self.message.ends_with(&self.discord_url)
            || self.message.contains('@')
        {
            return Err(ApiError::bad_request("invalid patch announcement"));
        }
        Ok(())
    }
}

fn fresh(at: DateTime<Utc>, now: DateTime<Utc>, ttl: i64) -> bool {
    at <= now && now.signed_duration_since(at) <= chrono::Duration::seconds(ttl)
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
    async fn streams(&self, ids: &[String]) -> Result<Vec<HelixStream>, ()>;
    async fn send(&self, id: &str, message: &str) -> &'static str;
}

struct LiveTransport {
    helix: HelixClient,
    chat: Arc<dyn ChatApi>,
    suppression: Arc<dyn tb_chat::promos::OutboundSuppressionCheck>,
    pool: PgPool,
}

#[async_trait::async_trait]
impl Transport for LiveTransport {
    async fn streams(&self, ids: &[String]) -> Result<Vec<HelixStream>, ()> {
        self.helix
            .get_streams_by_user_ids(ids, None)
            .await
            .map_err(|_| ())
    }

    async fn send(&self, id: &str, message: &str) -> &'static str {
        // Resolve by stable identity, and fail closed on disconnect/opt-out or
        // a policy read failure. Share the existing timeout and promo guards.
        let login = sqlx::query_scalar::<_, String>(
            "SELECT twitch_login FROM twitch_streamers_partner_state \
             WHERE twitch_user_id=$1 AND is_partner_active=1 \
               AND COALESCE(manual_partner_opt_out,0)=0 LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await;
        let Ok(Some(login)) = login else {
            return "skipped";
        };
        if self.suppression.is_muted(&login).await {
            return "skipped";
        }
        match self.chat.send_source_only_message(id, message).await {
            Ok(SendOutcome::Sent) => "sent",
            Ok(SendOutcome::Dropped { .. }) => "dropped",
            // Once attempted, retrying could duplicate a message accepted before
            // a network timeout. Prefer one missed announcement over chat spam.
            Ok(SendOutcome::HttpError { .. }) | Err(_) => "uncertain",
        }
    }
}

fn database_error(_: sqlx::Error) -> ApiError {
    // No SQL bodies, credentials, upstream responses or community data in logs.
    tracing::error!("Patch announcement persistence failed");
    ApiError::unavailable()
}

async fn process(
    pool: &PgPool,
    transport: &dyn Transport,
    event: &PatchEvent,
) -> Result<serde_json::Value, ApiError> {
    event.validate(Utc::now())?;
    // The event row is committed together with its immutable recipient snapshot.
    // Concurrent retries serialize at the unique insert; only the winner selects.
    let mut tx = pool.begin().await.map_err(database_error)?;
    let inserted = sqlx::query(
        "INSERT INTO twitch_patch_announcements (event_id, source_url, detected_at, message) \
         VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(&event.event_id)
    .bind(&event.source_url)
    .bind(event.detected_at)
    .bind(&event.message)
    .execute(&mut *tx)
    .await
    .map_err(database_error)?
    .rows_affected()
        == 1;
    if inserted {
        let candidates = sqlx::query_as::<_, Candidate>(
            "SELECT DISTINCT p.twitch_user_id, l.last_stream_id, l.last_seen_at \
             FROM twitch_streamers_partner_state p \
             JOIN twitch_live_state l ON l.twitch_user_id = p.twitch_user_id \
             WHERE p.is_partner_active = 1 AND l.is_live = 1 \
               AND l.last_game = 'Deadlock' AND COALESCE(p.twitch_user_id, '') <> ''",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(database_error)?;
        let ids: Vec<_> = candidates
            .iter()
            .map(|c| c.twitch_user_id.clone())
            .collect();
        let streams = if ids.is_empty() {
            vec![]
        } else {
            transport
                .streams(&ids)
                .await
                .map_err(|_| ApiError::unavailable())?
        };
        let now = Utc::now();
        event.validate(now)?;
        let mut selected = HashSet::new();
        for candidate in &candidates {
            for stream in &streams {
                if eligible(candidate, stream, event, now)
                    && selected.insert(stream.user_id.clone())
                {
                    sqlx::query(
                        "INSERT INTO twitch_patch_announcement_deliveries (event_id, broadcaster_id, stream_id) \
                         VALUES ($1,$2,$3)",
                    ).bind(&event.event_id).bind(&stream.user_id).bind(&stream.id)
                        .execute(&mut *tx).await.map_err(database_error)?;
                }
            }
        }
        tracing::info!(event_id = %event.event_id, recipients = selected.len(), "Patch announcement snapshot saved");
    }
    tx.commit().await.map_err(database_error)?;
    // A changed payload must not rewrite the original timestamp or recipient set.
    let saved = sqlx::query_as::<_, (String, DateTime<Utc>, String)>(
        "SELECT source_url, detected_at, message FROM twitch_patch_announcements WHERE event_id=$1",
    )
    .bind(&event.event_id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)?;
    let Some((source, detected_at, message)) = saved else {
        return Ok(serde_json::json!({"status":"duplicate_source"}));
    };
    if source != event.source_url
        || detected_at.timestamp_micros() != event.detected_at.timestamp_micros()
        || message != event.message
    {
        return Err(ApiError::bad_request(
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
            break;
        }
        // Only frozen recipients, still in the same live Deadlock session now.
        // An API failure is not an offline result and must not create sends.
        let live = transport
            .streams(std::slice::from_ref(&id))
            .await
            .map_err(|_| ApiError::unavailable())?;
        let still_live = live.iter().any(|s| {
            s.user_id == id
                && s.id == stream_id
                && s.game_name == "Deadlock"
                && !s.game_id.is_empty()
        });
        let status = if still_live { "attempted" } else { "skipped" };
        // Commit the claim BEFORE the non-idempotent Twitch request. No lease
        // reclaim on attempted/uncertain: a crash must not lead to a duplicate.
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
        if claimed && still_live && fresh(detected_at, Utc::now(), EVENT_TTL_SECONDS) {
            let outcome = transport.send(&id, &message).await;
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
        "SELECT status, count(*) FROM twitch_patch_announcement_deliveries WHERE event_id=$1 GROUP BY status",
    ).bind(&event.event_id).fetch_all(pool).await.map_err(database_error)?;
    Ok(serde_json::json!({"status":"processed", "deliveries":counts}))
}

pub async fn handler(
    auth: AuthLevel,
    Extension(pool): Extension<PgPool>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    chat: Option<Extension<PatchChatExt>>,
    Json(event): Json<PatchEvent>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    event.validate(Utc::now())?;
    let (chat, suppression) = chat
        .and_then(|ext| ext.0 .0)
        .ok_or_else(ApiError::unavailable)?;
    let helix = helix.as_ref().clone().ok_or_else(ApiError::unavailable)?;
    let transport = LiveTransport {
        helix,
        chat,
        suppression,
        pool: pool.clone(),
    };
    // Bound the request; expired signals are never queued for a later stream.
    tokio::time::timeout(
        Duration::from_secs(EVENT_TTL_SECONDS as u64),
        process(&pool, &transport, &event),
    )
    .await
    .map_err(|_| ApiError::unavailable())?
    .map(Json)
}

#[cfg(test)]
#[path = "patch_announcement_tests.rs"]
mod integration_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> PatchEvent {
        let discord_url = "https://discord.com/channels/123/456".to_string();
        PatchEvent {
            event_id: "a".repeat(64),
            source_url: "https://forums.playdeadlock.com/posts/123/".into(),
            detected_at: Utc::now(),
            discord_url: discord_url.clone(),
            message: format!(
                "🔥 Neuer Deadlock-Patch ist da! Deutsche Patchnotes im Discord: {discord_url}"
            ),
        }
    }

    #[test]
    fn validates_only_fresh_signals_and_safe_short_chat() {
        let mut e = event();
        assert!(e.validate(Utc::now()).is_ok());
        e.detected_at -= chrono::Duration::seconds(121);
        assert!(e.validate(Utc::now()).is_err());
        e.detected_at = Utc::now() + chrono::Duration::seconds(10);
        assert!(e.validate(Utc::now()).is_err());
        e = event();
        e.message = format!("@everyone {}", e.discord_url);
        assert!(e.validate(Utc::now()).is_err());
        e = event();
        e.message = format!("{} {}", "x".repeat(451), e.discord_url);
        assert!(e.validate(Utc::now()).is_err());
    }

    #[test]
    fn rejects_lookalike_urls_credentials_and_extra_fields() {
        for source in [
            "https://forums.playdeadlock.com.evil.test/posts/1/",
            "https://token@forums.playdeadlock.com/posts/1/",
            "http://forums.playdeadlock.com/posts/1/",
        ] {
            let mut e = event();
            e.source_url = source.into();
            assert!(e.validate(Utc::now()).is_err());
        }
        for discord in [
            "https://discord.com.evil.test/channels/123/456",
            "https://discord.com/channels/123/456?token=x",
            "https://discord.com/channels/0/456",
        ] {
            let mut e = event();
            e.discord_url = discord.into();
            e.message = discord.into();
            assert!(e.validate(Utc::now()).is_err());
        }
        let mut value = serde_json::to_value(event()).unwrap();
        value["patchnotes"] = serde_json::json!("not forwarded");
        assert!(serde_json::from_value::<PatchEvent>(value).is_err());
    }

    #[test]
    fn only_current_deadlock_stream_with_matching_stable_identity() {
        let e = event();
        let now = Utc::now();
        let c = Candidate {
            twitch_user_id: "42".into(),
            last_stream_id: Some("s1".into()),
            last_seen_at: Some(now.to_rfc3339()),
        };
        let s = HelixStream {
            id: "s1".into(),
            user_id: "42".into(),
            game_id: "g1".into(),
            game_name: "Deadlock".into(),
            started_at: (e.detected_at - chrono::Duration::hours(1)).to_rfc3339(),
            ..Default::default()
        };
        assert!(eligible(&c, &s, &e, now));
        for wrong in [
            HelixStream {
                user_id: "99".into(),
                ..s.clone()
            },
            HelixStream {
                game_name: "Other game".into(),
                ..s.clone()
            },
            HelixStream {
                id: "s2".into(),
                ..s.clone()
            },
            HelixStream {
                started_at: (e.detected_at + chrono::Duration::seconds(1)).to_rfc3339(),
                ..s.clone()
            },
        ] {
            assert!(!eligible(&c, &wrong, &e, now));
        }
        let stale = Candidate {
            last_seen_at: Some((now - chrono::Duration::seconds(121)).to_rfc3339()),
            ..c
        };
        assert!(!eligible(&stale, &s, &e, now));
    }
}

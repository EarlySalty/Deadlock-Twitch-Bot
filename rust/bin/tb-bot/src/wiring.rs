//! Composition-Root des Monitorings: Adapter zwischen tb-monitoring-Ports
//! und den Transport-Crates (Hexagonal — die Ports kennen kein Helix).

use std::sync::Arc;

use serde_json::Value;
use tb_monitoring::poller::source::{ChannelInfo, ChannelInfoSource, SourceError, StreamSource};
use tb_monitoring::sessions::tracker::FollowerCountSource;
use tb_monitoring::{
    AnnouncementTransport, EventSubHooks, RemoteSubscription, StreamSnapshot, SubscriptionManager,
    SubscriptionTransport, VodPreviewSource,
};
use tb_transport_discord::{BrokerRelay, DiscordBackend, EditRichMessage, SendRichMessage};
use tb_transport_twitch::eventsub::CreateOutcome;
use tb_transport_twitch::{HelixClient, HelixStream};

/// Helix-Adapter für den Subscription-Port (App-Token, Core-Subs).
pub struct HelixSubscriptionTransport {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl SubscriptionTransport for HelixSubscriptionTransport {
    async fn create(
        &self,
        sub_type: &str,
        version: &str,
        condition: &Value,
        callback: &str,
        secret: &str,
    ) -> Result<bool, SourceError> {
        let outcome = self
            .helix
            .create_eventsub_webhook_subscription(
                sub_type, version, condition, callback, secret, None,
            )
            .await?;
        Ok(outcome == CreateOutcome::AlreadyExists)
    }

    async fn list(&self) -> Result<Vec<RemoteSubscription>, SourceError> {
        let subs = self.helix.list_eventsub_subscriptions(None).await?;
        Ok(subs
            .into_iter()
            .map(|sub| RemoteSubscription {
                id: sub.id,
                sub_type: sub.sub_type,
                status: sub.status,
                callback: sub.transport.callback,
                // Ziel-Extraktion wie Python `_eventsub_target_user_id`:
                // channel.raid trägt `to_broadcaster_user_id`.
                broadcaster_user_id: [
                    "broadcaster_user_id",
                    "to_broadcaster_user_id",
                    "from_broadcaster_user_id",
                    "user_id",
                ]
                .iter()
                .find_map(|key| sub.condition.get(key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string),
            })
            .collect())
    }

    async fn delete(&self, id: &str) -> Result<(), SourceError> {
        self.helix.delete_eventsub_subscription(id).await?;
        Ok(())
    }
}

/// Interim-Hooks ohne Raid-Anbindung (Fallback, wenn `DB_MASTER_KEY_V1`
/// fehlt): Go-Live registriert die stream.offline-Subscription, alle
/// Raid-/Score-Hooks bleiben Noop. Voll verdrahtet: `RaidEventSubHooks`.
pub struct SubscriptionEventSubHooks {
    pub manager: Arc<SubscriptionManager>,
}

#[async_trait::async_trait]
impl EventSubHooks for SubscriptionEventSubHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
    }
}

/// Helix-Adapter für den Stream-Port des Poll-Loops.
pub struct HelixStreamSource {
    pub helix: HelixClient,
}

fn to_snapshot(stream: HelixStream) -> StreamSnapshot {
    StreamSnapshot {
        id: Some(stream.id).filter(|i| !i.is_empty()),
        user_login: stream.user_login,
        user_name: stream.user_name,
        title: stream.title,
        game_name: stream.game_name,
        language: stream.language,
        viewer_count: stream.viewer_count as i32,
        is_mature: stream.is_mature,
        tags: stream.tags.unwrap_or_default(),
        started_at: Some(stream.started_at).filter(|s| !s.is_empty()),
        thumbnail_url: None,
    }
}

#[async_trait::async_trait]
impl StreamSource for HelixStreamSource {
    async fn streams_by_logins(
        &self,
        logins: &[String],
        language: Option<&str>,
    ) -> Result<Vec<StreamSnapshot>, SourceError> {
        let streams = self.helix.get_streams_by_logins(logins, language).await?;
        Ok(streams.into_iter().map(to_snapshot).collect())
    }

    async fn streams_by_category(
        &self,
        category_id: &str,
        language: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StreamSnapshot>, SourceError> {
        let streams = self
            .helix
            .get_streams_by_category(category_id, language, limit)
            .await?;
        Ok(streams.into_iter().map(to_snapshot).collect())
    }

    async fn category_id(&self, game_name: &str) -> Result<Option<String>, SourceError> {
        Ok(self.helix.search_category_id(game_name).await?)
    }
}

#[async_trait::async_trait]
impl ChannelInfoSource for HelixStreamSource {
    async fn channel_info(&self, broadcaster_id: &str) -> Result<Option<ChannelInfo>, SourceError> {
        let info = self.helix.get_channel_information(broadcaster_id).await?;
        Ok(info.map(|i| ChannelInfo {
            title: Some(i.title).filter(|t| !t.trim().is_empty()),
            game_name: Some(i.game_name).filter(|g| !g.trim().is_empty()),
        }))
    }
}

/// Helix-Adapter für Follower-Zahlen (best-effort, App-Token).
pub struct HelixFollowerSource {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl FollowerCountSource for HelixFollowerSource {
    async fn follower_total(&self, twitch_user_id: Option<&str>, _login: &str) -> Option<i32> {
        let user_id = twitch_user_id?.trim();
        if user_id.is_empty() {
            return None;
        }
        match self.helix.get_followers_total(user_id).await {
            Ok(total) => total.map(|t| t as i32),
            Err(error) => {
                tracing::debug!(%error, "Follower-Total nicht abrufbar");
                None
            }
        }
    }
}

/// Helix-Adapter fürs VOD-Vorschaubild des Offline-Embeds.
pub struct HelixVodPreview {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl VodPreviewSource for HelixVodPreview {
    async fn latest_preview(&self, twitch_user_id: Option<&str>, login: &str) -> Option<String> {
        // Login-Fallback wie Python: kein/nicht-numerischer user_id → via /users auflösen.
        let mut user_id = twitch_user_id
            .map(str::trim)
            .filter(|u| !u.is_empty() && u.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string);
        if user_id.is_none() {
            let users = self.helix.get_users(&[login]).await.ok()?;
            user_id = users.get(&login.to_lowercase()).map(|u| u.id.clone());
        }
        self.helix
            .get_latest_vod_thumbnail(&user_id?)
            .await
            .ok()
            .flatten()
    }
}

/// Broker-Adapter für den Announcement-Port — inklusive des
/// `view_resolver_unavailable`-Fallbacks auf einen einfachen Link-Button
/// (Python `_send_live_announcement_via_broker`).
pub struct BrokerAnnouncementTransport {
    pub relay: BrokerRelay,
}

fn fallback_view_spec(view_spec: &Value) -> Option<Value> {
    let referral_url = view_spec.get("referral_url")?.as_str()?.trim().to_string();
    if referral_url.is_empty() {
        return None;
    }
    let label = view_spec
        .get("button_label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .unwrap_or("Auf Twitch ansehen");
    Some(serde_json::json!({
        "type": "link_button",
        "label": label,
        "url": referral_url,
    }))
}

fn is_view_resolver_unavailable(error: &tb_transport_discord::DiscordError) -> bool {
    match error {
        tb_transport_discord::DiscordError::BrokerError { body, .. } => {
            body.contains("view_resolver_unavailable")
        }
        _ => false,
    }
}

#[async_trait::async_trait]
impl AnnouncementTransport for BrokerAnnouncementTransport {
    async fn send(
        &self,
        channel_id: i64,
        content: Option<String>,
        embed: Value,
        allowed_role_ids: Vec<i64>,
        view_spec: Option<Value>,
    ) -> Result<String, SourceError> {
        let payload = SendRichMessage {
            channel_id,
            content: content.clone(),
            embed: embed.clone(),
            allowed_role_ids: allowed_role_ids.clone(),
            view_spec: view_spec.clone(),
        };
        match self.relay.send_rich_message(payload).await {
            Ok(result) => Ok(result.result.message_id),
            Err(error) if is_view_resolver_unavailable(&error) => {
                let fallback = view_spec.as_ref().and_then(fallback_view_spec);
                let Some(fallback) = fallback else {
                    return Err(Box::new(error));
                };
                tracing::warn!(
                    channel_id,
                    "Broker-Tracking-View nicht verfügbar — Fallback auf Link-Button"
                );
                let result = self
                    .relay
                    .send_rich_message(SendRichMessage {
                        channel_id,
                        content,
                        embed,
                        allowed_role_ids,
                        view_spec: Some(fallback),
                    })
                    .await?;
                Ok(result.result.message_id)
            }
            Err(error) => Err(Box::new(error)),
        }
    }

    async fn edit(
        &self,
        channel_id: i64,
        message_id: String,
        content: Option<String>,
        embed: Value,
        view_spec: Option<Value>,
    ) -> Result<(), SourceError> {
        self.relay
            .edit_rich_message(EditRichMessage {
                channel_id,
                message_id,
                content,
                embed,
                view_spec,
            })
            .await?;
        Ok(())
    }
}

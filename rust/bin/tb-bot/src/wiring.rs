//! Composition-Root des Monitorings: Adapter zwischen tb-monitoring-Ports
//! und den Transport-Crates (Hexagonal — die Ports kennen kein Helix).

use std::sync::Arc;

use serde_json::Value;
use sqlx::PgPool;
use tb_monitoring::poller::source::{ChannelInfo, ChannelInfoSource, SourceError, StreamSource};
use tb_monitoring::sessions::tracker::FollowerCountSource;
use tb_monitoring::{
    AnnouncementTransport, EventSubHooks, LivePingRoleProvider, RemoteSubscription, StreamSnapshot,
    SubscriptionManager, SubscriptionTransport, VodPreviewSource,
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
        bearer_override: Option<&str>,
    ) -> Result<bool, SourceError> {
        let outcome = self
            .helix
            .create_eventsub_webhook_subscription(
                sub_type,
                version,
                condition,
                callback,
                secret,
                bearer_override,
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
        user_id: stream.user_id,
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

    /// Speist das Poll-Tick-Circuit-Breaker-Gate (engine.rs): im App-Auth-
    /// Cooldown (invalid_client → 15min, B18-3) überspringt der Tick Helix-
    /// Requests, statt sie weiter ins offene Messer laufen zu lassen.
    /// `HelixClient::is_auth_blocked` ist synchron + lock-frei.
    fn is_auth_blocked(&self) -> bool {
        self.helix.is_auth_blocked()
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

/// Helix-Adapter für den Highlight-Clipper: liefert User-Info + Archiv-VODs
/// (implementiert [`tb_highlight::twitch_vod::TwitchVodApi`] über den App-Token).
pub struct HelixVodSource {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl tb_highlight::twitch_vod::TwitchVodApi for HelixVodSource {
    async fn get_user_info(&self, login: &str) -> Option<serde_json::Value> {
        let users = self.helix.get_users(&[login]).await.ok()?;
        let user = users.get(&login.to_lowercase())?;
        Some(serde_json::json!({ "id": user.id.clone() }))
    }

    async fn get_archive_videos(&self, channel_id: &str, first: u32) -> Vec<serde_json::Value> {
        self.helix
            .get_archive_videos(channel_id, first)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|v| {
                serde_json::json!({
                    "id": v.id,
                    "created_at": v.created_at,
                    "duration": v.duration,
                })
            })
            .collect()
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

/// Auto-Anlage der Live-Ping-Rolle via Master-Broker (Port von Python
/// `embeds_mixin._ensure_live_ping_role`). Wird vom [`BrokerAnnouncementSink`]
/// aufgerufen, wenn ein Partner mit aktiviertem Live-Ping aber ohne gesetzte
/// `live_ping_role_id` live geht: legt eine mentionable Rolle „<login> ist live"
/// an und persistiert die ID am aktiven Partner.
pub struct LivePingRoleAuto {
    pub relay: Arc<BrokerRelay>,
    pub pool: PgPool,
    pub guild_id: u64,
}

/// Rollenname wie Python `_sanitize_live_ping_role_name`
/// (`embeds_mixin.py:79-86`): nur `[A-Za-z0-9 _-]` behalten, Whitespace
/// kollabieren, leer → "STREAMER", Suffix " ist live", auf 100 Zeichen cappen.
fn sanitize_live_ping_role_name(login: &str) -> String {
    let filtered: String = login
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_' || *c == '-')
        .collect();
    let mut cleaned = filtered.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        cleaned = "STREAMER".to_string();
    }
    let name = format!("{cleaned} ist live");
    name.chars().take(100).collect()
}

#[async_trait::async_trait]
impl LivePingRoleProvider for LivePingRoleAuto {
    async fn ensure_role(&self, login: &str, _twitch_user_id: &str) -> Option<i64> {
        let name = sanitize_live_ping_role_name(login);
        let reason = format!("Auto-created Twitch live ping role for {login}");
        let role_id = match self
            .relay
            .create_role(self.guild_id, &name, true, &reason)
            .await
        {
            Ok(id) if id > 0 => id,
            Ok(_) => {
                tracing::warn!(login, "Live-Ping-Rolle angelegt, aber role_id == 0");
                return None;
            }
            Err(error) => {
                tracing::warn!(%error, login, "Auto-Anlage der Live-Ping-Rolle fehlgeschlagen");
                return None;
            }
        };

        let role_id_i64 = role_id as i64;
        let updated = sqlx::query(
            r#"
            UPDATE twitch_partners
               SET live_ping_role_id = $1
             WHERE id = (
                   SELECT id FROM twitch_partners
                    WHERE LOWER(twitch_login) = LOWER($2)
                      AND status = 'active'
                    ORDER BY id DESC
                    LIMIT 1
             )
            "#,
        )
        .bind(role_id_i64)
        .bind(login)
        .execute(&self.pool)
        .await;
        match updated {
            Ok(result) if result.rows_affected() > 0 => {}
            Ok(_) => {
                tracing::warn!(
                    login,
                    "Live-Ping-Rolle angelegt, aber kein aktiver Partner zum Persistieren gefunden"
                );
            }
            Err(error) => {
                tracing::warn!(%error, login, "Live-Ping-role_id konnte nicht persistiert werden");
            }
        }

        Some(role_id_i64)
    }
}

//! Composition-Root des Monitorings: Adapter zwischen tb-monitoring-Ports
//! und den Transport-Crates (Hexagonal — die Ports kennen kein Helix).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;
use sqlx::PgPool;
use tb_engagement::crew_review_store::CrewReviewStore;
use tb_monitoring::poller::source::{ChannelInfo, ChannelInfoSource, SourceError, StreamSource};
use tb_monitoring::sessions::tracker::{FollowerCountSource, FollowerFetch};
use tb_monitoring::{
    AnnouncementTransport, ChannelProfileSource, EventSubHooks, LivePingRoleProvider,
    RemoteSubscription, StreamSnapshot, SubscriptionCreateError, SubscriptionManager,
    SubscriptionTransport, VodPreviewSource,
};
use tb_transport_discord::{BrokerRelay, DiscordBackend, EditRichMessage, SendRichMessage};
use tb_transport_twitch::eventsub::{CreateOutcome, EventSubCreateError};
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
    ) -> Result<bool, SubscriptionCreateError> {
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
            .await
            .map_err(map_eventsub_create_error)?;
        Ok(outcome == CreateOutcome::AlreadyExists)
    }

    async fn refresh_auth(
        &self,
        _broadcaster_id: &str,
        _login: &str,
        bearer_override: Option<&str>,
    ) -> Result<(), SourceError> {
        if bearer_override.is_none() {
            self.helix.invalidate_app_token().await;
        }
        Ok(())
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

fn map_eventsub_create_error(error: EventSubCreateError) -> SubscriptionCreateError {
    match error {
        EventSubCreateError::Status {
            status,
            retry_after,
            body,
        } => SubscriptionCreateError::http_status(status, retry_after, body),
        EventSubCreateError::Helix(helix_error) => SubscriptionCreateError::transport(helix_error),
    }
}

/// Interim-Hooks ohne Raid-Anbindung (Fallback, wenn `DB_MASTER_KEY_V1`
/// fehlt): Go-Live registriert die stream.offline-Subscription, alle
/// Raid-/Score-Hooks bleiben Noop. Voll verdrahtet: `RaidEventSubHooks`.
pub struct SubscriptionEventSubHooks {
    pub manager: Arc<SubscriptionManager>,
    pub pool: PgPool,
}

#[async_trait::async_trait]
impl EventSubHooks for SubscriptionEventSubHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
    }

    async fn on_stream_offline(&self, _twitch_user_id: &str, login: Option<&str>) {
        if let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) {
            if let Err(error) = CrewReviewStore::new(self.pool.clone())
                .close_channel_session(login, "stream_offline", chrono::Utc::now())
                .await
            {
                tracing::warn!(
                    login,
                    %error,
                    "Ricky-Review: Fallback-EventSub-Offline-Close fehlgeschlagen"
                );
            }
        }
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
        thumbnail_url: Some(stream.thumbnail_url)
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty()),
        profile_image_url: None,
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

/// Liefert einen User-Token mit `moderator:read:followers` für den
/// Follower-Total-Abruf (P1.7). Python (`sessions_mixin.py:907-926`) bevorzugt
/// den zentralen Bot-Token, sofern dieser den Scope trägt; ohne Scope/Token
/// fällt der Abruf auf den App-Token-Pfad zurück (`None`).
#[async_trait::async_trait]
pub trait FollowerTokenSource: Send + Sync {
    /// `Some(token)` = User-Token mit `moderator:read:followers`, der die echte
    /// `total`-Zahl liefert; `None` = App-Token-Pfad (Twitch antwortet ohne
    /// Scope und es kommt `None` zurück).
    async fn moderator_followers_token(&self) -> Option<String>;

    /// P1.19-Fallback: per-Streamer OAuth-Token (Broadcaster moderiert seinen
    /// eigenen Kanal → `total` lesbar). Wird genutzt, wenn der Bot-Token-Pfad
    /// keine Zahl liefert (kein Token/Scope oder 403). Default `None` für
    /// Quellen, die keinen Streamer-Token auflösen können.
    async fn streamer_followers_token(&self, _twitch_user_id: &str) -> Option<String> {
        None
    }
}

/// Bot-Token-Quelle für den Follower-Total-Abruf: reicht den zentralen
/// Bot-User-Token nur durch, wenn er `moderator:read:followers` trägt (P1.7,
/// Python `sessions_mixin.py:925`). Nutzt denselben `BotTokenManager` wie der
/// Chat-Pfad — KEIN zweiter Refresher.
///
/// P2.46/P2.50: Die Scope-/Token-Normalisierung läuft über den zentralen
/// [`tb_raid::resolve_bot_oauth_context`]-Resolver (`oauth:`-Strip, lowercased
/// Scope-Set, Gating via [`BotOAuthContext::can_read_followers`]); diese Quelle
/// implementiert dafür [`BotOAuthSource`] über den `BotTokenManager`.
pub struct BotFollowerTokenSource {
    pub token_manager: Arc<tb_chat::token::BotTokenManager>,
}

#[async_trait::async_trait]
impl tb_raid::BotOAuthSource for BotFollowerTokenSource {
    async fn raw_bot_oauth(&self) -> (Option<String>, Option<String>, Vec<String>) {
        // Token best-effort (Fehler/leer → None); der Resolver strippt `oauth:`.
        let token = match self.token_manager.access_token().await {
            Ok(t) if !t.trim().is_empty() => Some(t),
            _ => None,
        };
        let bot_id = {
            let id = self.token_manager.bot_user_id().await;
            (!id.trim().is_empty()).then_some(id)
        };
        let scopes = self.token_manager.scopes().await;
        (token, bot_id, scopes)
    }
}

#[async_trait::async_trait]
impl FollowerTokenSource for BotFollowerTokenSource {
    async fn moderator_followers_token(&self) -> Option<String> {
        // P2.50: Gating exakt wie Python (`bot_can_read_followers`,
        // followers.py:271-280) — Bot-Token nur, wenn `moderator:read:followers`
        // vorhanden ODER die Scope-Liste unbekannt/leer ist.
        let ctx = tb_raid::resolve_bot_oauth_context(Some(self)).await;
        if ctx.can_read_followers() {
            ctx.token
        } else {
            None
        }
    }
}

/// P1.19: Komponiert eine Bot-Token-Quelle mit dem per-Streamer
/// OAuth-Token-Fallback (Raid-Auth). Delegiert `moderator_followers_token` an
/// die innere Quelle und ergänzt `streamer_followers_token` über den
/// [`tb_raid::TokenProvider`] (Broadcaster moderiert seinen eigenen Kanal →
/// `total` lesbar). So bleibt [`BotFollowerTokenSource`] unverändert und der
/// Fallback ist in der Composition-Root opt-in einschaltbar.
// WIRING-TODO(P1.19): main.rs HelixFollowerSource.token_source mit dieser
// Quelle umwickeln (inner = BotFollowerTokenSource, token_provider = der
// bereits gebootete TokenProvider), damit der Streamer-Token-Fallback live ist.
// Bis dahin: nur Bot-/App-Token-Pfad → dead_code.
#[allow(dead_code)]
pub struct FollowerTokenSourceWithStreamerFallback {
    pub inner: Arc<dyn FollowerTokenSource>,
    pub token_provider: Arc<tb_raid::TokenProvider>,
}

#[async_trait::async_trait]
impl FollowerTokenSource for FollowerTokenSourceWithStreamerFallback {
    async fn moderator_followers_token(&self) -> Option<String> {
        self.inner.moderator_followers_token().await
    }

    async fn streamer_followers_token(&self, twitch_user_id: &str) -> Option<String> {
        let user_id = twitch_user_id.trim();
        if user_id.is_empty() {
            return None;
        }
        // Broadcaster moderiert seinen eigenen Kanal → kein expliziter
        // Scope-Filter (Python `get_valid_token_for_login`, sessions_mixin.py:1053).
        // unrestricted: der Follower-Read soll auch bei deaktivierten Raids greifen.
        match self
            .token_provider
            .get_valid_token_unrestricted(user_id, chrono::Utc::now())
            .await
        {
            Ok(Some(token)) if !token.trim().is_empty() => Some(token),
            Ok(_) => None,
            Err(error) => {
                tracing::debug!(%error, user_id, "Streamer-Follower-Token-Lookup fehlgeschlagen");
                None
            }
        }
    }
}

/// Helix-Adapter für Follower-Zahlen. Mit verdrahteter [`FollowerTokenSource`]
/// (P1.7) läuft der Abruf über den Bot-User-Token mit
/// `moderator:read:followers` und liefert die echte Zahl; ohne Quelle bleibt es
/// beim App-Token-Pfad (best-effort, Twitch liefert dann meist `None`).
pub struct HelixFollowerSource {
    pub helix: HelixClient,
    /// `None` = App-Token-Pfad wie vor P1.7.
    pub token_source: Option<Arc<dyn FollowerTokenSource>>,
    /// P3.9: Once-only-WARN, wenn der Abruf vom Bot-Token auf den
    /// Legacy-/Streamer-Token zurückfällt. `None` = kein Operator-Signal.
    pub scope_fallback_warner: Option<Arc<tb_raid::ScopeFallbackWarner>>,
}

impl HelixFollowerSource {
    /// Ein einzelner `/channels/followers`-Abruf mit dem gegebenen Token.
    /// Transport-/JSON-Fehler werden als Diagnose-Code weitergereicht.
    async fn fetch_total(&self, user_id: &str, token: Option<&str>) -> FollowerFetch {
        match self.helix.get_followers_total(user_id, token).await {
            Ok(fetch) => FollowerFetch {
                total: fetch.total.map(|t| t as i32),
                http_status: fetch.http_status.map(|s| s as i64),
                error_code: fetch.error_code,
            },
            Err(error) => {
                tracing::debug!(%error, with_token = token.is_some(), "Follower-Total nicht abrufbar");
                FollowerFetch {
                    total: None,
                    http_status: None,
                    error_code: Some("transport_error".to_string()),
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl FollowerCountSource for HelixFollowerSource {
    async fn follower_total(&self, twitch_user_id: Option<&str>, _login: &str) -> FollowerFetch {
        let Some(user_id) = twitch_user_id.map(str::trim) else {
            return FollowerFetch::default();
        };
        if user_id.is_empty() {
            return FollowerFetch::default();
        }
        let source = self.token_source.as_ref();

        // 1. Bevorzugt Bot-Token mit moderator:read:followers (P1.7).
        let bot_token = match source {
            Some(s) => s.moderator_followers_token().await,
            None => None,
        };
        let bot = self.fetch_total(user_id, bot_token.as_deref()).await;
        if bot.total.is_some() {
            // P3.9: Bot-Token-Pfad hat geliefert → Fallback-WARN re-armieren,
            // damit ein späterer Rückfall wieder einmal warnt.
            if let Some(warner) = self.scope_fallback_warner.as_ref() {
                warner.clear("followers", user_id);
            }
            return bot;
        }
        let mut last = bot;

        // 2. P2.48/P1.19-Fallback: liefert der Bot-/App-Token keine Zahl (kein
        //    Scope, 403, oder Twitch antwortet ohne `total`), den per-Streamer
        //    OAuth-Token versuchen (Broadcaster moderiert sich selbst).
        //    Der letzte Fetch wird zurückgegeben, damit Diagnosefelder erhalten
        //    bleiben (Python `_fetch_followers_total_safe`:1048-1144).
        if let Some(s) = source {
            if let Some(streamer_token) = s.streamer_followers_token(user_id).await {
                // P3.9: Once-only-WARN — wir nutzen den Legacy-Broadcaster-Token
                // statt des Bot-Tokens (Python followers.py:153).
                if let Some(warner) = self.scope_fallback_warner.as_ref() {
                    warner.warn_once("followers", user_id);
                }
                let streamer = self.fetch_total(user_id, Some(&streamer_token)).await;
                if streamer.total.is_some() {
                    return streamer;
                }
                last = streamer;
            }
        }
        last
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

/// Helix-Adapter fürs Kanalprofilbild der Components-V2-Ansage.
pub struct HelixChannelProfile {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl ChannelProfileSource for HelixChannelProfile {
    async fn profile_image_url(&self, login: &str) -> Option<String> {
        let users = self.helix.get_users(&[login]).await.ok()?;
        users
            .get(&login.trim().to_lowercase())
            .and_then(|user| user.profile_image_url.as_deref())
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string)
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
        components: Option<Value>,
        allowed_role_ids: Vec<i64>,
        view_spec: Option<Value>,
    ) -> Result<String, SourceError> {
        let payload = SendRichMessage {
            channel_id,
            content: content.clone(),
            embed: embed.clone(),
            components: components.clone(),
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
                        components,
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
        components: Option<Value>,
        view_spec: Option<Value>,
    ) -> Result<(), SourceError> {
        self.relay
            .edit_rich_message(EditRichMessage {
                channel_id,
                message_id,
                content,
                embed,
                components,
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

static ROLE_CREATE_UNSUPPORTED: AtomicBool = AtomicBool::new(false);

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
        let existing = self
            .relay
            .find_role_by_name(self.guild_id, &name)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, login, "Bestehende Live-Ping-Rolle nicht ladbar");
                None
            });
        let role_id = if let Some(role_id) = existing {
            role_id
        } else if ROLE_CREATE_UNSUPPORTED.load(Ordering::Relaxed) {
            return None;
        } else {
            match self
                .relay
                .create_role(self.guild_id, &name, true, &reason)
                .await
            {
                Ok(id) if id > 0 => id,
                Ok(_) => {
                    tracing::warn!(login, "Live-Ping-Rolle angelegt, aber role_id == 0");
                    return None;
                }
                Err(tb_transport_discord::DiscordError::BrokerError { status: 404, .. }) => {
                    ROLE_CREATE_UNSUPPORTED.store(true, Ordering::Relaxed);
                    tracing::warn!(
                        "Live-Ping-Rollenanlage vom laufenden Broker nicht unterstützt; \
                     weitere Anlageversuche bis zum Neustart ausgesetzt"
                    );
                    return None;
                }
                Err(error) => {
                    tracing::warn!(%error, login, "Auto-Anlage der Live-Ping-Rolle fehlgeschlagen");
                    return None;
                }
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

// ─── Tests (P1.19 Follower-Total Bot→Streamer-Fallback) ──────────────────────

#[cfg(test)]
mod follower_fallback_tests {
    use super::*;
    use tb_transport_twitch::HelixConfig;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Stub-Token-Quelle: liefert konfigurierbare Bot-/Streamer-Tokens, um den
    /// Fallback-Pfad ohne echten BotTokenManager/TokenProvider zu testen.
    struct StubTokenSource {
        bot: Option<String>,
        streamer: Option<String>,
    }

    #[async_trait::async_trait]
    impl FollowerTokenSource for StubTokenSource {
        async fn moderator_followers_token(&self) -> Option<String> {
            self.bot.clone()
        }
        async fn streamer_followers_token(&self, _twitch_user_id: &str) -> Option<String> {
            self.streamer.clone()
        }
    }

    async fn helix_at(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "apptok",
                "expires_in": 3600
            })))
            .mount(server)
            .await;
        HelixClient::new(HelixConfig {
            client_id: "cid".into(),
            client_secret: "sec".into(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

    /// Bot-Token liefert keine Zahl (403) → Streamer-Token-Fallback liefert die
    /// echte total. Beweist den P1.19-Fallback statt FOLLOWERS_UNKNOWN.
    #[tokio::test]
    async fn fallback_auf_streamer_token_bei_bot_403() {
        let server = MockServer::start().await;
        let helix = helix_at(&server).await;

        // Bot-Token: 403 (nicht Moderator) → None.
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .and(header("Authorization", "Bearer bottok"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        // Streamer-Token: 200 mit total.
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .and(header("Authorization", "Bearer streamertok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": 4242, "data": []
            })))
            .mount(&server)
            .await;

        let source = HelixFollowerSource {
            helix,
            token_source: Some(Arc::new(StubTokenSource {
                bot: Some("bottok".into()),
                streamer: Some("streamertok".into()),
            })),
            scope_fallback_warner: None,
        };
        let fetch = source.follower_total(Some("42"), "chan").await;
        assert_eq!(
            fetch.total,
            Some(4242),
            "Streamer-Token-Fallback liefert echte Zahl"
        );
        assert_eq!(fetch.http_status, Some(200));
        assert_eq!(fetch.error_code, None);
    }

    /// Bot-Token liefert direkt eine Zahl → kein Streamer-Fallback nötig
    /// (Streamer-Token wäre 500, würde der Fallback ihn anfassen).
    #[tokio::test]
    async fn bot_token_erfolg_ueberspringt_fallback() {
        let server = MockServer::start().await;
        let helix = helix_at(&server).await;

        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .and(header("Authorization", "Bearer bottok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": 7, "data": []
            })))
            .mount(&server)
            .await;
        // Streamer-Pfad würde fehlschlagen — darf nicht erreicht werden.
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .and(header("Authorization", "Bearer streamertok"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let source = HelixFollowerSource {
            helix,
            token_source: Some(Arc::new(StubTokenSource {
                bot: Some("bottok".into()),
                streamer: Some("streamertok".into()),
            })),
            scope_fallback_warner: None,
        };
        let fetch = source.follower_total(Some("42"), "chan").await;
        assert_eq!(fetch.total, Some(7));
        assert_eq!(fetch.http_status, Some(200));
        assert_eq!(fetch.error_code, None);
    }

    /// Kein Token verfügbar (App-Token-Pfad) und Twitch antwortet ohne total →
    /// None, kein Fallback möglich.
    #[tokio::test]
    async fn ohne_tokens_app_pfad_none() {
        let server = MockServer::start().await;
        let helix = helix_at(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let source = HelixFollowerSource {
            helix,
            token_source: Some(Arc::new(StubTokenSource {
                bot: None,
                streamer: None,
            })),
            scope_fallback_warner: None,
        };
        let fetch = source.follower_total(Some("42"), "chan").await;
        assert_eq!(fetch.total, None);
        assert_eq!(fetch.http_status, Some(401));
        assert_eq!(fetch.error_code.as_deref(), Some("unauthorized"));
    }

    /// P3.9: Fällt der Abruf auf den Streamer-/Legacy-Token zurück, feuert der
    /// Once-only-WARN genau einmal je Subject; ein anschließender Bot-Erfolg
    /// re-armiert ihn.
    #[tokio::test]
    async fn legacy_token_fallback_warnt_genau_einmal_und_rearmt() {
        let server = MockServer::start().await;
        let helix = helix_at(&server).await;

        // Bot-Token: 403 → None (kein Scope).
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .and(header("Authorization", "Bearer bottok"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        // Streamer-Token: 200 mit total.
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .and(header("Authorization", "Bearer streamertok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": 99, "data": []
            })))
            .mount(&server)
            .await;

        let warner = Arc::new(tb_raid::ScopeFallbackWarner::new());
        let source = HelixFollowerSource {
            helix,
            token_source: Some(Arc::new(StubTokenSource {
                bot: Some("bottok".into()),
                streamer: Some("streamertok".into()),
            })),
            scope_fallback_warner: Some(warner.clone()),
        };

        // Erster Abruf → Fallback aktiv → WARN gefeuert. Danach meldet ein
        // erneutes warn_once `false` (schon gewarnt).
        let fetch = source.follower_total(Some("42"), "chan").await;
        assert_eq!(fetch.total, Some(99));
        assert!(
            !warner.warn_once("followers", "42"),
            "WARN muss durch den Fallback bereits gefeuert sein"
        );

        // Re-Arm: clear → warn_once meldet wieder `true`.
        warner.clear("followers", "42");
        assert!(warner.warn_once("followers", "42"));
    }
}

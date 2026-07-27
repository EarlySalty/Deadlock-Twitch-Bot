//! Broker-gestützter [`AnnouncementSink`]: Go-Live-Posting + Offline-Edit
//! über den Master-Broker (Python `_send_live_announcement_via_broker` /
//! `_edit_live_announcement_via_broker` + `_build_live_announcement_message`).
//!
//! Bewusste Abweichung: Der Retry-Zustand hält Tracking-Token + Render-Zeit
//! (für stabile Cache-Buster/Tokens über Versuche hinweg), aber nicht den
//! Original-Stream-Payload — beim Retry rendert der aktuelle Tick. Die
//! Live-Ping-Rollen bleiben als dormant Datenmodell erhalten, werden im
//! Live-Announcement-Pfad aber bewusst nicht mehr erwähnt oder angelegt.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::announce::template::{
    build_context, build_offline_components, build_offline_embed, render_announcement,
    sanitize_live_content, AnnouncementConfig, OfflineComponentsContext, TWITCH_VOD_BUTTON_LABEL,
};
use crate::poller::hooks::{
    AnnounceLiveRequest, AnnounceLiveResult, AnnouncementSink, EndAnnouncementOutcome,
    EndAnnouncementRequest,
};
use crate::poller::source::SourceError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnouncementEditOutcome {
    Updated,
    Gone,
}

/// Send-/Edit-Port zum Master-Broker (Adapter über `tb-transport-discord`
/// im Composition-Root; übernimmt auch den `view_resolver_unavailable`-
/// Fallback auf einen einfachen Link-Button).
#[async_trait::async_trait]
pub trait AnnouncementTransport: Send + Sync {
    /// Liefert die Discord-message_id des Postings.
    async fn send(
        &self,
        channel_id: i64,
        content: Option<String>,
        embed: Value,
        components: Option<Value>,
        allowed_role_ids: Vec<i64>,
        view_spec: Option<Value>,
    ) -> Result<String, SourceError>;

    async fn edit(
        &self,
        channel_id: i64,
        message_id: String,
        content: Option<String>,
        embed: Value,
        components: Option<Value>,
        view_spec: Option<Value>,
    ) -> Result<AnnouncementEditOutcome, SourceError>;
}

/// VOD-Vorschaubild fürs Offline-Embed (Helix-Adapter in `tb-bot`).
#[async_trait::async_trait]
pub trait VodPreviewSource: Send + Sync {
    async fn latest_preview(&self, twitch_user_id: Option<&str>, login: &str) -> Option<String>;
}

/// Kanalprofilbild fürs Components-V2-Thumbnail (Helix `/users` in `tb-bot`).
#[async_trait::async_trait]
pub trait ChannelProfileSource: Send + Sync {
    async fn profile_image_url(&self, login: &str) -> Option<String>;
}

/// Auto-Anlage der Live-Ping-Rolle, wenn ein Partner mit `live_ping_enabled`
/// aber ohne `live_ping_role_id` live geht (Python
/// `embeds_mixin._ensure_live_ping_role`). Die konkrete Impl liegt im
/// Composition-Root (`tb-bot`): Discord-Rolle via Master-Broker anlegen und
/// die ID in `twitch_partners.live_ping_role_id` persistieren.
///
/// `tb-monitoring` bleibt damit frei von Discord-/Broker-Wissen — der Port
/// liefert nur die fertige Rollen-ID zurück (`None` = nicht anlegbar).
#[async_trait::async_trait]
pub trait LivePingRoleProvider: Send + Sync {
    async fn ensure_role(&self, login: &str, twitch_user_id: &str) -> Option<i64>;
}

/// Quelle ohne VOD-Vorschau.
pub struct NoVodPreview;

#[async_trait::async_trait]
impl VodPreviewSource for NoVodPreview {
    async fn latest_preview(&self, _twitch_user_id: Option<&str>, _login: &str) -> Option<String> {
        None
    }
}

/// Quelle ohne Profilbild-Lookup.
pub struct NoChannelProfile;

#[async_trait::async_trait]
impl ChannelProfileSource for NoChannelProfile {
    async fn profile_image_url(&self, _login: &str) -> Option<String> {
        None
    }
}

/// Statische Announcement-Einstellungen (Env/Runtime-Config).
#[derive(Debug, Clone)]
pub struct AnnouncementSettings {
    /// Discord-Kanal der Go-Live-Postings.
    pub notify_channel_id: i64,
    /// Dormant: Rollen-Mentions werden im Live-Announce-Pfad nicht mehr gesendet.
    pub alert_mention: Option<String>,
    /// Referral-Code für die Twitch-URL (`?ref=`).
    pub ref_code: Option<String>,
    pub target_game: String,
}

struct RetryState {
    tracking_token: String,
    render_now: DateTime<Utc>,
}

struct LiveSyncState {
    /// Bucket des letzten *erfolgreichen* Live-Edits. `None` = es steht kein
    /// aktueller Stand in Discord, der nächste Poll-Tick ist wieder fällig
    /// (Wiedervorlage nach einem Broker-Ausfall).
    bucket: Option<u64>,
    shows_offline: bool,
    /// Seit dem letzten Erfolg gescheiterte Sync-Versuche — nur für die
    /// Nachvollziehbarkeit im Log: eine WARN ohne Folgemeldung wäre sonst
    /// nicht von „endgültig verloren" zu unterscheiden.
    failed_syncs: u32,
}

struct LivePayload {
    content: Option<String>,
    notification_text: String,
    embed: Value,
    components: Option<Value>,
    view_spec: Option<Value>,
}

pub struct BrokerAnnouncementSink {
    transport: Arc<dyn AnnouncementTransport>,
    vod: Arc<dyn VodPreviewSource>,
    profile: Arc<dyn ChannelProfileSource>,
    settings: AnnouncementSettings,
    #[allow(dead_code)]
    live_ping_role_provider: Option<Arc<dyn LivePingRoleProvider>>,
    retry: Mutex<HashMap<String, RetryState>>,
    live_sync: Mutex<HashMap<String, LiveSyncState>>,
}

impl BrokerAnnouncementSink {
    pub fn new(
        transport: Arc<dyn AnnouncementTransport>,
        vod: Arc<dyn VodPreviewSource>,
        profile: Arc<dyn ChannelProfileSource>,
        settings: AnnouncementSettings,
        live_ping_role_provider: Option<Arc<dyn LivePingRoleProvider>>,
    ) -> Self {
        Self {
            transport,
            vod,
            profile,
            settings,
            live_ping_role_provider,
            retry: Mutex::new(HashMap::new()),
            live_sync: Mutex::new(HashMap::new()),
        }
    }

    /// Twitch-URL mit Referral-Parameter (Python `_build_referral_url`).
    fn referral_url(&self, login: &str) -> String {
        let base = format!("https://www.twitch.tv/{}", login.trim());
        match self.settings.ref_code.as_deref().map(str::trim) {
            Some(code) if !code.is_empty() => format!("{base}?ref={code}"),
            _ => base,
        }
    }

    /// Stabiler Tracking-Token. Zuerst der von der DB durchgereichte
    /// `previous_tracking_token` (Carry-forward), sonst deterministisch aus der
    /// Stream-Identität — 1:1 zu Python `_build_live_announcement_tracking_token`:
    /// `sha256(login|stream_id|started_at|title)[:16]`. Über Retries UND Prozess-
    /// Neustarts hinweg stabil → Idempotenz gegen Doppel-Postings (vorher zufälliges
    /// UUID, das im Fenster Send-Timeout/Neustart die Absicherung verlor).
    fn tracking_token(request: &AnnounceLiveRequest) -> String {
        if let Some(prev) = request
            .previous_tracking_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return prev.to_string();
        }
        use sha2::{Digest, Sha256};
        let raw = format!(
            "{}|{}|{}|{}",
            request.login.trim().to_lowercase(),
            request.stream_id.as_deref().unwrap_or("").trim(),
            request.started_at_iso.as_deref().unwrap_or("").trim(),
            request.stream.title.trim(),
        );
        Sha256::digest(raw.as_bytes())
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn live_bucket(render_now: DateTime<Utc>) -> u64 {
        (render_now.timestamp().max(0) / 300) as u64
    }

    fn live_cache_seed(tracking_token: &str, render_now: DateTime<Utc>) -> String {
        format!("{tracking_token}:{}", Self::live_bucket(render_now))
    }

    /// Rollen-ID aus einer Mention wie `<@&123>` (Python `_extract_role_id_from_mention`).
    #[allow(dead_code)]
    fn role_id_from_mention(text: &str) -> Option<i64> {
        let trimmed = text.trim();
        let inner = trimmed.strip_prefix("<@&")?.strip_suffix('>')?;
        inner.parse::<i64>().ok().filter(|id| *id > 0)
    }

    async fn fill_profile_image_url(
        &self,
        login: &str,
        stream: &mut crate::stream::StreamSnapshot,
    ) {
        if stream
            .profile_image_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|url| !url.is_empty())
        {
            return;
        }
        stream.profile_image_url = self
            .profile
            .profile_image_url(login)
            .await
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty());
    }

    async fn render_live_payload(
        &self,
        login: &str,
        request: &AnnounceLiveRequest,
        tracking_token: &str,
        render_now: DateTime<Utc>,
    ) -> LivePayload {
        let config = AnnouncementConfig::default();
        let mut stream = request.stream.clone();
        self.fill_profile_image_url(login, &mut stream).await;

        let referral_url = self.referral_url(login);
        let context = build_context(
            login,
            &stream,
            &referral_url,
            "",
            render_now,
            stream.thumbnail_url.as_deref(),
        );
        let cache_seed = Self::live_cache_seed(tracking_token, render_now);
        let rendered = render_announcement(&config, &context, render_now, Some(&cache_seed));
        let notification_text = sanitize_live_content(&rendered.content);
        let view_spec = rendered.button_enabled.then(|| {
            serde_json::json!({
                "type": "twitch_live_tracking",
                "streamer_login": login,
                "tracking_token": tracking_token,
                "referral_url": referral_url,
                "button_label": rendered.button_label,
            })
        });

        LivePayload {
            content: Some(notification_text.clone()).filter(|c| !c.is_empty()),
            notification_text,
            embed: rendered.embed,
            components: rendered.components,
            view_spec,
        }
    }

    async fn sync_live_announcement_at(
        &self,
        request: AnnounceLiveRequest,
        render_now: DateTime<Utc>,
    ) -> EndAnnouncementOutcome {
        let login = request.login.to_lowercase();
        let Some(message_id) = request
            .previous_message_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
        else {
            return EndAnnouncementOutcome::Failed;
        };
        let bucket = Self::live_bucket(render_now);
        let should_edit = {
            let live_sync = self.live_sync.lock().expect("live sync lock");
            live_sync
                .get(&login)
                .map(|state| state.shows_offline || state.bucket != Some(bucket))
                .unwrap_or(true)
        };
        if !should_edit {
            return EndAnnouncementOutcome::Failed;
        }

        let tracking_token = Self::tracking_token(&request);
        let payload = self
            .render_live_payload(&login, &request, &tracking_token, render_now)
            .await;
        let discord_message_id = message_id.clone();
        match self
            .transport
            .edit(
                self.settings.notify_channel_id,
                message_id,
                payload.content,
                payload.embed,
                payload.components,
                payload.view_spec,
            )
            .await
        {
            Ok(AnnouncementEditOutcome::Updated) => {
                let recovered_after = {
                    let mut live_sync = self.live_sync.lock().expect("live sync lock");
                    let previous = live_sync.get(&login).map_or(0, |state| state.failed_syncs);
                    live_sync.insert(
                        login.clone(),
                        LiveSyncState {
                            bucket: Some(bucket),
                            shows_offline: false,
                            failed_syncs: 0,
                        },
                    );
                    previous
                };
                if recovered_after > 0 {
                    tracing::info!(
                        login,
                        discord_message_id = %discord_message_id,
                        fehlversuche = recovered_after,
                        "Live-Sync nach Broker-Ausfall nachgeholt"
                    );
                }
                EndAnnouncementOutcome::Updated
            }
            // Endgültig: Die Nachricht ist gelöscht, weitere Versuche sind
            // sinnlos. Die Engine verwirft daraufhin die message_id.
            Ok(AnnouncementEditOutcome::Gone) => {
                tracing::info!(
                    login,
                    discord_message_id = %discord_message_id,
                    "Ankündigung existiert nicht mehr, Live-Sync eingestellt"
                );
                EndAnnouncementOutcome::Gone
            }
            Err(error) => {
                let failed_syncs = {
                    let mut live_sync = self.live_sync.lock().expect("live sync lock");
                    let state = live_sync.entry(login.clone()).or_insert(LiveSyncState {
                        bucket: None,
                        shows_offline: false,
                        failed_syncs: 0,
                    });
                    // Kein aktueller Stand in Discord → nächster Tick ist fällig.
                    state.bucket = None;
                    state.failed_syncs = state.failed_syncs.saturating_add(1);
                    state.failed_syncs
                };
                tracing::warn!(
                    %error,
                    login,
                    discord_message_id = %discord_message_id,
                    fehlversuche = failed_syncs,
                    "Live-Sync via Broker fehlgeschlagen, nächster Poll-Tick holt nach"
                );
                EndAnnouncementOutcome::Failed
            }
        }
    }
}

#[async_trait::async_trait]
impl AnnouncementSink for BrokerAnnouncementSink {
    fn ready(&self) -> bool {
        self.settings.notify_channel_id > 0
    }

    async fn announce_live(&self, request: AnnounceLiveRequest) -> Option<AnnounceLiveResult> {
        let login = request.login.to_lowercase();
        let (tracking_token, render_now) = {
            let retry = self.retry.lock().expect("retry lock");
            match retry.get(&login) {
                Some(state) => (state.tracking_token.clone(), state.render_now),
                None => (Self::tracking_token(&request), Utc::now()),
            }
        };
        let bucket = Self::live_bucket(render_now);
        let payload = self
            .render_live_payload(&login, &request, &tracking_token, render_now)
            .await;

        let send_result = self
            .transport
            .send(
                self.settings.notify_channel_id,
                payload.content.clone(),
                payload.embed,
                payload.components,
                Vec::new(),
                payload.view_spec.clone(),
            )
            .await;
        match send_result {
            Ok(message_id) if !message_id.trim().is_empty() => {
                let message_id = message_id.trim().to_string();
                tracing::info!(login, discord_message_id = %message_id, "Go-Live-Posting via Broker gesendet");
                self.retry.lock().expect("retry lock").remove(&login);
                self.live_sync.lock().expect("live sync lock").insert(
                    login,
                    LiveSyncState {
                        bucket: Some(bucket),
                        shows_offline: false,
                        failed_syncs: 0,
                    },
                );
                Some(AnnounceLiveResult {
                    message_id,
                    tracking_token: payload.view_spec.is_some().then_some(tracking_token),
                    notification_text: payload.notification_text,
                })
            }
            Ok(_) => None,
            Err(error) => {
                tracing::warn!(%error, login, "Go-Live-Posting via Broker fehlgeschlagen");
                self.retry.lock().expect("retry lock").insert(
                    login,
                    RetryState {
                        tracking_token,
                        render_now,
                    },
                );
                None
            }
        }
    }

    async fn end_announcement(&self, request: EndAnnouncementRequest) -> EndAnnouncementOutcome {
        let login = request.login.to_lowercase();
        if self
            .live_sync
            .lock()
            .expect("live sync lock")
            .get(&login)
            .is_some_and(|state| state.shows_offline)
        {
            return EndAnnouncementOutcome::Failed;
        }
        let now = Utc::now();
        let avatar_url = self.profile.profile_image_url(&login).await;
        let preview = self
            .vod
            .latest_preview(request.twitch_user_id.as_deref(), &login)
            .await;
        let preview_image_url = preview
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .or_else(|| {
                avatar_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
            });
        let embed = build_offline_embed(
            &request.display_name,
            request.last_title.as_deref(),
            request.last_game.as_deref(),
            preview_image_url,
            &self.settings.target_game,
            now,
        );
        let components = build_offline_components(OfflineComponentsContext {
            display_name: &request.display_name,
            last_title: request.last_title.as_deref(),
            last_game: request.last_game.as_deref(),
            preview_image_url,
            channel_avatar_url: avatar_url.as_deref(),
            target_game: &self.settings.target_game,
            started_at: request
                .started_at_iso
                .as_deref()
                .and_then(crate::stream::parse_dt_utc),
            now,
        });
        let referral_url = self.referral_url(&login);
        let view_spec = serde_json::json!({
            "type": "link_button",
            "label": TWITCH_VOD_BUTTON_LABEL,
            "url": referral_url,
        });
        let discord_message_id = request.message_id.clone();
        match self
            .transport
            .edit(
                self.settings.notify_channel_id,
                request.message_id,
                None,
                embed,
                Some(components),
                Some(view_spec),
            )
            .await
        {
            Ok(AnnouncementEditOutcome::Updated) => {
                tracing::info!(login, discord_message_id = %discord_message_id, "Announcement via Broker auf beendet editiert");
                let bucket = Self::live_bucket(now);
                let mut live_sync = self.live_sync.lock().expect("live sync lock");
                let state = live_sync.entry(login).or_insert(LiveSyncState {
                    bucket: Some(bucket),
                    shows_offline: false,
                    failed_syncs: 0,
                });
                state.shows_offline = true;
                EndAnnouncementOutcome::Updated
            }
            Ok(AnnouncementEditOutcome::Gone) => {
                tracing::info!(login, discord_message_id = %discord_message_id, "Announcement existiert nicht mehr");
                EndAnnouncementOutcome::Gone
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    login,
                    discord_message_id = %discord_message_id,
                    "Offline-Edit via Broker fehlgeschlagen, nächster Poll-Tick holt nach"
                );
                EndAnnouncementOutcome::Failed
            }
        }
    }

    async fn sync_live_announcement(&self, request: AnnounceLiveRequest) -> EndAnnouncementOutcome {
        self.sync_live_announcement_at(request, Utc::now()).await
    }

    async fn on_stream_not_live(&self, login: &str) {
        // Bewusst nur `retry` verwerfen, NICHT `live_sync`: on_stream_not_live
        // feuert im Poller bei JEDEM Offline-Tick (engine `!is_live`). Würde hier
        // das shows_offline-Flag gelöscht, editierte end_announcement das
        // Offline-Embed jeden Tick neu (inkl. Helix-Avatar/VOD-Calls). Ein neuer
        // Stream überschreibt den live_sync-Zustand ohnehin via announce_live,
        // ein Kategoriewechsel via end_announcement.
        self.retry
            .lock()
            .expect("retry lock")
            .remove(&login.to_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::stream::{parse_dt_utc, StreamSnapshot};

    #[test]
    fn live_cache_seed_bucket_rendert_stabile_und_rollende_preview_urls() {
        let config = AnnouncementConfig::default();
        let base = parse_dt_utc("2026-06-09T18:00:00Z").expect("valid time");
        let same_bucket = base + Duration::seconds(299);
        let next_bucket = base + Duration::seconds(300);
        let stream = StreamSnapshot {
            user_login: "drag".to_string(),
            user_name: "Drag".to_string(),
            title: "Ranked Grind".to_string(),
            game_name: "Deadlock".to_string(),
            thumbnail_url: Some("https://cdn/{width}x{height}.jpg".to_string()),
            ..Default::default()
        };
        let context = build_context(
            "drag",
            &stream,
            "https://www.twitch.tv/drag",
            "",
            base,
            stream.thumbnail_url.as_deref(),
        );
        let image_for = |now| {
            let seed = BrokerAnnouncementSink::live_cache_seed("token-1", now);
            render_announcement(&config, &context, now, Some(&seed)).embed["image"]["url"]
                .as_str()
                .expect("image url")
                .to_string()
        };

        assert_eq!(image_for(base), image_for(same_bucket));
        assert_ne!(image_for(base), image_for(next_bucket));
    }
}

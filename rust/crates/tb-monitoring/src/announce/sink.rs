//! Broker-gestützter [`AnnouncementSink`]: Go-Live-Posting + Offline-Edit
//! über den Master-Broker (Python `_send_live_announcement_via_broker` /
//! `_edit_live_announcement_via_broker` + `_build_live_announcement_message`).
//!
//! Bewusste Abweichung: Der Retry-Zustand hält Tracking-Token + Render-Zeit
//! (für stabile Cache-Buster/Tokens über Versuche hinweg), aber nicht den
//! Original-Stream-Payload — beim Retry rendert der aktuelle Tick. Die
//! Live-Ping-Rollen-**Erstellung** braucht das Discord-Gateway und bleibt
//! eine Cutover-Kopplung; verwendet wird die Rollen-ID aus der Partner-Config.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

use crate::announce::template::{
    build_context, build_offline_embed, render_announcement, sanitize_live_content,
    AnnouncementConfig, TWITCH_VOD_BUTTON_LABEL,
};
use crate::poller::hooks::{
    AnnounceLiveRequest, AnnounceLiveResult, AnnouncementSink, EndAnnouncementOutcome,
    EndAnnouncementRequest,
};
use crate::poller::source::SourceError;

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
        allowed_role_ids: Vec<i64>,
        view_spec: Option<Value>,
    ) -> Result<String, SourceError>;

    async fn edit(
        &self,
        channel_id: i64,
        message_id: String,
        content: Option<String>,
        embed: Value,
        view_spec: Option<Value>,
    ) -> Result<(), SourceError>;
}

/// VOD-Vorschaubild fürs Offline-Embed (Helix-Adapter in `tb-bot`).
#[async_trait::async_trait]
pub trait VodPreviewSource: Send + Sync {
    async fn latest_preview(&self, twitch_user_id: Option<&str>, login: &str) -> Option<String>;
}

/// Quelle ohne VOD-Vorschau.
pub struct NoVodPreview;

#[async_trait::async_trait]
impl VodPreviewSource for NoVodPreview {
    async fn latest_preview(&self, _twitch_user_id: Option<&str>, _login: &str) -> Option<String> {
        None
    }
}

/// Statische Announcement-Einstellungen (Env/Runtime-Config).
#[derive(Debug, Clone)]
pub struct AnnouncementSettings {
    /// Discord-Kanal der Go-Live-Postings.
    pub notify_channel_id: i64,
    /// Optionale Alert-Mention (z. B. `<@&123>`), wird dem Content vorangestellt.
    pub alert_mention: Option<String>,
    /// Referral-Code für die Twitch-URL (`?ref=`).
    pub ref_code: Option<String>,
    pub target_game: String,
}

/// Lädt die per-Streamer-Announcement-Config (`config_json`).
#[derive(Clone)]
pub struct AnnounceConfigStore {
    pool: PgPool,
}

impl AnnounceConfigStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn load(&self, login: &str) -> AnnouncementConfig {
        let raw: Result<Option<Option<String>>, sqlx::Error> = sqlx::query_scalar(
            "SELECT config_json FROM twitch_live_announcement_configs
              WHERE LOWER(streamer_login) = LOWER($1) LIMIT 1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await;
        let text = match raw {
            Ok(Some(Some(text))) => text,
            Ok(_) => return AnnouncementConfig::default(),
            Err(error) => {
                tracing::debug!(%error, login, "Announcement-Config nicht ladbar — Defaults");
                return AnnouncementConfig::default();
            }
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(parsed) if parsed.is_object() => AnnouncementConfig::from_json(&parsed),
            _ => AnnouncementConfig::default(),
        }
    }
}

struct RetryState {
    tracking_token: String,
    render_now: DateTime<Utc>,
}

pub struct BrokerAnnouncementSink {
    transport: Arc<dyn AnnouncementTransport>,
    configs: AnnounceConfigStore,
    vod: Arc<dyn VodPreviewSource>,
    settings: AnnouncementSettings,
    retry: Mutex<HashMap<String, RetryState>>,
}

impl BrokerAnnouncementSink {
    pub fn new(
        transport: Arc<dyn AnnouncementTransport>,
        configs: AnnounceConfigStore,
        vod: Arc<dyn VodPreviewSource>,
        settings: AnnouncementSettings,
    ) -> Self {
        Self {
            transport,
            configs,
            vod,
            settings,
            retry: Mutex::new(HashMap::new()),
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

    /// Rollen-ID aus einer Mention wie `<@&123>` (Python `_extract_role_id_from_mention`).
    fn role_id_from_mention(text: &str) -> Option<i64> {
        let trimmed = text.trim();
        let inner = trimmed.strip_prefix("<@&")?.strip_suffix('>')?;
        inner.parse::<i64>().ok().filter(|id| *id > 0)
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
        let config = self.configs.load(&login).await;

        // Mention: Live-Ping-Rolle aus der Partner-Config + statische Rollen.
        let streamer_role_id = request
            .entry
            .live_ping_role_id
            .filter(|id| *id > 0 && request.entry.live_ping_enabled);
        let mut mention_text = String::new();
        let mut allowed_role_ids: Vec<i64> = Vec::new();
        for role_id in &config.static_ping_role_ids {
            if !allowed_role_ids.contains(role_id) {
                allowed_role_ids.push(*role_id);
            }
        }
        if config.use_streamer_ping_role {
            if let Some(role_id) = streamer_role_id {
                if !allowed_role_ids.contains(&role_id) {
                    allowed_role_ids.push(role_id);
                }
                mention_text = format!("<@&{role_id}>");
            } else if request.entry.live_ping_enabled {
                // Live-Ping aktiviert, aber keine Rollen-ID gesetzt → der Ping
                // fiele sonst STILL weg. Python (embeds_mixin.py:_ensure_live_ping_role)
                // legte die Rolle beim Go-Live automatisch an; diese Auto-Erstellung
                // ist im Rust-Port noch nicht portiert (braucht Discord-Guild-Write).
                // Bis dahin den Ausfall sichtbar machen, damit die role_id im
                // Dashboard nachgepflegt werden kann statt unbemerkt zu fehlen.
                tracing::warn!(
                    login = %login,
                    "Live-Ping aktiviert, aber live_ping_role_id fehlt — Rollen-Ping übersprungen (role_id im Dashboard setzen)"
                );
            }
        }

        let referral_url = self.referral_url(&login);
        let context = build_context(
            &login,
            &request.stream,
            &referral_url,
            &mention_text,
            render_now,
            request.stream.thumbnail_url.as_deref(),
        );
        let rendered = render_announcement(&config, &context, render_now, Some(&tracking_token));

        let mut content = if rendered.content.is_empty() {
            mention_text.clone()
        } else {
            rendered.content.clone()
        };
        if let Some(alert) = self.settings.alert_mention.as_deref().map(str::trim) {
            if !alert.is_empty() {
                let alert = sanitize_live_content(alert);
                content = format!("{alert} {content}").trim().to_string();
                if let Some(role_id) = Self::role_id_from_mention(&alert) {
                    if !allowed_role_ids.contains(&role_id) {
                        allowed_role_ids.push(role_id);
                    }
                }
            }
        }
        let content = sanitize_live_content(&content);

        // Tracking-Button (Klick-Zählung vor Redirect, Python view_spec).
        let view_spec = rendered.button_enabled.then(|| {
            serde_json::json!({
                "type": "twitch_live_tracking",
                "streamer_login": login,
                "tracking_token": tracking_token,
                "referral_url": referral_url,
                "button_label": rendered.button_label,
            })
        });

        let send_result = self
            .transport
            .send(
                self.settings.notify_channel_id,
                Some(content.clone()).filter(|c| !c.is_empty()),
                rendered.embed,
                allowed_role_ids,
                view_spec.clone(),
            )
            .await;
        match send_result {
            Ok(message_id) if !message_id.trim().is_empty() => {
                self.retry.lock().expect("retry lock").remove(&login);
                Some(AnnounceLiveResult {
                    message_id: message_id.trim().to_string(),
                    tracking_token: view_spec.is_some().then_some(tracking_token),
                    notification_text: content,
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
        let preview = self
            .vod
            .latest_preview(request.twitch_user_id.as_deref(), &login)
            .await;
        let embed = build_offline_embed(
            &request.display_name,
            request.last_title.as_deref(),
            request.last_game.as_deref(),
            preview.as_deref(),
            &self.settings.target_game,
            Utc::now(),
        );
        let referral_url = self.referral_url(&login);
        let view_spec = serde_json::json!({
            "type": "link_button",
            "label": TWITCH_VOD_BUTTON_LABEL,
            "url": referral_url,
        });
        match self
            .transport
            .edit(
                self.settings.notify_channel_id,
                request.message_id,
                None,
                embed,
                Some(view_spec),
            )
            .await
        {
            Ok(()) => EndAnnouncementOutcome::Updated,
            Err(error) => {
                tracing::warn!(%error, login, "Offline-Edit via Broker fehlgeschlagen");
                EndAnnouncementOutcome::Failed
            }
        }
    }

    async fn on_stream_not_live(&self, login: &str) {
        self.retry
            .lock()
            .expect("retry lock")
            .remove(&login.to_lowercase());
    }
}

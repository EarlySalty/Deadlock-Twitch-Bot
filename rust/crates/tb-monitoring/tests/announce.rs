//! Tests des Broker-Announcement-Sinks (Slice 4e): Standard-Rendering,
//! Retry-Token-Stabilität, deaktivierte Rollen-Pings und Offline-Edit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tb_monitoring::poller::hooks::{
    AnnounceLiveRequest, AnnouncementSink, EndAnnouncementOutcome, EndAnnouncementRequest,
};
use tb_monitoring::poller::source::SourceError;
use tb_monitoring::{
    AnnouncementSettings, AnnouncementTransport, BrokerAnnouncementSink, ChannelProfileSource,
    LivePingRoleProvider, NoVodPreview, StreamSnapshot, TrackedEntry,
};

mod support;

macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
}

type SentMessage = (
    i64,
    Option<String>,
    Value,
    Option<Value>,
    Vec<i64>,
    Option<Value>,
);
type EditedMessage = (i64, String, Value, Option<Value>, Option<Value>);

fn stable_cache_buster(seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

#[derive(Default)]
struct StubTransport {
    fail_next_send: AtomicBool,
    sends: Mutex<Vec<SentMessage>>,
    edits: Mutex<Vec<EditedMessage>>,
}

#[async_trait::async_trait]
impl AnnouncementTransport for StubTransport {
    async fn send(
        &self,
        channel_id: i64,
        content: Option<String>,
        embed: Value,
        components: Option<Value>,
        allowed_role_ids: Vec<i64>,
        view_spec: Option<Value>,
    ) -> Result<String, SourceError> {
        if self.fail_next_send.swap(false, Ordering::SeqCst) {
            return Err("broker down".into());
        }
        self.sends.lock().unwrap().push((
            channel_id,
            content,
            embed,
            components,
            allowed_role_ids,
            view_spec,
        ));
        Ok("msg-1".to_string())
    }
    async fn edit(
        &self,
        channel_id: i64,
        message_id: String,
        _content: Option<String>,
        embed: Value,
        components: Option<Value>,
        view_spec: Option<Value>,
    ) -> Result<(), SourceError> {
        self.edits
            .lock()
            .unwrap()
            .push((channel_id, message_id, embed, components, view_spec));
        Ok(())
    }
}

/// Stub des Live-Ping-Rollen-Providers: liefert eine konfigurierte Rollen-ID
/// zurück und merkt sich die `ensure_role`-Aufrufe (login, twitch_user_id),
/// damit der Sink-Pfad (Auto-Anlage beim Go-Live) ohne Discord/Broker prüfbar ist.
#[derive(Default)]
struct StubRoleProvider {
    role_id: Option<i64>,
    calls: Mutex<Vec<(String, String)>>,
}

#[async_trait::async_trait]
impl LivePingRoleProvider for StubRoleProvider {
    async fn ensure_role(&self, login: &str, twitch_user_id: &str) -> Option<i64> {
        self.calls
            .lock()
            .unwrap()
            .push((login.to_string(), twitch_user_id.to_string()));
        self.role_id
    }
}

#[derive(Default)]
struct StubProfileSource {
    avatar_url: Option<String>,
    calls: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl ChannelProfileSource for StubProfileSource {
    async fn profile_image_url(&self, login: &str) -> Option<String> {
        self.calls.lock().unwrap().push(login.to_string());
        self.avatar_url.clone()
    }
}

fn sink_with(transport: Arc<StubTransport>) -> BrokerAnnouncementSink {
    sink_with_provider(transport, None)
}

fn sink_with_provider(
    transport: Arc<StubTransport>,
    live_ping_role_provider: Option<Arc<dyn LivePingRoleProvider>>,
) -> BrokerAnnouncementSink {
    sink_with_profile(
        transport,
        Arc::new(StubProfileSource::default()),
        live_ping_role_provider,
    )
}

fn sink_with_profile(
    transport: Arc<StubTransport>,
    profile: Arc<dyn ChannelProfileSource>,
    live_ping_role_provider: Option<Arc<dyn LivePingRoleProvider>>,
) -> BrokerAnnouncementSink {
    BrokerAnnouncementSink::new(
        transport,
        Arc::new(NoVodPreview),
        profile,
        AnnouncementSettings {
            notify_channel_id: 555,
            alert_mention: Some("<@&777>".to_string()),
            ref_code: Some("dc".to_string()),
            target_game: "Deadlock".to_string(),
        },
        live_ping_role_provider,
    )
}

fn live_request(login: &str) -> AnnounceLiveRequest {
    AnnounceLiveRequest {
        login: login.to_string(),
        entry: TrackedEntry {
            login: login.to_string(),
            twitch_user_id: Some("42".to_string()),
            require_link: false,
            is_partner_active: true,
            is_archived: false,
            is_inactivity_flagged: false,
            discord_user_id: None,
            live_ping_role_id: Some(999),
            live_ping_enabled: true,
        },
        stream: StreamSnapshot {
            id: Some("s-1".to_string()),
            user_login: login.to_string(),
            user_id: "0".to_string(),
            user_name: "Drag".to_string(),
            title: "Ranked Grind".to_string(),
            game_name: "Deadlock".to_string(),
            language: "de".to_string(),
            viewer_count: 42,
            started_at: Some("2026-06-09T17:30:00Z".to_string()),
            thumbnail_url: Some("https://cdn/{width}x{height}.jpg".to_string()),
            profile_image_url: Some("https://avatar/drag.png".to_string()),
            ..Default::default()
        },
        previous_message_id: None,
        previous_tracking_token: None,
        stream_id: Some("s-1".to_string()),
        started_at_iso: Some("2026-06-09T17:30:00+00:00".to_string()),
        active_session_id: Some(1),
        suppress_role_pings: false,
    }
}

/// Live-Ping aktiviert, aber noch KEINE Rollen-ID gesetzt → bleibt im
/// Announcement-Pfad dormant; der Provider darf nicht getriggert werden.
fn live_request_no_role(login: &str) -> AnnounceLiveRequest {
    let mut req = live_request(login);
    req.entry.live_ping_role_id = None;
    req.entry.live_ping_enabled = true;
    req
}

#[tokio::test]
async fn announce_live_default_config_ohne_rollen_ping() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    assert!(sink.ready());
    let result = sink
        .announce_live(live_request("drag"))
        .await
        .expect("gesendet");
    assert_eq!(result.message_id, "msg-1");
    assert!(result.tracking_token.is_some());
    assert!(
        !result.notification_text.contains("<@&"),
        "Live-Announce darf keine Rollen-Mention im Content enthalten"
    );

    let sends = transport.sends.lock().unwrap();
    let (channel_id, content, embed, components, roles, view_spec) = &sends[0];
    assert_eq!(*channel_id, 555);
    assert!(content.is_none(), "Default-Content war nur Rollen-Mention");
    assert_eq!(embed["title"], "Drag ist LIVE in Deadlock!");
    assert_eq!(embed["url"], "https://www.twitch.tv/drag?ref=dc");
    assert_eq!(embed["thumbnail"]["url"], "https://avatar/drag.png");
    let components = components.as_ref().expect("Components V2");
    assert_eq!(components[0]["type"], 17);
    assert_eq!(components[0]["accent_color"], 0xC8A86B);
    assert_eq!(components[0]["components"][0]["type"], 9);
    assert_eq!(
        components[0]["components"][0]["accessory"]["media"]["url"],
        "https://avatar/drag.png"
    );
    assert!(
        roles.is_empty(),
        "allowed_role_ids muss trotz live_ping_role_id und TWITCH_ALERT_MENTION leer bleiben"
    );
    let view = view_spec.as_ref().expect("Tracking-View");
    assert_eq!(view["type"], "twitch_live_tracking");
    assert_eq!(
        view["tracking_token"].as_str(),
        result.tracking_token.as_deref()
    );
}

#[tokio::test]
async fn announce_live_setzt_stream_preview_mit_cache_buster() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    let result = sink
        .announce_live(live_request("drag"))
        .await
        .expect("gesendet");
    let token = result.tracking_token.as_deref().expect("tracking token");
    let expected = format!(
        "https://cdn/1280x720.jpg?rand={}",
        stable_cache_buster(token)
    );

    let sends = transport.sends.lock().unwrap();
    let (_, _, embed, components, _, _) = &sends[0];
    assert_eq!(embed["image"]["url"], expected);
    assert_eq!(
        components.as_ref().expect("components")[0]["components"][3]["items"][0]["media"]["url"],
        expected
    );
}

#[tokio::test]
async fn announce_live_fuellt_fehlendes_profilbild_ueber_profile_source() {
    let transport = Arc::new(StubTransport::default());
    let profile = Arc::new(StubProfileSource {
        avatar_url: Some("https://avatar/from-users.png".to_string()),
        ..Default::default()
    });
    let sink = sink_with_profile(transport.clone(), profile.clone(), None);
    let mut request = live_request("drag");
    request.stream.profile_image_url = None;

    sink.announce_live(request).await.expect("gesendet");

    assert_eq!(
        profile.calls.lock().unwrap().as_slice(),
        &["drag".to_string()],
        "Live-Pfad fuellt das Profilbild per Get-Users-Port"
    );
    let sends = transport.sends.lock().unwrap();
    let (_, _, _, components, _, _) = &sends[0];
    assert_eq!(
        components.as_ref().expect("components")[0]["components"][0]["accessory"]["media"]["url"],
        "https://avatar/from-users.png"
    );
}

#[tokio::test]
async fn announce_live_ohne_stream_thumbnail_setzt_kein_image() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());
    let mut request = live_request("drag");
    request.stream.thumbnail_url = Some(String::new());

    sink.announce_live(request).await.expect("gesendet");

    let sends = transport.sends.lock().unwrap();
    let (_, _, embed, components, _, _) = &sends[0];
    assert!(
        embed.get("image").is_none(),
        "leere thumbnail_url darf kein kaputtes Image-Feld erzeugen"
    );
    let container_children = components.as_ref().expect("components")[0]["components"]
        .as_array()
        .expect("container components");
    assert!(
        !container_children
            .iter()
            .any(|component| component["type"] == 12),
        "ohne Stream-Thumbnail darf keine kaputte MediaGallery entstehen"
    );
}

/// Rollen-Pings bleiben auch dann komplett aus, wenn der alte Streamer-Ping
/// deaktiviert ist und eine dormant `live_ping_role_id` in der Entry steht.
#[tokio::test]
async fn announce_live_ping_disabled_unterdrueckt_streamer_rolle() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    let mut request = live_request("drag");
    request.entry.live_ping_enabled = false; // Rolle gesetzt, aber Ping aus.

    let result = sink.announce_live(request).await.expect("gesendet");

    assert!(
        !result.notification_text.contains("<@&"),
        "auch die Alert-Rollen-Mention muss entfernt bleiben"
    );
    let sends = transport.sends.lock().unwrap();
    let (_, content, _, _, roles, _) = &sends[0];
    assert!(content.is_none(), "Default-Content war nur Rollen-Mention");
    assert!(
        roles.is_empty(),
        "keine Rollen-ID darf in allowed_mentions landen"
    );
}

#[tokio::test]
async fn announce_live_suppress_role_pings_unterdrueckt_alle_rollen_mentions() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    let mut request = live_request("drag");
    request.suppress_role_pings = true;

    let result = sink.announce_live(request).await.expect("gesendet");

    assert!(
        !result.notification_text.contains("<@&"),
        "Cooldown-Reannounce darf keine Rollen-Mention senden"
    );
    let sends = transport.sends.lock().unwrap();
    let (_, content, embed, _, roles, _) = &sends[0];
    assert!(content.is_none());
    assert!(
        embed["image"]["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://cdn/1280x720.jpg?rand=")),
        "Cooldown-Reannounce behaelt das Stream-Preview"
    );
    assert!(
        roles.is_empty(),
        "Cooldown-Reannounce darf keine allowed role IDs setzen"
    );
}

#[tokio::test]
async fn announce_live_ignoriert_config_json_row_und_nutzt_standard_mit_retry_token() {
    let pool = pool_or_skip!("t4e_config_retry");
    sqlx::query(
        r#"INSERT INTO twitch_live_announcement_configs (streamer_login, config_json)
           VALUES ('drag', '{"title_template": "{channel} zockt {game}", "button": {"enabled": false}}')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    // Erster Versuch scheitert → Retry-Zustand mit stabilem Token.
    transport.fail_next_send.store(true, Ordering::SeqCst);
    assert!(sink.announce_live(live_request("drag")).await.is_none());
    let result = sink
        .announce_live(live_request("drag"))
        .await
        .expect("Retry ok");

    let sends = transport.sends.lock().unwrap();
    let (_, _, embed, components, roles, view_spec) = &sends[0];
    assert_eq!(embed["title"], "Drag ist LIVE in Deadlock!");
    assert_eq!(
        components.as_ref().expect("components")[0]["components"][0]["components"][0]["content"],
        "🔴 **LIVE** · Drag spielt Deadlock\n## Ranked Grind"
    );
    assert!(
        roles.is_empty(),
        "Retry-Announce darf keine Rollen-Pings erlauben"
    );
    let view = view_spec.as_ref().expect("Standard-Button bleibt aktiv");
    assert_eq!(view["button_label"], "Auf Twitch ansehen");
    assert_eq!(
        view["tracking_token"].as_str(),
        result.tracking_token.as_deref()
    );
    assert!(
        result.tracking_token.is_some(),
        "Standard-Button liefert Token"
    );
}

#[tokio::test]
async fn end_announcement_editiert_offline_embed() {
    let transport = Arc::new(StubTransport::default());
    let profile = Arc::new(StubProfileSource {
        avatar_url: Some("https://avatar/offline.png".to_string()),
        ..Default::default()
    });
    let sink = sink_with_profile(transport.clone(), profile.clone(), None);

    let outcome = sink
        .end_announcement(EndAnnouncementRequest {
            login: "drag".to_string(),
            display_name: "Drag".to_string(),
            message_id: "msg-7".to_string(),
            previous_tracking_token: None,
            last_title: Some("Letzter Titel".to_string()),
            last_game: Some("Deadlock".to_string()),
            twitch_user_id: Some("42".to_string()),
            started_at_iso: Some("2026-06-09T17:00:00+00:00".to_string()),
        })
        .await;
    assert_eq!(outcome, EndAnnouncementOutcome::Updated);

    assert_eq!(
        profile.calls.lock().unwrap().as_slice(),
        &["drag".to_string()],
        "Offline-Pfad fuellt das Profilbild per Get-Users-Port"
    );
    let edits = transport.edits.lock().unwrap();
    let (channel_id, message_id, embed, components, view_spec) = &edits[0];
    assert_eq!((*channel_id, message_id.as_str()), (555, "msg-7"));
    assert_eq!(embed["title"], "Drag ist OFFLINE");
    assert_eq!(embed["description"], "Letzter Titel");
    let components = components.as_ref().expect("Offline Components V2");
    assert_eq!(components[0]["type"], 17);
    assert_eq!(components[0]["accent_color"], 0x8F7A4E);
    assert_eq!(
        components[0]["components"][0]["components"][0]["content"],
        "💤 **Stream beendet** · Drag\n## Letzter Titel"
    );
    assert_eq!(
        components[0]["components"][0]["accessory"]["media"]["url"],
        "https://avatar/offline.png"
    );
    assert!(
        embed.get("image").is_none(),
        "Offline-Embed bleibt ohne Live-Preview, wenn keine VOD-Vorschau vorhanden ist"
    );
    let view = view_spec.as_ref().expect("Link-Button");
    assert_eq!(view["type"], "link_button");
    assert_eq!(view["url"], "https://www.twitch.tv/drag?ref=dc");
}

#[tokio::test]
async fn live_ping_auto_anlage_wird_im_announce_pfad_nicht_getriggert() {
    let transport = Arc::new(StubTransport::default());
    let provider = Arc::new(StubRoleProvider {
        role_id: Some(424242),
        ..Default::default()
    });
    let sink = sink_with_provider(transport.clone(), Some(provider.clone()));

    let result = sink
        .announce_live(live_request_no_role("drag"))
        .await
        .expect("gesendet");

    let calls = provider.calls.lock().unwrap();
    assert!(
        calls.is_empty(),
        "Live-Ping-Provider darf im Announce-Pfad nicht mehr aufgerufen werden"
    );
    assert!(
        !result.notification_text.contains("<@&"),
        "Provider-Rolle darf nicht als Mention gerendert werden"
    );
    let sends = transport.sends.lock().unwrap();
    let (_, _, _, _, roles, _) = &sends[0];
    assert!(roles.is_empty());
}

#[tokio::test]
async fn live_ping_ohne_role_id_bleibt_ohne_rollen_ping() {
    let transport = Arc::new(StubTransport::default());
    let provider = Arc::new(StubRoleProvider {
        role_id: None,
        ..Default::default()
    });
    let sink = sink_with_provider(transport.clone(), Some(provider.clone()));

    let result = sink
        .announce_live(live_request_no_role("drag"))
        .await
        .expect("gesendet");

    assert!(provider.calls.lock().unwrap().is_empty());
    assert!(
        !result.notification_text.contains("<@&"),
        "keine Alert-, statische oder Streamer-Rollen-Mention"
    );
    let sends = transport.sends.lock().unwrap();
    let (_, _, _, _, roles, _) = &sends[0];
    assert!(roles.is_empty());
}

//! Tests des Broker-Announcement-Sinks (Slice 4e): Standard-Rendering,
//! Retry-Token-Stabilität, Rollen-Ping und Offline-Edit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tb_monitoring::poller::hooks::{
    AnnounceLiveRequest, AnnouncementSink, EndAnnouncementOutcome, EndAnnouncementRequest,
};
use tb_monitoring::poller::source::SourceError;
use tb_monitoring::{
    AnnouncementSettings, AnnouncementTransport, BrokerAnnouncementSink, LivePingRoleProvider,
    NoVodPreview, StreamSnapshot, TrackedEntry,
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

type SentMessage = (i64, Option<String>, Value, Vec<i64>, Option<Value>);
type EditedMessage = (i64, String, Value, Option<Value>);

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
        allowed_role_ids: Vec<i64>,
        view_spec: Option<Value>,
    ) -> Result<String, SourceError> {
        if self.fail_next_send.swap(false, Ordering::SeqCst) {
            return Err("broker down".into());
        }
        self.sends
            .lock()
            .unwrap()
            .push((channel_id, content, embed, allowed_role_ids, view_spec));
        Ok("msg-1".to_string())
    }
    async fn edit(
        &self,
        channel_id: i64,
        message_id: String,
        _content: Option<String>,
        embed: Value,
        view_spec: Option<Value>,
    ) -> Result<(), SourceError> {
        self.edits
            .lock()
            .unwrap()
            .push((channel_id, message_id, embed, view_spec));
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

fn sink_with(transport: Arc<StubTransport>) -> BrokerAnnouncementSink {
    sink_with_provider(transport, None)
}

fn sink_with_provider(
    transport: Arc<StubTransport>,
    live_ping_role_provider: Option<Arc<dyn LivePingRoleProvider>>,
) -> BrokerAnnouncementSink {
    BrokerAnnouncementSink::new(
        transport,
        Arc::new(NoVodPreview),
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
            ..Default::default()
        },
        previous_message_id: None,
        previous_tracking_token: None,
        stream_id: Some("s-1".to_string()),
        started_at_iso: Some("2026-06-09T17:30:00+00:00".to_string()),
        active_session_id: Some(1),
    }
}

/// Live-Ping aktiviert, aber noch KEINE Rollen-ID gesetzt → triggert im Sink
/// den Auto-Anlage-Pfad (Provider) bzw. den Warn-Fallback ohne Provider.
fn live_request_no_role(login: &str) -> AnnounceLiveRequest {
    let mut req = live_request(login);
    req.entry.live_ping_role_id = None;
    req.entry.live_ping_enabled = true;
    req
}

#[tokio::test]
async fn announce_live_default_config_und_mentions() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    assert!(sink.ready());
    let result = sink
        .announce_live(live_request("drag"))
        .await
        .expect("gesendet");
    assert_eq!(result.message_id, "msg-1");
    assert!(result.tracking_token.is_some());
    // Alert-Mention + Streamer-Ping-Rolle im Content.
    assert!(result.notification_text.starts_with("<@&777>"));
    assert!(result.notification_text.contains("<@&999>"));

    let sends = transport.sends.lock().unwrap();
    let (channel_id, content, embed, roles, view_spec) = &sends[0];
    assert_eq!(*channel_id, 555);
    assert_eq!(content.as_deref(), Some(result.notification_text.as_str()));
    assert_eq!(embed["title"], "Drag ist LIVE in Deadlock!");
    assert_eq!(embed["url"], "https://www.twitch.tv/drag?ref=dc");
    assert!(roles.contains(&999) && roles.contains(&777));
    let view = view_spec.as_ref().expect("Tracking-View");
    assert_eq!(view["type"], "twitch_live_tracking");
    assert_eq!(
        view["tracking_token"].as_str(),
        result.tracking_token.as_deref()
    );
}

/// #222 Verify: `live_ping_enabled = false` muss den Streamer-Rollen-Ping
/// vollständig unterdrücken (Python `_ensure_live_ping_role` → frühes
/// `("", None)`). Trotz gesetzter `live_ping_role_id` darf weder die Mention
/// im Content noch die Rollen-ID in `allowed_role_ids` landen; die statische
/// Alert-Mention (`<@&777>`) bleibt unberührt.
#[tokio::test]
async fn announce_live_ping_disabled_unterdrueckt_streamer_rolle() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    let mut request = live_request("drag");
    request.entry.live_ping_enabled = false; // Rolle gesetzt, aber Ping aus.

    let result = sink.announce_live(request).await.expect("gesendet");

    // Nur die statische Alert-Mention, kein Streamer-Rollen-Ping.
    assert!(result.notification_text.starts_with("<@&777>"));
    assert!(!result.notification_text.contains("<@&999>"));
    let sends = transport.sends.lock().unwrap();
    let (_, _, _, roles, _) = &sends[0];
    assert!(roles.contains(&777));
    assert!(
        !roles.contains(&999),
        "Streamer-Rolle bei disabled Ping verboten"
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
    let (_, _, embed, roles, view_spec) = &sends[0];
    assert_eq!(embed["title"], "Drag ist LIVE in Deadlock!");
    assert!(roles.contains(&999), "Streamer-Rollen-Ping bleibt aktiv");
    assert!(roles.contains(&777), "Alert-Mention bleibt erlaubt");
    let view = view_spec.as_ref().expect("Standard-Button bleibt aktiv");
    assert_eq!(view["button_label"], "Auf Twitch ansehen");
    assert_eq!(
        view["tracking_token"].as_str(),
        result.tracking_token.as_deref()
    );
    assert!(result.tracking_token.is_some(), "Standard-Button liefert Token");
}

#[tokio::test]
async fn end_announcement_editiert_offline_embed() {
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(transport.clone());

    let outcome = sink
        .end_announcement(EndAnnouncementRequest {
            login: "drag".to_string(),
            display_name: "Drag".to_string(),
            message_id: "msg-7".to_string(),
            previous_tracking_token: None,
            last_title: Some("Letzter Titel".to_string()),
            last_game: Some("Deadlock".to_string()),
            twitch_user_id: Some("42".to_string()),
        })
        .await;
    assert_eq!(outcome, EndAnnouncementOutcome::Updated);

    let edits = transport.edits.lock().unwrap();
    let (channel_id, message_id, embed, view_spec) = &edits[0];
    assert_eq!((*channel_id, message_id.as_str()), (555, "msg-7"));
    assert_eq!(embed["title"], "Drag ist OFFLINE");
    assert_eq!(embed["description"], "Letzter Titel");
    let view = view_spec.as_ref().expect("Link-Button");
    assert_eq!(view["type"], "link_button");
    assert_eq!(view["url"], "https://www.twitch.tv/drag?ref=dc");
}

#[tokio::test]
async fn live_ping_auto_anlage_nutzt_provider_rolle() {
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

    // Provider wurde mit login + twitch_user_id aufgerufen.
    let calls = provider.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), &[("drag".to_string(), "42".to_string())]);
    // Frisch angelegte Rolle landet im Ping-Text und in allowed_role_ids.
    assert!(result.notification_text.contains("<@&424242>"));
    let sends = transport.sends.lock().unwrap();
    let (_, _, _, roles, _) = &sends[0];
    assert!(roles.contains(&424242));
}

#[tokio::test]
async fn live_ping_ohne_provider_faellt_auf_warn_zurueck() {
    let transport = Arc::new(StubTransport::default());
    // Provider liefert None (z. B. Discord-Anlage scheitert) → kein Rollen-Ping.
    let provider = Arc::new(StubRoleProvider {
        role_id: None,
        ..Default::default()
    });
    let sink = sink_with_provider(transport.clone(), Some(provider.clone()));

    let result = sink
        .announce_live(live_request_no_role("drag"))
        .await
        .expect("gesendet");

    assert_eq!(provider.calls.lock().unwrap().len(), 1);
    // Nur Alert-Mention, KEIN Rollen-Ping (kein zusätzliches <@&…> über 777).
    assert!(result.notification_text.starts_with("<@&777>"));
    assert!(!result.notification_text.contains("<@&424242>"));
    let sends = transport.sends.lock().unwrap();
    let (_, _, _, roles, _) = &sends[0];
    assert!(!roles.iter().any(|&r| r != 777));
}

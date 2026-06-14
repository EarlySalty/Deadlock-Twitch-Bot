//! Tests des Broker-Announcement-Sinks (Slice 4e): Default-Rendering,
//! per-Streamer-Config, Retry-Token-Stabilität, Offline-Edit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tb_monitoring::poller::hooks::{
    AnnounceLiveRequest, AnnouncementSink, EndAnnouncementOutcome, EndAnnouncementRequest,
};
use tb_monitoring::poller::source::SourceError;
use tb_monitoring::{
    AnnounceConfigStore, AnnouncementSettings, AnnouncementTransport, BrokerAnnouncementSink,
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

fn sink_with(pool: &sqlx::PgPool, transport: Arc<StubTransport>) -> BrokerAnnouncementSink {
    BrokerAnnouncementSink::new(
        transport,
        AnnounceConfigStore::new(pool.clone()),
        Arc::new(NoVodPreview),
        AnnouncementSettings {
            notify_channel_id: 555,
            alert_mention: Some("<@&777>".to_string()),
            ref_code: Some("dc".to_string()),
            target_game: "Deadlock".to_string(),
        },
    )
}

fn live_request(login: &str) -> AnnounceLiveRequest {
    AnnounceLiveRequest {
        login: login.to_string(),
        entry: TrackedEntry {
            login: login.to_string(),
            twitch_user_id: Some("42".to_string()),
            require_link: false,
            is_verified: true,
            is_archived: false,
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

#[tokio::test]
async fn announce_live_default_config_und_mentions() {
    let pool = pool_or_skip!("t4e_announce");
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(&pool, transport.clone());

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

#[tokio::test]
async fn announce_live_nutzt_streamer_config_und_retry_token() {
    let pool = pool_or_skip!("t4e_config_retry");
    sqlx::query(
        r#"INSERT INTO twitch_live_announcement_configs (streamer_login, config_json)
           VALUES ('drag', '{"title_template": "{channel} zockt {game}", "button": {"enabled": false}}')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(&pool, transport.clone());

    // Erster Versuch scheitert → Retry-Zustand mit stabilem Token.
    transport.fail_next_send.store(true, Ordering::SeqCst);
    assert!(sink.announce_live(live_request("drag")).await.is_none());
    let result = sink
        .announce_live(live_request("drag"))
        .await
        .expect("Retry ok");

    let sends = transport.sends.lock().unwrap();
    let (_, _, embed, _, view_spec) = &sends[0];
    assert_eq!(embed["title"], "Drag zockt Deadlock");
    assert!(view_spec.is_none(), "Button deaktiviert → kein View-Spec");
    assert!(result.tracking_token.is_none(), "ohne Button kein Token");
}

#[tokio::test]
async fn end_announcement_editiert_offline_embed() {
    let pool = pool_or_skip!("t4e_offline");
    let transport = Arc::new(StubTransport::default());
    let sink = sink_with(&pool, transport.clone());

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

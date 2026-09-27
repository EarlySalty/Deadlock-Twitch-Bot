struct RecordingPersonalLinks {
    viewers: Mutex<Vec<(String, String, String)>>,
    broadcasters: Mutex<Vec<String>>,
}

#[async_trait]
impl DiscordLinkPort for RecordingPersonalLinks {
    async fn discord_invite(&self, broadcaster_id: &str) -> Result<Option<String>, String> {
        self.broadcasters.lock().await.push(broadcaster_id.into());
        Ok(Some("https://discord.gg/ChannelCode".into()))
    }

    async fn personal_discord_invite(
        &self,
        broadcaster_id: &str,
        login: &str,
        viewer_id: &str,
    ) -> Result<Option<String>, String> {
        self.viewers
            .lock()
            .await
            .push((broadcaster_id.into(), login.into(), viewer_id.into()));
        Ok(Some("https://discord.gg/ViewerCode".into()))
    }
}

#[tokio::test]
async fn dldc_viewer_identity_is_forwarded_and_chat_contains_only_the_url() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1/unused")
        .expect("unused pool");
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(pool, api.clone());
    let links = Arc::new(RecordingPersonalLinks {
        viewers: Mutex::new(Vec::new()),
        broadcasters: Mutex::new(Vec::new()),
    });
    engine.discord_link = links.clone();
    let mut event = make_event("!dldc", false, true);
    event.broadcaster_user_id = "42".into();
    event.chatter_user_id = "43".into();
    event.broadcaster_user_login = "Streamer".into();
    event.chatter_user_login = "Streamer".into();
    engine.cmd_dldc(&event).await;
    assert_eq!(
        api.last_message().await.as_deref(),
        Some("https://discord.gg/ViewerCode")
    );
    assert_eq!(
        *links.viewers.lock().await,
        vec![("42".into(), "streamer".into(), "43".into())]
    );
    assert!(links.broadcasters.lock().await.is_empty());
}

#[tokio::test]
async fn dldc_broadcaster_is_identified_by_id_and_keeps_the_channel_link() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1/unused")
        .expect("unused pool");
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(pool, api.clone());
    let links = Arc::new(RecordingPersonalLinks {
        viewers: Mutex::new(Vec::new()),
        broadcasters: Mutex::new(Vec::new()),
    });
    engine.discord_link = links.clone();
    let mut event = make_event("!dlde", false, false);
    event.broadcaster_user_id = "42".into();
    event.chatter_user_id = "42".into();
    event.chatter_user_login = "renamed".into();
    engine.cmd_dldc(&event).await;
    assert_eq!(
        api.last_message().await.as_deref(),
        Some("https://discord.gg/ChannelCode")
    );
    assert_eq!(*links.broadcasters.lock().await, vec!["42".to_string()]);
    assert!(links.viewers.lock().await.is_empty());
}

#[tokio::test]
async fn dldc_channel_fallback_is_still_a_bare_url() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1/unused")
        .expect("unused pool");
    let api = MockApi::new();
    let engine = make_engine_with_pool(pool, api.clone());
    engine.cmd_dldc(&make_event("!dldc", false, false)).await;
    assert_eq!(
        api.last_message().await.as_deref(),
        Some("https://discord.gg/test")
    );
}

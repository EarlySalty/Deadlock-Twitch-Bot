// Dispatch integration tests: real isolated PostgreSQL, mocked external services.
fn mentioned_event(command: &str, login: &str, id: &str) -> ChatMessageEvent {
    let mut event = make_event(command, false, false);
    event.message.fragments.push(MessageFragment {
        fragment_type: "mention".into(),
        text: format!("@{login}"),
        mention: Some(crate::types::MentionRef {
            user_id: id.into(),
            user_login: login.into(),
        }),
    });
    event
}

#[tokio::test]
async fn target_watchtime_mention_counts_target_not_sender_and_stays_in_channel() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    let event = mentioned_event("!watchtime @Other", "other", "other-id");
    assert!(engine.handle(&event).await);
    assert_eq!(api.last_message().await.unwrap(), "@testuser Zuschauerzeit von @other: Hier bisher erfasst: ca. 1 Std 40 Min, davon 0 Min in diesem Stream.");
    assert!(api.lookup_calls.lock().await.is_empty());
    assert_eq!(api.sent.lock().await[0].0, "bc123");
    assert_eq!(event.chatter_user_id, "u999");
}

#[tokio::test]
async fn target_watchtime_helix_login_and_punctuation_are_normalized() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    for command in [
        "!watchtime @OtHeR,",
        "!WATCHTIME\tother",
        "!watchtime\n@other",
    ] {
        let api = MockApi::new();
        api.user_lookups
            .lock()
            .await
            .insert("other".into(), Ok(Some("other-id".into())));
        let engine = make_engine_with_pool(database.pool.clone(), api.clone());
        assert!(engine.handle(&make_event(command, false, false)).await);
        assert!(api
            .last_message()
            .await
            .unwrap()
            .contains("@other: Hier bisher erfasst: ca. 1 Std 40 Min"));
        assert_eq!(*api.lookup_calls.lock().await, vec!["other".to_string()]);
    }
}

#[tokio::test]
async fn target_watchtime_unknown_or_api_error_does_not_return_sender_watchtime() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    for (lookup, expected) in [
        (
            Ok(None),
            "Den Twitch-Account @other habe ich nicht gefunden.",
        ),
        (
            Err("Helix unavailable".into()),
            "Den Twitch-Namen kann ich gerade nicht auflösen.",
        ),
        (
            Ok(Some(String::new())),
            "Die Twitch-ID kann ich gerade nicht zuordnen.",
        ),
    ] {
        let api = MockApi::new();
        api.user_lookups.lock().await.insert("other".into(), lookup);
        let engine = make_engine_with_pool(database.pool.clone(), api.clone());
        assert!(
            engine
                .handle(&make_event("!watchtime @other", false, false))
                .await
        );
        let text = api.last_message().await.unwrap();
        assert!(text.contains(expected), "{text}");
        assert!(!text.contains("Hier bisher erfasst"));
    }
}

#[tokio::test]
async fn target_watchtime_cooldown_belongs_to_requester_not_target() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    assert!(
        engine
            .handle(&mentioned_event("!watchtime @other", "other", "other-id"))
            .await
    );
    assert!(
        engine
            .handle(&make_event("!watchtime @testuser", false, false))
            .await
    );
    assert!(
        engine
            .handle(&make_event("!watchtime @testchannel", false, false))
            .await
    );
    assert_eq!(api.message_count().await, 1);
    assert!(engine
        .watchtime_cooldowns
        .lock()
        .await
        .contains_key(&("bc123".into(), "u999".into())));
}

#[tokio::test]
async fn target_watchtime_explicit_self_preserves_original_reply_and_id() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    assert!(
        engine
            .handle(&make_event("!watchtime @TestUser", false, false))
            .await
    );
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Hier bisher erfasst: ca. 1 Std, davon 24 Min in diesem Stream."
    );
    assert!(api.lookup_calls.lock().await.is_empty());
}

#[tokio::test]
async fn target_watchtime_invalid_and_multiple_mentions_are_not_silently_ignored() {
    let database = crate::test_postgres::TestPostgres::start().await;
    for command in [
        "!watchtime @",
        "!watchtime @one @two",
        "!watchtime @one/other",
    ] {
        let api = MockApi::new();
        let engine = make_engine_with_pool(database.pool.clone(), api.clone());
        assert!(engine.handle(&make_event(command, false, false)).await);
        assert!(api
            .last_message()
            .await
            .unwrap()
            .contains("genau einen Twitch-Namen"));
        assert!(api.lookup_calls.lock().await.is_empty());
    }
}

async fn target_stats_fixture(pool: &PgPool) {
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260918100000_twitch_player_steam_links.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260920170000_twitch_player_multi_steam.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TABLE streamer_plans(twitch_user_id TEXT PRIMARY KEY, stat_command_settings JSONB NOT NULL DEFAULT '{}');
        CREATE TABLE twitch_streamer_identities(twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT);
        INSERT INTO streamer_plans VALUES ('bc123', '{}'), ('other-id', '{\"wins\":false,\"rank\":false}');")
        .execute(pool).await.unwrap();
}

#[tokio::test]
async fn target_all_player_stats_and_aliases_use_named_person_without_changing_channel_settings() {
    let database = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    for command in [
        "!wins",
        "!winrate",
        "!mmr",
        "!climb",
        "!live",
        "!lastmatch",
        "!last",
        "!streak",
        "!mostplayed",
        "!main",
    ] {
        let event = mentioned_event(&format!("{command} @other"), "other", "other-id");
        assert!(engine.handle(&event).await);
        let text = api.last_message().await.unwrap();
        assert!(
            text.contains("other hat noch keinen Steam-Account"),
            "{command}: {text}"
        );
        assert!(!text.contains("TestChannel"));
    }
    assert_eq!(api.message_count().await, 10);
    sqlx::query("UPDATE streamer_plans SET stat_command_settings='{\"wins\":false}' WHERE twitch_user_id='bc123'").execute(&database.pool).await.unwrap();
    assert!(
        engine
            .handle(&mentioned_event("!wins @other", "other", "other-id"))
            .await
    );
    assert_eq!(api.message_count().await, 10);
}

#[tokio::test]
async fn target_rank_dispatch_uses_target_discord_id_and_keeps_reply_in_original_channel() {
    use wiremock::{
        matchers::{path, query_param},
        Mock, MockServer, ResponseTemplate,
    };
    let database = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&database.pool).await;
    sqlx::query("INSERT INTO twitch_streamer_identities VALUES ('other-id','101'),('bc123','202')")
        .execute(&database.pool)
        .await
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/rank")).and(query_param("discord_id", "101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "linked":true, "verified":true, "is_steam_friend":true, "rank_name":"Oracle", "subrank":4
        }))).expect(1).mount(&server).await;
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(database.pool.clone(), api.clone());
    engine.rank_lookup = crate::rank_lookup::RankLookup::with_urls(
        &format!("{}/v1", server.uri()),
        &format!("{}/rank", server.uri()),
    );
    assert!(
        engine
            .handle(&mentioned_event("!rank @Other", "other", "other-id"))
            .await
    );
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Rang von other: Oracle 4"
    );
    assert_eq!(api.sent.lock().await[0].0, "bc123");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn target_rank_me_uses_chatter_identity_not_broadcaster() {
    use wiremock::{
        matchers::{path, query_param},
        Mock, MockServer, ResponseTemplate,
    };
    let database = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&database.pool).await;
    sqlx::query("INSERT INTO twitch_streamer_identities VALUES ('u999','303'),('bc123','202')")
        .execute(&database.pool)
        .await
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/rank"))
        .and(query_param("discord_id", "303"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "linked":true, "verified":true, "is_steam_friend":true, "rank_name":"Phantom", "subrank":2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(database.pool.clone(), api.clone());
    engine.rank_lookup = crate::rank_lookup::RankLookup::with_urls(
        &format!("{}/v1", server.uri()),
        &format!("{}/rank", server.uri()),
    );
    assert!(engine.handle(&make_event("!rank me", false, false)).await);
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Rang von TestUser: Phantom 2"
    );
    assert!(api.lookup_calls.lock().await.is_empty());
}

#[tokio::test]
async fn target_rank_disabled_channel_blocks_even_direct_steam_and_name_lookups() {
    let database = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&database.pool).await;
    sqlx::query("UPDATE streamer_plans SET stat_command_settings='{\"rank\":false}' WHERE twitch_user_id='bc123'").execute(&database.pool).await.unwrap();
    let server = wiremock::MockServer::start().await;
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(database.pool.clone(), api.clone());
    engine.rank_lookup = crate::rank_lookup::RankLookup::with_urls(&server.uri(), &server.uri());
    for command in ["!rank @other", "!rank steam:12345"] {
        assert!(engine.handle(&make_event(command, false, false)).await);
    }
    assert_eq!(api.message_count().await, 0);
    assert!(api.lookup_calls.lock().await.is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn target_rank_explicit_steam_dispatch_does_not_need_a_twitch_identity() {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let database = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&database.pool).await;
    let server = MockServer::start().await;
    Mock::given(path("/v1/players/12345/rank"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"badge":85,"rank":8,"subrank":5})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/assets/ranks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!([{"tier":8,"name":"Oracle"}])),
        )
        .mount(&server)
        .await;
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(database.pool.clone(), api.clone());
    engine.rank_lookup = crate::rank_lookup::RankLookup::with_urls(
        &format!("{}/v1", server.uri()),
        &format!("{}/rank", server.uri()),
    );
    assert!(
        engine
            .handle(&make_event("!rank steam:76561197960278073", false, false))
            .await
    );
    assert!(api
        .last_message()
        .await
        .unwrap()
        .contains("Rang von Steam-Account 12345: Oracle 5"));
    assert!(api.lookup_calls.lock().await.is_empty());
}

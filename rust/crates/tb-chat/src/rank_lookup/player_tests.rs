#[tokio::test]
async fn player_direct_verified_link_wins_without_discord_or_name_search() {
    let db = database().await;
    let revision = crate::player_links::prepare(&db.pool, "target").await.unwrap();
    assert!(crate::player_links::complete(&db.pool, "target", revision, crate::player_links::STEAM64_BASE + 42, "direct").await.unwrap());
    let server = MockServer::start().await; public_rank(&server, 42, 1).await;
    let result = lookup(&server).twitch_reply(&db.pool, &target(), true).await;
    assert!(result.contains("Rang von Viewer: Oracle 5"), "{result}");
    assert!(!result.contains("unbestätigt"));
    assert!(server.received_requests().await.unwrap().iter().all(|r| r.url.path().starts_with("/v1/")));
}
#[tokio::test]
async fn player_disconnect_blocks_all_fallback_and_cached_rank_until_fresh_connect() {
    let db = database().await;
    let revision = crate::player_links::prepare(&db.pool, "target").await.unwrap();
    crate::player_links::complete(&db.pool, "target", revision, crate::player_links::STEAM64_BASE + 42, "direct").await.unwrap();
    let server = MockServer::start().await; public_rank(&server, 42, 1).await;
    let lookup = lookup(&server);
    assert!(lookup.twitch_reply(&db.pool, &target(), true).await.contains("Oracle 5"));
    crate::player_links::disconnect(&db.pool, "target").await.unwrap();
    assert_eq!(lookup.twitch_reply(&db.pool, &target(), true).await, crate::player_links::DISCONNECTED_REPLY);
    let revision = crate::player_links::prepare(&db.pool, "target").await.unwrap();
    crate::player_links::complete(&db.pool, "target", revision, crate::player_links::STEAM64_BASE + 42, "new").await.unwrap();
    assert!(lookup.twitch_reply(&db.pool, &target(), true).await.contains("Oracle 5"));
}
#[tokio::test]
async fn player_disconnect_while_legacy_http_in_flight_suppresses_old_identity() {
    let db = database().await;
    let server = MockServer::start().await;
    Mock::given(path("/rank")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(200))
        .set_body_json(json!({"linked":true,"verified":true,"is_steam_friend":true,"rank_name":"Oracle","subrank":5})))
        .mount(&server).await;
    let lookup = lookup(&server);
    let t = target();
    let (reply, ()) = tokio::join!(lookup.twitch_reply(&db.pool, &t, true), async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        crate::player_links::disconnect(&db.pool, "target").await.unwrap();
    });
    assert_eq!(reply, crate::player_links::DISCONNECTED_REPLY);
}
#[tokio::test]
async fn player_direct_link_changed_during_http_never_delivers_previous_rank() {
    let db = database().await;
    let revision = crate::player_links::prepare(&db.pool, "target").await.unwrap();
    crate::player_links::complete(&db.pool, "target", revision, crate::player_links::STEAM64_BASE + 42, "direct").await.unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/v1/players/42/rank")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(200))
        .set_body_json(json!({"badge":85,"rank":8,"subrank":5}))).mount(&server).await;
    let lookup = lookup(&server); let t = target();
    let (reply, ()) = tokio::join!(lookup.twitch_reply(&db.pool, &t, true), async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let revision = crate::player_links::prepare(&db.pool, "target").await.unwrap();
        crate::player_links::complete(&db.pool, "target", revision, crate::player_links::STEAM64_BASE + 43, "new").await.unwrap();
    });
    assert!(reply.contains("Zuordnung wurde geändert"));
}

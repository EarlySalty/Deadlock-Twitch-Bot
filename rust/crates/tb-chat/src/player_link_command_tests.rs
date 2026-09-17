// These tests use real command dispatch, an isolated database and no live chat.
#[tokio::test]
async fn player_connect_command_only_shares_public_url_no_identity_token() {
    let db = crate::test_postgres::TestPostgres::start().await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    assert!(engine.handle(&make_event("!connect", false, false)).await);
    let text = api.last_message().await.unwrap();
    assert!(text.contains(crate::player_links::CONNECT_URL));
    assert!(!text.contains("?state=") && !text.contains("u999") && !text.contains("bc123"));
}
#[tokio::test]
async fn player_unconnect_only_changes_sender_and_rejects_target_even_for_mod() {
    let db = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&db.pool).await;
    let rev = crate::player_links::prepare(&db.pool, "other-id").await.unwrap();
    assert!(crate::player_links::complete(&db.pool, "other-id", rev, crate::player_links::STEAM64_BASE + 42, "other").await.unwrap());
    let api = MockApi::new();
    let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    assert!(engine.handle(&make_event("!unconnect @other", true, false)).await);
    assert!(api.last_message().await.unwrap().contains("ohne @user"));
    assert!(crate::player_links::load(&db.pool, "u999").await.unwrap().is_none());
    for command in ["!unconnect", "!disconnect"] {
        assert!(engine.handle(&make_event(command, false, false)).await);
        let own = crate::player_links::load(&db.pool, "u999").await.unwrap().unwrap();
        assert!(!own.lookup_enabled); assert!(own.steam_id64.is_none());
        assert_eq!(crate::player_links::load(&db.pool, "other-id").await.unwrap().unwrap().account_id(), Some(42));
    }
}
#[tokio::test]
async fn player_unconnect_database_error_never_claims_success() {
    let db = crate::test_postgres::TestPostgres::start().await;
    let api = MockApi::new(); let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    assert!(engine.handle(&make_event("!unconnect", false, false)).await);
    assert!(!api.last_message().await.unwrap().contains("ist entfernt"));
}
#[tokio::test]
async fn player_optout_blocks_all_twitch_stats_and_aliases_but_not_watchtime() {
    let db = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&db.pool).await;
    crate::player_links::disconnect(&db.pool, "other-id").await.unwrap();
    let api = MockApi::new(); let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    for command in ["!rank", "!wins", "!winrate", "!mmr", "!climb", "!live", "!last", "!lastmatch", "!streak", "!main", "!mostplayed"] {
        assert!(engine.handle(&mentioned_event(&format!("{command} @other"), "other", "other-id")).await);
        assert!(api.last_message().await.unwrap().contains("Steam-Zuordnung deaktiviert"), "{command}");
    }
    watchtime_fixture(&db.pool).await;
    assert!(engine.handle(&mentioned_event("!watchtime @other", "other", "other-id")).await);
    assert!(api.last_message().await.unwrap().contains("1 Std 40 Min"));
}
#[tokio::test]
async fn player_direct_link_other_stats_do_not_silently_use_wrong_legacy_account() {
    let db = crate::test_postgres::TestPostgres::start().await;
    target_stats_fixture(&db.pool).await;
    let revision = crate::player_links::prepare(&db.pool, "other-id").await.unwrap();
    crate::player_links::complete(&db.pool, "other-id", revision, crate::player_links::STEAM64_BASE + 42, "direct").await.unwrap();
    let api = MockApi::new(); let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    assert!(engine.handle(&mentioned_event("!winrate @other", "other", "other-id")).await);
    assert!(api.last_message().await.unwrap().contains("Über diese Verbindung ist !rank verfügbar"));
}

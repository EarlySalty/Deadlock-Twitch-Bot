// Echter Handler und Analytics-SQL gegen einen privaten PostgreSQL-Prozess.
async fn watchtime_fixture(pool: &PgPool) {
    sqlx::raw_sql(
        "CREATE TABLE twitch_stream_sessions (id BIGINT PRIMARY KEY, twitch_user_id TEXT);
         CREATE TABLE twitch_session_chatters (
             session_id BIGINT, chatter_id TEXT, chatter_login TEXT,
             first_seen_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ);
         CREATE TABLE twitch_viewer_presence_ticks (
             session_id BIGINT, viewer_login TEXT, tick_at TIMESTAMPTZ, twitch_user_id TEXT);
         CREATE INDEX ON twitch_viewer_presence_ticks (session_id, viewer_login, tick_at);
         CREATE TABLE twitch_live_state (
             twitch_user_id TEXT PRIMARY KEY, is_live INTEGER, active_session_id BIGINT);
         INSERT INTO twitch_stream_sessions VALUES (1, 'bc123'), (2, 'bc123'),
             (3, 'other-channel'), (4, 'bc123');
         INSERT INTO twitch_session_chatters (session_id, chatter_id, chatter_login) VALUES
             (1, 'u999', 'oldname'), (2, 'u999', 'testuser'),
             (2, 'u999', 'renamed'), (3, 'u999', 'testuser'), (4, 'other-id', 'testuser');
         UPDATE twitch_session_chatters SET first_seen_at = '2026-09-09 10:00Z',
             last_seen_at = '2026-09-09 18:00Z';
         INSERT INTO twitch_viewer_presence_ticks (session_id, viewer_login, tick_at)
             SELECT 1, 'oldname', '2026-09-08 10:00Z'::timestamptz + n * INTERVAL '30 seconds'
             FROM generate_series(1,72) n;
         INSERT INTO twitch_viewer_presence_ticks (session_id, viewer_login, tick_at)
             SELECT 2, 'testuser', '2026-09-09 10:00Z'::timestamptz + n * INTERVAL '30 seconds'
                 + CASE WHEN n > 20 THEN INTERVAL '30 minutes' ELSE INTERVAL '0 seconds' END
             FROM generate_series(1,49) n;
         INSERT INTO twitch_viewer_presence_ticks (session_id, viewer_login, tick_at)
             SELECT 2, 'renamed', tick_at FROM twitch_viewer_presence_ticks WHERE session_id = 2;
         INSERT INTO twitch_viewer_presence_ticks (session_id, viewer_login, tick_at)
             SELECT s, 'testuser', '2026-09-09 10:00Z'::timestamptz + n * INTERVAL '30 seconds'
             FROM generate_series(3,4) s CROSS JOIN generate_series(1,200) n;
         INSERT INTO twitch_live_state VALUES ('bc123', 1, 2), ('other-channel', 1, 3);",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn watchtime_zaehlt_nur_eigene_ticks_dieses_kanals() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    assert!(engine.handle(&make_event("!watchtime", false, false)).await);
    assert_eq!(api.message_count().await, 1);
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Hier bisher erfasst: ca. 1 Std, davon 24 Min in diesem Stream."
    );
}

#[tokio::test]
async fn watchtime_leerzustand_und_fehlende_ids_ohne_login_fallback() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    let mut event = make_event("!watchtime @other", false, false);
    event.chatter_user_id = "unknown-id".into();
    assert!(engine.handle(&event).await);
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Für dich ist hier noch keine Zuschauerzeit erfasst."
    );
    event.chatter_user_id.clear();
    assert!(engine.handle(&event).await);
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Deine Zuschauerzeit kann ich gerade nicht zuordnen."
    );
    event.chatter_user_id = "u999".into();
    event.broadcaster_user_id.clear();
    assert!(engine.handle(&event).await);
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Deine Zuschauerzeit kann ich gerade nicht zuordnen."
    );
}

#[tokio::test]
async fn watchtime_offline_und_ohne_live_status_nur_gesamt() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    for sql in [
        "UPDATE twitch_live_state SET is_live = 0",
        "DELETE FROM twitch_live_state",
    ] {
        sqlx::query(sql).execute(&database.pool).await.unwrap();
        let api = MockApi::new();
        let engine = make_engine_with_pool(database.pool.clone(), api.clone());
        assert!(engine.handle(&make_event("!watchtime", false, false)).await);
        assert_eq!(
            api.last_message().await.unwrap(),
            "@testuser Hier bisher erfasst: ca. 1 Std."
        );
    }
}

#[tokio::test]
async fn watchtime_db_fehler_ist_keine_null() {
    let database = crate::test_postgres::TestPostgres::start().await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    assert!(engine.handle(&make_event("!watchtime", false, false)).await);
    assert_eq!(
        api.last_message().await.unwrap(),
        "@testuser Deine Zuschauerzeit kann ich gerade nicht abrufen. Versuch es gleich nochmal."
    );
}

#[tokio::test]
async fn watchtime_cooldown_atomisch_und_nach_ids() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    let event = make_event("!watchtime", false, false);
    let (a, b) = tokio::join!(engine.handle(&event), engine.handle(&event));
    assert!(a && b);
    assert_eq!(api.message_count().await, 1);
    let mut renamed = event.clone();
    renamed.chatter_user_login = "newname".into();
    assert!(engine.handle(&renamed).await);
    assert_eq!(api.message_count().await, 1);
    renamed.chatter_user_id = "other-id".into();
    assert!(engine.handle(&renamed).await);
    assert_eq!(api.message_count().await, 2);
    renamed.broadcaster_user_id = "other-channel".into();
    assert!(engine.handle(&renamed).await);
    assert_eq!(api.message_count().await, 3);
}

#[tokio::test]
async fn watchtime_sendefehler_erlaubt_wiederholung() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    api.fail_next_send().await;
    let event = make_event("!watchtime", false, false);
    assert!(engine.handle(&event).await);
    assert_eq!(api.message_count().await, 0);
    assert!(engine.handle(&event).await);
    assert_eq!(api.message_count().await, 1);
}

#[tokio::test]
async fn watchtime_twitch_ablehnungen_erlauben_neuen_versuch() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    for outcome in [
        SendOutcome::Dropped {
            code: "test".into(),
            message: "test".into(),
        },
        SendOutcome::HttpError {
            status: 503,
            body: "test".into(),
        },
    ] {
        let api = MockApi::new();
        api.outcomes.lock().await.push(outcome);
        let engine = make_engine_with_pool(database.pool.clone(), api.clone());
        let event = make_event("!watchtime", false, false);
        assert!(engine.handle(&event).await);
        assert_eq!(api.message_count().await, 0);
        assert!(engine.handle(&event).await);
        assert_eq!(api.message_count().await, 1);
    }
}

#[tokio::test]
async fn watchtime_cooldown_laeuft_ab_und_raeumt_alte_identitaeten_auf() {
    let database = crate::test_postgres::TestPostgres::start().await;
    watchtime_fixture(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    let old = Instant::now() - std::time::Duration::from_secs(11);
    engine.watchtime_cooldowns.lock().await.extend([
        (("bc123".into(), "u999".into()), old),
        (("gone".into(), "gone".into()), old),
    ]);
    assert!(engine.handle(&make_event("!watchtime", false, false)).await);
    assert_eq!(api.message_count().await, 1);
    assert_eq!(engine.watchtime_cooldowns.lock().await.len(), 1);
}

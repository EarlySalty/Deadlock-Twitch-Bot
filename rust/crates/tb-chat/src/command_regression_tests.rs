#[tokio::test]
async fn regression_statistik_db_fehler_ist_keine_fehlende_verknuepfung() {
    let database = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&database.pool).await;
    sqlx::query("DROP TABLE twitch_streamer_identities")
        .execute(&database.pool)
        .await
        .unwrap();
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    for command in [
        "!rank",
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
        assert!(engine.handle(&make_event(command, false, false)).await);
        let message = api.last_message().await.unwrap();
        assert!(
            message.contains("gerade nicht abrufen"),
            "{command}: {message}"
        );
        assert!(!message.contains("keinen Steam-Account"));
    }
}

#[tokio::test]
async fn regression_lurk_clip_schalter_nur_nach_kanal_id() {
    let database = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&database.pool).await;
    sqlx::query("INSERT INTO streamer_plans (twitch_user_id,twitch_login,lurk_command_enabled,clip_command_enabled) VALUES ('foreign','testchannel',0,0), ('bc123','oldchannel',1,1)")
        .execute(&database.pool).await.unwrap();
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    assert!(engine.handle(&make_event("!lurk", false, false)).await);
    assert_eq!(
        api.message_count().await,
        1,
        "eigener aktiver Schalter muss gelten"
    );
    assert!(engine.handle(&make_event("!clip", false, false)).await);
    assert_eq!(
        api.message_count().await,
        2,
        "aktiver Clip erreicht Partner-Prüfung"
    );
    sqlx::query("UPDATE streamer_plans SET lurk_command_enabled=CASE WHEN twitch_user_id='bc123' THEN 0 ELSE 1 END, clip_command_enabled=CASE WHEN twitch_user_id='bc123' THEN 0 ELSE 1 END")
        .execute(&database.pool).await.unwrap();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    assert!(engine.handle(&make_event("!lurk", false, false)).await);
    assert!(engine.handle(&make_event("!clip", false, false)).await);
    assert_eq!(
        api.message_count().await,
        2,
        "eigene deaktivierte Schalter müssen gelten"
    );
}

#[tokio::test]
async fn regression_partner_lookup_nutzt_id_bei_recyceltem_login() {
    let database = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&database.pool).await;
    sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_user_id,twitch_login,is_partner_active) VALUES ('foreign','testchannel',1), ('bc123','oldchannel',1)")
        .execute(&database.pool).await.unwrap();
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    let event = make_event("!raid_status", false, false);
    assert!(engine.handle(&event).await);
    assert!(api.last_message().await.unwrap().contains("Statistik:"));
    let mut missing = event;
    missing.broadcaster_user_id = "not-a-partner".into();
    assert!(engine.handle(&missing).await);
    assert!(
        api.last_message()
            .await
            .unwrap()
            .contains("nicht als Partner registriert"),
        "fremder Partnername darf keine Identität verleihen"
    );
}
#[tokio::test]
async fn regression_raid_status_letzter_fehlgeschlagen_trotz_frueherem_erfolg() {
    let database = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&database.pool).await;
    seed_partner(&database.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(database.pool.clone(), api.clone());
    assert!(
        engine
            .handle(&make_event("!raid_status", false, false))
            .await
    );
    let message = api.last_message().await.unwrap();
    assert!(message.contains("5 Raids (3 erfolgreich)"));
    assert!(message.contains("Letzter Raid ❌"), "{message}");
}

struct ErrorCommandPorts;
#[async_trait]
impl RaidCommandPort for ErrorCommandPorts {
    async fn manual_raid(&self, _: &str, _: &str) -> Result<RaidStartResult, String> {
        Err("test unavailable".into())
    }
    async fn raid_status(&self, _: &str) -> Result<RaidStatusInfo, String> {
        Err("test unavailable".into())
    }
    async fn toggle_silent_ban(&self, _: &str) -> Result<i32, String> {
        Err("test unavailable".into())
    }
    async fn toggle_silent_raid(&self, _: &str) -> Result<i32, String> {
        Err("test unavailable".into())
    }
}
#[async_trait]
impl DiscordLinkPort for ErrorCommandPorts {
    async fn discord_invite(&self, _: &str) -> Result<Option<String>, String> {
        Err("test unavailable".into())
    }
}
#[async_trait]
impl InvitePort for ErrorCommandPorts {
    async fn invite_line(&self, _: &str, _: &str) -> Result<Option<String>, String> {
        Err("test unavailable".into())
    }
}

#[tokio::test]
async fn regression_portfehler_antworten_statt_schweigen_oder_leerzustand() {
    let database = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&database.pool).await;
    seed_partner(&database.pool).await;
    sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id,raid_enabled,needs_reauth) VALUES ('bc123',true,false)")
        .execute(&database.pool).await.unwrap();
    sqlx::query("DROP TABLE twitch_raid_history")
        .execute(&database.pool)
        .await
        .unwrap();
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(database.pool.clone(), api.clone());
    engine.raid = Arc::new(ErrorCommandPorts);
    engine.discord_link = Arc::new(ErrorCommandPorts);
    engine.invite = Arc::new(ErrorCommandPorts);
    let mut failures = Vec::new();
    for command in [
        "!raid_status",
        "!raid_history",
        "!silentban",
        "!silentraid",
        "!discord",
        "!invite",
    ] {
        let before = api.message_count().await;
        assert!(engine.handle(&make_event(command, true, false)).await);
        let after = api.message_count().await;
        let text = api.last_message().await.unwrap_or_default();
        if after != before + 1 || !(text.contains("gerade nicht") || text.contains("gerade keinen"))
        {
            failures.push(format!("{command}: {before}->{after}, {text}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
async fn regression_invite_leerzustand_ohne_erfolgs_cooldown() {
    let database = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&database.pool).await;
    seed_partner(&database.pool).await;
    let api = MockApi::new();
    let mut engine = make_engine_with_pool(database.pool.clone(), api.clone());
    engine.invite = Arc::new(MockInvite { reply: None });
    assert!(engine.handle(&make_event("!invite", false, false)).await);
    assert!(api
        .last_message()
        .await
        .unwrap_or_default()
        .contains("Kein Einladungslink"));
    assert!(engine.invite_cooldowns.lock().await.is_empty());
}

#[tokio::test]
async fn zentraler_antwortweg_erkennt_alle_send_ergebnisse() {
    let api = MockApi::new();
    assert!(crate::api::send_reply(api.as_ref(), "test-channel", "test").await);
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
        api.outcomes.lock().await.push(outcome);
        assert!(!crate::api::send_reply(api.as_ref(), "test-channel", "test").await);
    }
    api.fail_next_send().await;
    assert!(!crate::api::send_reply(api.as_ref(), "test-channel", "test").await);
    assert_eq!(api.message_count().await, 1);
}

#[tokio::test]
async fn regression_engagement_umbenannter_kanal_aendert_nur_eigene_einstellung() {
    let db = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    sqlx::query("INSERT INTO twitch_engagement_settings (channel_login,channel_user_id,enabled) VALUES ('oldchannel','bc123',false), ('testchannel','foreign',false)")
        .execute(&db.pool).await.unwrap();
    let api = MockApi::new();
    let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    assert!(
        engine
            .handle(&make_event("!engagement_on", true, false))
            .await
    );
    let values: Vec<(String, bool)> = sqlx::query_as(
        "SELECT channel_user_id,enabled FROM twitch_engagement_settings ORDER BY channel_user_id",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        values,
        vec![("bc123".into(), true), ("foreign".into(), false)]
    );
    assert!(
        engine
            .handle(&make_event("!engagement_status", false, false))
            .await
    );
    assert!(api.last_message().await.unwrap().contains("AN"));
}

#[tokio::test]
async fn katalog_befehle_und_aliase_erreichen_den_dispatch() {
    let db = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(db.pool.clone(), api);
    for command in crate::catalog::catalog()
        .iter()
        .flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied()))
    {
        assert!(
            engine.handle(&make_event(command, false, false)).await,
            "Katalogbefehl {command} ohne Dispatch"
        );
    }
}

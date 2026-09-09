#[tokio::test]
async fn regression_lurker_eigenes_checkout_login_abo_ohne_override() {
    let db = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    seed_partner(&db.pool).await;
    sqlx::raw_sql("INSERT INTO streamer_plans (twitch_user_id,twitch_login,lurker_tax_enabled) VALUES ('bc123','testchannel',1);
        INSERT INTO twitch_streamer_identities (twitch_user_id,twitch_login) VALUES ('bc123','testchannel');
        DROP TABLE twitch_billing_subscriptions;
        CREATE TABLE twitch_billing_subscriptions (
            stripe_subscription_id TEXT PRIMARY KEY, stripe_customer_id TEXT, customer_reference TEXT,
            status TEXT NOT NULL DEFAULT 'unknown', plan_id TEXT, cycle_months INTEGER NOT NULL DEFAULT 1,
            quantity INTEGER NOT NULL DEFAULT 1, current_period_start TEXT, current_period_end TEXT,
            cancel_at_period_end INTEGER NOT NULL DEFAULT 0, canceled_at TEXT, ended_at TEXT,
            last_event_id TEXT, updated_at TEXT NOT NULL);")
        .execute(&db.pool).await.unwrap();
    // Derselbe Referenzvertrag wie customer_reference_for/Checkout: Login vor ID.
    // Der echte Webhook-Schreiber persistiert die unveränderte Checkoutreferenz.
    let checkout = serde_json::json!({
        "mode": "subscription", "subscription": "sub_test", "customer": "cus_test",
        "client_reference_id": "testchannel",
        "metadata": {"customer_reference": "testchannel", "plan_id": "raid_boost"}
    });
    let subscription = serde_json::json!({
        "id": "sub_test", "customer": "cus_test", "status": "active",
        "metadata": {"plan_id": "raid_boost"},
        "current_period_end": (chrono::Utc::now() + chrono::Duration::days(30)).timestamp()
    });
    let mut tx = db.pool.begin().await.unwrap();
    tb_analytics::stripe::webhook_apply::apply_event(
        &mut tx,
        "evt_test",
        "checkout.session.completed",
        &checkout,
        Some(&subscription),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let stored: (String,String,String,Option<String>) = sqlx::query_as("SELECT b.customer_reference,b.plan_id,b.status,p.manual_plan_id FROM twitch_billing_subscriptions b CROSS JOIN streamer_plans p WHERE p.twitch_user_id='bc123'").fetch_one(&db.pool).await.unwrap();
    assert_eq!(
        stored,
        (
            "testchannel".into(),
            "raid_boost".into(),
            "active".into(),
            None
        )
    );
    let legacy = tb_analytics::plan::resolve_plan_snapshot(&db.pool, "testchannel", "bc123")
        .await
        .unwrap();
    assert_eq!(legacy.source, "billing_subscription");
    assert!(legacy.entitlements.contains(&"chat.lurker_tax"));
    let api = MockApi::new();
    let promos = crate::promos::PromoEngine::new(
        db.pool.clone(),
        api.clone(),
        Arc::new(crate::promos::NoopSuppressionCheck),
    );
    promos
        .thank_lurker_tax_redeemer("bc123", "testchannel", "normal")
        .await;
    assert_eq!(
        api.message_count().await,
        1,
        "reguläres eigenes Login-Abo muss den echten Verbraucher freischalten"
    );
    // Nach Rename bleibt die ID-gebundene bisherige Checkoutreferenz gültig.
    sqlx::query(
        "UPDATE twitch_streamer_identities SET twitch_login='renamed' WHERE twitch_user_id='bc123'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    promos
        .thank_lurker_tax_redeemer("bc123", "renamed", "renamed-viewer")
        .await;
    assert_eq!(api.message_count().await, 2);
    // Widerspruch: alter Checkoutname gehört nun nachweislich einer fremden ID.
    sqlx::query("INSERT INTO twitch_streamer_identities VALUES ('foreign','testchannel')")
        .execute(&db.pool)
        .await
        .unwrap();
    promos
        .thank_lurker_tax_redeemer("bc123", "renamed", "conflicting")
        .await;
    assert_eq!(
        api.message_count().await,
        2,
        "recycelte Referenz ist nicht mehr eindeutig"
    );
    sqlx::query("DELETE FROM twitch_streamer_identities WHERE twitch_user_id='foreign'")
        .execute(&db.pool)
        .await
        .unwrap();
    let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    let mut event = make_event("!lurkersteuer_off", false, true);
    event.broadcaster_user_login = "renamed".into();
    assert!(engine.handle(&event).await);
    assert!(api
        .last_message()
        .await
        .unwrap()
        .contains("Lurker Steuer deaktiviert"));
    promos
        .thank_lurker_tax_redeemer("bc123", "renamed", "after-off")
        .await;
    assert_eq!(
        api.message_count().await,
        3,
        "Schreiber und Verbraucher verwenden dieselbe Planberechtigung"
    );
}

#[tokio::test]
async fn regression_statistik_settings_ausfall_antwortet_default_und_optout_bleiben() {
    let db = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    let api = MockApi::new();
    let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    let commands = [
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
    ];
    // Keine Planzeile: jeder Befehl erreicht den echten Handler.
    for command in commands {
        let before = api.message_count().await;
        assert!(engine.handle(&make_event(command, false, false)).await);
        assert_eq!(api.message_count().await, before + 1);
    }
    sqlx::query("INSERT INTO streamer_plans (twitch_user_id,stat_command_settings) VALUES ('bc123', '{\"rank\":false,\"wins\":false,\"winrate\":false,\"mmr\":false,\"live\":false,\"lastmatch\":false,\"streak\":false,\"mostplayed\":false}')").execute(&db.pool).await.unwrap();
    let before = api.message_count().await;
    for command in commands {
        assert!(engine.handle(&make_event(command, false, false)).await);
    }
    assert_eq!(
        api.message_count().await,
        before,
        "bewusst abgeschaltet bleibt still"
    );
    // Relationsfehler und Gesamtausfall müssen beide eine Antwort liefern.
    sqlx::query("DROP TABLE streamer_plans")
        .execute(&db.pool)
        .await
        .unwrap();
    for closed in [false, true] {
        if closed {
            db.pool.close().await;
        }
        for command in commands {
            let before = api.message_count().await;
            assert!(engine.handle(&make_event(command, false, false)).await);
            assert_eq!(
                api.message_count().await,
                before + 1,
                "{command}, closed={closed}"
            );
            let text = api.last_message().await.unwrap();
            assert!(text.contains("gerade nicht abrufen"), "{text}");
            assert!(!text.contains("keinen Steam-Account"));
        }
    }
}

#[tokio::test]
async fn regression_lurker_command_abschaltung_wirkt_im_verbraucher_bei_rename() {
    let db = crate::test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    seed_lurker_partner(&db.pool, 1, true).await;
    sqlx::query("UPDATE streamer_plans SET twitch_login='oldchannel' WHERE twitch_user_id='bc123'")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE twitch_streamers_partner_state SET twitch_login='oldchannel' WHERE twitch_user_id='bc123'").execute(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO streamer_plans (twitch_user_id,twitch_login,lurker_tax_enabled,manual_plan_id) VALUES ('foreign','testchannel',1,'raid_boost')").execute(&db.pool).await.unwrap();
    let api = MockApi::new();
    let engine = make_engine_with_pool(db.pool.clone(), api.clone());
    let promos = crate::promos::PromoEngine::new(
        db.pool.clone(),
        api.clone(),
        Arc::new(crate::promos::NoopSuppressionCheck),
    );
    promos
        .thank_lurker_tax_redeemer("bc123", "testchannel", "before")
        .await;
    assert_eq!(
        api.message_count().await,
        1,
        "aktivierter eigener Plan erlaubt bestehenden Dank"
    );
    assert!(
        engine
            .handle(&make_event("!lurkersteuer_off", false, true))
            .await
    );
    assert!(api
        .last_message()
        .await
        .unwrap()
        .contains("Lurker Steuer deaktiviert"));
    let before = api.message_count().await;
    promos
        .thank_lurker_tax_redeemer("bc123", "testchannel", "after")
        .await;
    assert_eq!(
        api.message_count().await,
        before,
        "bestätigtes Abschalten muss den Verbraucher stoppen"
    );
    let foreign: i32 = sqlx::query_scalar(
        "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id='foreign'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(foreign, 1);
    // Fehlende ID/Planzeile bleibt bei diesem Opt-in-Verbraucher aus.
    promos
        .thank_lurker_tax_redeemer("", "testchannel", "missing-id")
        .await;
    promos
        .thank_lurker_tax_redeemer("missing", "testchannel", "missing-plan")
        .await;
    assert_eq!(api.message_count().await, before);
}

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

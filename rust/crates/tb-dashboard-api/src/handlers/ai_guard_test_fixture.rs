use std::process::Command;

pub fn in_isolated_process(test_name: &str) -> bool {
    if std::env::var_os("TB_DASHBOARD_AI_GUARD_TEST").as_deref() == Some(test_name.as_ref()) {
        return true;
    }
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TB_DASHBOARD_AI_GUARD_TEST", test_name)
        .args(["--exact", test_name, "--include-ignored", "--nocapture"])
        .output()
        .expect("isolated dashboard AI test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success()
            && stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
        "{test_name}: {}\n{stdout}\n{stderr}",
        output.status
    );
    print!("{stdout}");
    false
}

pub async fn initialize(pool: &sqlx::PgPool) -> wiremock::MockServer {
    for schema in [
        include_str!("../../../../crates/tb-llm/sqlx/schema.sql"),
        include_str!("../../../../test-support/ai_policy_fixture.sql"),
        include_str!("../../../../migrations/20261014170000_central_ai_policy.sql"),
    ] {
        sqlx::raw_sql(schema).execute(pool).await.unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO public.discord_guilds (guild_id, ai_features) VALUES (1289721245281292288, true); \
         INSERT INTO public.streamer_guilds (twitch_user_id, guild_id, active) \
             VALUES ('42', 1289721245281292288, true); \
         INSERT INTO public.twitch_streamer_identities (twitch_user_id, twitch_login) \
             VALUES ('42', 'nani'); \
         INSERT INTO public.twitch_partners_all_state (twitch_user_id, is_partner_active) \
             VALUES ('42', 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    tb_llm::ledger::initialize(pool.clone(), "twitch-test", "dashboard-ai-fixture")
        .expect("isolated dashboard ledger");
    assert_eq!(
        tb_llm::streamer_ai_allowed(Some("42"), Some("nani")).await,
        Ok(true)
    );
    let server = wiremock::MockServer::start().await;
    std::env::set_var("FIREWORK_API_KEY", "synthetic-test-key");
    std::env::set_var("FIREWORK_BASE_URL", server.uri());
    server
}

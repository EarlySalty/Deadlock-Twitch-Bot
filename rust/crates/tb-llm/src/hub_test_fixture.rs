use std::process::Command;

#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

pub(crate) const TWITCH_USER_ID: &str = "4242000001";

pub(crate) fn in_isolated_process(test_name: &str) -> bool {
    if std::env::var_os("TB_LLM_FIXTURE_CHILD").as_deref() == Some(test_name.as_ref()) {
        return true;
    }
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .env("TB_LLM_FIXTURE_CHILD", test_name)
        .args(["--exact", test_name, "--include-ignored", "--nocapture"])
        .output()
        .expect("isolated test process");
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

pub(crate) async fn authorized_database(test_name: &str) -> Option<test_postgres::TestPostgres> {
    if !in_isolated_process(test_name) {
        return None;
    }
    let database = test_postgres::TestPostgres::start().await;
    for schema in [
        include_str!("../sqlx/schema.sql"),
        include_str!("../../../test-support/multiserver_ai_schema.sql"),
        include_str!("../../../test-support/20261012140000_ai_features_rule.sql"),
        include_str!("../../../test-support/20261013150200_multiserver_ai_contract.sql"),
    ] {
        sqlx::raw_sql(schema)
            .execute(&database.pool)
            .await
            .expect("accepted Hub authorization schema");
    }
    sqlx::raw_sql(
        "INSERT INTO public.discord_guilds (guild_id, ai_features) VALUES (900000000000000041, true); \
         INSERT INTO public.streamer_guilds (twitch_user_id, guild_id, active) \
             VALUES ('4242000001', 900000000000000041, true); \
         INSERT INTO public.twitch_streamer_identities (twitch_user_id, twitch_login) \
             VALUES ('4242000001', 'hub_fixture'); \
         INSERT INTO public.twitch_partners_all_state (twitch_user_id, is_partner_active) \
             VALUES ('4242000001', 1)",
    )
    .execute(&database.pool)
    .await
    .expect("synthetic Hub identity and scoped membership");
    crate::ledger::initialize(database.pool.clone(), "twitch-test", "hub-fixture")
        .expect("isolated Hub ledger");
    assert_eq!(
        crate::ai_guard::streamer_ai_allowed(Some(TWITCH_USER_ID), Some("hub_fixture")).await,
        Ok(true)
    );
    Some(database)
}

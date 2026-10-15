use std::process::Command;

#[path = "postgres.rs"]
mod postgres;

pub(crate) async fn authorized_database(qualified_name: &str) -> Option<postgres::TestPostgres> {
    let test_name = qualified_name
        .split_once("::")
        .expect("qualified test name")
        .1;
    if std::env::var("TB_AI_POLICY_TEST_CHILD").as_deref() != Ok(test_name) {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .env("TB_AI_POLICY_TEST_CHILD", test_name)
            .args(["--exact", test_name, "--nocapture"])
            .output()
            .expect("isolated policy test");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("1 passed; 0 failed"),
            "{test_name}: {stdout}\n{stderr}"
        );
        return None;
    }
    let database = postgres::TestPostgres::start().await;
    sqlx::raw_sql("CREATE TABLE public.twitch_streamer_identities(twitch_user_id text PRIMARY KEY,twitch_login text NOT NULL); INSERT INTO public.twitch_streamer_identities VALUES ('42','home'),('123456789','brainfixture');")
        .execute(&database.pool).await.expect("synthetic canonical identities");
    sqlx::raw_sql(include_str!(
        "../migrations/20261015120000_streamer_ai_policy.sql"
    ))
    .execute(&database.pool)
    .await
    .expect("actual AI policy migration");
    sqlx::raw_sql(include_str!("../crates/tb-llm/sqlx/schema.sql"))
        .execute(&database.pool)
        .await
        .expect("actual usage schema");
    tb_llm::ledger::initialize(database.pool.clone(), "twitch-test", "ai-policy-fixture")
        .expect("isolated ledger");
    Some(database)
}

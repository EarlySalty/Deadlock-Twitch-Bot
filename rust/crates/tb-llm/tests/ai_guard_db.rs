#[path = "../../../test-support/postgres.rs"]
mod postgres;

use tb_llm::{ai_guard::allowed_with_pool, complete, LlmEndpoint, LlmError, Request};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

async fn database() -> postgres::TestPostgres {
    let database = postgres::TestPostgres::start().await;
    sqlx::raw_sql("CREATE TABLE twitch_streamer_identities(twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT NOT NULL); INSERT INTO twitch_streamer_identities VALUES ('42','home'),('43','otherhome');")
        .execute(&database.pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261015120000_streamer_ai_policy.sql"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::raw_sql("INSERT INTO twitch_streamer_identities VALUES ('99','foreign'),('100','unknown'); INSERT INTO twitch_streamer_ai_policy(twitch_user_id,guild_id) VALUES ('99',987654321);")
        .execute(&database.pool).await.unwrap();
    database
}

#[tokio::test]
async fn migration_preserves_home_and_denies_foreign_unknown_and_conflicting_identity() {
    let db = database().await;
    for (id, login, expected) in [
        (Some("42"), None, true),
        (None, Some(" HOME "), true),
        (Some(" 42 "), Some("HOME"), true),
        (Some("42"), Some("foreign"), false),
        (Some("99"), None, false),
        (None, Some("foreign"), false),
        (Some("100"), None, false),
        (Some("999"), None, false),
        (None, None, false),
        (Some(" "), Some("\t"), false),
    ] {
        assert_eq!(
            allowed_with_pool(&db.pool, id, login).await.unwrap(),
            expected
        );
    }
    sqlx::query(
        "UPDATE twitch_streamer_ai_policy SET ai_features = false WHERE twitch_user_id='42'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(!allowed_with_pool(&db.pool, Some("42"), None).await.unwrap());
}

#[tokio::test]
async fn missing_policy_schema_fails_closed() {
    let db = postgres::TestPostgres::start().await;
    assert!(matches!(
        allowed_with_pool(&db.pool, Some("42"), None).await,
        Err(LlmError::Unavailable(_))
    ));
}

#[tokio::test]
async fn central_guard_prevents_http_and_brain_calls_and_observes_switch_changes() {
    let db = database().await;
    sqlx::raw_sql(include_str!("../sqlx/schema.sql"))
        .execute(&db.pool)
        .await
        .unwrap();
    tb_llm::ledger::initialize(db.pool.clone(), "test", "ai-switch").unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message":{"content":"ok"}}], "usage":{"prompt_tokens":1,"completion_tokens":1}
        }))).expect(1).mount(&server).await;
    let endpoint = LlmEndpoint {
        provider: "fireworks",
        base_url: server.uri(),
        model: tb_llm::selection::configured_fireworks_model().to_string(),
        api_key: Some("test".into()),
    };
    let request = |id: &str| {
        Request::prompt("test")
            .twitch_broadcaster(id)
            .endpoint(endpoint.clone())
    };
    assert!(matches!(
        complete("ai_chat", request("99")).await,
        Err(LlmError::Disabled(_))
    ));
    assert!(matches!(
        complete(
            "ai_chat",
            Request::prompt("test").endpoint(endpoint.clone())
        )
        .await,
        Err(LlmError::Disabled(_))
    ));
    assert_eq!(complete("ai_chat", request("42")).await.unwrap().text, "ok");
    sqlx::query("UPDATE twitch_streamer_ai_policy SET ai_features=false WHERE twitch_user_id='42'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(matches!(
        complete("ai_chat", request("42")).await,
        Err(LlmError::Disabled(_))
    ));
    assert!(matches!(
        complete(
            "community_sentiment",
            Request::prompt("test")
                .twitch_channels(["otherhome".into(), "foreign".into()])
                .endpoint(endpoint.clone())
        )
        .await,
        Err(LlmError::Disabled(_))
    ));
    tb_llm::brain_route::install(
        "title_ai",
        tb_llm::brain_route::BrainRoute::unavailable("must not dispatch"),
    )
    .unwrap();
    assert!(matches!(
        complete("title_ai", request("99").ledger_purpose("title")).await,
        Err(LlmError::Disabled(_))
    ));
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM llm_usage")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(attempts, 1);
    server.verify().await;
}

//! Prüft Verbrauch und Abbrüche ausschließlich in einer isolierten Testdatenbank.
use sqlx::postgres::PgPoolOptions;
use tb_engagement::llm_chat::{ChatMessage, EngagementLlmClient};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LLM_USAGE_SCHEMA: &str = include_str!("../../tb-llm/sqlx/schema.sql");

fn client_for(server: &MockServer) -> EngagementLlmClient {
    EngagementLlmClient::new(
        Some("test-key".to_string()),
        Some(server.uri()),
        Some("deepseek-v4-flash".to_string()),
        None,
    )
}

fn history() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".to_string(),
        content: "bebop auf der lane?".to_string(),
        name: Some("chatter1".to_string()),
    }]
}

async fn mock_usage(server: &MockServer, content: &str, prompt: i64, completion: i64) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": content}}],
            "usage": {"prompt_tokens": prompt, "completion_tokens": completion}
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn engagement_client_verbucht_usage_ins_zentrale_ledger() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TestDatabase {
        socket: String,
        database: String,
        user: String,
    }
    let Ok(config_text) = std::fs::read_to_string("/tmp/tb-llm-ledger-test-database.json") else {
        eprintln!("Ledger-DB-Test übersprungen: normale Testdatenbank-Konfigurationsdatei fehlt");
        return;
    };
    let config: TestDatabase =
        serde_json::from_str(&config_text).expect("Testdatenbankkonfiguration gültig");
    assert!(
        config.database.starts_with("fireworks_usage_test"),
        "Ausschließlich isolierte Ledger-Testdatenbanken verwenden"
    );
    let verify = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::postgres::PgConnectOptions::new()
                .host(&config.socket)
                .database(&config.database)
                .username(&config.user),
        )
        .await
        .expect("Test-DB verbinden");

    sqlx::query("DROP TABLE IF EXISTS public.llm_usage")
        .execute(&verify)
        .await
        .unwrap();
    sqlx::raw_sql(LLM_USAGE_SCHEMA)
        .execute(&verify)
        .await
        .expect("llm_usage-Schema anlegen");

    tb_llm::ledger::initialize(verify.clone(), "twitch-test", "ledger-side-effects").unwrap();

    let blockers = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            sqlx::postgres::PgConnectOptions::new()
                .host(&config.socket)
                .database(&config.database)
                .username(&config.user),
        )
        .await
        .expect("Isolierter Sperrpool");
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let mut lock = blockers.begin().await.unwrap();
    sqlx::query("LOCK TABLE public.llm_usage IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let start = std::time::Instant::now();
    let result = tb_llm::complete(
        "slow_start_test",
        tb_llm::Request::prompt("Fixture")
            .timeout(std::time::Duration::from_millis(100))
            .endpoint(tb_llm::selection::LlmEndpoint {
                provider: "fireworks",
                base_url: server.uri(),
                model: tb_llm::selection::configured_fireworks_model().into(),
                api_key: Some("synthetic-key".into()),
            }),
    )
    .await;
    assert!(matches!(result, Err(tb_llm::LlmError::Timeout(_))));
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    lock.rollback().await.unwrap();
    server.verify().await;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(150))
                .set_body_json(serde_json::json!({
                    "choices":[{"message":{"content":"Fixture"}}],
                    "usage":{"prompt_tokens":31,"completion_tokens":5,"total_tokens":36}
                })),
        )
        .mount(&server)
        .await;
    let start = std::time::Instant::now();
    let task = tokio::spawn(async move {
        tb_llm::complete(
            "slow_finish_test",
            tb_llm::Request::prompt("Fixture")
                .timeout(std::time::Duration::from_millis(400))
                .endpoint(tb_llm::selection::LlmEndpoint {
                    provider: "fireworks",
                    base_url: server.uri(),
                    model: tb_llm::selection::configured_fireworks_model().into(),
                    api_key: Some("synthetic-key".into()),
                }),
        )
        .await
    });
    let id = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM public.llm_usage WHERE purpose='slow_finish_test'",
            )
            .fetch_optional(&blockers)
            .await
            .unwrap();
            if let Some(id) = id {
                break id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Start ist vor HTTP sichtbar");
    let mut lock = blockers.begin().await.unwrap();
    sqlx::query("SELECT id FROM public.llm_usage WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("Abschluss hält Caller nicht unbegrenzt")
        .unwrap()
        .expect("Erhaltene Antwort bleibt nutzbar");
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    lock.rollback().await.unwrap();
    let total = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let total: Option<i64> =
                sqlx::query_scalar("SELECT total FROM public.llm_usage WHERE id=$1")
                    .bind(id)
                    .fetch_one(&blockers)
                    .await
                    .unwrap();
            if let Some(total) = total {
                break total;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Detached Abschluss trägt echte Fixtureusage nach");
    assert_eq!(total, 36);

    // 1) generate() → engagement 777/333.
    {
        let server = MockServer::start().await;
        mock_usage(&server, "klar", 777, 333).await;
        client_for(&server)
            .generate("system", &history(), 500, 480)
            .await
            .unwrap();
        let row: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT source, purpose, model FROM public.llm_usage \
             WHERE tokens_in = 777 AND tokens_out = 333 ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&verify)
        .await
        .expect("Ledger-Zeile mit 777/333 vorhanden");
        assert_eq!(row.0, "twitch-bot");
        assert_eq!(row.1.as_deref(), Some("engagement"));
        assert_eq!(row.2, Some(tb_llm::selection::configured_fireworks_model()));
    }

    // 2) raw_completion_tracked() → chat-deep-analysis 888/444.
    {
        let server = MockServer::start().await;
        mock_usage(&server, "tiefe analyse", 888, 444).await;
        let text = client_for(&server)
            .raw_completion_tracked("", "prompt", 5000, 0.1, "chat-deep-analysis")
            .await
            .unwrap();
        assert_eq!(text, "tiefe analyse");
        let row: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT source, purpose, model FROM public.llm_usage \
             WHERE tokens_in = 888 AND tokens_out = 444 ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&verify)
        .await
        .expect("Ledger-Zeile mit 888/444 vorhanden");
        assert_eq!(row.0, "twitch-bot");
        assert_eq!(row.1.as_deref(), Some("chat-deep-analysis"));
        assert_eq!(row.2, Some(tb_llm::selection::configured_fireworks_model()));
    }

    // Auch raw_completion() schreibt eine Verbrauchszeile mit 999/111.
    {
        let server = MockServer::start().await;
        mock_usage(&server, "x", 999, 111).await;
        client_for(&server)
            .raw_completion("", "p", 100, 0.4)
            .await
            .unwrap();
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM public.llm_usage \
             WHERE tokens_in = 999 AND tokens_out = 111",
        )
        .fetch_one(&verify)
        .await
        .expect("Count-Query");
        assert_eq!(count.0, 1, "Auch raw_completion muss im Ledger stehen");
    }

    for (status, body, state) in [
        (500, serde_json::json!({"error":"synthetic"}), "failed"),
        (
            200,
            serde_json::json!({"choices":[{"message":{"content":"ok"}}]}),
            "succeeded",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        let _ = client_for(&server)
            .raw_completion_tracked("", "p", 10, 0.1, "missing_usage_test")
            .await;
        let row:(String,Option<i64>,Option<i64>,Option<i64>)=sqlx::query_as("SELECT attempt_state,tokens_in,tokens_out,total FROM public.llm_usage WHERE purpose='missing_usage_test' ORDER BY id DESC LIMIT 1").fetch_one(&verify).await.unwrap();
        assert_eq!(row, (state.into(), None, None, None));
    }
    for body in [
        serde_json::json!([]),
        serde_json::json!("text"),
        serde_json::json!(42),
        serde_json::json!(true),
        serde_json::Value::Null,
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-request-id", "synthetic-malformed-request")
                    .set_body_json(body),
            )
            .mount(&server)
            .await;
        assert!(client_for(&server)
            .raw_completion_tracked("", "p", 10, 0.1, "malformed_response_test")
            .await
            .is_err());
        let row: (String, Option<i64>, Option<String>, Option<i32>, bool) = sqlx::query_as(
            "SELECT attempt_state,total,request_id,http_status,finished_at IS NOT NULL \
             FROM public.llm_usage WHERE purpose='malformed_response_test' ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&verify)
        .await
        .expect("Fehlerabschluss lesen");
        assert_eq!(
            row,
            (
                "failed".into(),
                None,
                Some("synthetic-malformed-request".into()),
                Some(200),
                true,
            )
        );
    }
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(5)))
        .mount(&server)
        .await;
    let task = tokio::spawn(async move {
        client_for(&server)
            .raw_completion_tracked("", "p", 10, 0.1, "cancelled_test")
            .await
    });
    loop {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM public.llm_usage WHERE purpose='cancelled_test'",
        )
        .fetch_one(&verify)
        .await
        .unwrap();
        if count > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    task.abort();
    let _ = task.await;
    let row: (String, Option<i64>) = sqlx::query_as(
        "SELECT attempt_state,total FROM public.llm_usage WHERE purpose='cancelled_test'",
    )
    .fetch_one(&verify)
    .await
    .unwrap();
    assert_eq!(row, ("started".into(), None));
    let id: i64 =
        sqlx::query_scalar("SELECT id FROM public.llm_usage WHERE purpose='cancelled_test'")
            .fetch_one(&verify)
            .await
            .unwrap();
    let completion = tb_llm::ledger::Completion {
        id,
        tokens_in: Some(12),
        tokens_out: Some(3),
        total: Some(15),
        request_id: Some("synthetic-request".into()),
        success: true,
        error_code: None,
        http_status: Some(200),
        latency_ms: 1,
    };
    assert!(tb_llm::ledger::recover_with_pool(
        &verify,
        &completion,
        "falsches-projekt",
        "ledger-side-effects"
    )
    .await
    .is_err());
    let saved = serde_json::to_string(&tb_llm::ledger::Recovery {
        project: "twitch-test".into(),
        service: "ledger-side-effects".into(),
        completion,
    })
    .unwrap();
    let record: tb_llm::ledger::Recovery = serde_json::from_str(&saved).unwrap();
    for _ in 0..2 {
        tb_llm::ledger::recover_with_pool(
            &verify,
            &record.completion,
            &record.project,
            &record.service,
        )
        .await
        .unwrap();
    }
    let count: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), SUM(total)::bigint FROM public.llm_usage WHERE purpose='cancelled_test'",
    )
    .fetch_one(&verify)
    .await
    .unwrap();
    assert_eq!(count, (1, 15));
    sqlx::query("ALTER TABLE public.llm_usage RENAME TO llm_usage_missing")
        .execute(&verify)
        .await
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    assert!(client_for(&server)
        .raw_completion_tracked("", "p", 10, 0.1, "blocked_test")
        .await
        .is_err());
    sqlx::query("ALTER TABLE public.llm_usage_missing RENAME TO llm_usage")
        .execute(&verify)
        .await
        .unwrap();
    verify.close().await;
}

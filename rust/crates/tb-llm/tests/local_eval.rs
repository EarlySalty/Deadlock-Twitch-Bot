#![cfg(feature = "local-eval")]

use serde_json::json;
use tb_llm::local_eval::{Dataset, LocalClient, Variant};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn fixture() -> serde_json::Value {
    json!({"schema_version":1,"cases":[{
        "case_id":"test-1","cutoff_unix_ms":3000,"twitch_user_id":"1186925760","channel_login":"target",
        "context":[{"created_at_unix_ms":2000,"author_id":"42","text":"hallo","kind":"chat"}],
        "style_examples":[{"created_at_unix_ms":1000,"author_id":"1186925760","channel_login":"other","text":"moin","context":[]}],
        "knowledge":[],"reference":{"created_at_unix_ms":3000,"text":"REFERENCE_CANARY"}
    }]})
}

#[test]
fn rejects_scope_models_and_nonlocal_destinations() {
    for url in [
        "https://127.0.0.1:1234/v1",
        "http://localhost:1234/v1",
        "http://127.0.0.2:1234/v1",
        "http://example.com:1234/v1",
        "http://127.0.0.1/v1",
        "http://user@127.0.0.1:1234/v1",
        "http://127.0.0.1:1234/v1?x=1",
        "http://127.0.0.1:1234/v1#x",
        "http://127.0.0.1:1234/v1/../v1",
    ] {
        assert!(
            LocalClient::new("twitch", url, "qwen3.5-4b-local").is_err(),
            "{url}"
        );
    }
    assert!(LocalClient::new("concierge", "http://127.0.0.1:1234/v1", "qwen3.5-4b-local").is_err());
    assert!(LocalClient::new("twitch", "http://127.0.0.1:1234/v1", "other-model").is_err());
    assert!(LocalClient::new("twitch", "http://[::1]:1234/v1", "qwen3.5-9b-local").is_ok());
}

#[test]
fn reference_is_not_generator_input() {
    let data: Dataset = serde_json::from_value(fixture()).unwrap();
    data.validate().unwrap();
    let improved = data.cases[0].prompt(Variant::Contextual).unwrap();
    let baseline = data.cases[0].prompt(Variant::Baseline).unwrap();
    assert!(!improved.contains("REFERENCE_CANARY"));
    assert!(!baseline.contains("REFERENCE_CANARY"));
    assert!(improved.contains("moin"));
    assert!(!baseline.contains("moin"));
    assert!(baseline.contains("hallo"));
    let mut changed = fixture();
    changed["cases"][0]["reference"]["text"] = json!("OTHER_REFERENCE");
    changed["cases"][0]["quality"] = json!({"phase_hint":"SECRET_LABEL"});
    changed["cases"][0]["style_examples"][0]["phase_hint"] = json!("SECRET_STYLE_LABEL");
    changed["cases"][0]["provenance"] = json!({"reference_metadata":"SECRET_METADATA"});
    let changed: Dataset = serde_json::from_value(changed).unwrap();
    assert_eq!(
        changed.cases[0].prompt(Variant::Contextual).unwrap(),
        improved
    );
    assert_eq!(
        changed.cases[0].prompt(Variant::Baseline).unwrap(),
        baseline
    );
}

#[test]
fn rejects_future_wrong_author_and_own_channel_style() {
    for (pointer, value) in [
        ("/cases/0/context/0/created_at_unix_ms", json!(3000)),
        ("/cases/0/style_examples/0/created_at_unix_ms", json!(3000)),
        ("/cases/0/style_examples/0/author_id", json!("42")),
        ("/cases/0/style_examples/0/channel_login", json!("target")),
        ("/cases/0/twitch_user_id", json!("42")),
    ] {
        let mut value_fixture = fixture();
        *value_fixture.pointer_mut(pointer).unwrap() = value;
        let data: Dataset = serde_json::from_value(value_fixture).unwrap();
        assert!(data.validate().is_err(), "{pointer}");
    }
    let mut value = fixture();
    value["cases"][0]["knowledge"] = json!([{"source":"fixture","title":"Fact","text":"example","scope":"twitch","documented_at":"2026-01-01T00:00:00Z"}]);
    assert!(serde_json::from_value::<Dataset>(value)
        .unwrap()
        .validate()
        .is_err());
}

#[tokio::test]
async fn local_transport_has_no_auth_and_does_not_follow_redirects() {
    let target = MockServer::start().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", format!("{}/leak", target.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = LocalClient::new(
        "twitch",
        &format!("{}/v1", server.uri()),
        "qwen3.5-4b-local",
    )
    .unwrap();
    let outcome = client
        .complete(
            tb_llm::Request::prompt("synthetic")
                .max_tokens(160)
                .timeout_secs(2)
                .denken_aus(),
        )
        .await;
    assert!(outcome.is_err());
    assert!(target.received_requests().await.unwrap().is_empty());
    let requests = server.received_requests().await.unwrap();
    assert!(!requests[0].headers.contains_key("authorization"));
}

#[tokio::test]
async fn flags_truncation_and_rejects_thinking_only_and_model_mismatch() {
    for (content, model, finish, expected) in [
        ("Antwort", "qwen3.5-4b-local", "length", true),
        ("<think>privat</think>", "qwen3.5-4b-local", "stop", false),
        ("<think>offen", "qwen3.5-4b-local", "stop", false),
        ("Antwort", "other-model", "stop", false),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model":model,"choices":[{"finish_reason":finish,"message":{"content":content,"reasoning_content":"internal"}}],
            "usage":{"prompt_tokens":12,"completion_tokens":4}
        }))).mount(&server).await;
        let client = LocalClient::new(
            "twitch",
            &format!("{}/v1", server.uri()),
            "qwen3.5-4b-local",
        )
        .unwrap();
        let response = client
            .complete(
                tb_llm::Request::prompt("synthetic")
                    .max_tokens(160)
                    .timeout_secs(2)
                    .denken_aus(),
            )
            .await;
        let response = response.unwrap();
        assert_eq!(response.error_code.is_none(), expected);
        assert_eq!(response.prompt_tokens, Some(12));
        assert_eq!(response.completion_tokens, Some(4));
        assert_eq!(response.finish_reason, finish);
        if !expected {
            assert!(response.text.is_empty());
        }
    }
}

#[tokio::test]
async fn replay_completes_all_failures_and_keeps_private_reports() {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use tb_llm::local_eval::{run, Config};
    let root = std::env::temp_dir().join(format!(
        "tb-replay-e2e-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let data = serde_json::to_vec(&fixture()).unwrap();
    let dataset = root.join("dataset.json");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&dataset)
        .unwrap()
        .write_all(&data)
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("SERVER_PRIVATE_CANARY"))
        .expect(4)
        .mount(&server)
        .await;
    let output = root.join("run");
    let config = Config {
        scope: "twitch".into(),
        base_url: format!("{}/v1", server.uri()),
        model: "qwen3.5-4b-local".into(),
        dataset_path: dataset,
        dataset_sha256: Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        output_dir: output.clone(),
        limit: 1,
        baseline_limit: 1,
        max_tokens: 160,
        timeout_secs: 2,
        temperature: 0.4,
        code_revision: "0123456789012345678901234567890123456789".into(),
        compare_with: None,
    };
    run(config.clone(), false).await.unwrap();
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["complete"], true);
    assert_eq!(summary["all"]["attempts"], 2);
    assert_eq!(summary["all"]["errors"], 2);
    for name in [
        "summary.json",
        "responses.jsonl",
        "comparison.html",
        "manifest.json",
        "REPORT.md",
    ] {
        let path = output.join(name);
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(!std::fs::read_to_string(path)
            .unwrap()
            .contains("SERVER_PRIVATE_CANARY"));
    }
    for request in server.received_requests().await.unwrap() {
        assert!(!String::from_utf8(request.body)
            .unwrap()
            .contains("REFERENCE_CANARY"));
    }
    assert!(std::fs::read_to_string(output.join("comparison.html"))
        .unwrap()
        .contains("REFERENCE_CANARY"));
    let mut second = Config {
        model: "qwen3.5-9b-local".into(),
        output_dir: root.join("second"),
        compare_with: Some(output.clone()),
        ..config
    };
    second.max_tokens = 159;
    assert_eq!(
        run(second.clone(), true).await.unwrap_err().0,
        "comparison_parameters"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    second.max_tokens = 160;
    let html_path = second.output_dir.join("comparison.html");
    run(second, false).await.unwrap();
    let html = std::fs::read_to_string(html_path).unwrap();
    assert!(html.contains("qwen3.5-4b-local"));
    assert!(html.contains("qwen3.5-9b-local"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn aborted_run_keeps_explicit_incomplete_checkpoint() {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use tb_llm::local_eval::{run, Config};
    let root = std::env::temp_dir().join(format!(
        "tb-replay-abort-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let data = serde_json::to_vec(&fixture()).unwrap();
    let dataset = root.join("dataset.json");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&dataset)
        .unwrap()
        .write_all(&data)
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_delay(std::time::Duration::from_secs(5)))
        .mount(&server)
        .await;
    let output = root.join("run");
    let config = Config {
        scope: "twitch".into(),
        base_url: format!("{}/v1", server.uri()),
        model: "qwen3.5-4b-local".into(),
        dataset_path: dataset,
        dataset_sha256: Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        output_dir: output.clone(),
        limit: 1,
        baseline_limit: 1,
        max_tokens: 160,
        timeout_secs: 10,
        temperature: 0.4,
        code_revision: "0123456789012345678901234567890123456789".into(),
        compare_with: None,
    };
    let task = tokio::spawn(run(config, false));
    for _ in 0..500 {
        if output.join("summary.json").exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["complete"], false);
    assert_eq!(summary["status"], "incomplete");
    assert_eq!(summary["planned"], 2);
    assert_eq!(summary["completed"], 0);
    std::fs::remove_dir_all(root).unwrap();
}

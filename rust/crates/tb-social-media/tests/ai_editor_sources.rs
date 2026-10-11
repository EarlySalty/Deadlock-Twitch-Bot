#[path = "../src/ai_editor/brain.rs"]
mod brain;
#[path = "../src/ai_editor/stt.rs"]
mod stt;

use std::{net::TcpListener, path::Path, time::Duration};

use brain::{retrieve_game_context, BrainRetrieveConfig, BrainStatus};
use stt::{SttError, SttJobConfig, SttLease};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

const VERIFIED_STT_SERVER: &str =
    "/home/nathanael/.worktrees/tb-social-ai-stt-deploy/rust/target/release/tb-stt-server";
const VERIFIED_STT_REVISION: &str = "30d39c41d2a4fcb6a3c938fc2e7eca70bbc53911";

fn assert_verified_server() {
    let output = std::process::Command::new(VERIFIED_STT_SERVER)
        .arg("--build-revision")
        .output()
        .expect("fresh reviewed STT server is required");
    assert!(output.status.success());
    assert_eq!(
        std::str::from_utf8(&output.stdout)
            .expect("build revision")
            .trim(),
        VERIFIED_STT_REVISION
    );
}

fn brain_config(server: &MockServer) -> BrainRetrieveConfig {
    BrainRetrieveConfig {
        endpoint: format!("{}/v1/retrieve", server.uri()),
        expected_release: "release-1".into(),
        deadline: Duration::from_secs(2),
        max_response_bytes: 4096,
        max_evidence_count: 3,
        max_evidence_bytes: 1024,
        allowed_game_terms: vec!["Haze".into(), "Abrams".into()],
    }
}

fn response(request_id: &str, term: &str) -> serde_json::Value {
    serde_json::json!({
        "contract_version": "brain.public.v1",
        "request_id": request_id,
        "knowledge_release": "release-1",
        "status": "answered",
        "evidence": [{
            "citation_id": "cite-1",
            "label": "Beleg 1",
            "text": format!("{term} kann mit dieser Fähigkeit Gegner im Kampf zurückdrängen und Distanz gewinnen."),
            "kind": "mechanic"
        }],
        "truncated": false,
        "out_of_domain": false
    })
}

#[test]
fn occupied_port_fails_without_attaching() {
    let owner = TcpListener::bind("127.0.0.1:0").expect("own listener");
    let port = owner.local_addr().expect("own address").port();
    assert_eq!(stt::check_port_available(port), Err(SttError::PortOccupied));
    assert!(owner.local_addr().is_ok());
}

#[tokio::test]
async fn occupied_port_never_creates_a_job_server() {
    assert_verified_server();
    let owner = TcpListener::bind("127.0.0.1:0").expect("own listener");
    let port = owner.local_addr().expect("own address").port();
    let job = tempfile::tempdir().expect("job directory");
    assert!(matches!(
        SttLease::start(&seed(), job.path(), &job_config(port)).await,
        Err(SttError::PortOccupied)
    ));
    assert!(!job.path().join("stt-transient").exists());
    assert!(owner.local_addr().is_ok());
}

#[tokio::test]
async fn health_requires_owned_fingerprint_before_audio() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok", "model": "ggml-large-v3-turbo-q5_0", "threads": 2,
            "backend": "whisper.cpp/whisper-rs", "config_fingerprint": "foreign"
        })))
        .mount(&server)
        .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("local client");
    let url = format!("{}/health", server.uri());
    assert_eq!(
        stt::probe_health(&client, &url, "owned", "ggml-large-v3-turbo-q5_0", 2).await,
        Ok(false)
    );
    assert_eq!(
        stt::probe_health(&client, &url, "foreign", "ggml-large-v3-turbo-q5_0", 2).await,
        Ok(true)
    );
}

#[tokio::test]
async fn brain_only_returns_term_bound_evidence_and_single_bearer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/retrieve"))
        .and(header("authorization", "Bearer mock-token"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).expect("synthetic JSON");
            assert_eq!(body["requested_scopes"], serde_json::json!(["bot.public"]));
            assert_eq!(body["profile"], "explain");
            assert_eq!(body["text"], "Deadlock: Haze");
            ResponseTemplate::new(200).set_body_json(response(
                body["request_id"].as_str().expect("request ID"),
                "Abrams",
            ))
        })
        .mount(&server)
        .await;
    let context = retrieve_game_context(&brain_config(&server), "mock-token", "Haze").await;
    assert_eq!(context.status, BrainStatus::NoRelevantContext);
    assert!(context.evidence.is_empty());
    assert_eq!(context.knowledge_release.as_deref(), Some("release-1"));
    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers.get_all("authorization").iter().count(),
        1
    );
}

#[tokio::test]
async fn brain_accepts_candidate_only_with_matching_identity_and_release() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/retrieve"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).expect("synthetic JSON");
            ResponseTemplate::new(200).set_body_json(response(
                body["request_id"].as_str().expect("request ID"),
                "Haze",
            ))
        })
        .mount(&server)
        .await;
    let context = retrieve_game_context(&brain_config(&server), "mock-token", "Haze").await;
    assert_eq!(context.status, BrainStatus::CandidateEvidence);
    assert_eq!(context.evidence.len(), 1);
    assert_eq!(context.evidence[0].kind, "mechanic");
    assert_eq!(context.evidence[0].citation_id, "cite-1");
    assert!(!context.evidence[0].label.is_empty());
    assert!(context.evidence[0].text.contains("Haze"));
    let mut wrong_release = brain_config(&server);
    wrong_release.expected_release = "release-2".into();
    assert_eq!(
        retrieve_game_context(&wrong_release, "mock-token", "Haze")
            .await
            .status,
        BrainStatus::Incomplete
    );
}

#[tokio::test]
async fn brain_rejects_wrong_identity_oversized_body_and_auth_failure() {
    let wrong_id = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response("wrong-id", "Haze")))
        .mount(&wrong_id)
        .await;
    assert_eq!(
        retrieve_game_context(&brain_config(&wrong_id), "mock-token", "Haze")
            .await
            .status,
        BrainStatus::Incomplete
    );

    let oversized = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(5000)))
        .mount(&oversized)
        .await;
    assert_eq!(
        retrieve_game_context(&brain_config(&oversized), "mock-token", "Haze")
            .await
            .status,
        BrainStatus::Incomplete
    );

    let denied = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("do not expose this body"))
        .mount(&denied)
        .await;
    let context = retrieve_game_context(&brain_config(&denied), "mock-token", "Haze").await;
    assert_eq!(context.status, BrainStatus::AuthenticationFailed);
    assert_eq!(context.http_status, Some(401));
    assert!(context.evidence.is_empty());
}

#[tokio::test]
async fn brain_rejects_non_loopback_without_network() {
    let server = MockServer::start().await;
    let mut config = brain_config(&server);
    config.endpoint = "http://example.com/v1/retrieve".into();
    assert_eq!(
        retrieve_game_context(&config, "mock-token", "Haze")
            .await
            .status,
        BrainStatus::Incomplete
    );
    assert_eq!(
        retrieve_game_context(&config, "mock-token", "Private Name")
            .await
            .status,
        BrainStatus::Incomplete
    );
    assert!(server
        .received_requests()
        .await
        .expect("requests")
        .is_empty());
}

fn seed() -> tb_config::BotConfigSnapshot {
    tb_config::BotConfigSnapshot::parse(
        "schema_version = 1\n[twitch]\nbot_user_id = \"1\"\nnotify_channel_id = \"2\"\neventsub_callback_url = \"https://example.invalid/callback\"\n[stt]\ntimeout_seconds = 240\nextraction_timeout_seconds = 300\n",
        Path::new("/tmp/ai-editor-test-seed.toml"),
    )
    .expect("synthetic seed")
}

fn job_config(port: u16) -> SttJobConfig {
    SttJobConfig {
        binary: VERIFIED_STT_SERVER.into(),
        model_directory: std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .expect("local home")
            .join(".cache/deadlock-stt"),
        legacy_cache_home: Some(std::env::var_os("HOME").expect("local home").into()),
        port,
        threads: 2,
        nice: 10,
        startup_deadline: Duration::from_secs(60),
        request_deadline: Duration::from_secs(180),
        extraction_deadline: Duration::from_secs(240),
        shutdown_deadline: Duration::from_secs(5),
        poll_interval: Duration::from_millis(250),
        max_upload_bytes: 25 * 1024 * 1024,
    }
}

#[cfg(unix)]
#[test]
fn stt_rejects_job_symlink_without_starting() {
    let root = tempfile::tempdir().expect("owned directory");
    let real = root.path().join("real");
    std::fs::create_dir(&real).expect("real job");
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).expect("alias");
    assert_eq!(
        stt::validate_job_dir(&alias),
        Err(SttError::DirectoryCollision)
    );
    assert!(!real.join("stt-transient").exists());
}

#[tokio::test]
async fn transient_real_stt_silence_and_cancellation_cleanup() {
    assert_verified_server();
    let port = TcpListener::bind("127.0.0.1:0").expect("reserve port");
    let number = port.local_addr().expect("reserved port").port();
    drop(port);
    let job = tempfile::tempdir().expect("job directory");
    let lease = SttLease::start(&seed(), job.path(), &job_config(number))
        .await
        .expect("real cached server ready");
    assert!(!lease.is_stopped());
    let clip = job.path().join("silence.wav");
    let status = std::process::Command::new("/usr/bin/ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=16000:cl=mono",
            "-t",
            "2",
            "-c:a",
            "pcm_s16le",
            "-y",
        ])
        .arg(&clip)
        .status()
        .expect("ffmpeg synthetic clip");
    assert!(status.success());
    let mut lease = lease;
    let result = lease.transcribe_clip(&clip).await.expect("local inference");
    assert!(result.text.is_empty(), "synthetic silence must stay silent");
    assert!(result.segments.is_empty());
    assert!(result.language.is_none());
    assert_eq!(result.model, "ggml-large-v3-turbo-q5_0");
    assert!(result.duration_seconds >= 1.9);
    assert!(result.startup_elapsed > Duration::ZERO);
    assert!(result.transcription_elapsed > Duration::ZERO);
    assert!(lease.is_stopped());
    let timing = lease.stop().await;
    assert!(timing.stopped);
    assert!(timing.startup_elapsed > Duration::ZERO);
    assert!(timing.total_elapsed >= timing.startup_elapsed);
    assert!(!job.path().join("stt-transient").exists());
    assert!(stt::check_port_available(number).is_ok());

    let lease = SttLease::start(&seed(), job.path(), &job_config(number))
        .await
        .expect("second server ready");
    let child_id = lease.child_id_for_test().expect("owned child");
    let task = tokio::spawn(async move {
        let _lease = lease;
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    task.abort();
    let _ = task.await;
    assert!(!job.path().join("stt-transient").exists());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !Path::new(&format!("/proc/{child_id}")).exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned child reaped");
    assert!(stt::check_port_available(number).is_ok());
}

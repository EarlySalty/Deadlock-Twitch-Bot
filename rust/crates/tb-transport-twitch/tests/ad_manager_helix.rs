//! Wire-Verträge für die nicht-idempotenten Twitch-Werbeaktionen.
//!
//! Beide POSTs dürfen bei einem mehrdeutigen Upstream-Fehler nicht automatisch
//! wiederholt werden. Ein Retry könnte sonst zwei Snoozes verbrauchen oder zwei
//! Commercials starten.

use tb_transport_twitch::{HelixClient, HelixConfig, HelixError};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_for(server: &MockServer) -> HelixClient {
    HelixClient::new(HelixConfig {
        client_id: "werbemanager-client".to_string(),
        client_secret: "nur-für-den-test".to_string(),
        token_url: format!("{}/oauth2/token", server.uri()),
        helix_base: format!("{}/helix", server.uri()),
    })
    .expect("Test-Client")
}

#[tokio::test]
async fn snooze_sendet_genau_den_twitch_wire_vertrag_und_parst_zahlenstrings() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/helix/channels/ads/schedule/snooze"))
        .and(query_param("broadcaster_id", "42"))
        .and(header("Client-Id", "werbemanager-client"))
        .and(header("Authorization", "Bearer user-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "snooze_count": "2",
                "snooze_refresh_at": "2026-09-01T14:30:00Z",
                "next_ad_at": "2026-09-01T14:35:00Z"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let outcome = client_for(&server)
        .snooze_next_ad("42", "user-token")
        .await
        .expect("Snooze-Antwort");

    assert_eq!(outcome.snooze_count, 2);
    assert_eq!(
        outcome.snooze_refresh_at.as_deref(),
        Some("2026-09-01T14:30:00Z")
    );
    assert_eq!(outcome.next_ad_at.as_deref(), Some("2026-09-01T14:35:00Z"));
    server.verify().await;
}

#[tokio::test]
async fn commercial_sendet_laenge_im_json_body_und_parst_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/helix/channels/commercial"))
        .and(header("Client-Id", "werbemanager-client"))
        .and(header("Authorization", "Bearer user-token"))
        .and(body_json(serde_json::json!({
            "broadcaster_id": "42",
            "length": 180
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "length": "180",
                "message": "Commercial gestartet",
                "retry_after": "480"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let outcome = client_for(&server)
        .start_commercial("42", 180, "user-token")
        .await
        .expect("Commercial-Antwort");

    assert_eq!(outcome.length, 180);
    assert_eq!(outcome.message, "Commercial gestartet");
    assert_eq!(outcome.retry_after, 480);
    server.verify().await;
}

#[tokio::test]
async fn nicht_idempotente_posts_werden_bei_upstream_fehler_nicht_wiederholt() {
    for endpoint in [
        "/helix/channels/ads/schedule/snooze",
        "/helix/channels/commercial",
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(500).set_body_string("Twitch wackelt"))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_for(&server);

        let error = if endpoint.ends_with("snooze") {
            client.snooze_next_ad("42", "user-token").await.unwrap_err()
        } else {
            client
                .start_commercial("42", 90, "user-token")
                .await
                .unwrap_err()
        };

        assert!(matches!(error, HelixError::Status { status: 500 }));
        server.verify().await;
    }
}

#[tokio::test]
async fn leeres_erfolgs_data_ist_ein_explizit_unbekannter_ausgang() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/helix/channels/ads/schedule/snooze"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
        .mount(&server)
        .await;

    let error = client_for(&server)
        .snooze_next_ad("42", "user-token")
        .await
        .unwrap_err();

    assert!(matches!(error, HelixError::AmbiguousOutcome { .. }));
}

#[tokio::test]
async fn leeres_snooze_objekt_ist_keine_erfolgsbestaetigung() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/helix/channels/ads/schedule/snooze"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{}]
        })))
        .mount(&server)
        .await;

    let error = client_for(&server)
        .snooze_next_ad("42", "user-token")
        .await
        .unwrap_err();

    assert!(matches!(error, HelixError::AmbiguousOutcome { .. }));
}

#[tokio::test]
async fn falsche_commercial_laenge_ist_keine_erfolgsbestaetigung() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/helix/channels/commercial"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "length": 60,
                "message": "Commercial gestartet",
                "retry_after": 480
            }]
        })))
        .mount(&server)
        .await;

    let error = client_for(&server)
        .start_commercial("42", 90, "user-token")
        .await
        .unwrap_err();

    assert!(matches!(error, HelixError::AmbiguousOutcome { .. }));
}

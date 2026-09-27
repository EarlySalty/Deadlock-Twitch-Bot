#[tokio::test]
async fn personal_invite_uses_verified_ids_and_reuses_the_broker_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PERSONAL_INVITE_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true, "result": {"invite_url": "https://discord.gg/ViewerCode", "personal": true}
        })))
        .expect(2)
        .mount(&server)
        .await;
    let relay = BrokerRelay::new(&test_config(&server.uri())).expect("relay fixture");
    for _ in 0..2 {
        let result = relay
            .personal_invite("Streamer", "42", "43")
            .await
            .expect("personal link");
        assert!(result.personal);
        assert_eq!(result.invite_url, "https://discord.gg/ViewerCode");
    }
    let requests = server.received_requests().await.expect("requests");
    assert_eq!(
        requests[0].headers.get("x-idempotency-key"),
        requests[1].headers.get("x-idempotency-key")
    );
    assert!(requests[0].headers.contains_key("x-internal-token"));
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("request JSON");
    assert_eq!(
        body,
        serde_json::json!({"streamer_login": "streamer", "streamer_twitch_user_id": "42", "inviter_twitch_user_id": "43"})
    );
}

#[tokio::test]
async fn personal_invite_accepts_channel_fallback_and_rejects_bad_envelopes() {
    for (ok, url, success) in [
        (true, "https://discord.gg/ChannelCode", true),
        (false, "https://discord.gg/ChannelCode", false),
        (true, "https://example.invalid/redirect", false),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PERSONAL_INVITE_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": ok, "result": {"invite_url": url, "personal": false}
            })))
            .mount(&server)
            .await;
        let relay = BrokerRelay::new(&test_config(&server.uri())).expect("relay fixture");
        let result = relay.personal_invite("streamer", "42", "43").await;
        assert_eq!(result.is_ok(), success);
        if let Ok(result) = result {
            assert!(!result.personal);
        }
    }
}

#[tokio::test]
async fn personal_invite_never_sends_to_non_loopback_or_for_invalid_platform_ids() {
    let server = MockServer::start().await;
    let mut relay = BrokerRelay::new(&test_config(&server.uri())).expect("relay fixture");
    for id in ["", "0", "invalid", "-1"] {
        assert!(relay.personal_invite("streamer", "42", id).await.is_err());
    }
    assert!(server
        .received_requests()
        .await
        .expect("requests")
        .is_empty());
    relay.base_url = "https://192.0.2.1".into();
    assert!(relay.personal_invite("streamer", "42", "43").await.is_err());
}

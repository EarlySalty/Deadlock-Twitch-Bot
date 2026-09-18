use serde_json::json;
use tb_transport_twitch::{HelixClient, HelixConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

async fn client(server: &MockServer) -> HelixClient {
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token":"synthetic","expires_in":3600})),
        )
        .mount(server)
        .await;
    HelixClient::new(HelixConfig {
        client_id: "synthetic-id".into(),
        client_secret: "synthetic-secret".into(),
        token_url: format!("{}/oauth2/token", server.uri()),
        helix_base: format!("{}/helix", server.uri()),
    })
    .unwrap()
}
#[tokio::test]
async fn category_pagination_has_no_1200_cap_no_language_filter_and_deduplicates() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    Mock::given(method("GET")).and(path("/helix/streams"))
        .respond_with(|request:&Request| {
            let params:std::collections::HashMap<_,_>=request.url.query_pairs().into_owned().collect();
            assert!(!params.contains_key("language"));
            assert_eq!(params["game_id"],"deadlock");
            let page=params.get("after").map(|v|v.parse::<usize>().unwrap()).unwrap_or(0);
            let count=if page==13 {1} else {100};
            let mut data:Vec<_>=(page*100..page*100+count).map(|i|json!({"id":format!("s{i}"),"user_id":format!("u{i}"),"game_id":"deadlock"})).collect();
            if page==1 {data.push(json!({"id":"s0","user_id":"u0","game_id":"deadlock"}));}
            ResponseTemplate::new(200).set_body_json(json!({"data":data,"pagination":if page<13 {json!({"cursor":(page+1).to_string()})}else{json!({})}}))
        }).expect(14).mount(&server).await;
    assert_eq!(
        client
            .get_all_streams_by_category("deadlock")
            .await
            .unwrap()
            .len(),
        1301
    );
    server.verify().await;
}
#[tokio::test]
async fn repeated_cursor_and_malformed_page_are_errors_not_partial_snapshots() {
    for body in [
        json!({"data":[{"id":"s","user_id":"u","game_id":"deadlock"}],"pagination":{"cursor":"same"}}),
        json!({}),
        json!({"data":[{"id":"s","user_id":"u","game_id":"wrong"}]}),
    ] {
        let server = MockServer::start().await;
        let client = client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        assert!(client
            .get_all_streams_by_category("deadlock")
            .await
            .is_err());
    }
}
#[tokio::test]
async fn empty_category_is_a_valid_complete_observation() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/helix/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[],"pagination":{}})))
        .mount(&server)
        .await;
    assert!(client
        .get_all_streams_by_category("deadlock")
        .await
        .unwrap()
        .is_empty());
}

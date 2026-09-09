#[tokio::test]
async fn statistik_http_json_und_fehlende_verknuepfung_sind_unterscheidbar() {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    for (route, response) in [
        ("/failed", ResponseTemplate::new(503)),
        (
            "/invalid",
            ResponseTemplate::new(200).set_body_string("not json"),
        ),
        (
            "/missing-field",
            ResponseTemplate::new(200).set_body_json(serde_json::json!({})),
        ),
        (
            "/unlinked",
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"linked":false})),
        ),
    ] {
        Mock::given(path(route))
            .respond_with(response)
            .mount(&server)
            .await;
    }
    for route in ["/failed", "/invalid", "/missing-field"] {
        let url = format!("{}{route}", server.uri());
        let rank = fetch_json::<RankInfo>(&url, &[], 1).await;
        let matches = fetch_json::<MatchHistory>(&url, &[], 1).await;
        let mmr = fetch_json::<MmrTrend>(&url, &[], 1).await;
        let live = fetch_json::<LiveStatus>(&url, &[], 1).await;
        assert!(
            rank.is_err() && matches.is_err() && mmr.is_err() && live.is_err(),
            "{route}"
        );
        for text in [
            command_reply("Streamer", rank.clone().map(Some), rank_reply),
            command_reply("Streamer", rank.map(Some), wins_reply),
            command_reply("Streamer", matches.clone().map(Some), winrate_reply),
            command_reply("Streamer", matches.clone().map(Some), lastmatch_reply),
            command_reply("Streamer", matches.clone().map(Some), streak_reply),
            command_reply("Streamer", matches.map(Some), mostplayed_reply),
            command_reply("Streamer", mmr.map(Some), mmr_reply),
            command_reply("Streamer", live.map(Some), live_reply),
        ] {
            assert!(
                text.contains("gerade nicht abrufen") && !text.contains("verknüpft"),
                "{text}"
            );
        }
    }
    let unlinked = fetch_json::<RankInfo>(&format!("{}/unlinked", server.uri()), &[], 1)
        .await
        .unwrap();
    assert!(rank_reply("Streamer", Some(&unlinked)).contains("keinen Steam-Account verknüpft"));
}

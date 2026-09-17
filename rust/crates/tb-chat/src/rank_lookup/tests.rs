include!("player_tests.rs");
use super::*;
use serde_json::json;
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

fn lookup(server: &MockServer) -> RankLookup {
    RankLookup::with_urls(
        &format!("{}/v1", server.uri()),
        &format!("{}/rank", server.uri()),
    )
}
async fn public_rank(server: &MockServer, id: u32, expected: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/players/{id}/rank")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "badge": 85, "rank": 8, "subrank": 5,
            "last_match": {"start_time": 1789603200}
        })))
        .expect(expected)
        .mount(server)
        .await;
    Mock::given(path("/v1/assets/ranks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"tier": 8, "name": "Oracle"}
        ])))
        .mount(server)
        .await;
}

#[test]
fn steam_ids_are_explicit_checked_and_do_not_steal_numeric_twitch_logins() {
    assert_eq!(parse_steam_id("12345"), Ok(None));
    assert_eq!(parse_steam_id("@12345"), Ok(None));
    assert_eq!(parse_steam_id("steam:12345"), Ok(Some(12345)));
    assert_eq!(parse_steam_id(" STEAM:76561197960278073 "), Ok(Some(12345)));
    for input in [
        "steam:",
        "steam:0",
        "steam:-1",
        "steam:76561197960265728",
        "steam:4294967296",
        "steam:18446744073709551616",
        "steam:1/../../rank",
        "steam:1 @other",
    ] {
        assert_eq!(parse_steam_id(input), Err(()), "{input}");
    }
    assert_eq!(parse_steam_id("éééé"), Ok(None));
}

#[test]
fn exact_name_is_only_a_unique_candidate_and_fuzzy_names_are_never_auto_selected() {
    let profiles = vec![
        SteamProfile {
            account_id: 10,
            personaname: "USER".into(),
        },
        SteamProfile {
            account_id: 11,
            personaname: "user_pro".into(),
        },
    ];
    assert!(
        matches!(select_profile("user", &profiles), ProfileSelection::Exact(p) if p.account_id == 10)
    );
    assert!(matches!(
        select_profile("usre", &profiles),
        ProfileSelection::Ambiguous(_)
    ));
    let profiles = vec![
        SteamProfile {
            account_id: 10,
            personaname: "user".into(),
        },
        SteamProfile {
            account_id: 11,
            personaname: "USER".into(),
        },
    ];
    assert!(matches!(
        select_profile("user", &profiles),
        ProfileSelection::Ambiguous(_)
    ));
    assert!(matches!(
        select_profile("user", &[]),
        ProfileSelection::None
    ));
}

#[test]
fn truncated_search_cannot_prove_uniqueness_and_duplicate_accounts_do_not_create_ambiguity() {
    let profiles: Vec<_> = (1..=100)
        .map(|id| SteamProfile {
            account_id: id,
            personaname: if id == 1 {
                "user".into()
            } else {
                format!("other{id}")
            },
        })
        .collect();
    assert!(matches!(
        select_profile("user", &profiles),
        ProfileSelection::Ambiguous(_)
    ));
    let duplicate = vec![
        SteamProfile {
            account_id: 10,
            personaname: "user".into(),
        },
        SteamProfile {
            account_id: 10,
            personaname: "user".into(),
        },
    ];
    assert!(matches!(
        select_profile("user", &duplicate),
        ProfileSelection::Exact(_)
    ));
}

#[tokio::test]
async fn public_rank_is_not_mmr_and_concurrent_calls_share_cache() {
    let server = MockServer::start().await;
    public_rank(&server, 12345, 1).await;
    let client = lookup(&server);
    let (one, two) = tokio::join!(
        client.account_reply(12345, "Viewer", false),
        client.account_reply(12345, "Viewer", false)
    );
    assert_eq!(one, two);
    assert!(one.contains("Rang von Viewer: Oracle 5"), "{one}");
    assert!(one.contains("Deadlock API; letztes erfasstes Ranked-Match"));
    assert!(!one.contains("MMR"));
    assert_eq!(client.account_reply(12345, "Viewer", false).await, one);
}

#[tokio::test]
async fn exact_name_fallback_is_labelled_unverified_and_search_includes_inactive_profiles() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/players/steam-search"))
        .and(query_param("search_query", "viewer"))
        .and(query_param("min_matches_played_last_30d", "0"))
        .and(query_param("matches_played_weight", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"account_id":12345,"personaname":"Viewer"},
            {"account_id":987,"personaname":"ViewerPro"}
        ])))
        .expect(1)
        .mount(&server)
        .await;
    public_rank(&server, 12345, 1).await;
    let text = lookup(&server).name_reply("viewer").await;
    assert!(text.contains("Steam-Namensfund Viewer"), "{text}");
    assert!(text.contains("Twitch-Zuordnung unbestätigt"));
    assert!(text.contains("Oracle 5"));
}

#[tokio::test]
async fn ambiguous_names_only_offer_explicit_ids_and_never_request_rank() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/players/steam-search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"account_id":1,"personaname":"Viewer"}, {"account_id":2,"personaname":"VIEWER"}
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let text = lookup(&server).name_reply("viewer").await;
    assert!(text.contains("!rank steam:1"));
    assert!(text.contains("!rank steam:2"));
    assert!(text.contains("Kein Rang automatisch zugeordnet"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn empty_search_404_is_not_a_service_outage() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/players/steam-search"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let text = lookup(&server).name_reply("viewer").await;
    assert!(text.contains("keinen passenden Steam-Namensfund"));
}

#[tokio::test]
async fn protected_account_does_not_fall_back_to_search_or_mmr() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/players/12/rank"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let text = lookup(&server).account_reply(12, "Viewer", false).await;
    assert!(text.contains("keine öffentlichen Rangdaten frei"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn no_rank_and_broken_payloads_never_invent_a_rank() {
    for (payload, expected) in [
        (
            json!({"badge":0,"rank":0,"subrank":0,"last_match":null}),
            "Kein Rang",
        ),
        (
            json!({"badge":85,"rank":7,"subrank":5}),
            "gerade nicht abrufen",
        ),
        (
            json!({"badge":87,"rank":8,"subrank":7}),
            "gerade nicht abrufen",
        ),
        (json!({"mmr":1500}), "gerade nicht abrufen"),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/v1/players/12/rank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload))
            .mount(&server)
            .await;
        let text = lookup(&server).account_reply(12, "Viewer", false).await;
        assert!(text.contains(expected), "{text}");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn rate_limits_and_failed_responses_are_cached_without_retry_storm() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/players/12/rank"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "60"))
        .expect(1)
        .mount(&server)
        .await;
    let client = lookup(&server);
    assert!(client
        .account_reply(12, "Viewer", false)
        .await
        .contains("ausgelastet"));
    assert!(client
        .account_reply(12, "Viewer", false)
        .await
        .contains("ausgelastet"));
    assert!(client
        .account_reply(13, "Other", false)
        .await
        .contains("ausgelastet"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn public_api_budget_is_shared_across_accounts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .expect(16)
        .mount(&server)
        .await;
    let client = lookup(&server);
    for id in 1..=16 {
        assert_eq!(client.account_reply(id, "Viewer", false).await, UNAVAILABLE);
    }
    assert!(client
        .account_reply(17, "Viewer", false)
        .await
        .contains("ausgelastet"));
}

#[tokio::test]
async fn expired_cache_refreshes_and_old_entries_are_bounded() {
    let server = MockServer::start().await;
    public_rank(&server, 12, 2).await;
    let client = lookup(&server);
    client.account_reply(12, "Viewer", false).await;
    for entry in client.cache.lock().await.entries.values_mut() {
        entry.created -= Duration::from_secs(4000);
    }
    client.account_reply(12, "Viewer", false).await;
    assert!(client.cache.lock().await.entries.len() <= 256);
}

async fn database() -> crate::test_postgres::TestPostgres {
    let db = crate::test_postgres::TestPostgres::start().await;
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20260918100000_twitch_player_steam_links.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TABLE twitch_streamer_identities(twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT);
        CREATE SCHEMA core;
        CREATE TABLE core.steam_links(discord_id BIGINT, steam_id TEXT, verified BOOLEAN, primary_account BOOLEAN, linked_at TIMESTAMPTZ);
        INSERT INTO twitch_streamer_identities VALUES ('target', '101');
        INSERT INTO core.steam_links VALUES (101, '76561197960278073', TRUE, TRUE, NOW()),
            (101, '76561197960266715', TRUE, FALSE, NOW());")
        .execute(&db.pool).await.unwrap();
    db
}
fn target() -> CommandTarget {
    CommandTarget {
        user_id: "target".into(),
        login: "viewer".into(),
        name: "Viewer".into(),
    }
}

#[tokio::test]
async fn verified_primary_gc_rank_wins_without_any_public_request() {
    let db = database().await;
    let server = MockServer::start().await;
    Mock::given(path("/rank"))
        .and(query_param("discord_id", "101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "linked":true,"verified":true,"is_steam_friend":true,"rank_name":"Oracle","subrank":4
        })))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        lookup(&server)
            .twitch_reply(&db.pool, &target(), true)
            .await,
        "Rang von Viewer: Oracle 4"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn missing_friend_missing_rank_and_gc_outage_use_known_primary_steam_id() {
    let db = database().await;
    for response in [
        ResponseTemplate::new(200)
            .set_body_json(json!({"linked":true,"verified":true,"is_steam_friend":false})),
        ResponseTemplate::new(200).set_body_json(
            json!({"linked":true,"verified":true,"is_steam_friend":true,"rank_name":null}),
        ),
        ResponseTemplate::new(503),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/rank"))
            .and(query_param("discord_id", "101"))
            .respond_with(response)
            .mount(&server)
            .await;
        public_rank(&server, 12345, 1).await;
        let text = lookup(&server)
            .twitch_reply(&db.pool, &target(), true)
            .await;
        assert!(text.starts_with("Rang von Viewer: Oracle 5"), "{text}");
        assert!(!text.contains("unbestätigt"));
        assert!(!server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|r| r.url.path().contains("steam-search")));
    }
}

#[tokio::test]
async fn unverified_primary_is_not_replaced_by_verified_secondary_or_similar_name() {
    let db = database().await;
    sqlx::query("UPDATE core.steam_links SET verified=FALSE WHERE primary_account=TRUE")
        .execute(&db.pool)
        .await
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/rank"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let text = lookup(&server)
        .twitch_reply(&db.pool, &target(), true)
        .await;
    assert!(text.contains("noch nicht bestätigt"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn missing_identity_database_is_not_treated_as_no_link() {
    let db = crate::test_postgres::TestPostgres::start().await;
    let server = MockServer::start().await;
    assert_eq!(
        lookup(&server)
            .twitch_reply(&db.pool, &target(), true)
            .await,
        UNAVAILABLE
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn missing_steam_database_is_not_treated_as_no_link() {
    let db = database().await;
    sqlx::query("DROP TABLE core.steam_links")
        .execute(&db.pool)
        .await
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/rank"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        lookup(&server)
            .twitch_reply(&db.pool, &target(), true)
            .await,
        UNAVAILABLE
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[test]
fn labels_cannot_inject_newlines_mentions_or_long_chat_messages() {
    let label = chat_label(&format!("@bad\r\n{}", "x".repeat(200)));
    assert!(!label.contains(['@', '\r', '\n']));
    assert!(label.chars().count() <= 40);
}

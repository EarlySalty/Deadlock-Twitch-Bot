use std::time::Instant;

use axum::body::Bytes;
use axum::http::Method;
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::DiscordRest;
use crate::{DirectDiscord, HubError, InboundRequest, OutboundResponse};

const GUILD: u64 = 42;
const TOKEN: &str = "test-bot-token";

fn rest(server: &MockServer) -> DiscordRest {
    DiscordRest::with_base_url(TOKEN, &server.uri()).expect("client")
}

fn post(path: &str, key: Option<&str>, body: Value) -> InboundRequest {
    InboundRequest {
        method: Method::POST,
        path: format!("/internal/master/v1/{path}"),
        query: None,
        idempotency_key: key.map(str::to_string),
        content_type: Some("application/json".to_string()),
        body: Bytes::from(body.to_string()),
    }
}

fn get(path: &str, query: &str) -> InboundRequest {
    InboundRequest {
        method: Method::GET,
        path: format!("/internal/master/v1/{path}"),
        query: Some(query.to_string()),
        idempotency_key: None,
        content_type: None,
        body: Bytes::new(),
    }
}

fn json_of(resp: &OutboundResponse) -> Value {
    serde_json::from_slice(&resp.body).expect("JSON-Antwort")
}

async fn mount_channel(server: &MockServer, channel_id: u64, guild_id: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/channels/{channel_id}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"id": channel_id.to_string(), "guild_id": guild_id.to_string()}),
            ),
        )
        .mount(server)
        .await;
}

fn rich_payload() -> Value {
    json!({
        "channel_id": 111,
        "content": "Live!",
        "embed": {"title": "Stream"},
        "allowed_role_ids": [7, GUILD],
        "view_spec": null,
    })
}

#[tokio::test]
async fn send_rich_message_liefert_broker_antwortformat() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .and(header("authorization", format!("Bot {TOKEN}").as_str()))
        .and(body_partial_json(json!({
            "content": "Live!",
            "embeds": [{"title": "Stream"}],
            "allowed_mentions": {"parse": [], "users": [], "roles": ["7"]},
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "999000111222333444"})))
        .expect(1)
        .mount(&server)
        .await;

    let resp = rest(&server)
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("k-1"), rich_payload()),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 200);
    assert_eq!(resp.content_type.as_deref(), Some("application/json"));
    let body = json_of(&resp);
    assert_eq!(body["ok"], true);
    assert_eq!(body["cached"], false);
    assert_eq!(body["idempotency_key"], "k-1");
    assert!(body["error"].is_null());
    assert_eq!(body["result"]["channel_id"], 111);
    assert_eq!(body["result"]["message_id"], "999000111222333444");
}

#[tokio::test]
async fn components_v2_setzt_flag_und_twitch_button_wird_link() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .and(body_partial_json(json!({
            "flags": 32768,
            "components": [
                {"type": 17, "components": []},
                {"type": 1, "components": [{"type": 2, "style": 5, "label": "Watch", "url": "https://www.twitch.tv/x"}]}
            ],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "5"})))
        .expect(1)
        .mount(&server)
        .await;

    let resp = rest(&server)
        .handle(
            GUILD,
            post(
                "discord/send-rich-message",
                Some("k-v2"),
                json!({
                    "channel_id": "111",
                    "content": null,
                    "embed": {},
                    "components": [{"type": 17, "components": []}],
                    "allowed_role_ids": [],
                    "view_spec": {
                        "type": "twitch_live_tracking",
                        "streamer_login": "x",
                        "referral_url": "https://www.twitch.tv/x",
                        "tracking_token": "t",
                        "button_label": "Watch",
                    },
                }),
            ),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 200, "{:?}", json_of(&resp));
}

#[tokio::test]
async fn kanal_einer_anderen_guild_wird_abgelehnt() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, 1).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "1"})))
        .expect(0)
        .mount(&server)
        .await;

    let client = rest(&server);
    let resp = client
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("k-x"), rich_payload()),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 403);
    let body = json_of(&resp);
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"]["code"], "forbidden");

    let resp = client
        .handle(
            GUILD,
            post(
                "discord/delete-message",
                Some("k-d"),
                json!({"channel_id": 111, "message_id": "5", "reason": "x"}),
            ),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 403);
}

#[tokio::test]
async fn rate_limit_429_wird_abgewartet_und_wiederholt() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("x-ratelimit-bucket", "b1")
                .set_body_json(json!({"message": "You are being rate limited.", "retry_after": 0.2, "global": false})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "77"})))
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;

    let started = Instant::now();
    let resp = rest(&server)
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("k-rl"), rich_payload()),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 200);
    assert_eq!(json_of(&resp)["result"]["message_id"], "77");
    assert!(
        started.elapsed().as_millis() >= 200,
        "retry_after nicht abgewartet"
    );
}

#[tokio::test]
async fn rate_limit_gibt_nach_drei_versuchen_auf() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .respond_with(
            ResponseTemplate::new(429).set_body_json(json!({"retry_after": 0.01, "global": false})),
        )
        .expect(3)
        .mount(&server)
        .await;
    let resp = rest(&server)
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("k-rl3"), rich_payload()),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 502);
}

#[tokio::test]
async fn sonstige_4xx_werden_nicht_wiederholt() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"code": 50013, "message": "Missing Permissions"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let resp = rest(&server)
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("k-403"), rich_payload()),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 502);
    assert_eq!(json_of(&resp)["error"]["code"], "discord_error");
}

#[tokio::test]
async fn idempotenz_wiederholung_liefert_gleiche_antwort() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "123"})))
        .expect(1)
        .mount(&server)
        .await;

    let client = rest(&server);
    let first = client
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("same"), rich_payload()),
        )
        .await
        .expect("erste");
    let second = client
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("same"), rich_payload()),
        )
        .await
        .expect("zweite");
    let (a, b) = (json_of(&first), json_of(&second));
    assert_eq!(second.status, 200);
    assert_eq!(a["result"], b["result"]);
    assert_eq!(a["request_id"], b["request_id"]);
    assert_eq!(b["cached"], true);

    let mut changed = rich_payload();
    changed["content"] = json!("anders");
    let conflict = client
        .handle(
            GUILD,
            post("discord/send-rich-message", Some("same"), changed),
        )
        .await
        .expect("konflikt");
    assert_eq!(conflict.status, 409);
    assert_eq!(json_of(&conflict)["error"]["code"], "idempotency_conflict");

    let missing = client
        .handle(
            GUILD,
            post("discord/send-rich-message", None, rich_payload()),
        )
        .await
        .expect("ohne key");
    assert_eq!(missing.status, 400);
    assert_eq!(
        json_of(&missing)["error"]["message"],
        "idempotency_key is required"
    );
}

#[tokio::test]
async fn add_role_nur_in_eigener_guild() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(format!("/guilds/{GUILD}/members/5/roles/7")))
        .and(header("x-audit-log-reason", "twitch-bot"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = rest(&server);
    let resp = client
        .handle(
            GUILD,
            post(
                "discord/member/add-role",
                Some("r-1"),
                json!({"guild_id": GUILD, "user_id": 5, "role_id": 7, "reason": "twitch-bot"}),
            ),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 200);
    let body = json_of(&resp);
    assert_eq!(body["ok"], true);
    assert_eq!(
        body["result"],
        json!({"guild_id": GUILD, "user_id": 5, "role_id": 7})
    );

    let resp = client
        .handle(
            GUILD,
            post(
                "discord/member/add-role",
                Some("r-2"),
                json!({"guild_id": 1, "user_id": 5, "role_id": 7, "reason": "x"}),
            ),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 403);
}

#[tokio::test]
async fn create_role_verwendet_vorhandene_rolle() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/guilds/{GUILD}/roles")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{"id": "900", "name": "Live", "position": 3}])),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/guilds/{GUILD}/roles")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "1"})))
        .expect(0)
        .mount(&server)
        .await;
    let resp = rest(&server)
        .handle(
            GUILD,
            post(
                "discord/role/create",
                Some("c-1"),
                json!({"guild_id": GUILD.to_string(), "name": "Live", "mentionable": true, "reason": "x"}),
            ),
        )
        .await
        .expect("antwort");
    assert_eq!(resp.status, 200);
    let body = json_of(&resp);
    assert_eq!(body["result"]["role_id"], 900);
    assert_eq!(body["result"]["guild_id"], GUILD.to_string());
}

#[tokio::test]
async fn roles_und_fremde_guild_im_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/guilds/{GUILD}/roles")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{"id": "900", "name": "Live", "position": 3}])),
        )
        .mount(&server)
        .await;
    let client = rest(&server);
    let resp = client
        .handle(GUILD, get("discord/roles", &format!("guild_id={GUILD}")))
        .await
        .expect("antwort");
    assert_eq!(resp.status, 200);
    let body = json_of(&resp);
    assert_eq!(body["roles"][0]["id"], "900");
    assert_eq!(body["guild_id"], GUILD.to_string());

    let resp = client
        .handle(GUILD, get("discord/roles", "guild_id=1"))
        .await
        .expect("antwort");
    assert_eq!(resp.status, 403);
}

#[tokio::test]
async fn unbekannte_pfade_sind_unsupported() {
    let server = MockServer::start().await;
    let client = rest(&server);
    for p in [
        "discord/twitch-links",
        "discord/streamer-voice-invite",
        "twitch/personal-invite",
        "clips/submit",
    ] {
        let err = client
            .handle(GUILD, post(p, Some("u"), json!({})))
            .await
            .expect_err("unsupported");
        assert!(matches!(err, HubError::Unsupported(_)), "{p}: {err:?}");
        assert_eq!(err.status(), 501);
    }
}

#[tokio::test]
async fn debug_zeigt_kein_token() {
    let server = MockServer::start().await;
    let rendered = format!("{:?}", rest(&server));
    assert!(!rendered.contains(TOKEN));
}

#[tokio::test]
async fn malformed_member_page_never_claims_complete_live_result() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/guilds/42/roles"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{"id": "7", "name": "Live"}])),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/guilds/42/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"unexpected": true})))
        .expect(1)
        .mount(&server)
        .await;
    let response = rest(&server)
        .handle(GUILD, get("discord/role-members", "role_id=7&live=true"))
        .await
        .expect("response");
    assert_eq!(response.status, 503);
    assert_eq!(json_of(&response)["error"]["code"], "unavailable");
    assert_ne!(json_of(&response)["complete"], true);
}

#[tokio::test]
async fn ambiguous_write_failures_are_not_reexecuted() {
    for (action, verb, route, payload) in [
        (
            "discord/send-rich-message",
            "POST",
            "/channels/111/messages",
            rich_payload(),
        ),
        (
            "discord/edit-rich-message",
            "PATCH",
            "/channels/111/messages/5",
            json!({"channel_id": 111, "message_id": 5, "content": "new", "embed": {}}),
        ),
        (
            "discord/delete-message",
            "DELETE",
            "/channels/111/messages/5",
            json!({"channel_id": 111, "message_id": 5, "reason": "test"}),
        ),
        (
            "discord/send-dm",
            "POST",
            "/channels/222/messages",
            json!({"user_id": 5, "content": "test"}),
        ),
        (
            "discord/member/add-role",
            "PUT",
            "/guilds/42/members/5/roles/7",
            json!({"guild_id": GUILD, "user_id": 5, "role_id": 7}),
        ),
        (
            "discord/member/remove-role",
            "DELETE",
            "/guilds/42/members/5/roles/7",
            json!({"guild_id": GUILD, "user_id": 5, "role_id": 7}),
        ),
        (
            "discord/role/create",
            "POST",
            "/guilds/42/roles",
            json!({"guild_id": GUILD, "name": "new"}),
        ),
        (
            "discord/create-invite",
            "POST",
            "/channels/111/invites",
            json!({"channel_id": 111}),
        ),
    ] {
        let server = MockServer::start().await;
        mount_channel(&server, 111, GUILD).await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/members/5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"user": {"id": "5"}})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/users/@me/channels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "222"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/roles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;
        let client = rest(&server);
        let first = client
            .handle(GUILD, post(action, Some("ambiguous"), payload.clone()))
            .await
            .expect("first");
        let second = client
            .handle(GUILD, post(action, Some("ambiguous"), payload))
            .await
            .expect("retry");
        assert_eq!(first.status, 502, "{action}");
        assert_eq!(second.status, 502, "{action}");
        assert_eq!(json_of(&second)["cached"], true, "{action}");
    }
}

#[tokio::test]
async fn transport_failure_keeps_write_reservation() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    Mock::given(method("POST"))
        .and(path("/channels/111/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "5"}))
                .set_delay(std::time::Duration::from_secs(11)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = rest(&server);
    for cached in [false, true] {
        let response = client
            .handle(
                GUILD,
                post("discord/send-rich-message", Some("timeout"), rich_payload()),
            )
            .await
            .expect("response");
        assert_eq!(response.status, 502);
        assert_eq!(json_of(&response)["cached"], cached);
    }
}

#[tokio::test]
async fn edits_clear_prior_state() {
    for (payload, expected) in [
        (
            json!({"channel_id": 111, "message_id": 5, "content": "new", "embed": {}, "view_spec": null}),
            json!({"content": "new", "embeds": [], "components": []}),
        ),
        (
            json!({"channel_id": 111, "message_id": 5, "embed": {}, "components": [{"type": 10, "content": "new"}]}),
            json!({"content": null, "embeds": [], "flags": 32768, "components": [{"type": 10, "content": "new"}]}),
        ),
        (
            json!({"channel_id": 111, "message_id": 5, "content": "new", "embed": {}, "components": []}),
            json!({"content": "new", "embeds": [], "components": []}),
        ),
    ] {
        let server = MockServer::start().await;
        mount_channel(&server, 111, GUILD).await;
        Mock::given(method("PATCH"))
            .and(path("/channels/111/messages/5"))
            .and(body_partial_json(expected))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "5"})))
            .expect(1)
            .mount(&server)
            .await;
        let response = rest(&server)
            .handle(
                GUILD,
                post("discord/edit-rich-message", Some("edit"), payload),
            )
            .await
            .expect("response");
        assert_eq!(response.status, 200);
    }
}

#[tokio::test]
async fn components_do_not_discard_text_or_embeds() {
    let server = MockServer::start().await;
    let client = rest(&server);
    for action in ["discord/send-rich-message", "discord/edit-rich-message"] {
        for field in ["content", "embed"] {
            let mut payload = json!({"channel_id": 111, "message_id": 5, "embed": {}, "components": [{"type": 10, "content": "new"}]});
            payload[field] = if field == "content" {
                json!("text")
            } else {
                json!({"title": "embed"})
            };
            let response = client
                .handle(GUILD, post(action, Some("mixed"), payload))
                .await
                .expect("response");
            assert_eq!(response.status, 400);
        }
    }
    assert!(server
        .received_requests()
        .await
        .expect("requests")
        .is_empty());
}

#[tokio::test]
async fn lookup_errors_are_not_absence() {
    for status in [403, 429, 500, 404] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"retry_after": 20})))
            .mount(&server)
            .await;
        let client = rest(&server);
        for request in [
            post("discord/resolve-user", None, json!({"user_id": 5})),
            get("discord/roles", ""),
            get("discord/members", ""),
            get("discord/role-members", "role_id=7"),
        ] {
            let resolve = request.path.ends_with("resolve-user");
            let response = client.handle(GUILD, request).await.expect("response");
            let expected = if status == 404 {
                if resolve {
                    200
                } else {
                    404
                }
            } else {
                502
            };
            assert_eq!(response.status, expected, "upstream {status}");
            if status != 404 {
                assert_eq!(json_of(&response)["error"]["code"], "discord_error");
            } else if resolve {
                assert_eq!(json_of(&response)["result"]["found"], false);
            }
        }
    }
}

#[tokio::test]
async fn role_member_errors_and_malformed_roles_are_not_missing_roles() {
    for malformed in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/roles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(if malformed {
                json!({"unexpected": true})
            } else {
                json!([{"id": "7", "name": "Live"}])
            }))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/members"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let response = rest(&server)
            .handle(GUILD, get("discord/role-members", "role_id=7"))
            .await
            .expect("response");
        assert_eq!(response.status, 502);
        assert_eq!(json_of(&response)["error"]["code"], "discord_error");
    }
}

#[tokio::test]
async fn recoverable_failures_are_retryable() {
    for (verb, route, status) in [
        ("GET", "/channels/111", 500),
        ("POST", "/channels/111/messages", 403),
        ("POST", "/channels/111/messages", 429),
    ] {
        let server = MockServer::start().await;
        mount_channel(&server, 111, GUILD).await;
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"retry_after": 0})))
            .up_to_n_times(if status == 429 { 3 } else { 1 })
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/channels/111/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "9"})))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;
        let client = rest(&server);
        for expected in [502, 200] {
            let response = client
                .handle(
                    GUILD,
                    post("discord/send-rich-message", Some("retry"), rich_payload()),
                )
                .await
                .expect("response");
            assert_eq!(response.status, expected);
            assert_eq!(json_of(&response)["cached"], false);
        }
    }
}

#[tokio::test]
async fn explicit_user_mentions_are_honoured() {
    let server = MockServer::start().await;
    mount_channel(&server, 111, GUILD).await;
    for (action, verb, route) in [
        (
            "discord/send-rich-message",
            "POST",
            "/channels/111/messages",
        ),
        (
            "discord/edit-rich-message",
            "PATCH",
            "/channels/111/messages/5",
        ),
    ] {
        Mock::given(method(verb))
            .and(path(route))
            .and(body_partial_json(
                json!({"allowed_mentions": {"parse": [], "users": ["5"], "roles": ["7"]}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "9"})))
            .expect(1)
            .mount(&server)
            .await;
        let mut payload = rich_payload();
        payload["allowed_user_ids"] = json!([5]);
        payload["message_id"] = json!(5);
        assert_eq!(
            rest(&server)
                .handle(GUILD, post(action, Some("mentions"), payload))
                .await
                .expect("response")
                .status,
            200
        );
    }
}

#[tokio::test]
async fn malformed_members_and_repeated_pages_fail() {
    for page in [
        json!([{}]),
        json!(vec![
            json!({"user": {"id": "5", "username": "test"}, "roles": ["7"]});
            1000
        ]),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/roles"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{"id": "7", "name": "Live"}])),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page))
            .mount(&server)
            .await;
        let client = rest(&server);
        for (request, expected) in [
            (get("discord/members", ""), 502),
            (get("discord/role-members", "role_id=7&live=true"), 503),
        ] {
            let response = client.handle(GUILD, request).await.expect("response");
            assert_eq!(response.status, expected);
            assert_ne!(json_of(&response)["complete"], true);
        }
    }
}

#[tokio::test]
async fn malformed_role_discovery_never_writes_and_can_retry() {
    for body in ["not-json", "{}", "[{}]"] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/roles"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/guilds/42/roles"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{"id": "7", "name": "Live"}])),
            )
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/guilds/42/roles"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let client = rest(&server);
        for expected in [502, 200] {
            let response = client
                .handle(
                    GUILD,
                    post(
                        "discord/role/create",
                        Some("role"),
                        json!({"guild_id": GUILD, "name": "Live"}),
                    ),
                )
                .await
                .expect("response");
            assert_eq!(response.status, expected);
        }
    }
}

#[tokio::test]
async fn cancellation_settles_reservations_without_duplicate_writes() {
    for wrote in [false, true] {
        let server = MockServer::start().await;
        let client = rest(&server);
        let ctx = super::Ctx {
            guild_id: GUILD,
            rid: "cancel".into(),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let operation = async {
            super::WRITE_UNCERTAIN
                .with(|state| state.store(wrote, std::sync::atomic::Ordering::Relaxed));
            let _ = tx.send(());
            std::future::pending::<(u16, Value)>().await
        };
        let mut executing =
            Box::pin(client.run_idempotent(&ctx, "cancel", "key", "hash", operation));
        tokio::select! { _ = &mut executing => panic!("must remain pending"), _ = rx => {} }
        drop(executing);
        let response = client
            .run_idempotent(&ctx, "cancel", "key", "hash", async {
                assert!(!wrote, "must not duplicate an ambiguous write");
                (200, json!({"ok": true}))
            })
            .await;
        assert_eq!(response.status, if wrote { 502 } else { 200 });
    }
}

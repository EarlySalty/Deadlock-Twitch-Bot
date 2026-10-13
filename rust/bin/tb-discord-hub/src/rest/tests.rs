//! Tests der Discord-REST-Ausführung gegen einen wiremock-Discord.

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
            // Nur Rolle 7; @everyone (Guild-ID) herausgefiltert, keine parse-Erwähnungen.
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
    // message_id als String – so parst ihn der Twitch-Bot (SendResult).
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

    // Auch Löschen/Bearbeiten in fremden Kanälen ist gesperrt.
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

    // Gleicher Schlüssel, anderer Inhalt → Konflikt.
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

    // Ohne Schlüssel → 400 wie im Broker.
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

    // guild_id einer anderen Guild → 403, kein Discord-Aufruf.
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

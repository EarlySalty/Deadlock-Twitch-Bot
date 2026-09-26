//! Steam owns presence data; the Twitch database owns the identity selection.
//! Failed or incomplete reads never certify an advertising window.
use super::{SteamMatchState, SteamMatchSummary, MATCH_STATUS_FRESH_SECS};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use std::{sync::Arc, time::Duration as StdDuration};

const STEAM_BASE: &str = "http://127.0.0.1:8783";
const STEAM64_BASE: i64 = 76_561_197_960_265_728;
const MAX_BODY: usize = 16 * 1024;

#[derive(Clone)]
pub(super) struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<Arc<str>>,
}

impl Client {
    pub(super) fn new() -> Self {
        Self::with_base(STEAM_BASE, std::env::var("TWITCH_INTERNAL_API_TOKEN").ok())
    }

    fn with_base(base: &str, token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .timeout(StdDuration::from_secs(3))
                .build()
                .expect("valid static Steam HTTP configuration"),
            base: base.trim_end_matches('/').into(),
            token: token.filter(|s| !s.trim().is_empty()).map(Arc::from),
        }
    }

    async fn fetch(&self, identity: &Identity) -> Option<Value> {
        let request = match identity {
            Identity::Steam { id, .. } => self
                .http
                .get(format!("{}/internal/player-live", self.base))
                .query(&[("steam_id", id.to_string())])
                .header("x-internal-token", self.token.as_deref()?),
            Identity::Discord(id) => self
                .http
                .get(format!("{}/player-live", self.base))
                .query(&[("discord_id", id)]),
            Identity::None | Identity::Disabled => return None,
        };
        let mut response = request.send().await.ok()?.error_for_status().ok()?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY as u64)
        {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len().saturating_add(chunk.len()) > MAX_BODY {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).ok()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Identity {
    None,
    Disabled,
    Steam { id: i64, revision: Option<i64> },
    Discord(String),
}

fn valid_steam_id(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<i64>()
        .ok()
        .filter(|id| *id > STEAM64_BASE && *id <= STEAM64_BASE + i64::from(u32::MAX))
}

async fn identity(pool: &PgPool, uid: &str) -> Result<Identity, sqlx::Error> {
    if let Some(row) = sqlx::query("SELECT steam_id64,lookup_enabled,revision FROM twitch_player_steam_links WHERE twitch_user_id=$1")
        .bind(uid).fetch_optional(pool).await? {
        if !row.try_get::<bool, _>("lookup_enabled")? { return Ok(Identity::Disabled); }
        if let Some(id) = row.try_get::<Option<i64>, _>("steam_id64")? {
            return Ok(match valid_steam_id(&id.to_string()) {
                Some(id) => Identity::Steam { id, revision: Some(row.try_get("revision")?) },
                None => Identity::Disabled,
            });
        }
        // A modern link record without a selected primary account is an
        // explicit absence. Falling through could advertise using an older
        // engagement or Discord account after the primary was removed.
        return Ok(Identity::None);
    }
    // The legacy field is selected only through the ID-owned channel record.
    let legacy: Option<String> = sqlx::query_scalar("SELECT NULLIF(BTRIM(es.steam_id),'') FROM twitch_ad_manager_settings s JOIN twitch_engagement_settings es ON es.channel_login=s.twitch_login WHERE s.twitch_user_id=$1")
        .bind(uid).fetch_optional(pool).await?.flatten();
    if let Some(raw) = legacy {
        return Ok(match valid_steam_id(&raw) {
            Some(id) => Identity::Steam { id, revision: None },
            None => Identity::Disabled,
        });
    }
    let discord: Option<String> = sqlx::query_scalar("SELECT NULLIF(BTRIM(discord_user_id),'') FROM twitch_streamer_identities WHERE twitch_user_id=$1")
        .bind(uid).fetch_optional(pool).await?.flatten();
    Ok(discord
        .filter(|id| {
            id.bytes().all(|b| b.is_ascii_digit()) && id.parse::<u64>().is_ok_and(|id| id > 0)
        })
        .map(Identity::Discord)
        .unwrap_or(Identity::None))
}

fn empty(linked: bool) -> SteamMatchSummary {
    SteamMatchSummary {
        steam_linked: linked,
        state: None,
        observed_at: None,
    }
}

fn parse_presence(value: &Value, discord: bool, now: DateTime<Utc>) -> SteamMatchSummary {
    let found = value
        .get(if discord { "linked" } else { "found" })
        .and_then(Value::as_bool);
    if found == Some(false) {
        return empty(!discord);
    }
    let observed_at = value
        .get("last_update")
        .and_then(Value::as_i64)
        .and_then(|secs| DateTime::from_timestamp(secs, 0));
    let mut summary = SteamMatchSummary {
        steam_linked: true,
        state: None,
        observed_at,
    };
    let Some(observed) = observed_at else {
        return summary;
    };
    if found != Some(true)
        || observed > now + Duration::seconds(30)
        || now.signed_duration_since(observed) > Duration::seconds(MATCH_STATUS_FRESH_SECS)
    {
        return summary;
    }
    let (Some(in_match), Some(in_deadlock)) = (
        value
            .get(if discord { "live" } else { "in_match" })
            .and_then(Value::as_bool),
        value.get("in_deadlock").and_then(Value::as_bool),
    ) else {
        return summary;
    };
    if in_match && !in_deadlock {
        return summary;
    }
    let stage = value
        .get("stage")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if !in_match
        && (stage.as_deref() == Some("match")
            || (in_deadlock && !matches!(stage.as_deref(), Some("lobby" | "queue" | "menu"))))
    {
        return summary;
    }
    summary.state = Some(SteamMatchState {
        in_match,
        in_deadlock,
        observed_at: observed,
        stage,
        hero: value.get("hero").and_then(Value::as_str).map(str::to_owned),
    });
    summary
}

pub(super) async fn summary(
    pool: &PgPool,
    client: &Client,
    uid: &str,
    now: DateTime<Utc>,
) -> Result<SteamMatchSummary, sqlx::Error> {
    let selected = identity(pool, uid).await?;
    if matches!(selected, Identity::None | Identity::Disabled) {
        return Ok(empty(false));
    }
    let response = client.fetch(&selected).await;
    // An unlink, primary-account change or revision change during HTTP wins.
    if identity(pool, uid).await? != selected {
        return Ok(empty(false));
    }
    Ok(response
        .as_ref()
        .map(|value| parse_presence(value, matches!(selected, Identity::Discord(_)), now))
        .unwrap_or_else(|| empty(true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn source_time_and_required_fields_determine_safety() {
        let now = DateTime::from_timestamp(1_780_000_000, 0).unwrap();
        let base = json!({"found":true,"in_match":true,"in_deadlock":true,"stage":"match","last_update":now.timestamp()});
        assert!(parse_presence(&base, false, now).state.unwrap().in_match);
        let queue = json!({"found":true,"in_match":false,"in_deadlock":true,"stage":"lobby","last_update":now.timestamp()});
        assert!(!parse_presence(&queue, false, now).state.unwrap().in_match);
        for field in ["found", "in_match", "in_deadlock", "last_update"] {
            let mut bad = base.clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(parse_presence(&bad, false, now).state.is_none(), "{field}");
        }
        for stamp in [now.timestamp() - 181, now.timestamp() + 31] {
            let mut old = queue.clone();
            old["last_update"] = json!(stamp);
            let summary = parse_presence(&old, false, now);
            assert!(summary.state.is_none());
            assert_eq!(summary.observed_at.unwrap().timestamp(), stamp);
        }
        assert!(parse_presence(&json!({"found":false}), false, now)
            .state
            .is_none());
        for (in_deadlock, stage) in [(true, None), (true, Some("match")), (false, Some("match"))] {
            let bad = json!({"found":true,"in_match":false,"in_deadlock":in_deadlock,"stage":stage,"last_update":now.timestamp()});
            assert!(parse_presence(&bad, false, now).state.is_none());
        }
        assert!(!parse_presence(&json!({"linked":false,"live":false}), true, now).steam_linked);
        let legacy =
            json!({"linked":true,"live":true,"in_deadlock":true,"last_update":now.timestamp()});
        assert!(parse_presence(&legacy, true, now).state.unwrap().in_match);
    }

    #[tokio::test]
    async fn http_contract_and_failures_are_closed() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), Some("test-token".into()));
        let selected = Identity::Steam {
            id: 76561198000000021,
            revision: Some(1),
        };
        Mock::given(method("GET"))
            .and(path("/internal/player-live"))
            .and(query_param("steam_id", "76561198000000021"))
            .and(header("x-internal-token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"found":false})))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(client.fetch(&selected).await.unwrap()["found"], false);
        server.verify().await;
        server.reset().await;
        for response in [
            ResponseTemplate::new(503),
            ResponseTemplate::new(401),
            ResponseTemplate::new(200).set_body_string("not json"),
            ResponseTemplate::new(200).set_body_string("x".repeat(MAX_BODY + 1)),
        ] {
            Mock::given(method("GET"))
                .respond_with(response)
                .mount(&server)
                .await;
            assert!(client.fetch(&selected).await.is_none());
            server.reset().await;
        }
        assert!(Client::with_base(&server.uri(), None)
            .fetch(&selected)
            .await
            .is_none());
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

#[cfg(test)]
mod db_tests;

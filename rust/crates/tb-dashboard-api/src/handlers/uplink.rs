//! Proxy vom Streamer-Dashboard zu rs-relay. Das Relay-Secret bleibt serverseitig.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::level::DashboardAuthLevel;

fn relay_base() -> String {
    std::env::var("RS_RELAY_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8891".into())
}

fn relay_secret() -> Option<String> {
    std::env::var("RS_RELAY_API_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn partner_id(auth: &DashboardAuthLevel) -> Result<i64, Response> {
    let raw = match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => twitch_user_id.as_str(),
        DashboardAuthLevel::Admin {
            actor: Some(actor),
        } => actor.twitch_user_id.as_str(),
        DashboardAuthLevel::Admin { actor: None } => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "admin ohne twitch-identitaet" })),
            )
                .into_response());
        }
        DashboardAuthLevel::None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response());
        }
    };
    raw.trim().parse::<i64>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "twitch user id fehlt" })),
        )
            .into_response()
    })
}

async fn relay_json(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, Response> {
    let secret = relay_secret().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Uplink ist noch nicht verbunden." })),
        )
            .into_response()
    })?;
    let url = format!("{}{path}", relay_base().trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req = client
        .request(method, url)
        .header("X-Relay-Auth", secret)
        .header("Accept", "application/json");
    if let Some(body) = body {
        req = req.json(&body);
    }
    let antwort = req.send().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "Uplink antwortet nicht." })),
        )
            .into_response()
    })?;
    let status = antwort.status();
    let wert = antwort.json::<Value>().await.unwrap_or_else(|_| json!({}));
    if !status.is_success() {
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(wert),
        )
            .into_response());
    }
    Ok(wert)
}

pub async fn me_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let wert = relay_json(
        reqwest::Method::GET,
        &format!("/v1/me?streamer_id={id}"),
        None,
    )
    .await?;
    Ok(Json(wert))
}

pub async fn waitlist_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let wert = relay_json(
        reqwest::Method::POST,
        &format!("/v1/me/waitlist?streamer_id={id}"),
        Some(json!({})),
    )
    .await?;
    Ok(Json(wert))
}

#[derive(Deserialize)]
pub struct DestinationBody {
    pub platform: String,
    pub rtmp_url: String,
    pub stream_key: String,
}

pub async fn put_destination_handler(
    auth: DashboardAuthLevel,
    Json(body): Json<DestinationBody>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    if body.stream_key.trim().is_empty() || body.rtmp_url.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "rtmp_url und stream_key braucht es" })),
        )
            .into_response());
    }
    let wert = relay_json(
        reqwest::Method::PUT,
        "/v1/admin/destinations",
        Some(json!({
            "streamer_id": id,
            "platform": body.platform,
            "rtmp_url": body.rtmp_url,
            "stream_key": body.stream_key,
        })),
    )
    .await?;
    Ok(Json(wert))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partner_ohne_id_ist_fehler() {
        let auth = DashboardAuthLevel::None;
        assert!(partner_id(&auth).is_err());
    }

    #[test]
    fn partner_id_wird_gelesen() {
        let auth = DashboardAuthLevel::Partner {
            twitch_login: "earlysalty".into(),
            twitch_user_id: "123".into(),
            display_name: "Early".into(),
        };
        assert_eq!(partner_id(&auth).unwrap(), 123);
    }
}

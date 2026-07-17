//! Versand der fertigen Highlight-Clips an den lokalen highlight-clips-Relay.
//!
//! Port von `bot/highlight_clipper/dm_sender.py` (der Name ist irreführend:
//! es ist KEIN Discord-DM, sondern ein HTTP-POST an den lokalen Dienst auf
//! `127.0.0.1:8899`, der die Clips dann in den Highlight-Discord-**Channel**
//! postet — analog zum B3-Recruiting-Send über einen lokalen Relay, daher kein
//! B10-Ausschluss). Fehler werden geloggt, nie propagiert.

use std::time::Duration;

use crate::config::HIGHLIGHT_DISCORD_CHANNEL_ID;
use crate::event_detector::HighlightEvent;

/// Endpoint des lokalen highlight-clips-Relays (Python `_HIGHLIGHT_API_URL`).
pub const HIGHLIGHT_API_URL: &str = "http://127.0.0.1:8899/highlight-clips";

/// Lokaler Service-Token (kein Secret — Klartext im Quellcode des Relays).
const API_TOKEN: &str = "changeme-local";

/// Request-Timeout wie Python (`_TIMEOUT = 120`).
const TIMEOUT: Duration = Duration::from_secs(120);

/// Postet die Clip-Pfade + Events an den highlight-clips-Relay. Best-effort:
/// jeder Fehler (Netzwerk, `ok != true`) wird geloggt und verschluckt.
pub async fn send_highlight_to_channel(
    api_url: &str,
    streamer_login: &str,
    match_id: i64,
    events: &[HighlightEvent],
    clip_paths: &[String],
) {
    let event_json: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "event_type": e.event_type.as_str(),
                "label": e.label,
                "game_time_s": e.game_time_s,
            })
        })
        .collect();
    let payload = serde_json::json!({
        "token": API_TOKEN,
        "channel_id": HIGHLIGHT_DISCORD_CHANNEL_ID,
        "streamer_login": streamer_login,
        "match_id": match_id,
        "events": event_json,
        "clip_paths": clip_paths,
    });

    let client = match reqwest::Client::builder().timeout(TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "HighlightClipper: HTTP-Client-Fehler");
            return;
        }
    };

    let result = client.post(api_url).json(&payload).send().await;
    match result {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(body) => {
                if body.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                    tracing::error!(?body, "HighlightClipper: highlight-clips API Fehler");
                    return;
                }
                tracing::info!(
                    clips = clip_paths.len(),
                    streamer = streamer_login,
                    match_id,
                    "HighlightClipper: Clips gepostet"
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "HighlightClipper: Fehler beim Senden an highlight-clips API");
            }
        },
        Err(e) => {
            tracing::error!(error = %e, "HighlightClipper: Fehler beim Senden an highlight-clips API");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_detector::EventType;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_events() -> Vec<HighlightEvent> {
        vec![HighlightEvent {
            event_type: EventType::Multikill,
            game_time_s: 100,
            duration_s: 5,
            kill_count: 3,
            label: "Triple Kill".to_string(),
            pre_roll_s: 8,
        }]
    }

    #[tokio::test]
    async fn postet_payload_mit_token_und_events() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/highlight-clips"))
            .and(body_string_contains("changeme-local"))
            .and(body_string_contains("\"streamer_login\":\"nani\""))
            .and(body_string_contains("\"event_type\":\"multikill\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        send_highlight_to_channel(
            &format!("{}/highlight-clips", server.uri()),
            "nani",
            42,
            &sample_events(),
            &["/clips/a.mp4".to_string()],
        )
        .await;
        // MockServer verifiziert .expect(1) beim Drop.
    }

    #[tokio::test]
    async fn api_fehler_wird_verschluckt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/highlight-clips"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": false, "error": "x"})),
            )
            .mount(&server)
            .await;
        // Kein Panic trotz ok=false.
        send_highlight_to_channel(
            &format!("{}/highlight-clips", server.uri()),
            "nani",
            1,
            &[],
            &[],
        )
        .await;
    }
}

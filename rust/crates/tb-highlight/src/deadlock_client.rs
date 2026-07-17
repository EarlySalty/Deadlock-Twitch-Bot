//! HTTP-Client für die öffentliche deadlock-api.com — Match-History + Metadata.
//!
//! Port von `bot/highlight_clipper/deadlock_client.py`. Wie das Python-Original
//! ein dünner JSON-Holer: HTTP-Fehler propagieren (Python `raise_for_status` →
//! [`reqwest::Response::error_for_status`]), Form-Abweichungen degradieren auf
//! ein leeres Ergebnis. Die Basis-URL ist injizierbar (wie bei `title_ai`),
//! damit Tests gegen einen Mock-Server laufen; produktiv gilt
//! [`DEADLOCK_API_BASE`].

use std::time::Duration;

/// Basis-URL der öffentlichen Deadlock-API (Python: hartcodiert in den f-Strings).
pub const DEADLOCK_API_BASE: &str = "https://api.deadlock-api.com/v1";

/// Request-Timeout wie Python (`_REQUEST_TIMEOUT = 30`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Holt eine URL und parst die Antwort als JSON. HTTP-Status ≥400 wird zum
/// Fehler (Parität zu `response.raise_for_status()`).
async fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())
}

/// Match-History eines Spielers als Liste von Match-Objekten.
///
/// Python akzeptiert zwei Antwortformen: eine reine Liste oder ein Objekt mit
/// `matches`/`data` (`payload.get("matches") or payload.get("data") or []`).
/// Nicht-Objekte in der Liste werden verworfen.
pub async fn get_match_history(
    base_url: &str,
    account_id: i64,
    limit: u32,
) -> Result<Vec<serde_json::Value>, String> {
    let url = format!("{base_url}/players/{account_id}/match-history?limit={limit}");
    let payload = get_json(&url).await?;
    Ok(extract_match_list(payload))
}

/// Metadaten eines Matches — `match_info` falls vorhanden, sonst das Objekt
/// selbst; bei Nicht-Objekt ein leeres Objekt.
pub async fn get_match_metadata(
    base_url: &str,
    match_id: i64,
) -> Result<serde_json::Value, String> {
    let url = format!("{base_url}/matches/{match_id}/metadata");
    let payload = get_json(&url).await?;
    Ok(extract_match_info(payload))
}

/// Python-Wahrheitswert eines JSON-Werts (für `a or b`-Ketten): leere
/// Container/Strings, `null`, `false` und `0` gelten als falsy.
fn json_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        serde_json::Value::Object(o) => !o.is_empty(),
    }
}

fn extract_match_list(payload: serde_json::Value) -> Vec<serde_json::Value> {
    match payload {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter(serde_json::Value::is_object)
            .collect(),
        serde_json::Value::Object(map) => {
            // Python: matches = payload.get("matches") or payload.get("data") or []
            let chosen = [map.get("matches"), map.get("data")]
                .into_iter()
                .flatten()
                .find(|v| json_truthy(v));
            match chosen {
                Some(serde_json::Value::Array(items)) => {
                    items.iter().filter(|v| v.is_object()).cloned().collect()
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

fn extract_match_info(payload: serde_json::Value) -> serde_json::Value {
    match &payload {
        serde_json::Value::Object(map) => match map.get("match_info") {
            Some(info @ serde_json::Value::Object(_)) => info.clone(),
            _ => payload,
        },
        _ => serde_json::Value::Object(serde_json::Map::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn match_history_liste_filtert_nicht_objekte() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/players/42/match-history"))
            .and(query_param("limit", "20"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"match_id": 1}, "not-a-dict", {"match_id": 2}
            ])))
            .mount(&server)
            .await;
        let out = get_match_history(&server.uri(), 42, 20).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["match_id"], 1);
    }

    #[tokio::test]
    async fn match_history_leeres_matches_faellt_auf_data() {
        // Python `matches or data`: matches=[] ist falsy → data gewinnt.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/players/7/match-history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "matches": [],
                "data": [{"match_id": 9}]
            })))
            .mount(&server)
            .await;
        let out = get_match_history(&server.uri(), 7, 20).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["match_id"], 9);
    }

    #[tokio::test]
    async fn match_metadata_extrahiert_match_info() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/matches/123/metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "match_info": {"winning_team": 1}
            })))
            .mount(&server)
            .await;
        let out = get_match_metadata(&server.uri(), 123).await.unwrap();
        assert_eq!(out["winning_team"], 1);
    }

    #[tokio::test]
    async fn match_metadata_ohne_match_info_gibt_payload() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/matches/55/metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "duration_s": 1800
            })))
            .mount(&server)
            .await;
        let out = get_match_metadata(&server.uri(), 55).await.unwrap();
        assert_eq!(out["duration_s"], 1800);
    }

    #[tokio::test]
    async fn http_fehler_propagiert() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/matches/500/metadata"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        assert!(get_match_metadata(&server.uri(), 500).await.is_err());
    }
}

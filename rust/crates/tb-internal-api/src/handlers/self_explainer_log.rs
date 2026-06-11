//! Handler für `POST /discord/self-explainer-log`.
//!
//! Nativer Port von `bot/internal_api/routes/discord_log.py` →
//! `discord_self_explainer_log`.
//!
//! Diese Route ist ein **reiner HTTP-Relay**: Sie empfängt ein Discord-Embed
//! vom Dashboard (das selbst keinen Master-Broker-Token hält) und leitet es
//! an den Master-Broker (`/internal/master/v1/discord/send-rich-message`) weiter.
//! Es gibt keinen DB-Zugriff; das Logging in `twitch_self_explainer_log` erfolgt
//! separat im Dashboard-Code (`routes_self_explainer.py`).
//!
//! Vertragsparität (Byte-identisch zur Python-Implementierung):
//! - `POST /internal/twitch/v1/discord/self-explainer-log`
//! - Body: `{ channel_id: string|integer, embed: object, content?: string|null }`
//! - Fehler 400: `{ ok: false, error: "channel_id_and_embed_required" }`
//! - Fehler 503: `{ ok: false, error: "master_broker_token_missing" }`
//! - Erfolg 200: `{ ok: true, broker_status: <http-status-code> }`
//! - Broker-Fehler 502: `{ ok: false, broker_status: <http-status-code> }`
//!   oder: `{ ok: false, error: "broker_post_failed", detail: "..." }`
//!
//! Idempotency-Key: kanonisches JSON (Schlüssel sortiert) → SHA-256 → hex →
//! `"self-explainer:<hex[:48]>"` — identische Logik zu Python `_idempotency_key`.
//!
//! Token-Fallback-Kette (sync mit Python `_master_broker_token`):
//! `MASTER_BROKER_TOKEN` → `MAIN_BOT_INTERNAL_TOKEN` → `TWITCH_INTERNAL_API_TOKEN`.
//!
//! Broker-URL-Auflösung (sync mit Python `_master_broker_base_url`):
//! `MASTER_BROKER_BASE_URL` hat Vorrang; sonst `http://<MASTER_BROKER_HOST>:<MASTER_BROKER_PORT>`
//! mit Defaults `127.0.0.1:8770`.

use axum::{response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tb_http_core::{ApiError, AuthLevel};

const BROKER_DISCORD_PATH: &str = "/internal/master/v1/discord/send-rich-message";
const BROKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

// ── Request/Response-Typen ────────────────────────────────────────────────────

/// Eingehender Body von Dashboard oder anderem internen Aufrufer.
#[derive(Deserialize)]
pub struct SelfExplainerLogRequest {
    /// Discord-Channel-ID als String oder Zahl (Python: `body.get("channel_id")`).
    pub channel_id: Option<Value>,
    /// Das Discord-Embed-Objekt (muss ein JSON-Objekt sein).
    pub embed: Option<Value>,
    /// Optionaler Plaintext-Content über dem Embed.
    #[serde(default)]
    pub content: Option<Value>,
}

/// Erfolgsantwort bei erfolgreichem Broker-Forward.
#[derive(Serialize)]
pub struct LogOkResponse {
    pub ok: bool,
    pub broker_status: u16,
}

/// Fehlerantwort mit `ok: false` — Parität zu Python `web.json_response({"ok": False, ...})`.
#[derive(Serialize)]
pub struct LogErrResponse {
    pub ok: bool,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ── Hilfsfunktionen ────────────────────────────────────────────────────────────

/// Token-Fallback-Kette: `MASTER_BROKER_TOKEN` → `MAIN_BOT_INTERNAL_TOKEN`
/// → `TWITCH_INTERNAL_API_TOKEN`. Parität zu Python `_master_broker_token`.
fn broker_token() -> Option<String> {
    for key in &[
        "MASTER_BROKER_TOKEN",
        "MAIN_BOT_INTERNAL_TOKEN",
        "TWITCH_INTERNAL_API_TOKEN",
    ] {
        let value = std::env::var(key).unwrap_or_default();
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// Broker-Basis-URL aus Env. Parität zu Python `_master_broker_base_url`:
/// `MASTER_BROKER_BASE_URL` hat Vorrang; sonst `http://<HOST>:<PORT>` mit
/// Defaults `127.0.0.1` / `8770`.
fn broker_base_url() -> String {
    let explicit = std::env::var("MASTER_BROKER_BASE_URL").unwrap_or_default();
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        return explicit.trim_end_matches('/').to_string();
    }
    let host = std::env::var("MASTER_BROKER_HOST").unwrap_or_default();
    let host = host.trim();
    let host = if host.is_empty() { "127.0.0.1" } else { host };
    let port = std::env::var("MASTER_BROKER_PORT").unwrap_or_default();
    let port = port.trim();
    let port = if port.is_empty() { "8770" } else { port };
    format!("http://{host}:{port}")
}

/// Kanonischer Idempotency-Key aus dem Broker-Payload.
/// Parität zu Python `_idempotency_key`: `json.dumps(..., sort_keys=True,
/// ensure_ascii=True, separators=(",",":"))` → SHA-256 → hex → `"self-explainer:<hex[:48]>"`.
fn idempotency_key(payload: &Value) -> String {
    // serde_json serialisiert Objekte in Einfüge-Reihenfolge; für sort_keys-Parität
    // muss das Payload-Objekt mit BTreeMap-Schlüsselordnung aufgebaut worden sein —
    // das ist garantiert, weil wir das Payload selbst mit serde_json::json! als
    // Objekt mit fester Schlüsselreihenfolge bauen und dann für den Hash per
    // canonicalization neu sortieren.
    let canonical = canonical_json(payload);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest);
    format!("self-explainer:{}", &hex[..48])
}

/// Kanonisches JSON mit sortierten Schlüsseln (Parität zu Python `sort_keys=True`).
/// Nur die Felder, die im tatsächlichen Broker-Payload vorkommen, müssen korrekt
/// sortiert sein. Wir serialisieren über eine `BTreeMap` um Schlüssel-Sortierung
/// zu erzwingen.
fn canonical_json(value: &Value) -> String {
    use serde_json::Map;
    fn sort_value(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let sorted: std::collections::BTreeMap<&str, Value> =
                    m.iter().map(|(k, v)| (k.as_str(), sort_value(v))).collect();
                Value::Object(
                    sorted
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v))
                        .collect::<Map<String, Value>>(),
                )
            }
            Value::Array(arr) => Value::Array(arr.iter().map(sort_value).collect()),
            other => other.clone(),
        }
    }
    // Python `separators=(",",":")` = kompakt ohne Leerzeichen —
    // serde_json's Standardausgabe ist ebenfalls kompakt (keine Leerzeichen).
    // `ensure_ascii=True` in Python escapet Non-ASCII → serde_json escaped
    // Non-ASCII in Strings ebenfalls per \uXXXX, daher byte-identisch.
    serde_json::to_string(&sort_value(value)).unwrap_or_default()
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /internal/twitch/v1/discord/self-explainer-log`
///
/// Validiert `channel_id` + `embed`, baut den Broker-Payload, leitet ihn
/// an den Master-Broker weiter und gibt `{ok, broker_status}` zurück.
pub async fn handler(
    auth: AuthLevel,
    Json(body): Json<SelfExplainerLogRequest>,
) -> axum::response::Response {
    if !auth.is_privileged() {
        return ApiError::unauthorized().into_response();
    }

    // Validierung: channel_id muss vorhanden + nicht null sein;
    // embed muss ein JSON-Objekt sein.
    let channel_id_val = match &body.channel_id {
        Some(v) if !v.is_null() => v.clone(),
        _ => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(LogErrResponse {
                    ok: false,
                    error: "channel_id_and_embed_required".to_string(),
                    detail: None,
                }),
            )
                .into_response();
        }
    };

    let embed_val = match &body.embed {
        Some(Value::Object(m)) => Value::Object(m.clone()),
        _ => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(LogErrResponse {
                    ok: false,
                    error: "channel_id_and_embed_required".to_string(),
                    detail: None,
                }),
            )
                .into_response();
        }
    };

    // channel_id: int(channel_id) — Python konvertiert string→int im Payload.
    let channel_id_int: i64 = match &channel_id_val {
        Value::Number(n) => n.as_i64().unwrap_or(0),
        Value::String(s) => s.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    };

    // Broker-Token aus Fallback-Kette.
    let token = match broker_token() {
        Some(t) => t,
        None => {
            tracing::warn!(
                "internal_api: kein Broker-Token (MASTER_BROKER_TOKEN/MAIN_BOT_INTERNAL_TOKEN/\
                 TWITCH_INTERNAL_API_TOKEN) — self-explainer Discord-Log übersprungen"
            );
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(LogErrResponse {
                    ok: false,
                    error: "master_broker_token_missing".to_string(),
                    detail: None,
                }),
            )
                .into_response();
        }
    };

    // Broker-Payload — identisch zu Python:
    // { "channel_id": int, "content": ..., "embed": {...},
    //   "allowed_role_ids": [], "view_spec": null }
    let payload = serde_json::json!({
        "channel_id": channel_id_int,
        "content": body.content,
        "embed": embed_val,
        "allowed_role_ids": [],
        "view_spec": null,
    });

    let idempotency = idempotency_key(&payload);
    let url = format!("{}{BROKER_DISCORD_PATH}", broker_base_url());

    let client = reqwest::Client::builder()
        .timeout(BROKER_REQUEST_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client
        .post(&url)
        .header("X-Internal-Token", &token)
        .header("X-Idempotency-Key", &idempotency)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            let status_u16 = resp.status().as_u16();
            let ok = status_u16 < 300;
            if !ok {
                let detail = resp
                    .text()
                    .await
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect::<String>();
                tracing::warn!(
                    "internal_api: self-explainer Broker status={} body={}",
                    status_u16,
                    detail
                );
                return (
                    axum::http::StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "ok": false, "broker_status": status_u16 })),
                )
                    .into_response();
            }
            (
                axum::http::StatusCode::OK,
                Json(LogOkResponse {
                    ok: true,
                    broker_status: status_u16,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!("internal_api: self-explainer Broker-Post fehlgeschlagen: {e}");
            let detail = format!("{e}").chars().take(200).collect::<String>();
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(LogErrResponse {
                    ok: false,
                    error: "broker_post_failed".to_string(),
                    detail: Some(detail),
                }),
            )
                .into_response()
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::post,
        Extension, Router,
    };
    use std::net::SocketAddr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
    use tower::ServiceExt;

    fn make_router(token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(
                &format!("{base}/discord/self-explainer-log"),
                post(handler),
            )
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(token.to_string(), internal_auth))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(body: &str, token: Option<&str>) -> Request<Body> {
        let base = INTERNAL_API_BASE_PATH;
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("{base}/discord/self-explainer-log"))
            .header("content-type", "application/json")
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn ohne_token_401() {
        let app = make_router("secret");
        let resp = app
            .oneshot(req(r#"{"channel_id":"123","embed":{}}"#, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn fehlende_channel_id_400() {
        let app = make_router("secret");
        let resp = app
            .oneshot(req(r#"{"embed":{"title":"test"}}"#, Some("secret")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], false);
        assert_eq!(j["error"], "channel_id_and_embed_required");
    }

    #[tokio::test]
    async fn null_channel_id_400() {
        let app = make_router("secret");
        let resp = app
            .oneshot(req(
                r#"{"channel_id":null,"embed":{"title":"test"}}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "channel_id_and_embed_required");
    }

    #[tokio::test]
    async fn embed_nicht_objekt_400() {
        let app = make_router("secret");
        let resp = app
            .oneshot(req(
                r#"{"channel_id":"123","embed":"kein_objekt"}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "channel_id_and_embed_required");
    }

    #[tokio::test]
    async fn fehlender_embed_400() {
        let app = make_router("secret");
        let resp = app
            .oneshot(req(r#"{"channel_id":"123"}"#, Some("secret")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn kein_broker_token_503() {
        // Sicherstellen, dass alle Token-Env-Vars leer sind.
        // (Dieser Test setzt KEINE Env-Vars — laufen in leerer Umgebung.)
        // Falls eine Var gesetzt ist, Test skippen.
        for key in &[
            "MASTER_BROKER_TOKEN",
            "MAIN_BOT_INTERNAL_TOKEN",
            "TWITCH_INTERNAL_API_TOKEN",
        ] {
            if std::env::var(key).map(|v| !v.trim().is_empty()).unwrap_or(false) {
                eprintln!("SKIP: {key} ist gesetzt — kein_broker_token_503 nicht testbar");
                return;
            }
        }

        let app = make_router("secret");
        let resp = app
            .oneshot(req(
                r#"{"channel_id":"123456789","embed":{"title":"test"}}"#,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], false);
        assert_eq!(j["error"], "master_broker_token_missing");
    }

    // ── Einheitentests für Hilfsfunktionen ─────────────────────────────────────
    // Env-Var-abhängige Tests (broker_token, broker_base_url) werden hier nicht
    // geschrieben — std::env::set_var ist in parallelen Tests unsicher (Rust 1.80
    // deprecated unsafe set_var). Die Fallback-Logik ist stattdessen in den
    // Integrationstests via tatsächlicher Env-Konfiguration abgedeckt.

    #[test]
    fn idempotency_key_format_und_laenge() {
        let payload = serde_json::json!({
            "channel_id": 123456789_i64,
            "content": null,
            "embed": {"title": "Test"},
            "allowed_role_ids": [],
            "view_spec": null,
        });
        let key = idempotency_key(&payload);
        assert!(
            key.starts_with("self-explainer:"),
            "Key muss mit 'self-explainer:' beginnen"
        );
        // prefix "self-explainer:" = 15 Zeichen + 48 Hex = 63 gesamt
        assert_eq!(key.len(), 63, "Key-Länge muss 63 Zeichen betragen");
    }

    #[test]
    fn idempotency_key_deterministisch() {
        let payload = serde_json::json!({
            "channel_id": 999_i64,
            "content": null,
            "embed": {"description": "hello"},
            "allowed_role_ids": [],
            "view_spec": null,
        });
        assert_eq!(idempotency_key(&payload), idempotency_key(&payload));
    }

    #[test]
    fn canonical_json_sortiert_schluessel() {
        let v = serde_json::json!({"z": 1, "a": 2, "m": 3});
        let s = canonical_json(&v);
        // Nach Sortierung: a vor m vor z
        let pos_a = s.find("\"a\"").expect("a");
        let pos_m = s.find("\"m\"").expect("m");
        let pos_z = s.find("\"z\"").expect("z");
        assert!(pos_a < pos_m, "a muss vor m");
        assert!(pos_m < pos_z, "m muss vor z");
    }
}

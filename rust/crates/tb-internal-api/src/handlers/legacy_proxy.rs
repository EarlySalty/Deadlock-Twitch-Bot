//! Fallback-Proxy für noch nicht nach Rust portierte interne-API-Routen.
//!
//! Strangler-Fig-Baustein (Cutover Schritt 3/4/6): der Rust-Router bedient die
//! migrierten Endpoints selbst; alle *unbekannten* Pfade unter
//! `/internal/twitch/v1` werden 1:1 an die Legacy-Python-API durchgereicht
//! (Methode, Pfad+Query, Header inkl. `X-Internal-Token`/`X-Idempotency-Key`,
//! Body). Jede künftig nativ implementierte Route schattet den Proxy
//! automatisch aus, weil axum den Fallback nur für ungematchte Pfade ruft.
//!
//! Ohne konfigurierten Upstream (`TB_INTERNAL_API_LEGACY_FALLBACK_URL` leer)
//! antwortet der Fallback wie bisher mit 404.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderName, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};

/// JSON-Bodies der internen API sind klein; 2 MiB ist großzügig bemessen.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Großzügiger als der Client-Timeout des Dashboard-Service, damit der Proxy
/// nie die engste Schranke ist (OAuth-Callback macht Twitch-Roundtrips).
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(30);

/// Hop-by-hop-Header, die nicht weitergereicht werden dürfen (RFC 9110 §7.6.1);
/// `host`/`content-length` setzt reqwest selbst.
const HOP_HEADERS: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "host",
    "content-length",
];

pub struct LegacyProxy {
    base_url: String,
    client: reqwest::Client,
}

impl LegacyProxy {
    /// `base_url` z. B. `http://127.0.0.1:8779` (ohne Slash am Ende).
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .timeout(UPSTREAM_TIMEOUT)
            .build()
            .expect("reqwest client builder darf nicht fehlschlagen");
        Self { base_url, client }
    }
}

/// Router-Extension: `None` → Fallback deaktiviert (404 wie vor dem Proxy).
#[derive(Clone)]
pub struct LegacyProxyExt(pub Option<Arc<LegacyProxy>>);

fn is_hop_header(name: &HeaderName) -> bool {
    HOP_HEADERS.contains(&name.as_str())
}

pub async fn legacy_fallback_handler(
    Extension(proxy): Extension<LegacyProxyExt>,
    req: Request,
) -> Response {
    let Some(proxy) = proxy.0 else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found" })),
        )
            .into_response();
    };

    let method = req.method().clone();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let headers = req.headers().clone();

    let body = match axum::body::to_bytes(req.into_body(), MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({ "error": "payload_too_large" })),
            )
                .into_response();
        }
    };

    let url = format!("{}{}", proxy.base_url, path_and_query);
    let mut upstream = proxy.client.request(method.clone(), &url);
    for (name, value) in headers.iter() {
        if !is_hop_header(name) {
            upstream = upstream.header(name, value);
        }
    }
    if !body.is_empty() {
        upstream = upstream.body(body.to_vec());
    }

    match upstream.send().await {
        Ok(resp) => {
            let status = resp.status();
            let mut builder = Response::builder().status(status);
            for (name, value) in resp.headers().iter() {
                if !is_hop_header(name) {
                    builder = builder.header(name, value);
                }
            }
            let bytes = match resp.bytes().await {
                Ok(bytes) => bytes,
                Err(err) => {
                    tracing::warn!(
                        "Legacy-Fallback: Upstream-Body nicht lesbar ({method} {path_and_query}): {err}"
                    );
                    return legacy_unavailable();
                }
            };
            builder
                .body(Body::from(bytes))
                .unwrap_or_else(|_| legacy_unavailable())
        }
        Err(err) => {
            tracing::warn!(
                "Legacy-Fallback: Upstream nicht erreichbar ({method} {path_and_query}): {err}"
            );
            legacy_unavailable()
        }
    }
}

fn legacy_unavailable() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": "legacy_upstream_unavailable",
            "message": "legacy internal api not reachable",
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{any, post};
    use axum::Router;
    use tower::ServiceExt;

    fn fallback_router(proxy: Option<Arc<LegacyProxy>>) -> Router {
        Router::new()
            .fallback(legacy_fallback_handler)
            .layer(Extension(LegacyProxyExt(proxy)))
    }

    #[tokio::test]
    async fn ohne_proxy_konfiguration_404() {
        let app = fallback_router(None);
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/internal/twitch/v1/raid/auth-url?login=test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn proxyt_methode_pfad_query_header_und_body() {
        // Mini-Upstream, der die relevanten Request-Teile zurückspiegelt.
        let upstream = Router::new().route(
            "/internal/twitch/v1/raid/oauth-callback",
            post(|req: Request| async move {
                let token = req
                    .headers()
                    .get("x-internal-token")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let query = req.uri().query().unwrap_or("").to_string();
                let body = axum::body::to_bytes(req.into_body(), 1024).await.unwrap();
                Json(serde_json::json!({
                    "token": token,
                    "query": query,
                    "body": String::from_utf8_lossy(&body),
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });

        let app = fallback_router(Some(Arc::new(LegacyProxy::new(format!("http://{addr}")))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/internal/twitch/v1/raid/oauth-callback?source=test")
                    .header("x-internal-token", "tok-123")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"code":"abc"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["token"], "tok-123");
        assert_eq!(parsed["query"], "source=test");
        assert_eq!(parsed["body"], r#"{"code":"abc"}"#);
    }

    #[tokio::test]
    async fn upstream_down_ergibt_502() {
        // Port aus dem TEST-NET-Bereich, auf dem garantiert nichts lauscht:
        // wir binden kurz einen Listener und schließen ihn wieder.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let app = fallback_router(Some(Arc::new(LegacyProxy::new(format!("http://{addr}")))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/internal/twitch/v1/raid/auth-url")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn any_route_methoden_erreichen_upstream() {
        let upstream =
            Router::new().route("/internal/twitch/v1/raid/go-url", any(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });

        let app = fallback_router(Some(Arc::new(LegacyProxy::new(format!("http://{addr}")))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/internal/twitch/v1/raid/go-url?state=xyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

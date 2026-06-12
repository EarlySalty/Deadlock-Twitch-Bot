//! Strangler-Fig-Fallback-Proxy für noch nicht nach Rust portierte Dashboard-Routen.
//!
//! Vorbild: `tb-internal-api/src/handlers/legacy_proxy.rs` (gleiche Struktur,
//! angepasst auf Dashboard-spezifische Besonderheiten).
//!
//! Alle Routen, die axum nicht nativ matched, werden 1:1 an den Python-Legacy-
//! Service (Standard: `http://127.0.0.1:8765`) weitergereicht — Methode, Pfad,
//! Query, Header inkl. Cookies, Body, Status-Code und Response-Headers werden
//! transparent durchgereicht. Nativ registrierte Routen schatten den Proxy
//! automatisch aus.
//!
//! # Localhost-Bypass und Host-Header (KRITISCH)
//!
//! Python prüft in `bot/analytics/api_v2.py::_is_localhost()` (Zeilen 534–553):
//!   1. `Host`-Header muss ein Loopback-Host sein (`127.0.0.1`, `::1`, `localhost`)
//!   2. TCP-Peer-IP (Socket-Ebene) muss ebenfalls Loopback sein
//!
//! Der Proxy-Hop macht die Peer-IP aus Pythons Sicht zwingend zu `127.0.0.1`
//! (reqwest baut eine Loopback-TCP-Verbindung auf) — Bedingung 2 ist also für
//! JEDEN proxied Request erfüllt. Damit entscheidet allein der `Host`-Header.
//!
//! **Deshalb MUSS der Original-Host-Header 1:1 durchgereicht werden:**
//! - Externe Requests kommen über Caddy, dessen Site-Block auf die öffentliche
//!   Domain matcht — der Host-Header ist dort zwangsläufig die externe Domain
//!   (nicht Loopback) → Python verweigert den Bypass. Ein Angreifer kann keinen
//!   Loopback-Host einschleusen, weil ein Request mit `Host: 127.0.0.1` bei
//!   Caddy keinen öffentlichen Site-Block matcht und nie hier ankommt.
//! - Lokale Aufrufe (`curl 127.0.0.1:8769/...`) tragen ihren Loopback-Host und
//!   behalten den Bypass — identisch zum heutigen Direktzugriff auf 8765.
//!
//! Die naive Alternative — Host strippen und reqwest setzen lassen — wäre eine
//! **Bypass-Öffnung**: reqwest setzt dann `Host: 127.0.0.1:8765` (Loopback!),
//! womit beide Bedingungen für jeden externen Request erfüllt wären.
//!
//! Alle übrigen Header (inkl. `x-forwarded-host`, `x-real-ip`) werden ebenfalls
//! transparent durchgereicht — exakt wie heute auf der Strecke Caddy→Python.
//! Python wertet sie in `_is_localhost()` nicht aus. `x-forwarded-for` wird um
//! die Peer-IP ergänzt (Standard-Proxy-Semantik, reine Audit-Info).
//!
//! # Konfiguration
//!
//! `TB_DASHBOARD_LEGACY_FALLBACK_URL` — wenn leer oder nicht gesetzt, antwortet
//! der Fallback mit 404 (Proxy deaktiviert).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};

/// Analytics-Responses können groß sein (Viewer-Profiles, Chat-Graph etc.) —
/// 16 MiB ist großzügig aber verhindert OOM bei normalen API-Antworten.
/// Streaming wäre schöner; reqwest `.bytes()` buffert leider auf Client-Seite.
/// Für eine echte Streaming-Lösung bräuchten wir `reqwest::Response::bytes_stream()`
/// und `axum::body::Body::from_stream()` — TODO wenn Analytics-Payloads > 10 MiB
/// in der Praxis auftreten.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Analytics-Endpunkte können langsam sein (DB-Aggregationen, MiniMax-AI-Chat).
/// 120 Sekunden ist bewusst sehr großzügig — der Rust-Proxy soll nie die engste
/// Timeout-Schranke sein. Caddy hat einen eigenen, konfigurierbaren Timeout.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(120);

/// Hop-by-hop-Header nach RFC 9110 §7.6.1 plus `content-length` (wird von
/// reqwest aus dem Body neu berechnet). `host` wird bewusst NICHT gestrippt —
/// siehe Modul-Doku: der Original-Host trägt die Localhost-Bypass-Entscheidung
/// in Python und muss den Upstream unverändert erreichen.
const HOP_HEADERS: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "content-length",
];

pub struct DashboardLegacyProxy {
    base_url: String,
    client: reqwest::Client,
}

impl DashboardLegacyProxy {
    /// `base_url` z. B. `http://127.0.0.1:8765` (kein Slash am Ende).
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .timeout(UPSTREAM_TIMEOUT)
            .build()
            .expect("reqwest-Client-Builder darf nicht fehlschlagen");
        Self { base_url, client }
    }
}

/// Router-Extension: `None` → Proxy deaktiviert, Fallback antwortet mit 404.
#[derive(Clone)]
pub struct DashboardProxyExt(pub Option<Arc<DashboardLegacyProxy>>);

fn is_hop_header(name: &HeaderName) -> bool {
    HOP_HEADERS.contains(&name.as_str())
}

/// Catch-all-Fallback-Handler: reicht Requests 1:1 an den Python-Legacy-Service weiter.
///
/// Registrierung in main.rs:
/// ```rust,ignore
/// router.fallback(dashboard_fallback_handler)
///       .layer(Extension(DashboardProxyExt(proxy)))
///       .layer(axum::extract::ConnectInfo::<SocketAddr>::...)
/// ```
pub async fn dashboard_fallback_handler(
    Extension(proxy): Extension<DashboardProxyExt>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
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
        if is_hop_header(name) {
            continue;
        }
        // Inklusive `host`: der Original-Host trägt in Python die
        // Localhost-Bypass-Entscheidung (siehe Modul-Doku).
        upstream = upstream.header(name, value);
    }

    // X-Forwarded-For ergänzen: Client-IP hinten anhängen (Standard-Proxy-Semantik).
    // Python wertet diesen Header in _is_localhost() nicht aus — er ist reine
    // Audit-Info. Wenn kein ConnectInfo vorhanden (z. B. in Unit-Tests), wird
    // der Header nicht gesetzt.
    if let Some(ConnectInfo(peer_addr)) = connect_info {
        let peer_ip = peer_addr.ip().to_string();
        let new_xff = match headers.get("x-forwarded-for") {
            Some(existing) => {
                let existing_str = existing.to_str().unwrap_or("");
                format!("{existing_str}, {peer_ip}")
            }
            None => peer_ip,
        };
        if let Ok(xff_val) = HeaderValue::from_str(&new_xff) {
            upstream = upstream.header("x-forwarded-for", xff_val);
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
                if is_hop_header(name) {
                    continue;
                }
                builder = builder.header(name, value);
            }
            let bytes = match resp.bytes().await {
                Ok(bytes) => bytes,
                Err(err) => {
                    tracing::warn!(
                        "Dashboard-Proxy: Upstream-Body nicht lesbar ({method} {path_and_query}): {err}"
                    );
                    return proxy_unavailable();
                }
            };
            builder
                .body(Body::from(bytes))
                .unwrap_or_else(|_| proxy_unavailable())
        }
        Err(err) => {
            tracing::warn!(
                "Dashboard-Proxy: Upstream nicht erreichbar ({method} {path_and_query}): {err}"
            );
            proxy_unavailable()
        }
    }
}

fn proxy_unavailable() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": "legacy_upstream_unavailable",
            "message": "legacy dashboard api not reachable",
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::StatusCode,
        routing::{any, get},
        Router,
    };
    use tower::ServiceExt;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Hilfsfunktion: baut einen Router mit dem Fallback-Handler und dem
    /// gegebenen optionalen Proxy.
    fn make_fallback_router(proxy: Option<Arc<DashboardLegacyProxy>>) -> Router {
        Router::new()
            .fallback(dashboard_fallback_handler)
            .layer(Extension(DashboardProxyExt(proxy)))
    }

    // ─── 1. Kein Proxy konfiguriert → 404 ─────────────────────────────────

    #[tokio::test]
    async fn ohne_proxy_konfiguration_404() {
        let app = make_fallback_router(None);
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/twitch/api/v2/overview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ─── 2. GET-Roundtrip: Pfad, Query und Status werden korrekt weitergereicht ──

    #[tokio::test]
    async fn get_roundtrip_pfad_und_query() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/twitch/api/v2/overview"))
            .and(query_param("streamer", "nanikeks"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "ok": true })),
            )
            .mount(&mock_server)
            .await;

        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(
            mock_server.uri(),
        ))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/twitch/api/v2/overview?streamer=nanikeks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["ok"], true);
    }

    // ─── 3. POST-Roundtrip: Body, Cookie, Content-Type werden weitergereicht ──

    #[tokio::test]
    async fn post_roundtrip_body_und_cookie() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/twitch/api/v2/stream-report/rate"))
            .and(header("cookie", "twitch_dash_session=abc123"))
            .and(header("content-type", "application/json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "rated": true })),
            )
            .mount(&mock_server)
            .await;

        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(
            mock_server.uri(),
        ))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/twitch/api/v2/stream-report/rate")
                    .header("cookie", "twitch_dash_session=abc123")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_id":42,"rating":5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["rated"], true);
    }

    // ─── 4. Fehler-Status-Codes werden 1:1 durchgereicht ─────────────────

    #[tokio::test]
    async fn status_code_401_wird_durchgereicht() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/twitch/api/v2/lurker-analysis"))
            .respond_with(ResponseTemplate::new(401).set_body_json(
                serde_json::json!({ "error": "unauthorized" }),
            ))
            .mount(&mock_server)
            .await;

        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(
            mock_server.uri(),
        ))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/twitch/api/v2/lurker-analysis")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ─── 5. Upstream down → 502 ───────────────────────────────────────────

    #[tokio::test]
    async fn upstream_down_ergibt_502() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // Port sofort wieder freigeben — nichts lauscht dort.

        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(format!(
            "http://{addr}"
        )))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/twitch/api/v2/overview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    // ─── 6. Nativ registrierte Route wird NICHT proxied ──────────────────
    //
    // Simuliert das Strangler-Fig-Muster: eine nativ registrierte Route
    // schirmt den Fallback ab.

    #[tokio::test]
    async fn nativ_registrierte_route_schirmt_proxy_ab() {
        let mock_server = MockServer::start().await;
        // Mock registrieren, der NICHT aufgerufen werden darf.
        Mock::given(method("GET"))
            .and(path("/twitch/api/v2/public/recent-bans"))
            .respond_with(ResponseTemplate::new(200).set_body_string("vom-proxy"))
            .expect(0) // darf NICHT erreicht werden
            .mount(&mock_server)
            .await;

        let app = Router::new()
            .route(
                "/twitch/api/v2/public/recent-bans",
                get(|| async { (StatusCode::OK, "nativ") }),
            )
            .fallback(dashboard_fallback_handler)
            .layer(Extension(DashboardProxyExt(Some(Arc::new(
                DashboardLegacyProxy::new(mock_server.uri()),
            )))));

        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/twitch/api/v2/public/recent-bans")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 256).await.unwrap();
        assert_eq!(&bytes[..], b"nativ");
        // wiremock verifiziert expect(0) beim Drop des mock_server.
    }

    // ─── 7. Host-Header wird 1:1 durchgereicht (Bypass-Schließung) ────────
    //
    // SICHERHEITSTEST: Pythons `_is_localhost()` entscheidet am Host-Header
    // (die Peer-IP ist durch den Proxy-Hop immer Loopback). Externe Requests
    // tragen via Caddy die öffentliche Domain als Host — genau dieser Wert
    // MUSS unverändert beim Upstream ankommen, sonst (Host gestrippt →
    // reqwest setzt Loopback-Host) wäre der Admin-Bypass für ALLE externen
    // Requests offen.

    /// Startet einen Upstream, der den empfangenen `Host`-Header zurückspiegelt.
    async fn spawn_host_echo_upstream() -> SocketAddr {
        let upstream = Router::new().route(
            "/twitch/api/v2/overview",
            get(|req: axum::http::Request<Body>| async move {
                let host_val = req
                    .headers()
                    .get("host")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                axum::Json(serde_json::json!({ "received_host": host_val }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn host_header_wird_unveraendert_durchgereicht() {
        let addr = spawn_host_echo_upstream().await;
        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(format!(
            "http://{addr}"
        )))));

        // Externer Request: Caddy setzt die öffentliche Domain als Host.
        // Sie muss den Upstream unverändert erreichen (→ Python: kein Bypass).
        let resp = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/twitch/api/v2/overview")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            val["received_host"].as_str().unwrap_or(""),
            "dashboard.example.com",
            "Original-Host muss den Upstream unverändert erreichen"
        );

        // Lokaler Request: Loopback-Host bleibt erhalten (Bypass für lokale
        // Admin-Tools wie heute beim Direktzugriff auf 8765).
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/twitch/api/v2/overview")
                    .header("host", "127.0.0.1:8769")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["received_host"].as_str().unwrap_or(""), "127.0.0.1:8769");
    }

    // ─── 8. x-forwarded-host wird transparent durchgereicht (Parität) ─────
    //
    // Heute erreicht der Header Python via Caddy ungefiltert; Python wertet
    // ihn in `_is_localhost()` nicht aus. Der Proxy bleibt transparent.

    #[tokio::test]
    async fn x_forwarded_host_wird_durchgereicht() {
        let upstream = Router::new().route(
            "/twitch/api/v2/overview",
            get(|req: axum::http::Request<Body>| async move {
                let xfh = req
                    .headers()
                    .get("x-forwarded-host")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                axum::Json(serde_json::json!({ "x_forwarded_host": xfh }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });

        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(format!(
            "http://{addr}"
        )))));

        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/twitch/api/v2/overview")
                    .header("x-forwarded-host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            val["x_forwarded_host"].as_str().unwrap_or(""),
            "dashboard.example.com",
            "x-forwarded-host muss transparent durchgereicht werden"
        );
    }

    // ─── 9. Alle HTTP-Methoden werden korrekt weitergeleitet ─────────────

    #[tokio::test]
    async fn patch_und_delete_werden_weitergereicht() {
        let upstream = Router::new()
            .route(
                "/twitch/api/v2/roadmap/:id",
                any(|req: axum::http::Request<Body>| async move {
                    let method = req.method().to_string();
                    axum::Json(serde_json::json!({ "method": method }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });

        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(format!(
            "http://{addr}"
        )))));

        for http_method in &["PATCH", "DELETE"] {
            let resp = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method(*http_method)
                        .uri("/twitch/api/v2/roadmap/42")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), 512).await.unwrap();
            let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(val["method"].as_str().unwrap_or(""), *http_method);
        }
    }

    // ─── 10. Response-Header werden korrekt durchgereicht ────────────────

    #[tokio::test]
    async fn response_header_werden_weitergereicht() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/twitch/api/v2/overview"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("x-custom-header", "wert123")
                    .set_body_string("ok"),
            )
            .mount(&mock_server)
            .await;

        let app = make_fallback_router(Some(Arc::new(DashboardLegacyProxy::new(
            mock_server.uri(),
        ))));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/twitch/api/v2/overview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("x-custom-header")
                .and_then(|v| v.to_str().ok()),
            Some("wert123")
        );
    }
}

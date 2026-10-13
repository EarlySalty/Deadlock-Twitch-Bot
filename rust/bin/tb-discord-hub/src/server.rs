//! HTTP-Eingang des Discord-Hubs.
//!
//! - `GET /health` ohne Authentifizierung.
//! - Alle Pfade unter [`INTERNAL_PREFIX`] (beliebige Methode): Token prüfen,
//!   dann Guild-Weiche → [`BrokerForward`] oder [`DirectDiscord`].
//! - Alles andere → 404.

use std::sync::Arc;
use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use sha2::{Digest, Sha256};

use crate::{
    BrokerForward, DirectDiscord, HubError, InboundRequest, OutboundResponse, GUILD_HEADER,
    IDEMPOTENCY_HEADER, INTERNAL_PREFIX, TOKEN_HEADER,
};

/// Maximale Größe eines Anfrage-Bodys.
pub const BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Gemeinsamer Zustand aller Handler.
#[derive(Clone)]
pub struct AppState {
    /// SHA-256 des erwarteten internen Tokens (nur der Digest wird gehalten).
    token_digest: [u8; 32],
    pub community_guild_id: u64,
    pub forward: Arc<dyn BrokerForward>,
    pub direct: Option<Arc<dyn DirectDiscord>>,
}

impl AppState {
    pub fn new(
        internal_token: &str,
        community_guild_id: u64,
        forward: Arc<dyn BrokerForward>,
        direct: Option<Arc<dyn DirectDiscord>>,
    ) -> Self {
        Self {
            token_digest: digest(internal_token.as_bytes()),
            community_guild_id,
            forward,
            direct,
        }
    }

    /// Konstant-zeitiger Vergleich über die SHA-256-Digests.
    fn token_ok(&self, presented: Option<&[u8]>) -> bool {
        let Some(presented) = presented else {
            return false;
        };
        let got = digest(presented);
        got.iter()
            .zip(self.token_digest.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Baut den Router des Hubs.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .fallback(internal)
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "direct": state.direct.is_some(),
    }))
}

fn error_json(status: u16, message: &str) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (
        status,
        Json(serde_json::json!({ "ok": false, "error": message })),
    )
        .into_response()
}

fn hub_error_response(err: &HubError) -> Response {
    // Bei Upstream wird der Status über `status()` durchgereicht.
    error_json(err.status(), &err.to_string())
}

fn outbound_response(out: OutboundResponse) -> Response {
    let status = StatusCode::from_u16(out.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    if let Some(ct) = out.content_type.as_deref() {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    builder
        .body(Body::from(out.body))
        .unwrap_or_else(|_| error_json(500, "ungültige Upstream-Antwort"))
}

/// Pfad liegt unter dem Präfix und enthält keine Traversal-Muster.
fn path_allowed(path: &str) -> bool {
    if !path.starts_with(INTERNAL_PREFIX) || path.len() == INTERNAL_PREFIX.len() {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    !(lower.contains("..")
        || lower.contains('\\')
        || lower.contains("//")
        || lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c"))
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Ziel einer Anfrage nach der Guild-Weiche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Forward,
    Direct(u64),
}

impl Target {
    fn label(self) -> &'static str {
        match self {
            Target::Forward => "forward",
            Target::Direct(_) => "direct",
        }
    }
}

fn route_guild(headers: &HeaderMap, community: u64) -> Result<(Target, Option<u64>), ()> {
    let Some(raw) = headers.get(GUILD_HEADER) else {
        return Ok((Target::Forward, None));
    };
    let raw = raw.to_str().map_err(|_| ())?.trim();
    if raw.is_empty() {
        return Ok((Target::Forward, None));
    }
    let guild: u64 = raw.parse().map_err(|_| ())?;
    if guild == 0 {
        return Err(());
    }
    if guild == community {
        Ok((Target::Forward, Some(guild)))
    } else {
        Ok((Target::Direct(guild), Some(guild)))
    }
}

async fn internal(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let path = uri.path().to_string();

    if !path.starts_with(INTERNAL_PREFIX) {
        tracing::debug!(path = %path, "Unbekannter Pfad");
        return error_json(404, "nicht gefunden");
    }
    if !state.token_ok(headers.get(TOKEN_HEADER).map(|v| v.as_bytes())) {
        tracing::warn!(path = %path, "Anfrage ohne gültiges internes Token abgewiesen");
        return error_json(401, "nicht autorisiert");
    }
    if !path_allowed(&path) {
        tracing::warn!(path = %path, "Unzulässiger Pfad abgewiesen");
        return error_json(404, "nicht gefunden");
    }

    let (target, guild) = match route_guild(&headers, state.community_guild_id) {
        Ok(v) => v,
        Err(()) => {
            tracing::warn!(path = %path, "Ungültige Guild-ID im Header");
            return error_json(400, "ungültige Guild-ID");
        }
    };

    let req = InboundRequest {
        method: method.clone(),
        path: path.clone(),
        query: uri.query().map(|q| q.to_string()),
        idempotency_key: header_str(&headers, IDEMPOTENCY_HEADER),
        content_type: header_str(&headers, header::CONTENT_TYPE.as_str()),
        body,
    };

    let result = match target {
        Target::Forward => state.forward.forward(req).await,
        Target::Direct(guild_id) => match state.direct.as_ref() {
            Some(direct) => direct.handle(guild_id, req).await,
            None => {
                tracing::warn!(
                    path = %path,
                    guild = guild_id,
                    "Fremde Guild, aber kein Bot-Token konfiguriert"
                );
                return error_json(503, "direkter Discord-Zugriff nicht konfiguriert");
            }
        },
    };

    let response = match result {
        Ok(out) => outbound_response(out),
        Err(err) => {
            tracing::warn!(
                path = %path,
                guild = ?guild,
                target = target.label(),
                error = %err,
                "Anfrage fehlgeschlagen"
            );
            hub_error_response(&err)
        }
    };
    tracing::info!(
        method = %method,
        path = %path,
        guild = ?guild,
        target = target.label(),
        status = response.status().as_u16(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Interne Anfrage bearbeitet"
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use std::sync::Mutex;
    use tower::ServiceExt;

    const TOKEN: &str = "test-token";
    const COMMUNITY: u64 = 1_289_721_245_281_292_288;

    #[derive(Default)]
    struct FakeForward {
        calls: Mutex<Vec<InboundRequest>>,
        fail: Option<u16>,
    }

    #[async_trait::async_trait]
    impl BrokerForward for FakeForward {
        async fn forward(&self, req: InboundRequest) -> Result<OutboundResponse, HubError> {
            self.calls.lock().unwrap().push(req);
            if let Some(status) = self.fail {
                return Err(HubError::Upstream {
                    status,
                    body: "kaputt".into(),
                });
            }
            Ok(OutboundResponse {
                status: 202,
                content_type: Some("text/plain".into()),
                body: Bytes::from_static(b"vom-broker"),
            })
        }
    }

    #[derive(Default)]
    struct FakeDirect {
        calls: Mutex<Vec<(u64, InboundRequest)>>,
    }

    #[async_trait::async_trait]
    impl DirectDiscord for FakeDirect {
        async fn handle(
            &self,
            guild_id: u64,
            req: InboundRequest,
        ) -> Result<OutboundResponse, HubError> {
            self.calls.lock().unwrap().push((guild_id, req));
            Ok(OutboundResponse::json(
                200,
                &serde_json::json!({"ok": true, "via": "direct"}),
            ))
        }
    }

    fn app(fwd: Arc<FakeForward>, direct: Option<Arc<FakeDirect>>) -> Router {
        router(AppState::new(
            TOKEN,
            COMMUNITY,
            fwd,
            direct.map(|d| d as Arc<dyn DirectDiscord>),
        ))
    }

    fn post(path: &str) -> axum::http::request::Builder {
        Request::builder()
            .method("POST")
            .uri(path)
            .header(TOKEN_HEADER, TOKEN)
    }

    async fn body_of(resp: Response) -> (StatusCode, Option<String>, Vec<u8>) {
        let status = resp.status();
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string());
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, ct, bytes.to_vec())
    }

    const PATH: &str = "/internal/master/v1/discord/messages";

    #[tokio::test]
    async fn health_ohne_auth() {
        let resp = app(Arc::default(), None)
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, _, body) = body_of(resp).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn fehlendes_oder_falsches_token_401() {
        let fwd = Arc::new(FakeForward::default());
        let resp = app(fwd.clone(), None)
            .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = app(fwd.clone(), None)
            .oneshot(
                Request::post(PATH)
                    .header(TOKEN_HEADER, "falsch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, _, body) = body_of(resp).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], false);
        assert!(fwd.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ohne_guild_header_an_broker() {
        let fwd = Arc::new(FakeForward::default());
        let direct = Arc::new(FakeDirect::default());
        let resp = app(fwd.clone(), Some(direct.clone()))
            .oneshot(
                post("/internal/master/v1/discord/messages?a=1&b=2")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(IDEMPOTENCY_HEADER, "idem-1")
                    .body(Body::from("{\"x\":1}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, ct, body) = body_of(resp).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(ct.as_deref(), Some("text/plain"));
        assert_eq!(body, b"vom-broker");
        let calls = fwd.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let req = &calls[0];
        assert_eq!(req.method, Method::POST);
        assert_eq!(req.path, PATH);
        assert_eq!(req.query.as_deref(), Some("a=1&b=2"));
        assert_eq!(req.idempotency_key.as_deref(), Some("idem-1"));
        assert_eq!(req.content_type.as_deref(), Some("application/json"));
        assert_eq!(&req.body[..], b"{\"x\":1}");
        assert!(direct.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn community_guild_an_broker() {
        let fwd = Arc::new(FakeForward::default());
        let direct = Arc::new(FakeDirect::default());
        let resp = app(fwd.clone(), Some(direct.clone()))
            .oneshot(
                post(PATH)
                    .header(GUILD_HEADER, COMMUNITY.to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        assert_eq!(fwd.calls.lock().unwrap().len(), 1);
        assert!(direct.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fremde_guild_direkt() {
        let fwd = Arc::new(FakeForward::default());
        let direct = Arc::new(FakeDirect::default());
        let resp = app(fwd.clone(), Some(direct.clone()))
            .oneshot(
                Request::get(PATH)
                    .header(TOKEN_HEADER, TOKEN)
                    .header(GUILD_HEADER, "123456789")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, ct, body) = body_of(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ct.as_deref(), Some("application/json"));
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["via"], "direct");
        let calls = direct.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, 123_456_789);
        assert_eq!(calls[0].1.method, Method::GET);
        assert!(fwd.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fremde_guild_ohne_direct_503() {
        let fwd = Arc::new(FakeForward::default());
        let resp = app(fwd.clone(), None)
            .oneshot(
                post(PATH)
                    .header(GUILD_HEADER, "42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, _, body) = body_of(resp).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], false);
        assert!(fwd.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ungueltige_guild_400() {
        for bad in ["abc", "-1", "0", "1.5", "99999999999999999999999"] {
            let fwd = Arc::new(FakeForward::default());
            let resp = app(fwd.clone(), Some(Arc::default()))
                .oneshot(
                    post(PATH)
                        .header(GUILD_HEADER, bad)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{bad}");
            assert!(fwd.calls.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn upstream_status_wird_durchgereicht() {
        let fwd = Arc::new(FakeForward {
            fail: Some(429),
            ..Default::default()
        });
        let resp = app(fwd, None)
            .oneshot(post(PATH).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, ct, body) = body_of(resp).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(ct.as_deref(), Some("application/json"));
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("429"));
    }

    #[tokio::test]
    async fn pfade_ausserhalb_oder_traversal_404() {
        for path in [
            "/",
            "/internal/master/v2/x",
            "/internal/master/v1/",
            "/internal/master/v1/discord/../admin",
            "/internal/master/v1/discord/%2e%2e/admin",
            "/internal/master/v1/discord/a%5Cb",
            "/internal/master/v1/discord//x",
        ] {
            let fwd = Arc::new(FakeForward::default());
            let resp = app(fwd.clone(), None)
                .oneshot(post(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
            assert!(fwd.calls.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn body_limit_413() {
        let fwd = Arc::new(FakeForward::default());
        let resp = app(fwd.clone(), None)
            .oneshot(
                post(PATH)
                    .body(Body::from(vec![b'x'; BODY_LIMIT_BYTES + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(fwd.calls.lock().unwrap().is_empty());
    }
}

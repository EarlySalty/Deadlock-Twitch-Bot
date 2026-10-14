use std::time::Duration;

use axum::body::Bytes;
use axum::http::Method;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Url};

use crate::{
    BrokerForward, HubError, InboundRequest, OutboundResponse, IDEMPOTENCY_HEADER, INTERNAL_PREFIX,
    TOKEN_HEADER,
};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: u32 = 2;
const RETRY_WAIT: Duration = Duration::from_secs(2);

pub struct BrokerForwarder {
    client: Client,
    base_url: Url,
    token: String,
}

impl std::fmt::Debug for BrokerForwarder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerForwarder")
            .field("base_url", &self.base_url.as_str())
            .finish_non_exhaustive()
    }
}

impl BrokerForwarder {
    pub fn new(base_url: &str, token: &str) -> Result<Self, HubError> {
        let base_url = validate_base_url(base_url)?;
        if token.is_empty() {
            return Err(HubError::Internal("Broker-Token fehlt".into()));
        }
        let client = Client::builder()
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| HubError::Internal(format!("HTTP-Client nicht erstellbar: {e}")))?;
        Ok(Self {
            client,
            base_url,
            token: token.to_string(),
        })
    }

    fn request_url(&self, path: &str, query: Option<&str>) -> Result<Url, HubError> {
        if !path.starts_with(INTERNAL_PREFIX)
            || path.contains(['?', '#', '\\'])
            || path.split('/').any(|part| part == "." || part == "..")
        {
            return Err(HubError::BadRequest(format!(
                "ungültiger Broker-Pfad: {path}"
            )));
        }
        if query.is_some_and(|q| q.contains('#')) {
            return Err(HubError::BadRequest("ungültige Query".into()));
        }
        let mut url = self.base_url.clone();
        url.set_path(path);
        if !url.path().starts_with(INTERNAL_PREFIX) {
            return Err(HubError::BadRequest(format!(
                "ungültiger Broker-Pfad: {path}"
            )));
        }
        match query {
            Some(q) if !q.is_empty() => url.set_query(Some(q)),
            _ => url.set_query(None),
        }
        Ok(url)
    }
}

fn validate_base_url(raw: &str) -> Result<Url, HubError> {
    let invalid = || {
        HubError::Internal(
            "ungültiger Broker-Endpunkt: HTTPS oder numerischer Loopback erforderlich".into(),
        )
    };
    let url = Url::parse(raw).map_err(|_| invalid())?;
    let loopback = url.host_str().is_some_and(|host| {
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    });
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(invalid());
    }
    Ok(url)
}

#[async_trait::async_trait]
impl BrokerForward for BrokerForwarder {
    async fn forward(&self, req: InboundRequest) -> Result<OutboundResponse, HubError> {
        let url = self.request_url(&req.path, req.query.as_deref())?;
        let retryable = req.idempotency_key.is_some() || req.method == Method::GET;
        let attempts = if retryable { MAX_ATTEMPTS } else { 1 };
        let mut last_err = String::from("Broker-Aufruf ohne Ergebnis");

        for attempt in 0..attempts {
            let mut builder = self
                .client
                .request(req.method.clone(), url.clone())
                .header(TOKEN_HEADER, &self.token);
            if let Some(key) = &req.idempotency_key {
                builder = builder.header(IDEMPOTENCY_HEADER, key);
            }
            if let Some(ct) = &req.content_type {
                builder = builder.header(CONTENT_TYPE, ct);
            }
            if !req.body.is_empty() {
                builder = builder.body(req.body.clone());
            }

            match builder.send().await {
                Ok(resp) => return read_response(resp).await,
                Err(e) if e.is_timeout() || e.is_connect() => {
                    tracing::warn!(
                        path = %req.path,
                        versuch = attempt + 1,
                        fehler = %e,
                        "Broker nicht erreichbar"
                    );
                    last_err = e.to_string();
                    if attempt + 1 < attempts {
                        tokio::time::sleep(RETRY_WAIT).await;
                    }
                }
                Err(e) => {
                    tracing::warn!(path = %req.path, fehler = %e, "Broker-Aufruf fehlgeschlagen");
                    return Err(HubError::Unreachable(e.to_string()));
                }
            }
        }
        Err(HubError::Unreachable(last_err))
    }
}

async fn read_response(resp: reqwest::Response) -> Result<OutboundResponse, HubError> {
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body: Bytes = resp
        .bytes()
        .await
        .map_err(|e| HubError::Unreachable(format!("Broker-Antwort unvollständig: {e}")))?;
    Ok(OutboundResponse {
        status,
        content_type,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GUILD_HEADER;
    use std::time::Instant;
    use wiremock::matchers::{body_bytes, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn req(m: Method, p: &str) -> InboundRequest {
        InboundRequest {
            method: m,
            path: p.to_string(),
            query: None,
            idempotency_key: None,
            content_type: None,
            body: Bytes::new(),
        }
    }

    #[test]
    fn new_lehnt_ungueltige_basisadressen_ab() {
        for bad in [
            "http://example.com",
            "http://localhost:8080",
            "http://10.0.0.1:8080",
            "https://user:pw@example.com",
            "https://example.com/?a=b",
            "https://example.com/#x",
            "https://example.com/pfad",
            "ftp://127.0.0.1",
            "kein-url",
        ] {
            assert!(
                BrokerForwarder::new(bad, "t").is_err(),
                "{bad} müsste abgelehnt werden"
            );
        }
        for good in [
            "https://broker.example.com",
            "http://127.0.0.1:9000",
            "http://[::1]:9000/",
        ] {
            assert!(
                BrokerForwarder::new(good, "t").is_ok(),
                "{good} müsste gültig sein"
            );
        }
    }

    #[tokio::test]
    async fn ungueltige_pfade_werden_abgelehnt() {
        let f = BrokerForwarder::new("http://127.0.0.1:9", "t").unwrap();
        for bad in [
            "/internal/other",
            "/internal/master/v1/../admin",
            "/internal/master/v1/a\\b",
            "/internal/master/v1/%2e%2e/x",
            "/internal/master/v1/a?x=1",
        ] {
            let err = f.forward(req(Method::GET, bad)).await.unwrap_err();
            assert!(matches!(err, HubError::BadRequest(_)), "{bad}: {err:?}");
        }
    }

    #[tokio::test]
    async fn reicht_anfrage_unveraendert_durch() {
        let server = MockServer::start().await;
        let body = r#"{"channel_id":1,"content":"ä hallo"}"#.as_bytes().to_vec();
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/messages"))
            .and(query_param("a", "1"))
            .and(query_param("b", "x y"))
            .and(header(TOKEN_HEADER, "geheim"))
            .and(header(IDEMPOTENCY_HEADER, "key-123"))
            .and(header("content-type", "application/json; charset=utf-8"))
            .and(body_bytes(body.clone()))
            .respond_with(
                ResponseTemplate::new(201).set_body_raw(r#"{"ok":true}"#, "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let f = BrokerForwarder::new(&server.uri(), "geheim").unwrap();
        let resp = f
            .forward(InboundRequest {
                method: Method::POST,
                path: "/internal/master/v1/discord/messages".into(),
                query: Some("a=1&b=x%20y".into()),
                idempotency_key: Some("key-123".into()),
                content_type: Some("application/json; charset=utf-8".into()),
                body: Bytes::from(body),
            })
            .await
            .unwrap();
        assert_eq!(resp.status, 201);
        assert_eq!(resp.content_type.as_deref(), Some("application/json"));
        assert_eq!(&resp.body[..], br#"{"ok":true}"#);

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        let r = &received[0];
        assert_eq!(r.url.query(), Some("a=1&b=x%20y"));
        assert!(
            r.headers.get(GUILD_HEADER).is_none(),
            "Guild-Header darf nicht weiter"
        );
    }

    #[tokio::test]
    async fn fehlerstatus_werden_durchgereicht() {
        let server = MockServer::start().await;
        Mock::given(path("/internal/master/v1/discord/a"))
            .respond_with(ResponseTemplate::new(404).set_body_string("weg"))
            .mount(&server)
            .await;
        Mock::given(path("/internal/master/v1/discord/b"))
            .respond_with(
                ResponseTemplate::new(503).set_body_raw(r#"{"error":"x"}"#, "application/json"),
            )
            .mount(&server)
            .await;
        let f = BrokerForwarder::new(&server.uri(), "t").unwrap();

        let a = f
            .forward(req(Method::GET, "/internal/master/v1/discord/a"))
            .await
            .unwrap();
        assert_eq!(a.status, 404);
        assert_eq!(&a.body[..], b"weg");
        let b = f
            .forward(req(Method::POST, "/internal/master/v1/discord/b"))
            .await
            .unwrap();
        assert_eq!(b.status, 503);
        assert_eq!(b.content_type.as_deref(), Some("application/json"));
        assert_eq!(&b.body[..], br#"{"error":"x"}"#);
    }

    #[tokio::test]
    async fn redirect_wird_nicht_verfolgt() {
        let ziel = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&ziel)
            .await;
        let server = MockServer::start().await;
        Mock::given(path("/internal/master/v1/discord/r"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/abgefangen", ziel.uri())),
            )
            .mount(&server)
            .await;

        let f = BrokerForwarder::new(&server.uri(), "geheim").unwrap();
        let resp = f
            .forward(req(Method::GET, "/internal/master/v1/discord/r"))
            .await
            .unwrap();
        assert_eq!(resp.status, 302);
        assert!(ziel.received_requests().await.unwrap().is_empty());
    }

    fn freier_port() -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn retry_bei_verbindungsfehler_mit_idempotenz() {
        let port = freier_port();
        let server = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            let l = tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .unwrap();
            let (mut s, _) = l.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = vec![0u8; 4096];
            let _ = s.read(&mut buf).await;
            s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await
                .unwrap();
            String::from_utf8_lossy(&buf).to_string()
        });

        let f = BrokerForwarder::new(&format!("http://127.0.0.1:{port}"), "t").unwrap();
        let mut r = req(Method::POST, "/internal/master/v1/discord/x");
        r.idempotency_key = Some("k1".into());
        let resp = f.forward(r).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(&resp.body[..], b"ok");
        let raw = server.await.unwrap().to_ascii_lowercase();
        assert!(raw.contains("x-idempotency-key: k1"));
    }

    #[tokio::test]
    async fn kein_retry_ohne_idempotenz_bei_post() {
        let port = freier_port();
        let f = BrokerForwarder::new(&format!("http://127.0.0.1:{port}"), "t").unwrap();
        let start = Instant::now();
        let err = f
            .forward(req(Method::POST, "/internal/master/v1/discord/x"))
            .await
            .unwrap_err();
        assert!(matches!(err, HubError::Unreachable(_)), "{err:?}");
        assert!(start.elapsed() < RETRY_WAIT, "darf nicht gewartet haben");
    }
}

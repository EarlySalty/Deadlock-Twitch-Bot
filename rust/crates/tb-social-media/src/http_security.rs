//! Transport policy for credential-bearing OAuth and upload requests.
//!
//! Public endpoints require TLS. Literal loopback HTTP is retained for isolated
//! tests; DNS names such as localhost are deliberately not an exception.

pub(crate) fn endpoint(raw: &str) -> Result<reqwest::Url, &'static str> {
    let url = reqwest::Url::parse(raw).map_err(|_| "invalid endpoint URL")?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("endpoint requires HTTPS or literal loopback without credentials or fragment");
    }
    Ok(url)
}

pub(crate) fn client(timeout: std::time::Duration) -> reqwest::Client {
    // Never silently fall back to Client::default(): that restores redirects.
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("secure social-media HTTP client must initialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_endpoint_requires_tls_except_literal_loopback() {
        for value in [
            "https://graph.instagram.com/v23.0/me",
            "http://127.0.0.1:8080/token",
            "http://[::1]:8080/token",
        ] {
            assert!(endpoint(value).is_ok());
        }
        for value in [
            "http://example.test/token",
            "http://localhost/token",
            "http://127.0.0.1.evil.test/token",
            "http://127.0.0.1@evil.test/token",
            "https://user@example.test/token",
            "https://example.test/#token",
            "ftp://example.test/token",
            "http://10.0.0.1/token",
        ] {
            assert!(endpoint(value).is_err());
        }
    }

    #[tokio::test]
    async fn security_client_never_follows_credential_redirects() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
        let target = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&target)
            .await;
        let origin = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(307).insert_header("Location", target.uri()))
            .expect(1)
            .mount(&origin)
            .await;
        let response = client(std::time::Duration::from_secs(2))
            .post(endpoint(&origin.uri()).unwrap())
            .form(&[("access_token", "synthetic-test-value")])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 307);
        assert!(target.received_requests().await.unwrap().is_empty());
    }
}

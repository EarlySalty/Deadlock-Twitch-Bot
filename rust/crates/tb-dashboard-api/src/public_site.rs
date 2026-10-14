//! Öffentliche Domain einer Dashboard-Anfrage (Paket O).
//!
//! Caddy reicht den ursprünglichen `Host`-Header an das Backend durch. Daraus
//! wird nur über die feste Allowlist in [`tb_config::PublicSite`] gewählt; ein
//! unbekannter oder fehlender Host fällt auf die Hauptdomain zurück.

use axum::http::{header, HeaderMap};
pub use tb_config::PublicSite;

/// Domain, auf der die Anfrage gestartet wurde.
pub fn request_site(headers: &HeaderMap) -> PublicSite {
    PublicSite::from_host_or_primary(headers.get(header::HOST).and_then(|v| v.to_str().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        headers
    }

    #[test]
    fn host_header_waehlt_domain_aus_allowlist() {
        assert_eq!(
            request_site(&headers("dead-lock.de")),
            PublicSite::International
        );
        assert_eq!(
            request_site(&headers("deutsche-deadlock-community.de")),
            PublicSite::Primary
        );
        assert_eq!(request_site(&headers("evil.test")), PublicSite::Primary);
        assert_eq!(request_site(&HeaderMap::new()), PublicSite::Primary);
    }
}

//! AuthLevel-Kaskade für das Dashboard-API.
//!
//! Reihenfolge (Python: `server_v2.py:745-755`, `_is_local_request`):
//! 1. **Localhost** — Host-Header UND Peer-IP sind beide Loopback
//!    (gleiche Bedingung wie Python `_is_local_request`)
//! 2. **Admin** — Cookie `master_dash_session` ist gültig in der DB
//!    (Discord-Admin-Session, Typ `discord_admin`)
//! 3. **Partner** — Cookie `twitch_dash_session` ist gültig in der DB
//!    (Twitch-OAuth-Session, Typ `twitch`) UND nicht in `twitch_token_blacklist`
//!    UND in `twitch_partners` vorhanden (gleiche WHERE-Bedingung wie Python
//!    `_is_partner_allowed`, `auth_mixin.py:741-780`)
//! 4. **None** — alles andere
//!
//! UNSICHER: Hinter Reverse-Proxy (Caddy) ist `peer_ip` immer 127.0.0.1.
//! Der Localhost-Check verhält sich dann wie ein reiner Host-Header-Check —
//! das entspricht dem Python-Verhalten (Python hat denselben Proxy-Blindspot,
//! `auth_mixin.py:735-755`). Nicht geeignet als Sicherheitsgrenze gegenüber
//! dem Internet, aber identisch zu Python.

use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use std::net::{IpAddr, SocketAddr};

/// Auth-Level eines eingehenden Dashboard-Requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardAuthLevel {
    /// Loopback-IP UND Loopback-Host (kein Session-Lookup nötig).
    Localhost,
    /// Gültige `master_dash_session`-Cookie (Discord-Admin).
    Admin,
    /// Gültige `twitch_dash_session`-Cookie + Partner in DB + nicht blacklisted.
    Partner { twitch_login: String, twitch_user_id: String },
    /// Nicht authentifiziert.
    None,
}

impl DashboardAuthLevel {
    /// `true` wenn Localhost oder Admin.
    pub fn is_privileged(&self) -> bool {
        matches!(self, Self::Localhost | Self::Admin)
    }

    /// `true` wenn Localhost, Admin oder Partner.
    pub fn is_authenticated(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Localhost => "localhost",
            Self::Admin => "admin",
            Self::Partner { .. } => "partner",
            Self::None => "none",
        }
    }
}

/// Hilfe: prüft ob ein Host-String auf Loopback zeigt.
///
/// Python: `_is_loopback_host`, `server_v2.py:707-715`
pub(crate) fn is_loopback_host(raw: &str) -> bool {
    let host = strip_port(raw);
    if host.is_empty() {
        return false;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // IPv6-Literal wie [::1] oder rohes ::1
    let candidate = if host.starts_with('[') && host.ends_with(']') {
        &host[1..host.len() - 1]
    } else {
        host
    };
    candidate
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Entfernt Port aus `host:port`-String.
///
/// Sonderfälle:
/// - `[::1]:8080` → `[::1]`
/// - `::1` (rohes IPv6 ohne Brackets) → `::1` (Port nicht entfernbar)
/// - `127.0.0.1:8080` → `127.0.0.1`
fn strip_port(raw: &str) -> &str {
    let s = raw.trim();
    // IPv6 in Brackets: [::1]:8080 → [::1]
    if s.starts_with('[') {
        if let Some(end) = s.find(']') {
            return &s[..=end];
        }
        return s;
    }
    // Rohes IPv6 (enthält mehr als einen Doppelpunkt) → kein Port-Strip
    if s.chars().filter(|&c| c == ':').count() > 1 {
        return s;
    }
    // IPv4/DNS: host:port — genau ein Doppelpunkt, Port muss Ziffern sein
    if let Some(colon) = s.find(':') {
        let port_part = &s[colon + 1..];
        if port_part.chars().all(|c| c.is_ascii_digit()) {
            return &s[..colon];
        }
    }
    s
}

/// Prüft ob Peer-IP + Host-Header beide Loopback sind.
///
/// Python: `_is_local_request`, `server_v2.py:745-755`
pub(crate) fn is_local_request(parts: &Parts) -> bool {
    // Host-Header muss Loopback sein
    let host_header = parts
        .headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !is_loopback_host(host_header) {
        return false;
    }

    // Peer-IP muss Loopback sein
    let peer_ip = parts
        .extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());

    match peer_ip {
        Some(ip) => ip.is_loopback(),
        // Kein ConnectInfo → konservativ false
        None => false,
    }
}

/// Liest den Session-Cookie-Wert aus dem `Cookie`-Header.
pub(crate) fn extract_cookie<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
    let cookie_header = parts
        .headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())?;

    // Cookie-Header: "name1=val1; name2=val2"
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k.trim() == name {
                return Some(v.trim());
            }
        }
    }
    None
}

/// Twitch-Logins mit Admin-Zugriff (wie Discord-Admin / Localhost).
/// Spiegelt Python `_TWITCH_ADMIN_LOGINS` (api_v2.py:464), kleingeschrieben.
const TWITCH_ADMIN_LOGINS: &[&str] = &["earlysalty"];

/// Axum-Extractor für `DashboardAuthLevel`.
///
/// Benötigt `DashboardAuthState` als Extension im Router.
/// Ohne Extension → immer `None` (fail-closed).
#[async_trait]
impl<S> FromRequestParts<S> for DashboardAuthLevel
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // Localhost-Check zuerst (kein DB-Lookup nötig)
        if is_local_request(parts) {
            return Ok(DashboardAuthLevel::Localhost);
        }

        // Auth-State-Extension holen (enthält Pool + Cache)
        let Some(state) = parts.extensions.get::<crate::auth::session::DashboardAuthState>().cloned() else {
            return Ok(DashboardAuthLevel::None);
        };

        // Admin: master_dash_session
        if let Some(session_id) = extract_cookie(parts, "master_dash_session") {
            if !session_id.is_empty() {
                if let Ok(Some(_)) = state.load_admin_session(session_id).await {
                    return Ok(DashboardAuthLevel::Admin);
                }
            }
        }

        // Partner: twitch_dash_session
        if let Some(session_id) = extract_cookie(parts, "twitch_dash_session") {
            if !session_id.is_empty() {
                if let Ok(Some(partner)) = state.load_partner_session(session_id).await {
                    // Admin-Login-Promotion (Python api_v2.py:1339-1342): loggt sich
                    // ein Admin per Twitch-OAuth statt Discord ein, bekommt er
                    // Admin-Rechte (canViewAllStreamers), nicht nur Partner.
                    if TWITCH_ADMIN_LOGINS.contains(&partner.twitch_login.trim().to_lowercase().as_str()) {
                        return Ok(DashboardAuthLevel::Admin);
                    }
                    return Ok(DashboardAuthLevel::Partner {
                        twitch_login: partner.twitch_login,
                        twitch_user_id: partner.twitch_user_id,
                    });
                }
            }
        }

        Ok(DashboardAuthLevel::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_erkannt() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("localhost:8769"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.0.0.1:8769"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("[::1]"));
        assert!(is_loopback_host("[::1]:8080"));
    }

    #[test]
    fn nicht_loopback_hosts_abgelehnt() {
        assert!(!is_loopback_host("example.com"));
        assert!(!is_loopback_host("192.168.1.1"));
        assert!(!is_loopback_host("10.0.0.1:8080"));
        assert!(!is_loopback_host(""));
    }

    #[test]
    fn cookie_extraktion_korrekt() {
        use axum::http::{HeaderMap, HeaderValue, Request, header::COOKIE};

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("twitch_dash_session=abc123; master_dash_session=xyz789; other=val"),
        );
        let req = Request::builder()
            .header(COOKIE, "twitch_dash_session=abc123; master_dash_session=xyz789; other=val")
            .body(())
            .unwrap();
        let (parts, _) = req.into_parts();

        assert_eq!(extract_cookie(&parts, "twitch_dash_session"), Some("abc123"));
        assert_eq!(extract_cookie(&parts, "master_dash_session"), Some("xyz789"));
        assert_eq!(extract_cookie(&parts, "other"), Some("val"));
        assert_eq!(extract_cookie(&parts, "missing"), None);
    }

    #[test]
    fn cookie_extraktion_leer() {
        let req = axum::http::Request::builder().body(()).unwrap();
        let (parts, _) = req.into_parts();
        assert_eq!(extract_cookie(&parts, "any"), None);
    }

    #[test]
    fn auth_level_as_str() {
        assert_eq!(DashboardAuthLevel::Localhost.as_str(), "localhost");
        assert_eq!(DashboardAuthLevel::Admin.as_str(), "admin");
        assert_eq!(
            DashboardAuthLevel::Partner {
                twitch_login: "x".into(),
                twitch_user_id: "1".into()
            }
            .as_str(),
            "partner"
        );
        assert_eq!(DashboardAuthLevel::None.as_str(), "none");
    }

    #[test]
    fn is_privileged_korrekt() {
        assert!(DashboardAuthLevel::Localhost.is_privileged());
        assert!(DashboardAuthLevel::Admin.is_privileged());
        assert!(!DashboardAuthLevel::Partner {
            twitch_login: "x".into(),
            twitch_user_id: "1".into()
        }
        .is_privileged());
        assert!(!DashboardAuthLevel::None.is_privileged());
    }
}

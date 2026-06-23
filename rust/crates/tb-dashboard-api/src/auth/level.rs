//! AuthLevel-Kaskade für das Dashboard-API.
//!
//! Reihenfolge:
//! 1. **Partner/Twitch-Admin** — Cookie `twitch_dash_session` ist gültig in der DB
//!    (Twitch-OAuth-Session, Typ `twitch`) UND in `twitch_partners` vorhanden
//!    (gleiche WHERE-Bedingung wie Python `_is_partner_allowed`,
//!    `auth_mixin.py:741-780`). KEINE `twitch_token_blacklist`-Prüfung — ein
//!    token_error-Blacklist-Eintrag sperrt den Dashboard-Zugang nicht.
//! 2. **Admin** — Cookie `master_dash_session` ist gültig in der DB
//!    (Discord-Admin-Session, Typ `discord_admin`)
//! 3. **None** — alles andere

use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use std::net::{IpAddr, SocketAddr};

/// Twitch-Session-Identität eines per OAuth eingeloggten Admins (senderauth-01).
///
/// Wird gesetzt, wenn ein Twitch-Login mit Admin-Rechten (`TWITCH_ADMIN_LOGINS`,
/// z. B. `earlysalty`) zum `DashboardAuthLevel::Admin` promoted wird. Trägt die
/// Session-Identität weiter, damit Handler sie für die Audit-Attribution
/// (z. B. `enabled_by`) nutzen können — analog zu Pythons `_extract_session_user`,
/// das `actor_id`/`actor_login` IMMER aus der Session liest, auch bei `admin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminActor {
    pub twitch_user_id: String,
    /// Twitch-Login, bereits kleingeschrieben.
    pub twitch_login: String,
}

/// Auth-Level eines eingehenden Dashboard-Requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardAuthLevel {
    /// Admin-Zugang. `actor = None` für Discord-Admin (`master_dash_session`,
    /// keine Twitch-Identität); `actor = Some(..)` für einen
    /// per Twitch-OAuth eingeloggten Admin (Login-Promotion, senderauth-01).
    Admin { actor: Option<AdminActor> },
    /// Gültige `twitch_dash_session`-Cookie + Partner in DB + nicht blacklisted.
    Partner {
        twitch_login: String,
        twitch_user_id: String,
        /// Twitch-`display_name` aus dem Login-Snapshot (leer → Login-Fallback).
        display_name: String,
    },
    /// Nicht authentifiziert.
    None,
}

impl DashboardAuthLevel {
    /// Admin-Level ohne Twitch-Session-Identität (Discord-Admin / Tests).
    /// Kurzform für `Admin { actor: None }`.
    pub fn admin() -> Self {
        Self::Admin { actor: None }
    }

    /// `true` wenn Admin.
    pub fn is_privileged(&self) -> bool {
        matches!(self, Self::Admin { .. })
    }

    /// `true` wenn Admin oder Partner.
    pub fn is_authenticated(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin { .. } => "admin",
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
/// Caddy setzt bei proxied Requests `X-Forwarded-For`; ein echter Direkt-
/// Loopback-curl trägt keine Forwarding-Header. Der Bypass bleibt damit
/// fail-closed, auch wenn Reverse-Proxy-Routing falsch konfiguriert ist.
pub(crate) fn is_local_request(parts: &Parts) -> bool {
    if parts.headers.contains_key("x-forwarded-for") || parts.headers.contains_key("x-forwarded-host") {
        return false;
    }

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

/// Twitch-Logins mit Admin-Zugriff (wie Discord-Admin).
/// Spiegelt Python `_TWITCH_ADMIN_LOGINS` (api_v2.py:464), kleingeschrieben.
const TWITCH_ADMIN_LOGINS: &[&str] = &["earlysalty"];

pub(crate) fn is_admin_login(login: &str) -> bool {
    let login = login.trim().to_lowercase();
    TWITCH_ADMIN_LOGINS.contains(&login.as_str())
}

fn admin_mode_cookie_active(parts: &Parts) -> bool {
    extract_cookie(parts, crate::handlers::auth_status::ADMIN_MODE_COOKIE) == Some("2")
}

/// Macht aus einer geladenen Partner-Session das Auth-Level: Admin-Login-Promotion
/// nur bei aktivem Admin-Mode-Cookie, sonst bleibt auch ein admin-eligibler Login
/// Partner.
fn partner_or_admin(partner: crate::auth::session::PartnerSession, admin_mode_active: bool) -> DashboardAuthLevel {
    let login = partner.twitch_login.trim().to_lowercase();
    if is_admin_login(&login) && admin_mode_active {
        // senderauth-01: Twitch-Session-Identität an den Admin durchreichen, damit
        // Handler sie für die Audit-Attribution nutzen können (Python liest
        // actor_id/actor_login IMMER aus der Session, auch bei auth_level='admin').
        return DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: partner.twitch_user_id,
                twitch_login: login,
            }),
        };
    }
    DashboardAuthLevel::Partner {
        twitch_login: partner.twitch_login,
        twitch_user_id: partner.twitch_user_id,
        display_name: partner.display_name,
    }
}

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
        // Auth-State-Extension holen (enthält Pool + Cache)
        let Some(state) = parts.extensions.get::<crate::auth::session::DashboardAuthState>().cloned() else {
            return Ok(DashboardAuthLevel::None);
        };

        let admin_mode_active = admin_mode_cookie_active(parts);

        // Partner: twitch_dash_session
        if let Some(session_id) = extract_cookie(parts, crate::auth::session::PARTNER_COOKIE_NAME) {
            if !session_id.is_empty() {
                if let Ok(Some(partner)) = state.load_partner_session(session_id).await {
                    return Ok(partner_or_admin(partner, admin_mode_active));
                }
            }
        }

        // Partner-Access-Session: twitch_dash_session_partner (B3-9). Durable
        // Session nach Einmal-Login; überdauert die kurzlebige twitch_dash_session.
        // Konsumiert wie Python `_get_partner_access_session` (api_v2.py:1349-1352)
        // NACH dem Dashboard-Session-Check, mit Fingerprint-Bindung gegen den
        // Request-User-Agent.
        if let Some(session_id) =
            extract_cookie(parts, crate::auth::session::PARTNER_ACCESS_COOKIE_NAME)
        {
            if !session_id.is_empty() {
                let user_agent = parts
                    .headers
                    .get(axum::http::header::USER_AGENT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if let Ok(Some(partner)) =
                    state.load_partner_access_session(session_id, user_agent).await
                {
                    return Ok(partner_or_admin(partner, admin_mode_active));
                }
            }
        }

        // Admin: master_dash_session
        if let Some(session_id) = extract_cookie(parts, crate::auth::session::ADMIN_COOKIE_NAME) {
            if !session_id.is_empty() {
                if let Ok(Some(_)) = state.load_admin_session(session_id).await {
                    // Discord-Admin: keine Twitch-Session-Identität → keine Attribution.
                    return Ok(DashboardAuthLevel::admin());
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
        assert_eq!(DashboardAuthLevel::admin().as_str(), "admin");
        assert_eq!(
            DashboardAuthLevel::Admin {
                actor: Some(AdminActor {
                    twitch_user_id: "1".into(),
                    twitch_login: "earlysalty".into()
                })
            }
            .as_str(),
            "admin"
        );
        assert_eq!(
            DashboardAuthLevel::Partner {
                twitch_login: "x".into(),
                twitch_user_id: "1".into(),
                display_name: "X".into()
            }
            .as_str(),
            "partner"
        );
        assert_eq!(DashboardAuthLevel::None.as_str(), "none");
    }

    #[test]
    fn is_admin_login_normalisiert() {
        assert!(is_admin_login(" earlysalty "));
        assert!(is_admin_login("EarlySalty"));
        assert!(!is_admin_login("someoneelse"));
    }

    fn local_parts(extra_header: Option<(&str, &str)>) -> Parts {
        use axum::extract::ConnectInfo;
        use std::net::SocketAddr;

        let mut builder = axum::http::Request::builder().header("host", "127.0.0.1:8769");
        if let Some((name, value)) = extra_header {
            builder = builder.header(name, value);
        }
        let mut req = builder.body(()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo("127.0.0.1:9999".parse::<SocketAddr>().unwrap()));
        req.into_parts().0
    }

    #[test]
    fn is_local_request_loopback_ohne_forwarding_header_true() {
        assert!(is_local_request(&local_parts(None)));
    }

    #[test]
    fn is_local_request_mit_forwarded_for_false() {
        assert!(!is_local_request(&local_parts(Some(("x-forwarded-for", "203.0.113.1")))));
    }

    #[test]
    fn is_local_request_mit_forwarded_host_false() {
        assert!(!is_local_request(&local_parts(Some(("x-forwarded-host", "dash.example.com")))));
    }

    #[test]
    fn is_privileged_korrekt() {
        assert!(DashboardAuthLevel::admin().is_privileged());
        assert!(!DashboardAuthLevel::Partner {
            twitch_login: "x".into(),
            twitch_user_id: "1".into(),
            display_name: "X".into()
        }
        .is_privileged());
        assert!(!DashboardAuthLevel::None.is_privileged());
    }

    async fn maybe_test_state() -> Option<(sqlx::PgPool, crate::auth::session::DashboardAuthState)> {
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("auth_level");
        let admin_pool = sqlx::PgPool::connect(&url).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin_pool)
            .await
            .ok()?;
        admin_pool.close().await;

        let opts: sqlx::postgres::PgConnectOptions = url.parse().ok()?;
        let opts = opts.options([("search_path", schema.as_str())]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .ok()?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id BIGINT PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                twitch_user_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                technical_pause_reason TEXT,
                admin_archived_at TEXT,
                departnered_at TEXT,
                partnered_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .ok()?;
        let state = crate::auth::session::DashboardAuthState::new(pool.clone(), "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string());
        Some((pool, state))
    }

    async fn ensure_partner(pool: &sqlx::PgPool, id: i64, login: &str, user_id: &str) {
        sqlx::query("DELETE FROM twitch_partners WHERE id = $1 OR twitch_login = $2 OR twitch_user_id = $3")
            .bind(id)
            .bind(login)
            .bind(user_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (id, twitch_login, twitch_user_id, status)
             VALUES ($1, $2, $3, 'active')
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(login)
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
    }

    fn request_parts(cookie: Option<String>) -> Parts {
        let mut builder = axum::http::Request::builder().header("host", "dash.example.com");
        if let Some(cookie) = cookie {
            builder = builder.header("cookie", cookie);
        }
        builder.body(()).unwrap().into_parts().0
    }

    async fn extract_auth(mut parts: Parts, state: crate::auth::session::DashboardAuthState) -> DashboardAuthLevel {
        parts.extensions.insert(state);
        DashboardAuthLevel::from_request_parts(&mut parts, &()).await.unwrap()
    }

    #[tokio::test]
    async fn twitch_admin_ohne_mode_cookie_bleibt_partner() {
        let Some((pool, state)) = maybe_test_state().await else { return; };
        ensure_partner(&pool, 9062301, "earlysalty", "9062301").await;
        let session = state.create_partner_session("earlysalty", "9062301", "EarlySalty").await.unwrap();
        let auth = extract_auth(request_parts(Some(format!("{}={}", crate::auth::session::PARTNER_COOKIE_NAME, session.session_id))), state.clone()).await;
        assert!(matches!(auth, DashboardAuthLevel::Partner { ref twitch_login, .. } if twitch_login == "earlysalty"));
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1").bind(&session.session_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM twitch_partners WHERE id = 9062301").execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn twitch_admin_mit_mode_cookie_wird_admin_actor() {
        let Some((pool, state)) = maybe_test_state().await else { return; };
        ensure_partner(&pool, 9062302, "earlysalty", "9062302").await;
        let session = state.create_partner_session("earlysalty", "9062302", "EarlySalty").await.unwrap();
        let auth = extract_auth(request_parts(Some(format!("{}={}; tb_admin_mode=2", crate::auth::session::PARTNER_COOKIE_NAME, session.session_id))), state.clone()).await;
        assert!(matches!(auth, DashboardAuthLevel::Admin { actor: Some(AdminActor { ref twitch_login, .. }) } if twitch_login == "earlysalty"));
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1").bind(&session.session_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM twitch_partners WHERE id = 9062302").execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn twitch_session_schlaegt_master_session() {
        let Some((pool, state)) = maybe_test_state().await else { return; };
        ensure_partner(&pool, 9062303, "earlysalty", "9062303").await;
        let partner = state.create_partner_session("earlysalty", "9062303", "EarlySalty").await.unwrap();
        let admin = state.create_admin_session("discord-9062303", "Discord Admin").await.unwrap();
        let auth = extract_auth(request_parts(Some(format!("{}={}; {}={}", crate::auth::session::PARTNER_COOKIE_NAME, partner.session_id, crate::auth::session::ADMIN_COOKIE_NAME, admin.session_id))), state.clone()).await;
        assert!(matches!(auth, DashboardAuthLevel::Partner { ref twitch_login, .. } if twitch_login == "earlysalty"));
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = ANY($1)").bind(&vec![partner.session_id, admin.session_id]).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM twitch_partners WHERE id = 9062303").execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn reine_master_session_wird_admin_none() {
        let Some((pool, state)) = maybe_test_state().await else { return; };
        let admin = state.create_admin_session("discord-9062304", "Discord Admin").await.unwrap();
        let auth = extract_auth(request_parts(Some(format!("{}={}", crate::auth::session::ADMIN_COOKIE_NAME, admin.session_id))), state.clone()).await;
        assert_eq!(auth, DashboardAuthLevel::admin());
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1").bind(&admin.session_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn loopback_ohne_state_bleibt_none() {
        let mut parts = local_parts(None);
        let auth = DashboardAuthLevel::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(auth, DashboardAuthLevel::None);
    }
}

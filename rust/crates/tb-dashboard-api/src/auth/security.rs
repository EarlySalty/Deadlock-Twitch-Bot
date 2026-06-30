//! Security-Layer-Primitive (F6) für das Dashboard.
//!
//! - **Login-Rate-Limiter** (`RateLimiter`) — atomares Sliding-Window gegen
//!   Brute-Force auf den OAuth-/Login-Flow. Port von Python
//!   `DashboardAuthRateLimitStore.allow_request` (state_store.py:370-427) +
//!   `sessions_db.reserve_rate_limit_slot` (sessions_db.py:252-310). Die Hit-Zähler
//!   liegen als kurzlebige Rows in `dashboard_sessions` (Typ `rate_limit:...`);
//!   die Atomarität kommt aus `pg_advisory_xact_lock` — ohne den Lock könnten
//!   zwei parallele Logins denselben Slot gleichzeitig reservieren (Race).
//!
//! - **Loopback-/Peer-Guard** (`require_internal`) — interner Zugriff nur von
//!   Loopback-IP UND mit gültigem Internal-Token (konstant-zeitlicher Vergleich).
//!   Härtung gegenüber dem reinen IP-Check der bestehenden `loopback_only`-
//!   Middleware: hinter einem Reverse-Proxy ist die Peer-IP immer Loopback, der
//!   Token bleibt dann die eigentliche Grenze.
//!
//! CSRF-Erzeugung/-Validierung liegt session-nah in [`super::session`]
//! (`create_*_session` setzt das Token, `validate_csrf` prüft es).

use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// Session-Typ der Rate-Limit-Hit-Rows (Python: state_store.py:20).
const RATE_LIMIT_SESSION_TYPE: &str = "rate_limit:dashboard_auth";

/// Default-Limit für den Login-/OAuth-Callback-Bucket: 10 Versuche pro 60 s
/// (Python: auth_mixin.py:1436/1641, partner_auth_mixin.py:109).
pub const LOGIN_MAX_REQUESTS: u32 = 10;
/// Default-Fenster des Login-Limiters in Sekunden.
pub const LOGIN_WINDOW_SECS: u64 = 60;

/// Atomarer Sliding-Window-Rate-Limiter, gestützt auf die geteilte
/// `dashboard_sessions`-Tabelle.
#[derive(Clone)]
pub struct RateLimiter {
    pool: PgPool,
    fernet_key: String,
}

impl RateLimiter {
    /// `fernet_key`: derselbe Key wie für die Session-Verschlüsselung — die
    /// Hit-Rows tragen einen (irrelevanten, aber konsistent verschlüsselten)
    /// Payload, damit das Spaltenformat identisch zu echten Sessions bleibt.
    pub fn new(pool: PgPool, fernet_key: String) -> Self {
        Self { pool, fernet_key }
    }

    /// Reserviert atomar einen Slot im Bucket für `key` (z. B. die Client-IP).
    ///
    /// Gibt `Ok(true)` zurück, wenn der Request erlaubt ist (Slot reserviert),
    /// `Ok(false)`, wenn das Limit im aktuellen Fenster erreicht ist.
    ///
    /// **Fail-open bei DB-Fehler** (Python-Parität, auth_mixin.py:641-650): Der
    /// Rate-Limiter ist Brute-Force-Dämpfung, kein hartes Sicherheits-Gate — fällt
    /// der Store aus, blockieren wir keinen legitimen Login. Der DB-Fehler wird als
    /// Warning geloggt (ohne Secrets) und der Request durchgelassen.
    pub async fn allow(&self, key: &str, max_requests: u32, window_secs: u64) -> bool {
        if max_requests == 0 {
            return false;
        }
        if window_secs == 0 {
            return true;
        }
        match self.try_reserve(key, max_requests, window_secs).await {
            Ok(allowed) => allowed,
            Err(error) => {
                tracing::warn!(%error, "Rate-Limit-Store nicht verfügbar — fail-open");
                true
            }
        }
    }

    /// Convenience für den Login-/OAuth-Bucket mit den Default-Limits.
    pub async fn allow_login(&self, key: &str) -> bool {
        self.allow(key, LOGIN_MAX_REQUESTS, LOGIN_WINDOW_SECS).await
    }

    async fn try_reserve(
        &self,
        key: &str,
        max_requests: u32,
        window_secs: u64,
    ) -> Result<bool, sqlx::Error> {
        let now = unix_now_f64();
        let bucket_prefix = bucket_prefix(key, window_secs);
        let (lock_a, lock_b) =
            advisory_lock_pair(&format!("{RATE_LIMIT_SESSION_TYPE}:{bucket_prefix}"));
        let session_id = hit_record_id(&bucket_prefix, now);
        let like_pattern = format!("{}%", escape_like(&bucket_prefix));
        let expires_at = now + window_secs as f64;

        // Payload ist bewusst inhaltsarm — nur Format-Parität zu echten Sessions.
        let payload = serde_json::json!({
            "seen_at": now,
            "window_seconds": window_secs as f64,
        });
        let token = fernet_encrypt(&self.fernet_key, &payload)?;

        let mut tx = self.pool.begin().await?;

        // Serialisiert konkurrierende Reservierungen desselben Buckets bis
        // Transaktionsende — verhindert die Count-then-Insert-Race.
        sqlx::query!("SELECT pg_advisory_xact_lock($1, $2)", lock_a, lock_b)
            .execute(&mut *tx)
            .await?;

        let active_hits: i64 = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM dashboard_sessions
            WHERE session_type = $1
              AND expires_at > $2
              AND session_id LIKE $3
            "#,
            RATE_LIMIT_SESSION_TYPE,
            now,
            like_pattern
        )
        .fetch_one(&mut *tx)
        .await?;

        if active_hits >= max_requests as i64 {
            // Lock wird beim Rollback freigegeben.
            tx.rollback().await?;
            return Ok(false);
        }

        sqlx::query!(
            r#"
            INSERT INTO dashboard_sessions
                (session_id, session_type, payload_enc, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (session_id) DO UPDATE SET
                payload_enc = EXCLUDED.payload_enc,
                expires_at  = EXCLUDED.expires_at
            "#,
            session_id,
            RATE_LIMIT_SESSION_TYPE,
            token.as_bytes(),
            now,
            expires_at
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(true)
    }
}

/// Bucket-Prefix `rl:{window}:{sha256_hex(key)}` (Python: state_store.py:421-423).
fn bucket_prefix(key: &str, window_secs: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    format!("rl:{window_secs}:{}", hex::encode(digest))
}

/// Eindeutige Hit-Record-ID `{prefix}:{ms}:{rand}` (Python: state_store.py:426-427).
fn hit_record_id(bucket_prefix: &str, now: f64) -> String {
    let ms = (now * 1000.0) as i64;
    let suffix = tb_crypto::random_urlsafe_token(6);
    format!("{bucket_prefix}:{ms}:{suffix}")
}

/// Advisory-Lock-Paar aus SHA-256 — byte-identisch zu Python
/// `sessions_db._advisory_lock_pair` (digest[:4]/[4:8], big-endian **signed** i32).
/// Garantiert, dass Python- und Rust-Login auf DEMSELBEN Advisory-Lock serialisieren.
fn advisory_lock_pair(value: &str) -> (i32, i32) {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let a = i32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    let b = i32::from_be_bytes([digest[4], digest[5], digest[6], digest[7]]);
    (a, b)
}

/// Escaped LIKE-Sonderzeichen (Python: sessions_db._escape_like) — verhindert,
/// dass `%`/`_` im Bucket-Hash als Wildcard wirken.
fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Verschlüsselt einen Rate-Limit-Payload (gibt `sqlx::Error` zurück, damit der
/// Aufrufer nur einen Fehlertyp behandeln muss). Nachricht enthält keine Secrets.
fn fernet_encrypt(key: &str, payload: &serde_json::Value) -> Result<String, sqlx::Error> {
    super::fernet::encrypt(key, payload.to_string().as_bytes())
        .map_err(|e| sqlx::Error::Encode(Box::new(RateLimitEncryptError(e.to_string()))))
}

#[derive(Debug)]
struct RateLimitEncryptError(String);

impl std::fmt::Display for RateLimitEncryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Rate-Limit-Payload-Verschlüsselung fehlgeschlagen: {}",
            self.0
        )
    }
}

impl std::error::Error for RateLimitEncryptError {}

fn unix_now_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ───────────────────────────────────────────────────────────────────────────
// Loopback-/Peer-Guard
// ───────────────────────────────────────────────────────────────────────────

/// Prüft, ob ein interner Request zugelassen wird: Peer-IP ist Loopback UND der
/// präsentierte Token stimmt konstant-zeitlich mit dem erwarteten überein.
///
/// Beide Bedingungen müssen erfüllt sein:
/// - **Loopback-Peer**: blockt Zugriffe, die nicht von der lokalen Maschine
///   stammen (Defense-in-Depth; hinter einem Proxy stets erfüllt, deshalb nicht
///   allein tragfähig — daher zusätzlich der Token).
/// - **Token-Match**: konstant-zeitlicher Vergleich (`tb_crypto::constant_time_eq`,
///   Pendant zu Pythons `hmac.compare_digest`) verhindert Timing-Seitenkanäle.
///   Leerer erwarteter Token → immer `false` (fail-closed): ohne konfiguriertes
///   Secret darf der interne Zugriff nie offenstehen.
pub fn require_internal(
    peer_is_loopback: bool,
    presented_token: &str,
    expected_token: &str,
) -> bool {
    if expected_token.is_empty() {
        return false;
    }
    if !peer_is_loopback {
        return false;
    }
    tb_crypto::constant_time_eq(presented_token.as_bytes(), expected_token.as_bytes())
}

// ───────────────────────────────────────────────────────────────────────────
// Rate-Limit-Middleware (P2.86 / P2.133 / P2.138 / P2.140)
// ───────────────────────────────────────────────────────────────────────────
//
// Wiederverwendbare axum-Middleware, die den [`RateLimiter`] pro Client-IP auf
// eine Route(ngruppe) anwendet. Die LAYER-Registrierung auf die konkreten Router
// passiert zentral in `lib.rs` (WIRING-TODO) — hier liegt nur die Mechanik.

use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::net::SocketAddr;

/// Konfiguration eines Rate-Limit-Layers: logischer Bucket-Name + Limits.
#[derive(Clone)]
pub struct RateLimitLayerConfig {
    limiter: RateLimiter,
    /// Stabiler Bucket-Name (z. B. `"auth_login"`), geht in den Bucket-Key ein,
    /// damit verschiedene Routen-Gruppen getrennte Kontingente haben.
    bucket: &'static str,
    max_requests: u32,
    window_secs: u64,
}

impl RateLimitLayerConfig {
    pub fn new(
        limiter: RateLimiter,
        bucket: &'static str,
        max_requests: u32,
        window_secs: u64,
    ) -> Self {
        Self {
            limiter,
            bucket,
            max_requests,
            window_secs,
        }
    }
}

/// Ermittelt den Rate-Limit-Schlüssel (Client-IP) aus dem Request.
///
/// Bevorzugt die echte Peer-IP (`ConnectInfo`); hinter dem Reverse-Proxy ist die
/// immer Loopback, daher fällt es auf den **ersten** `X-Forwarded-For`-Eintrag
/// zurück (der vom vertrauenswürdigen Proxy gesetzt wird). Ohne beides → `"unknown"`.
fn client_key(request: &Request) -> String {
    if let Some(ci) = request
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
    {
        let ip = ci.0.ip();
        if !ip.is_loopback() {
            return ip.to_string();
        }
    }
    if let Some(xff) = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            let first = first.trim();
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    // Loopback-Peer ohne XFF (lokale Tools) → eigener Bucket.
    "loopback".to_string()
}

/// axum-Middleware-Funktion: erzwingt das Rate-Limit pro IP. Über das Limit →
/// `429 {error:rate_limit_exceeded}` + `Retry-After`-Header.
pub async fn rate_limit_middleware(
    axum::extract::State(config): axum::extract::State<RateLimitLayerConfig>,
    request: Request,
    next: Next,
) -> Response {
    let key = format!("{}:{}", config.bucket, client_key(&request));
    let allowed = config
        .limiter
        .allow(&key, config.max_requests, config.window_secs)
        .await;
    if allowed {
        next.run(request).await
    } else {
        too_many_requests(config.window_secs)
    }
}

/// 429-Antwort mit `Retry-After` (Sekunden bis zum Fensterende, konservativ das
/// volle Fenster).
fn too_many_requests(window_secs: u64) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": "rate_limit_exceeded",
            "message": "Zu viele Anfragen. Bitte später erneut versuchen.",
        })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&window_secs.to_string()) {
        response
            .headers_mut()
            .insert(axum::http::header::RETRY_AFTER, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_lock_pair_deterministisch() {
        assert_eq!(
            advisory_lock_pair("rl:60:abc"),
            advisory_lock_pair("rl:60:abc")
        );
        assert_ne!(
            advisory_lock_pair("rl:60:abc"),
            advisory_lock_pair("rl:60:abd")
        );
    }

    #[test]
    fn advisory_lock_pair_byte_identisch_zu_python() {
        // Referenz: hashlib.sha256(b"x").digest(), [:4]/[4:8] big-endian signed.
        // python3 -c "import hashlib,struct;d=hashlib.sha256(b'x').digest();
        //   print(struct.unpack('>i',d[:4])[0],struct.unpack('>i',d[4:8])[0])"
        // → 762385986 -1222201276  (Cross-Repo-Serialisierung mit Python-Login)
        assert_eq!(advisory_lock_pair("x"), (762385986, -1222201276));
    }

    #[test]
    fn bucket_prefix_format() {
        let p = bucket_prefix("1.2.3.4", 60);
        assert!(p.starts_with("rl:60:"));
        // sha256-hex = 64 Zeichen, + "rl:60:" Prefix.
        assert_eq!(p.len(), "rl:60:".len() + 64);
    }

    #[test]
    fn escape_like_neutralisiert_wildcards() {
        assert_eq!(escape_like("a%b_c\\d"), "a\\%b\\_c\\\\d");
    }

    #[test]
    fn hit_record_id_eindeutig() {
        let p = bucket_prefix("ip", 60);
        let a = hit_record_id(&p, 1.0);
        let b = hit_record_id(&p, 1.0);
        assert!(a.starts_with(&p));
        assert_ne!(a, b, "Zufalls-Suffix macht IDs eindeutig");
    }

    #[test]
    fn require_internal_loopback_und_token() {
        // Glücksfall: Loopback + korrektes Token.
        assert!(require_internal(true, "geheim", "geheim"));
    }

    #[test]
    fn require_internal_falsches_token_abgelehnt() {
        assert!(!require_internal(true, "falsch", "geheim"));
    }

    #[test]
    fn require_internal_fremder_peer_abgelehnt() {
        // Korrektes Token, aber nicht-Loopback-Peer → abgelehnt.
        assert!(!require_internal(false, "geheim", "geheim"));
    }

    #[test]
    fn require_internal_leerer_erwarteter_token_fail_closed() {
        assert!(!require_internal(true, "", ""));
        assert!(!require_internal(true, "irgendwas", ""));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        sqlx::PgPool::connect(&url).await.ok()
    }

    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    async fn ensure_table(pool: &PgPool) {
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
        .execute(pool)
        .await
        .unwrap();
    }

    /// N Requests im Fenster erlaubt, N+1 blockiert (Brute-Force-Schutz).
    #[tokio::test]
    async fn rate_limiter_blockt_nach_limit() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_table(&pool).await;

        let limiter = RateLimiter::new(pool.clone(), test_fernet_key());
        // Eindeutiger Key, damit parallele Testläufe sich nicht stören.
        let key = format!("ratelimit-test-{}", unix_now_f64());
        let max = 3u32;
        let window = 60u64;

        // Vorherige Hits dieses Buckets aufräumen (idempotenter Testlauf).
        let prefix = bucket_prefix(&key, window);
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id LIKE $1")
            .bind(format!("{}%", super::escape_like(&prefix)))
            .execute(&pool)
            .await
            .ok();

        // Erste `max` Requests erlaubt.
        for i in 0..max {
            assert!(
                limiter.allow(&key, max, window).await,
                "Request {i} muss im Limit erlaubt sein"
            );
        }
        // N+1 blockiert.
        assert!(
            !limiter.allow(&key, max, window).await,
            "Request über dem Limit muss blockiert werden"
        );

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id LIKE $1")
            .bind(format!("{}%", super::escape_like(&prefix)))
            .execute(&pool)
            .await
            .ok();
    }

    /// P2.86/133/138/140: Die Rate-Limit-Middleware liefert nach dem Limit 429
    /// mit Retry-After. Self-contained Router (keine lib.rs-Verdrahtung).
    #[tokio::test]
    async fn rate_limit_middleware_429_nach_limit() {
        use axum::{routing::get, Router};
        use tower::ServiceExt;

        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_table(&pool).await;
        let limiter = RateLimiter::new(pool.clone(), test_fernet_key());
        let config = RateLimitLayerConfig::new(limiter, "test_bucket", 2, 60);

        let app = Router::new().route("/x", get(|| async { "ok" })).layer(
            axum::middleware::from_fn_with_state(config, super::rate_limit_middleware),
        );

        let make_req = || {
            let mut req = axum::http::Request::builder()
                .uri("/x")
                .body(axum::body::Body::empty())
                .unwrap();
            // Nicht-Loopback-Peer, damit der IP-Bucket greift.
            req.extensions_mut().insert(axum::extract::ConnectInfo(
                "203.0.113.7:5555".parse::<SocketAddr>().unwrap(),
            ));
            req
        };

        // max=2: erste zwei OK, dritte 429.
        assert_eq!(
            app.clone().oneshot(make_req()).await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(
            app.clone().oneshot(make_req()).await.unwrap().status(),
            StatusCode::OK
        );
        let limited = app.clone().oneshot(make_req()).await.unwrap();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(limited
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .is_some());
    }

    /// Abgelaufene Hits zählen nicht mehr → Slot wird wieder frei.
    #[tokio::test]
    async fn rate_limiter_fenster_gleitet() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_table(&pool).await;

        let limiter = RateLimiter::new(pool.clone(), test_fernet_key());
        let key = format!("ratelimit-slide-{}", unix_now_f64());
        let prefix = bucket_prefix(&key, 60);

        // Einen bereits abgelaufenen Hit einschleusen.
        let now = unix_now_f64();
        sqlx::query(
            "INSERT INTO dashboard_sessions (session_id, session_type, payload_enc, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(format!("{prefix}:old:xxxx"))
        .bind(RATE_LIMIT_SESSION_TYPE)
        .bind(b"x".to_vec())
        .bind(now - 120.0)
        .bind(now - 1.0) // abgelaufen
        .execute(&pool)
        .await
        .unwrap();

        // max=1: trotz des abgelaufenen Hits ist genau ein frischer erlaubt.
        assert!(limiter.allow(&key, 1, 60).await);
        // Zweiter frischer Hit blockiert.
        assert!(!limiter.allow(&key, 1, 60).await);

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id LIKE $1")
            .bind(format!("{}%", super::escape_like(&prefix)))
            .execute(&pool)
            .await
            .ok();
    }
}

//! Security-Härtung der internen API (Block 10).
//!
//! Bündelt die sicherheitskritischen Quervergleiche zum Python-Original
//! (`bot/internal_api/policy.py`, `bot/runtime_mode.py`) an EINER Stelle, statt
//! sie über Middleware und Handler zu streuen:
//!
//! 1. **Loopback-Guard** — Parität zu `_is_loopback_request` (`app.py:453`):
//!    *sowohl* der `Origin`-Header (falls gesetzt) *als auch* die Peer-IP müssen
//!    Loopback sein. `tb_http_core::loopback_only` prüft nur die Peer-IP; für die
//!    interne API ersetzt [`internal_api_loopback_guard`] das durch den
//!    vollständigen Python-Vertrag. (Die generische tb-http-core-Middleware bleibt
//!    unverändert, da sie auch das Dashboard bedient.)
//! 2. **Token-Vergleich** — Parität zu `compare_internal_token` (`policy.py:30`):
//!    beide Seiten getrimmt, leere Werte → `false`, sonst konstant-zeitlich.
//! 3. **JSON-Serialisierungs-Parität** — Parität zu `json_default` (`policy.py:18`):
//!    `datetime → isoformat`, `Decimal → float`, `UUID → str`, `set → list`.
//! 4. **Split-Deployment-Härtung** — Parität zu `enforce_internal_api_runtime`
//!    (`runtime_mode.py`): Start verweigern, wenn die Runtime-Rolle nicht
//!    `twitch_worker` ist oder der Port nicht der erwartete (8776 bzw. Legacy).

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, State},
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use tb_http_core::{ApiError, INTERNAL_TOKEN_HEADER};

// ─────────────────────────────────────────────────────────────────────────────
// 1. Loopback-Guard (Origin + Peer)
// ─────────────────────────────────────────────────────────────────────────────

/// Extrahiert den Host aus einem rohen Host[:Port]-Wert — Parität zu
/// `host_without_port` (`policy.py:41`).
///
/// - Trimmt, nimmt den ersten Komma-getrennten Eintrag (Proxy-Listen),
/// - klammert `[::1]`-IPv6-Literale aus,
/// - lowercased und entfernt einen evtl. trailing `.`,
/// - akzeptiert nackte IP-Adressen unverändert,
/// - schneidet ein angehängtes `:port` nur ab, wenn genau EIN `:` vorliegt
///   (sonst wäre es eine IPv6-Adresse ohne Klammern → unverändert lassen).
pub fn host_without_port(raw: Option<&str>) -> String {
    let value = raw.unwrap_or("").trim();
    if value.is_empty() {
        return String::new();
    }
    let host = value.split(',').next().unwrap_or("").trim();
    if host.is_empty() {
        return String::new();
    }

    // `[::1]` / `[::1]:8776` → Inhalt zwischen den Klammern.
    if let Some(stripped) = host.strip_prefix('[') {
        if let Some(end) = stripped.find(']') {
            return stripped[..end].to_lowercase().trim_end_matches('.').to_string();
        }
        return stripped.to_lowercase().trim_end_matches('.').to_string();
    }

    let normalized = host.to_lowercase();
    let normalized = normalized.trim_end_matches('.');
    if normalized.is_empty() {
        return String::new();
    }

    // Nackte IP-Adresse (IPv4/IPv6) → unverändert übernehmen.
    if normalized.parse::<IpAddr>().is_ok() {
        return normalized.to_string();
    }

    // Genau ein `:` → host:port; nur abschneiden wenn der Port numerisch ist.
    if normalized.matches(':').count() == 1 {
        if let Some((host_part, port_part)) = normalized.rsplit_once(':') {
            if !host_part.is_empty() && !port_part.is_empty() && port_part.bytes().all(|b| b.is_ascii_digit()) {
                return host_part.to_string();
            }
        }
    }
    normalized.to_string()
}

/// Prüft, ob ein roher Host[:Port]-Wert auf Loopback zeigt — Parität zu
/// `is_loopback_host` (`policy.py:71`).
///
/// `localhost` zählt als Loopback; ansonsten muss der Host eine IP-Adresse mit
/// gesetztem Loopback-Flag sein (`127.0.0.0/8`, `::1`).
pub fn is_loopback_host(raw: Option<&str>) -> bool {
    let host = host_without_port(raw);
    if host.is_empty() {
        return false;
    }
    if host == "localhost" {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(addr) => addr.is_loopback(),
        Err(_) => false,
    }
}

/// Prüft den `Origin`-Header — Parität zu `is_loopback_origin` (`policy.py:83`).
///
/// - Fehlend/leer → `true` (kein Origin = same-origin/loopback-Tooling),
/// - Schema muss `http`/`https` sein und ein Host vorhanden,
/// - eingebettete Credentials (`user:pass@`) → `false`,
/// - der Host muss Loopback sein.
pub fn is_loopback_origin(raw_origin: Option<&str>) -> bool {
    let origin = raw_origin.unwrap_or("").trim();
    if origin.is_empty() {
        return true;
    }
    let parsed = match url::Url::parse(origin) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    if parsed.host().is_none() {
        return false;
    }
    // urlsplit().netloc ist in Python leer, wenn kein `//authority` vorliegt;
    // url::Url verlangt bei http/https ohnehin eine Authority, daher genügt der
    // Host-Check oben.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    is_loopback_host(parsed.host_str())
}

/// Vollständiger Loopback-Request-Check — Parität zu `_is_loopback_request`
/// (`app.py:453`): `is_loopback_origin(Origin) && is_loopback_host(peer)`.
pub fn is_loopback_request(origin: Option<&str>, peer_ip: IpAddr) -> bool {
    is_loopback_origin(origin) && peer_ip.is_loopback()
}

/// Axum-Middleware der internen API: erzwingt den vollständigen Python-
/// Loopback-Vertrag (`Origin`-Header **und** Peer-IP), nicht nur die Peer-IP.
///
/// Ersetzt `tb_http_core::loopback_only` im internen Router; muss vor der
/// Auth-Prüfung stehen. Fremder/ungültiger Origin → 403, exakt wie Pythons
/// `_loopback_middleware` (`app.py:1006`).
pub async fn internal_api_loopback_guard(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let origin = req
        .headers()
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    if !is_loopback_request(origin, addr.ip()) {
        return ApiError::forbidden().into_response();
    }
    next.run(req).await
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Token-Vergleich (trim + constant-time)
// ─────────────────────────────────────────────────────────────────────────────

/// Vergleicht präsentiertes und erwartetes Internal-Token — Parität zu
/// `compare_internal_token` (`policy.py:30`).
///
/// Beide Seiten werden getrimmt; ist eine der beiden danach leer → `false`.
/// Der eigentliche Vergleich ist konstant-zeitlich (kein Early-Return über den
/// Inhalt), damit Timing keine Token-Bytes verrät.
pub fn compare_internal_token(presented: Option<&str>, expected: Option<&str>) -> bool {
    let presented = presented.unwrap_or("").trim();
    let expected = expected.unwrap_or("").trim();
    if presented.is_empty() || expected.is_empty() {
        return false;
    }
    constant_time_eq(presented.as_bytes(), expected.as_bytes())
}

/// Axum-Middleware der internen API: prüft den `X-Internal-Token`-Header gegen
/// das konfigurierte Token via [`compare_internal_token`] (beide Seiten
/// getrimmt, leer → 401, sonst konstant-zeitlich).
///
/// Ersetzt `tb_http_core::internal_auth` im internen Router. Leeres
/// konfiguriertes Token → fail-closed (immer 401), da `compare_internal_token`
/// einen leeren erwarteten Wert ablehnt.
pub async fn internal_api_auth_guard(
    State(expected_token): State<String>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let provided = req
        .headers()
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok());
    if !compare_internal_token(provided, Some(&expected_token)) {
        return ApiError::unauthorized().into_response();
    }
    next.run(req).await
}

/// Konstant-zeitlicher Byte-Vergleich (verhindert Timing-Angriffe).
///
/// Bei Längenunterschied `false` — die Tokenlänge ist für einen Angreifer keine
/// geheime Information. Bei gleicher Länge läuft die Schleife immer komplett
/// durch, unabhängig von der Position eines Mismatches.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. JSON-Serialisierungs-Parität (json_default)
// ─────────────────────────────────────────────────────────────────────────────

/// Serialisiert ein `DateTime<Utc>` exakt wie Pythons `datetime.isoformat()`.
///
/// - Zeitzone immer als `+00:00` (nicht `Z`),
/// - Mikrosekunden nur, wenn != 0 (Python lässt `.000000` weg).
///
/// Kanonischer Ersatz für die früher dreifach duplizierten Helfer
/// (`session_detail::ts_to_iso`, `streamer_analytics_native::format_python_isoformat`,
/// verstreute `to_rfc3339()`-Aufrufe — letztere wichen ab, da `to_rfc3339`
/// stets Nanosekunden-Präzision anhängt). Referenz: `json_default` (`policy.py:18`).
pub fn datetime_to_iso(dt: DateTime<Utc>) -> String {
    let micros = dt.timestamp_subsec_micros();
    if micros == 0 {
        dt.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        format!("{}.{:06}+00:00", dt.format("%Y-%m-%dT%H:%M:%S"), micros)
    }
}

/// Python `Decimal → float`: serialisiert als JSON-Zahl (`policy.py:21`).
///
/// Nicht-endliche Werte (NaN/Inf) sind in JSON nicht darstellbar → `Null`,
/// statt zu panicen.
pub fn decimal_to_json(value: f64) -> serde_json::Value {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

/// Python `UUID → str`: serialisiert als JSON-String (`policy.py:23`).
pub fn uuid_to_json(value: uuid::Uuid) -> serde_json::Value {
    serde_json::Value::String(value.to_string())
}

/// Python `set → list`: serialisiert eine Menge als JSON-Array (`policy.py:25`).
///
/// Reihenfolge ist in Python (und für eine `HashSet`) nicht definiert; für
/// stabile Antworten erwartet der Aufrufer ggf. eine sortierte Eingabe.
pub fn set_to_json<I, T>(values: I) -> serde_json::Value
where
    I: IntoIterator<Item = T>,
    T: Into<serde_json::Value>,
{
    serde_json::Value::Array(values.into_iter().map(Into::into).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Split-Deployment-Härtung (Runtime-Rolle + Port)
// ─────────────────────────────────────────────────────────────────────────────

/// Erwartete Runtime-Rolle der internen API (`runtime_mode.ROLE_TWITCH_WORKER`).
pub const ROLE_TWITCH_WORKER: &str = "twitch_worker";
/// Default-Port der internen API (`runtime_mode.INTERNAL_API_PORT`).
pub const INTERNAL_API_PORT: u16 = 8776;
/// Für die Master-API reservierter Port (`runtime_mode.MASTER_API_RESERVED_PORT`).
pub const MASTER_API_RESERVED_PORT: u16 = 8766;

/// Fehlkonfigurations-Fehler beim API-Start (Split-Deployment-Härtung).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHardeningError(pub String);

impl std::fmt::Display for RuntimeHardeningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeHardeningError {}

/// Normalisiert eine rohe Rollenangabe — Parität zu `resolve_runtime_role`
/// (`runtime_mode.py:45`): trim, lowercase, `-`→`_`, Aliase auf `twitch_worker`.
fn resolve_runtime_role(raw: Option<&str>) -> String {
    let normalized = raw.unwrap_or("").trim().to_lowercase().replace('-', "_");
    match normalized.as_str() {
        "bot" | "worker" | "twitch_worker" => ROLE_TWITCH_WORKER.to_string(),
        other => other.to_string(),
    }
}

/// Liest die Runtime-Rolle aus der Umgebung (`TWITCH_RUNTIME_ROLE`, Fallback
/// `TWITCH_SPLIT_RUNTIME_ROLE`) und normalisiert sie.
fn runtime_role_from_env() -> String {
    let raw = nonempty_env("TWITCH_RUNTIME_ROLE")
        .or_else(|| nonempty_env("TWITCH_SPLIT_RUNTIME_ROLE"));
    resolve_runtime_role(raw.as_deref())
}

/// Ob die Split-Runtime-Härtung aktiv ist — Parität zu `split_runtime_enforced`
/// (`runtime_mode.py:38`): `TWITCH_RUNTIME_ENFORCE` hat Vorrang, sonst
/// `TWITCH_SPLIT_RUNTIME_ENFORCE`; Default ist aktiv (`true`).
fn split_runtime_enforced() -> bool {
    if let Some(raw) = nonempty_env("TWITCH_RUNTIME_ENFORCE") {
        return parse_env_bool(&raw, true);
    }
    if let Some(raw) = nonempty_env("TWITCH_SPLIT_RUNTIME_ENFORCE") {
        return parse_env_bool(&raw, true);
    }
    true
}

/// Optionaler Legacy-Port während des Rust-Takeovers — Parität zu
/// `legacy_internal_api_port` (`runtime_mode.py:130`): aus
/// `TWITCH_INTERNAL_API_LEGACY_PORT`; <=0 oder der reservierte Master-Port
/// werden verworfen.
fn legacy_internal_api_port() -> Option<u16> {
    let raw = nonempty_env("TWITCH_INTERNAL_API_LEGACY_PORT")?;
    let port: i64 = raw.trim().parse().ok()?;
    if port <= 0 || port == i64::from(MASTER_API_RESERVED_PORT) {
        return None;
    }
    u16::try_from(port).ok()
}

/// Verweigert den Start bei Fehlkonfiguration — Parität zu
/// `enforce_internal_api_runtime` (`runtime_mode.py:147`).
///
/// Bei aktiver Härtung muss die Rolle `twitch_worker` sein und der Port der
/// erwartete (8776, oder ein konfigurierter Legacy-Port). Ist die Härtung
/// deaktiviert (`TWITCH_RUNTIME_ENFORCE=0`), wird nur die aufgelöste Rolle
/// zurückgegeben.
///
/// `role = None` liest aus der Umgebung; expliziter Wert (Tests) hat Vorrang.
pub fn enforce_internal_api_runtime(
    role: Option<&str>,
    port: u16,
) -> Result<String, RuntimeHardeningError> {
    let resolved_role = match role {
        Some(value) => resolve_runtime_role(Some(value)),
        None => runtime_role_from_env(),
    };

    if !split_runtime_enforced() {
        return Ok(resolved_role);
    }

    let mut expected_port = INTERNAL_API_PORT;
    if let Some(legacy) = legacy_internal_api_port() {
        if port == legacy {
            expected_port = legacy;
        }
    }

    if resolved_role != ROLE_TWITCH_WORKER {
        return Err(RuntimeHardeningError(role_error_message(&resolved_role)));
    }

    if port != expected_port {
        return Err(RuntimeHardeningError(port_error_message(expected_port, port)));
    }

    Ok(resolved_role)
}

/// Fehlertext für eine falsche/fehlende Rolle — Parität zu `_role_error_message`.
fn role_error_message(got_role: &str) -> String {
    const ALLOWED: [&str; 3] = ["master", "twitch_worker", "dashboard"];
    if got_role.is_empty() {
        return format!(
            "Runtime hardening violation for internal_api: runtime role is missing. \
             Set TWITCH_RUNTIME_ROLE={ROLE_TWITCH_WORKER} \
             (or TWITCH_SPLIT_RUNTIME_ROLE={ROLE_TWITCH_WORKER})."
        );
    }
    if !ALLOWED.contains(&got_role) {
        return format!(
            "Runtime hardening violation for internal_api: unsupported runtime role '{got_role}'. \
             Allowed roles: master, twitch_worker, dashboard."
        );
    }
    format!(
        "Runtime hardening violation for internal_api: expected role '{ROLE_TWITCH_WORKER}', \
         got '{got_role}'."
    )
}

/// Fehlertext für einen falschen Port — Parität zu `_port_error_message`.
fn port_error_message(expected_port: u16, got_port: u16) -> String {
    if got_port == MASTER_API_RESERVED_PORT {
        return format!(
            "Runtime hardening violation for internal_api: port {MASTER_API_RESERVED_PORT} \
             is reserved for the master API service."
        );
    }
    format!(
        "Runtime hardening violation for internal_api: expected port {expected_port}, \
         got {got_port}."
    )
}

/// Liest eine Env-Variable und gibt `None` zurück, wenn sie fehlt oder (nach
/// Trim) leer ist.
fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// Parst einen Env-Bool — Parität zu `_parse_env_bool` (`runtime_mode.py:28`).
fn parse_env_bool(raw: &str, default: bool) -> bool {
    match raw.trim().to_lowercase().as_str() {
        "" => default,
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ── 1. Loopback-Guard ────────────────────────────────────────────────────

    #[test]
    fn host_without_port_strips_numeric_port() {
        assert_eq!(host_without_port(Some("localhost:8776")), "localhost");
        assert_eq!(host_without_port(Some("Example.COM.")), "example.com");
        assert_eq!(host_without_port(Some("127.0.0.1")), "127.0.0.1");
    }

    #[test]
    fn host_without_port_handles_ipv6_brackets() {
        assert_eq!(host_without_port(Some("[::1]:8776")), "::1");
        assert_eq!(host_without_port(Some("[::1]")), "::1");
        // Nackte IPv6 (mehrere `:`) bleibt unverändert — kein Port-Split.
        assert_eq!(host_without_port(Some("::1")), "::1");
    }

    #[test]
    fn host_without_port_takes_first_proxy_entry() {
        assert_eq!(host_without_port(Some("127.0.0.1, 10.0.0.1")), "127.0.0.1");
        assert_eq!(host_without_port(Some("")), "");
        assert_eq!(host_without_port(None), "");
    }

    #[test]
    fn is_loopback_host_accepts_localhost_and_loopback_ips() {
        assert!(is_loopback_host(Some("localhost")));
        assert!(is_loopback_host(Some("127.0.0.1")));
        assert!(is_loopback_host(Some("127.5.6.7")));
        assert!(is_loopback_host(Some("[::1]")));
        assert!(is_loopback_host(Some("localhost:8776")));
    }

    #[test]
    fn is_loopback_host_rejects_external() {
        assert!(!is_loopback_host(Some("10.0.0.1")));
        assert!(!is_loopback_host(Some("evil.example.com")));
        assert!(!is_loopback_host(Some("")));
        assert!(!is_loopback_host(None));
    }

    #[test]
    fn is_loopback_origin_empty_is_allowed() {
        assert!(is_loopback_origin(None));
        assert!(is_loopback_origin(Some("")));
        assert!(is_loopback_origin(Some("   ")));
    }

    #[test]
    fn is_loopback_origin_accepts_loopback_origins() {
        assert!(is_loopback_origin(Some("http://localhost")));
        assert!(is_loopback_origin(Some("http://127.0.0.1:8776")));
        assert!(is_loopback_origin(Some("https://localhost:3000")));
        assert!(is_loopback_origin(Some("http://[::1]:8080")));
    }

    #[test]
    fn is_loopback_origin_rejects_foreign_origin() {
        // Kern der Security-Lücke: fremder Origin trotz Loopback-Peer.
        assert!(!is_loopback_origin(Some("https://evil.example.com")));
        assert!(!is_loopback_origin(Some("http://10.0.0.1")));
    }

    #[test]
    fn is_loopback_origin_rejects_bad_scheme_and_credentials() {
        assert!(!is_loopback_origin(Some("ftp://localhost")));
        assert!(!is_loopback_origin(Some("file:///etc/passwd")));
        // Eingebettete Credentials → abgelehnt (Python prüft username/password).
        assert!(!is_loopback_origin(Some("http://user:pass@localhost")));
        assert!(!is_loopback_origin(Some("not a url")));
    }

    #[test]
    fn is_loopback_origin_rejects_empty_host() {
        // Python: urlsplit("http:///path").netloc == "" → abgelehnt.
        assert!(!is_loopback_origin(Some("http:///path")));
        assert!(!is_loopback_origin(Some("http://")));
    }

    #[test]
    fn is_loopback_origin_normalizes_case_and_trailing_dot() {
        // Python lowercased Scheme + Host via urlsplit/host_without_port.
        assert!(is_loopback_origin(Some("HTTP://LOCALHOST")));
        assert!(is_loopback_origin(Some("http://localhost.")));
        assert!(is_loopback_origin(Some("  http://localhost  ")));
    }

    #[test]
    fn is_loopback_request_requires_both_origin_and_peer() {
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        let external: IpAddr = "10.0.0.1".parse().unwrap();
        // Beide loopback → ok.
        assert!(is_loopback_request(Some("http://localhost"), loopback));
        assert!(is_loopback_request(None, loopback));
        // Fremder Origin trotz Loopback-Peer → abgelehnt.
        assert!(!is_loopback_request(Some("https://evil.example.com"), loopback));
        // Loopback-Origin aber externer Peer → abgelehnt.
        assert!(!is_loopback_request(Some("http://localhost"), external));
    }

    // ── 2. Token-Vergleich ───────────────────────────────────────────────────

    #[test]
    fn token_compare_trims_both_sides() {
        assert!(compare_internal_token(Some("  secret  "), Some("secret")));
        assert!(compare_internal_token(Some("secret"), Some("\tsecret\n")));
        assert!(compare_internal_token(Some(" abc "), Some(" abc ")));
    }

    #[test]
    fn token_compare_empty_is_false() {
        assert!(!compare_internal_token(Some(""), Some("secret")));
        assert!(!compare_internal_token(Some("   "), Some("secret")));
        assert!(!compare_internal_token(Some("secret"), Some("")));
        assert!(!compare_internal_token(Some("secret"), None));
        assert!(!compare_internal_token(None, Some("secret")));
        assert!(!compare_internal_token(None, None));
    }

    #[test]
    fn token_compare_mismatch_is_false() {
        assert!(!compare_internal_token(Some("secret"), Some("wrong")));
        assert!(!compare_internal_token(Some("secret"), Some("secret2")));
    }

    // ── 3. JSON-Serialisierungs-Parität ──────────────────────────────────────

    #[test]
    fn datetime_iso_omits_zero_micros() {
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 14, 30, 0).unwrap();
        assert_eq!(datetime_to_iso(dt), "2026-06-12T14:30:00+00:00");
    }

    #[test]
    fn datetime_iso_keeps_nonzero_micros() {
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 14, 30, 0).unwrap()
            + chrono::Duration::microseconds(123_456);
        assert_eq!(datetime_to_iso(dt), "2026-06-12T14:30:00.123456+00:00");
    }

    #[test]
    fn decimal_maps_to_json_number() {
        assert_eq!(decimal_to_json(1.5), serde_json::json!(1.5));
        assert_eq!(decimal_to_json(0.0), serde_json::json!(0.0));
        // Nicht-endlich → Null statt Panic.
        assert_eq!(decimal_to_json(f64::NAN), serde_json::Value::Null);
    }

    #[test]
    fn uuid_maps_to_json_string() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            uuid_to_json(id),
            serde_json::json!("550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn set_maps_to_json_array() {
        let mut sorted: Vec<i64> = vec![3, 1, 2];
        sorted.sort_unstable();
        assert_eq!(set_to_json(sorted), serde_json::json!([1, 2, 3]));
    }

    // ── 4. Split-Deployment-Härtung ──────────────────────────────────────────

    #[test]
    fn runtime_ok_for_worker_on_default_port() {
        let role = enforce_internal_api_runtime(Some("twitch_worker"), INTERNAL_API_PORT)
            .expect("worker on 8776 must be accepted");
        assert_eq!(role, ROLE_TWITCH_WORKER);
    }

    #[test]
    fn runtime_role_aliases_resolve_to_worker() {
        assert!(enforce_internal_api_runtime(Some("bot"), INTERNAL_API_PORT).is_ok());
        assert!(enforce_internal_api_runtime(Some("twitch-worker"), INTERNAL_API_PORT).is_ok());
        assert!(enforce_internal_api_runtime(Some("WORKER"), INTERNAL_API_PORT).is_ok());
    }

    #[test]
    fn runtime_rejects_wrong_role() {
        let err = enforce_internal_api_runtime(Some("master"), INTERNAL_API_PORT)
            .expect_err("master role must be rejected");
        assert!(err.0.contains("expected role 'twitch_worker'"));
    }

    #[test]
    fn runtime_rejects_missing_role() {
        let err = enforce_internal_api_runtime(Some(""), INTERNAL_API_PORT)
            .expect_err("missing role must be rejected");
        assert!(err.0.contains("runtime role is missing"));
    }

    #[test]
    fn runtime_rejects_unsupported_role() {
        let err = enforce_internal_api_runtime(Some("frobnicate"), INTERNAL_API_PORT)
            .expect_err("unknown role must be rejected");
        assert!(err.0.contains("unsupported runtime role"));
    }

    #[test]
    fn runtime_rejects_wrong_port() {
        let err = enforce_internal_api_runtime(Some("twitch_worker"), 9999)
            .expect_err("wrong port must be rejected");
        assert!(err.0.contains("expected port 8776"));
    }

    #[test]
    fn runtime_rejects_reserved_master_port() {
        let err = enforce_internal_api_runtime(Some("twitch_worker"), MASTER_API_RESERVED_PORT)
            .expect_err("reserved master port must be rejected");
        assert!(err.0.contains("reserved for the master API service"));
    }

    #[test]
    fn parse_env_bool_matches_python() {
        assert!(parse_env_bool("1", false));
        assert!(parse_env_bool("TRUE", false));
        assert!(parse_env_bool("on", false));
        assert!(!parse_env_bool("0", true));
        assert!(!parse_env_bool("off", true));
        assert!(parse_env_bool("garbage", true));
        assert!(!parse_env_bool("garbage", false));
    }

    // ── Middleware-Integration (Wiring) ──────────────────────────────────────

    mod middleware_wiring {
        use super::super::*;
        use axum::{
            body::Body,
            extract::ConnectInfo,
            http::{header, Request, StatusCode},
            middleware,
            routing::get,
            Router,
        };
        use std::net::SocketAddr;
        use tower::ServiceExt;

        /// Baut den gehärteten Middleware-Stack genau wie `build_internal_router`
        /// (Loopback-Guard + Auth-Guard), aber ohne DB-State.
        fn guarded_router(token: &str) -> Router {
            Router::new()
                .route("/x", get(|| async { "ok" }))
                .layer(middleware::from_fn_with_state(
                    token.to_string(),
                    internal_api_auth_guard,
                ))
                .layer(middleware::from_fn(internal_api_loopback_guard))
        }

        fn req(ip: &str, origin: Option<&str>, token: Option<&str>) -> Request<Body> {
            let addr: SocketAddr = format!("{ip}:54321").parse().unwrap();
            let mut b = Request::builder().uri("/x").extension(ConnectInfo(addr));
            if let Some(o) = origin {
                b = b.header(header::ORIGIN, o);
            }
            if let Some(t) = token {
                b = b.header(INTERNAL_TOKEN_HEADER, t);
            }
            b.body(Body::empty()).unwrap()
        }

        #[tokio::test]
        async fn loopback_peer_with_foreign_origin_is_forbidden() {
            // Kern der Lücke: Loopback-Peer, aber fremder Origin → 403.
            let app = guarded_router("secret");
            let res = app
                .oneshot(req("127.0.0.1", Some("https://evil.example.com"), Some("secret")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::FORBIDDEN);
        }

        #[tokio::test]
        async fn loopback_peer_with_loopback_origin_passes() {
            let app = guarded_router("secret");
            let res = app
                .oneshot(req("127.0.0.1", Some("http://localhost:8776"), Some("secret")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn no_origin_header_passes_on_loopback() {
            let app = guarded_router("secret");
            let res = app
                .oneshot(req("127.0.0.1", None, Some("secret")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn external_peer_is_forbidden() {
            let app = guarded_router("secret");
            let res = app
                .oneshot(req("10.0.0.1", None, Some("secret")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::FORBIDDEN);
        }

        #[tokio::test]
        async fn whitespace_padded_token_is_accepted() {
            // Block 10: präsentierter Token mit Whitespace → trim + match → 200.
            let app = guarded_router("secret");
            let res = app
                .oneshot(req("127.0.0.1", None, Some("  secret  ")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn empty_token_is_unauthorized() {
            let app = guarded_router("secret");
            let res = app
                .oneshot(req("127.0.0.1", None, Some("   ")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn empty_configured_token_is_fail_closed() {
            // Leeres konfiguriertes Token → immer 401, auch mit präsentiertem Wert.
            let app = guarded_router("");
            let res = app
                .oneshot(req("127.0.0.1", None, Some("anything")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
    }
}

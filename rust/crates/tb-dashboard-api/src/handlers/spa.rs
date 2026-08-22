//! SPA-Handler für `/analyse` (Haupt-HTML) und `/analyse/{path:.*}` (Assets).
//!
//! Port von `bot/analytics/api_overview.py:_serve_dashboard_v2` (Z. 542–587)
//! und `_serve_dashboard_v2_assets` / `_resolve_dashboard_v2_asset_response`
//! (Z. 807–879).
//!
//! Auth-Flow:
//! - None       → Redirect → /twitch/auth/login?next=%2Fanalyse
//! - Admin/Localhost → immer erlaubt
//! - Partner    → landing_access_allowed? → analytics_access_allowed?
//!   (bei false → Redirect → /twitch/dashboard)
//!
//! Main-Domain-Shells (`/twitch/dashboard`, `/twitch/verwaltung`,
//! `/twitch/uplink`) laufen ueber [`main_domain_spa_shell_gated_handler`] und
//! sind serverseitig gegated: ohne Session 303 auf den Login mit dem eigenen
//! Pfad als `next`, gesperrte Partner bekommen 403. Nur `/twitch/pricing`
//! bleibt als Marketing-Seite oeffentlich ([`main_domain_spa_shell_handler`]).
//!
//! Dist-Pfad: Env `DASHBOARD_V2_DIST_PATH`, Default `bot/analytics/dashboard_v2/dist`
//! (relativ zum WorkingDirectory des Service = Repo-Root).

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Redirect, Response},
};
use sqlx::PgPool;
use std::path::PathBuf;

use crate::auth::level::DashboardAuthLevel;

// ── Konstanten ──────────────────────────────────────────────────────────────

const LOGIN_URL: &str = "/twitch/auth/login?next=%2Fanalyse";
/// Vite baut das dashboard_v2-Bundle mit genau diesem Prefix; die
/// Main-Domain-Shells duerfen ihn deshalb nicht umschreiben.
const MAIN_DOMAIN_ASSET_PREFIX: &str = "/twitch/dashboard-v2/";
const LEGACY_DASHBOARD_URL: &str = "/twitch/dashboard";
const DEFAULT_DIST_PATH: &str = "bot/analytics/dashboard_v2/dist";

/// Literal-Fallback-Hostname des Admin-Dashboards.
///
/// Python: letzter Kandidat in `_configured_admin_dashboard_host`
/// (`api_v2.py:938-949`) ist `https://admin.deutsche-deadlock-community.de`,
/// und `_configured_admin_dashboard_host` liefert bei leerer Kette ebenfalls
/// genau diesen Hostnamen zurück.
const ADMIN_DASHBOARD_HOST_DEFAULT: &str = "admin.deutsche-deadlock-community.de";

/// Inline-Script das die React-App über apiBase und demoMode informiert.
/// Python: `_dashboard_runtime_script` + `_inject_dashboard_runtime_config`.
const RUNTIME_SCRIPT: &str = concat!(
    "<script>window.__TWITCH_DASHBOARD_RUNTIME__=Object.freeze(",
    r#"{"apiBase":"/twitch/api/v2","demoMode":false,"allowedDemoProfiles":[]}"#,
    ");</script>",
);

// ── Handler ─────────────────────────────────────────────────────────────────

/// `GET /analyse` — Haupt-HTML mit injizierten Runtime-Daten.
pub async fn analyse_handler(
    headers: HeaderMap,
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    // Admin-Host-Gate VOR der Auth-Prüfung (Python:
    // `_serve_dashboard_v2` ruft zuerst `_admin_dashboard_host_page_gate`,
    // `api_overview.py:544-546`). Wenn der Request auf dem Admin-Host
    // landet, liefern wir 404 — nutzerseitige Dashboard-Seiten bleiben
    // strikt vom Admin-Host fern.
    if let Some(r) = admin_dashboard_host_page_gate(&headers) {
        return r;
    }
    if let Some(r) = check_spa_auth(&auth, &pool).await {
        return r;
    }

    serve_dashboard_v2_index().await
}

/// Liefert die dashboard_v2-SPA-Shell ohne Host- oder Auth-Gate.
pub(crate) async fn serve_dashboard_v2_index() -> Response {
    serve_dashboard_v2_index_with_asset_prefix("/analyse/").await
}

/// Liefert die dashboard_v2-SPA-Shell fuer die oeffentliche Main-Domain-Seite
/// `/twitch/pricing`. Bewusst ohne Login-Gate: die Preisseite ist Marketing und
/// muss auch ausgeloggt laden.
///
/// Alle eingeloggten Shells laufen ueber
/// [`main_domain_spa_shell_gated_handler`].
pub async fn main_domain_spa_shell_handler() -> Response {
    serve_dashboard_v2_index_with_asset_prefix(MAIN_DOMAIN_ASSET_PREFIX).await
}

/// `GET /twitch/dashboard`, `/twitch/verwaltung`, `/twitch/uplink` — dieselbe
/// SPA-Shell, aber serverseitig gegated.
///
/// Bis zum Cutover-Commit c8ada2a7 lagen diese Seiten ungegated auf
/// [`main_domain_spa_shell_handler`]: jeder Besucher sah die komplette
/// Dashboard-Shell samt Navigation, nur die JSON-API antwortete "unauthorized".
///
/// Kaskade wie bei [`analyse_handler`], aber ohne das Analytics-Gate: das gilt
/// nur fuer `/analyse`. Wuerde es hier greifen, schickte `check_spa_auth` einen
/// Partner ohne Analytics-Zugang auf `/twitch/dashboard` — also im Kreis.
///
/// 1. Admin-Host-Gate (Admin-Host → 404),
/// 2. keine Session → 303 auf den Login, `next` ist der eigene Pfad,
/// 3. Partner ohne Landing-Freigabe → 403 mit derselben Ansage wie der
///    OAuth-Callback,
/// 4. Admin, Localhost und freigeschaltete Partner → Shell.
pub async fn main_domain_spa_shell_gated_handler(
    headers: HeaderMap,
    uri: Uri,
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    if let Some(r) = admin_dashboard_host_page_gate(&headers) {
        return r;
    }
    if let Some(r) = check_shell_auth(&auth, &pool, &shell_next_target(&uri)).await {
        return r;
    }
    serve_dashboard_v2_index_with_asset_prefix(MAIN_DOMAIN_ASSET_PREFIX).await
}

/// Rueckkehrziel fuer den Login: Pfad samt Query, damit `/twitch/dashboard?tab=x`
/// nach dem Login nicht auf dem nackten Dashboard landet.
///
/// Eine ueberlange Query (praeparierter Link) faellt auf den reinen Pfad
/// zurueck, damit der `Location`-Header nicht aufgeblaeht wird.
fn shell_next_target(uri: &Uri) -> String {
    const MAX_NEXT_LEN: usize = 512;
    match uri.path_and_query() {
        Some(pq) if pq.as_str().len() <= MAX_NEXT_LEN => pq.as_str().to_string(),
        _ => uri.path().to_string(),
    }
}

/// Login-Ziel einer gegateten Shell: das Rueckkehrziel als `next`, damit der
/// Streamer nach dem Twitch-Login dort landet, wo er hinwollte.
///
/// Alle drei Pfade stehen in `ALLOWED_NEXT_PREFIXES` (`auth/oauth_login.rs`),
/// sonst wuerde `sanitize_next_path` sie auf `/twitch/dashboard` zurueckfallen
/// lassen.
fn shell_login_url(path: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(path.as_bytes()).collect();
    format!("/twitch/auth/login?next={encoded}")
}

/// Wortgleiche Absage wie der OAuth-Callback (`handlers/auth_login.rs`), damit
/// ein gesperrter Account ueberall dieselbe Erklaerung bekommt.
fn kein_zugriff_response(display_name: &str, twitch_login: &str) -> Response {
    let who = if display_name.trim().is_empty() {
        twitch_login
    } else {
        display_name
    };
    (
        StatusCode::FORBIDDEN,
        [(header::CACHE_CONTROL, "no-store")],
        format!("Kein Zugriff: Twitch-Account '{who}' ist nicht als Streamer-Partner freigegeben."),
    )
        .into_response()
}

/// Reine Gate-Entscheidung fuer die Main-Domain-Shells. `landing_allowed` ist
/// nur fuer Partner relevant und wird von [`check_shell_auth`] vorher geladen.
fn shell_gate_decision(
    auth: &DashboardAuthLevel,
    landing_allowed: bool,
    path: &str,
) -> Option<Response> {
    match auth {
        DashboardAuthLevel::None => Some(Redirect::to(&shell_login_url(path)).into_response()),
        DashboardAuthLevel::Admin { .. } => None,
        DashboardAuthLevel::Partner {
            twitch_login,
            display_name,
            ..
        } => {
            if landing_allowed {
                None
            } else {
                Some(kein_zugriff_response(display_name, twitch_login))
            }
        }
    }
}

/// Laedt fuer Partner den Access-State und faellt damit in
/// [`shell_gate_decision`]. Bei DB-Fehlern bleibt der bereits eingeloggte
/// Partner drin (gleiche Kulanz wie [`check_spa_auth`]).
async fn check_shell_auth(
    auth: &DashboardAuthLevel,
    pool: &PgPool,
    path: &str,
) -> Option<Response> {
    let landing_allowed = match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => tb_analytics::partner_access::load_partner_access_state(
            pool,
            twitch_login,
            twitch_user_id,
        )
        .await
        .map(|access| access.landing_access_allowed)
        .unwrap_or_else(|e| {
            tracing::warn!("spa: Partner-Access-Fehler für {twitch_login}: {e}");
            true
        }),
        _ => true,
    };
    shell_gate_decision(auth, landing_allowed, path)
}

async fn serve_dashboard_v2_index_with_asset_prefix(asset_prefix: &str) -> Response {
    serve_dashboard_v2_index_from_root(dist_root(), asset_prefix).await
}

async fn serve_dashboard_v2_index_from_root(dist: PathBuf, asset_prefix: &str) -> Response {
    let index = dist.join("index.html");
    let mut html = match tokio::fs::read_to_string(&index).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                "Dashboard not built. Run npm run build in dashboard_v2/",
            )
                .into_response()
        }
    };

    // Vite baut die Assets mit Prefix /twitch/dashboard-v2/.
    if asset_prefix != MAIN_DOMAIN_ASSET_PREFIX {
        html = html.replace(MAIN_DOMAIN_ASSET_PREFIX, asset_prefix);
    }
    // Runtime-Script vor </head> injizieren (erstes Vorkommen).
    let html = html.replacen("</head>", &format!("{RUNTIME_SCRIPT}\n  </head>"), 1);

    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response()
}

/// Wie [`serve_dashboard_v2_index`], aber mit demselben Host-/Auth-Gate wie
/// [`analyse_handler`]. Ohne Session wird zum Login umgeleitet, statt die leere
/// SPA-Shell auszuliefern — deren Assets (`/analyse/assets/*`) sind auth-gegated
/// und würden sonst auf Login 303en, was im Browser ein Weißbild ergibt.
pub(crate) async fn serve_dashboard_v2_index_gated(
    headers: &HeaderMap,
    auth: &DashboardAuthLevel,
    pool: &PgPool,
) -> Response {
    if let Some(r) = admin_dashboard_host_page_gate(headers) {
        return r;
    }
    if let Some(r) = check_spa_auth(auth, pool).await {
        return r;
    }
    serve_dashboard_v2_index().await
}

/// `GET /analyse/{path:.*}` — statische Assets aus dist/.
pub async fn analyse_assets_handler(
    headers: HeaderMap,
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(asset_path): Path<String>,
) -> Response {
    // Admin-Host-Gate VOR der Auth-Prüfung (Python:
    // `_serve_dashboard_v2_assets` ruft zuerst `_admin_dashboard_host_page_gate`,
    // `api_overview.py:809-811`).
    if let Some(r) = admin_dashboard_host_page_gate(&headers) {
        return r;
    }
    if let Some(r) = check_spa_auth(&auth, &pool).await {
        return r;
    }
    // axum 0.7 liefert bei `/*path` den Wert mit führendem `/`
    serve_asset(asset_path.trim_start_matches('/')).await
}

/// `GET /twitch/dashboard-v2/{path:.*}` — dashboard_v2-Assets der
/// Main-Domain-Shells. Bewusst ohne Login-Gate, damit die oeffentliche
/// Preisseite `/twitch/pricing` auch ausgeloggt vollstaendig laedt.
///
/// Unbedenklich, weil hier ausschliesslich statische Dateien aus dem
/// Vite-Build-Verzeichnis rausgehen (`serve_asset`, segmentweise
/// Traversal-Pruefung): kein DB-Zugriff, keine Session, keine Nutzerdaten.
pub async fn dashboard_v2_public_assets_handler(Path(asset_path): Path<String>) -> Response {
    serve_asset(asset_path.trim_start_matches('/')).await
}

/// `GET /twitch/analyse` — Legacy-Redirect auf `/analyse` (301, Query erhalten).
pub async fn legacy_analyse_root_redirect_handler(uri: Uri) -> Response {
    moved_permanently(with_query("/analyse".to_string(), &uri))
}

/// `GET /twitch/analyse/{path:.*}` — Legacy-Redirect auf `/analyse/{path}`
/// (301, Query erhalten).
pub async fn legacy_analyse_path_redirect_handler(
    Path(raw_path): Path<String>,
    uri: Uri,
) -> Response {
    let normalized = raw_path.trim_start_matches('/');
    let location = if normalized.is_empty() {
        "/analyse".to_string()
    } else {
        format!("/analyse/{normalized}")
    };
    moved_permanently(with_query(location, &uri))
}

/// `GET /twitch/dashboard-v2` und weitere alte Main-Domain-Seiten — 301 auf
/// `/analyse` (Query erhalten).
pub async fn analyse_root_redirect_handler(uri: Uri) -> Response {
    moved_permanently(with_query("/analyse".to_string(), &uri))
}

fn with_query(location: String, uri: &Uri) -> String {
    match uri.query() {
        Some(q) if !q.is_empty() => format!("{location}?{q}"),
        _ => location,
    }
}

fn moved_permanently(location: String) -> Response {
    let value = match HeaderValue::from_str(&location) {
        Ok(value) => value,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid redirect target").into_response(),
    };
    let mut response = StatusCode::MOVED_PERMANENTLY.into_response();
    response.headers_mut().insert(header::LOCATION, value);
    response
}

// ── Social-Media-Admin-SPA (P2.66) ────────────────────────────────────────────

/// `GET /social-media-admin` — dedizierte Social-Media-Admin-SPA-Shell.
///
/// Port von `api_overview.py:_serve_social_media_admin`. Nutzt dasselbe
/// dashboard_v2-Bundle wie `/analyse`; der Client-Router rendert anhand des
/// Pfads das Social-Media-Admin-Dashboard. Anders als `/analyse` wird der
/// Asset-Prefix `/twitch/dashboard-v2/` NICHT umgeschrieben (Python-Parität:
/// `_inject_dashboard_runtime_config(..., asset_prefix="/twitch/dashboard-v2/")`),
/// die Assets liefert die `*path`-Route über den geteilten Dist.
///
/// Auth wie die Admin-SPA: Host-Gate VOR der Auth (admin-Host → 404), dann
/// Login-Gate (None → Redirect, Partner → landing/analytics-Access, Admin/
/// Localhost → frei).
pub async fn social_media_admin_handler(
    headers: HeaderMap,
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    if let Some(r) = admin_dashboard_host_page_gate(&headers) {
        return r;
    }
    if let Some(r) = check_spa_auth(&auth, &pool).await {
        return r;
    }

    let index = dist_root().join("index.html");
    let html = match tokio::fs::read_to_string(&index).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                "Dashboard not built. Run npm run build in dashboard_v2/",
            )
                .into_response()
        }
    };
    // Asset-Prefix bleibt /twitch/dashboard-v2/ (kein Rewrite); nur Runtime-
    // Script injizieren.
    let html = html.replacen("</head>", &format!("{RUNTIME_SCRIPT}\n  </head>"), 1);

    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response()
}

/// `GET /social-media-admin/{path:.*}` — statische Assets aus dem dashboard_v2-
/// Dist (geteilt mit `/analyse`). Gleiche Auth- + Host-Gate-Kaskade wie die
/// Shell. Python: `_serve_dashboard_v2_assets`.
pub async fn social_media_admin_assets_handler(
    headers: HeaderMap,
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(asset_path): Path<String>,
) -> Response {
    if let Some(r) = admin_dashboard_host_page_gate(&headers) {
        return r;
    }
    if let Some(r) = check_spa_auth(&auth, &pool).await {
        return r;
    }
    serve_asset(asset_path.trim_start_matches('/')).await
}

// ── Auth-Prüfung ─────────────────────────────────────────────────────────────

/// Gibt `Some(Response)` zurück wenn der Zugriff verweigert wird, sonst `None`.
async fn check_spa_auth(auth: &DashboardAuthLevel, pool: &PgPool) -> Option<Response> {
    match auth {
        DashboardAuthLevel::None => Some(Redirect::to(LOGIN_URL).into_response()),
        DashboardAuthLevel::Admin { .. } => None,
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => {
            let access =
                tb_analytics::partner_access::load_partner_access_state(pool, twitch_login, twitch_user_id)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!("spa: Partner-Access-Fehler für {twitch_login}: {e}");
                        tb_analytics::partner_access::AccessState {
                            analytics_access_allowed: true,
                            landing_access_allowed: true,
                            ..Default::default()
                        }
                    });

            if !access.landing_access_allowed {
                return Some(
                    (
                        StatusCode::FORBIDDEN,
                        "This account is blocked from all dashboard surfaces.",
                    )
                        .into_response(),
                );
            }
            if !access.analytics_access_allowed {
                return Some(Redirect::to(LEGACY_DASHBOARD_URL).into_response());
            }
            None
        }
    }
}

// ── Admin-Host-Gate ───────────────────────────────────────────────────────────

/// Hält nutzerseitige Dashboard-Seiten strikt vom Admin-Host fern.
///
/// Python: `_admin_dashboard_host_page_gate` (`api_overview.py:357-371`).
/// Liefert `Some(404)` wenn der `Host`-Header dem konfigurierten
/// Admin-Dashboard-Host entspricht, sonst `None` (Request läuft weiter).
///
/// Topologie-Hinweis: Hinter Caddy erreicht der `/analyse`-Vhost diesen
/// Handler nur, wenn Caddy den entsprechenden Host hierher proxyt. Auf dem
/// regulären Nutzer-Vhost ist der Gate ein No-op (Host != Admin-Host), greift
/// aber treu, falls jemand `/analyse` über den Admin-Host aufruft.
fn admin_dashboard_host_page_gate(headers: &HeaderMap) -> Option<Response> {
    if is_admin_dashboard_host_request(headers) {
        // Keep user-facing dashboard pages strictly off the admin host.
        return Some((StatusCode::NOT_FOUND, "Not found.").into_response());
    }
    None
}

/// `true` wenn der `Host`-Header dem konfigurierten Admin-Dashboard-Host
/// entspricht.
///
/// Python: `_is_admin_dashboard_host_request` (`api_v2.py:951-955`):
/// normalisierter Host-Header == `_configured_admin_dashboard_host()`.
/// Leerer Host → `false`.
pub(crate) fn is_admin_dashboard_host_request(headers: &HeaderMap) -> bool {
    let request_host = normalize_host_header(
        headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    );
    if request_host.is_empty() {
        return false;
    }
    request_host == configured_admin_dashboard_host()
}

/// Liefert den konfigurierten Admin-Dashboard-Hostnamen (lowercased).
///
/// Python: `_configured_admin_dashboard_host` (`api_v2.py:938-949`).
/// Kandidatenkette in Reihenfolge:
/// 1. Env `TWITCH_ADMIN_PUBLIC_URL`
/// 2. Env `MASTER_DASHBOARD_PUBLIC_URL`
/// 3. (Python: `self._discord_admin_redirect_uri` — im Code nie gesetzt, also
///    immer leer; entfällt nativ)
/// 4. Literal `https://admin.deutsche-deadlock-community.de`
///
/// Jeder Kandidat wird wie eine Origin/URL geparst; der erste mit nicht-leerem
/// Hostnamen gewinnt. Bei kompletter Leere → `ADMIN_DASHBOARD_HOST_DEFAULT`.
fn configured_admin_dashboard_host() -> String {
    let env_candidates = [
        std::env::var("TWITCH_ADMIN_PUBLIC_URL").ok(),
        std::env::var("MASTER_DASHBOARD_PUBLIC_URL").ok(),
        Some(format!("https://{ADMIN_DASHBOARD_HOST_DEFAULT}")),
    ];
    for candidate in env_candidates.into_iter().flatten() {
        let host = host_from_origin_like(&candidate);
        if !host.is_empty() {
            return host;
        }
    }
    ADMIN_DASHBOARD_HOST_DEFAULT.to_string()
}

/// Extrahiert den Hostnamen aus einem Origin-/URL-artigen String (lowercased).
///
/// Python: `_host_from_origin_like` (`api_v2.py:923-936`). Fügt `https://`
/// hinzu, wenn kein Schema vorhanden ist, und parst den Hostnamen. Fällt auf
/// `normalize_host_header` zurück, wenn das Parsen keinen Host liefert.
fn host_from_origin_like(raw_value: &str) -> String {
    let value = raw_value.trim();
    if value.is_empty() {
        return String::new();
    }
    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let host = parse_url_hostname(&candidate);
    if !host.is_empty() {
        return host;
    }
    normalize_host_header(value)
}

/// Normalisiert einen `Host`-Header zum reinen Hostnamen (lowercased, ohne
/// Port, ohne IPv6-Brackets).
///
/// Python: `_normalize_host_header` (`api_v2.py:908-921`). Nimmt das erste
/// Komma-getrennte Token (wie ein `X-Forwarded`-Stil-Header), prefixt `//`
/// wenn kein Schema vorhanden ist, und liest den Hostnamen.
fn normalize_host_header(raw_value: &str) -> String {
    let value = raw_value.trim();
    if value.is_empty() {
        return String::new();
    }
    let token = value.split(',').next().unwrap_or("").trim();
    if token.is_empty() {
        return String::new();
    }
    let candidate = if token.contains("://") {
        token.to_string()
    } else {
        format!("//{token}")
    };
    parse_url_hostname(&candidate)
}

/// Liest den Hostnamen aus einem URL-artigen String (lowercased, ohne Port,
/// ohne IPv6-Brackets). Entspricht Pythons `urlsplit(...).hostname`.
fn parse_url_hostname(candidate: &str) -> String {
    // Schema/Authority abtrennen: nach "//" beginnt die Authority.
    let after_scheme = match candidate.find("//") {
        Some(idx) => &candidate[idx + 2..],
        None => candidate,
    };
    // Authority endet beim ersten /, ? oder #.
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();
    if authority.is_empty() {
        return String::new();
    }
    // Userinfo abtrennen (user:pass@host).
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    // IPv6-Literal in Brackets: [::1]:8080 → ::1
    if let Some(start) = host_port.strip_prefix('[') {
        if let Some(end) = start.find(']') {
            return start[..end].to_lowercase();
        }
    }
    // IPv4/DNS: host:port — Port abtrennen (genau ein Doppelpunkt).
    let host = match host_port.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host_port,
    };
    host.to_lowercase()
}

// ── Asset-Serving ─────────────────────────────────────────────────────────────

/// Dist-Wurzel des Dashboard-Builds (von `/analyse` und `/twitch/demo` geteilt).
pub(crate) fn dist_root() -> PathBuf {
    let base = std::env::var("DASHBOARD_V2_DIST_PATH")
        .unwrap_or_else(|_| DEFAULT_DIST_PATH.to_string());
    PathBuf::from(base)
}

/// Dient eine Datei aus `dist/` mit strikter Pfad-Validierung.
///
/// Jedes Segment wird gegen `.`, `..` und `\` geprüft (Python-Parität).
/// Symlink-Angriffe sind bei eigenem Build-Output kein reales Angriffsszenario.
pub(crate) async fn serve_asset(raw_path: &str) -> Response {
    serve_asset_from_root(dist_root(), raw_path).await
}

async fn serve_asset_from_root(dist: PathBuf, raw_path: &str) -> Response {
    // Segmentweise Validierung — verhindert Path-Traversal
    let mut candidate = dist.clone();
    for segment in raw_path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
        candidate.push(segment);
    }

    let data = match tokio::fs::read(&candidate).await {
        Ok(d) => d,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };

    let mime = mime_for_path(&candidate);
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, cache_control_for_asset(raw_path)),
        ],
        data,
    ).into_response()
}

fn cache_control_for_asset(raw_path: &str) -> &'static str {
    if raw_path.starts_with("assets/") {
        // Vite haengt einen Hash an den Dateinamen, die Datei aendert sich nie.
        "public, max-age=31536000, immutable"
    } else if raw_path.starts_with("uplink/") {
        // Die Hilfe-Fragmente tragen keinen Hash im Namen. Mit einer Stunde
        // Cache saehe der Streamer nach einem Deploy die alte Anleitung neben
        // dem neuen Dashboard.
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}

fn mime_for_path(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("json") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body, http::HeaderValue};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Die Hilfe-Fragmente tragen keinen Hash im Dateinamen, ein Deploy tauscht
    /// sie unter demselben Pfad aus.
    #[test]
    fn hilfe_fragmente_werden_nicht_gecacht() {
        assert_eq!(cache_control_for_asset("uplink/obs.html"), "no-cache");
        assert_eq!(
            cache_control_for_asset("assets/index-a1b2c3.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control_for_asset("favicon.ico"), "public, max-age=3600");
    }

    #[test]
    fn host_header_normalisierung() {
        // Reiner Hostname bleibt.
        assert_eq!(normalize_host_header("example.com"), "example.com");
        // Port wird entfernt.
        assert_eq!(normalize_host_header("example.com:8769"), "example.com");
        // Lowercased.
        assert_eq!(normalize_host_header("Example.COM"), "example.com");
        // Erstes Komma-Token (X-Forwarded-Stil).
        assert_eq!(
            normalize_host_header("first.example.com, second.example.com"),
            "first.example.com"
        );
        // Leer bleibt leer.
        assert_eq!(normalize_host_header(""), "");
        assert_eq!(normalize_host_header("   "), "");
        // IPv6 in Brackets mit Port.
        assert_eq!(normalize_host_header("[::1]:8080"), "::1");
    }

    #[test]
    fn origin_zu_host() {
        // Mit Schema.
        assert_eq!(
            host_from_origin_like("https://admin.deutsche-deadlock-community.de"),
            "admin.deutsche-deadlock-community.de"
        );
        // Ohne Schema → https:// wird ergänzt.
        assert_eq!(
            host_from_origin_like("admin.deutsche-deadlock-community.de"),
            "admin.deutsche-deadlock-community.de"
        );
        // Mit Pfad und Port.
        assert_eq!(
            host_from_origin_like("https://admin.example.com:443/path?q=1"),
            "admin.example.com"
        );
        // Leer bleibt leer.
        assert_eq!(host_from_origin_like(""), "");
    }

    #[test]
    fn admin_host_default_erkannt() {
        // Ohne gesetzte Env-Variablen entspricht der Default-Hostname dem
        // Literal aus der Python-Kandidatenkette.
        // Hinweis: setzt KEINE Env-Variablen, um andere Tests nicht zu
        // beeinflussen — verlässt sich auf den Literal-Fallback.
        let mut admin = HeaderMap::new();
        admin.insert(
            header::HOST,
            "admin.deutsche-deadlock-community.de".parse().unwrap(),
        );
        assert!(is_admin_dashboard_host_request(&admin));

        // Mit Port am Admin-Host greift der Gate ebenfalls.
        let mut admin_port = HeaderMap::new();
        admin_port.insert(
            header::HOST,
            "admin.deutsche-deadlock-community.de:443".parse().unwrap(),
        );
        assert!(is_admin_dashboard_host_request(&admin_port));
    }

    #[test]
    fn nicht_admin_host_abgelehnt() {
        // Regulärer Nutzer-Host → kein Admin-Host.
        let mut user = HeaderMap::new();
        user.insert(header::HOST, "deutsche-deadlock-community.de".parse().unwrap());
        assert!(!is_admin_dashboard_host_request(&user));

        // Localhost → kein Admin-Host.
        let mut local = HeaderMap::new();
        local.insert(header::HOST, "localhost:8769".parse().unwrap());
        assert!(!is_admin_dashboard_host_request(&local));

        // Fehlender Host-Header → false.
        let empty = HeaderMap::new();
        assert!(!is_admin_dashboard_host_request(&empty));
    }

    #[test]
    fn gate_liefert_404_nur_auf_admin_host() {
        // Admin-Host → 404-Gate greift.
        let mut admin = HeaderMap::new();
        admin.insert(
            header::HOST,
            "admin.deutsche-deadlock-community.de".parse().unwrap(),
        );
        let resp = admin_dashboard_host_page_gate(&admin);
        assert!(resp.is_some());
        assert_eq!(resp.unwrap().status(), StatusCode::NOT_FOUND);

        // Nutzer-Host → kein Gate, Request läuft weiter.
        let mut user = HeaderMap::new();
        user.insert(header::HOST, "deutsche-deadlock-community.de".parse().unwrap());
        assert!(admin_dashboard_host_page_gate(&user).is_none());
    }

    #[tokio::test]
    async fn asset_handler_setzt_cache_header() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("tb_spa_asset_test_{unique}"));
        let assets = root.join("assets");
        tokio::fs::create_dir_all(&assets).await.unwrap();
        tokio::fs::write(assets.join("index-abc123.js"), b"console.log('ok');").await.unwrap();

        let resp = serve_asset_from_root(root.clone(), "assets/index-abc123.js").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("public, max-age=31536000, immutable"))
        );
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/javascript; charset=utf-8"))
        );
        let body = body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"console.log('ok');");

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    fn test_partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "ismile_e".to_string(),
            twitch_user_id: "4242".to_string(),
            display_name: "iSmile_E".to_string(),
        }
    }

    /// Ohne Session darf keine Shell rausgehen. Der Login bekommt den eigenen
    /// Pfad als `next`, sonst landet der Streamer nach dem Twitch-Login auf
    /// einer anderen Seite als der, die er aufgerufen hat.
    #[test]
    fn ohne_session_fuehrt_die_shell_auf_den_login() {
        let erwartet = [
            (
                "/twitch/dashboard",
                "/twitch/auth/login?next=%2Ftwitch%2Fdashboard",
            ),
            (
                "/twitch/verwaltung",
                "/twitch/auth/login?next=%2Ftwitch%2Fverwaltung",
            ),
            (
                "/twitch/uplink",
                "/twitch/auth/login?next=%2Ftwitch%2Fuplink",
            ),
        ];
        for (pfad, ziel) in erwartet {
            let resp = shell_gate_decision(&DashboardAuthLevel::None, true, pfad)
                .expect("ohne Session muss der Gate greifen");
            assert_eq!(resp.status(), StatusCode::SEE_OTHER, "{pfad}");
            assert_eq!(
                resp.headers()
                    .get(header::LOCATION)
                    .unwrap()
                    .to_str()
                    .unwrap(),
                ziel,
                "{pfad}"
            );
            // Der Login wirft ein nicht gelistetes `next` weg — dann liefe der
            // Redirect ins Leere statt zurueck auf die aufgerufene Seite.
            assert_eq!(
                crate::auth::oauth_login::sanitize_next_path(Some(pfad)),
                pfad,
                "{pfad} fehlt in ALLOWED_NEXT_PREFIXES"
            );
        }
    }

    /// Der Login soll den Streamer dorthin zurueckbringen, wo er hinwollte,
    /// also samt Query. Nur ein praeparierter Riesen-Link faellt auf den
    /// nackten Pfad zurueck.
    #[test]
    fn next_ziel_behaelt_die_query_und_deckelt_ausreisser() {
        let mit_query: Uri = "/twitch/dashboard?tab=growth".parse().unwrap();
        assert_eq!(
            shell_next_target(&mit_query),
            "/twitch/dashboard?tab=growth"
        );

        let resp = shell_gate_decision(
            &DashboardAuthLevel::None,
            true,
            &shell_next_target(&mit_query),
        )
        .expect("ohne Session muss der Gate greifen");
        assert_eq!(
            resp.headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/twitch/auth/login?next=%2Ftwitch%2Fdashboard%3Ftab%3Dgrowth"
        );
        // Die Whitelist prueft nur den Pfadteil, die Query ueberlebt.
        assert_eq!(
            crate::auth::oauth_login::sanitize_next_path(Some("/twitch/dashboard?tab=growth")),
            "/twitch/dashboard?tab=growth"
        );

        let riesig: Uri = format!("/twitch/uplink?x={}", "a".repeat(600))
            .parse()
            .unwrap();
        assert_eq!(shell_next_target(&riesig), "/twitch/uplink");
    }

    /// Kulanz bei DB-Wackler: ein bereits eingeloggter Partner fliegt nicht
    /// raus, nur weil der Access-State gerade nicht lesbar ist. Der lazy Pool
    /// zeigt auf einen toten Port, `load_partner_access_state` scheitert also.
    #[tokio::test]
    async fn db_fehler_sperrt_eingeloggte_partner_nicht_aus() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            // Ohne kurzes Timeout haengt der Pool 30 s im Default-Retry.
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .expect("lazy pool");
        assert!(check_shell_auth(&test_partner(), &pool, "/twitch/uplink")
            .await
            .is_none());
        // Ohne Session bleibt es trotz DB-Fehler beim Login-Redirect.
        let resp = check_shell_auth(&DashboardAuthLevel::None, &pool, "/twitch/uplink")
            .await
            .expect("ohne Session muss der Gate greifen");
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    }

    /// Ein Partner ohne Landing-Freigabe bekommt dieselbe Absage wie im
    /// OAuth-Callback, keine Shell und keinen Redirect.
    #[tokio::test]
    async fn partner_ohne_freigabe_bekommt_kein_zugriff() {
        let resp = shell_gate_decision(&test_partner(), false, "/twitch/uplink")
            .expect("gesperrter Partner muss geblockt werden");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
        let body = body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(
            text,
            "Kein Zugriff: Twitch-Account 'iSmile_E' ist nicht als Streamer-Partner freigegeben."
        );
    }

    /// Freigegebener Partner und Admin kommen durch, und die Shell behaelt den
    /// Asset-Prefix /twitch/dashboard-v2/ (Vite baut das Bundle nur dafuer).
    #[tokio::test]
    async fn freigegebener_partner_bekommt_die_shell() {
        for pfad in ["/twitch/dashboard", "/twitch/verwaltung", "/twitch/uplink"] {
            assert!(
                shell_gate_decision(&test_partner(), true, pfad).is_none(),
                "{pfad}"
            );
            assert!(
                shell_gate_decision(&DashboardAuthLevel::admin(), true, pfad).is_none(),
                "{pfad}"
            );
        }

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tb_spa_shell_test_{unique}"));
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(
            root.join("index.html"),
            b"<html><head></head><body><script src=\"/twitch/dashboard-v2/assets/app.js\"></script></body></html>",
        )
        .await
        .unwrap();

        let resp = serve_dashboard_v2_index_from_root(root.clone(), MAIN_DOMAIN_ASSET_PREFIX).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            html.contains("/twitch/dashboard-v2/assets/app.js"),
            "{html}"
        );
        assert!(html.contains("__TWITCH_DASHBOARD_RUNTIME__"), "{html}");

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn asset_handler_blockiert_traversal() {
        let root = std::env::temp_dir().join("tb_spa_asset_traversal_test");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let resp = serve_asset_from_root(root.clone(), "../secret.txt").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}

//! Handler für `GET /twitch/api/v2/internal-home` (+ POST `…/changelog`).
//!
//! Nativer Port von Python `bot/analytics/services/internal_home.py`
//! (Payload-Builder) und `bot/analytics/api_v2.py` (Handler, Identity-Resolve,
//! Changelog, Scope-Snapshot, health_score, week_comparison, access_state).
//!
//! Jeder Datenblock ist fehlertolerant: bei DB-/IO-Fehler liefert er den
//! Python-Default (leere Liste / null / 0) und loggt nur `tracing::warn!` —
//! ein einzelner kaputter Block darf NIE einen 500 für den ganzen Endpoint
//! auslösen (entspricht dem `asyncio.gather(return_exceptions=True)`-Muster
//! plus `internal_home_result_or_default` in Python).
//!
//! Bewusste Abweichungen ggü. Python (siehe Report):
//! - `display_name` = echter Twitch-display_name aus dem Login-Session-Snapshot
//!   (B16-FIX-INTERNALHOME-DISPLAYNAME), Fallback auf resolved_login wenn leer.
//! - Logfile-Kandidaten: nur `logs/<datei>` relativ zum CWD (= Repo-Root),
//!   nicht die zusätzlichen `log_path`/Sibling-Kandidaten von Python.
//! - Rate-Limit / CSRF-Origin-Check NICHT portiert (Python-spezifische Hooks).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::auth::level::{DashboardAuthLevel, is_local_request};
use crate::handlers::auth_status::{
    ADMIN_DEFAULT_STREAMER, admin_mode_header_active, is_public_dashboard,
};

// ── Konstanten (Python api_v2.py:466-492) ────────────────────────────────────

const DEFAULT_DAYS: i64 = 30;
const BAN_REASON_KEYWORDS: &[&str] = &["bot", "spam", "scam", "phish", "link", "promo", "werbung"];
const REQUIRED_SCOPES: &[&str] = &["channel:manage:raids"];

const CHANGELOG_MAX_ENTRIES: i64 = 20;
const CHANGELOG_TITLE_MAX_LENGTH: usize = 160;
const CHANGELOG_CONTENT_MAX_LENGTH: usize = 4000;

const SERVICE_WARNING_LOG_FILENAME: &str = "twitch_service_warnings.log";
const SERVICE_WARNING_MAX_SCAN_LINES: usize = 5000;
const SERVICE_WARNING_MAX_EVENTS: usize = 20;
const AUTOBAN_LOG_FILENAME: &str = "twitch_autobans.log";
const AUTOBAN_MAX_SCAN_LINES: usize = 5000;
const AUTOBAN_MAX_EVENTS: usize = 20;
const ACTIVITY_MAX_EVENTS: usize = 10;

const PARTNER_STATUS_ACTIVE: &str = "active";
const PARTNER_STATUS_ARCHIVED: &str = "archived";
const PARTNER_STATUS_DEPARTNERED: &str = "departnered";
const PARTNER_STATUS_NON_PARTNER: &str = "non_partner";
const PARTNER_STATUS_TOKEN_ERROR: &str = "token_error";
const PARTNER_STATUS_BLOCKED: &str = "blocked";
/// `_ANALYTICS_BLOCKED_PARTNER_STATUSES` (api_v2.py:74-81).
const ANALYTICS_BLOCKED_PARTNER_STATUSES: &[&str] = &[
    PARTNER_STATUS_BLOCKED,
    PARTNER_STATUS_DEPARTNERED,
    PARTNER_STATUS_NON_PARTNER,
    PARTNER_STATUS_TOKEN_ERROR,
];

const INTERNAL_HOME_LOGIN_URL: &str = "/twitch/auth/login?next=%2Ftwitch%2Fdashboard";
const INTERNAL_HOME_DISCORD_CONNECT_URL: &str =
    "/twitch/auth/discord/link?next=%2Ftwitch%2Fverwaltung";
const STEAM_LINK_DEFAULT_BASE_URL: &str = "https://deutsche-deadlock-community.de/link";

/// Basis-URL für den Steam-Verknüpfungs-Flow (Steam-Link läuft über die
/// Community-Seite, gekoppelt an die Discord-ID). Liest `STEAM_LINK_START_BASE_URL`
/// und schneidet einen evtl. abschließenden `/` ab.
fn steam_link_base() -> String {
    std::env::var("STEAM_LINK_START_BASE_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| STEAM_LINK_DEFAULT_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

// ── Request-Parameter ────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct InternalHomeQuery {
    #[serde(default)]
    pub days: Option<String>,
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ChangelogBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub entry_date: Option<String>,
}

// ── Identity-Resolution (api_v2.py:1803-1846) ────────────────────────────────

/// Aufgelöste Identität für die Payload + ein erkennbarer Override-Marker.
struct ResolvedIdentity {
    twitch_login: String,
    twitch_user_id: String,
    display_name: String,
}

/// Auth-Identity treu zu `_resolve_internal_home_identity`.
///
/// - `None` → 401 `auth_required`
/// - `Partner` → eigener Login/User-Id; `?streamer=` nur == eigener Login,
///   sonst 403 `streamer_override_requires_admin`. Bei Override → user_id="".
/// - `Admin { actor: Some }` → ohne Override eigener Twitch-Account, mit
///   Override der gewählte Streamer.
/// - `Localhost`/`Admin { actor: None }` → `?streamer=` Pflicht (sonst 401
///   `streamer_session_required`, weil keine Twitch-Identität vorhanden ist).
#[allow(clippy::result_large_err)]
fn resolve_identity(
    auth: &DashboardAuthLevel,
    streamer_override: &Option<String>,
    public_user_view: bool,
) -> Result<ResolvedIdentity, Response> {
    let override_login = normalize_override(streamer_override);

    match auth {
        DashboardAuthLevel::None => Err(unauthorized_json(
            "auth_required",
            "A valid dashboard session is required.",
        )),
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            display_name,
        } => {
            let own_login = twitch_login.trim().to_lowercase();
            let own_user_id = twitch_user_id.trim().to_string();
            // Echter Twitch-display_name aus dem Login-Snapshot; leer → Login-
            // Fallback (Python api_v2.py:1826: session display_name → twitch_login).
            let own_display = display_name.trim().to_string();
            if !override_login.is_empty() {
                if override_login != own_login {
                    return Err(forbidden_json(
                        "streamer_override_requires_admin",
                        "Only admin sessions may view another streamer's profile.",
                    ));
                }
                // Eigener Login als Override → echter display_name bleibt erhalten.
                let display = if own_display.is_empty() {
                    override_login.clone()
                } else {
                    own_display
                };
                return Ok(ResolvedIdentity {
                    twitch_login: override_login,
                    twitch_user_id: String::new(),
                    display_name: display,
                });
            }
            if own_login.is_empty() && own_user_id.is_empty() {
                return Err(unauthorized_json(
                    "streamer_session_required",
                    "The dashboard session must be bound to a Twitch streamer account.",
                ));
            }
            let login_fallback = if own_login.is_empty() {
                own_user_id.clone()
            } else {
                own_login.clone()
            };
            let display = if own_display.is_empty() {
                login_fallback
            } else {
                own_display
            };
            Ok(ResolvedIdentity {
                twitch_login: own_login,
                twitch_user_id: own_user_id,
                display_name: display,
            })
        }
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            if !override_login.is_empty() {
                return Ok(ResolvedIdentity {
                    twitch_login: override_login.clone(),
                    twitch_user_id: String::new(),
                    display_name: override_login,
                });
            }

            let own_login = actor.twitch_login.trim().to_lowercase();
            let own_user_id = actor.twitch_user_id.trim().to_string();
            if own_login.is_empty() && own_user_id.is_empty() {
                return Err(unauthorized_json(
                    "streamer_session_required",
                    "The dashboard session must be bound to a Twitch streamer account.",
                ));
            }

            let display_name = actor.twitch_login.trim();
            Ok(ResolvedIdentity {
                twitch_login: own_login,
                twitch_user_id: own_user_id.clone(),
                display_name: if display_name.is_empty() {
                    own_user_id
                } else {
                    display_name.to_string()
                },
            })
        }
        DashboardAuthLevel::Admin { actor: None } => {
            if override_login.is_empty() {
                if public_user_view {
                    return Ok(ResolvedIdentity {
                        twitch_login: ADMIN_DEFAULT_STREAMER.to_string(),
                        twitch_user_id: String::new(),
                        display_name: ADMIN_DEFAULT_STREAMER.to_string(),
                    });
                }
                // Python: keine Twitch-Session vorhanden → auth_required/streamer_session_required.
                return Err(unauthorized_json(
                    "streamer_session_required",
                    "The dashboard session must be bound to a Twitch streamer account.",
                ));
            }
            Ok(ResolvedIdentity {
                twitch_login: override_login.clone(),
                twitch_user_id: String::new(),
                display_name: override_login,
            })
        }
    }
}

fn normalize_override(raw: &Option<String>) -> String {
    raw.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .unwrap_or_default()
}

fn unauthorized_json(code: &str, message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": code,
            "message": message,
            "loginUrl": INTERNAL_HOME_LOGIN_URL,
        })),
    )
        .into_response()
}

fn forbidden_json(code: &str, message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "error": code, "message": message })),
    )
        .into_response()
}

// ── ISO-Helfer ───────────────────────────────────────────────────────────────

/// `_internal_home_iso`: TIMESTAMPTZ → ISO8601-String, sonst "".
fn iso_ts(value: Option<DateTime<Utc>>) -> String {
    value.map(|v| v.to_rfc3339()).unwrap_or_default()
}

fn parse_text_ts(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .ok()
        .map(|ts| ts.with_timezone(&Utc))
}

fn iso_text(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    if let Some(ts) = parse_text_ts(value) {
        return ts.to_rfc3339();
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return date.to_string();
    }
    value.to_string()
}

/// Liest eine optionale Zeit-/Datumsspalte als ISO-String.
fn row_ts_iso(row: &PgRow, col: &str) -> String {
    if let Ok(value) = row.try_get::<Option<DateTime<Utc>>, _>(col) {
        return iso_ts(value);
    }
    if let Ok(value) = row.try_get::<Option<NaiveDate>, _>(col) {
        return value.map(|v| v.to_string()).unwrap_or_default();
    }
    if let Ok(value) = row.try_get::<Option<String>, _>(col) {
        return value.as_deref().map(iso_text).unwrap_or_default();
    }
    String::new()
}

// ── GET-Handler ──────────────────────────────────────────────────────────────

/// `GET /twitch/api/v2/internal-home?days=&streamer=`
pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<InternalHomeQuery>,
    headers: HeaderMap,
) -> Response {
    // days parsen + clamp 1..=365 (api_v2.py:2015-2020)
    let days = query
        .days
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_DAYS)
        .clamp(1, 365);

    let public_user_view = matches!(auth, DashboardAuthLevel::Admin { actor: None })
        && is_public_dashboard(&headers)
        && !admin_mode_header_active(&headers);
    let identity = match resolve_identity(&auth, &query.streamer, public_user_view) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let has_admin_access = auth.is_privileged() && !public_user_view;

    // Identity-Resolve (DB): twitch_streamer_identities (internal_home.py:403-446)
    let (resolved_login, resolved_user_id, discord_connected) =
        identity_block(&pool, &identity.twitch_login, &identity.twitch_user_id).await;

    let generated_at = Utc::now().to_rfc3339();
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    // ── Datenblöcke (jeder fehlertolerant) ──────────────────────────────────
    let access_state = access_state_block(&pool, &resolved_login, &resolved_user_id).await;
    let oauth = oauth_block(&pool, &resolved_login, &resolved_user_id).await;
    let kpis = kpis_recent_block(&pool, &resolved_login, since).await;
    let ban = ban_events_block(&pool, &resolved_user_id, since).await;
    let raid_events = raid_events_block(&pool, &resolved_login, &resolved_user_id, since).await;
    let last_stream = last_stream_summary(&pool, &resolved_login, &kpis.recent_streams).await;
    let health_score = health_score_block(&pool, &resolved_login).await;
    let week_comparison = week_comparison_block(&pool, &resolved_login).await;
    let live_status = live_status_block(&pool, &resolved_login, &resolved_user_id).await;

    // Steam-Verknüpfung läuft über die Discord-ID (Vorbild: onboarding.rs).
    // Ohne aufgelöste Discord-ID gar kein fetch_rank-Call (kein unnötiger I/O).
    let steam_discord_id = tb_chat::stats::resolve_discord_id(&pool, &resolved_user_id).await;
    let steam_connected = match steam_discord_id.as_deref() {
        Some(discord_id) => tb_chat::stats::fetch_rank(discord_id, false)
            .await
            .map(|rank| rank.linked)
            .unwrap_or(false),
        None => false,
    };
    let steam_connect_url = steam_discord_id
        .as_deref()
        .map(|discord_id| format!("{}/steam/login?uid={}", steam_link_base(), discord_id));

    let autoban_events = load_autoban_events(&resolved_login, since);
    let service_warning_events = load_service_warning_events(&resolved_login, since);

    // bot_events-Merge: ban(DB)+raid(DB)+autoban(log)+service(log),
    // sort timestamp DESC, cap 10 (internal_home.py:1071-1089)
    let mut bot_events: Vec<Value> = Vec::new();
    bot_events.extend(ban.events.iter().cloned());
    bot_events.extend(raid_events.iter().cloned());
    bot_events.extend(autoban_events.iter().cloned());
    bot_events.extend(service_warning_events.iter().cloned());
    bot_events.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(Value::as_str).unwrap_or("");
        let tb = b.get("timestamp").and_then(Value::as_str).unwrap_or("");
        tb.cmp(ta) // DESC
    });
    bot_events.truncate(ACTIVITY_MAX_EVENTS);

    // target_id-Stripping für nicht-privilegierte (api_v2.py:1774-1785)
    if !has_admin_access {
        for ev in bot_events.iter_mut() {
            if let Some(obj) = ev.as_object_mut() {
                obj.remove("target_id");
            }
        }
    }

    let streamer_bound = !(resolved_login.is_empty() && resolved_user_id.is_empty());
    let partner_status = access_state.partner_status.clone();
    let can_access_analytics = access_state.analytics_access_allowed;
    let oauth_reconnect_url = if !resolved_login.is_empty() {
        "/twitch/raid/auth".to_string()
    } else {
        INTERNAL_HOME_LOGIN_URL.to_string()
    };

    // links (internal_home.py:1170-1181)
    let overview_query = if resolved_login.is_empty() {
        format!("days={days}")
    } else {
        format!("days={days}&streamer={resolved_login}")
    };

    let display_name = if identity.display_name.is_empty() {
        resolved_login.clone()
    } else {
        identity.display_name.clone()
    };

    // changelog (api_v2.py:2047-2069)
    let changelog = changelog_payload(&pool, has_admin_access).await;

    let payload = json!({
        "profile": {
            "twitch_login": resolved_login,
            "twitch_user_id": resolved_user_id,
            "display_name": if display_name.is_empty() { resolved_login.clone() } else { display_name },
        },
        "status": {
            "authenticated": true,
            "streamer_bound": streamer_bound,
            "period_days": days,
            "oauth": {
                "connected": !oauth.granted_scopes.is_empty(),
                "status": oauth.oauth_status,
                "needs_reauth": oauth.oauth_needs_reauth,
                "granted_scopes": oauth.granted_scopes,
                "missing_scopes": oauth.missing_scopes,
                "reconnect_url": oauth_reconnect_url,
                "profile_url": "/twitch/dashboard",
                "last_checked_at": generated_at,
            },
            "discord": {
                "connected": discord_connected,
                "status": if discord_connected { "connected" } else { "missing" },
                "connect_url": INTERNAL_HOME_DISCORD_CONNECT_URL,
                "last_checked_at": generated_at,
            },
            "steam": {
                "connected": steam_connected,
                "status": if steam_connected { "connected" } else { "missing" },
                "connect_url": steam_connect_url,
            },
            "raid_status": { "state": "active", "read_only": true },
            "partner": {
                "status": partner_status,
                "technical_pause_reason": access_state.technical_pause_reason,
                "operational_state": access_state.operational_state,
                "token_error_grace_expires_at": access_state.token_error_grace_expires_at,
                "token_error_error_count": access_state.token_error_error_count,
            },
            "access": { "landing": true, "analytics": can_access_analytics },
        },
        "kpis": {
            "streams_count": kpis.streams_count,
            "avg_viewers": round1(kpis.avg_viewers),
            "follower_delta": kpis.follower_delta,
            "bot_bans_keyword_count": ban.bot_bans_keyword_count,
        },
        "recent_streams": kpis.recent_streams,
        "last_stream_summary": last_stream,
        "health_score": health_score,
        "week_comparison": week_comparison,
        "live_status": live_status,
        "bot_impact": {
            "events": bot_events,
            "summary": {
                "ban_keyword_hits_30d": ban.bot_bans_keyword_count,
                "recent_raid_events": raid_events.len(),
                "recent_autoban_events": autoban_events.len(),
                "recent_service_warnings": service_warning_events.len(),
            },
            "note": "Raid automation is active in read-only mode. Bot impact events are informational and no write action is triggered here.",
        },
        "bot_activity": { "events": bot_events },
        "links": {
            "dashboard": "/twitch/dashboard",
            "dashboard_v2": if can_access_analytics { "/analyse" } else { "/twitch/dashboard" },
            "raid_history": "/twitch/raid/history",
            "raid_requirements": "/twitch/raid/requirements",
            "billing": "/twitch/abbo",
            "oauth_reconnect": oauth_reconnect_url,
            "profile_status": "/twitch/dashboard",
            "discord_connect": INTERNAL_HOME_DISCORD_CONNECT_URL,
            "internal_home_api": format!("/twitch/api/v2/internal-home?days={days}"),
            "overview_api": format!("/twitch/api/v2/overview?{overview_query}"),
        },
        "generated_at": generated_at,
        "changelog": changelog,
    });

    Json(payload).into_response()
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

// ── Block 0: Identity-Resolve (internal_home.py:403-446) ─────────────────────

async fn identity_block(
    pool: &PgPool,
    twitch_login: &str,
    twitch_user_id: &str,
) -> (String, String, bool) {
    let mut resolved_login = twitch_login.to_string();
    let mut resolved_user_id = twitch_user_id.to_string();
    let mut discord_connected = false;

    let sql = r#"
        SELECT
            LOWER(twitch_login) AS login,
            COALESCE(twitch_user_id, '') AS user_id,
            CASE
                WHEN COALESCE(is_on_discord, 0) = 1 THEN 1
                WHEN COALESCE(discord_user_id, '') <> '' THEN 1
                ELSE 0
            END AS discord_connected
        FROM twitch_streamer_identities
        WHERE (COALESCE($1, '') != '' AND LOWER(twitch_login) = $2)
           OR (COALESCE($3, '') != '' AND twitch_user_id = $4)
        ORDER BY CASE
            WHEN (COALESCE($1, '') != '' AND LOWER(twitch_login) = $2) THEN 0
            ELSE 1
        END
        LIMIT 1
    "#;
    let login_lower = twitch_login.to_lowercase();
    match sqlx::query(sql)
        .bind(twitch_login)
        .bind(&login_lower)
        .bind(twitch_user_id)
        .bind(twitch_user_id)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => {
            let l: String = row.try_get("login").unwrap_or_default();
            if !l.trim().is_empty() {
                resolved_login = l.trim().to_lowercase();
            }
            let u: String = row.try_get("user_id").unwrap_or_default();
            if !u.trim().is_empty() {
                resolved_user_id = u.trim().to_string();
            }
            let dc: i32 = row.try_get("discord_connected").unwrap_or(0);
            discord_connected = dc != 0;
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("internal-home identity block: {e}"),
    }

    (
        resolved_login.trim().to_lowercase(),
        resolved_user_id.trim().to_string(),
        discord_connected,
    )
}

// ── Block: access_state / partner (api_v2.py:976-1135) ───────────────────────

struct AccessState {
    partner_status: String,
    technical_pause_reason: Option<String>,
    operational_state: Option<String>,
    token_error_grace_expires_at: Option<String>,
    token_error_error_count: i64,
    analytics_access_allowed: bool,
}

impl AccessState {
    /// Default: leerer Login → active/alles erlaubt (Python-Default).
    fn default_active() -> Self {
        Self {
            partner_status: PARTNER_STATUS_ACTIVE.to_string(),
            technical_pause_reason: None,
            operational_state: None,
            token_error_grace_expires_at: None,
            token_error_error_count: 0,
            analytics_access_allowed: true,
        }
    }
}

async fn access_state_block(
    pool: &PgPool,
    twitch_login: &str,
    twitch_user_id: &str,
) -> AccessState {
    let normalized_login = twitch_login.trim().to_lowercase();
    let normalized_user_id = twitch_user_id.trim().to_string();
    if normalized_login.is_empty() && normalized_user_id.is_empty() {
        return AccessState::default_active();
    }

    // Spaltenprüfung: technical_pause_reason existiert ggf. nicht.
    let has_tpr = column_exists(pool, "twitch_partners", "technical_pause_reason").await;
    let tpr_expr = if has_tpr {
        "technical_pause_reason"
    } else {
        "NULL::text AS technical_pause_reason"
    };

    let partner_sql = format!(
        r#"
        SELECT
            twitch_login,
            twitch_user_id,
            status,
            COALESCE(
                admin_archived_at,
                CASE WHEN status = 'archived' THEN departnered_at ELSE NULL END
            ) AS archived_at,
            manual_partner_opt_out,
            {tpr_expr},
            partnered_at AS created_at,
            departnered_at
        FROM twitch_partners
        WHERE (COALESCE($1, '') != '' AND LOWER(twitch_login) = LOWER($1))
           OR (COALESCE($2, '') != '' AND twitch_user_id = $2)
        ORDER BY CASE
            WHEN COALESCE(status, '') = 'active' THEN 0
            WHEN COALESCE(status, '') = 'archived' THEN 1
            WHEN COALESCE(status, '') = 'departnered' THEN 2
            ELSE 3
        END,
        COALESCE(departnered_at, admin_archived_at, partnered_at) DESC
        LIMIT 1
    "#
    );

    let partner_row = match sqlx::query(&partner_sql)
        .bind(&normalized_login)
        .bind(&normalized_user_id)
        .fetch_optional(pool)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("internal-home access-state partner query: {e}");
            return AccessState::default_active();
        }
    };

    let mut technical_pause_reason = String::new();
    let mut operational_state = String::new();
    let mut partner_status = if normalized_login.is_empty() && normalized_user_id.is_empty() {
        PARTNER_STATUS_ACTIVE.to_string()
    } else {
        PARTNER_STATUS_NON_PARTNER.to_string()
    };

    if let Some(row) = &partner_row {
        let status_text = row
            .try_get::<Option<String>, _>("status")
            .unwrap_or(None)
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let archived_at = row
            .try_get::<Option<String>, _>("archived_at")
            .unwrap_or(None)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let manual_opt_out = row
            .try_get::<Option<i32>, _>("manual_partner_opt_out")
            .unwrap_or(None)
            .unwrap_or(0)
            != 0;
        technical_pause_reason = row
            .try_get::<Option<String>, _>("technical_pause_reason")
            .unwrap_or(None)
            .unwrap_or_default()
            .trim()
            .to_lowercase();

        if technical_pause_reason == PARTNER_STATUS_BLOCKED {
            operational_state = PARTNER_STATUS_BLOCKED.to_string();
        } else if status_text != PARTNER_STATUS_ACTIVE {
            operational_state = "inactive".to_string();
        } else if manual_opt_out {
            operational_state = "admin_non_partner".to_string();
        } else if !technical_pause_reason.is_empty() {
            operational_state = technical_pause_reason.clone();
        } else {
            operational_state = PARTNER_STATUS_ACTIVE.to_string();
        }

        if technical_pause_reason == PARTNER_STATUS_BLOCKED {
            partner_status = PARTNER_STATUS_BLOCKED.to_string();
        } else if manual_opt_out {
            partner_status = PARTNER_STATUS_NON_PARTNER.to_string();
        } else if technical_pause_reason == PARTNER_STATUS_TOKEN_ERROR {
            partner_status = PARTNER_STATUS_TOKEN_ERROR.to_string();
        } else if status_text == PARTNER_STATUS_ARCHIVED || archived_at.is_some() {
            partner_status = PARTNER_STATUS_ARCHIVED.to_string();
        } else if status_text == PARTNER_STATUS_DEPARTNERED {
            partner_status = PARTNER_STATUS_DEPARTNERED.to_string();
        } else if status_text != PARTNER_STATUS_ACTIVE {
            partner_status = PARTNER_STATUS_NON_PARTNER.to_string();
        }
    }

    // Blacklist (api_v2.py:1076-1121)
    let mut grace_expires_at: Option<DateTime<Utc>> = None;
    let mut token_error_error_count: i64 = 0;
    let blacklist_sql = r#"
        SELECT grace_expires_at, error_count, role_removed
        FROM twitch_token_blacklist
        WHERE (COALESCE($1, '') != '' AND twitch_user_id = $1)
           OR (COALESCE($2, '') != '' AND LOWER(twitch_login) = LOWER($2))
        ORDER BY last_error_at DESC NULLS LAST, first_error_at DESC NULLS LAST
        LIMIT 1
    "#;
    match sqlx::query(blacklist_sql)
        .bind(&normalized_user_id)
        .bind(&normalized_login)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => {
            grace_expires_at = row
                .try_get::<Option<String>, _>("grace_expires_at")
                .unwrap_or(None)
                .as_deref()
                .and_then(parse_text_ts);
            token_error_error_count = row
                .try_get::<Option<i32>, _>("error_count")
                .unwrap_or(None)
                .map(i64::from)
                .unwrap_or(0);
            let role_removed = row
                .try_get::<Option<i32>, _>("role_removed")
                .unwrap_or(None)
                .unwrap_or(0)
                != 0;
            let grace_active = grace_expires_at.is_some_and(|g| g > Utc::now()) && !role_removed;
            let blocking = matches!(
                partner_status.as_str(),
                PARTNER_STATUS_BLOCKED | PARTNER_STATUS_NON_PARTNER | PARTNER_STATUS_DEPARTNERED
            );
            if !blocking
                && grace_active
                && (technical_pause_reason == PARTNER_STATUS_TOKEN_ERROR
                    || token_error_error_count > 0)
            {
                partner_status = PARTNER_STATUS_TOKEN_ERROR.to_string();
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("internal-home access-state blacklist query: {e}"),
    }

    let analytics_access_allowed =
        !ANALYTICS_BLOCKED_PARTNER_STATUSES.contains(&partner_status.as_str());

    AccessState {
        partner_status,
        technical_pause_reason: if technical_pause_reason.is_empty() {
            None
        } else {
            Some(technical_pause_reason)
        },
        operational_state: if operational_state.is_empty() {
            None
        } else {
            Some(operational_state)
        },
        token_error_grace_expires_at: grace_expires_at.map(|g| g.to_rfc3339()),
        token_error_error_count,
        analytics_access_allowed,
    }
}

/// Prüft, ob eine Spalte in `public.<table>` existiert.
async fn column_exists(pool: &PgPool, table: &str, column: &str) -> bool {
    let sql = r#"
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = $1 AND column_name = $2
        LIMIT 1
    "#;
    matches!(
        sqlx::query(sql)
            .bind(table)
            .bind(column)
            .fetch_optional(pool)
            .await,
        Ok(Some(_))
    )
}

// ── Block: oauth (internal_home.py:449-498 + scope-snapshot api_v2.py:114-142) ─

struct OauthData {
    granted_scopes: Vec<String>,
    missing_scopes: Vec<String>,
    oauth_needs_reauth: bool,
    oauth_status: String,
}

async fn oauth_block(pool: &PgPool, resolved_login: &str, resolved_user_id: &str) -> OauthData {
    if resolved_login.is_empty() {
        return OauthData {
            granted_scopes: Vec::new(),
            missing_scopes: Vec::new(),
            oauth_needs_reauth: false,
            oauth_status: "missing".to_string(),
        };
    }

    let sql = r#"
        SELECT scopes, needs_reauth
        FROM twitch_raid_auth
        WHERE ($1 != '' AND TRIM(COALESCE(twitch_user_id, '')) = $1)
           OR ($2 != '' AND LOWER(COALESCE(twitch_login, '')) = LOWER($2))
        ORDER BY CASE
            WHEN ($1 != '' AND TRIM(COALESCE(twitch_user_id, '')) = $1) THEN 0
            ELSE 1
        END
        LIMIT 1
    "#;
    match sqlx::query(sql)
        .bind(resolved_user_id)
        .bind(resolved_login)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => {
            let scopes_raw: String = row
                .try_get::<Option<String>, _>("scopes")
                .unwrap_or(None)
                .unwrap_or_default();
            // needs_reauth kann bool oder int sein; tolerant lesen.
            let needs_reauth = read_truthy(&row, "needs_reauth");
            scope_snapshot(&scopes_raw, needs_reauth)
        }
        Ok(None) => OauthData {
            granted_scopes: Vec::new(),
            missing_scopes: REQUIRED_SCOPES.iter().map(|s| s.to_string()).collect(),
            oauth_needs_reauth: false,
            oauth_status: "missing".to_string(),
        },
        Err(e) => {
            tracing::warn!("internal-home oauth block: {e}");
            OauthData {
                granted_scopes: Vec::new(),
                missing_scopes: Vec::new(),
                oauth_needs_reauth: false,
                oauth_status: "missing".to_string(),
            }
        }
    }
}

/// `_oauth_scope_snapshot` (api_v2.py:114-142).
fn scope_snapshot(scopes_raw: &str, needs_reauth: bool) -> OauthData {
    let mut granted: Vec<String> = scopes_raw
        .split_whitespace()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    granted.sort();
    granted.dedup();

    let missing: Vec<String> = REQUIRED_SCOPES
        .iter()
        .filter(|s| !granted.iter().any(|g| g == *s))
        .map(|s| s.to_string())
        .collect();

    let connected = !granted.is_empty();
    let status = if needs_reauth {
        "reauth"
    } else if !connected {
        "missing"
    } else if !missing.is_empty() {
        "partial"
    } else {
        "connected"
    };

    OauthData {
        granted_scopes: granted,
        missing_scopes: missing,
        oauth_needs_reauth: needs_reauth,
        oauth_status: status.to_string(),
    }
}

/// Liest eine Spalte tolerant als bool (bool, oder int != 0).
fn read_truthy(row: &PgRow, col: &str) -> bool {
    if let Ok(b) = row.try_get::<Option<bool>, _>(col) {
        return b.unwrap_or(false);
    }
    if let Ok(i) = row.try_get::<Option<i64>, _>(col) {
        return i.unwrap_or(0) != 0;
    }
    if let Ok(i) = row.try_get::<Option<i32>, _>(col) {
        return i.unwrap_or(0) != 0;
    }
    false
}

// ── Block 2a: KPIs + recent_streams (internal_home.py:501-588) ───────────────

struct KpisData {
    streams_count: i64,
    avg_viewers: f64,
    follower_delta: i64,
    recent_streams: Vec<Value>,
}

async fn kpis_recent_block(pool: &PgPool, resolved_login: &str, since: DateTime<Utc>) -> KpisData {
    let mut data = KpisData {
        streams_count: 0,
        avg_viewers: 0.0,
        follower_delta: 0,
        recent_streams: Vec::new(),
    };
    if resolved_login.is_empty() {
        return data;
    }

    let kpi_sql = r#"
        SELECT
            COUNT(*) AS streams_count,
            COALESCE(AVG(s.avg_viewers), 0)::float8 AS avg_viewers,
            COALESCE(SUM(CASE
                WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                THEN s.follower_delta ELSE 0 END), 0)::bigint AS follower_delta
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $2
    "#;
    match sqlx::query(kpi_sql)
        .bind(since)
        .bind(resolved_login)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => {
            data.streams_count = row.try_get::<i64, _>("streams_count").unwrap_or(0);
            data.avg_viewers = row.try_get::<f64, _>("avg_viewers").unwrap_or(0.0);
            data.follower_delta = row.try_get::<i64, _>("follower_delta").unwrap_or(0);
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("internal-home kpi query: {e}"),
    }

    let recent_sql = r#"
        SELECT
            s.started_at,
            s.ended_at,
            s.duration_seconds,
            s.avg_viewers,
            s.peak_viewers,
            CASE
                WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                THEN s.follower_delta ELSE 0 END AS follower_delta,
            s.stream_title
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $2
        ORDER BY s.started_at DESC
        LIMIT 5
    "#;
    match sqlx::query(recent_sql)
        .bind(since)
        .bind(resolved_login)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => {
            for row in &rows {
                let started_iso = row_ts_iso(row, "started_at");
                let date = if started_iso.len() >= 10 {
                    started_iso[..10].to_string()
                } else {
                    String::new()
                };
                let avg_v = read_f64(row, "avg_viewers");
                data.recent_streams.push(json!({
                    "date": date,
                    "started_at": started_iso,
                    "ended_at": row_ts_iso(row, "ended_at"),
                    "duration_seconds": read_i64(row, "duration_seconds"),
                    "avg_viewers": round1(avg_v),
                    "peak_viewers": read_i64(row, "peak_viewers"),
                    "follower_delta": read_i64(row, "follower_delta"),
                    "title": row.try_get::<Option<String>, _>("stream_title").unwrap_or(None).unwrap_or_default(),
                }));
            }
        }
        Err(e) => tracing::warn!("internal-home recent-streams query: {e}"),
    }

    data
}

/// Liest eine numerische Spalte tolerant als i64 (i64/i32/f64-Quellen).
fn read_i64(row: &PgRow, col: &str) -> i64 {
    if let Ok(v) = row.try_get::<Option<i64>, _>(col) {
        return v.unwrap_or(0);
    }
    if let Ok(v) = row.try_get::<Option<i32>, _>(col) {
        return v.unwrap_or(0) as i64;
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(col) {
        return v.unwrap_or(0.0) as i64;
    }
    0
}

/// Liest eine numerische Spalte tolerant als f64.
fn read_f64(row: &PgRow, col: &str) -> f64 {
    if let Ok(v) = row.try_get::<Option<f64>, _>(col) {
        return v.unwrap_or(0.0);
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(col) {
        return v.unwrap_or(0) as f64;
    }
    if let Ok(v) = row.try_get::<Option<i32>, _>(col) {
        return v.unwrap_or(0) as f64;
    }
    0.0
}

// ── Block 2b: last_stream_summary (internal_home.py:717-734,827-839) ─────────

async fn last_stream_summary(
    pool: &PgPool,
    resolved_login: &str,
    recent_streams: &[Value],
) -> Value {
    let Some(ls) = recent_streams.first() else {
        return Value::Null;
    };
    let started_at = ls.get("started_at").and_then(Value::as_str).unwrap_or("");
    let ended_at = ls.get("ended_at").and_then(Value::as_str).unwrap_or("");

    let chat_count: Option<i64> = if resolved_login.is_empty() {
        None
    } else {
        chat_count_block(pool, resolved_login, started_at, ended_at).await
    };

    let mut obj = ls.clone();
    if let Some(map) = obj.as_object_mut() {
        map.insert("chat_messages".to_string(), json!(chat_count));
    }
    obj
}

async fn chat_count_block(
    pool: &PgPool,
    resolved_login: &str,
    started_at: &str,
    ended_at: &str,
) -> Option<i64> {
    // Python bindet started_at/ended_at als String-Timestamps an message_ts.
    let started_dt = parse_iso(started_at)?;
    let ended_dt = parse_iso(ended_at)?;
    let sql = r#"
        SELECT COUNT(*) AS c FROM twitch_chat_messages
        WHERE LOWER(streamer_login) = LOWER($1)
          AND message_ts >= $2 AND message_ts <= $3
    "#;
    match sqlx::query(sql)
        .bind(resolved_login)
        .bind(started_dt)
        .bind(ended_dt)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => Some(row.try_get::<i64, _>("c").unwrap_or(0)),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!("internal-home chat-count query: {e}");
            None
        }
    }
}

fn parse_iso(text: &str) -> Option<DateTime<Utc>> {
    if text.trim().is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ── Block 2c: ban-events (DB) (internal_home.py:591-660) ─────────────────────

struct BanData {
    bot_bans_keyword_count: i64,
    events: Vec<Value>,
}

async fn ban_events_block(pool: &PgPool, resolved_user_id: &str, since: DateTime<Utc>) -> BanData {
    let mut data = BanData {
        bot_bans_keyword_count: 0,
        events: Vec::new(),
    };
    if resolved_user_id.is_empty() {
        return data;
    }

    // KEYWORD_CLAUSE (internal_home.py:34-46): dynamische OR-LIKE-Kette.
    // Bind-Positionen: $1=since, $2=user_id, $3.. = keyword-LIKEs.
    let mut clause_parts: Vec<String> = Vec::new();
    for i in 0..BAN_REASON_KEYWORDS.len() {
        clause_parts.push(format!("LOWER(COALESCE(b.reason, '')) LIKE ${}", i + 3));
    }
    let clause = if clause_parts.is_empty() {
        "1=0".to_string()
    } else {
        format!("({})", clause_parts.join(" OR "))
    };

    let count_sql = format!(
        r#"
        SELECT COUNT(*) AS c
        FROM twitch_ban_events b
        WHERE b.received_at >= $1
          AND b.twitch_user_id = $2
          AND LOWER(COALESCE(b.event_type, '')) = 'ban'
          AND {clause}
    "#
    );
    let mut q = sqlx::query(&count_sql).bind(since).bind(resolved_user_id);
    let like_params: Vec<String> = BAN_REASON_KEYWORDS
        .iter()
        .map(|k| format!("%{k}%"))
        .collect();
    for p in &like_params {
        q = q.bind(p);
    }
    match q.fetch_optional(pool).await {
        Ok(Some(row)) => data.bot_bans_keyword_count = row.try_get::<i64, _>("c").unwrap_or(0),
        Ok(None) => {}
        Err(e) => tracing::warn!("internal-home ban-count query: {e}"),
    }

    let list_sql = r#"
        SELECT b.received_at, b.target_login, b.target_id, b.moderator_login, b.reason
        FROM twitch_ban_events b
        WHERE b.received_at >= $1
          AND b.twitch_user_id = $2
          AND LOWER(COALESCE(b.event_type, '')) = 'ban'
        ORDER BY b.received_at DESC
        LIMIT 20
    "#;
    match sqlx::query(list_sql)
        .bind(since)
        .bind(resolved_user_id)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => {
            for row in &rows {
                let target_login = opt_str(row, "target_login").trim().to_string();
                let moderator_login = opt_str(row, "moderator_login").trim().to_string();
                let reason = opt_str(row, "reason").trim().to_string();
                let mut summary_parts: Vec<String> = Vec::new();
                if !reason.is_empty() {
                    summary_parts.push(reason.clone());
                }
                if !moderator_login.is_empty() {
                    summary_parts.push(format!("Mod: @{moderator_login}"));
                }
                let summary = if summary_parts.is_empty() {
                    "Ban ausgeführt".to_string()
                } else {
                    summary_parts.join(" | ")
                };
                let title = if target_login.is_empty() {
                    "Ban ausgeführt".to_string()
                } else {
                    format!("Ban gegen @{target_login}")
                };
                data.events.push(json!({
                    "type": "ban",
                    "event_type": "ban",
                    "timestamp": row_ts_iso(row, "received_at"),
                    "target_login": target_login,
                    "target_id": opt_str(row, "target_id"),
                    "moderator_login": moderator_login,
                    "reason": reason,
                    "status_label": "[BANNED]",
                    "title": title,
                    "summary": summary,
                    "severity": "warning",
                }));
            }
        }
        Err(e) => tracing::warn!("internal-home ban-list query: {e}"),
    }

    data
}

fn opt_str(row: &PgRow, col: &str) -> String {
    row.try_get::<Option<String>, _>(col)
        .unwrap_or(None)
        .unwrap_or_default()
}

// ── Block 2d: raid-events (DB) (internal_home.py:663-714) ────────────────────

async fn raid_events_block(
    pool: &PgPool,
    resolved_login: &str,
    resolved_user_id: &str,
    since: DateTime<Utc>,
) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();
    if resolved_login.is_empty() && resolved_user_id.is_empty() {
        return events;
    }

    let sql = r#"
        SELECT
            r.executed_at,
            r.to_broadcaster_login,
            r.to_broadcaster_id,
            r.viewer_count,
            r.reason,
            r.success
        FROM twitch_raid_history r
        WHERE r.executed_at >= $1
          AND (
              (COALESCE($2, '') != '' AND r.from_broadcaster_id = $2)
              OR (COALESCE($3, '') != '' AND LOWER(r.from_broadcaster_login) = $3)
          )
        ORDER BY r.executed_at DESC
        LIMIT 10
    "#;
    match sqlx::query(sql)
        .bind(since)
        .bind(resolved_user_id)
        .bind(resolved_login)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => {
            for row in &rows {
                let success = match row.try_get::<Option<bool>, _>("success") {
                    Ok(Some(b)) => b,
                    Ok(None) => true, // Python: None → True
                    Err(_) => true,
                };
                events.push(json!({
                    "type": "raid_history",
                    "timestamp": row_ts_iso(row, "executed_at"),
                    "target_login": opt_str(row, "to_broadcaster_login"),
                    "target_id": opt_str(row, "to_broadcaster_id"),
                    "viewer_count": read_i64(row, "viewer_count"),
                    "reason": opt_str(row, "reason"),
                    "success": success,
                    "status_label": "[RAID]",
                }));
            }
        }
        Err(e) => tracing::warn!("internal-home raid-events query: {e}"),
    }
    events
}

// ── Block 2e: autoban-events (LOGFILE) (internal_home.py:268-400) ────────────

fn load_autoban_events(resolved_login: &str, since: DateTime<Utc>) -> Vec<Value> {
    let channel_key = resolved_login.trim().to_lowercase();
    if channel_key.is_empty() {
        return Vec::new();
    }
    let Some(lines) = read_log_tail(AUTOBAN_LOG_FILENAME, AUTOBAN_MAX_SCAN_LINES) else {
        return Vec::new();
    };

    let mut events: Vec<Value> = Vec::new();
    for raw_line in lines.iter().rev() {
        let Some(parsed) = parse_autoban_line(raw_line) else {
            continue;
        };
        let event_channel = parsed
            .get("actor_login")
            .and_then(Value::as_str)
            .or_else(|| parsed.get("moderator_login").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if event_channel != channel_key {
            continue;
        }
        if let Some(ts) = parsed.get("timestamp").and_then(Value::as_str) {
            if let Some(event_dt) = parse_iso(ts) {
                if event_dt < since {
                    continue;
                }
            } else {
                continue;
            }
        }
        events.push(parsed);
        if events.len() >= AUTOBAN_MAX_EVENTS {
            break;
        }
    }
    events
}

/// `parse_internal_home_autoban_line` (internal_home.py:268-338).
fn parse_autoban_line(raw_line: &str) -> Option<Value> {
    let line = raw_line.trim();
    if line.is_empty() {
        return None;
    }
    let parts = split_tab(line, 6);
    if parts.len() < 6 {
        return None;
    }
    let part = |i: usize| parts.get(i).map(|s| s.trim()).unwrap_or("");

    let timestamp_raw = part(0);
    let status_raw = part(1);
    let channel_login = part(2).to_lowercase();
    let chatter_login = part(3).to_lowercase();
    let chatter_id = part(4);
    let reason_text = part(5);
    let content_text = part(6);

    let normalized_status = status_raw.trim().trim_matches(['[', ']']).to_uppercase();
    if normalized_status != "BANNED" {
        return None;
    }

    let timestamp = normalize_log_timestamp(timestamp_raw);
    let reason = empty_if_dash(reason_text);
    let content = empty_if_dash(content_text);
    let target_login = empty_if_dash(&chatter_login);
    let target_id = empty_if_dash(chatter_id);
    let status_label = if status_raw.starts_with('[') && status_raw.ends_with(']') {
        status_raw.to_string()
    } else {
        "[BANNED]".to_string()
    };

    let mut summary_parts: Vec<String> = Vec::new();
    if !reason.is_empty() {
        summary_parts.push(reason.clone());
    }
    if !content.is_empty() {
        summary_parts.push(content.clone());
    }
    if !channel_login.is_empty() {
        summary_parts.push(format!("Mod: @{channel_login}"));
    }
    let summary = if summary_parts.is_empty() {
        "Ban ausgeführt".to_string()
    } else {
        summary_parts.join(" | ")
    };

    let mut description_parts: Vec<String> = Vec::new();
    if !reason.is_empty() {
        description_parts.push(format!("Signale: {reason}"));
    }
    if !content.is_empty() {
        description_parts.push(format!("Nachricht: {content}"));
    }
    let description = description_parts.join(" | ");

    let title = if target_login.is_empty() {
        "Ban ausgeführt".to_string()
    } else {
        format!("Ban gegen @{target_login}")
    };

    Some(json!({
        "type": "ban",
        "event_type": "ban",
        "timestamp": timestamp,
        "target_login": target_login,
        "target_id": target_id,
        "moderator_login": channel_login,
        "actor_login": channel_login,
        "reason": reason,
        "status_label": status_label,
        "title": title,
        "summary": summary,
        "description": description,
        "severity": "warning",
        "source": "autoban_log",
    }))
}

// ── Block 2f: service-pitch-warnings (LOGFILE) (internal_home.py:132-265) ─────

fn load_service_warning_events(resolved_login: &str, since: DateTime<Utc>) -> Vec<Value> {
    let channel_key = resolved_login.trim().to_lowercase();
    if channel_key.is_empty() {
        return Vec::new();
    }
    let Some(lines) = read_log_tail(SERVICE_WARNING_LOG_FILENAME, SERVICE_WARNING_MAX_SCAN_LINES)
    else {
        return Vec::new();
    };

    let mut events: Vec<Value> = Vec::new();
    for raw_line in lines.iter().rev() {
        let Some(parsed) = parse_service_warning_line(raw_line) else {
            continue;
        };
        // HINT-Filter (internal_home.py:248-250)
        let severity_label = parsed
            .get("status_label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_uppercase();
        if severity_label.contains("HINT") {
            continue;
        }
        let event_channel = parsed
            .get("actor_login")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if event_channel != channel_key {
            continue;
        }
        if let Some(ts) = parsed.get("timestamp").and_then(Value::as_str) {
            if let Some(event_dt) = parse_iso(ts) {
                if event_dt < since {
                    continue;
                }
            } else {
                continue;
            }
        }
        events.push(parsed);
        if events.len() >= SERVICE_WARNING_MAX_EVENTS {
            break;
        }
    }
    events
}

/// `parse_internal_home_service_warning_line` (internal_home.py:132-201).
fn parse_service_warning_line(raw_line: &str) -> Option<Value> {
    let line = raw_line.trim();
    if line.is_empty() {
        return None;
    }
    let parts = split_tab(line, 10);
    if parts.len() < 10 {
        return None;
    }
    let part = |i: usize| parts.get(i).map(|s| s.trim()).unwrap_or("");

    let timestamp_raw = part(0);
    let severity_code = part(1).to_uppercase();
    let channel_login = part(2).to_lowercase();
    let chatter_login = part(3).to_lowercase();
    let chatter_id = part(4);
    let age_days = parse_prefixed_int(part(5), "age_days=");
    let follower_count = parse_prefixed_int(part(6), "followers=");
    let score = parse_prefixed_int(part(7), "score=");
    let message_count = parse_prefixed_int(part(8), "msgs=");
    let reasons_text = part(9);
    let content_text = part(10);

    let timestamp = normalize_log_timestamp(timestamp_raw);

    let mut metric_parts: Vec<String> = Vec::new();
    if let Some(s) = score {
        metric_parts.push(format!("Score {s}"));
    }
    if let Some(m) = message_count {
        metric_parts.push(format!("Msgs {m}"));
    }
    if let Some(a) = age_days {
        if a >= 0 {
            metric_parts.push(format!("Account {a}d"));
        }
    }
    if let Some(f) = follower_count {
        metric_parts.push(format!("Followers {f}"));
    }
    let metric = metric_parts.join(" | ");

    let reason = if reasons_text == "-" {
        String::new()
    } else {
        reasons_text.to_string()
    };
    let mut description_parts: Vec<String> = Vec::new();
    if !reason.is_empty() {
        description_parts.push(format!("Signale: {reason}"));
    }
    if !content_text.is_empty() {
        description_parts.push(format!("Nachricht: {content_text}"));
    }
    let description = description_parts.join(" | ");

    let chatter_label = if !chatter_login.is_empty() && chatter_login != "-" {
        format!("@{chatter_login}")
    } else {
        "Unbekannt".to_string()
    };
    let mut summary_parts: Vec<String> = vec![chatter_label];
    if !metric.is_empty() {
        summary_parts.push(metric.clone());
    }
    let summary = summary_parts.join(" | ");

    let status_label = format!(
        "[{}]",
        if severity_code.is_empty() {
            "WARNING"
        } else {
            &severity_code
        }
    );

    Some(json!({
        "type": "service_pitch_warning",
        "event_type": "service_pitch_warning",
        "timestamp": timestamp,
        "target_login": if chatter_login == "-" { String::new() } else { chatter_login.clone() },
        "target_id": if chatter_id == "-" { String::new() } else { chatter_id.to_string() },
        "actor_login": channel_login,
        "status_label": status_label,
        "title": service_warning_title(&severity_code),
        "summary": summary,
        "description": description,
        "reason": reason,
        "metric": metric,
        "severity": service_warning_severity(&severity_code),
        "source": "service_warning_log",
    }))
}

/// `internal_home_service_warning_title` (internal_home.py:108-118).
fn service_warning_title(severity_code: &str) -> &'static str {
    match severity_code.trim().to_uppercase().as_str() {
        "ESCALATED_TIMEOUT" => "Service-Pitch eskaliert (Timeout)",
        "WARNING_STRONG" => "Service-Pitch Warnung (stark)",
        "WARNING_PUBLIC" => "Service-Pitch Warnung",
        "HINT" => "Service-Pitch Hinweis",
        _ => "Service-Pitch Ereignis",
    }
}

/// `internal_home_service_warning_severity` (internal_home.py:121-129).
fn service_warning_severity(severity_code: &str) -> &'static str {
    match severity_code.trim().to_uppercase().as_str() {
        "ESCALATED_TIMEOUT" => "critical",
        "WARNING_STRONG" | "WARNING_PUBLIC" => "warning",
        "HINT" => "info",
        _ => "warning",
    }
}

/// `internal_home_parse_prefixed_int` (internal_home.py:73-83).
fn parse_prefixed_int(token: &str, prefix: &str) -> Option<i64> {
    let normalized = token.trim();
    if !normalized
        .to_lowercase()
        .starts_with(&prefix.to_lowercase())
    {
        return None;
    }
    let raw_value = normalized[prefix.len()..].trim();
    if raw_value.is_empty() || raw_value == "-" {
        return None;
    }
    raw_value.parse::<i64>().ok()
}

/// Python `str.split("\t", maxsplit)`: höchstens `maxsplit` Splits → maxsplit+1 Teile.
fn split_tab(line: &str, maxsplit: usize) -> Vec<String> {
    let mut out: Vec<String> = line
        .splitn(maxsplit + 1, '\t')
        .map(|s| s.to_string())
        .collect();
    // Python: bei genau maxsplit+0-Splits hängt es ein "" an, damit Index maxsplit existiert.
    // splitn liefert maxsplit+1 Teile nur wenn genug Tabs da sind; sonst weniger.
    // parse_*-Funktionen lesen Index `maxsplit` (= Content) tolerant via `part()`.
    if out.len() == maxsplit {
        out.push(String::new());
    }
    out
}

fn empty_if_dash(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() || t == "-" {
        String::new()
    } else {
        t.to_string()
    }
}

/// Normalisiert einen Log-Timestamp: parst ISO → rfc3339, sonst Rohwert.
fn normalize_log_timestamp(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    let normalized = text.replace('Z', "+00:00");
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return dt.with_timezone(&Utc).to_rfc3339();
    }
    // Naive ISO ohne TZ → als UTC interpretieren (Python-Verhalten).
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        return DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc).to_rfc3339();
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S") {
        return DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc).to_rfc3339();
    }
    text.to_string()
}

/// Liest die letzten `max_lines` Zeilen von `logs/<filename>` (relativ zum CWD).
fn read_log_tail(filename: &str, max_lines: usize) -> Option<Vec<String>> {
    let path = std::path::Path::new("logs").join(filename);
    let file = std::fs::File::open(&path).ok()?;
    let reader = BufReader::new(file);
    let mut buf: VecDeque<String> = VecDeque::with_capacity(max_lines.min(1024));
    for line in reader.lines() {
        match line {
            Ok(l) => {
                if buf.len() == max_lines {
                    buf.pop_front();
                }
                buf.push_back(l);
            }
            Err(_) => continue,
        }
    }
    Some(buf.into_iter().collect())
}

// ── Block 2g: health_score (api_v2.py:215-355) ───────────────────────────────

/// Bekannte Service-/Chat-Bot-Accounts (Python `bot/core/chat_bots.py:8`,
/// `KNOWN_CHAT_BOTS`). Single Source of Truth für den Community-Score-Filter.
const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

/// Baut Pythons `build_known_chat_bot_not_in_clause` nach (`chat_bots.py:34`):
/// schließt bekannte Bot-Logins UND den eigenen Streamer-Login aus, behält aber
/// Zeilen ohne `chatter_login` (NULL/'') — damit anonyme `chatter_id`-Zeilen
/// nicht fälschlich rausfallen. Gibt die SQL-Klausel (mit `$start..`-Platzhaltern)
/// und die sortierten, normalisierten Bot-Logins zurück.
fn known_chat_bot_not_in_clause(
    column_expr: &str,
    own_login: &str,
    start_param: usize,
) -> (String, Vec<String>) {
    let mut set: std::collections::BTreeSet<String> =
        KNOWN_CHAT_BOTS.iter().map(|b| b.to_string()).collect();
    let own = own_login.trim().to_lowercase();
    if !own.is_empty() {
        set.insert(own);
    }
    let logins: Vec<String> = set.into_iter().collect();
    if logins.is_empty() {
        return ("1=1".to_string(), Vec::new());
    }
    let placeholders: Vec<String> = (0..logins.len())
        .map(|i| format!("${}", start_param + i))
        .collect();
    let clause = format!(
        "(({col}) IS NULL OR ({col}) = '' OR LOWER({col}) NOT IN ({ph}))",
        col = column_expr,
        ph = placeholders.join(", ")
    );
    (clause, logins)
}

async fn health_score_block(pool: &PgPool, resolved_login: &str) -> Value {
    if resolved_login.is_empty() {
        return Value::Null;
    }
    match compute_health_score(pool, resolved_login).await {
        Some(v) => v,
        None => Value::Null,
    }
}

async fn compute_health_score(pool: &PgPool, login: &str) -> Option<Value> {
    let mut now = Utc::now();

    // Anchor: jüngster beobachteter Zeitpunkt, nicht in der Zukunft.
    let anchor_sql = r#"
        SELECT MAX(observed_ts) AS anchor FROM (
            SELECT MAX(ts_utc) AS observed_ts FROM twitch_stats_tracked
              WHERE LOWER(streamer) = LOWER($1)
            UNION ALL
            SELECT MAX(started_at) AS observed_ts FROM twitch_stream_sessions
              WHERE LOWER(streamer_login) = LOWER($1)
        ) observed
    "#;
    if let Ok(Some(row)) = sqlx::query(anchor_sql)
        .bind(login)
        .fetch_optional(pool)
        .await
    {
        if let Some(anchor) = row
            .try_get::<Option<DateTime<Utc>>, _>("anchor")
            .unwrap_or(None)
        {
            if anchor < now {
                now = anchor;
            }
        }
    }

    let week_ago = now - Duration::days(7);
    let two_weeks_ago = now - Duration::days(14);

    // Current week
    let cur_sql = r#"
        SELECT AVG(viewer_count)::float8 AS avg_viewers,
               COUNT(DISTINCT DATE(ts_utc)) AS stream_days
        FROM twitch_stats_tracked
        WHERE LOWER(streamer) = LOWER($1) AND ts_utc >= $2
    "#;
    let cur_row = sqlx::query(cur_sql)
        .bind(login)
        .bind(week_ago)
        .fetch_optional(pool)
        .await
        .ok()?;
    // prev braucht zusätzlich ts_utc < week_ago; eigene Query:
    let prev_sql = r#"
        SELECT AVG(viewer_count)::float8 AS avg_viewers,
               COUNT(DISTINCT DATE(ts_utc)) AS stream_days
        FROM twitch_stats_tracked
        WHERE LOWER(streamer) = LOWER($1) AND ts_utc >= $2 AND ts_utc < $3
    "#;
    let prev_row = sqlx::query(prev_sql)
        .bind(login)
        .bind(two_weeks_ago)
        .bind(week_ago)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

    let cur_avg = cur_row
        .as_ref()
        .map(|r| read_f64(r, "avg_viewers"))
        .unwrap_or(0.0);
    let cur_days = cur_row
        .as_ref()
        .map(|r| read_i64(r, "stream_days"))
        .unwrap_or(0);
    let prev_avg = prev_row
        .as_ref()
        .map(|r| read_f64(r, "avg_viewers"))
        .unwrap_or(0.0);

    // Growth (api_v2.py:275-279)
    let growth: i64 = if prev_avg > 0.0 {
        let growth_ratio = cur_avg / prev_avg;
        (50.0 + (growth_ratio - 1.0) * 100.0) as i64
    } else if cur_avg > 0.0 {
        50
    } else {
        0
    }
    .clamp(0, 100);

    // Retention
    let retention: i64 = (cur_days * 15).clamp(0, 100);

    // Engagement (api_v2.py:286-300)
    let mut engagement: i64 = 50;
    let chat_sql = r#"
        SELECT COUNT(*) AS c FROM twitch_chat_messages
        WHERE LOWER(streamer_login) = LOWER($1) AND message_ts >= $2
    "#;
    if let Ok(Some(row)) = sqlx::query(chat_sql)
        .bind(login)
        .bind(week_ago)
        .fetch_optional(pool)
        .await
    {
        let msg_count = row.try_get::<i64, _>("c").unwrap_or(0);
        if msg_count > 0 {
            let denom = if cur_avg < 1.0 { 1.0 } else { cur_avg };
            engagement = ((msg_count as f64 / denom * 2.0) as i64).clamp(0, 100);
        }
    }

    // Community: KNOWN_CHAT_BOTS-Filter (Python `build_known_chat_bot_not_in_clause`
    // mit bots=[*KNOWN_CHAT_BOTS, login]). Anonyme chatter_id-Zeilen ohne
    // chatter_login bleiben erhalten (NULL/'' werden NICHT gefiltert).
    let mut community: i64 = 0;
    let (bot_clause, bot_logins) = known_chat_bot_not_in_clause("sc.chatter_login", login, 3);
    let community_sql = format!(
        r#"
        SELECT
            COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) AS total_viewers,
            COUNT(DISTINCT CASE
                WHEN LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0'))
                     NOT IN ('1', 't', 'true')
                THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
            END) AS returning_viewers
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE LOWER(s.streamer_login) = LOWER($1)
          AND s.started_at >= $2
          AND {bot_clause}
    "#
    );
    let mut community_query = sqlx::query(&community_sql).bind(login).bind(week_ago);
    for bot in &bot_logins {
        community_query = community_query.bind(bot.clone());
    }
    if let Ok(Some(row)) = community_query.fetch_optional(pool).await {
        let total = read_i64(&row, "total_viewers");
        let returning = read_i64(&row, "returning_viewers");
        if total > 0 {
            community = ((returning as f64 / total as f64 * 100.0).round() as i64).clamp(0, 100);
        }
    }

    let overall = (growth as f64 * 0.30
        + retention as f64 * 0.25
        + engagement as f64 * 0.25
        + community as f64 * 0.20) as i64;

    let trend: Value = if prev_avg > 0.0 {
        json!(round1((cur_avg - prev_avg) / prev_avg * 100.0))
    } else {
        Value::Null
    };

    Some(json!({
        "overall": overall,
        "trend": trend,
        "sub_scores": {
            "growth": growth,
            "retention": retention,
            "engagement": engagement,
            "community": community,
        },
    }))
}

// ── Block 2h: week_comparison (api_v2.py:358-456) ────────────────────────────

async fn week_comparison_block(pool: &PgPool, resolved_login: &str) -> Value {
    if resolved_login.is_empty() {
        return Value::Null;
    }
    compute_week_comparison(pool, resolved_login).await
}

async fn week_stats(pool: &PgPool, login: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> Value {
    let stats_sql = r#"
        SELECT AVG(viewer_count)::float8 AS avg_v, (COUNT(*) * 15.0 / 3600)::float8 AS hours
        FROM twitch_stats_tracked
        WHERE LOWER(streamer) = LOWER($1) AND ts_utc >= $2 AND ts_utc < $3
    "#;
    let (avg_v, hours): (Value, Value) = match sqlx::query(stats_sql)
        .bind(login)
        .bind(start)
        .bind(end)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => {
            let a = row.try_get::<Option<f64>, _>("avg_v").unwrap_or(None);
            let h = row.try_get::<Option<f64>, _>("hours").unwrap_or(None);
            (
                a.filter(|v| *v != 0.0)
                    .map(|v| json!(round1(v)))
                    .unwrap_or(Value::Null),
                h.filter(|v| *v != 0.0)
                    .map(|v| json!(round1(v)))
                    .unwrap_or(Value::Null),
            )
        }
        _ => (Value::Null, Value::Null),
    };

    let foll_sql = r#"
        SELECT SUM(follower_delta)::bigint AS f
        FROM twitch_stream_sessions
        WHERE LOWER(streamer_login) = LOWER($1) AND started_at >= $2 AND started_at < $3
    "#;
    let followers: Value = match sqlx::query(foll_sql)
        .bind(login)
        .bind(start)
        .bind(end)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => row
            .try_get::<Option<i64>, _>("f")
            .unwrap_or(None)
            .filter(|v| *v != 0)
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
        _ => Value::Null,
    };

    json!({
        "avg_viewers": avg_v,
        "total_followers": followers,
        "chat_activity": Value::Null,
        "stream_hours": hours,
    })
}

async fn compute_week_comparison(pool: &PgPool, login: &str) -> Value {
    let now = Utc::now();
    let week_ago = now - Duration::days(7);
    let two_weeks_ago = now - Duration::days(14);

    let current = week_stats(pool, login, week_ago, now).await;
    let previous = week_stats(pool, login, two_weeks_ago, week_ago).await;

    let pct = |cur: &Value, prev: &Value| -> Value {
        match (cur.as_f64(), prev.as_f64()) {
            (Some(c), Some(p)) if p != 0.0 => json!(round1((c - p) / p * 100.0)),
            _ => Value::Null,
        }
    };

    let changes = json!({
        "avg_viewers_pct": pct(&current["avg_viewers"], &previous["avg_viewers"]),
        "followers_pct": pct(&current["total_followers"], &previous["total_followers"]),
        "chat_activity_pct": Value::Null,
        "stream_hours_pct": pct(&current["stream_hours"], &previous["stream_hours"]),
    });

    // daily_series: 7 Tage rückwärts (api_v2.py:410-449)
    let mut avg_series = vec![0.0_f64; 7];
    let mut foll_series = vec![0.0_f64; 7];
    let mut hours_series = vec![0.0_f64; 7];
    let chat_series = vec![0.0_f64; 7];

    for day_offset in 0..7usize {
        let day_start = (now - Duration::days(6 - day_offset as i64))
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|n| DateTime::<Utc>::from_naive_utc_and_offset(n, Utc));
        let Some(day_start) = day_start else { continue };
        let day_end = day_start + Duration::days(1);

        let day_sql = r#"
            SELECT AVG(viewer_count)::float8 AS avg_v, (COUNT(*) * 15.0 / 3600)::float8 AS hours
            FROM twitch_stats_tracked
            WHERE LOWER(streamer) = LOWER($1) AND ts_utc >= $2 AND ts_utc < $3
        "#;
        if let Ok(Some(row)) = sqlx::query(day_sql)
            .bind(login)
            .bind(day_start)
            .bind(day_end)
            .fetch_optional(pool)
            .await
        {
            if let Some(a) = row.try_get::<Option<f64>, _>("avg_v").unwrap_or(None) {
                if a != 0.0 {
                    avg_series[day_offset] = round1(a);
                }
            }
            if let Some(h) = row.try_get::<Option<f64>, _>("hours").unwrap_or(None) {
                if h != 0.0 {
                    hours_series[day_offset] = (h * 100.0).round() / 100.0;
                }
            }
        }

        let day_foll_sql = r#"
            SELECT SUM(follower_delta)::bigint AS f
            FROM twitch_stream_sessions
            WHERE LOWER(streamer_login) = LOWER($1) AND started_at >= $2 AND started_at < $3
        "#;
        if let Ok(Some(row)) = sqlx::query(day_foll_sql)
            .bind(login)
            .bind(day_start)
            .bind(day_end)
            .fetch_optional(pool)
            .await
        {
            if let Some(f) = row.try_get::<Option<i64>, _>("f").unwrap_or(None) {
                if f != 0 {
                    foll_series[day_offset] = f as f64;
                }
            }
        }
    }

    json!({
        "current_week": current,
        "previous_week": previous,
        "changes": changes,
        "daily_series": {
            "avg_viewers": avg_series,
            "followers": foll_series,
            "chat_activity": chat_series,
            "stream_hours": hours_series,
        },
    })
}

// ── Block 2i: live_status (internal_home.py:880-933) ─────────────────────────

async fn live_status_block(pool: &PgPool, resolved_login: &str, resolved_user_id: &str) -> Value {
    if resolved_login.is_empty() && resolved_user_id.is_empty() {
        return Value::Null;
    }

    let sql = r#"
        SELECT
            COALESCE(is_live, 0) AS is_live,
            last_started_at,
            last_seen_at,
            last_title,
            last_game,
            COALESCE(last_viewer_count, 0) AS viewer_count
        FROM twitch_live_state
        WHERE (COALESCE($1, '') != '' AND twitch_user_id = $1)
           OR (COALESCE($2, '') != '' AND LOWER(streamer_login) = LOWER($2))
        ORDER BY CASE
            WHEN (COALESCE($1, '') != '' AND twitch_user_id = $1) THEN 0
            ELSE 1
        END
        LIMIT 1
    "#;
    match sqlx::query(sql)
        .bind(resolved_user_id)
        .bind(resolved_login)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => {
            let is_live = read_truthy(&row, "is_live");
            let title = opt_str(&row, "last_title");
            let game = opt_str(&row, "last_game");
            json!({
                "is_live": is_live,
                "viewer_count": read_i64(&row, "viewer_count"),
                "started_at": if is_live { json!(row_ts_iso(&row, "last_started_at")) } else { Value::Null },
                "last_seen_at": row_ts_iso(&row, "last_seen_at"),
                "title": if title.is_empty() { Value::Null } else { json!(title) },
                "game": if game.is_empty() { Value::Null } else { json!(game) },
            })
        }
        Ok(None) => json!({
            "is_live": false,
            "viewer_count": 0,
            "started_at": Value::Null,
            "last_seen_at": Value::Null,
            "title": Value::Null,
            "game": Value::Null,
        }),
        Err(e) => {
            tracing::warn!("internal-home live-status query: {e}");
            Value::Null
        }
    }
}

// ── Changelog: Lese-Payload (api_v2.py:1496-1609) ────────────────────────────

async fn changelog_payload(pool: &PgPool, can_write: bool) -> Value {
    let entries = fetch_changelog_entries(pool).await;
    json!({
        "entries": entries,
        "can_write": can_write,
        "max_entries": CHANGELOG_MAX_ENTRIES,
    })
}

async fn fetch_changelog_entries(pool: &PgPool) -> Vec<Value> {
    let sql = r#"
        SELECT id, entry_date, title, content, created_at
        FROM internal_home_changelog
        ORDER BY entry_date DESC, created_at DESC, id DESC
        LIMIT $1
    "#;
    match sqlx::query(sql)
        .bind(CHANGELOG_MAX_ENTRIES)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows.iter().map(serialize_changelog_entry).collect(),
        Err(e) => {
            tracing::warn!("internal-home changelog fetch: {e}");
            Vec::new()
        }
    }
}

/// `_serialize_internal_home_changelog_entry` (api_v2.py:1564-1588).
fn serialize_changelog_entry(row: &PgRow) -> Value {
    let id = row.try_get::<Option<i64>, _>("id").unwrap_or(None);
    let entry_date = row
        .try_get::<Option<NaiveDate>, _>("entry_date")
        .unwrap_or(None)
        .map(|d| d.format("%Y-%m-%d").to_string());
    let created_at = row
        .try_get::<Option<DateTime<Utc>>, _>("created_at")
        .unwrap_or(None)
        .map(|c| c.to_rfc3339());
    json!({
        "id": id,
        "entry_date": entry_date,
        "title": opt_str(row, "title"),
        "content": opt_str(row, "content"),
        "created_at": created_at,
    })
}

// ── POST-Handler: changelog create (api_v2.py:2077-2189) ─────────────────────

/// `POST /twitch/api/v2/internal-home/changelog` — Admin or direct loopback only.
pub async fn changelog_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    parts: Parts,
    body: Option<Json<ChangelogBody>>,
) -> Response {
    // Auth (api_v2.py:1295-1315): none → 401, partner → 403, admin/direct loopback → ok.
    // P1.32: Same-Origin-CSRF-Schutz statt erzwungenem X-CSRF-Token-Header
    // (Vorfall #235). Direct loopback kennt keinen Browser-Cross-Site-Vektor
    // → Bypass; ein Browser-Admin (Cookie-Session) muss dagegen same-origin sein.
    // Nur ein nachweislich fremder Origin → 403.
    let is_local = is_local_request(&parts);
    if !is_local {
        match auth {
            DashboardAuthLevel::Admin { .. } => {}
            DashboardAuthLevel::None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "auth_required", "required": "admin" })),
                )
                    .into_response();
            }
            DashboardAuthLevel::Partner { .. } => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "admin_required",
                        "required": "admin",
                        "auth_level": "partner",
                    })),
                )
                    .into_response();
            }
        }
    }

    // Same-Origin-Guard für Browser-Admins (P1.32). Cross-Origin-POST → 403.
    if !is_local && !crate::auth::csrf::is_allowed_origin(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "invalid_csrf",
                "message": "Cross-Origin-Anfrage abgelehnt.",
            })),
        )
            .into_response();
    }

    let Some(Json(body)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_json", "message": "Request body must be valid JSON." })),
        )
            .into_response();
    };

    let title = body.title.unwrap_or_default().trim().to_string();
    let content = body.content.unwrap_or_default().trim().to_string();
    let raw_entry_date = body.entry_date.unwrap_or_default();
    let raw_entry_date = raw_entry_date.trim();

    if content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content_required", "message": "content is required." })),
        )
            .into_response();
    }
    if title.chars().count() > CHANGELOG_TITLE_MAX_LENGTH {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "title_too_long",
                "message": format!("title must be {CHANGELOG_TITLE_MAX_LENGTH} characters or fewer."),
            })),
        )
            .into_response();
    }
    if content.chars().count() > CHANGELOG_CONTENT_MAX_LENGTH {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "content_too_long",
                "message": format!("content must be {CHANGELOG_CONTENT_MAX_LENGTH} characters or fewer."),
            })),
        )
            .into_response();
    }

    let entry_date: NaiveDate = if raw_entry_date.is_empty() {
        Utc::now().date_naive()
    } else {
        match NaiveDate::parse_from_str(raw_entry_date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "invalid_entry_date",
                        "message": "entry_date must use YYYY-MM-DD.",
                    })),
                )
                    .into_response();
            }
        }
    };

    match create_changelog_entry(&pool, &title, &content, entry_date).await {
        Ok(entry) => (StatusCode::CREATED, Json(entry)).into_response(),
        Err(e) => {
            tracing::error!("internal-home changelog write: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "internal_home_changelog_write_failed",
                    "message": "Could not persist changelog entry.",
                })),
            )
                .into_response()
        }
    }
}

/// INSERT + Cap auf 20 (api_v2.py:1611-1642).
async fn create_changelog_entry(
    pool: &PgPool,
    title: &str,
    content: &str,
    entry_date: NaiveDate,
) -> Result<Value, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let row = sqlx::query!(
        r#"
        INSERT INTO internal_home_changelog (entry_date, title, content)
        VALUES ($1, $2, $3)
        RETURNING id, entry_date, title, content, created_at
        "#,
        entry_date,
        title,
        content
    )
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        DELETE FROM internal_home_changelog
        WHERE id IN (
            SELECT id FROM internal_home_changelog
            ORDER BY entry_date DESC, created_at DESC, id DESC
            OFFSET $1
        )
        "#,
        CHANGELOG_MAX_ENTRIES
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(json!({
        "id": row.id,
        "entry_date": row.entry_date.format("%Y-%m-%d").to_string(),
        "title": row.title,
        "content": row.content,
        "created_at": row.created_at.to_rfc3339(),
    }))
}

#[cfg(test)]
mod changelog_origin_tests {
    //! P1.32: Same-Origin-CSRF-Guard auf dem Changelog-POST. Browser-Admin mit
    //! gültiger `master_dash_session` aber Cross-Origin → 403 invalid_csrf;
    //! same-origin → kein invalid_csrf (passiert die Origin-Prüfung).
    use super::*;
    use crate::auth::session::{ADMIN_COOKIE_NAME, DashboardAuthState};
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use axum::{Extension, Router};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tower::ServiceExt;

    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE dashboard_sessions (
                session_id TEXT PRIMARY KEY, session_type TEXT NOT NULL,
                payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL,
                expires_at DOUBLE PRECISION NOT NULL)"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE internal_home_changelog (
                id BIGSERIAL PRIMARY KEY,
                entry_date DATE NOT NULL DEFAULT CURRENT_DATE,
                title TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL DEFAULT '',
                created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE INDEX idx_internal_home_changelog_order \
             ON internal_home_changelog (entry_date DESC, created_at DESC, id DESC)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    /// Baut einen Router mit dem changelog_handler + Auth-State und einer gültigen
    /// Admin-Session; gibt (App, Admin-Cookie-Value) zurück.
    async fn app_with_admin(pool: PgPool) -> (Router, String) {
        let auth_state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let created = auth_state
            .create_admin_session("admin-user-1", "Admin")
            .await
            .expect("admin session");
        let app = Router::new()
            .route("/changelog", post(super::changelog_handler))
            .layer(Extension(auth_state))
            .with_state(pool);
        (app, created.session_id)
    }

    fn admin_request(cookie_value: &str, origin: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/changelog")
            // Nicht-Loopback-Host erzwingen, sonst greift der Localhost-Bypass.
            .header("host", "dash.example.com")
            .header("content-type", "application/json")
            .header(
                axum::http::header::COOKIE,
                format!("{ADMIN_COOKIE_NAME}={cookie_value}"),
            );
        if let Some(o) = origin {
            builder = builder.header(axum::http::header::ORIGIN, o);
        }
        builder
            .body(Body::from(r#"{"title":"T","content":"C"}"#))
            .unwrap()
    }

    #[tokio::test]
    async fn browser_admin_cross_origin_403() {
        let Some(pool) = make_pool("t_changelog_xorigin").await else {
            return;
        };
        let (app, cookie) = app_with_admin(pool).await;
        let resp = app
            .oneshot(admin_request(&cookie, Some("https://evil.example.org")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 16)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "invalid_csrf");
    }

    #[tokio::test]
    async fn browser_admin_same_origin_passiert_csrf_gate() {
        let Some(pool) = make_pool("t_changelog_sameorigin").await else {
            return;
        };
        let (app, cookie) = app_with_admin(pool).await;
        let resp = app
            .oneshot(admin_request(&cookie, Some("https://dash.example.com")))
            .await
            .unwrap();
        // Same-origin darf NICHT am invalid_csrf scheitern (kein 403 invalid_csrf).
        // Der Write läuft danach gegen die Test-Fixture → 201.
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
}

#[cfg(test)]
mod identity_tests {
    //! B16-FIX-INTERNALHOME-DISPLAYNAME: `resolve_identity` muss den echten
    //! Twitch-display_name aus der Partner-Session liefern (nicht den Login).
    use super::*;
    use crate::auth::level::AdminActor;

    fn partner(login: &str, display: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: "42".into(),
            display_name: display.into(),
        }
    }

    #[test]
    fn partner_eigener_display_name() {
        let id = resolve_identity(&partner("nani", "NaNiAdm"), &None, false).unwrap();
        assert_eq!(id.twitch_login, "nani");
        assert_eq!(id.display_name, "NaNiAdm");
    }

    #[test]
    fn partner_leerer_display_name_faellt_auf_login() {
        let id = resolve_identity(&partner("nani", "  "), &None, false).unwrap();
        assert_eq!(id.display_name, "nani");
    }

    #[test]
    fn partner_eigener_override_behaelt_display_name() {
        // Override == eigener Login → echter display_name bleibt erhalten.
        let id =
            resolve_identity(&partner("nani", "NaNiAdm"), &Some("NANI".into()), false).unwrap();
        assert_eq!(id.twitch_login, "nani");
        assert_eq!(id.display_name, "NaNiAdm");
    }

    #[test]
    fn twitch_admin_ohne_override_nutzt_actor_identitaet() {
        let auth = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "42".into(),
                twitch_login: "EarlySalty".into(),
            }),
        };

        let id = resolve_identity(&auth, &None, false).unwrap();

        assert_eq!(id.twitch_login, "earlysalty");
        assert_eq!(id.twitch_user_id, "42");
        assert_eq!(id.display_name, "EarlySalty");
    }

    #[test]
    fn twitch_admin_mit_override_nutzt_gewaehlten_streamer() {
        let auth = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "42".into(),
                twitch_login: "earlysalty".into(),
            }),
        };

        let id = resolve_identity(&auth, &Some("AndererPartner".into()), false).unwrap();

        assert_eq!(id.twitch_login, "andererpartner");
        assert!(id.twitch_user_id.is_empty());
        assert_eq!(id.display_name, "andererpartner");
    }

    #[test]
    fn discord_admin_in_public_user_view_nutzt_owner_identitaet() {
        let id = resolve_identity(&DashboardAuthLevel::admin(), &None, true).unwrap();

        assert_eq!(id.twitch_login, "earlysalty");
        assert!(id.twitch_user_id.is_empty());
        assert_eq!(id.display_name, "earlysalty");
    }
}

#[cfg(test)]
mod community_bot_filter_tests {
    //! P2.83: KNOWN_CHAT_BOTS-Filter im Community-Sub-Score (Parität zu
    //! `build_known_chat_bot_not_in_clause`).
    use super::*;

    #[test]
    fn clause_enthaelt_alle_bots_und_eigenen_login_sortiert() {
        let (clause, logins) = known_chat_bot_not_in_clause("sc.chatter_login", "Nani", 3);
        // Eigener Login (lowercased) + alle Bots, dedupliziert + sortiert.
        assert!(logins.contains(&"nani".to_string()));
        assert!(logins.contains(&"nightbot".to_string()));
        assert!(logins.contains(&"streamelements".to_string()));
        assert_eq!(logins.len(), KNOWN_CHAT_BOTS.len() + 1);
        let mut sorted = logins.clone();
        sorted.sort();
        assert_eq!(logins, sorted, "Logins müssen sortiert sein");
        // NULL/''-Erhalt + NOT IN-Klausel mit $3-startenden Platzhaltern.
        assert!(clause.contains("(sc.chatter_login) IS NULL"));
        assert!(clause.contains("(sc.chatter_login) = ''"));
        assert!(clause.contains("LOWER(sc.chatter_login) NOT IN ($3,"));
    }

    #[test]
    fn eigener_login_bereits_in_bots_keine_dopplung() {
        // "nightbot" ist bereits ein Bot; als own_login darf er nicht doppeln.
        let (_clause, logins) = known_chat_bot_not_in_clause("sc.chatter_login", "nightbot", 3);
        assert_eq!(logins.len(), KNOWN_CHAT_BOTS.len());
    }

    #[test]
    fn leerer_login_nur_bots() {
        let (_clause, logins) = known_chat_bot_not_in_clause("sc.chatter_login", "  ", 3);
        assert_eq!(logins.len(), KNOWN_CHAT_BOTS.len());
    }
}

//! Admin-Streamer-Queries: List + Detail + Stats/Sessions.
//!
//! Alle drei async-Funktionen greifen direkt auf den DB-Pool zu.
//! Der WHERE-Zweig in `list_streamers` wird anhand des `StreamerView`-Enums
//! als statischer String gewählt — kein Nutzereingabe-Wert fließt in den SQL-String.

// ── Konstanten ────────────────────────────────────────────────────────────────

pub const REQUIRED_SCOPES: &[&str] = &[
    "channel:manage:raids",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:redemptions",
];

// ── ScopeSnapshot ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScopeSnapshot {
    pub granted_scopes: Vec<String>,
    pub missing_scopes: Vec<String>,
    pub connected: bool,
    pub needs_reauth: bool,
    pub status: &'static str, // "reauth" | "missing" | "partial" | "connected"
}

/// Berechnet OAuth-Scope-Status aus rohem Scope-String und reauth-Flag.
///
/// `scopes_raw`: Space-separierter String aus der DB, kann None sein.
/// `needs_reauth`: DB-Wert aus `twitch_raid_auth.needs_reauth` (BOOLEAN).
pub fn scope_snapshot(scopes_raw: Option<&str>, needs_reauth: bool) -> ScopeSnapshot {
    let mut granted: Vec<String> = scopes_raw
        .unwrap_or("")
        .split_whitespace()
        .map(|s| s.trim().to_lowercase())
        .collect();
    granted.sort();
    granted.dedup();

    let missing: Vec<String> = REQUIRED_SCOPES
        .iter()
        .filter(|&&req| !granted.iter().any(|g| g == req))
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

    ScopeSnapshot {
        granted_scopes: granted,
        missing_scopes: missing,
        connected,
        needs_reauth,
        status,
    }
}

// ── partner_status ────────────────────────────────────────────────────────────

/// Berechnet den logischen Partner-Status aus DB-Feldern.
///
/// Reihenfolge der Prüfungen ist fix — blocked hat höchste Priorität.
///
/// `archived_at` ist in Prod TEXT — hier als `Option<&str>` übergeben.
pub fn partner_status(
    status: Option<&str>,
    archived_at: Option<&str>,
    manual_partner_opt_out: i32,
    technical_pause_reason: Option<&str>,
) -> &'static str {
    let pause = technical_pause_reason.unwrap_or("").trim().to_lowercase();

    if pause == "blocked" {
        return "blocked";
    }
    if manual_partner_opt_out != 0 {
        return "non_partner";
    }
    if pause == "token_error" {
        return "token_error";
    }

    let s = status.unwrap_or("").trim().to_lowercase();
    // archived_at ist ein TEXT-Timestamp aus Prod — non-empty gilt als gesetzt
    if s == "archived" || archived_at.is_some_and(|v| !v.trim().is_empty()) {
        return "archived";
    }
    if s == "departnered" {
        return "departnered";
    }
    if s == "active" {
        return "active";
    }

    "non_partner"
}

// ── StreamerView ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamerView {
    Active,
    Archived,
    Departnered,
    Blocked,
    NonPartner,
    TokenError,
    All,
}

impl StreamerView {
    /// Parst den Query-Parameter-String inkl. Default-Verhalten.
    ///
    /// Parität Python `_admin_parse_streamer_view` (api_admin.py:1000-1006):
    /// fehlender/leerer View → `Active` (nicht `All`). Unbekannte Werte → `None`,
    /// damit der Handler einen 400 zurückgeben kann.
    pub fn parse_or_default(s: Option<&str>) -> Option<Self> {
        match s.map(str::trim).unwrap_or("") {
            "" => Some(Self::Active),
            other => Self::parse(other),
        }
    }

    /// Parst den Query-Parameter-String. Case-insensitive. Gibt `None` bei unbekanntem Wert.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            "departnered" => Some(Self::Departnered),
            "blocked" => Some(Self::Blocked),
            "non_partner" => Some(Self::NonPartner),
            "token_error" => Some(Self::TokenError),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    /// Kanonischer View-Name (für das `view`-Feld der Response).
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Departnered => "departnered",
            Self::Blocked => "blocked",
            Self::NonPartner => "non_partner",
            Self::TokenError => "token_error",
            Self::All => "all",
        }
    }

    /// Gibt alle unterstützten View-Namen zurück (für 400-Fehler-Body).
    pub fn all_names() -> &'static [&'static str] {
        &[
            "active",
            "archived",
            "departnered",
            "blocked",
            "non_partner",
            "token_error",
            "all",
        ]
    }

    /// Gibt den WHERE-Ausdruck für diesen View zurück.
    /// Kein Binding nötig — alle Bedingungen sind gegen DB-Felder, kein Nutzereingabe-Wert.
    fn where_clause(self) -> &'static str {
        match self {
            // active: status='active', nicht archiviert, kein opt-out, kein token_error
            Self::Active => {
                "COALESCE(s.manual_partner_opt_out, 0) = 0 \
                 AND COALESCE(s.status, '') = 'active' \
                 AND s.archived_at IS NULL \
                 AND LOWER(COALESCE(s.technical_pause_reason, '')) <> 'token_error'"
            }
            // archived: kein opt-out UND (status=active+archiviert ODER status=archived)
            Self::Archived => {
                "COALESCE(s.manual_partner_opt_out, 0) = 0 \
                 AND (\
                   (COALESCE(s.status, '') = 'active' AND s.archived_at IS NOT NULL) \
                   OR COALESCE(s.status, '') = 'archived'\
                 )"
            }
            Self::Departnered => {
                "COALESCE(s.manual_partner_opt_out, 0) = 0 \
                 AND COALESCE(s.status, '') = 'departnered'"
            }
            Self::Blocked => "LOWER(COALESCE(s.technical_pause_reason, '')) = 'blocked'",
            Self::NonPartner => {
                "COALESCE(s.manual_partner_opt_out, 0) = 1 \
                 AND LOWER(COALESCE(s.technical_pause_reason, '')) <> 'blocked'"
            }
            Self::TokenError => {
                "COALESCE(s.manual_partner_opt_out, 0) = 0 \
                 AND LOWER(COALESCE(s.technical_pause_reason, '')) = 'token_error'"
            }
            Self::All => "1=1",
        }
    }
}

// ── DB-Row-Typen ──────────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
pub struct AdminStreamerRow {
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    /// TEXT in Prod (twitch_partners_all_state.created_at)
    pub created_at: Option<String>,
    /// TEXT in Prod (twitch_partners_all_state.archived_at)
    pub archived_at: Option<String>,
    pub require_discord_link: Option<i32>,
    pub is_on_discord: Option<i32>,
    pub manual_partner_opt_out: Option<i32>,
    pub status: Option<String>,
    pub raid_bot_enabled: Option<i32>,
    pub silent_ban: Option<i32>,
    pub silent_raid: Option<i32>,
    pub is_monitored_only: Option<i32>,
    pub is_verified: i32,
    pub is_partner_active: i32,
    pub is_live: i32,
    /// TEXT in Prod (twitch_live_state.last_seen_at)
    pub last_seen_at: Option<String>,
    /// int4 in Prod (twitch_live_state.last_viewer_count)
    pub last_viewer_count: Option<i32>,
    pub active_session_id: Option<i64>,
    pub last_game: Option<String>,
    /// TIMESTAMPTZ (twitch_stream_sessions.ended_at/started_at)
    pub last_stream_at: Option<chrono::DateTime<chrono::Utc>>,
    pub scopes: Option<String>,
    pub needs_reauth: Option<bool>,
    /// TIMESTAMPTZ (twitch_raid_auth.authorized_at)
    pub authorized_at: Option<chrono::DateTime<chrono::Utc>>,
    pub promo_disabled: Option<i32>,
    pub promo_message: Option<String>,
    pub raid_boost_enabled: Option<i32>,
    pub manual_plan_id: Option<String>,
    pub manual_plan_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub manual_plan_notes: Option<String>,
    pub billing_plan_id: Option<String>,
    pub billing_status: Option<String>,
    pub billing_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub technical_pause_reason: Option<String>,
    pub operational_state: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AdminStreamerDetailRow {
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    /// TEXT in Prod (twitch_partners_all_state.created_at)
    pub created_at: Option<String>,
    /// TEXT in Prod (twitch_partners_all_state.archived_at)
    pub archived_at: Option<String>,
    pub require_discord_link: Option<i32>,
    pub is_on_discord: Option<i32>,
    pub manual_partner_opt_out: Option<i32>,
    pub raid_bot_enabled: Option<i32>,
    pub silent_ban: Option<i32>,
    pub silent_raid: Option<i32>,
    pub is_monitored_only: Option<i32>,
    pub is_verified: i32,
    pub is_partner_active: i32,
    pub live_ping_enabled: i32,
    pub status: Option<String>,
    pub is_live: i32,
    /// TEXT in Prod (twitch_live_state.last_seen_at)
    pub last_seen_at: Option<String>,
    /// int4 in Prod (twitch_live_state.last_viewer_count)
    pub last_viewer_count: Option<i32>,
    pub active_session_id: Option<i64>,
    /// TEXT in Prod (twitch_live_state.last_started_at)
    pub last_started_at: Option<String>,
    pub last_game: Option<String>,
    pub scopes: Option<String>,
    pub needs_reauth: Option<bool>,
    pub oauth_raid_enabled: Option<bool>,
    /// TIMESTAMPTZ (twitch_raid_auth.authorized_at)
    pub authorized_at: Option<chrono::DateTime<chrono::Utc>>,
    pub plan_name: Option<String>,
    pub promo_disabled: Option<i32>,
    pub promo_message: Option<String>,
    pub raid_boost_enabled: Option<i32>,
    pub notes: Option<String>,
    pub manual_plan_id: Option<String>,
    pub manual_plan_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub manual_plan_notes: Option<String>,
    pub billing_plan_id: Option<String>,
    pub billing_status: Option<String>,
    pub billing_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub technical_pause_reason: Option<String>,
    pub operational_state: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct StreamerStatsRow {
    pub total_sessions: i64,
    pub total_duration_seconds: i64,
    pub avg_viewers: f64,
    pub peak_viewers: i64,
    pub follower_delta: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct StreamerSessionRow {
    pub id: i64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub stream_title: Option<String>,
    pub game_name: Option<String>,
    pub avg_viewers: Option<f64>,
    pub peak_viewers: Option<i64>,
    pub duration_seconds: Option<i64>,
    pub follower_delta: Option<i64>,
}

// ── CTE-Konstanten ────────────────────────────────────────────────────────────

const CTE_LATEST_BILLING: &str = r#"latest_billing AS (
    SELECT customer_reference, plan_id, status, updated_at,
        ROW_NUMBER() OVER (PARTITION BY LOWER(customer_reference) ORDER BY updated_at DESC) AS rn
    FROM twitch_billing_subscriptions
)"#;

const CTE_PARTNER_STATE: &str = r#", partner_state AS (
    SELECT twitch_login, twitch_user_id, require_discord_link, discord_user_id, discord_display_name,
        is_on_discord, manual_partner_opt_out, created_at, archived_at, raid_bot_enabled, silent_ban,
        silent_raid, is_monitored_only, is_verified, is_partner_active, live_ping_enabled, status,
        technical_pause_reason, operational_state
    FROM (
        SELECT s.twitch_login, s.twitch_user_id, s.require_discord_link, s.discord_user_id,
            s.discord_display_name, s.is_on_discord, s.manual_partner_opt_out, s.created_at,
            s.archived_at, s.raid_bot_enabled, s.silent_ban, s.silent_raid,
            CASE WHEN NOT EXISTS (
                SELECT 1 FROM twitch_partners p
                WHERE p.twitch_user_id = s.twitch_user_id
                   OR LOWER(p.twitch_login) = LOWER(s.twitch_login)
            ) THEN 1 ELSE 0 END AS is_monitored_only,
            s.is_verified, s.is_partner_active, s.live_ping_enabled, s.status,
            s.technical_pause_reason, s.operational_state,
            ROW_NUMBER() OVER (
                PARTITION BY LOWER(s.twitch_login)
                ORDER BY
                    CASE WHEN s.status = 'active' THEN 0 ELSE 1 END,
                    CASE WHEN s.created_at IS NULL AND s.archived_at IS NULL THEN 1 ELSE 0 END,
                    CASE WHEN s.created_at IS NOT NULL THEN s.created_at ELSE s.archived_at END DESC,
                    CASE WHEN s.archived_at IS NULL THEN 1 ELSE 0 END,
                    s.archived_at DESC,
                    LOWER(s.twitch_login) ASC
            ) AS rn
        FROM twitch_partners_all_state s
        WHERE COALESCE(TRIM(s.twitch_login), '') <> ''
    ) ranked_partner_state
    WHERE rn = 1
)"#;

const CTE_PARTNER_LIVE_STATE: &str = r#", partner_live_state AS (
    SELECT partner_login, twitch_user_id, streamer_login, is_live, last_seen_at,
        last_viewer_count, active_session_id, last_started_at, last_game
    FROM (
        SELECT s.twitch_login AS partner_login, l.twitch_user_id, l.streamer_login, l.is_live,
            l.last_seen_at, l.last_viewer_count, l.active_session_id, l.last_started_at, l.last_game,
            ROW_NUMBER() OVER (
                PARTITION BY LOWER(s.twitch_login)
                ORDER BY
                    CASE
                        WHEN NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                         AND NULLIF(TRIM(COALESCE(l.twitch_user_id, '')), '') IS NOT NULL
                         AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(l.twitch_user_id))
                        THEN 0
                        WHEN LOWER(COALESCE(l.twitch_user_id, '')) = LOWER(COALESCE(l.streamer_login, ''))
                        THEN 2
                        ELSE 1
                    END,
                    CASE WHEN COALESCE(l.is_live, 0) = 1 THEN 0 ELSE 1 END,
                    CASE WHEN l.last_seen_at IS NULL AND l.last_started_at IS NULL THEN 1 ELSE 0 END,
                    CASE WHEN l.last_seen_at IS NOT NULL THEN l.last_seen_at ELSE l.last_started_at END DESC
            ) AS rn
        FROM partner_state s
        LEFT JOIN twitch_live_state l
            ON (
                NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                AND NULLIF(TRIM(COALESCE(l.twitch_user_id, '')), '') IS NOT NULL
                AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(l.twitch_user_id))
            )
            OR LOWER(s.twitch_login) = LOWER(l.streamer_login)
    ) ranked_partner_live_state
    WHERE rn = 1
)"#;

const CTE_PARTNER_OAUTH: &str = r#", partner_oauth AS (
    SELECT partner_login, scopes, needs_reauth, raid_enabled, authorized_at
    FROM (
        SELECT s.twitch_login AS partner_login, a.scopes, a.needs_reauth, a.raid_enabled, a.authorized_at,
            ROW_NUMBER() OVER (
                PARTITION BY LOWER(s.twitch_login)
                ORDER BY
                    CASE
                        WHEN NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                         AND NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                         AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(a.twitch_user_id))
                        THEN 0
                        WHEN LOWER(COALESCE(a.twitch_login, '')) = LOWER(s.twitch_login) THEN 1
                        ELSE 2
                    END,
                    CASE WHEN a.authorized_at IS NULL THEN 1 ELSE 0 END,
                    a.authorized_at DESC
            ) AS rn
        FROM partner_state s
        LEFT JOIN twitch_raid_auth a
            ON (
                NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                AND NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(a.twitch_user_id))
            )
            OR LOWER(COALESCE(a.twitch_login, '')) = LOWER(s.twitch_login)
    ) ranked_oauth
    WHERE rn = 1
)"#;

const CTE_LAST_STREAM_SESSION: &str = r#", last_stream_session AS (
    SELECT LOWER(streamer_login) AS streamer_login, MAX(COALESCE(ended_at, started_at)) AS last_stream_at
    FROM twitch_stream_sessions
    GROUP BY LOWER(streamer_login)
)"#;

// ── Query-Funktionen ──────────────────────────────────────────────────────────

/// Holt alle Streamer für den angegebenen View.
///
/// Der WHERE-Zweig wird anhand des `view`-Enums als statischer String gewählt —
/// kein Nutzereingabe-Wert fließt in den SQL-String, alle Bindings kommen via `.bind()`.
pub async fn list_streamers(
    pool: &sqlx::PgPool,
    view: StreamerView,
) -> Result<Vec<AdminStreamerRow>, sqlx::Error> {
    let where_clause = view.where_clause();
    let sql = format!(
        r#"WITH {cte_billing}
{cte_partner_state}
{cte_live_state}
{cte_oauth}
{cte_last_session}
SELECT
    s.twitch_login, s.twitch_user_id, s.discord_user_id, s.discord_display_name,
    s.created_at, s.archived_at, s.require_discord_link, s.is_on_discord, s.manual_partner_opt_out,
    s.status, s.raid_bot_enabled, s.silent_ban, s.silent_raid, s.is_monitored_only,
    COALESCE(s.is_verified, 0) AS is_verified, COALESCE(s.is_partner_active, 0) AS is_partner_active,
    COALESCE(pls.is_live, 0) AS is_live, pls.last_seen_at, pls.last_viewer_count,
    pls.active_session_id, pls.last_game, lss.last_stream_at,
    po.scopes, po.needs_reauth, po.authorized_at,
    sp.promo_disabled, sp.promo_message, sp.raid_boost_enabled,
    sp.manual_plan_id, sp.manual_plan_expires_at, sp.manual_plan_notes,
    lb.plan_id AS billing_plan_id, lb.status AS billing_status, lb.updated_at AS billing_updated_at,
    s.technical_pause_reason, s.operational_state
FROM partner_state s
LEFT JOIN partner_live_state pls ON LOWER(pls.partner_login) = LOWER(s.twitch_login)
LEFT JOIN partner_oauth po ON LOWER(po.partner_login) = LOWER(s.twitch_login)
LEFT JOIN last_stream_session lss ON lss.streamer_login = LOWER(s.twitch_login)
LEFT JOIN streamer_plans sp ON LOWER(sp.twitch_login) = LOWER(s.twitch_login)
LEFT JOIN latest_billing lb ON LOWER(lb.customer_reference) = LOWER(s.twitch_login) AND lb.rn = 1
WHERE {where}
ORDER BY
    CASE WHEN LOWER(COALESCE(s.technical_pause_reason, '')) = 'blocked' THEN 2
         WHEN COALESCE(s.manual_partner_opt_out, 0) = 1 THEN 3
         WHEN LOWER(COALESCE(s.technical_pause_reason, '')) = 'token_error' THEN 2
         WHEN COALESCE(s.status, 'departnered') = 'active' AND s.archived_at IS NULL THEN 0
         WHEN COALESCE(s.status, 'departnered') IN ('active', 'archived') THEN 1
         ELSE 4 END,
    CASE WHEN COALESCE(pls.is_live, 0) = 1 THEN 0 ELSE 1 END,
    LOWER(s.twitch_login) ASC"#,
        cte_billing = CTE_LATEST_BILLING,
        cte_partner_state = CTE_PARTNER_STATE,
        cte_live_state = CTE_PARTNER_LIVE_STATE,
        cte_oauth = CTE_PARTNER_OAUTH,
        cte_last_session = CTE_LAST_STREAM_SESSION,
        where = where_clause,
    );

    sqlx::query_as(&sql).fetch_all(pool).await
}

/// Holt Detail-Row für einen einzelnen Streamer (case-insensitive Login-Suche).
///
/// Gibt `None` zurück wenn kein Streamer mit diesem Login gefunden wird.
pub async fn streamer_detail(
    pool: &sqlx::PgPool,
    login: &str,
) -> Result<Option<AdminStreamerDetailRow>, sqlx::Error> {
    let sql = format!(
        r#"WITH {cte_billing}
{cte_partner_state}
{cte_live_state}
{cte_oauth}
SELECT
    s.twitch_login, s.twitch_user_id, s.discord_user_id, s.discord_display_name,
    s.created_at, s.archived_at, s.require_discord_link, s.is_on_discord, s.manual_partner_opt_out,
    s.raid_bot_enabled, s.silent_ban, s.silent_raid, s.is_monitored_only,
    COALESCE(s.is_verified, 0) AS is_verified, COALESCE(s.is_partner_active, 0) AS is_partner_active,
    COALESCE(s.live_ping_enabled, 1) AS live_ping_enabled, s.status,
    COALESCE(pls.is_live, 0) AS is_live, pls.last_seen_at, pls.last_viewer_count,
    pls.active_session_id, pls.last_started_at, pls.last_game,
    po.scopes, po.needs_reauth, po.raid_enabled AS oauth_raid_enabled, po.authorized_at,
    sp.plan_name, sp.promo_disabled, sp.promo_message, sp.raid_boost_enabled, sp.notes,
    sp.manual_plan_id, sp.manual_plan_expires_at, sp.manual_plan_notes,
    lb.plan_id AS billing_plan_id, lb.status AS billing_status, lb.updated_at AS billing_updated_at,
    s.technical_pause_reason, s.operational_state
FROM partner_state s
LEFT JOIN partner_live_state pls ON LOWER(pls.partner_login) = LOWER(s.twitch_login)
LEFT JOIN partner_oauth po ON LOWER(po.partner_login) = LOWER(s.twitch_login)
LEFT JOIN streamer_plans sp ON LOWER(sp.twitch_login) = LOWER(s.twitch_login)
LEFT JOIN latest_billing lb ON LOWER(lb.customer_reference) = LOWER(s.twitch_login) AND lb.rn = 1
WHERE LOWER(s.twitch_login) = LOWER($1)
LIMIT 1"#,
        cte_billing = CTE_LATEST_BILLING,
        cte_partner_state = CTE_PARTNER_STATE,
        cte_live_state = CTE_PARTNER_LIVE_STATE,
        cte_oauth = CTE_PARTNER_OAUTH,
    );

    sqlx::query_as(&sql).bind(login).fetch_optional(pool).await
}

/// Findet den Twitch-Login zu einer Discord-User-ID (kanonische Lookup-Quelle).
///
/// Spiegelt die Lookup-Logik des Raid-OAuth-Pfads: die jüngste Erstellung
/// gewinnt. `None` = kein verknüpfter Account.
/// Nur lesend; ein Statement gegen die Partner-State-View.
pub async fn login_for_discord_user(
    pool: &sqlx::PgPool,
    discord_user_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        r#"
        SELECT twitch_login
        FROM twitch_streamers_partner_state
        WHERE discord_user_id = $1
        ORDER BY
            CASE WHEN created_at IS NULL THEN 1 ELSE 0 END,
            created_at DESC
        LIMIT 1
        "#,
    )
    .bind(discord_user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(login,)| login))
}

/// Holt Statistik-Aggregat und letzte 10 Sessions für einen Streamer.
///
/// Beide Queries laufen sequentiell auf derselben Verbindung.
pub async fn streamer_stats_and_sessions(
    pool: &sqlx::PgPool,
    login: &str,
) -> Result<(StreamerStatsRow, Vec<StreamerSessionRow>), sqlx::Error> {
    let stats: StreamerStatsRow = sqlx::query_as(
        r#"SELECT
            COUNT(*) AS total_sessions,
            COALESCE(SUM(duration_seconds), 0)::BIGINT AS total_duration_seconds,
            COALESCE(AVG(avg_viewers), 0.0)::FLOAT8 AS avg_viewers,
            COALESCE(MAX(peak_viewers), 0)::BIGINT AS peak_viewers,
            COALESCE(SUM(follower_delta), 0)::BIGINT AS follower_delta
        FROM twitch_stream_sessions
        WHERE LOWER(streamer_login) = LOWER($1)"#,
    )
    .bind(login)
    .fetch_one(pool)
    .await?;

    let sessions: Vec<StreamerSessionRow> = sqlx::query_as(
        r#"SELECT id, started_at, ended_at, stream_title, game_name,
            avg_viewers, peak_viewers, duration_seconds, follower_delta
        FROM twitch_stream_sessions
        WHERE LOWER(streamer_login) = LOWER($1)
        ORDER BY started_at DESC
        LIMIT 10"#,
    )
    .bind(login)
    .fetch_all(pool)
    .await?;

    Ok((stats, sessions))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> sqlx::PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        // twitch_partners_all_state — Prod-Typen: created_at/archived_at sind TEXT
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners_all_state (
                id                       BIGSERIAL PRIMARY KEY,
                twitch_login             TEXT NOT NULL,
                twitch_user_id           TEXT,
                discord_user_id          TEXT,
                discord_display_name     TEXT,
                created_at               TEXT,
                archived_at              TEXT,
                require_discord_link     INTEGER NOT NULL DEFAULT 0,
                is_on_discord            INTEGER NOT NULL DEFAULT 0,
                manual_partner_opt_out   INTEGER NOT NULL DEFAULT 0,
                status                   TEXT,
                raid_bot_enabled         INTEGER NOT NULL DEFAULT 1,
                silent_ban               INTEGER NOT NULL DEFAULT 0,
                silent_raid              INTEGER NOT NULL DEFAULT 0,
                is_monitored_only        INTEGER NOT NULL DEFAULT 0,
                is_verified              INTEGER NOT NULL DEFAULT 0,
                is_partner_active        INTEGER NOT NULL DEFAULT 1,
                live_ping_enabled        INTEGER NOT NULL DEFAULT 1,
                technical_pause_reason   TEXT,
                operational_state        TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL partners_all_state");

        // Prod-Typen: last_seen_at/last_started_at sind TEXT, last_viewer_count ist INTEGER (int4)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login   TEXT PRIMARY KEY,
                twitch_user_id   TEXT,
                is_live          INTEGER NOT NULL DEFAULT 0,
                last_seen_at     TEXT,
                last_started_at  TEXT,
                last_viewer_count INTEGER,
                active_session_id BIGINT,
                last_game        TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL live_state");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_auth (
                id              BIGSERIAL PRIMARY KEY,
                twitch_login    TEXT,
                twitch_user_id  TEXT,
                scopes          TEXT,
                needs_reauth    BOOLEAN NOT NULL DEFAULT FALSE,
                raid_enabled    BOOLEAN NOT NULL DEFAULT TRUE,
                authorized_at   TIMESTAMPTZ
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL raid_auth");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_billing_subscriptions (
                id                   BIGSERIAL PRIMARY KEY,
                customer_reference   TEXT NOT NULL,
                plan_id              TEXT,
                status               TEXT,
                updated_at           TIMESTAMPTZ
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL billing");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS streamer_plans (
                twitch_login         TEXT PRIMARY KEY,
                plan_name            TEXT,
                promo_disabled       INTEGER NOT NULL DEFAULT 0,
                promo_message        TEXT,
                raid_boost_enabled   INTEGER NOT NULL DEFAULT 0,
                notes                TEXT,
                manual_plan_id       TEXT,
                manual_plan_expires_at TIMESTAMPTZ,
                manual_plan_notes    TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL streamer_plans");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id               BIGSERIAL PRIMARY KEY,
                streamer_login   TEXT NOT NULL,
                started_at       TIMESTAMPTZ NOT NULL,
                ended_at         TIMESTAMPTZ,
                stream_title     TEXT,
                game_name        TEXT,
                avg_viewers      DOUBLE PRECISION,
                peak_viewers     BIGINT,
                duration_seconds BIGINT,
                follower_delta   BIGINT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL stream_sessions");

        // twitch_partners — Basistabelle, gegen die list_streamers/streamer_detail per
        // NOT EXISTS prüfen (is_monitored_only via twitch_user_id/twitch_login). In Prod
        // eigene Tabelle; twitch_partners_all_state ist die View darüber. Die Query
        // referenziert beide, also muss die Fixture beide bereitstellen.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS twitch_partners (twitch_user_id TEXT, twitch_login TEXT)",
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_partners");

        sqlx::query(
            "TRUNCATE twitch_partners_all_state, twitch_partners, twitch_live_state, twitch_raid_auth, \
             twitch_billing_subscriptions, streamer_plans, twitch_stream_sessions",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");

        pool
    }

    // --- Hilfsfunktionen ---

    #[test]
    fn scope_snapshot_connected() {
        let snap = scope_snapshot(
            Some(
                "channel:manage:raids channel:manage:moderators channel:bot clips:edit channel:read:ads bits:read channel:read:redemptions",
            ),
            false,
        );
        assert_eq!(snap.status, "connected");
        assert!(snap.missing_scopes.is_empty());
        assert!(snap.connected);
        assert!(!snap.needs_reauth);
    }

    #[test]
    fn scope_snapshot_partial() {
        let snap = scope_snapshot(Some("channel:manage:raids bits:read"), false);
        assert_eq!(snap.status, "partial");
        assert!(!snap.missing_scopes.is_empty());
    }

    #[test]
    fn scope_snapshot_reauth_vorrang() {
        // Selbst wenn vollständig: reauth-Flag dominiert
        let snap = scope_snapshot(
            Some(
                "channel:manage:raids channel:manage:moderators channel:bot clips:edit channel:read:ads bits:read channel:read:redemptions",
            ),
            true,
        );
        assert_eq!(snap.status, "reauth");
    }

    #[test]
    fn scope_snapshot_leer() {
        let snap = scope_snapshot(None, false);
        assert_eq!(snap.status, "missing");
        assert!(!snap.connected);
    }

    #[test]
    fn view_parse_or_default_ist_active() {
        // P2.80: fehlender/leerer View → Active (Python-Default), nicht All.
        assert_eq!(
            StreamerView::parse_or_default(None),
            Some(StreamerView::Active)
        );
        assert_eq!(
            StreamerView::parse_or_default(Some("")),
            Some(StreamerView::Active)
        );
        assert_eq!(
            StreamerView::parse_or_default(Some("  ")),
            Some(StreamerView::Active)
        );
        // Explizite Werte bleiben erhalten.
        assert_eq!(
            StreamerView::parse_or_default(Some("all")),
            Some(StreamerView::All)
        );
        assert_eq!(
            StreamerView::parse_or_default(Some("archived")),
            Some(StreamerView::Archived)
        );
        // Unbekannt → None (Handler liefert 400).
        assert_eq!(StreamerView::parse_or_default(Some("bogus")), None);
    }

    #[test]
    fn partner_status_blocked_vorrang() {
        // blocked überschreibt opt_out
        assert_eq!(
            partner_status(Some("active"), None, 1, Some("blocked")),
            "blocked"
        );
    }

    #[test]
    fn partner_status_opt_out() {
        assert_eq!(partner_status(Some("active"), None, 1, None), "non_partner");
    }

    #[test]
    fn partner_status_token_error() {
        assert_eq!(
            partner_status(Some("active"), None, 0, Some("token_error")),
            "token_error"
        );
    }

    #[test]
    fn partner_status_archived_via_flag() {
        // archived_at ist TEXT in Prod — non-empty String gilt als gesetzt
        assert_eq!(
            partner_status(Some("active"), Some("2024-01-01T00:00:00Z"), 0, None),
            "archived"
        );
    }

    // --- DB-Tests ---

    #[tokio::test]
    async fn leere_liste_gibt_leeres_array() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_str_leer").await;
        let rows = list_streamers(&pool, StreamerView::Active)
            .await
            .expect("query");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn aktiver_streamer_erscheint_in_active_view() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_str_active").await;
        // created_at ist TEXT in Prod → expliziter Cast
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, created_at) \
             VALUES ('teststreamer', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert");
        let rows = list_streamers(&pool, StreamerView::Active)
            .await
            .expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].twitch_login, "teststreamer");
    }

    #[tokio::test]
    async fn list_streamers_dekodiert_bool_oauth_flags() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_str_bool_oauth").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, status, created_at) \
             VALUES ('booloauth', '42', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert partner");
        sqlx::query(
            "INSERT INTO twitch_raid_auth \
             (twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at) \
             VALUES ('booloauth', '42', 'bits:read', TRUE, FALSE, NOW())",
        )
        .execute(&pool)
        .await
        .expect("insert auth");

        let rows = list_streamers(&pool, StreamerView::Active)
            .await
            .expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].needs_reauth, Some(true));
    }

    #[tokio::test]
    async fn view_filter_funktioniert_fuer_archived() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_str_archived").await;
        // created_at/archived_at sind TEXT in Prod → expliziter Cast
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, created_at) \
             VALUES ('aktiver', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert aktiver");
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, archived_at) \
             VALUES ('archivierter', 'archived', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert archivierter");

        let active_rows = list_streamers(&pool, StreamerView::Active)
            .await
            .expect("active query");
        assert_eq!(active_rows.len(), 1);
        assert_eq!(active_rows[0].twitch_login, "aktiver");

        let archived_rows = list_streamers(&pool, StreamerView::Archived)
            .await
            .expect("archived query");
        assert_eq!(archived_rows.len(), 1);
        assert_eq!(archived_rows[0].twitch_login, "archivierter");
    }

    #[tokio::test]
    async fn detail_gibt_none_fuer_unbekannten_login() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_str_detail_none").await;
        let row = streamer_detail(&pool, "gibts_nicht").await.expect("query");
        assert!(row.is_none());
    }

    #[tokio::test]
    async fn detail_gibt_row_zurueck_fuer_bekannten_login() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_str_detail_found").await;
        // created_at ist TEXT in Prod → expliziter Cast
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, created_at) \
             VALUES ('bekannter', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert");
        let row = streamer_detail(&pool, "Bekannter").await.expect("query"); // case-insensitive!
        assert!(row.is_some());
        assert_eq!(row.unwrap().twitch_login, "bekannter");
    }

    #[tokio::test]
    async fn detail_dekodiert_bool_oauth_flags() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_str_detail_bool_oauth").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, status, created_at) \
             VALUES ('detailoauth', '43', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert partner");
        sqlx::query(
            "INSERT INTO twitch_raid_auth \
             (twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at) \
             VALUES ('detailoauth', '43', 'bits:read', TRUE, FALSE, NOW())",
        )
        .execute(&pool)
        .await
        .expect("insert auth");

        let row = streamer_detail(&pool, "detailoauth")
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.needs_reauth, Some(true));
        assert_eq!(row.oauth_raid_enabled, Some(false));
    }
}

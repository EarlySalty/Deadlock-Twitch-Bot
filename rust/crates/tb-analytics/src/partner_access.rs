//! Partner-Zugangsstatus für das Dashboard.
//!
//! Port von `bot/analytics/api_v2.py:_dashboard_access_state_from_conn` (916–1080 Z.)
//!
//! Liefert alle Felder die auth-status für die `access`- und `permissions`-
//! Sektionen braucht, ohne dabei den Auth-Level oder die Session zu kennen.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use sqlx::PgPool;

/// Port von `_parse_access_state_datetime` (api_v2.py Z. 898–914): toleranter
/// ISO-Parser (`Z`→`+00:00`, naive Zeit → UTC). Parse-Fehler → `None`.
fn parse_access_state_datetime(raw: &str) -> Option<DateTime<Utc>> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let normalized = text.replace('Z', "+00:00");
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&Utc));
    }
    // Naive Varianten ohne Offset → als UTC interpretieren (wie Python).
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, fmt) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    None
}

// ── Status-Konstanten ───────────────────────────────────────────────────────

const STATUS_ACTIVE: &str = "active";
const STATUS_ARCHIVED: &str = "archived";
const STATUS_DEPARTNERED: &str = "departnered";
const STATUS_NON_PARTNER: &str = "non_partner";
const STATUS_TOKEN_ERROR: &str = "token_error";
const STATUS_BLOCKED: &str = "blocked";

/// Statuses bei denen der Analytics-Zugang gesperrt ist.
fn is_analytics_blocked(status: &str) -> bool {
    matches!(
        status,
        STATUS_BLOCKED | STATUS_DEPARTNERED | STATUS_NON_PARTNER | STATUS_TOKEN_ERROR | "archived"
    )
}

// ── Ergebnis-Typ ────────────────────────────────────────────────────────────

/// Abgeleiteter Zugangsstatus für einen Streamer (für auth-status-Response).
#[derive(Debug, Default, Clone)]
pub struct AccessState {
    pub partner_status: String,
    pub technical_pause_reason: Option<String>,
    pub operational_state: Option<String>,
    pub token_error_grace_expires_at: Option<String>,
    pub analytics_access_allowed: bool,
    pub landing_access_allowed: bool,
}

// ── Interne Hilfsstrukturen ─────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct PartnerRow {
    status: Option<String>,
    archived_at: Option<String>,
    // DB-Spalte ist INT4 (vgl. admin_streamers.rs); Option<i64> brach das Decode
    // und ließ auth-status für jeden Partner-Lookup fail-open laufen.
    manual_partner_opt_out: Option<i32>,
    technical_pause_reason: Option<String>,
}

#[derive(sqlx::FromRow)]
struct BlacklistRow {
    grace_expires_at: Option<String>,
    error_count: Option<i64>,
    role_removed: Option<i32>,
}

// ── Haupt-Funktion ──────────────────────────────────────────────────────────

/// Lädt den Zugangsstatus für `login` / `user_id` aus DB und leitet daraus
/// partner_status, analytics_access_allowed etc. ab.
///
/// Schlägt entweder Login oder User-ID nach — beide dürfen leer sein; dann
/// wird der Streamer als "aktiv" behandelt (kein twitch_partners-Eintrag nötig,
/// z. B. für Localhost/Admin-Calls ohne Partner-Session).
pub async fn load_partner_access_state(
    pool: &PgPool,
    login: &str,
    user_id: &str,
) -> Result<AccessState, sqlx::Error> {
    let login = login.trim().to_lowercase();
    let user_id = user_id.trim().to_string();

    // Kein Lookup möglich → als aktiv behandeln (Python-Parität:
    // "active if not (normalized_login or normalized_user_id)")
    if login.is_empty() && user_id.is_empty() {
        return Ok(AccessState {
            partner_status: STATUS_ACTIVE.to_string(),
            analytics_access_allowed: true,
            landing_access_allowed: true,
            ..Default::default()
        });
    }

    // ── twitch_partners Lookup ──────────────────────────────────────────────
    let partner_row = sqlx::query_as!(
        PartnerRow,
        r#"
        SELECT
            status,
            COALESCE(
                admin_archived_at,
                CASE WHEN status = 'archived' THEN departnered_at ELSE NULL END
            )::text AS archived_at,
            manual_partner_opt_out,
            technical_pause_reason
        FROM twitch_partners
        WHERE (COALESCE($1, '') != '' AND LOWER(twitch_login) = LOWER($1))
           OR (COALESCE($2, '') != '' AND twitch_user_id = $2)
        ORDER BY
            CASE
                WHEN COALESCE(status, '') = 'active'      THEN 0
                WHEN COALESCE(status, '') = 'archived'    THEN 1
                WHEN COALESCE(status, '') = 'departnered' THEN 2
                ELSE 3
            END,
            COALESCE(departnered_at, admin_archived_at, partnered_at) DESC
        LIMIT 1
        "#,
        &login,
        &user_id
    )
    .fetch_optional(pool)
    .await?;

    // ── twitch_token_blacklist Lookup ───────────────────────────────────────
    let blacklist_row = sqlx::query_as!(
        BlacklistRow,
        r#"
        SELECT
            grace_expires_at::text,
            error_count::bigint AS error_count,
            role_removed
        FROM twitch_token_blacklist
        WHERE (COALESCE($1, '') != '' AND twitch_user_id = $1)
           OR (COALESCE($2, '') != '' AND LOWER(twitch_login) = LOWER($2))
        ORDER BY last_error_at DESC NULLS LAST, first_error_at DESC NULLS LAST
        LIMIT 1
        "#,
        &user_id,
        &login
    )
    .fetch_optional(pool)
    .await?;

    // ── Partner-Status ableiten ─────────────────────────────────────────────
    let mut partner_status = if partner_row.is_none() {
        STATUS_NON_PARTNER.to_string()
    } else {
        STATUS_ACTIVE.to_string()
    };
    let mut technical_pause_reason = String::new();
    let mut operational_state = String::new();

    if let Some(ref row) = partner_row {
        let status_text = row.status.as_deref().unwrap_or("").trim().to_lowercase();
        let archived_at = row.archived_at.as_deref().unwrap_or("").trim().to_string();
        let manual_opt_out = row.manual_partner_opt_out.unwrap_or(0) != 0;
        technical_pause_reason = row
            .technical_pause_reason
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_lowercase();

        // operational_state ableiten (Python api_v2.py:993-1002)
        if technical_pause_reason == STATUS_BLOCKED {
            operational_state = STATUS_BLOCKED.to_string();
        } else if status_text != STATUS_ACTIVE {
            operational_state = "inactive".to_string();
        } else if manual_opt_out {
            operational_state = "admin_non_partner".to_string();
        } else if !technical_pause_reason.is_empty() {
            operational_state = technical_pause_reason.clone();
        } else {
            operational_state = STATUS_ACTIVE.to_string();
        }

        // partner_status ableiten (Python api_v2.py:1003-1014)
        if technical_pause_reason == STATUS_BLOCKED {
            partner_status = STATUS_BLOCKED.to_string();
        } else if manual_opt_out {
            partner_status = STATUS_NON_PARTNER.to_string();
        } else if technical_pause_reason == STATUS_TOKEN_ERROR {
            partner_status = STATUS_TOKEN_ERROR.to_string();
        } else if status_text == STATUS_ARCHIVED || !archived_at.is_empty() {
            partner_status = STATUS_ARCHIVED.to_string();
        } else if status_text == STATUS_DEPARTNERED {
            partner_status = STATUS_DEPARTNERED.to_string();
        } else if status_text != STATUS_ACTIVE {
            partner_status = STATUS_NON_PARTNER.to_string();
        }
    }

    // ── Blacklist-Grace prüfen ──────────────────────────────────────────────
    let mut token_error_grace_expires_at: Option<String> = None;
    if let Some(ref bl) = blacklist_row {
        let grace_raw = bl
            .grace_expires_at
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let error_count = bl.error_count.unwrap_or(0);
        let role_removed = bl.role_removed.unwrap_or(0) != 0;

        // Python _parse_access_state_datetime: ISO parsen (Z→+00:00, naive→UTC).
        let grace_parsed = parse_access_state_datetime(&grace_raw);
        // token_error_grace_expires_at wird gesetzt, sobald der Timestamp parst —
        // unabhängig von grace_active (Python api_v2.py:1069-1071).
        token_error_grace_expires_at = grace_parsed.map(|dt| dt.to_rfc3339());

        // grace_active: nur wenn die Frist in der ZUKUNFT liegt und die Rolle nicht
        // entfernt wurde (Python Z.1043-1047). Ohne den >now()-Check blieb eine
        // abgelaufene Frist „aktiv" und sperrte den Streamer dauerhaft aus den Analytics.
        let grace_active = grace_parsed.is_some_and(|dt| dt > Utc::now()) && !role_removed;

        // Python api_v2.py:1045-1057: token_error setzen wenn grace aktiv
        let not_hard_blocked = !matches!(
            partner_status.as_str(),
            STATUS_BLOCKED | STATUS_NON_PARTNER | STATUS_DEPARTNERED
        );
        if not_hard_blocked
            && grace_active
            && (technical_pause_reason == STATUS_TOKEN_ERROR || error_count > 0)
        {
            partner_status = STATUS_TOKEN_ERROR.to_string();
        }
    }

    let analytics_access_allowed = !is_analytics_blocked(&partner_status);
    let landing_access_allowed = partner_status != STATUS_BLOCKED;

    Ok(AccessState {
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
        token_error_grace_expires_at,
        analytics_access_allowed,
        landing_access_allowed,
    })
}

//! Plan-Katalog und DB-Resolver für Streamer-Abonnements.
//!
//! Port von `bot/entitlements/catalog.py` (statische Tabellen) und
//! `bot/entitlements/repository.py:resolve_plan_snapshot` (DB-Queries).
//!
//! # Priorität
//! 1. Manual-Override in `streamer_plans` (wenn aktiv + nicht abgelaufen)
//! 2. Stripe-Abo in `twitch_billing_subscriptions` (Status active/trialing/past_due)
//! 3. Default: `raid_free`

use sqlx::PgPool;

// ── Statischer Katalog ──────────────────────────────────────────────────────

/// Plan-Tier aus Plan-ID ableiten (Python: `PLAN_TIER_MAP`).
pub fn plan_tier(plan_id: &str) -> &'static str {
    match plan_id {
        "raid_free" => "free",
        "chat_quiet" | "raid_boost" | "bundle_chat_quiet_raid_boost" => "basic",
        "analysis_dashboard"
        | "bundle_analysis_raid_boost"
        | "bundle_werbefrei_analyse"
        | "bundle_komplett"
        | "analytics_trial" => "extended",
        _ => "free",
    }
}

/// Anzeigename aus Plan-ID (Python: `PLAN_DISPLAY_NAME_MAP`).
pub fn plan_display_name(plan_id: &str) -> &'static str {
    match plan_id {
        "raid_free" => "Free",
        "chat_quiet" => "Werbefrei",
        "raid_boost" => "Basic",
        "bundle_chat_quiet_raid_boost" => "Werbefrei + Raid Boost",
        "analysis_dashboard" => "Erweitert",
        "bundle_analysis_raid_boost" => "Erweitert (Bundle)",
        "bundle_werbefrei_analyse" => "Werbefrei + Analyse",
        "bundle_komplett" => "Alles drin",
        "analytics_trial" => "Trial",
        _ => "Free",
    }
}

/// Ob Plan Extended-Analytics beinhaltet.
pub fn plan_is_extended(plan_id: &str) -> bool {
    plan_tier(plan_id) == "extended"
}

/// Entitlements aus Plan-ID (Python: `PLAN_ENTITLEMENTS_MAP`).
pub fn plan_entitlements(plan_id: &str) -> &'static [&'static str] {
    match plan_id {
        // analytics.daily = kostenlose "Tagesform" (Snapshot des letzten Streams).
        // Paid-Plaene brauchen es nicht zusaetzlich: sie bekommen via
        // analytics.basic/extended ohnehin den vollen Verlauf.
        "raid_free" => &["analytics.daily"],
        "chat_quiet" => &["chat.promos.disable"],
        "raid_boost" => &[
            "analytics.ai_mini",
            "analytics.basic",
            "chat.lurker_tax",
            "raid.priority",
        ],
        "bundle_chat_quiet_raid_boost" => &[
            "analytics.ai_mini",
            "analytics.basic",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        "analysis_dashboard" => &[
            "analytics.basic",
            "analytics.ai_full",
            "analytics.extended",
            "chat.lurker_tax",
        ],
        "bundle_analysis_raid_boost" => &[
            "analytics.basic",
            "analytics.ai_full",
            "analytics.extended",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        "bundle_werbefrei_analyse" => &[
            "analytics.basic",
            "analytics.ai_full",
            "analytics.extended",
            "chat.lurker_tax",
            "chat.promos.disable",
        ],
        "bundle_komplett" => &[
            "analytics.ai_mini",
            "analytics.basic",
            "analytics.ai_full",
            "analytics.extended",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        "analytics_trial" => &[
            "analytics.ai_mini",
            "analytics.basic",
            "analytics.extended",
            "chat.lurker_tax",
        ],
        _ => &[],
    }
}

/// Normalisiert eine Plan-ID auf den kanonischen Wert.
///
/// Python: `normalize_plan_id` / `LEGACY_PLAN_NAME_TO_ID_MAP`
fn normalize_plan_id(raw: &str) -> &'static str {
    match raw.trim().to_lowercase().as_str() {
        "free" | "raid_free" => "raid_free",
        "werbefrei" | "quiet" | "chat_quiet" => "chat_quiet",
        "raid_boost" => "raid_boost",
        "chat_quiet_bundle" | "bundle_chat_quiet_raid_boost" => "bundle_chat_quiet_raid_boost",
        "analysis" | "analysis_dashboard" => "analysis_dashboard",
        "bundle" | "bundle_analysis_raid_boost" => "bundle_analysis_raid_boost",
        "bundle_werbefrei_analyse" => "bundle_werbefrei_analyse",
        "bundle_komplett" => "bundle_komplett",
        "analytics_trial" => "analytics_trial",
        _ => "raid_free",
    }
}

// ── Ergebnis-Typ ────────────────────────────────────────────────────────────

/// Aufgelöster Plan-Snapshot für einen Streamer.
#[derive(Debug, Clone)]
pub struct PlanSnapshot {
    pub plan_id: &'static str,
    pub plan_name: &'static str,
    pub tier: &'static str,
    pub is_extended: bool,
    pub entitlements: Vec<&'static str>,
    pub expires_at: Option<String>,
    pub source: &'static str,
}

impl PlanSnapshot {
    fn from_plan(plan_id: &'static str, source: &'static str, expires_at: Option<String>) -> Self {
        PlanSnapshot {
            plan_id,
            plan_name: plan_display_name(plan_id),
            tier: plan_tier(plan_id),
            is_extended: plan_is_extended(plan_id),
            entitlements: plan_entitlements(plan_id).to_vec(),
            expires_at,
            source,
        }
    }
}

// ── DB-Queries ──────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ManualOverrideRow {
    manual_plan_id: Option<String>,
    manual_plan_expires_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct BillingRow {
    plan_id: Option<String>,
    current_period_end: Option<String>,
}

const ACTIVE_BILLING_STATUSES: [&str; 3] = ["active", "trialing", "past_due"];

/// Löst den effektiven Plan für `login` auf.
///
/// Priorität: Manual-Override → Stripe-Abo → Default `raid_free`.
pub async fn resolve_plan_snapshot(
    pool: &PgPool,
    login: &str,
) -> Result<PlanSnapshot, sqlx::Error> {
    let login = login.trim().to_lowercase();
    if login.is_empty() {
        return Ok(PlanSnapshot::from_plan("raid_free", "default_basic", None));
    }

    // ── Manual Override ─────────────────────────────────────────────────────
    let manual: Option<ManualOverrideRow> = sqlx::query_as(
        r#"
        SELECT manual_plan_id, manual_plan_expires_at::text
        FROM streamer_plans
        WHERE LOWER(twitch_login) = LOWER($1)
        ORDER BY manual_plan_updated_at DESC NULLS LAST
        LIMIT 1
        "#,
    )
    .bind(&login)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = manual {
        let pid_raw = row.manual_plan_id.as_deref().unwrap_or("").trim().to_lowercase();
        let pid = normalize_plan_id(&pid_raw);
        if pid != "raid_free" || pid_raw == "raid_free" {
            // Ablauf prüfen
            let expired = row
                .manual_plan_expires_at
                .as_deref()
                .map(is_expired_timestamp)
                .unwrap_or(false);
            // Ein aktiver (nicht abgelaufener) expliziter Override ist terminal —
            // auch ein bewusster Admin-Downgrade auf raid_free sperrt den Billing-
            // Fallthrough (Python repository.py: jeder aktive Override gewinnt).
            // Der äußere Guard hat „explizit gesetzt" bereits sichergestellt; ein
            // leerer manual_plan_id (→ raid_free) kommt hier gar nicht an.
            if !expired {
                return Ok(PlanSnapshot::from_plan(
                    pid,
                    "manual_override",
                    row.manual_plan_expires_at,
                ));
            }
        }
    }

    // ── Stripe-Abo ──────────────────────────────────────────────────────────
    let billing: Option<BillingRow> = sqlx::query_as(
        r#"
        SELECT plan_id, current_period_end::text
        FROM twitch_billing_subscriptions
        WHERE LOWER(customer_reference) = LOWER($1)
          AND status = ANY($2)
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(&login)
    .bind(&ACTIVE_BILLING_STATUSES[..])
    .fetch_optional(pool)
    .await?;

    if let Some(row) = billing {
        let pid_raw = row.plan_id.as_deref().unwrap_or("").trim().to_lowercase();
        let pid = normalize_plan_id(&pid_raw);
        return Ok(PlanSnapshot::from_plan(pid, "billing_subscription", row.current_period_end));
    }

    Ok(PlanSnapshot::from_plan("raid_free", "default_basic", None))
}

/// Einfacher Ablauf-Check: ISO-Zeitstempel in der Vergangenheit?
///
/// Robuster Parse: lehnt nur eindeutig abgelaufene Timestamps ab; bei Parse-
/// Fehler → nicht abgelaufen (fail-open, wie Python).
fn is_expired_timestamp(raw: &str) -> bool {
    let raw = raw.trim();
    if raw.is_empty() {
        return false;
    }
    // Normalisierung: Z → +00:00
    let normalized = raw.replace('Z', "+00:00");
    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&normalized) {
        return ts < chrono::Utc::now();
    }
    // Fallback: nur Datum (YYYY-MM-DD) → als Mitternacht UTC
    if raw.len() == 10 && raw.as_bytes().get(4) == Some(&b'-') {
        let with_time = format!("{raw}T23:59:59+00:00");
        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&with_time) {
            return ts < chrono::Utc::now();
        }
    }
    false
}

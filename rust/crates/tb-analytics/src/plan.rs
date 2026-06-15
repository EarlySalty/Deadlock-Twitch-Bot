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

/// Normalisiert eine Plan-ID strikt-kanonisch auf einen bekannten Plan.
///
/// Spiegelt Python `catalog.py:normalize_plan_id` (Zeile 138-140):
/// nur Whitespace trimmen, dann **exakter, case-sensitiver** Abgleich gegen
/// `KNOWN_PLAN_IDS`; sonst Fallback `raid_free`. KEIN Lowercasing, KEINE
/// Legacy-Aliase — die liegen in Python in `normalize_plan_id_from_legacy_name`
/// (nur vom Raid-Subsystem `partner_scores.py` genutzt, nicht in der
/// Entitlement-DB-Auflösung).
///
/// Die zuvor hier eingebaute case-insensitive/Legacy-Normalisierung war eine
/// Migrations-Divergenz: sie hätte z. B. `"Raid_Boost"` oder `"analysis"` an
/// DB-Resolution-Stellen akzeptiert, wo Python sie auf `raid_free` (Billing)
/// bzw. ganz aus dem Override (Manual) wirft. Test-Gate: siehe `tests`.
fn normalize_plan_id(raw: &str) -> &'static str {
    match raw.trim() {
        "raid_free" => "raid_free",
        "chat_quiet" => "chat_quiet",
        "raid_boost" => "raid_boost",
        "bundle_chat_quiet_raid_boost" => "bundle_chat_quiet_raid_boost",
        "analysis_dashboard" => "analysis_dashboard",
        "bundle_analysis_raid_boost" => "bundle_analysis_raid_boost",
        "bundle_werbefrei_analyse" => "bundle_werbefrei_analyse",
        "bundle_komplett" => "bundle_komplett",
        "analytics_trial" => "analytics_trial",
        _ => "raid_free",
    }
}

/// Kanonische Plan-IDs (Python `KNOWN_PLAN_IDS`).
const KNOWN_PLAN_IDS: [&str; 9] = [
    "raid_free",
    "chat_quiet",
    "raid_boost",
    "bundle_chat_quiet_raid_boost",
    "analysis_dashboard",
    "bundle_analysis_raid_boost",
    "bundle_werbefrei_analyse",
    "bundle_komplett",
    "analytics_trial",
];

/// `true`, wenn `raw` (nur whitespace-getrimmt) ein bekannter kanonischer Plan
/// ist. Spiegelt Python `manual_override_from_row` (repository.py:82):
/// `if plan_id not in KNOWN_PLAN_IDS: return None`.
fn is_known_plan_id(raw: &str) -> bool {
    KNOWN_PLAN_IDS.contains(&raw.trim())
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
    user_id: &str,
) -> Result<PlanSnapshot, sqlx::Error> {
    let login = login.trim().to_lowercase();
    let user_id = user_id.trim();
    if login.is_empty() && user_id.is_empty() {
        return Ok(PlanSnapshot::from_plan("raid_free", "default_basic", None));
    }

    // 24h-Grace-Auto-Grant des Analytics-Trials VOR der Auflösung (Python
    // `_resolve_plan_for_request` ruft `_billing_check_and_grant_trial_eligibility`
    // vor `_resolve_plan_snapshot_for_refs`). Nur bei user_id UND login; Fehler
    // werden intern geschluckt. Idempotent über `trial_ever_granted`.
    if !user_id.is_empty() && !login.is_empty() {
        crate::trial::check_and_grant_trial_eligibility(pool, user_id, &login).await;
    }

    // ── Manual Override ─────────────────────────────────────────────────────
    // Python load_manual_override matcht twitch_user_id ODER twitch_login und
    // priorisiert den user_id-Treffer (CASE-ORDER). Ein nur per user_id (mit
    // abweichendem/leerem Login) eingetragener Override wurde sonst nicht
    // gefunden → Streamer verlor seinen bezahlten/gecompten Plan.
    let manual: Option<ManualOverrideRow> = sqlx::query_as(
        r#"
        SELECT manual_plan_id, manual_plan_expires_at::text
        FROM streamer_plans
        WHERE LOWER(COALESCE(twitch_login, '')) = LOWER($1)
           OR ($2 <> '' AND TRIM(COALESCE(twitch_user_id, '')) = $2)
        ORDER BY
            CASE WHEN $2 <> '' AND TRIM(COALESCE(twitch_user_id, '')) = $2 THEN 0 ELSE 1 END,
            manual_plan_updated_at DESC NULLS LAST
        LIMIT 1
        "#,
    )
    .bind(&login)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = manual {
        let pid_raw = row.manual_plan_id.as_deref().unwrap_or("");
        // Strikt-kanonisch wie Python `manual_override_from_row` (repository.py:82):
        // ein `manual_plan_id`, der NICHT in KNOWN_PLAN_IDS liegt (Case-Mismatch,
        // Legacy-Alias, Tippfehler), macht den Override ungültig → Fall-Through zu
        // Billing/Default. Vorher normalisierte Rust hier lowercased + Legacy-Aliase
        // und behandelte Müll als raid_free-Override (Migrations-Divergenz).
        if is_known_plan_id(pid_raw) {
            let pid = normalize_plan_id(pid_raw);
            // Ablauf prüfen
            let expired = row
                .manual_plan_expires_at
                .as_deref()
                .map(is_expired_timestamp)
                .unwrap_or(false);
            // Ein aktiver (nicht abgelaufener) expliziter Override ist terminal —
            // auch ein bewusster Admin-Downgrade auf raid_free sperrt den Billing-
            // Fallthrough (Python repository.py: jeder aktive Override gewinnt).
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
    // customer_reference kann Login ODER twitch_user_id sein — beide prüfen,
    // sonst bleibt ein per user_id referenziertes Stripe-Abo unsichtbar.
    let billing: Option<BillingRow> = sqlx::query_as(
        r#"
        SELECT plan_id, current_period_end::text
        FROM twitch_billing_subscriptions
        WHERE (LOWER(customer_reference) = LOWER($1)
               OR ($2 <> '' AND LOWER(customer_reference) = LOWER($2)))
          AND status = ANY($3)
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(&login)
    .bind(user_id)
    .bind(&ACTIVE_BILLING_STATUSES[..])
    .fetch_optional(pool)
    .await?;

    if let Some(row) = billing {
        // Strikt-kanonisch wie Python `load_billing_subscription` (repository.py:186):
        // `normalize_plan_id(plan_id, default="raid_free")` ohne Lowercasing/Legacy.
        let pid = normalize_plan_id(row.plan_id.as_deref().unwrap_or("raid_free"));
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_plan_id: strikt-kanonisch (Python catalog.py:138-140) ─────

    #[test]
    fn normalize_akzeptiert_alle_kanonischen_ids() {
        for id in KNOWN_PLAN_IDS {
            assert_eq!(normalize_plan_id(id), id, "kanonische ID muss erhalten bleiben: {id}");
        }
    }

    #[test]
    fn normalize_trimmt_whitespace() {
        // Python `str(...).strip()` trimmt vor dem KNOWN_PLAN_IDS-Abgleich.
        assert_eq!(normalize_plan_id("  raid_boost  "), "raid_boost");
        assert_eq!(normalize_plan_id("\tanalysis_dashboard\n"), "analysis_dashboard");
    }

    #[test]
    fn normalize_ist_case_sensitive() {
        // Python lowercased NICHT in normalize_plan_id → Case-Mismatch fällt auf raid_free.
        assert_eq!(normalize_plan_id("Raid_Boost"), "raid_free");
        assert_eq!(normalize_plan_id("ANALYSIS_DASHBOARD"), "raid_free");
        assert_eq!(normalize_plan_id("Chat_Quiet"), "raid_free");
    }

    #[test]
    fn normalize_lehnt_legacy_aliase_ab() {
        // Legacy-Aliase (free/werbefrei/quiet/analysis/bundle/chat_quiet_bundle)
        // gehören in Python NUR zu normalize_plan_id_from_legacy_name (Raid-
        // Subsystem), NICHT zur Entitlement-DB-Auflösung → hier kein Mapping.
        for alias in ["free", "werbefrei", "quiet", "analysis", "bundle", "chat_quiet_bundle"] {
            assert_eq!(
                normalize_plan_id(alias),
                "raid_free",
                "Legacy-Alias darf in DB-Resolution nicht aufgelöst werden: {alias}"
            );
        }
    }

    #[test]
    fn normalize_unbekannt_faellt_auf_raid_free() {
        assert_eq!(normalize_plan_id(""), "raid_free");
        assert_eq!(normalize_plan_id("garbage"), "raid_free");
        assert_eq!(normalize_plan_id("premium_max"), "raid_free");
    }

    // ── is_known_plan_id: Manual-Override-Gate (Python repository.py:82) ─────

    #[test]
    fn known_plan_id_nur_fuer_kanonische_ids() {
        assert!(is_known_plan_id("raid_free"));
        assert!(is_known_plan_id("analytics_trial"));
        assert!(is_known_plan_id("  bundle_komplett  ")); // trim wie Python
    }

    #[test]
    fn known_plan_id_false_fuer_case_mismatch_und_legacy() {
        // Diese Werte machen einen Manual-Override in Python ungültig (→ None,
        // Fall-Through zu Billing) statt ihn als raid_free zu honorieren.
        assert!(!is_known_plan_id("Raid_Boost"));
        assert!(!is_known_plan_id("analysis"));
        assert!(!is_known_plan_id("free"));
        assert!(!is_known_plan_id(""));
        assert!(!is_known_plan_id("garbage"));
    }
}

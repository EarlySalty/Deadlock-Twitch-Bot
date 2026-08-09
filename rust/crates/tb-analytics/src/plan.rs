//! Plan-Katalog und DB-Resolver für Streamer-Abonnements.
//!
//! Port von `bot/entitlements/catalog.py` (statische Tabellen) und
//! `bot/entitlements/repository.py:resolve_plan_snapshot` (DB-Queries).
//!
//! # Priorität
//! 1. Manual-Override in `streamer_plans` (wenn aktiv + nicht abgelaufen)
//! 2. Stripe-Abo in `twitch_billing_subscriptions` (Status active/trialing/past_due)
//! 3. Default: `free` (bis 2026-08-09: `raid_free`)

use serde_json::{json, Value};
use sqlx::PgPool;

// ── Statischer Katalog ──────────────────────────────────────────────────────

/// Plan-Tier aus Plan-ID ableiten (Python: `PLAN_TIER_MAP`).
pub fn plan_tier(plan_id: &str) -> &'static str {
    match plan_id {
        // Neuer Katalog seit 2026-08-09.
        "free" => "free",
        "premium" => "extended",
        // Abgeschaffte Pläne: nicht mehr kaufbar, aber in der DB vorhanden.
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
        "free" => "Free",
        "premium" => "Premium",
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
        // Analytics-Konsolidierung auf EIN Flag: kein Flag => `last_stream`
        // (kostenlose Tagesform) ist der Default; das Flag `"analytics"` => voller
        // Analytics-Zugang (voller Verlauf, Vergleiche, KI-Analyse via Opus).
        "free" => &[],
        // Premium trägt alle vier Entitlements — eine bezahlte Stufe, kein Graph.
        "premium" => &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        "raid_free" => &[],
        "chat_quiet" => &["chat.promos.disable"],
        "raid_boost" => &["chat.lurker_tax", "raid.priority"],
        "bundle_chat_quiet_raid_boost" => {
            &["chat.lurker_tax", "chat.promos.disable", "raid.priority"]
        }
        "analysis_dashboard" => &["analytics", "chat.lurker_tax"],
        "bundle_analysis_raid_boost" => &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        "bundle_werbefrei_analyse" => &["analytics", "chat.lurker_tax", "chat.promos.disable"],
        "bundle_komplett" => &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        "analytics_trial" => &["analytics", "chat.lurker_tax"],
        _ => &[],
    }
}

/// `true`, wenn der Plan den konsolidierten `"analytics"`-Zugang trägt.
pub fn plan_has_analytics(plan_id: &str) -> bool {
    plan_entitlements(plan_id).contains(&"analytics")
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
        "free" => "free",
        "premium" => "premium",
        "raid_free" => "raid_free",
        "chat_quiet" => "chat_quiet",
        "raid_boost" => "raid_boost",
        "bundle_chat_quiet_raid_boost" => "bundle_chat_quiet_raid_boost",
        "analysis_dashboard" => "analysis_dashboard",
        "bundle_analysis_raid_boost" => "bundle_analysis_raid_boost",
        "bundle_werbefrei_analyse" => "bundle_werbefrei_analyse",
        "bundle_komplett" => "bundle_komplett",
        "analytics_trial" => "analytics_trial",
        // Pricing-Umbau 2026-08-09: Fallback ist `free`, nicht mehr `raid_free`.
        // `raid_free` bleibt lesbar (Bestandszeilen), taucht aber nicht mehr im
        // Katalog auf — ein Streamer ohne Zeile bekaeme sonst eine plan_id, zu
        // der es keine Karte gibt. Tier und Entitlements sind identisch leer.
        _ => "free",
    }
}

/// Kanonische Plan-IDs (Python `KNOWN_PLAN_IDS`).
const KNOWN_PLAN_IDS: [&str; 11] = [
    "free",
    "premium",
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
pub(crate) fn is_known_plan_id(raw: &str) -> bool {
    KNOWN_PLAN_IDS.contains(&raw.trim())
}

// ── Ergebnis-Typ ────────────────────────────────────────────────────────────

/// Aufgelöster Plan-Snapshot für einen Streamer.
///
/// Voll-Port von Pythons `build_plan_snapshot` (repository.py:194-242) inkl. der
/// vier zuvor fehlenden Felder (B20-ent-3): `status`, `customer_reference`,
/// `manual_override`, `billing_subscription`. Die beiden letztgenannten sind die
/// kompletten Quell-Sub-Dicts (oder `Value::Null`) — identisch zu Pythons
/// `manual_override`/`billing_subscription`-Payloads.
#[derive(Debug, Clone)]
pub struct PlanSnapshot {
    pub plan_id: &'static str,
    pub plan_name: &'static str,
    pub tier: &'static str,
    pub is_extended: bool,
    pub entitlements: Vec<&'static str>,
    /// Abo-/Plan-Status: "active" (Default + Manual-Override), sonst der Stripe-
    /// Status (`active`/`trialing`/`past_due`) wenn `source == billing_subscription`.
    pub status: String,
    /// Normalisierter Ablauf-Zeitstempel (ISO-8601 UTC) oder `None`.
    pub expires_at: Option<String>,
    pub source: &'static str,
    /// Bevorzugt Login, sonst user_id, sonst Fallback-Ref (Python-Reihenfolge).
    pub customer_reference: String,
    /// Quell-Sub-Dict des Manual-Overrides (`Value::Null` wenn keiner griff).
    pub manual_override: Value,
    /// Quell-Sub-Dict des Stripe-Abos (`Value::Null` wenn keins griff).
    pub billing_subscription: Value,
}

impl PlanSnapshot {
    /// Default-/Manual-Snapshot ohne Stripe-Status (`status = "active"`).
    fn from_plan(
        plan_id: &'static str,
        source: &'static str,
        expires_at: Option<String>,
        customer_reference: String,
        manual_override: Value,
        billing_subscription: Value,
    ) -> Self {
        Self::with_status(
            plan_id,
            source,
            "active".to_string(),
            expires_at,
            customer_reference,
            manual_override,
            billing_subscription,
        )
    }

    fn with_status(
        plan_id: &'static str,
        source: &'static str,
        status: String,
        expires_at: Option<String>,
        customer_reference: String,
        manual_override: Value,
        billing_subscription: Value,
    ) -> Self {
        PlanSnapshot {
            plan_id,
            plan_name: plan_display_name(plan_id),
            tier: plan_tier(plan_id),
            is_extended: plan_is_extended(plan_id),
            entitlements: plan_entitlements(plan_id).to_vec(),
            status,
            expires_at,
            source,
            customer_reference,
            manual_override,
            billing_subscription,
        }
    }

    /// Default-Snapshot (`free`, kein Override/Abo) mit Fallback-Ref.
    /// Öffentlich, damit Konsumenten (z.B. Billing-Page-Fail-Safe) den
    /// kanonischen Free-Snapshot bauen, ohne das Feld-Set zu duplizieren.
    pub fn default_basic(fallback_ref: &str) -> Self {
        Self::from_plan(
            "free",
            "default_basic",
            None,
            fallback_ref.trim().to_string(),
            Value::Null,
            Value::Null,
        )
    }
}

// ── DB-Queries ──────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ManualOverrideRow {
    twitch_user_id: Option<String>,
    twitch_login: Option<String>,
    manual_plan_id: Option<String>,
    manual_plan_expires_at: Option<String>,
    manual_plan_notes: Option<String>,
    manual_plan_updated_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct BillingRow {
    customer_reference: Option<String>,
    plan_id: Option<String>,
    status: Option<String>,
    current_period_end: Option<String>,
    updated_at: Option<String>,
}

const ACTIVE_BILLING_STATUSES: [&str; 3] = ["active", "trialing", "past_due"];

/// `true`, wenn der Streamer den bezahlten Zugang hat.
///
/// Eine Wahrheit für die gesamte Paywall. Bei einer einzigen bezahlten Stufe
/// braucht es keinen Entitlement-Graph; geprüft wird das konsolidierte
/// `analytics`-Flag, das `premium`, die abgeschafften Analyse-Pläne und der
/// laufende Trial tragen. Kommt später eine zweite Stufe, wird aus dem Prädikat
/// eine Tier-Abfrage und die Aufrufstellen bleiben unverändert.
///
/// Der Fehler wird bewusst durchgereicht statt geschluckt: jede Aufrufstelle
/// muss sich für fail-closed entscheiden, ein stiller `false` an der falschen
/// Stelle wäre ein Gratis-Zugang aus Versehen.
pub async fn is_premium(pool: &PgPool, streamer: &str) -> Result<bool, sqlx::Error> {
    Ok(resolve_plan_snapshot(pool, streamer, "")
        .await?
        .entitlements
        .contains(&"analytics"))
}

/// Lesefenster eines Streamers: `"full"` mit Premium, sonst `"last_stream"`.
///
/// Gegenstück zu [`is_premium`] für die Endpunkte, die ohne Premium nicht
/// gesperrt, sondern auf den letzten Stream verkürzt werden. Fail-closed: bei
/// DB-Fehler das kleine Fenster, nie mehr Daten zeigen als erlaubt.
pub async fn read_window(pool: &PgPool, streamer: &str) -> &'static str {
    match is_premium(pool, streamer).await {
        Ok(true) => "full",
        _ => "last_stream",
    }
}

/// Löst den effektiven Plan für `login` auf.
///
/// Priorität: Manual-Override → Stripe-Abo → Default `free`.
pub async fn resolve_plan_snapshot(
    pool: &PgPool,
    login: &str,
    user_id: &str,
) -> Result<PlanSnapshot, sqlx::Error> {
    let login = login.trim().to_lowercase();
    let user_id = user_id.trim();
    // Fallback-Ref (Python `fallback_ref`): bevorzugt Login, sonst user_id.
    let fallback_ref = if !login.is_empty() {
        login.clone()
    } else {
        user_id.to_string()
    };
    if login.is_empty() && user_id.is_empty() {
        return Ok(PlanSnapshot::default_basic(&fallback_ref));
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
    let manual = sqlx::query_as!(
        ManualOverrideRow,
        r#"
        SELECT
            COALESCE(twitch_user_id, '') AS "twitch_user_id?",
            COALESCE(twitch_login, '')   AS "twitch_login?",
            manual_plan_id,
            manual_plan_expires_at::text,
            COALESCE(manual_plan_notes, '')      AS "manual_plan_notes?",
            manual_plan_updated_at::text         AS manual_plan_updated_at
        FROM streamer_plans
        WHERE LOWER(COALESCE(twitch_login, '')) = LOWER($1)
           OR ($2 <> '' AND TRIM(COALESCE(twitch_user_id, '')) = $2)
        ORDER BY
            CASE WHEN $2 <> '' AND TRIM(COALESCE(twitch_user_id, '')) = $2 THEN 0 ELSE 1 END,
            manual_plan_updated_at DESC NULLS LAST
        LIMIT 1
        "#,
        &login,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row) = manual {
        let pid_raw = row
            .manual_plan_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        // Strikt-kanonisch wie Python `manual_override_from_row` (repository.py:82):
        // ein `manual_plan_id`, der NICHT in KNOWN_PLAN_IDS liegt (Case-Mismatch,
        // Legacy-Alias, Tippfehler), macht den Override ungültig → Fall-Through zu
        // Billing/Default. Vorher normalisierte Rust hier lowercased + Legacy-Aliase
        // und behandelte Müll als raid_free-Override (Migrations-Divergenz).
        if is_known_plan_id(&pid_raw) {
            let pid = normalize_plan_id(&pid_raw);
            // expires_at via parse_datetime_value normalisieren (B20-ent-2): die
            // DB liefert manual_plan_expires_at als Roh-TEXT (auch Date-only) —
            // Python legt es als `.isoformat()` ab, nicht roh.
            let expires_norm = normalize_expires_at(row.manual_plan_expires_at.as_deref());
            let expired = row
                .manual_plan_expires_at
                .as_deref()
                .map(is_expired_timestamp)
                .unwrap_or(false);
            let is_active = !expired;

            let mo_login = row.twitch_login.as_deref().unwrap_or("").trim().to_string();
            let mo_user_id = row
                .twitch_user_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string();
            // customer_reference (Python build_plan_snapshot:209): login || user_id || fallback.
            let customer_reference = first_non_empty(&[&mo_login, &mo_user_id, &fallback_ref]);
            // Voll-Sub-Dict identisch zu Pythons `manual_override_from_row`.
            let manual_override = json!({
                "twitch_user_id": mo_user_id,
                "twitch_login": mo_login,
                "plan_id": pid_raw,
                "expires_at": expires_norm.clone(),
                "notes": row.manual_plan_notes.as_deref().unwrap_or("").trim(),
                "updated_at": non_empty_or_null(row.manual_plan_updated_at.as_deref()),
                "is_active": is_active,
                "is_expired": !is_active,
            });

            // Ein aktiver (nicht abgelaufener) expliziter Override ist terminal —
            // auch ein bewusster Admin-Downgrade auf raid_free sperrt den Billing-
            // Fallthrough (Python repository.py: jeder aktive Override gewinnt).
            if is_active {
                return Ok(PlanSnapshot::from_plan(
                    pid,
                    "manual_override",
                    expires_norm,
                    customer_reference,
                    manual_override,
                    Value::Null,
                ));
            }
        }
    }

    // ── Stripe-Abo ──────────────────────────────────────────────────────────
    // customer_reference kann Login ODER twitch_user_id sein — beide prüfen,
    // sonst bleibt ein per user_id referenziertes Stripe-Abo unsichtbar.
    let active_billing_statuses: Vec<String> = ACTIVE_BILLING_STATUSES
        .iter()
        .map(|status| status.to_string())
        .collect();
    let billing = sqlx::query_as!(
        BillingRow,
        r#"
        SELECT
            COALESCE(customer_reference, '') AS "customer_reference?",
            plan_id,
            COALESCE(status, '')             AS "status?",
            current_period_end::text,
            updated_at::text                 AS updated_at
        FROM twitch_billing_subscriptions
        WHERE (LOWER(customer_reference) = LOWER($1)
               OR ($2 <> '' AND LOWER(customer_reference) = LOWER($2)))
          AND status = ANY($3::text[])
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
        &login,
        user_id,
        &active_billing_statuses
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row) = billing {
        // Strikt-kanonisch wie Python `load_billing_subscription` (repository.py:186):
        // `normalize_plan_id(plan_id, default="raid_free")` ohne Lowercasing/Legacy.
        let plan_raw = row.plan_id.as_deref().unwrap_or("free");
        let pid = normalize_plan_id(plan_raw);
        // status: leerer Wert → "active" (Python build_plan_snapshot:219).
        let status_raw = row.status.as_deref().unwrap_or("").trim();
        let status = if status_raw.is_empty() {
            "active".to_string()
        } else {
            status_raw.to_string()
        };
        // current_period_end normalisiert → expires_at (Python:223-228).
        let expires_norm = normalize_expires_at(row.current_period_end.as_deref());
        // customer_reference: row-Wert || fallback (Python:220-222).
        let cust_raw = row.customer_reference.as_deref().unwrap_or("").trim();
        let customer_reference = if cust_raw.is_empty() {
            fallback_ref.clone()
        } else {
            cust_raw.to_string()
        };
        // Voll-Sub-Dict identisch zu Pythons `load_billing_subscription`-Payload.
        let billing_subscription = json!({
            "customer_reference": customer_reference.clone(),
            "plan_id": pid,
            "status": status.clone(),
            "current_period_end": expires_norm.clone(),
            "updated_at": non_empty_or_null(row.updated_at.as_deref()),
        });
        return Ok(PlanSnapshot::with_status(
            pid,
            "billing_subscription",
            status,
            expires_norm,
            customer_reference,
            Value::Null,
            billing_subscription,
        ));
    }

    Ok(PlanSnapshot::default_basic(&fallback_ref))
}

/// Erster getrimmt-nichtleerer Wert aus der Kandidatenliste (sonst `""`).
/// Spiegelt Pythons `str(a or b or c or "").strip()`-Idiom.
fn first_non_empty(candidates: &[&str]) -> String {
    candidates
        .iter()
        .map(|c| c.trim())
        .find(|c| !c.is_empty())
        .unwrap_or("")
        .to_string()
}

/// `Some(trimmed)` wenn nichtleer, sonst `None` (für JSON-`null`). Spiegelt
/// Pythons `str(...).strip() or None`.
fn non_empty_or_null(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Parst einen Roh-Zeitstempel nach UTC — Port von Pythons `parse_datetime_value`
/// (repository.py:44-62).
///
/// Reihenfolge wie Python:
/// 1. leer → `None`
/// 2. Date-only (`YYYY-MM-DD`) → als `…T23:59:59+00:00` (Tagesende)
/// 3. `Z` → `+00:00`
/// 4. ISO-8601 parsen (mit oder ohne Offset; naiv ⇒ UTC angenommen)
/// 5. nach UTC konvertieren
///
/// Bei Parse-Fehler → `None` (fail-open wie Pythons `except ValueError: return None`).
fn parse_datetime_value(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let mut text = raw.trim().to_string();
    if text.is_empty() {
        return None;
    }
    // Date-only-Fallback (Python: len==10 && text[4]=='-' && text[7]=='-').
    if text.len() == 10
        && text.as_bytes().get(4) == Some(&b'-')
        && text.as_bytes().get(7) == Some(&b'-')
    {
        text = format!("{text}T23:59:59+00:00");
    }
    let normalized = text.replace('Z', "+00:00");
    // Mit Offset (`fromisoformat` mit tzinfo → astimezone(UTC)).
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // Naive ISO-Timestamps (kein Offset) — Python nimmt UTC an.
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&normalized, fmt) {
            return Some(chrono::DateTime::from_naive_utc_and_offset(
                naive,
                chrono::Utc,
            ));
        }
    }
    None
}

/// Normalisierter ISO-8601-String (`parse_datetime_value(...).isoformat()`), oder
/// `None` bei unparsbarem/leerem Wert. Spiegelt Pythons `expires_at.isoformat()`
/// (B20-ent-2): die DB liefert `manual_plan_expires_at` als Roh-TEXT (auch
/// Date-only); Python normalisiert es VOR der Snapshot-Ablage.
fn normalize_expires_at(raw: Option<&str>) -> Option<String> {
    parse_datetime_value(raw?).map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, false))
}

/// Einfacher Ablauf-Check: ISO-Zeitstempel in der Vergangenheit?
///
/// Nutzt denselben Parser wie die Snapshot-Normalisierung; bei Parse-Fehler →
/// nicht abgelaufen (fail-open, wie Pythons `is_active = not (expires_at && …)`).
fn is_expired_timestamp(raw: &str) -> bool {
    match parse_datetime_value(raw) {
        Some(ts) => ts < chrono::Utc::now(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_plan_id: strikt-kanonisch (Python catalog.py:138-140) ─────

    #[test]
    fn normalize_akzeptiert_alle_kanonischen_ids() {
        for id in KNOWN_PLAN_IDS {
            assert_eq!(
                normalize_plan_id(id),
                id,
                "kanonische ID muss erhalten bleiben: {id}"
            );
        }
    }

    #[test]
    fn normalize_trimmt_whitespace() {
        // Python `str(...).strip()` trimmt vor dem KNOWN_PLAN_IDS-Abgleich.
        assert_eq!(normalize_plan_id("  raid_boost  "), "raid_boost");
        assert_eq!(
            normalize_plan_id("\tanalysis_dashboard\n"),
            "analysis_dashboard"
        );
    }

    #[test]
    fn normalize_ist_case_sensitive() {
        // Kein Lowercasing → Case-Mismatch fällt auf den Gratis-Plan.
        assert_eq!(normalize_plan_id("Raid_Boost"), "free");
        assert_eq!(normalize_plan_id("ANALYSIS_DASHBOARD"), "free");
        assert_eq!(normalize_plan_id("Chat_Quiet"), "free");
        assert_eq!(normalize_plan_id("Premium"), "free");
    }

    #[test]
    fn normalize_lehnt_legacy_aliase_ab() {
        // Legacy-Aliase (werbefrei/quiet/analysis/bundle/chat_quiet_bundle)
        // gehören in Python NUR zu normalize_plan_id_from_legacy_name (Raid-
        // Subsystem), NICHT zur Entitlement-DB-Auflösung → hier kein Mapping.
        for alias in [
            "werbefrei",
            "quiet",
            "analysis",
            "bundle",
            "chat_quiet_bundle",
        ] {
            assert_eq!(
                normalize_plan_id(alias),
                "free",
                "Legacy-Alias darf in DB-Resolution nicht aufgelöst werden: {alias}"
            );
        }
    }

    #[test]
    fn normalize_unbekannt_faellt_auf_free() {
        assert_eq!(normalize_plan_id(""), "free");
        assert_eq!(normalize_plan_id("garbage"), "free");
        assert_eq!(normalize_plan_id("premium_max"), "free");
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
        assert!(!is_known_plan_id(""));
        assert!(!is_known_plan_id("garbage"));
    }

    // ── Analytics-Konsolidierung: EIN Flag ──────────────────────────────────

    /// Drift-Guard: für jeden bekannten Plan stimmen die Entitlements aus diesem
    /// Modul mit dem Billing-Katalog überein (eine Quelle der Wahrheit).
    #[test]
    fn plan_entitlements_match_catalog() {
        for plan in crate::billing::catalog::BILLING_PLANS {
            assert_eq!(
                plan_entitlements(plan.id),
                plan.entitlements,
                "entitlements drift between plan module and catalog for {}",
                plan.id
            );
        }
    }

    /// Das konsolidierte `analytics`-Flag tragen genau die 5 Analyse-Pläne; die
    /// reinen Raid-/Chat-/Free-Pläne nicht.
    #[test]
    fn analytics_flag_nur_auf_analyse_plaenen() {
        for id in [
            "free",
            "raid_boost",
            "bundle_chat_quiet_raid_boost",
            "raid_free",
            "chat_quiet",
        ] {
            assert!(
                !plan_has_analytics(id),
                "{id} darf kein analytics-Flag tragen"
            );
            assert!(
                !plan_entitlements(id).contains(&"analytics"),
                "{id} entitlements dürfen kein analytics enthalten"
            );
        }
        for id in [
            "premium",
            "analysis_dashboard",
            "bundle_werbefrei_analyse",
            "bundle_komplett",
            "bundle_analysis_raid_boost",
            "analytics_trial",
        ] {
            assert!(plan_has_analytics(id), "{id} muss analytics-Flag tragen");
        }
    }

    // ── Pricing-Umbau 2026-08-09: Free + Premium neben den Altlasten ────────

    #[test]
    fn free_und_premium_sind_kanonisch() {
        assert_eq!(plan_tier("free"), "free");
        assert_eq!(plan_tier("premium"), "extended");
        assert_eq!(plan_display_name("free"), "Free");
        assert_eq!(plan_display_name("premium"), "Premium");
        assert_eq!(normalize_plan_id("free"), "free");
        assert_eq!(normalize_plan_id("premium"), "premium");
        assert!(is_known_plan_id("free"));
        assert!(is_known_plan_id("premium"));
        assert_eq!(
            plan_entitlements("premium"),
            &[
                "analytics",
                "chat.lurker_tax",
                "chat.promos.disable",
                "raid.priority"
            ]
        );
        assert!(plan_entitlements("free").is_empty());
    }

    /// Die abgeschafften Pläne stehen weiter in der DB. Sie sind nicht mehr
    /// kaufbar, müssen aber unverändert auflösbar bleiben und ihre Entitlements
    /// behalten — sonst verlieren Bestandsnutzer beim Deploy ihren Zugang.
    #[test]
    fn alte_plan_ids_bleiben_aufloesbar_und_behalten_entitlements() {
        for (id, expected) in [
            ("raid_free", &[][..]),
            ("chat_quiet", &["chat.promos.disable"][..]),
            ("raid_boost", &["chat.lurker_tax", "raid.priority"][..]),
            (
                "bundle_chat_quiet_raid_boost",
                &["chat.lurker_tax", "chat.promos.disable", "raid.priority"][..],
            ),
            ("analysis_dashboard", &["analytics", "chat.lurker_tax"][..]),
            (
                "bundle_werbefrei_analyse",
                &["analytics", "chat.lurker_tax", "chat.promos.disable"][..],
            ),
            (
                "bundle_komplett",
                &[
                    "analytics",
                    "chat.lurker_tax",
                    "chat.promos.disable",
                    "raid.priority",
                ][..],
            ),
            (
                "bundle_analysis_raid_boost",
                &[
                    "analytics",
                    "chat.lurker_tax",
                    "chat.promos.disable",
                    "raid.priority",
                ][..],
            ),
            ("analytics_trial", &["analytics", "chat.lurker_tax"][..]),
        ] {
            assert!(is_known_plan_id(id), "{id} muss bekannt bleiben");
            assert_eq!(normalize_plan_id(id), id, "{id} darf nicht wegnormalisiert werden");
            assert_eq!(plan_entitlements(id), expected, "entitlements für {id}");
        }
    }

    // ── B20-ent-2: expiresAt isoformat-normalisiert ─────────────────────────

    #[test]
    fn normalize_expires_at_leer_und_unparsbar_ist_none() {
        assert_eq!(normalize_expires_at(None), None);
        assert_eq!(normalize_expires_at(Some("")), None);
        assert_eq!(normalize_expires_at(Some("   ")), None);
        assert_eq!(normalize_expires_at(Some("garbage")), None);
    }

    #[test]
    fn normalize_expires_at_date_only_wird_tagesende_utc() {
        // Python parse_datetime_value: "YYYY-MM-DD" → 23:59:59 UTC, dann isoformat.
        let got = normalize_expires_at(Some("2026-06-30")).unwrap();
        assert!(got.starts_with("2026-06-30T23:59:59"), "got={got}");
        assert!(got.ends_with("+00:00"), "muss UTC-Offset tragen: {got}");
    }

    #[test]
    fn normalize_expires_at_z_und_offset_nach_utc() {
        // Z → +00:00; bereits-UTC bleibt UTC.
        let got = normalize_expires_at(Some("2026-06-30T12:00:00Z")).unwrap();
        assert!(got.starts_with("2026-06-30T12:00:00"), "got={got}");
        assert!(got.ends_with("+00:00"), "got={got}");
        // Nicht-UTC-Offset wird nach UTC konvertiert (Python astimezone(UTC)).
        let got2 = normalize_expires_at(Some("2026-06-30T12:00:00+02:00")).unwrap();
        assert!(
            got2.starts_with("2026-06-30T10:00:00"),
            "Offset→UTC: {got2}"
        );
    }

    #[test]
    fn normalize_expires_at_naiv_wird_als_utc_interpretiert() {
        // Naiver Timestamp ohne Offset → Python nimmt UTC an.
        let got = normalize_expires_at(Some("2026-06-30T12:00:00")).unwrap();
        assert!(got.starts_with("2026-06-30T12:00:00"), "got={got}");
        assert!(got.ends_with("+00:00"), "naiv→UTC: {got}");
    }

    #[test]
    fn is_expired_date_only_und_fail_open() {
        // Date-only in der Vergangenheit → abgelaufen.
        assert!(is_expired_timestamp("2000-01-01"));
        // Weit in der Zukunft → nicht abgelaufen.
        assert!(!is_expired_timestamp("2999-01-01"));
        // Unparsbar → fail-open (nicht abgelaufen).
        assert!(!is_expired_timestamp("kaputt"));
        assert!(!is_expired_timestamp(""));
    }

    // ── B20-ent-3: PlanSnapshot-Felder ──────────────────────────────────────

    #[test]
    fn default_basic_snapshot_hat_alle_felder() {
        let snap = PlanSnapshot::default_basic("Nani");
        assert_eq!(snap.plan_id, "free");
        assert_eq!(snap.source, "default_basic");
        assert_eq!(snap.status, "active");
        assert_eq!(snap.customer_reference, "Nani"); // getrimmt durchgereicht
        assert_eq!(snap.expires_at, None);
        assert!(snap.manual_override.is_null());
        assert!(snap.billing_subscription.is_null());
    }

    // ── DB-Integration: voller resolve_plan_snapshot (skip ohne DB) ─────────

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn snapshot_pool(schema: &str) -> Option<PgPool> {
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
        // PK auf twitch_user_id + first_login_at, damit der Trial-Auto-Grant-Pfad
        // sauber durchläuft (er bleibt mangels first_login_at ein No-op).
        sqlx::query(
            "CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, \
             manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT, \
             manual_plan_updated_at TEXT, first_login_at TEXT, \
             trial_ever_granted INTEGER DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_billing_subscriptions (customer_reference TEXT, plan_id TEXT, \
             status TEXT, current_period_end TEXT, updated_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    /// Das eine Praedikat der Paywall (Pricing-Umbau 2026-08-09): `is_premium`
    /// und das davon abgeleitete Lesefenster, gegen alle drei Faelle.
    #[tokio::test]
    async fn is_premium_und_read_window_pro_plan() {
        let Some(pool) = snapshot_pool("plan_is_premium").await else {
            return;
        };
        for (user_id, login, plan) in [
            ("1", "kostenlos", "free"),
            ("2", "bezahlt", "premium"),
            ("3", "bestand", "analysis_dashboard"),
        ] {
            sqlx::query(
                "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id) \
                 VALUES ($1, $2, $3)",
            )
            .bind(user_id)
            .bind(login)
            .bind(plan)
            .execute(&pool)
            .await
            .unwrap();
        }

        assert!(!is_premium(&pool, "kostenlos").await.unwrap());
        assert!(is_premium(&pool, "bezahlt").await.unwrap());
        assert!(
            is_premium(&pool, "bestand").await.unwrap(),
            "alter Bezahlplan muss weiter als Premium gelten"
        );
        // Ohne Zeile in der Tabelle: Free.
        assert!(!is_premium(&pool, "gibtsnicht").await.unwrap());

        assert_eq!(read_window(&pool, "kostenlos").await, "last_stream");
        assert_eq!(read_window(&pool, "bezahlt").await, "full");
        assert_eq!(read_window(&pool, "bestand").await, "full");
        assert_eq!(read_window(&pool, "gibtsnicht").await, "last_stream");
    }

    /// Fail-closed: ohne die Plan-Tabellen (DB-Fehler) ist das Fenster
    /// `last_stream`, nicht `full`. Sonst oeffnet ein Ausfall die Paywall.
    #[tokio::test]
    async fn read_window_ist_bei_db_fehler_last_stream() {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            // Ohne kurzes Timeout wartet sqlx 30 s je Versuch.
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .unwrap();
        assert_eq!(read_window(&pool, "egal").await, "last_stream");
        assert!(is_premium(&pool, "egal").await.is_err());
    }

    #[tokio::test]
    async fn resolve_manual_override_full_snapshot() {
        let Some(pool) = snapshot_pool("plan_manual").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, \
             manual_plan_expires_at, manual_plan_notes, manual_plan_updated_at) \
             VALUES ('42', 'nani', 'raid_boost', '2999-01-15', 'comped', '2026-06-01T10:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let snap = resolve_plan_snapshot(&pool, "nani", "42").await.unwrap();
        assert_eq!(snap.plan_id, "raid_boost");
        assert_eq!(snap.source, "manual_override");
        assert_eq!(snap.status, "active");
        assert_eq!(snap.customer_reference, "nani"); // login bevorzugt
                                                     // expires_at normalisiert (Date-only → Tagesende UTC).
        let exp = snap.expires_at.as_deref().unwrap();
        assert!(exp.starts_with("2999-01-15T23:59:59"), "exp={exp}");
        // manual_override-Sub-Dict gefüllt, billing leer.
        assert_eq!(snap.manual_override["plan_id"], "raid_boost");
        assert_eq!(snap.manual_override["twitch_login"], "nani");
        assert_eq!(snap.manual_override["is_active"], true);
        assert_eq!(snap.manual_override["notes"], "comped");
        assert!(snap.billing_subscription.is_null());
    }

    #[tokio::test]
    async fn resolve_billing_full_snapshot_with_status() {
        let Some(pool) = snapshot_pool("plan_billing").await else {
            return;
        };
        // Kein Manual-Override; aktives Stripe-Abo mit trialing-Status.
        sqlx::query(
            "INSERT INTO twitch_billing_subscriptions (customer_reference, plan_id, status, \
             current_period_end, updated_at) \
             VALUES ('nani', 'analysis_dashboard', 'trialing', '2030-03-01T00:00:00Z', '2026-06-01T00:00:00Z')",
        ).execute(&pool).await.unwrap();

        let snap = resolve_plan_snapshot(&pool, "nani", "42").await.unwrap();
        assert_eq!(snap.plan_id, "analysis_dashboard");
        assert_eq!(snap.source, "billing_subscription");
        // status kommt aus dem Abo, nicht hartem "active".
        assert_eq!(snap.status, "trialing");
        assert_eq!(snap.customer_reference, "nani");
        assert!(snap.manual_override.is_null());
        assert_eq!(snap.billing_subscription["plan_id"], "analysis_dashboard");
        assert_eq!(snap.billing_subscription["status"], "trialing");
        let exp = snap.expires_at.as_deref().unwrap();
        assert!(exp.starts_with("2030-03-01T00:00:00"), "exp={exp}");
    }
}

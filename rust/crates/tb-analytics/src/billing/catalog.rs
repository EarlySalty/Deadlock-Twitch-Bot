//! Statischer Billing-Katalog: Plan-Blueprints, Zyklus-Rabatte, Preis-Logik
//! und die in Source eingecheckten Stripe-Price/Product-Defaults.
//!
//! Referenz (Orakel): `bot/dashboard/billing/billing_plans.py` und
//! `bot/entitlements/catalog.py`. Werte sind 1:1 übernommen; die Preis-Arithmetik
//! spiegelt `build_billing_catalog` exakt (inkl. Rundungs-Semantik).

/// Ein Plan-Blueprint des Katalogs.
///
/// Felder entsprechen den Schlüsseln eines Eintrags in Pythons `BILLING_PLANS`.
/// `tier`/`entitlements` stammen aus `entitlements/catalog.py` (`PLAN_TIER_MAP`
/// bzw. `plan_entitlements`, alphabetisch sortiert wie in Python).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillingPlan {
    /// Stabiler Plan-Identifier (z. B. `"chat_quiet"`).
    pub id: &'static str,
    /// Anzeigename des Plans.
    pub name: &'static str,
    /// Tier-Stufe (`"free" | "basic" | "extended"`).
    pub tier: &'static str,
    /// UI-Badge-Schlüssel.
    pub badge: &'static str,
    /// Marketing-Beschreibung.
    pub description: &'static str,
    /// Monatlicher Netto-Preis in Cent (`0` = kostenlos).
    pub monthly_net_cents: u32,
    /// Empfehlungs-Hervorhebung in der UI.
    pub recommended: bool,
    /// Freigeschaltete Entitlements (alphabetisch sortiert).
    pub entitlements: &'static [&'static str],
    /// Feature-Bulletpoints für die UI.
    pub features: &'static [&'static str],
}

/// Berechnetes Preis-Tableau eines Plans für einen Abrechnungszyklus.
///
/// Entspricht dem `price`-Objekt eines Plans im Python-Katalog (numerische
/// Felder; Label-Formatierung liegt in der späteren JSON-API-Schicht).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanPrice {
    /// Abrechnungszyklus in Monaten.
    pub cycle_months: u32,
    /// Zwischensumme netto in Cent (`monthly * cycle`).
    pub subtotal_net_cents: u32,
    /// Tatsächlich angewandter Rabatt-Prozentsatz (0, falls Zyklus ≤ 1).
    pub discount_percent: u32,
    /// Rabattbetrag in Cent (kaufmännisch gerundet, `(x*p + 50) / 100`).
    pub discount_cents: u32,
    /// Gesamtsumme netto in Cent (`subtotal - discount`).
    pub total_net_cents: u32,
    /// Effektiver Monatspreis netto in Cent (`total` auf Monate gerundet).
    pub effective_monthly_net_cents: u32,
}

/// Zyklus-Rabatte: `(Monate, Rabatt-Prozent)`. Spiegelt Pythons
/// `BILLING_CYCLE_DISCOUNTS = {1: 0, 12: 0}`.
pub const CYCLE_DISCOUNTS: &[(u32, u32)] = &[(1, 0), (12, 0)];

/// Die acht Plan-Blueprints — Reihenfolge identisch zu Pythons `BILLING_PLANS`.
pub const BILLING_PLANS: &[BillingPlan] = &[
    BillingPlan {
        id: "raid_free",
        name: "Raid Free",
        tier: "free",
        badge: "free",
        description: "Starte kostenlos mit automatischen Raids in die Community.",
        monthly_net_cents: 0,
        recommended: false,
        entitlements: &[],
        features: &[
            "Auto-Raid Grundfunktion bleibt aktiv",
            "Keine monatlichen Kosten für Basis-Raids",
            "Upgrade auf Raid Boost jederzeit moeglich",
        ],
    },
    BillingPlan {
        id: "chat_quiet",
        name: "Werbefrei",
        tier: "basic",
        badge: "quiet",
        description: "Discord-Werbung im eigenen Chat dauerhaft aus — kein Boost, keine Analytics.",
        monthly_net_cents: 199,
        recommended: false,
        entitlements: &["chat.promos.disable"],
        features: &[
            "Chat-Werbung des Bots dauerhaft deaktiviert",
            "Greift auch bei aktiven Admin-Promo-Events",
            "Jederzeit monatlich kündbar",
        ],
    },
    BillingPlan {
        id: "raid_boost",
        name: "Raid Boost",
        tier: "basic",
        badge: "raids",
        description: "Dein Kanal wird bevorzugt als Raid-Ziel vorgeschlagen — mehr eingehende Zuschauer.",
        monthly_net_cents: 199,
        recommended: false,
        entitlements: &["chat.lurker_tax", "raid.priority"],
        features: &[
            "Bevorzugte Platzierung im Raid-Netzwerk",
            "Sichtbarkeit auch bei deiner Inaktivität",
            "Lurker Steuer Erinnerungen für bekannte Lurker",
            "Kein Setup nötig — läuft automatisch",
        ],
    },
    BillingPlan {
        id: "bundle_chat_quiet_raid_boost",
        name: "Werbefrei + Raid Boost",
        tier: "basic",
        badge: "bundle",
        description: "Werbefrei + bevorzugte Raid-Platzierung im Paket — günstiger als einzeln.",
        monthly_net_cents: 349,
        recommended: false,
        entitlements: &[
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        features: &[
            "Chat-Werbung dauerhaft aus",
            "Bevorzugte Platzierung im Raid-Netzwerk",
            "Lurker Steuer Erinnerungen für bekannte Lurker",
            "Spart 49¢ gegenüber Einzelkauf",
        ],
    },
    BillingPlan {
        id: "analysis_dashboard",
        name: "Analyse Dashboard",
        tier: "extended",
        badge: "analytics",
        description: "Vollständiges Analytics-Dashboard mit Stream-Statistiken, Viewer-Kurven und Wachstumsvergleichen.",
        monthly_net_cents: 199,
        recommended: true,
        entitlements: &["analytics", "chat.lurker_tax"],
        features: &[
            "Viewer-Verlauf & Peak-Analyse pro Stream",
            "Zeitraumvergleiche und Wachstumstrends",
            "Lurker Steuer Erinnerungen für bekannte Lurker",
            "Follower- und Retention-Übersichten",
        ],
    },
    BillingPlan {
        id: "bundle_werbefrei_analyse",
        name: "Werbefrei + Analyse",
        tier: "extended",
        badge: "bundle",
        description: "Chat-Werbung dauerhaft aus + volles Analytics-Dashboard — günstiger als einzeln.",
        monthly_net_cents: 349,
        recommended: false,
        entitlements: &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
        ],
        features: &[
            "Chat-Werbung dauerhaft deaktiviert",
            "Vollständiges Analytics-Dashboard",
            "KI-Analyse & Viewer-Auswertung",
            "Spart 49¢ gegenüber Einzelkauf",
        ],
    },
    BillingPlan {
        id: "bundle_komplett",
        name: "Alles drin",
        tier: "extended",
        badge: "bundle",
        description: "Werbefrei + Raid Boost + Analytics — das komplette Paket zum besten Preis.",
        monthly_net_cents: 499,
        recommended: false,
        entitlements: &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        features: &[
            "Alle Features aus allen Plänen",
            "Bevorzugte Raid-Platzierung aktiv",
            "Volles Analytics + KI-Analyse",
            "Spart 0,98€ gegenüber Einzelkauf",
        ],
    },
    BillingPlan {
        id: "bundle_analysis_raid_boost",
        name: "Bundle: Analyse + Raid Boost",
        tier: "extended",
        badge: "bundle",
        description: "Analyse Dashboard + Raid Boost im Paket — günstiger als einzeln.",
        monthly_net_cents: 349,
        recommended: false,
        entitlements: &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        features: &[
            "Alle Analytics-Features inklusive",
            "Bevorzugte Raid-Platzierung aktiv",
            "Lurker Steuer Erinnerungen für bekannte Lurker",
            "Spart 49¢ gegenüber Einzelkauf",
        ],
    },
];

/// In Source eingecheckte Stripe-Price-IDs (keine Secrets). `(plan_id, &[(cycle, price_id)])`.
/// Spiegelt `STRIPE_PRICE_ID_DEFAULTS`. `raid_free` fehlt (kostenlos, kein Stripe-Price).
pub const PRICE_ID_DEFAULTS: &[(&str, &[(u32, &str)])] = &[
    (
        "chat_quiet",
        &[
            (1, "price_1TeNGF0yU8I2yGJ0crjsfhHO"),
            (12, "price_1TeNGF0yU8I2yGJ0YLkz7PCX"),
        ],
    ),
    (
        "raid_boost",
        &[
            (1, "price_1TeNGG0yU8I2yGJ0DhWzKQWU"),
            (12, "price_1TeNGG0yU8I2yGJ0f9iYs3w1"),
        ],
    ),
    (
        "analysis_dashboard",
        &[
            (1, "price_1TeNGH0yU8I2yGJ0UqKylecO"),
            (12, "price_1TeNGH0yU8I2yGJ0tdHu8izl"),
        ],
    ),
    (
        "bundle_chat_quiet_raid_boost",
        &[
            (1, "price_1TeNGH0yU8I2yGJ06sCbRobW"),
            (12, "price_1TeNGI0yU8I2yGJ0GaUNdWmK"),
        ],
    ),
    (
        "bundle_werbefrei_analyse",
        &[
            (1, "price_1TeNGI0yU8I2yGJ0YX5iUzX4"),
            (12, "price_1TeNGJ0yU8I2yGJ0NlPVBIHZ"),
        ],
    ),
    (
        "bundle_komplett",
        &[
            (1, "price_1TeNGJ0yU8I2yGJ0V8gH6IGg"),
            (12, "price_1TeNGK0yU8I2yGJ0QTewVRfi"),
        ],
    ),
    (
        "bundle_analysis_raid_boost",
        &[
            (1, "price_1TeNGK0yU8I2yGJ0guZX1iD8"),
            (12, "price_1TeNGL0yU8I2yGJ0Alhd0ZPo"),
        ],
    ),
];

/// In Source eingecheckte Stripe-Product-IDs (keine Secrets). Spiegelt `STRIPE_PRODUCT_ID_DEFAULTS`.
pub const PRODUCT_ID_DEFAULTS: &[(&str, &str)] = &[
    ("chat_quiet", "prod_UYKKvIg1sbjVrl"),
    ("bundle_chat_quiet_raid_boost", "prod_UYKKwFHm0ozy5w"),
    ("bundle_werbefrei_analyse", "prod_UYJjXXe90gt8WO"),
    ("bundle_komplett", "prod_UYJjhWpzqyNqr0"),
];

/// Normalisiert einen Roh-Zyklus auf einen bekannten Wert.
///
/// Entspricht Pythons `normalize_billing_cycle`: unbekannte Zyklen (alles außer
/// `1`/`12`) fallen auf `1` zurück.
pub fn normalize_billing_cycle(raw_cycle: u32) -> u32 {
    if CYCLE_DISCOUNTS.iter().any(|(cycle, _)| *cycle == raw_cycle) {
        raw_cycle
    } else {
        1
    }
}

/// Roh-Rabatt-Prozentsatz eines Zyklus (`0`, falls Zyklus unbekannt).
pub fn cycle_discount_percent(cycle_months: u32) -> u32 {
    CYCLE_DISCOUNTS
        .iter()
        .find(|(cycle, _)| *cycle == cycle_months)
        .map(|(_, discount)| *discount)
        .unwrap_or(0)
}

/// Findet einen Plan-Blueprint per ID.
pub fn find_plan(plan_id: &str) -> Option<&'static BillingPlan> {
    BILLING_PLANS.iter().find(|plan| plan.id == plan_id)
}

/// `true`, wenn der Plan kostenpflichtig ist (`monthly_net_cents > 0`).
/// Spiegelt `billing_is_paid_plan_id` / `PAID_PLAN_IDS`.
pub fn is_paid_plan_id(plan_id: &str) -> bool {
    find_plan(plan_id).is_some_and(|plan| plan.monthly_net_cents > 0)
}

/// Stripe-Lookup-Key eines Plans für einen Zyklus.
///
/// Format identisch zu `routes_billing.py`: `deadlock_{plan_id}_{cycle}m_net_v2`.
pub fn lookup_key(plan_id: &str, cycle_months: u32) -> String {
    format!("deadlock_{plan_id}_{cycle_months}m_net_v2")
}

/// Default-Price-ID eines Plans für einen Zyklus (aus [`PRICE_ID_DEFAULTS`]).
pub fn price_id_default(plan_id: &str, cycle_months: u32) -> Option<&'static str> {
    PRICE_ID_DEFAULTS
        .iter()
        .find(|(id, _)| *id == plan_id)
        .and_then(|(_, cycles)| {
            cycles
                .iter()
                .find(|(cycle, _)| *cycle == cycle_months)
                .map(|(_, price_id)| *price_id)
        })
}

/// Plan-ID aus einem Stripe-Lookup-Key (`deadlock_{plan}_{cycle}m_{...}`).
///
/// Der Suffix hinter dem Zyklus-Segment ist versioniert (`net_v2`, `gross_v3`),
/// deshalb wird nicht auf einen festen Suffix geprüft, sondern bis zum ersten
/// Segment der Form `<zahl>m` gelesen. `None`, wenn das Format nicht passt.
pub fn plan_id_from_lookup_key(lookup_key: &str) -> Option<String> {
    let rest = lookup_key.trim().strip_prefix("deadlock_")?;
    let segments: Vec<&str> = rest.split('_').collect();
    let cycle_at = segments.iter().position(|seg| {
        seg.strip_suffix('m')
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    })?;
    if cycle_at == 0 {
        return None;
    }
    Some(segments[..cycle_at].join("_"))
}

/// Plan-ID aus einer Stripe-Price-ID (Rückwärts-Suche in [`PRICE_ID_DEFAULTS`]).
///
/// Nötig für den Webhook: Prices ohne `lookup_key` und ohne `metadata.plan_id`
/// liefern sonst gar keine Plan-Zuordnung, und `twitch_billing_subscriptions.plan_id`
/// bleibt leer.
pub fn plan_id_from_price_id(price_id: &str) -> Option<&'static str> {
    let needle = price_id.trim();
    if needle.is_empty() {
        return None;
    }
    PRICE_ID_DEFAULTS
        .iter()
        .find(|(_, cycles)| cycles.iter().any(|(_, pid)| *pid == needle))
        .map(|(plan_id, _)| *plan_id)
}

/// Default-Product-ID eines Plans (aus [`PRODUCT_ID_DEFAULTS`]).
pub fn product_id_default(plan_id: &str) -> Option<&'static str> {
    PRODUCT_ID_DEFAULTS
        .iter()
        .find(|(id, _)| *id == plan_id)
        .map(|(_, product_id)| *product_id)
}

/// Reine Preis-Arithmetik für einen Monatspreis, Zyklus und Roh-Rabatt.
///
/// Spiegelt den Preisblock aus Pythons `build_billing_catalog` 1:1, inklusive
/// kaufmännischer Rundung (`(x*p + 50) / 100`) und der Effektiv-Monatsrundung
/// (`(total + cycle/2) / cycle`). Da alle Werte nicht-negativ sind, entspricht
/// Rusts Integer-Division Pythons `//`.
pub fn compute_plan_price(monthly_net_cents: u32, cycle_months: u32, cycle_discount: u32) -> PlanPrice {
    let cycle = cycle_months;
    let subtotal = monthly_net_cents.saturating_mul(cycle);
    let discount_percent = if cycle > 1 && subtotal > 0 { cycle_discount } else { 0 };
    let discount_cents = if discount_percent > 0 {
        (subtotal.saturating_mul(discount_percent) + 50) / 100
    } else {
        0
    };
    let total = subtotal.saturating_sub(discount_cents);
    // Python: (total + cycle//2) // cycle if cycle > 0 else total.
    let effective_monthly = (total + cycle / 2).checked_div(cycle).unwrap_or(total);
    PlanPrice {
        cycle_months: cycle,
        subtotal_net_cents: subtotal,
        discount_percent,
        discount_cents,
        total_net_cents: total,
        effective_monthly_net_cents: effective_monthly,
    }
}

impl BillingPlan {
    /// Preis-Tableau dieses Plans für einen (zu normalisierenden) Zyklus.
    pub fn price_for_cycle(&self, cycle_months: u32) -> PlanPrice {
        let cycle = normalize_billing_cycle(cycle_months);
        compute_plan_price(self.monthly_net_cents, cycle, cycle_discount_percent(cycle))
    }

    /// Stripe-Lookup-Key dieses Plans für einen Zyklus.
    pub fn lookup_key(&self, cycle_months: u32) -> String {
        lookup_key(self.id, cycle_months)
    }
}

/// Stripe-Onboarding-Doku-Link (Python `BILLING_STRIPE_QUICKSTART_URL`).
pub const STRIPE_QUICKSTART_URL: &str = "https://docs.stripe.com/billing/quickstart";

/// Geplante (noch nicht aktivierte) Zahlungsmethoden für die Katalog-`payment`-
/// Sektion (Python `supported_methods_planned`).
pub const SUPPORTED_METHODS_PLANNED: &[&str] =
    &["card", "sepa_debit", "paypal_via_wallet_if_enabled"];

/// Aus der Readiness abgeleiteter Zahlungs-Integrationszustand.
///
/// Port von `billing_plans.py:billing_payment_state_from_readiness`:
/// `integration_state` wird aus der Readiness übernommen, sonst `"live"` wenn
/// Checkout- UND Price-Map-Readiness vorliegen, sonst `"planned"`.
/// `checkout_enabled` = beide Readiness-Flags gesetzt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaymentState {
    pub integration_state: &'static str,
    pub checkout_enabled: bool,
}

/// Leitet [`PaymentState`] aus den Readiness-Flags ab.
///
/// `integration_state_override` entspricht Pythons `readiness["integration_state"]`
/// (leer → automatisch ableiten).
pub fn payment_state_from_readiness(
    checkout_ready: bool,
    price_map_ready: bool,
    integration_state_override: Option<&str>,
) -> PaymentState {
    let integration_state = match integration_state_override.map(str::trim) {
        Some(s) if !s.is_empty() => {
            if s == "live" {
                "live"
            } else {
                "planned"
            }
        }
        _ => {
            if checkout_ready && price_map_ready {
                "live"
            } else {
                "planned"
            }
        }
    };
    PaymentState {
        integration_state,
        checkout_enabled: checkout_ready && price_map_ready,
    }
}

/// Formatiert Cent als deutschen EUR-String (`199` → `"1,99 EUR"`).
///
/// Port von `billing_plans.py:format_eur_cents` (negative Werte → `0`).
pub fn format_eur_cents(cents: i64) -> String {
    let cents = cents.max(0);
    let euros = cents / 100;
    let remainder = cents % 100;
    format!("{euros},{remainder:02} EUR")
}

/// Zyklus-Label (`1` → `"30 Tage"`, sonst `"{n} Monate"`).
///
/// Port von `billing_plans.py:billing_cycle_label`.
pub fn cycle_label(cycle_months: u32) -> String {
    if cycle_months == 1 {
        "30 Tage".to_string()
    } else {
        format!("{cycle_months} Monate")
    }
}

/// Baut die `build_billing_catalog`-JSON-Struktur für einen Zyklus.
///
/// Wert-identischer Port von `billing_plans.py:build_billing_catalog`. Liefert
/// `currency`/`tax_mode`/`gross_available`/`cycle_*`/`discount_percent`/`plans`.
/// Die `payment`-Sektion und plan-spezifische Felder (`is_current`,
/// `stripe_price_id`, `checkout_available`) werden in der HTTP-Schicht ergänzt
/// (analog Python `api_billing_catalog`).
pub fn catalog_json(cycle_months: u32) -> serde_json::Value {
    let cycle = normalize_billing_cycle(cycle_months);
    let cycle_lbl = cycle_label(cycle);
    let plans: Vec<serde_json::Value> = BILLING_PLANS
        .iter()
        .map(|plan| {
            let price = plan.price_for_cycle(cycle);
            serde_json::json!({
                "id": plan.id,
                "name": plan.name,
                "tier": plan.tier,
                "badge": plan.badge,
                "description": plan.description,
                "recommended": plan.recommended,
                "monthly_net_cents": plan.monthly_net_cents,
                "entitlements": plan.entitlements,
                "features": plan.features,
                "price": {
                    "cycle_months": price.cycle_months,
                    "cycle_label": cycle_lbl,
                    "subtotal_net_cents": price.subtotal_net_cents,
                    "discount_percent": price.discount_percent,
                    "discount_cents": price.discount_cents,
                    "total_net_cents": price.total_net_cents,
                    "effective_monthly_net_cents": price.effective_monthly_net_cents,
                    "subtotal_net_label": format_eur_cents(price.subtotal_net_cents as i64),
                    "total_net_label": format_eur_cents(price.total_net_cents as i64),
                    "effective_monthly_net_label": format_eur_cents(
                        price.effective_monthly_net_cents as i64,
                    ),
                },
            })
        })
        .collect();

    serde_json::json!({
        "currency": "EUR",
        "tax_mode": "net_only",
        "gross_available": false,
        "cycle_months": cycle,
        "cycle_label": cycle_lbl,
        "discount_percent": if cycle > 1 { cycle_discount_percent(cycle) } else { 0 },
        "plans": plans,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// P2.126: Stripe Price-/Product-ID Vault-Override-Layer
// ─────────────────────────────────────────────────────────────────────────────
//
// Port von `billing_mixin.py:_billing_price_id_map`/`_billing_product_id_map` +
// `billing_plans.py:billing_parse_*_mapping`/`billing_merge_*_defaults`.
//
// Die Maps stammen aus den Env-/Infisical-Variablen `STRIPE_PRICE_ID_MAP` /
// `STRIPE_PRODUCT_ID_MAP` (Alias `TWITCH_BILLING_STRIPE_*`, erster nicht-leerer
// gewinnt) und werden über die eingecheckten Defaults gelegt. **Price-IDs:**
// Code-Defaults gewinnen für bekannte Pläne; das Vault kann nur NEUE (noch nicht
// eingecheckte) Pläne ergänzen. **Product-IDs:** Vault gewinnt (Python
// `result.update(mapping)`). Das sind keine Secrets, daher Plaintext-Env zulässig
// (Direktive: Secrets read-only aus Infisical/Env; hier nur ID-Strings).
//
// Schreib-Rückweg (Python `_billing_set_*_map` via Keyring) liegt im
// Sync-Handler (anderes Crate) und ist Folge-Wiring (siehe WIRING-TODO).

/// Geparste Price-Map: `(plan_id, [(cycle_months, price_id)])` (normalisiert).
type PriceMap = Vec<(String, Vec<(u32, String)>)>;
/// Geparste Product-Map: `(plan_id, product_id)`.
type ProductMap = Vec<(String, String)>;

/// Liest die erste nicht-leere Env-Variable aus `keys` (getrimmt).
fn first_env(keys: &[&str]) -> String {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

/// Parst eine JSON-Price-Map (`{"plan":{"1":"price_x","12":"price_y"}}`).
///
/// Port von `billing_parse_price_id_mapping`: ungültiges JSON / Nicht-Objekt → leer;
/// Zyklen außerhalb `{1,12}` und leere IDs werden verworfen; nur Pläne mit
/// mindestens einem gültigen Slot bleiben erhalten.
pub fn parse_price_id_mapping(raw: &str) -> PriceMap {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let mut result: PriceMap = Vec::new();
    for (raw_plan_id, raw_cycle_map) in obj {
        let plan_id = raw_plan_id.trim().to_string();
        let serde_json::Value::Object(cycle_obj) = raw_cycle_map else {
            continue;
        };
        if plan_id.is_empty() {
            continue;
        }
        let mut cycle_map: Vec<(u32, String)> = Vec::new();
        for (raw_cycle, raw_price_id) in cycle_obj {
            let Some(cycle) = parse_cycle_key(&raw_cycle) else {
                continue;
            };
            let price_id = raw_price_id.as_str().unwrap_or("").trim().to_string();
            if !price_id.is_empty() {
                cycle_map.retain(|(c, _)| *c != cycle);
                cycle_map.push((cycle, price_id));
            }
        }
        if !cycle_map.is_empty() {
            result.push((plan_id, cycle_map));
        }
    }
    result
}

/// Parst eine JSON-Product-Map (`{"plan":"prod_x"}`).
///
/// Port von `billing_parse_product_id_mapping`: nur nicht-leere Plan-/Product-IDs.
pub fn parse_product_id_mapping(raw: &str) -> ProductMap {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let mut result: ProductMap = Vec::new();
    for (raw_plan_id, raw_product_id) in obj {
        let plan_id = raw_plan_id.trim().to_string();
        let product_id = raw_product_id.as_str().unwrap_or("").trim().to_string();
        if !plan_id.is_empty() && !product_id.is_empty() {
            result.retain(|(p, _)| *p != plan_id);
            result.push((plan_id, product_id));
        }
    }
    result
}

/// Parst einen Zyklus-Schlüssel; nur `{1,12}` sind gültig (Python
/// `billing_parse_cycle_key`).
fn parse_cycle_key(raw: &str) -> Option<u32> {
    let cycle: u32 = raw.trim().parse().ok()?;
    CYCLE_DISCOUNTS
        .iter()
        .any(|(c, _)| *c == cycle)
        .then_some(cycle)
}

/// Effektive Price-ID eines Plans für einen Zyklus mit Vault-Override.
///
/// Reihenfolge (Python `_billing_price_id_for_plan` + `billing_merge_price_id_defaults`):
/// eingecheckter Default gewinnt für bekannte Pläne; nur für Pläne OHNE
/// eingecheckten Default greift die übergebene Vault-Map. `vault_price_map` kommt
/// aus [`parse_price_id_mapping`]; in Produktion via [`price_id_map_from_env`].
pub fn resolved_price_id(plan_id: &str, cycle_months: u32, vault_price_map: &PriceMap) -> Option<String> {
    let cycle = normalize_billing_cycle(cycle_months);
    if let Some(default) = price_id_default(plan_id, cycle) {
        return Some(default.to_string());
    }
    // Kein eingecheckter Default → Vault darf den (neuen) Plan liefern.
    vault_price_map
        .iter()
        .find(|(id, _)| id == plan_id)
        .and_then(|(_, cycles)| cycles.iter().find(|(c, _)| *c == cycle))
        .map(|(_, price_id)| price_id.clone())
}

/// Effektive Product-ID eines Plans mit Vault-Override (Vault gewinnt, Python
/// `result.update(mapping)`).
pub fn resolved_product_id(plan_id: &str, vault_product_map: &ProductMap) -> Option<String> {
    if let Some((_, product_id)) = vault_product_map.iter().find(|(id, _)| id == plan_id) {
        return Some(product_id.clone());
    }
    product_id_default(plan_id).map(str::to_string)
}

/// Liest die Price-Map aus der Umgebung (`STRIPE_PRICE_ID_MAP`, Alias
/// `TWITCH_BILLING_STRIPE_PRICE_ID_MAP`) und parst sie.
pub fn price_id_map_from_env() -> PriceMap {
    parse_price_id_mapping(&first_env(&[
        "STRIPE_PRICE_ID_MAP",
        "TWITCH_BILLING_STRIPE_PRICE_ID_MAP",
    ]))
}

/// Liest die Product-Map aus der Umgebung (`STRIPE_PRODUCT_ID_MAP`, Alias
/// `TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP`) und parst sie.
pub fn product_id_map_from_env() -> ProductMap {
    parse_product_id_mapping(&first_env(&[
        "STRIPE_PRODUCT_ID_MAP",
        "TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP",
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Erwartete Monatspreise je Plan — Orakel aus `billing_plans.py`.
    /// `(id, monthly_net_cents, tier, recommended)`.
    const EXPECTED: &[(&str, u32, &str, bool)] = &[
        ("raid_free", 0, "free", false),
        ("chat_quiet", 199, "basic", false),
        ("raid_boost", 199, "basic", false),
        ("bundle_chat_quiet_raid_boost", 349, "basic", false),
        ("analysis_dashboard", 199, "extended", true),
        ("bundle_werbefrei_analyse", 349, "extended", false),
        ("bundle_komplett", 499, "extended", false),
        ("bundle_analysis_raid_boost", 349, "extended", false),
    ];

    #[test]
    fn catalog_has_exactly_eight_plans_in_python_order() {
        assert_eq!(BILLING_PLANS.len(), 8);
        for (plan, exp) in BILLING_PLANS.iter().zip(EXPECTED.iter()) {
            assert_eq!(plan.id, exp.0, "plan id order mismatch");
            assert_eq!(plan.monthly_net_cents, exp.1, "monthly for {}", exp.0);
            assert_eq!(plan.tier, exp.2, "tier for {}", exp.0);
            assert_eq!(plan.recommended, exp.3, "recommended for {}", exp.0);
        }
    }

    #[test]
    fn cycle_discounts_match_python_zero_zero() {
        assert_eq!(CYCLE_DISCOUNTS, &[(1, 0), (12, 0)]);
    }

    #[test]
    fn payment_state_derives_like_python() {
        // Beide Readiness-Flags → live + checkout enabled.
        let live = payment_state_from_readiness(true, true, None);
        assert_eq!(live.integration_state, "live");
        assert!(live.checkout_enabled);
        // Nur eins → planned + disabled.
        let half = payment_state_from_readiness(true, false, None);
        assert_eq!(half.integration_state, "planned");
        assert!(!half.checkout_enabled);
        // Override "live" gewinnt; "planned" bleibt planned.
        assert_eq!(
            payment_state_from_readiness(false, false, Some("live")).integration_state,
            "live"
        );
        assert_eq!(
            payment_state_from_readiness(true, true, Some("planned")).integration_state,
            "planned"
        );
        // Leerer Override → automatische Ableitung.
        assert_eq!(
            payment_state_from_readiness(false, true, Some("  ")).integration_state,
            "planned"
        );
    }

    /// Kern-Orakel: 8 Pläne × Zyklen {1, 12} ergeben wert-identische Preise/lookup_keys.
    /// Bei 0 % Rabatt gilt: subtotal = monthly*cycle, total = subtotal,
    /// effective_monthly = monthly (Rundung verschwindet, da +cycle/2 < cycle).
    #[test]
    fn prices_and_lookup_keys_value_identical_to_python() {
        for (id, monthly, _tier, _rec) in EXPECTED {
            let plan = find_plan(id).expect("plan present");
            for &cycle in &[1u32, 12u32] {
                let price = plan.price_for_cycle(cycle);
                assert_eq!(price.cycle_months, cycle);
                assert_eq!(price.subtotal_net_cents, monthly * cycle, "subtotal {id}/{cycle}m");
                assert_eq!(price.discount_percent, 0, "discount_percent {id}/{cycle}m");
                assert_eq!(price.discount_cents, 0, "discount_cents {id}/{cycle}m");
                assert_eq!(price.total_net_cents, monthly * cycle, "total {id}/{cycle}m");
                assert_eq!(
                    price.effective_monthly_net_cents, *monthly,
                    "effective_monthly {id}/{cycle}m"
                );
                assert_eq!(
                    plan.lookup_key(cycle),
                    format!("deadlock_{id}_{cycle}m_net_v2"),
                    "lookup_key {id}/{cycle}m"
                );
            }
        }
    }

    /// Validiert die Rundungs-Semantik unabhängig von den (aktuell 0 %) Live-Rabatten:
    /// hypothetischer 10 %-Rabatt auf 199 ¢ × 12 → exakt wie Pythons Integer-Arithmetik.
    #[test]
    fn rounding_semantics_match_python_with_synthetic_discount() {
        // subtotal = 199*12 = 2388; discount = (2388*10 + 50)/100 = 239; total = 2149;
        // effective = (2149 + 6)/12 = 179
        let price = compute_plan_price(199, 12, 10);
        assert_eq!(price.subtotal_net_cents, 2388);
        assert_eq!(price.discount_percent, 10);
        assert_eq!(price.discount_cents, 239);
        assert_eq!(price.total_net_cents, 2149);
        assert_eq!(price.effective_monthly_net_cents, 179);
    }

    #[test]
    fn cycle_normalization_falls_back_to_one() {
        assert_eq!(normalize_billing_cycle(1), 1);
        assert_eq!(normalize_billing_cycle(12), 12);
        assert_eq!(normalize_billing_cycle(0), 1);
        assert_eq!(normalize_billing_cycle(6), 1);
        assert_eq!(normalize_billing_cycle(3), 1);
    }

    #[test]
    fn paid_plan_predicate_matches_monthly_price() {
        assert!(!is_paid_plan_id("raid_free"));
        assert!(is_paid_plan_id("chat_quiet"));
        assert!(is_paid_plan_id("bundle_komplett"));
        assert!(!is_paid_plan_id("unknown_plan"));
    }

    #[test]
    fn price_and_product_id_defaults_value_identical() {
        // Spot-Checks gegen STRIPE_PRICE_ID_DEFAULTS / STRIPE_PRODUCT_ID_DEFAULTS.
        assert_eq!(
            price_id_default("chat_quiet", 1),
            Some("price_1TeNGF0yU8I2yGJ0crjsfhHO")
        );
        assert_eq!(
            price_id_default("bundle_komplett", 12),
            Some("price_1TeNGK0yU8I2yGJ0QTewVRfi")
        );
        // raid_free hat keinen Stripe-Price (kostenlos).
        assert_eq!(price_id_default("raid_free", 1), None);
        assert_eq!(product_id_default("chat_quiet"), Some("prod_UYKKvIg1sbjVrl"));
        assert_eq!(product_id_default("raid_boost"), None);

        // Jeder kostenpflichtige Plan hat Price-IDs für beide Zyklen.
        for plan in BILLING_PLANS.iter().filter(|p| p.monthly_net_cents > 0) {
            assert!(
                price_id_default(plan.id, 1).is_some(),
                "missing 1m price id for {}",
                plan.id
            );
            assert!(
                price_id_default(plan.id, 12).is_some(),
                "missing 12m price id for {}",
                plan.id
            );
        }
    }

    #[test]
    fn plan_id_from_lookup_key_zerlegt_beide_key_generationen() {
        assert_eq!(
            plan_id_from_lookup_key("deadlock_chat_quiet_1m_net_v2").as_deref(),
            Some("chat_quiet")
        );
        assert_eq!(
            plan_id_from_lookup_key("deadlock_premium_12m_gross_v3").as_deref(),
            Some("premium")
        );
        assert_eq!(
            plan_id_from_lookup_key("deadlock_bundle_analysis_raid_boost_1m_net_v2").as_deref(),
            Some("bundle_analysis_raid_boost")
        );
        // Kein deadlock-Prefix, kein Zyklus-Segment, leerer Plan-Teil → None.
        assert_eq!(plan_id_from_lookup_key("fallback_key"), None);
        assert_eq!(plan_id_from_lookup_key("deadlock_premium_gross_v3"), None);
        assert_eq!(plan_id_from_lookup_key("deadlock_1m_net_v2"), None);
        assert_eq!(plan_id_from_lookup_key(""), None);
    }

    #[test]
    fn plan_id_from_price_id_findet_jeden_eingecheckten_price() {
        for (plan_id, cycles) in PRICE_ID_DEFAULTS {
            for (_, price) in *cycles {
                assert_eq!(
                    plan_id_from_price_id(price),
                    Some(*plan_id),
                    "price {price} muss auf {plan_id} zurückführen"
                );
            }
        }
        assert_eq!(plan_id_from_price_id("price_unbekannt"), None);
        assert_eq!(plan_id_from_price_id("  "), None);
    }

    #[test]
    fn format_eur_cents_matches_python() {
        assert_eq!(format_eur_cents(0), "0,00 EUR");
        assert_eq!(format_eur_cents(199), "1,99 EUR");
        assert_eq!(format_eur_cents(349), "3,49 EUR");
        assert_eq!(format_eur_cents(2388), "23,88 EUR");
        assert_eq!(format_eur_cents(5), "0,05 EUR");
        // Negativ → 0 (Python: max(int(cents), 0)).
        assert_eq!(format_eur_cents(-50), "0,00 EUR");
    }

    #[test]
    fn cycle_label_matches_python() {
        assert_eq!(cycle_label(1), "30 Tage");
        assert_eq!(cycle_label(12), "12 Monate");
        assert_eq!(cycle_label(3), "3 Monate");
    }

    #[test]
    fn catalog_json_shape_and_values() {
        let cat = catalog_json(1);
        assert_eq!(cat["currency"], "EUR");
        assert_eq!(cat["tax_mode"], "net_only");
        assert_eq!(cat["gross_available"], false);
        assert_eq!(cat["cycle_months"], 1);
        assert_eq!(cat["cycle_label"], "30 Tage");
        assert_eq!(cat["discount_percent"], 0);
        let plans = cat["plans"].as_array().unwrap();
        assert_eq!(plans.len(), 8);
        // Erster Plan = raid_free (kostenlos).
        assert_eq!(plans[0]["id"], "raid_free");
        assert_eq!(plans[0]["price"]["total_net_cents"], 0);
        assert_eq!(plans[0]["price"]["total_net_label"], "0,00 EUR");
        // chat_quiet → 1,99 EUR.
        let chat_quiet = plans.iter().find(|p| p["id"] == "chat_quiet").unwrap();
        assert_eq!(chat_quiet["price"]["total_net_cents"], 199);
        assert_eq!(chat_quiet["price"]["total_net_label"], "1,99 EUR");
        assert_eq!(chat_quiet["tier"], "basic");
        assert_eq!(chat_quiet["price"]["cycle_label"], "30 Tage");
        // 12-Monats-Zyklus: subtotal = monthly*12, kein Rabatt (0%).
        let cat12 = catalog_json(12);
        assert_eq!(cat12["cycle_months"], 12);
        assert_eq!(cat12["cycle_label"], "12 Monate");
        let cq12 = cat12["plans"].as_array().unwrap().iter().find(|p| p["id"] == "chat_quiet").unwrap();
        assert_eq!(cq12["price"]["subtotal_net_cents"], 2388);
        assert_eq!(cq12["price"]["total_net_cents"], 2388);
        assert_eq!(cq12["price"]["total_net_label"], "23,88 EUR");
        // Unbekannter Zyklus fällt auf 1 zurück.
        assert_eq!(catalog_json(7)["cycle_months"], 1);
    }

    // ── P2.126: Vault-Override-Layer ────────────────────────────────────────
    #[test]
    fn parse_price_id_mapping_normalizes_and_filters() {
        let raw = r#"{
            "new_plan": {"1": "price_new_1m", "12": "price_new_12m", "6": "price_invalid_cycle"},
            "leer": {"1": "  "},
            "  ": {"1": "x"}
        }"#;
        let map = parse_price_id_mapping(raw);
        // "leer" (nur leere ID) und "" (leerer Plan) fallen raus.
        assert_eq!(map.len(), 1);
        let (plan, cycles) = &map[0];
        assert_eq!(plan, "new_plan");
        // Zyklus 6 ist ungültig → verworfen; nur 1 + 12 bleiben.
        assert_eq!(cycles.len(), 2);
        assert!(cycles.contains(&(1, "price_new_1m".to_string())));
        assert!(cycles.contains(&(12, "price_new_12m".to_string())));
        // Ungültiges JSON / Nicht-Objekt → leer.
        assert!(parse_price_id_mapping("nicht json").is_empty());
        assert!(parse_price_id_mapping("[1,2]").is_empty());
        assert!(parse_price_id_mapping("").is_empty());
    }

    #[test]
    fn resolved_price_id_default_wins_for_known_plan() {
        // Vault versucht raid_boost umzubiegen — Default gewinnt (bekannter Plan).
        let vault = parse_price_id_mapping(r#"{"raid_boost": {"1": "price_VAULT_HIJACK"}}"#);
        assert_eq!(
            resolved_price_id("raid_boost", 1, &vault).as_deref(),
            Some("price_1TeNGG0yU8I2yGJ0DhWzKQWU")
        );
    }

    #[test]
    fn resolved_price_id_vault_adds_new_plan() {
        // Neuer Plan ohne eingecheckten Default → Vault liefert die ID.
        let vault = parse_price_id_mapping(r#"{"future_plan": {"1": "price_future_1m"}}"#);
        assert_eq!(
            resolved_price_id("future_plan", 1, &vault).as_deref(),
            Some("price_future_1m")
        );
        // Ohne Vault-Eintrag und ohne Default → None.
        assert_eq!(resolved_price_id("future_plan", 12, &vault), None);
    }

    #[test]
    fn resolved_product_id_vault_wins() {
        // Product-IDs: Vault gewinnt (Python result.update).
        let vault = parse_product_id_mapping(r#"{"chat_quiet": "prod_VAULT_OVERRIDE"}"#);
        assert_eq!(
            resolved_product_id("chat_quiet", &vault).as_deref(),
            Some("prod_VAULT_OVERRIDE")
        );
        // Ohne Vault-Eintrag → eingecheckter Default.
        let empty = parse_product_id_mapping("");
        assert_eq!(
            resolved_product_id("chat_quiet", &empty).as_deref(),
            Some("prod_UYKKvIg1sbjVrl")
        );
        // Plan ohne Default und ohne Vault → None.
        assert_eq!(resolved_product_id("raid_boost", &empty), None);
    }

    #[test]
    fn parse_product_id_mapping_filters_empty() {
        let map = parse_product_id_mapping(r#"{"a": "prod_a", "b": "", "  ": "prod_c"}"#);
        assert_eq!(map.len(), 1);
        assert_eq!(map[0], ("a".to_string(), "prod_a".to_string()));
    }

    #[test]
    fn entitlements_are_sorted() {
        // raid_free trägt nach der Analytics-Konsolidierung keine Entitlements
        // mehr (kein Flag => last_stream-Default). Sortier-Invariante bleibt für
        // alle nicht-leeren Listen bestehen (Python plan_entitlements sortiert).
        for plan in BILLING_PLANS {
            let mut sorted = plan.entitlements.to_vec();
            sorted.sort_unstable();
            assert_eq!(
                plan.entitlements, &sorted[..],
                "entitlements for {} must be sorted (Python plan_entitlements sorts)",
                plan.id
            );
        }
    }

    /// Drift-Guard: für jeden bekannten Plan stimmen die Katalog-Entitlements mit
    /// [`crate::plan::plan_entitlements`] überein (eine Quelle der Wahrheit).
    #[test]
    fn catalog_entitlements_match_plan_module() {
        for plan in BILLING_PLANS {
            assert_eq!(
                plan.entitlements,
                crate::plan::plan_entitlements(plan.id),
                "entitlements drift between catalog and plan module for {}",
                plan.id
            );
        }
    }

    /// Konsolidiertes `"analytics"`-Flag: genau die 5 Analyse-Pläne tragen es,
    /// die reinen Chat-/Raid-Pläne nicht.
    #[test]
    fn analytics_flag_only_on_analysis_plans() {
        for id in ["raid_boost", "bundle_chat_quiet_raid_boost", "raid_free", "chat_quiet"] {
            assert!(
                !crate::plan::plan_has_analytics(id),
                "{id} darf kein analytics-Flag tragen"
            );
        }
        for id in [
            "analysis_dashboard",
            "bundle_werbefrei_analyse",
            "bundle_komplett",
            "bundle_analysis_raid_boost",
            "analytics_trial",
        ] {
            assert!(crate::plan::plan_has_analytics(id), "{id} muss analytics-Flag tragen");
        }
    }
}

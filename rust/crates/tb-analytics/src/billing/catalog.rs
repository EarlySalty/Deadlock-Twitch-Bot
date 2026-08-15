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
    /// Monatlicher Endpreis in Cent (`0` = kostenlos). Kleinunternehmer, kein
    /// Steuerausweis.
    pub monthly_gross_cents: u32,
    /// Jahres-Endpreis in Cent. `0` = kein eigener Jahrespreis (dann
    /// Monatspreis × 12 minus Anzeige-Rabatt). Premium hinterlegt 2990,
    /// weil 17 % auf 299 × 12 auf 2978 runden würde.
    pub annual_gross_cents: u32,
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
    /// Zwischensumme brutto in Cent (`monthly * cycle`).
    pub subtotal_gross_cents: u32,
    /// Tatsächlich angewandter Rabatt-Prozentsatz (0, falls Zyklus ≤ 1).
    pub discount_percent: u32,
    /// Rabattbetrag in Cent. Bei festem Jahrespreis = `subtotal - total`.
    pub discount_cents: u32,
    /// Gesamtsumme brutto in Cent.
    pub total_gross_cents: u32,
    /// Effektiver Monatspreis brutto in Cent (`total` auf Monate gerundet).
    pub effective_monthly_gross_cents: u32,
    /// Alias der Brutto-Zwischensumme (alte JSON-Konsumenten).
    pub subtotal_net_cents: u32,
    /// Alias der Brutto-Gesamtsumme (alte JSON-Konsumenten).
    pub total_net_cents: u32,
    /// Alias des effektiven Monatspreises (alte JSON-Konsumenten).
    pub effective_monthly_net_cents: u32,
}

/// Zyklus-Rabatte: `(Monate, Rabatt-Prozent)`. Der 17-Prozent-Wert ist nur
/// Anzeige; der Jahrespreis von Premium steht fest auf 2990 Cent.
pub const CYCLE_DISCOUNTS: &[(u32, u32)] = &[(1, 0), (12, 17)];

/// Pflichthinweis für Preis, Checkout und Rechnung (§ 19 UStG).
pub const TAX_NOTE: &str = "Kein Ausweis von Umsatzsteuer gemäß § 19 UStG.";

/// Alle bestehenden Entitlements. Premium bekommt sie geschlossen.
pub const PREMIUM_ENTITLEMENTS: &[&str] = &[
    "analytics",
    "chat.lurker_tax",
    "chat.promos.disable",
    "raid.priority",
];

/// Kaufbarer Katalog: Free + Premium. Alte Plan-IDs bleiben in `plan.rs` lesbar.
pub const BILLING_PLANS: &[BillingPlan] = &[
    BillingPlan {
        id: "free",
        name: "Free",
        tier: "free",
        badge: "free",
        description: "Tagesform des letzten Streams, Auto-Raid, Chat-Befehle, Overlay und Planung.",
        monthly_gross_cents: 0,
        annual_gross_cents: 0,
        recommended: false,
        entitlements: &[],
        features: &[
            "Tagesform des letzten Streams",
            "Auto-Raid Grundfunktion",
            "Alle Chat-Befehle",
            "Overlay-Builder und Planung",
        ],
    },
    BillingPlan {
        id: "premium",
        name: "Premium",
        tier: "extended",
        badge: "premium",
        description: "Voller Verlauf, KI, Coaching und die Clip-Pipeline. Deine Zahlen, sichtbar.",
        monthly_gross_cents: 299,
        annual_gross_cents: 2990,
        recommended: true,
        entitlements: PREMIUM_ENTITLEMENTS,
        features: &[
            "Voller Verlauf, Vergleiche und Wachstum",
            "KI-Analyse, KI-Chat, Coaching",
            "Clip- und Social-Pipeline",
            "Werbefrei, Raid-Prio, Lurker-Steuer",
        ],
    },
];

/// Alte Plan-IDs, die in der DB stehen und lesbar bleiben, aber nicht mehr
/// kaufbar sind.
pub const LEGACY_PLAN_IDS: &[&str] = &[
    "raid_free",
    "chat_quiet",
    "raid_boost",
    "bundle_chat_quiet_raid_boost",
    "analysis_dashboard",
    "bundle_werbefrei_analyse",
    "bundle_komplett",
    "bundle_analysis_raid_boost",
    "analytics_trial",
];

/// `true`, wenn die ID ein alter, nur noch lesbarer Plan ist.
pub fn is_legacy_plan_id(plan_id: &str) -> bool {
    LEGACY_PLAN_IDS.contains(&plan_id.trim())
}

/// In Source eingecheckte Stripe-Price-IDs (keine Secrets). `(plan_id, &[(cycle, price_id)])`.
/// Spiegelt `STRIPE_PRICE_ID_DEFAULTS`. `raid_free` fehlt (kostenlos, kein Stripe-Price).
pub const PRICE_ID_DEFAULTS: &[(&str, &[(u32, &str)])] = &[
    (
        "premium",
        &[
            (1, "price_pending_premium_1m_gross_v3"),
            (12, "price_pending_premium_12m_gross_v3"),
        ],
    ),
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

/// `true`, wenn der Plan kostenpflichtig ist (`monthly_gross_cents > 0`).
pub fn is_paid_plan_id(plan_id: &str) -> bool {
    find_plan(plan_id).is_some_and(|plan| plan.monthly_gross_cents > 0)
}

/// Stripe-Lookup-Key eines Plans für einen Zyklus.
///
/// Neues Format: `deadlock_{plan_id}_{cycle}m_gross_v3`.
pub fn lookup_key(plan_id: &str, cycle_months: u32) -> String {
    format!("deadlock_{plan_id}_{cycle_months}m_gross_v3")
}

/// Extrahiert die Plan-ID aus einem Stripe-Lookup-Key.
///
/// Akzeptiert `deadlock_{plan_id}_{cycle}m_{tax}_v{n}` (netto `v2` und brutto
/// `v3`). Unbekannte Formate → `None`.
pub fn plan_id_from_lookup_key(lookup_key: &str) -> Option<&str> {
    let rest = lookup_key.trim().strip_prefix("deadlock_")?;
    let mut search_end = rest.len();
    while let Some(underscore) = rest[..search_end].rfind('_') {
        let after = &rest[underscore + 1..];
        if let Some(m_pos) = after.find('m') {
            if m_pos > 0
                && after[..m_pos].bytes().all(|b| b.is_ascii_digit())
                && after[m_pos..].starts_with("m_")
            {
                let plan = rest[..underscore].trim();
                if !plan.is_empty() {
                    return Some(plan);
                }
            }
        }
        if underscore == 0 {
            break;
        }
        search_end = underscore;
    }
    None
}

/// Findet die Plan-ID zu einer eingecheckten Stripe-Price-ID.
pub fn plan_id_from_price_id(price_id: &str) -> Option<&'static str> {
    let price_id = price_id.trim();
    if price_id.is_empty() {
        return None;
    }
    PRICE_ID_DEFAULTS.iter().find_map(|(plan, cycles)| {
        cycles
            .iter()
            .any(|(_, id)| *id == price_id)
            .then_some(*plan)
    })
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
pub fn compute_plan_price(
    monthly_net_cents: u32,
    cycle_months: u32,
    cycle_discount: u32,
) -> PlanPrice {
    let cycle = cycle_months;
    let subtotal = monthly_net_cents.saturating_mul(cycle);
    let discount_percent = if cycle > 1 && subtotal > 0 {
        cycle_discount
    } else {
        0
    };
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
        subtotal_gross_cents: subtotal,
        discount_percent,
        discount_cents,
        total_gross_cents: total,
        effective_monthly_gross_cents: effective_monthly,
        subtotal_net_cents: subtotal,
        total_net_cents: total,
        effective_monthly_net_cents: effective_monthly,
    }
}

impl BillingPlan {
    /// Preis-Tableau dieses Plans für einen (zu normalisierenden) Zyklus.
    ///
    /// Jahrespreis kommt aus `annual_gross_cents`, nicht aus dem Rabattsatz.
    pub fn price_for_cycle(&self, cycle_months: u32) -> PlanPrice {
        let cycle = normalize_billing_cycle(cycle_months);
        if cycle == 12 && self.annual_gross_cents > 0 {
            let subtotal = self.monthly_gross_cents.saturating_mul(cycle);
            let total = self.annual_gross_cents;
            let discount_cents = subtotal.saturating_sub(total);
            let effective_monthly = (total + cycle / 2) / cycle;
            return PlanPrice {
                cycle_months: cycle,
                subtotal_gross_cents: subtotal,
                discount_percent: cycle_discount_percent(cycle),
                discount_cents,
                total_gross_cents: total,
                effective_monthly_gross_cents: effective_monthly,
                subtotal_net_cents: subtotal,
                total_net_cents: total,
                effective_monthly_net_cents: effective_monthly,
            };
        }
        compute_plan_price(
            self.monthly_gross_cents,
            cycle,
            cycle_discount_percent(cycle),
        )
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
                "monthly_gross_cents": plan.monthly_gross_cents,
                "annual_gross_cents": plan.annual_gross_cents,
                "monthly_net_cents": plan.monthly_gross_cents,
                "entitlements": plan.entitlements,
                "features": plan.features,
                "price": {
                    "cycle_months": price.cycle_months,
                    "cycle_label": cycle_lbl,
                    "subtotal_gross_cents": price.subtotal_gross_cents,
                    "discount_percent": price.discount_percent,
                    "discount_cents": price.discount_cents,
                    "total_gross_cents": price.total_gross_cents,
                    "effective_monthly_gross_cents": price.effective_monthly_gross_cents,
                    "total_gross_label": format_eur_cents(price.total_gross_cents as i64),
                    "effective_monthly_gross_label": format_eur_cents(
                        price.effective_monthly_gross_cents as i64,
                    ),
                    "subtotal_net_cents": price.subtotal_gross_cents,
                    "total_net_cents": price.total_gross_cents,
                    "effective_monthly_net_cents": price.effective_monthly_gross_cents,
                    "subtotal_net_label": format_eur_cents(price.subtotal_gross_cents as i64),
                    "total_net_label": format_eur_cents(price.total_gross_cents as i64),
                    "effective_monthly_net_label": format_eur_cents(
                        price.effective_monthly_gross_cents as i64,
                    ),
                },
            })
        })
        .collect();

    serde_json::json!({
        "currency": "EUR",
        "tax_mode": "small_business",
        "gross_available": true,
        "tax_note": TAX_NOTE,
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
pub fn resolved_price_id(
    plan_id: &str,
    cycle_months: u32,
    vault_price_map: &PriceMap,
) -> Option<String> {
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

    /// Erwartete Endpreise je Plan. `(id, monthly_gross_cents, annual_gross_cents, tier, recommended)`.
    const EXPECTED: &[(&str, u32, u32, &str, bool)] = &[
        ("free", 0, 0, "free", false),
        ("premium", 299, 2990, "extended", true),
    ];

    #[test]
    fn catalog_has_exactly_two_plans_free_premium() {
        assert_eq!(BILLING_PLANS.len(), 2);
        for (plan, exp) in BILLING_PLANS.iter().zip(EXPECTED.iter()) {
            assert_eq!(plan.id, exp.0, "plan id order mismatch");
            assert_eq!(plan.monthly_gross_cents, exp.1, "monthly for {}", exp.0);
            assert_eq!(plan.annual_gross_cents, exp.2, "annual for {}", exp.0);
            assert_eq!(plan.tier, exp.3, "tier for {}", exp.0);
            assert_eq!(plan.recommended, exp.4, "recommended for {}", exp.0);
        }
    }

    #[test]
    fn cycle_discounts_are_month_and_year() {
        assert_eq!(CYCLE_DISCOUNTS, &[(1, 0), (12, 17)]);
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

    #[test]
    fn plan_id_from_lookup_key_parst_netto_und_brutto() {
        assert_eq!(
            plan_id_from_lookup_key("deadlock_chat_quiet_12m_net_v2"),
            Some("chat_quiet")
        );
        assert_eq!(
            plan_id_from_lookup_key("deadlock_premium_1m_gross_v3"),
            Some("premium")
        );
        assert_eq!(
            plan_id_from_lookup_key("deadlock_bundle_chat_quiet_raid_boost_1m_net_v2"),
            Some("bundle_chat_quiet_raid_boost")
        );
        assert_eq!(plan_id_from_lookup_key("fallback_key"), None);
        assert_eq!(plan_id_from_lookup_key(""), None);
    }

    #[test]
    fn plan_id_from_price_id_trifft_defaults() {
        assert_eq!(
            plan_id_from_price_id("price_1TeNGF0yU8I2yGJ0YLkz7PCX"),
            Some("chat_quiet")
        );
        assert_eq!(plan_id_from_price_id("price_missing"), None);
    }

    #[test]
    fn prices_and_lookup_keys_for_free_and_premium() {
        let free = find_plan("free").expect("free present");
        let month = free.price_for_cycle(1);
        assert_eq!(month.total_gross_cents, 0);
        assert_eq!(free.lookup_key(1), "deadlock_free_1m_gross_v3");

        let premium = find_plan("premium").expect("premium present");
        let month = premium.price_for_cycle(1);
        assert_eq!(month.total_gross_cents, 299);
        assert_eq!(month.discount_percent, 0);
        assert_eq!(premium.lookup_key(1), "deadlock_premium_1m_gross_v3");

        let year = premium.price_for_cycle(12);
        assert_eq!(year.subtotal_gross_cents, 3588);
        assert_eq!(year.total_gross_cents, 2990);
        assert_eq!(year.discount_percent, 17);
        assert_eq!(year.discount_cents, 598);
        assert_eq!(year.effective_monthly_gross_cents, 249);
        assert_eq!(premium.lookup_key(12), "deadlock_premium_12m_gross_v3");
        // 17 % auf 3588 wäre 2978. Der Jahrespreis ist fest 2990.
        let computed = compute_plan_price(299, 12, 17);
        assert_eq!(computed.total_gross_cents, 2978);
        assert_ne!(year.total_gross_cents, computed.total_gross_cents);
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
        assert!(!is_paid_plan_id("free"));
        assert!(is_paid_plan_id("premium"));
        assert!(!is_paid_plan_id("raid_free"));
        assert!(!is_paid_plan_id("chat_quiet"));
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
        assert_eq!(
            product_id_default("chat_quiet"),
            Some("prod_UYKKvIg1sbjVrl")
        );
        assert_eq!(product_id_default("raid_boost"), None);

        // Premium-Platzhalter bis M8 (echte Stripe-Prices legt der User an).
        assert_eq!(
            price_id_default("premium", 1),
            Some("price_pending_premium_1m_gross_v3")
        );
        assert_eq!(price_id_default("free", 1), None);
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
        assert_eq!(cat["tax_mode"], "small_business");
        assert_eq!(cat["gross_available"], true);
        assert_eq!(cat["tax_note"], TAX_NOTE);
        assert_eq!(cat["cycle_months"], 1);
        assert_eq!(cat["cycle_label"], "30 Tage");
        assert_eq!(cat["discount_percent"], 0);
        let plans = cat["plans"].as_array().unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0]["id"], "free");
        assert_eq!(plans[0]["price"]["total_gross_cents"], 0);
        let premium = plans.iter().find(|p| p["id"] == "premium").unwrap();
        assert_eq!(premium["monthly_gross_cents"], 299);
        assert_eq!(premium["price"]["total_gross_cents"], 299);
        assert_eq!(premium["price"]["total_gross_label"], "2,99 EUR");
        assert_eq!(premium["tier"], "extended");

        let cat12 = catalog_json(12);
        assert_eq!(cat12["cycle_months"], 12);
        assert_eq!(cat12["discount_percent"], 17);
        let p12 = cat12["plans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "premium")
            .unwrap();
        assert_eq!(p12["price"]["total_gross_cents"], 2990);
        assert_eq!(p12["price"]["total_gross_label"], "29,90 EUR");
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
                plan.entitlements,
                &sorted[..],
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
        for id in [
            "raid_boost",
            "bundle_chat_quiet_raid_boost",
            "raid_free",
            "free",
            "chat_quiet",
        ] {
            assert!(
                !crate::plan::plan_has_analytics(id),
                "{id} darf kein analytics-Flag tragen"
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
            assert!(
                crate::plan::plan_has_analytics(id),
                "{id} muss analytics-Flag tragen"
            );
        }
    }
}

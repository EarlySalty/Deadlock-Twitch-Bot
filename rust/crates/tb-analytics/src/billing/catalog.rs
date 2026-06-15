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
        entitlements: &["analytics.daily"],
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
        entitlements: &[
            "analytics.ai_mini",
            "analytics.basic",
            "chat.lurker_tax",
            "raid.priority",
        ],
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
            "analytics.ai_mini",
            "analytics.basic",
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
        entitlements: &[
            "analytics.ai_full",
            "analytics.basic",
            "analytics.extended",
            "chat.lurker_tax",
        ],
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
            "analytics.ai_full",
            "analytics.basic",
            "analytics.extended",
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
            "analytics.ai_full",
            "analytics.ai_mini",
            "analytics.basic",
            "analytics.extended",
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
            "analytics.ai_full",
            "analytics.basic",
            "analytics.extended",
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
    fn entitlements_are_sorted_and_nonempty() {
        for plan in BILLING_PLANS {
            assert!(!plan.entitlements.is_empty(), "no entitlements for {}", plan.id);
            let mut sorted = plan.entitlements.to_vec();
            sorted.sort_unstable();
            assert_eq!(
                plan.entitlements, &sorted[..],
                "entitlements for {} must be sorted (Python plan_entitlements sorts)",
                plan.id
            );
        }
    }
}

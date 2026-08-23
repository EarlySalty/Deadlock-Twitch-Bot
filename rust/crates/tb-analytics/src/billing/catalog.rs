//! Statischer Billing-Katalog: die drei Stufen Free, Plus und Pro, ihre
//! Preis-Logik und der Stripe-Price/Product-Override-Layer.
//!
//! Alle Betraege sind **Endpreise** (Kleinunternehmerregelung nach Paragraph 19
//! UStG, kein Umsatzsteuerausweis). Der Jahresbetrag ist als eigener Wert
//! hinterlegt und wird nicht aus einem Rabattsatz gerechnet: er entspricht genau
//! zehn Monatspreisen ("zwei Monate geschenkt"). Der Prozentwert im Katalog ist
//! reine Anzeige und wird aus den beiden Betraegen abgeleitet.
//!
//! Spec: `.tasks/2026-08-23-pricing-drei-stufen/SPEC.md` (M2).

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
    /// Tier-Stufe (`"free" | "extended"`). Bleibt als Kompatibilitaets-Feld
    /// fuer `plan_is_extended`; die eigentliche Leiter ist `crate::stufe::Stufe`.
    pub tier: &'static str,
    /// UI-Badge-Schlüssel.
    pub badge: &'static str,
    /// Marketing-Beschreibung.
    pub description: &'static str,
    /// Monatlicher Endpreis in Cent (`0` = kostenlos).
    pub monthly_gross_cents: u32,
    /// Jahres-Endpreis in Cent als eigener Betrag (`0` = kostenlos). Zehn
    /// Monatspreise, also zwei geschenkte Monate.
    pub yearly_gross_cents: u32,
    /// Empfehlungs-Hervorhebung in der UI.
    pub recommended: bool,
    /// Freigeschaltete Entitlements (alphabetisch sortiert).
    pub entitlements: &'static [&'static str],
    /// Feature-Bulletpoints für die UI.
    pub features: &'static [&'static str],
}

/// Berechnetes Preis-Tableau eines Plans fuer einen Abrechnungszyklus.
/// Alle Betraege sind Endpreise (Paragraph 19 UStG).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanPrice {
    /// Abrechnungszyklus in Monaten.
    pub cycle_months: u32,
    /// Zwischensumme in Cent (`monthly * cycle`), also der Preis ohne Jahresvorteil.
    pub subtotal_gross_cents: u32,
    /// Ersparnis in Prozent, kaufmaennisch gerundet. Reine Anzeige, aus den
    /// hinterlegten Betraegen abgeleitet (0 bei Monatszyklus oder Gratis-Plan).
    pub discount_percent: u32,
    /// Ersparnis in Cent (`subtotal - total`).
    pub discount_cents: u32,
    /// Tatsaechlich faelliger Endpreis in Cent fuer den Zyklus.
    pub total_gross_cents: u32,
    /// Rechnerischer Monatspreis in Cent (`total` auf Monate gerundet).
    pub effective_monthly_gross_cents: u32,
}

/// Buchbare Abrechnungszyklen in Monaten.
pub const BILLING_CYCLES: &[u32] = &[1, 12];

/// Wie viele Monatspreise ein Jahresabo kostet: zehn, also zwei Monate geschenkt.
/// Nur Doku-/Testanker; massgeblich ist `BillingPlan::yearly_gross_cents`.
pub const YEARLY_MONTHS_CHARGED: u32 = 10;

/// Die drei buchbaren Stufen. Reihenfolge = Anzeigereihenfolge.
pub const BILLING_PLANS: &[BillingPlan] = &[
    BillingPlan {
        id: "free",
        name: "Netzwerk Free",
        tier: "free",
        badge: "free",
        description: "Dauerhaft kostenlos: Auto-Raid, Chat-Schutz und die Tagesform deines letzten Streams.",
        monthly_gross_cents: 0,
        yearly_gross_cents: 0,
        recommended: false,
        entitlements: &[],
        features: &[
            "Auto-Raid in beide Richtungen",
            "Kompletter Chat-Schutz und alle Chat-Befehle",
            "Go-Live-Post im Community-Discord",
            "Overlay-Builder und Sendeplanung",
            "Tagesform deines letzten Streams",
            "3 Clips im Monat, mit Wasserzeichen",
        ],
    },
    BillingPlan {
        id: "plus",
        name: "Netzwerk Plus",
        tier: "extended",
        badge: "plus",
        description: "Dein voller Verlauf, Zeitraumvergleiche und die komplette KI-Auswertung.",
        monthly_gross_cents: 499,
        yearly_gross_cents: 4990,
        recommended: false,
        entitlements: &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        features: &[
            "Voller Verlauf statt nur letztem Stream",
            "Zeitraumvergleiche und Wachstumskurven",
            "KI-Analyse, KI-Chat, Coaching und KI-Wochenreport",
            "Werbefreier Chat, Raid-Vorrang, Lurker-Erinnerung, eigener Bot-Name",
            "10 Clips im Monat, ohne Wasserzeichen",
        ],
    },
    BillingPlan {
        id: "pro",
        name: "Creator Pro",
        tier: "extended",
        badge: "pro",
        description: "Alles aus Netzwerk Plus, dazu Clips ohne Limit und automatisches Posten auf deinen Kanälen.",
        monthly_gross_cents: 999,
        yearly_gross_cents: 9990,
        recommended: false,
        entitlements: &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
            "social.auto_post",
        ],
        features: &[
            "Alles aus Netzwerk Plus",
            "Clips ohne Mengenbegrenzung",
            "Automatisches Posten auf TikTok, Instagram und YouTube",
            "Untertitel und mehrere Formate",
            "Vorrang bei Support und neuen Funktionen",
        ],
    },
];

/// Stripe-Price-IDs, die fest im Code stehen. Nach dem Umbau auf drei Stufen
/// ist die Liste **leer**: die alten Netto-Preise (1,99 / 3,49 / 4,99) duerfen
/// nicht mehr gebucht werden, und fuer Free/Plus/Pro sind die Stripe-Preise noch
/// nicht angelegt. Bis dahin liefert der Vault-Override
/// (`STRIPE_PRICE_ID_MAP`) die IDs; ohne Eintrag meldet der Checkout sauber
/// `missing_stripe_price_id`. `(plan_id, &[(cycle, price_id)])`.
pub const PRICE_ID_DEFAULTS: &[(&str, &[(u32, &str)])] = &[];

/// Stripe-Product-IDs, die fest im Code stehen. Leer aus demselben Grund wie
/// [`PRICE_ID_DEFAULTS`]; der Vault-Override gewinnt hier ohnehin.
pub const PRODUCT_ID_DEFAULTS: &[(&str, &str)] = &[];

/// Normalisiert einen Roh-Zyklus auf einen bekannten Wert.
///
/// Entspricht Pythons `normalize_billing_cycle`: unbekannte Zyklen (alles außer
/// `1`/`12`) fallen auf `1` zurück.
pub fn normalize_billing_cycle(raw_cycle: u32) -> u32 {
    if BILLING_CYCLES.contains(&raw_cycle) {
        raw_cycle
    } else {
        1
    }
}

/// Angezeigte Ersparnis eines Plans fuer einen Zyklus in Prozent.
///
/// Reine Anzeige: der Wert wird aus den beiden hinterlegten Betraegen
/// abgeleitet (`(subtotal - total) / subtotal`), nicht umgekehrt der Preis aus
/// einem Rabattsatz gerechnet. Monatszyklus und Gratis-Plan liefern `0`.
pub fn cycle_discount_percent(plan_id: &str, cycle_months: u32) -> u32 {
    find_plan(plan_id)
        .map(|plan| plan.price_for_cycle(cycle_months).discount_percent)
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

/// Stripe-Lookup-Key eines Plans fuer einen Zyklus.
///
/// Suffix `_gross_v3`: die Betraege sind jetzt Endpreise. Der alte Suffix
/// `_net_v2` gehoert zu den Netto-Preisen der acht Vorgaenger-Plaene und darf
/// nicht kollidieren.
pub fn lookup_key(plan_id: &str, cycle_months: u32) -> String {
    format!("deadlock_{plan_id}_{cycle_months}m_gross_v3")
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

/// Preis-Tableau aus den beiden hinterlegten Betraegen.
///
/// Der Jahresbetrag ist ein **eigener** Wert, kein Ergebnis einer Rabatt-
/// rechnung: die frueher hier stehende Rundungsarithmetik traf die beworbene
/// Zahl nicht. Ersparnis und Prozentwert werden umgekehrt aus den Betraegen
/// abgeleitet und dienen nur der Anzeige. Zyklen ausser 1 und 12 gibt es nicht;
/// `cycle_months` wird vom Aufrufer normalisiert.
pub fn compute_plan_price(
    monthly_gross_cents: u32,
    yearly_gross_cents: u32,
    cycle_months: u32,
) -> PlanPrice {
    let cycle = cycle_months.max(1);
    let subtotal = monthly_gross_cents.saturating_mul(cycle);
    let total = if cycle == 1 {
        monthly_gross_cents
    } else if cycle == 12 {
        yearly_gross_cents
    } else {
        subtotal
    };
    let discount_cents = subtotal.saturating_sub(total);
    // Kaufmaennisch gerundet: (x*100 + subtotal/2) / subtotal.
    let discount_percent = if subtotal > 0 && discount_cents > 0 {
        (discount_cents.saturating_mul(100) + subtotal / 2) / subtotal
    } else {
        0
    };
    let effective_monthly = (total + cycle / 2) / cycle;
    PlanPrice {
        cycle_months: cycle,
        subtotal_gross_cents: subtotal,
        discount_percent,
        discount_cents,
        total_gross_cents: total,
        effective_monthly_gross_cents: effective_monthly,
    }
}

impl BillingPlan {
    /// Preis-Tableau dieses Plans fuer einen (zu normalisierenden) Zyklus.
    pub fn price_for_cycle(&self, cycle_months: u32) -> PlanPrice {
        let cycle = normalize_billing_cycle(cycle_months);
        compute_plan_price(self.monthly_gross_cents, self.yearly_gross_cents, cycle)
    }

    /// Stripe-Lookup-Key dieses Plans fuer einen Zyklus.
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

/// Formatiert Cent als deutschen EUR-String (`499` → `"4,99 EUR"`).
/// Negative Werte werden als `0` behandelt.
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

/// Baut die Katalog-JSON-Struktur fuer einen Zyklus.
///
/// Liefert `currency`/`tax_mode`/`gross_available`/`cycle_*`/`discount_percent`/
/// `plans`. `tax_mode` ist `small_business`: Endpreise nach Paragraph 19 UStG,
/// kein Umsatzsteuerausweis. Die `payment`-Sektion und plan-spezifische Felder
/// (`is_current`, `stripe_price_id`, `checkout_available`) ergaenzt die
/// HTTP-Schicht.
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
                "yearly_gross_cents": plan.yearly_gross_cents,
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
                    "subtotal_gross_label": format_eur_cents(price.subtotal_gross_cents as i64),
                    "total_gross_label": format_eur_cents(price.total_gross_cents as i64),
                    "effective_monthly_gross_label": format_eur_cents(
                        price.effective_monthly_gross_cents as i64,
                    ),
                },
            })
        })
        .collect();

    // Top-Level-Prozentwert: die beworbene Jahres-Ersparnis. Beide bezahlten
    // Stufen haben dasselbe Verhaeltnis (zehn von zwoelf Monatspreisen),
    // deshalb genuegt der Wert des Referenzplans `plus`.
    let discount_percent = cycle_discount_percent("plus", cycle);

    serde_json::json!({
        "currency": "EUR",
        "tax_mode": "small_business",
        "gross_available": true,
        "cycle_months": cycle,
        "cycle_label": cycle_lbl,
        "discount_percent": discount_percent,
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
    BILLING_CYCLES.contains(&cycle).then_some(cycle)
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

    /// Erwarteter Katalog: `(id, name, monatlich, jaehrlich, tier)`.
    const EXPECTED: &[(&str, &str, u32, u32, &str)] = &[
        ("free", "Netzwerk Free", 0, 0, "free"),
        ("plus", "Netzwerk Plus", 499, 4990, "extended"),
        ("pro", "Creator Pro", 999, 9990, "extended"),
    ];

    #[test]
    fn katalog_hat_genau_drei_stufen() {
        assert_eq!(BILLING_PLANS.len(), 3);
        for (plan, exp) in BILLING_PLANS.iter().zip(EXPECTED.iter()) {
            assert_eq!(plan.id, exp.0, "Reihenfolge der Plan-IDs");
            assert_eq!(plan.name, exp.1, "Name fuer {}", exp.0);
            assert_eq!(plan.monthly_gross_cents, exp.2, "Monatspreis fuer {}", exp.0);
            assert_eq!(plan.yearly_gross_cents, exp.3, "Jahrespreis fuer {}", exp.0);
            assert_eq!(plan.tier, exp.4, "tier fuer {}", exp.0);
            // Kein Empfohlen-Badge (Spec M6).
            assert!(!plan.recommended, "recommended fuer {}", exp.0);
        }
    }

    /// Jahrespreis = zehn Monatspreise, also exakt zwei geschenkte Monate.
    #[test]
    fn jahrespreis_ist_zehn_monatspreise() {
        for plan in BILLING_PLANS.iter().filter(|p| p.monthly_gross_cents > 0) {
            assert_eq!(
                plan.yearly_gross_cents,
                plan.monthly_gross_cents * YEARLY_MONTHS_CHARGED,
                "Jahrespreis fuer {} passt nicht zu zwei geschenkten Monaten",
                plan.id
            );
        }
    }

    #[test]
    fn buchbare_zyklen_sind_monat_und_jahr() {
        assert_eq!(BILLING_CYCLES, &[1, 12]);
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

    /// Kern-Orakel: die drei Stufen ueber beide Zyklen, inklusive Lookup-Keys.
    #[test]
    fn preise_und_lookup_keys_je_stufe() {
        for (id, _name, monthly, yearly, _tier) in EXPECTED {
            let plan = find_plan(id).expect("Plan vorhanden");

            let monatlich = plan.price_for_cycle(1);
            assert_eq!(monatlich.cycle_months, 1);
            assert_eq!(monatlich.subtotal_gross_cents, *monthly);
            assert_eq!(monatlich.total_gross_cents, *monthly);
            assert_eq!(monatlich.discount_cents, 0, "Monatszyklus ohne Ersparnis");
            assert_eq!(monatlich.discount_percent, 0);
            assert_eq!(monatlich.effective_monthly_gross_cents, *monthly);

            let jaehrlich = plan.price_for_cycle(12);
            assert_eq!(jaehrlich.cycle_months, 12);
            assert_eq!(jaehrlich.subtotal_gross_cents, monthly * 12);
            // Jahresbetrag kommt aus dem Katalog, nicht aus einer Rabattrechnung.
            assert_eq!(jaehrlich.total_gross_cents, *yearly, "Jahresbetrag {id}");
            assert_eq!(jaehrlich.discount_cents, monthly * 12 - yearly);

            for &cycle in &[1u32, 12u32] {
                assert_eq!(
                    plan.lookup_key(cycle),
                    format!("deadlock_{id}_{cycle}m_gross_v3"),
                    "lookup_key {id}/{cycle}m"
                );
            }
        }
    }

    /// Der Prozentwert ist reine Anzeige und muss zu "zwei Monate geschenkt"
    /// passen: 2 von 12 Monaten sind 16,67 %, kaufmaennisch 17 %.
    #[test]
    fn jahres_prozentwert_passt_zu_zwei_geschenkten_monaten() {
        assert_eq!(cycle_discount_percent("plus", 12), 17);
        assert_eq!(cycle_discount_percent("pro", 12), 17);
        // Monatszyklus und Gratis-Stufe zeigen nichts an.
        assert_eq!(cycle_discount_percent("plus", 1), 0);
        assert_eq!(cycle_discount_percent("free", 12), 0);
        // Unbekannter Plan → 0 statt Panik.
        assert_eq!(cycle_discount_percent("bundle_komplett", 12), 0);
    }

    /// Rundungssemantik von `compute_plan_price` unabhaengig vom Katalog.
    #[test]
    fn ersparnis_wird_kaufmaennisch_gerundet() {
        // 499 * 12 = 5988, Jahrespreis 4990 → 998 gespart → 16,67 % → 17 %.
        let jahr = compute_plan_price(499, 4990, 12);
        assert_eq!(jahr.subtotal_gross_cents, 5988);
        assert_eq!(jahr.total_gross_cents, 4990);
        assert_eq!(jahr.discount_cents, 998);
        assert_eq!(jahr.discount_percent, 17);
        // 4990 / 12 = 415,83 → gerundet 416.
        assert_eq!(jahr.effective_monthly_gross_cents, 416);
        // Ohne Jahresvorteil (Jahrespreis = 12 Monatspreise) bleibt alles bei 0.
        let ohne = compute_plan_price(499, 5988, 12);
        assert_eq!(ohne.discount_cents, 0);
        assert_eq!(ohne.discount_percent, 0);
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
        assert!(is_paid_plan_id("plus"));
        assert!(is_paid_plan_id("pro"));
        // Alte Plan-IDs sind lesbar (plan.rs), aber nicht mehr kaufbar.
        assert!(!is_paid_plan_id("bundle_komplett"));
        assert!(!is_paid_plan_id("chat_quiet"));
        assert!(!is_paid_plan_id("unknown_plan"));
    }

    /// Die alten Netto-Plaene sind aus dem Verkaufskatalog raus.
    #[test]
    fn alte_plaene_sind_nicht_mehr_buchbar() {
        for alt in [
            "raid_free",
            "chat_quiet",
            "raid_boost",
            "bundle_chat_quiet_raid_boost",
            "analysis_dashboard",
            "bundle_analysis_raid_boost",
            "bundle_werbefrei_analyse",
            "bundle_komplett",
            "analytics_trial",
        ] {
            assert!(find_plan(alt).is_none(), "{alt} darf nicht im Katalog stehen");
        }
    }

    /// Kein eingecheckter Stripe-Price mehr: die alten IDs zeigten auf
    /// Netto-Preise und duerfen nicht versehentlich gebucht werden.
    #[test]
    fn keine_eingecheckten_stripe_ids_mehr() {
        assert!(PRICE_ID_DEFAULTS.is_empty());
        assert!(PRODUCT_ID_DEFAULTS.is_empty());
        for plan in BILLING_PLANS {
            assert_eq!(price_id_default(plan.id, 1), None);
            assert_eq!(price_id_default(plan.id, 12), None);
            assert_eq!(product_id_default(plan.id), None);
        }
    }

    #[test]
    fn format_eur_cents_formatiert_deutsch() {
        assert_eq!(format_eur_cents(0), "0,00 EUR");
        assert_eq!(format_eur_cents(499), "4,99 EUR");
        assert_eq!(format_eur_cents(4990), "49,90 EUR");
        assert_eq!(format_eur_cents(999), "9,99 EUR");
        assert_eq!(format_eur_cents(9990), "99,90 EUR");
        assert_eq!(format_eur_cents(5), "0,05 EUR");
        // Negativ → 0.
        assert_eq!(format_eur_cents(-50), "0,00 EUR");
    }

    #[test]
    fn cycle_label_matches_python() {
        assert_eq!(cycle_label(1), "30 Tage");
        assert_eq!(cycle_label(12), "12 Monate");
        assert_eq!(cycle_label(3), "3 Monate");
    }

    #[test]
    fn catalog_json_liefert_drei_stufen_mit_endpreisen() {
        let cat = catalog_json(1);
        assert_eq!(cat["currency"], "EUR");
        assert_eq!(cat["tax_mode"], "small_business");
        assert_eq!(cat["gross_available"], true);
        assert_eq!(cat["cycle_months"], 1);
        assert_eq!(cat["cycle_label"], "30 Tage");
        assert_eq!(cat["discount_percent"], 0);
        let plans = cat["plans"].as_array().unwrap();
        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0]["id"], "free");
        assert_eq!(plans[0]["price"]["total_gross_cents"], 0);
        assert_eq!(plans[0]["price"]["total_gross_label"], "0,00 EUR");
        assert_eq!(plans[1]["id"], "plus");
        assert_eq!(plans[1]["price"]["total_gross_cents"], 499);
        assert_eq!(plans[1]["price"]["total_gross_label"], "4,99 EUR");
        assert_eq!(plans[2]["id"], "pro");
        assert_eq!(plans[2]["price"]["total_gross_cents"], 999);
        assert_eq!(plans[2]["price"]["total_gross_label"], "9,99 EUR");

        let cat12 = catalog_json(12);
        assert_eq!(cat12["cycle_months"], 12);
        assert_eq!(cat12["cycle_label"], "12 Monate");
        assert_eq!(cat12["discount_percent"], 17);
        let p12 = cat12["plans"].as_array().unwrap();
        assert_eq!(p12[1]["price"]["total_gross_cents"], 4990);
        assert_eq!(p12[1]["price"]["total_gross_label"], "49,90 EUR");
        assert_eq!(p12[2]["price"]["total_gross_cents"], 9990);
        assert_eq!(p12[2]["price"]["total_gross_label"], "99,90 EUR");
        assert_eq!(p12[0]["price"]["total_gross_cents"], 0);
        // Unbekannter Zyklus faellt auf 1 zurueck.
        assert_eq!(catalog_json(7)["cycle_months"], 1);
    }

    /// Abnahmekriterium 4: nirgends mehr eine Netto-Angabe oder ein Bundle.
    #[test]
    fn katalog_json_zeigt_keine_netto_felder_und_keine_bundles() {
        for cycle in [1u32, 12u32] {
            let raw = catalog_json(cycle).to_string();
            assert!(!raw.contains("net_cents"), "Netto-Feld im Katalog ({cycle}m)");
            assert!(!raw.contains("net_label"), "Netto-Label im Katalog ({cycle}m)");
            assert!(!raw.contains("bundle"), "Bundle im Katalog ({cycle}m)");
            assert!(!raw.contains("1,99"), "1,99 im Katalog ({cycle}m)");
        }
    }

    // ── Vault-Override-Layer ────────────────────────────────────────────────
    #[test]
    fn parse_price_id_mapping_normalizes_and_filters() {
        let raw = r#"{
            "plus": {"1": "price_plus_1m", "12": "price_plus_12m", "6": "price_invalid_cycle"},
            "leer": {"1": "  "},
            "  ": {"1": "x"}
        }"#;
        let map = parse_price_id_mapping(raw);
        assert_eq!(map.len(), 1);
        let (plan, cycles) = &map[0];
        assert_eq!(plan, "plus");
        // Zyklus 6 ist ungueltig → verworfen; nur 1 + 12 bleiben.
        assert_eq!(cycles.len(), 2);
        assert!(cycles.contains(&(1, "price_plus_1m".to_string())));
        assert!(cycles.contains(&(12, "price_plus_12m".to_string())));
        // Ungueltiges JSON / Nicht-Objekt → leer.
        assert!(parse_price_id_mapping("nicht json").is_empty());
        assert!(parse_price_id_mapping("[1,2]").is_empty());
        assert!(parse_price_id_mapping("").is_empty());
    }

    /// Ohne eingecheckte Defaults liefert der Vault die Price-IDs der neuen
    /// Stufen; das ist der einzige Weg, bis die Stripe-Preise angelegt sind.
    #[test]
    fn resolved_price_id_kommt_aus_dem_vault() {
        let vault = parse_price_id_mapping(
            r#"{"plus": {"1": "price_plus_1m", "12": "price_plus_12m"}}"#,
        );
        assert_eq!(
            resolved_price_id("plus", 1, &vault).as_deref(),
            Some("price_plus_1m")
        );
        assert_eq!(
            resolved_price_id("plus", 12, &vault).as_deref(),
            Some("price_plus_12m")
        );
        // Ohne Vault-Eintrag und ohne Default → None (Checkout meldet das sauber).
        assert_eq!(resolved_price_id("pro", 1, &vault), None);
    }

    #[test]
    fn resolved_product_id_vault_wins() {
        let vault = parse_product_id_mapping(r#"{"plus": "prod_plus"}"#);
        assert_eq!(
            resolved_product_id("plus", &vault).as_deref(),
            Some("prod_plus")
        );
        let empty = parse_product_id_mapping("");
        assert_eq!(resolved_product_id("plus", &empty), None);
    }

    #[test]
    fn parse_product_id_mapping_filters_empty() {
        let map = parse_product_id_mapping(r#"{"a": "prod_a", "b": "", "  ": "prod_c"}"#);
        assert_eq!(map.len(), 1);
        assert_eq!(map[0], ("a".to_string(), "prod_a".to_string()));
    }

    #[test]
    fn entitlements_are_sorted() {
        for plan in BILLING_PLANS {
            let mut sorted = plan.entitlements.to_vec();
            sorted.sort_unstable();
            assert_eq!(
                plan.entitlements, &sorted[..],
                "entitlements fuer {} muessen sortiert sein",
                plan.id
            );
        }
    }

    /// Drift-Guard: fuer jeden Katalog-Plan stimmen die Entitlements mit
    /// [`crate::plan::plan_entitlements`] ueberein (eine Quelle der Wahrheit).
    #[test]
    fn catalog_entitlements_match_plan_module() {
        for plan in BILLING_PLANS {
            assert_eq!(
                plan.entitlements,
                crate::plan::plan_entitlements(plan.id),
                "Entitlement-Drift zwischen Katalog und plan-Modul fuer {}",
                plan.id
            );
        }
    }

    /// Das `analytics`-Flag haengt an Plus und Pro, nie an Free.
    #[test]
    fn analytics_flag_ab_plus() {
        assert!(!crate::plan::plan_has_analytics("free"));
        assert!(crate::plan::plan_has_analytics("plus"));
        assert!(crate::plan::plan_has_analytics("pro"));
        // Alt-IDs bleiben lesbar und behalten ihre Einstufung.
        assert!(!crate::plan::plan_has_analytics("raid_free"));
        assert!(!crate::plan::plan_has_analytics("chat_quiet"));
        assert!(crate::plan::plan_has_analytics("analysis_dashboard"));
        assert!(crate::plan::plan_has_analytics("bundle_komplett"));
        assert!(crate::plan::plan_has_analytics("analytics_trial"));
    }
}

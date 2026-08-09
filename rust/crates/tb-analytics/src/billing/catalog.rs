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
    /// Monatlicher Endpreis in Cent (`0` = kostenlos).
    ///
    /// Endpreis heißt: kein Umsatzsteueraufschlag. Kleinunternehmer nach
    /// § 19 UStG, deshalb weist der Katalog keine Steuer aus.
    pub monthly_gross_cents: u32,
    /// Endpreis für den 12-Monats-Zyklus in Cent (`0` = kostenlos).
    ///
    /// **Eigener Betrag, nicht aus dem Rabattsatz gerechnet.** 299 × 12 minus
    /// 17 Prozent ergäbe 2978; beworben und abgerechnet werden aber 2990. Der
    /// Prozentwert in [`CYCLE_DISCOUNTS`] dient nur der Anzeige.
    pub yearly_gross_cents: u32,
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
    /// Zwischensumme in Cent (`monthly * cycle`), also der Preis ohne Jahresvorteil.
    pub subtotal_gross_cents: u32,
    /// Ersparnis in Prozent, aus den beiden Endpreisen gerechnet und gerundet.
    pub discount_percent: u32,
    /// Ersparnis in Cent (`subtotal - total`).
    pub discount_cents: u32,
    /// Gesamtsumme in Cent — Endpreis des Zyklus.
    pub total_gross_cents: u32,
    /// Effektiver Monatspreis in Cent (`total` auf Monate gerundet).
    pub effective_monthly_gross_cents: u32,
}

/// Beworbene Ersparnis je Zyklus: `(Monate, Prozent)`.
///
/// **Nur Anzeigewert.** Der Jahrespreis steht als eigener Betrag am Plan
/// (`yearly_gross_cents`); aus 17 Prozent auf 12 × 299 kämen 2978 statt 2990.
pub const CYCLE_DISCOUNTS: &[(u32, u32)] = &[(1, 0), (12, 17)];

/// Die zwei Plan-Blueprints: Free und Premium.
///
/// Der Katalog ist seit dem Pricing-Umbau vom 2026-08-09 auf eine einzige
/// bezahlte Stufe reduziert. Die alten Plan-IDs (`chat_quiet`, `raid_boost`,
/// `analysis_dashboard`, `analytics_trial`, alle `bundle_*`) bleiben in
/// [`crate::plan`] auflösbar, weil sie in der DB stehen — kaufbar sind sie nicht
/// mehr und deshalb stehen sie nicht mehr hier.
pub const BILLING_PLANS: &[BillingPlan] = &[
    BillingPlan {
        id: "free",
        name: "Free",
        tier: "free",
        badge: "free",
        description: "Die Tagesform deines letzten Streams, alle Chat-Befehle, Auto-Raid, Overlay und Planung.",
        monthly_gross_cents: 0,
        yearly_gross_cents: 0,
        recommended: false,
        entitlements: &[],
        features: &[
            "Tagesform deines letzten Streams",
            "Alle Chat-Befehle",
            "Auto-Raid Grundfunktion",
            "Overlay-Builder und Sendeplan",
        ],
    },
    BillingPlan {
        id: "premium",
        name: "Premium",
        tier: "extended",
        badge: "premium",
        description: "Dein voller Verlauf, Vergleiche, KI-Analyse und die Clip-Pipeline.",
        monthly_gross_cents: 299,
        yearly_gross_cents: 2990,
        recommended: true,
        entitlements: &[
            "analytics",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        ],
        features: &[
            "Voller Verlauf statt nur letzter Stream",
            "Zeitraumvergleiche und Wachstum",
            "KI-Analyse, KI-Chat und Coaching",
            "Clip- und Social-Pipeline",
            "Werbefrei im eigenen Chat",
            "Raid-Prio und Lurker-Steuer",
        ],
    },
];

/// In Source eingecheckte Stripe-Price-IDs (keine Secrets). `(plan_id, &[(cycle, price_id)])`.
///
/// Leer, bis die beiden Premium-Preise in Stripe angelegt sind (Milestone 8,
/// braucht den Stripe-Zugang des Betreibers). Bis dahin liefert die Vault-Map
/// `STRIPE_PRICE_ID_MAP` die IDs; siehe [`resolved_price_id`].
pub const PRICE_ID_DEFAULTS: &[(&str, &[(u32, &str)])] = &[];

/// Price-IDs der abgeschafften Pläne: `(price_id, plan_id)`.
///
/// Nicht mehr kaufbar, aber Bestands-Subscriptions in Stripe hängen weiter an
/// diesen Preisen. Der Webhook braucht die Rückwärts-Zuordnung, sonst bleibt
/// `twitch_billing_subscriptions.plan_id` bei genau diesen Abos leer.
pub const LEGACY_PRICE_ID_PLANS: &[(&str, &str)] = &[
    ("price_1TeNGF0yU8I2yGJ0crjsfhHO", "chat_quiet"),
    ("price_1TeNGF0yU8I2yGJ0YLkz7PCX", "chat_quiet"),
    ("price_1TeNGG0yU8I2yGJ0DhWzKQWU", "raid_boost"),
    ("price_1TeNGG0yU8I2yGJ0f9iYs3w1", "raid_boost"),
    ("price_1TeNGH0yU8I2yGJ0UqKylecO", "analysis_dashboard"),
    ("price_1TeNGH0yU8I2yGJ0tdHu8izl", "analysis_dashboard"),
    ("price_1TeNGH0yU8I2yGJ06sCbRobW", "bundle_chat_quiet_raid_boost"),
    ("price_1TeNGI0yU8I2yGJ0GaUNdWmK", "bundle_chat_quiet_raid_boost"),
    ("price_1TeNGI0yU8I2yGJ0YX5iUzX4", "bundle_werbefrei_analyse"),
    ("price_1TeNGJ0yU8I2yGJ0NlPVBIHZ", "bundle_werbefrei_analyse"),
    ("price_1TeNGJ0yU8I2yGJ0V8gH6IGg", "bundle_komplett"),
    ("price_1TeNGK0yU8I2yGJ0QTewVRfi", "bundle_komplett"),
    ("price_1TeNGK0yU8I2yGJ0guZX1iD8", "bundle_analysis_raid_boost"),
    ("price_1TeNGL0yU8I2yGJ0Alhd0ZPo", "bundle_analysis_raid_boost"),
];

/// In Source eingecheckte Stripe-Product-IDs (keine Secrets).
///
/// Leer aus demselben Grund wie [`PRICE_ID_DEFAULTS`]: das Premium-Produkt legt
/// der Betreiber in Milestone 8 an, danach kommt die ID über
/// `STRIPE_PRODUCT_ID_MAP` oder wird hier eingecheckt.
pub const PRODUCT_ID_DEFAULTS: &[(&str, &str)] = &[];

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
/// Format `deadlock_{plan_id}_{cycle}m_gross_v3`. Der Suffix ist neu: die
/// alten Netto-Keys (`_net_v2`) hängen an den abgeschafften Preisen und dürfen
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
        .or_else(|| {
            LEGACY_PRICE_ID_PLANS
                .iter()
                .find(|(pid, _)| *pid == needle)
                .map(|(_, plan_id)| *plan_id)
        })
}

/// Default-Product-ID eines Plans (aus [`PRODUCT_ID_DEFAULTS`]).
pub fn product_id_default(plan_id: &str) -> Option<&'static str> {
    PRODUCT_ID_DEFAULTS
        .iter()
        .find(|(id, _)| *id == plan_id)
        .map(|(_, product_id)| *product_id)
}

/// Preis-Tableau aus zwei Endpreisen: Monatspreis und Zyklus-Endpreis.
///
/// Beide Beträge sind hinterlegt, nicht gerechnet. Abgeleitet werden nur
/// Zwischensumme (`monthly × cycle`), Ersparnis (`subtotal − total`) und der
/// effektive Monatspreis (`total` auf Monate gerundet, `(total + cycle/2) / cycle`).
///
/// Der Prozentwert wird aus den echten Beträgen gerechnet und nicht aus
/// [`CYCLE_DISCOUNTS`] übernommen: 2990 gegen 3588 sind 16,66 Prozent, gerundet
/// 17. So kann die angezeigte Ersparnis nicht vom abgerechneten Betrag abdriften.
pub fn compute_plan_price(monthly_gross_cents: u32, total_gross_cents: u32, cycle_months: u32) -> PlanPrice {
    let cycle = cycle_months.max(1);
    let subtotal = monthly_gross_cents.saturating_mul(cycle);
    let total = if cycle == 1 { monthly_gross_cents } else { total_gross_cents };
    let discount_cents = subtotal.saturating_sub(total);
    let discount_percent = (discount_cents.saturating_mul(100) + subtotal / 2)
        .checked_div(subtotal)
        .unwrap_or(0);
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
    /// Endpreis dieses Plans für einen Zyklus (`1` → Monatspreis, sonst Jahrespreis).
    pub fn total_gross_cents(&self, cycle_months: u32) -> u32 {
        if normalize_billing_cycle(cycle_months) == 1 {
            self.monthly_gross_cents
        } else {
            self.yearly_gross_cents
        }
    }

    /// Preis-Tableau dieses Plans für einen (zu normalisierenden) Zyklus.
    pub fn price_for_cycle(&self, cycle_months: u32) -> PlanPrice {
        let cycle = normalize_billing_cycle(cycle_months);
        compute_plan_price(self.monthly_gross_cents, self.total_gross_cents(cycle), cycle)
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

/// Pflichthinweis zur Preisangabe (§ 19 UStG, Kleinunternehmer).
///
/// Gehört an jede Preisangabe, in den Checkout und auf die Rechnung. Steht hier,
/// damit Dashboard, Checkout und Rechnung denselben Satz benutzen.
pub const TAX_NOTICE: &str = "Kein Ausweis von Umsatzsteuer gemäß § 19 UStG.";

/// Baut die Katalog-JSON-Struktur für einen Zyklus.
///
/// Liefert `currency`/`tax_mode`/`gross_available`/`tax_notice`/`cycle_*`/
/// `discount_percent`/`plans`. Die `payment`-Sektion und plan-spezifische Felder
/// (`is_current`, `stripe_price_id`, `checkout_available`) werden in der
/// HTTP-Schicht ergänzt.
///
/// Alle Beträge sind Endpreise (`*_gross_cents`). Der Katalog hieß bis zum
/// Umbau vom 2026-08-09 netto und war zugleich brutto beschriftet — genau diese
/// Doppeldeutigkeit ist hier aufgelöst.
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

    serde_json::json!({
        "currency": "EUR",
        "tax_mode": "small_business",
        "gross_available": true,
        "tax_notice": TAX_NOTICE,
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

    /// Erwartete Endpreise je Plan — Orakel ist die Spec vom 2026-08-09.
    /// `(id, monthly_gross_cents, yearly_gross_cents, tier, recommended)`.
    const EXPECTED: &[(&str, u32, u32, &str, bool)] = &[
        ("free", 0, 0, "free", false),
        ("premium", 299, 2990, "extended", true),
    ];

    #[test]
    fn catalog_has_exactly_two_plans_free_and_premium() {
        assert_eq!(BILLING_PLANS.len(), 2, "genau zwei Pläne, kein Bundle mehr");
        for (plan, exp) in BILLING_PLANS.iter().zip(EXPECTED.iter()) {
            assert_eq!(plan.id, exp.0, "plan id order mismatch");
            assert_eq!(plan.monthly_gross_cents, exp.1, "monthly für {}", exp.0);
            assert_eq!(plan.yearly_gross_cents, exp.2, "yearly für {}", exp.0);
            assert_eq!(plan.tier, exp.3, "tier für {}", exp.0);
            assert_eq!(plan.recommended, exp.4, "recommended für {}", exp.0);
        }
    }

    /// Kein Betrag aus dem alten Katalog überlebt im neuen.
    #[test]
    fn keine_alten_betraege_mehr_im_katalog() {
        for plan in BILLING_PLANS {
            for cents in [199u32, 349, 499, 2388] {
                assert_ne!(
                    plan.monthly_gross_cents, cents,
                    "{} trägt noch einen abgeschafften Monatsbetrag",
                    plan.id
                );
                assert_ne!(
                    plan.yearly_gross_cents, cents,
                    "{} trägt noch einen abgeschafften Jahresbetrag",
                    plan.id
                );
            }
        }
    }

    #[test]
    fn cycle_discounts_sind_null_und_siebzehn() {
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

    /// Kern-Orakel: beide Pläne × Zyklen {1, 12} liefern die Beträge aus der Spec
    /// und Lookup-Keys mit dem neuen Suffix `_gross_v3`.
    #[test]
    fn preise_und_lookup_keys_entsprechen_der_spec() {
        for (id, monthly, yearly, _tier, _rec) in EXPECTED {
            let plan = find_plan(id).expect("plan present");

            let m = plan.price_for_cycle(1);
            assert_eq!(m.cycle_months, 1);
            assert_eq!(m.subtotal_gross_cents, *monthly, "subtotal {id}/1m");
            assert_eq!(m.total_gross_cents, *monthly, "total {id}/1m");
            assert_eq!(m.effective_monthly_gross_cents, *monthly, "effektiv {id}/1m");
            assert_eq!(m.discount_cents, 0, "kein Rabatt im Monatszyklus {id}");
            assert_eq!(m.discount_percent, 0, "kein Rabatt im Monatszyklus {id}");

            let y = plan.price_for_cycle(12);
            assert_eq!(y.cycle_months, 12);
            assert_eq!(y.subtotal_gross_cents, monthly * 12, "subtotal {id}/12m");
            assert_eq!(
                y.total_gross_cents, *yearly,
                "Jahrespreis ist der hinterlegte Betrag, kein gerechneter: {id}"
            );

            for &cycle in &[1u32, 12u32] {
                assert_eq!(
                    plan.lookup_key(cycle),
                    format!("deadlock_{id}_{cycle}m_gross_v3"),
                    "lookup_key {id}/{cycle}m"
                );
                assert!(
                    plan.lookup_key(cycle).ends_with("_gross_v3"),
                    "Lookup-Key muss auf _gross_v3 enden: {id}/{cycle}m"
                );
            }
        }
    }

    /// Der Jahrespreis ist ein eigener Betrag. Aus dem Rabattsatz gerechnet
    /// käme 2978 heraus, abgerechnet werden 2990 — die Anzeige-Prozente werden
    /// aus den echten Beträgen zurückgerechnet und landen wieder bei 17.
    #[test]
    fn jahrespreis_ist_eigener_betrag_nicht_aus_prozent_gerechnet() {
        let premium = find_plan("premium").expect("premium");
        let y = premium.price_for_cycle(12);
        assert_eq!(y.subtotal_gross_cents, 3588);
        assert_eq!(y.total_gross_cents, 2990, "beworben und abgerechnet: 29,90 EUR");
        assert_ne!(
            y.total_gross_cents, 2978,
            "Jahrespreis darf nicht aus dem Rabattsatz gerechnet sein"
        );
        assert_eq!(y.discount_cents, 598);
        // 598/3588 = 16,66 % → gerundet 17, passend zur beworbenen Ersparnis.
        assert_eq!(y.discount_percent, 17);
        assert_eq!(y.discount_percent, cycle_discount_percent(12));
        // 2990 auf 12 Monate = 249,17 → 249.
        assert_eq!(y.effective_monthly_gross_cents, 249);
    }

    /// Rundungs-Semantik unabhängig von den Live-Beträgen.
    #[test]
    fn rundung_bei_synthetischen_betraegen() {
        // subtotal = 199*12 = 2388, total = 2149 → Ersparnis 239 = 10,008 % → 10.
        // effektiv = (2149 + 6)/12 = 179.
        let price = compute_plan_price(199, 2149, 12);
        assert_eq!(price.subtotal_gross_cents, 2388);
        assert_eq!(price.discount_cents, 239);
        assert_eq!(price.discount_percent, 10);
        assert_eq!(price.total_gross_cents, 2149);
        assert_eq!(price.effective_monthly_gross_cents, 179);
        // Kostenlos: keine Division durch Null, kein Rabatt.
        let free = compute_plan_price(0, 0, 12);
        assert_eq!(free.discount_percent, 0);
        assert_eq!(free.total_gross_cents, 0);
        // Ein Jahrespreis über der Zwischensumme erzeugt keinen negativen Rabatt.
        let teurer = compute_plan_price(299, 4000, 12);
        assert_eq!(teurer.discount_cents, 0);
        assert_eq!(teurer.discount_percent, 0);
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
        // Abgeschaffte Pläne sind nicht mehr kaufbar → kein Checkout.
        assert!(!is_paid_plan_id("chat_quiet"));
        assert!(!is_paid_plan_id("bundle_komplett"));
        assert!(!is_paid_plan_id("unknown_plan"));
    }

    #[test]
    fn price_und_product_defaults_sind_leer_bis_stripe_steht() {
        // Milestone 8 legt die Premium-Preise in Stripe an; bis dahin kommen die
        // IDs ausschließlich über die Vault-Map.
        assert!(PRICE_ID_DEFAULTS.is_empty());
        assert!(PRODUCT_ID_DEFAULTS.is_empty());
        assert_eq!(price_id_default("premium", 1), None);
        assert_eq!(product_id_default("premium"), None);
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

    /// Bestands-Abos in Stripe hängen an den alten Preisen. Fällt diese
    /// Rückwärts-Zuordnung weg, bleibt `plan_id` bei genau diesen Abos leer.
    #[test]
    fn plan_id_from_price_id_findet_jeden_legacy_price() {
        assert_eq!(LEGACY_PRICE_ID_PLANS.len(), 14, "7 Pläne × 2 Zyklen");
        for (price_id, plan_id) in LEGACY_PRICE_ID_PLANS {
            assert_eq!(
                plan_id_from_price_id(price_id),
                Some(*plan_id),
                "price {price_id} muss auf {plan_id} zurückführen"
            );
            assert!(
                crate::plan::is_known_plan_id(plan_id),
                "{plan_id} muss weiterhin auflösbar sein"
            );
            assert!(
                find_plan(plan_id).is_none(),
                "{plan_id} darf nicht mehr im Katalog stehen"
            );
        }
        assert_eq!(plan_id_from_price_id("price_unbekannt"), None);
        assert_eq!(plan_id_from_price_id("  "), None);
    }

    #[test]
    fn format_eur_cents_matches_python() {
        assert_eq!(format_eur_cents(0), "0,00 EUR");
        assert_eq!(format_eur_cents(299), "2,99 EUR");
        assert_eq!(format_eur_cents(2990), "29,90 EUR");
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
    fn catalog_json_liefert_zwei_plaene_mit_299_und_2990() {
        let cat = catalog_json(1);
        assert_eq!(cat["currency"], "EUR");
        assert_eq!(cat["tax_mode"], "small_business");
        assert_eq!(cat["gross_available"], true);
        assert_eq!(cat["tax_notice"], TAX_NOTICE);
        assert_eq!(cat["cycle_months"], 1);
        assert_eq!(cat["cycle_label"], "30 Tage");
        assert_eq!(cat["discount_percent"], 0);

        let plans = cat["plans"].as_array().unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0]["id"], "free");
        assert_eq!(plans[0]["price"]["total_gross_cents"], 0);
        assert_eq!(plans[0]["price"]["total_gross_label"], "0,00 EUR");
        assert_eq!(plans[0]["entitlements"].as_array().unwrap().len(), 0);

        let premium = plans.iter().find(|p| p["id"] == "premium").unwrap();
        assert_eq!(premium["price"]["total_gross_cents"], 299);
        assert_eq!(premium["price"]["total_gross_label"], "2,99 EUR");
        assert_eq!(premium["monthly_gross_cents"], 299);
        assert_eq!(premium["yearly_gross_cents"], 2990);
        assert_eq!(premium["tier"], "extended");
        assert_eq!(premium["recommended"], true);
        assert_eq!(premium["entitlements"].as_array().unwrap().len(), 4);

        // Jahreszyklus: 29,90 EUR gesamt, 2,49 EUR effektiv, 17 Prozent Ersparnis.
        let cat12 = catalog_json(12);
        assert_eq!(cat12["cycle_months"], 12);
        assert_eq!(cat12["cycle_label"], "12 Monate");
        assert_eq!(cat12["discount_percent"], 17);
        let p12 = cat12["plans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "premium")
            .unwrap();
        assert_eq!(p12["price"]["subtotal_gross_cents"], 3588);
        assert_eq!(p12["price"]["total_gross_cents"], 2990);
        assert_eq!(p12["price"]["total_gross_label"], "29,90 EUR");
        assert_eq!(p12["price"]["effective_monthly_gross_cents"], 249);
        assert_eq!(p12["price"]["effective_monthly_gross_label"], "2,49 EUR");
        assert_eq!(p12["price"]["discount_percent"], 17);

        // Kein Netto-Schlüssel und kein alter Betrag im ausgelieferten JSON.
        let raw = cat12.to_string();
        assert!(!raw.contains("net_cents"), "Netto-Schlüssel im Katalog: {raw}");
        assert!(!raw.contains("1,99"), "alter Betrag im Katalog: {raw}");
        assert!(!raw.contains("bundle_"), "Bundle-Plan im Katalog: {raw}");

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

    /// Solange kein Default eingecheckt ist, liefert die Vault-Map die Premium-
    /// Price-IDs. Genau darüber kommen die Preise aus Milestone 8 in den Betrieb.
    #[test]
    fn resolved_price_id_kommt_bis_milestone_8_aus_dem_vault() {
        let vault = parse_price_id_mapping(
            r#"{"premium": {"1": "price_premium_1m", "12": "price_premium_12m"}}"#,
        );
        assert_eq!(
            resolved_price_id("premium", 1, &vault).as_deref(),
            Some("price_premium_1m")
        );
        assert_eq!(
            resolved_price_id("premium", 12, &vault).as_deref(),
            Some("price_premium_12m")
        );
        // Ohne Vault-Eintrag und ohne Default → None.
        let leer = parse_price_id_mapping("");
        assert_eq!(resolved_price_id("premium", 1, &leer), None);
    }

    #[test]
    fn resolved_product_id_kommt_aus_dem_vault() {
        let vault = parse_product_id_mapping(r#"{"premium": "prod_premium"}"#);
        assert_eq!(
            resolved_product_id("premium", &vault).as_deref(),
            Some("prod_premium")
        );
        let leer = parse_product_id_mapping("");
        assert_eq!(resolved_product_id("premium", &leer), None);
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
                "entitlements für {} müssen sortiert sein",
                plan.id
            );
        }
    }

    /// Drift-Guard: für jeden Katalog-Plan stimmen die Entitlements mit
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

    /// Premium trägt alle vier Entitlements, Free keins.
    #[test]
    fn premium_traegt_alle_vier_entitlements() {
        let premium = find_plan("premium").expect("premium");
        assert_eq!(
            premium.entitlements,
            &[
                "analytics",
                "chat.lurker_tax",
                "chat.promos.disable",
                "raid.priority"
            ]
        );
        assert!(crate::plan::plan_has_analytics("premium"));
        assert!(crate::plan::plan_is_extended("premium"));
        let free = find_plan("free").expect("free");
        assert!(free.entitlements.is_empty());
        assert!(!crate::plan::plan_has_analytics("free"));
        assert!(!crate::plan::plan_is_extended("free"));
    }
}

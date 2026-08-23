//! Billing-Domaene: statischer Plan-Katalog und Preis-Logik.
//!
//! Drei Stufen (Free, Plus, Pro), Endpreise nach Paragraph 19 UStG, Jahrespreis
//! als eigener Betrag. Reine Konstanten und Preisarithmetik, kein HTTP, keine DB.

pub mod catalog;

pub use catalog::{
    catalog_json, compute_plan_price, cycle_discount_percent, cycle_label, find_plan,
    format_eur_cents, is_paid_plan_id, lookup_key, normalize_billing_cycle, parse_price_id_mapping,
    parse_product_id_mapping, price_id_default, price_id_map_from_env, product_id_default,
    product_id_map_from_env, resolved_price_id, resolved_product_id, BillingPlan, PlanPrice,
    BILLING_CYCLES, BILLING_PLANS, PRICE_ID_DEFAULTS, PRODUCT_ID_DEFAULTS, YEARLY_MONTHS_CHARGED,
    YEARLY_MONTHS_FREE,
};

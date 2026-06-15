//! Billing-Domäne: statischer Plan-Katalog und Preis-Logik.
//!
//! Wert-identische Portierung von Pythons `bot/dashboard/billing/billing_plans.py`
//! (Plan-Blueprints, Zyklus-Rabatte, Stripe-Price/Product-Defaults) und der
//! abgeleiteten Plan-Metadaten aus `bot/entitlements/catalog.py` (Tier,
//! Entitlements). Reine Konstanten + Preisarithmetik — kein HTTP, keine DB.
//!
//! Foundation-Ticket **B2-F2-billing-catalog-consts**: schaltet den gesamten
//! Block 2 (Checkout, Webhook, Katalog-/Readiness-APIs, Product-Price-Sync) frei.

pub mod catalog;

pub use catalog::{
    catalog_json, compute_plan_price, cycle_discount_percent, cycle_label, find_plan,
    format_eur_cents, is_paid_plan_id, lookup_key, normalize_billing_cycle, price_id_default,
    product_id_default, BillingPlan, PlanPrice, BILLING_PLANS, CYCLE_DISCOUNTS, PRICE_ID_DEFAULTS,
    PRODUCT_ID_DEFAULTS,
};

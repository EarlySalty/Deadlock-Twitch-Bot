//! Nativer Stripe-Zugriff (Ersatz für das `stripe`-Python-SDK).
//!
//! Foundation-Ticket **B2-F1-stripe-client**: stellt den HTTP-Client
//! ([`client`]) und die Webhook-Signatur-Verifikation ([`webhook_sig`]) bereit
//! und schaltet damit den gesamten Block 2 (Checkout, Webhook→Entitlements,
//! Cancel, Product-Price-Sync, Affiliate-Connect) frei.
//!
//! Secrets (Secret-Key, Webhook-Signing-Secret) stammen ausschließlich aus
//! Infisical/Env und werden niemals geloggt oder in Fehlertypen transportiert.

pub mod client;
pub mod webhook_apply;
pub mod webhook_sig;

pub use client::{form_pairs, StripeClient, StripeError};
pub use webhook_apply::{
    apply_event, ensure_event_table, plan_name_from_id, record_event_once,
    subscription_payload_from_object, SubscriptionState, WebhookAction,
};
pub use webhook_sig::{verify_signature, WebhookError, DEFAULT_TOLERANCE_SECONDS};

//! Anwendung verifizierter Stripe-Webhook-Events auf den Plan-/Entitlement-Zustand.
//!
//! Wert-identischer Port von
//! `bot/dashboard/billing/billing_mixin.py:_billing_apply_webhook_event`
//! (plus `_billing_subscription_payload_from_object`,
//! `_billing_upsert_subscription_state`, `_billing_sync_plan_to_streamer_plans`,
//! `_billing_plan_name_from_id`).
//!
//! **Quelle der Wahrheit fürs Bezahlt-Sein** (Grillme Block 2A): Stripe meldet
//! Subscription-Lifecycle-Events; dieser Code schreibt sie nach
//! `twitch_billing_subscriptions` (Roh-Abo-Zustand) und spiegelt den effektiven
//! Plan nach `streamer_plans` (`plan_name`/`expires_at`).
//!
//! Die HTTP-/Signatur-Schicht (Roh-Body, `Stripe-Signature`,
//! [`super::webhook_sig::verify_signature`], Event-Dedup) liegt im Dashboard-
//! Handler; hier ist nur die idempotente Zustands-Anwendung pro bereits
//! deduplizierter Event-Payload.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tb_raid::PartnerScoreRefresher;

use crate::billing::normalize_billing_cycle;

/// Ergebnis-Aktion der Event-Anwendung (stabile Strings, Python-Parität — werden
/// in der JSON-Antwort des Handlers gespiegelt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookAction {
    /// `event_type` war leer.
    IgnoredMissingType,
    /// `customer.subscription.*` → Abo-Zustand aktualisiert.
    SubscriptionStateUpdated,
    /// `checkout.session.completed` (mode=subscription) → Abo erfasst.
    CheckoutSubscriptionRecorded,
    /// `checkout.session.completed` mit anderem `mode` → ignoriert.
    CheckoutIgnoredNonSubscription,
    /// `invoice.payment_succeeded` mit Subscription → Zahlung erfasst.
    InvoicePaymentRecorded,
    /// `invoice.payment_failed` mit Subscription → `past_due` gesetzt.
    InvoiceFailureRecorded,
    /// Invoice-Event ohne `subscription` → ignoriert.
    InvoiceIgnoredWithoutSubscription,
    /// Unbekannter/nicht unterstützter Event-Typ → No-op.
    IgnoredUnsupportedEvent,
}

impl WebhookAction {
    /// Stabiler Status-String (identisch zu Pythons Rückgabewerten).
    pub fn as_str(self) -> &'static str {
        match self {
            WebhookAction::IgnoredMissingType => "ignored_missing_type",
            WebhookAction::SubscriptionStateUpdated => "subscription_state_updated",
            WebhookAction::CheckoutSubscriptionRecorded => "checkout_subscription_recorded",
            WebhookAction::CheckoutIgnoredNonSubscription => "checkout_ignored_non_subscription",
            WebhookAction::InvoicePaymentRecorded => "invoice_payment_recorded",
            WebhookAction::InvoiceFailureRecorded => "invoice_failure_recorded",
            WebhookAction::InvoiceIgnoredWithoutSubscription => {
                "invoice_ignored_without_subscription"
            }
            WebhookAction::IgnoredUnsupportedEvent => "ignored_unsupported_event",
        }
    }
}

/// Aus einer Stripe-Subscription extrahierter Roh-Zustand.
///
/// Port von `_billing_subscription_payload_from_object`. Felder sind bereits
/// normalisiert (getrimmte Strings, ISO-8601-Zeitstempel, `cycle_months` ≥ 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionState {
    pub stripe_subscription_id: String,
    pub stripe_customer_id: String,
    pub customer_reference: String,
    pub status: String,
    pub plan_id: String,
    pub cycle_months: i32,
    pub quantity: i32,
    pub current_period_start: Option<String>,
    pub current_period_end: Option<String>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<String>,
    pub ended_at: Option<String>,
    pub last_event_id: String,
}

/// Nachgelagerter Sync in `streamer_plans`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamerPlanSync {
    pub customer_reference: String,
    pub plan_id: String,
    pub status: String,
    pub current_period_end: Option<String>,
    pub bonus_months: i64,
}

impl StreamerPlanSync {
    fn from_subscription_state(state: &SubscriptionState, bonus_months: i64) -> Self {
        Self {
            customer_reference: state.customer_reference.clone(),
            plan_id: state.plan_id.clone(),
            status: state.status.clone(),
            current_period_end: state.current_period_end.clone(),
            bonus_months,
        }
    }
}

/// `plan_id` (Stripe-/Katalog-seitig) → `streamer_plans.plan_name`.
///
/// 1:1 `_billing_plan_name_from_id`: nur drei Pläne mappen auf einen
/// Nicht-Frei-Namen, alles andere fällt auf `"free"`. (Bewusst KEINE
/// Voll-Auflösung des Katalogs — diese Engführung ist das Python-Orakel.)
pub fn plan_name_from_id(plan_id: &str) -> &'static str {
    match plan_id.trim() {
        "raid_boost" => "raid_boost",
        "analysis_dashboard" => "analysis",
        "bundle_analysis_raid_boost" => "bundle",
        _ => "free",
    }
}

/// `serde_json`-Helfer: getrimmter String an `obj[key]` (sonst `""`).
fn str_field(obj: &Value, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Epoch-Sekunden (`obj[key]`) → ISO-8601-UTC, analog `_billing_epoch_to_iso`.
/// `<= 0`, fehlend oder nicht-numerisch → `None`.
fn epoch_to_iso(obj: &Value, key: &str) -> Option<String> {
    let epoch = obj.get(key).and_then(value_as_i64).unwrap_or(0);
    if epoch <= 0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp(epoch, 0).map(|dt| dt.to_rfc3339())
}

/// Liest einen i64 aus Zahl ODER numerischem String (Stripe liefert Epochs als
/// JSON-Zahl; Metadaten-Felder kommen als String).
fn value_as_i64(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(f) = v.as_f64() {
        return Some(f as i64);
    }
    v.as_str().and_then(|s| s.trim().parse::<i64>().ok())
}

/// Extrahiert den Abo-Zustand aus einem Stripe-Subscription-Objekt.
///
/// Port von `_billing_subscription_payload_from_object`. `plan_id` wird in der
/// Python-Reihenfolge aufgelöst: Subscription-`metadata.plan_id` →
/// `price.metadata.plan_id` → `price.lookup_key`.
pub fn subscription_payload_from_object(sub: &Value) -> SubscriptionState {
    let metadata = sub.get("metadata").cloned().unwrap_or(Value::Null);
    let items_data = sub
        .get("items")
        .and_then(|i| i.get("data"))
        .and_then(Value::as_array);
    let first_item = items_data
        .and_then(|arr| arr.first())
        .cloned()
        .unwrap_or(Value::Null);
    let price = first_item.get("price").cloned().unwrap_or(Value::Null);
    let price_metadata = price.get("metadata").cloned().unwrap_or(Value::Null);
    let recurring = price.get("recurring").cloned().unwrap_or(Value::Null);

    let interval = str_field(&recurring, "interval");
    let interval_count = recurring
        .get("interval_count")
        .and_then(value_as_i64)
        .unwrap_or(1);
    // Python: cycle_months = interval_count if interval == "month" else 1.
    let cycle_months = if interval == "month" {
        i32::try_from(interval_count).unwrap_or(1)
    } else {
        1
    };
    let quantity = first_item
        .get("quantity")
        .and_then(value_as_i64)
        .and_then(|q| i32::try_from(q).ok())
        .unwrap_or(1);

    // plan_id: metadata.plan_id → price.metadata.plan_id → price.lookup_key.
    let plan_id = {
        let from_meta = str_field(&metadata, "plan_id");
        if !from_meta.is_empty() {
            from_meta
        } else {
            let from_price_meta = str_field(&price_metadata, "plan_id");
            if !from_price_meta.is_empty() {
                from_price_meta
            } else {
                str_field(&price, "lookup_key")
            }
        }
    };

    SubscriptionState {
        stripe_subscription_id: str_field(sub, "id"),
        stripe_customer_id: str_field(sub, "customer"),
        customer_reference: str_field(&metadata, "customer_reference"),
        status: {
            let s = str_field(sub, "status");
            if s.is_empty() {
                "unknown".to_string()
            } else {
                s
            }
        },
        plan_id,
        cycle_months,
        quantity,
        current_period_start: epoch_to_iso(sub, "current_period_start"),
        current_period_end: epoch_to_iso(sub, "current_period_end"),
        cancel_at_period_end: sub
            .get("cancel_at_period_end")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        canceled_at: epoch_to_iso(sub, "canceled_at"),
        ended_at: epoch_to_iso(sub, "ended_at"),
        last_event_id: String::new(),
    }
}

fn checkout_subscription_state_from_event(
    event_id: &str,
    event_object: &Value,
    retrieved_subscription: Option<&Value>,
) -> (SubscriptionState, i64) {
    let metadata = event_object.get("metadata").cloned().unwrap_or(Value::Null);
    // customer_reference: metadata.customer_reference → client_reference_id.
    let customer_reference = {
        let from_meta = str_field(&metadata, "customer_reference");
        if !from_meta.is_empty() {
            from_meta
        } else {
            str_field(event_object, "client_reference_id")
        }
    };

    // Dünner Basis-Zustand aus der Checkout-Session.
    let mut payload = SubscriptionState {
        stripe_subscription_id: str_field(event_object, "subscription"),
        stripe_customer_id: str_field(event_object, "customer"),
        customer_reference: customer_reference.clone(),
        status: "active".to_string(),
        plan_id: str_field(&metadata, "plan_id"),
        cycle_months: i32::try_from(normalize_billing_cycle(
            metadata
                .get("cycle_months")
                .and_then(value_as_i64)
                .and_then(|c| u32::try_from(c).ok())
                .unwrap_or(1),
        ))
        .unwrap_or(1),
        quantity: metadata
            .get("quantity")
            .and_then(value_as_i64)
            .and_then(|q| i32::try_from(q).ok())
            .filter(|q| *q >= 1)
            .unwrap_or(1),
        cancel_at_period_end: false,
        last_event_id: event_id.to_string(),
        ..SubscriptionState::default()
    };

    // Volle Subscription nachladen (vom Aufrufer) → überschreibt den dünnen
    // Zustand, aber customer_reference aus der Session bleibt Fallback.
    if !payload.stripe_subscription_id.is_empty() {
        if let Some(sub) = retrieved_subscription {
            let mut sub_payload = subscription_payload_from_object(sub);
            if sub_payload.customer_reference.trim().is_empty() {
                sub_payload.customer_reference = customer_reference;
            }
            sub_payload.last_event_id = event_id.to_string();
            payload = sub_payload;
        }
    }

    // bonus_months (annual) aus der NACHGELADENEN Subscription-Metadata lesen
    // (Python liest sub_meta.bonus_months erst NACH dem Subscription-Retrieve,
    // billing_mixin.py:707-712). Nur der full-subscription-Pfad trägt sie.
    let bonus_months = retrieved_subscription
        .filter(|_| !payload.stripe_subscription_id.is_empty())
        .and_then(|sub| sub.get("metadata"))
        .and_then(|meta| meta.get("bonus_months"))
        .and_then(value_as_i64)
        .filter(|n| *n > 0)
        .unwrap_or(0);

    (payload, bonus_months)
}

/// Leitet den nachgelagerten `streamer_plans`-Sync aus einem Webhook-Event ab.
pub fn streamer_plan_sync_from_event(
    event_type: &str,
    event_object: &Value,
    retrieved_subscription: Option<&Value>,
) -> Option<StreamerPlanSync> {
    let event_name = event_type.trim();
    if event_name.starts_with("customer.subscription.") {
        let payload = subscription_payload_from_object(event_object);
        return Some(StreamerPlanSync::from_subscription_state(&payload, 0));
    }
    if event_name == "checkout.session.completed"
        && str_field(event_object, "mode") == "subscription"
    {
        let (payload, bonus_months) =
            checkout_subscription_state_from_event("", event_object, retrieved_subscription);
        return Some(StreamerPlanSync::from_subscription_state(
            &payload,
            bonus_months,
        ));
    }
    None
}

/// UPSERT in `twitch_billing_subscriptions` mit Merge gegen den Bestand
/// (Port von `_billing_upsert_subscription_state`).
///
/// Leere/`None`-Felder fallen auf den bestehenden Wert zurück, nie auf eine
/// Überschreibung mit Leer — so überschreiben dünne Events (z. B.
/// `invoice.payment_succeeded` mit nur Status) keine vollen Abo-Daten.
async fn upsert_subscription_state(
    tx: &mut sqlx::PgConnection,
    state: &SubscriptionState,
) -> Result<(), sqlx::Error> {
    let sub_id = state.stripe_subscription_id.trim();
    if sub_id.is_empty() {
        return Ok(());
    }

    let existing = sqlx::query_as!(
        ExistingRow,
        r#"SELECT
               stripe_customer_id, customer_reference, status AS "status?", plan_id, cycle_months AS "cycle_months?",
               quantity AS "quantity?", current_period_start, current_period_end, cancel_at_period_end AS "cancel_at_period_end?",
               canceled_at, ended_at, last_event_id
           FROM twitch_billing_subscriptions
           WHERE stripe_subscription_id = $1"#,
        sub_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let existing = existing.unwrap_or_default();

    let merge = |new: &str, old: &Option<String>| -> Option<String> {
        let n = new.trim();
        if !n.is_empty() {
            return Some(n.to_string());
        }
        old.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let final_customer_id = merge(&state.stripe_customer_id, &existing.stripe_customer_id);
    let final_customer_reference = merge(&state.customer_reference, &existing.customer_reference);
    let final_status = {
        let s = state.status.trim();
        if !s.is_empty() {
            s.to_string()
        } else {
            existing
                .status
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or("unknown")
                .to_string()
        }
    };
    let final_plan_id = merge(&state.plan_id, &existing.plan_id);
    // cycle_months: neuer Wert falls > 0, sonst Bestand, mind. 1.
    let final_cycle_months = {
        let candidate = if state.cycle_months != 0 {
            state.cycle_months
        } else {
            existing.cycle_months.unwrap_or(1)
        };
        candidate.max(1)
    };
    // quantity: neuer Wert falls > 0, sonst Bestand, geklemmt auf [1, 24].
    let final_quantity = {
        let candidate = if state.quantity != 0 {
            state.quantity
        } else {
            existing.quantity.unwrap_or(1)
        };
        candidate.clamp(1, 24)
    };
    // Zeit-/Flag-Felder: None → Bestand übernehmen.
    let final_period_start = state
        .current_period_start
        .clone()
        .or(existing.current_period_start);
    let final_period_end = state
        .current_period_end
        .clone()
        .or(existing.current_period_end);
    // cancel_at_period_end: Event setzt immer explizit (bool); Python merget nur
    // bei None — die Subscription-/Checkout-Pfade liefern aber stets einen Wert.
    let final_cancel = i32::from(state.cancel_at_period_end);
    let final_canceled_at = state.canceled_at.clone().or(existing.canceled_at);
    let final_ended_at = state.ended_at.clone().or(existing.ended_at);
    let final_last_event_id = merge(&state.last_event_id, &existing.last_event_id);
    let updated_at = Utc::now().to_rfc3339();

    sqlx::query!(
        r#"INSERT INTO twitch_billing_subscriptions (
               stripe_subscription_id, stripe_customer_id, customer_reference, status,
               plan_id, cycle_months, quantity, current_period_start, current_period_end,
               cancel_at_period_end, canceled_at, ended_at, last_event_id, updated_at
           ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
           ON CONFLICT (stripe_subscription_id) DO UPDATE SET
               stripe_customer_id = EXCLUDED.stripe_customer_id,
               customer_reference = EXCLUDED.customer_reference,
               status = EXCLUDED.status,
               plan_id = EXCLUDED.plan_id,
               cycle_months = EXCLUDED.cycle_months,
               quantity = EXCLUDED.quantity,
               current_period_start = EXCLUDED.current_period_start,
               current_period_end = EXCLUDED.current_period_end,
               cancel_at_period_end = EXCLUDED.cancel_at_period_end,
               canceled_at = EXCLUDED.canceled_at,
               ended_at = EXCLUDED.ended_at,
               last_event_id = EXCLUDED.last_event_id,
               updated_at = EXCLUDED.updated_at"#,
        sub_id,
        final_customer_id,
        final_customer_reference,
        &final_status,
        final_plan_id,
        final_cycle_months,
        final_quantity,
        final_period_start,
        final_period_end,
        final_cancel,
        final_canceled_at,
        final_ended_at,
        final_last_event_id,
        &updated_at
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

#[derive(Default, sqlx::FromRow)]
struct ExistingRow {
    stripe_customer_id: Option<String>,
    customer_reference: Option<String>,
    status: Option<String>,
    plan_id: Option<String>,
    cycle_months: Option<i32>,
    quantity: Option<i32>,
    current_period_start: Option<String>,
    current_period_end: Option<String>,
    #[allow(dead_code)]
    cancel_at_period_end: Option<i32>,
    canceled_at: Option<String>,
    ended_at: Option<String>,
    last_event_id: Option<String>,
}

/// Spiegelt den Abo-Plan nach `streamer_plans` (Port von
/// `_billing_sync_plan_to_streamer_plans`).
///
/// `customer_reference` ist ein Twitch-Login; er wird über die View
/// `twitch_streamers_partner_state` auf `twitch_user_id` aufgelöst, dann nach
/// `streamer_plans` (Konflikt auf `twitch_user_id`) geschrieben. Aktiv
/// (`active`/`trialing`) → gemappter Plan-Name, sonst `free`. `expires_at` wird
/// auf `NULL` gesetzt (Billing-Abo läuft, kein manuelles Ablaufdatum).
async fn sync_plan_to_streamer_plans_tx(
    tx: &mut sqlx::PgConnection,
    customer_reference: &str,
    plan_id: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    let reference = customer_reference.trim();
    if reference.is_empty() {
        return Ok(());
    }
    let is_active = matches!(status.trim(), "active" | "trialing");
    let effective_plan = if is_active {
        plan_name_from_id(plan_id)
    } else {
        "free"
    };

    sqlx::query!(
        r#"INSERT INTO streamer_plans (twitch_user_id, twitch_login, plan_name, expires_at)
           SELECT twitch_user_id, twitch_login, $1, NULL
           FROM twitch_streamers_partner_state
           WHERE LOWER(twitch_login) = LOWER($2)
           ON CONFLICT (twitch_user_id) DO UPDATE SET
               plan_name = EXCLUDED.plan_name,
               expires_at = EXCLUDED.expires_at"#,
        effective_plan,
        reference
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Wendet den aus einem Webhook abgeleiteten Plan-Sync nachgelagert in eigener
/// Transaktion an.
pub async fn sync_plan_to_streamer_plans(
    pool: &PgPool,
    sync: &StreamerPlanSync,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sync_plan_to_streamer_plans_tx(
        &mut tx,
        &sync.customer_reference,
        &sync.plan_id,
        &sync.status,
    )
    .await?;
    if sync.bonus_months > 0 && !sync.customer_reference.trim().is_empty() {
        grant_bonus_access_months(
            &mut tx,
            &sync.customer_reference,
            &sync.plan_id,
            sync.current_period_end.as_deref().unwrap_or(""),
            sync.bonus_months,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Verlängert `manual_plan_expires_at` um `bonus_months` über das Abo-Ende
/// hinaus (Port von `_billing_grant_bonus_access_months`, billing_mixin.py:589-630).
///
/// `period_end_iso` ist das ISO-8601-Abo-Ende (`current_period_end`). Ein Bonus
/// von N Monaten verschiebt das Ablaufdatum um `N*31` Tage. Ungültiges/leeres
/// `period_end_iso` oder `bonus_months <= 0` → No-op (wie Python). Der Login wird
/// über `twitch_streamers_partner_state` auf `twitch_user_id` aufgelöst.
async fn grant_bonus_access_months(
    tx: &mut sqlx::PgConnection,
    customer_reference: &str,
    plan_id: &str,
    period_end_iso: &str,
    bonus_months: i64,
) -> Result<(), sqlx::Error> {
    let reference = customer_reference.trim();
    let period_end_iso = period_end_iso.trim();
    if reference.is_empty() || period_end_iso.is_empty() || bonus_months <= 0 {
        return Ok(());
    }
    // Abo-Ende parsen; ungültig → No-op (Python schluckt den Fehler).
    let Ok(period_end) = DateTime::parse_from_rfc3339(period_end_iso) else {
        tracing::debug!(
            period_end_iso,
            "billing bonus grant: invalid period_end_iso"
        );
        return Ok(());
    };
    let bonus_expires_at =
        period_end.with_timezone(&Utc) + chrono::Duration::days(bonus_months * 31);
    let bonus_expires_iso = bonus_expires_at.to_rfc3339();
    let today = Utc::now().date_naive();
    // Hinweis: rein technischer Audit-Vermerk, kein user-sichtbarer Text.
    let notes = format!("bonus {bonus_months}mo: annual (auto {today})");
    let updated_at = Utc::now().to_rfc3339();

    sqlx::query!(
        r#"UPDATE streamer_plans
           SET manual_plan_id = $1,
               manual_plan_expires_at = $2,
               manual_plan_notes = $3,
               manual_plan_updated_at = $4
           WHERE twitch_user_id = (
               SELECT twitch_user_id
               FROM twitch_streamers_partner_state
               WHERE LOWER(twitch_login) = LOWER($5)
               LIMIT 1
           )"#,
        plan_id,
        &bonus_expires_iso,
        &notes,
        &updated_at,
        reference
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Ermittelt den Twitch-Login, dessen Partner-Raid-Score nach einer Billing-
/// Änderung neu berechnet werden muss (P2.127/P2.128).
///
/// Spiegelt Pythons Refresh-Trigger: `customer.subscription.*` und
/// `checkout.session.completed` (mode=subscription) ändern den Plan und damit den
/// Raid-Boost-Tier. `customer_reference` (Login) wird in Python-Reihenfolge
/// aufgelöst: Subscription-/Session-`metadata.customer_reference` →
/// `client_reference_id` (nur Checkout). Andere Events → `None` (kein Refresh).
pub fn affected_login_for_billing_refresh(
    event_type: &str,
    event_object: &Value,
    retrieved_subscription: Option<&Value>,
) -> Option<String> {
    let event_name = event_type.trim();
    let reference = if event_name.starts_with("customer.subscription.") {
        let payload = subscription_payload_from_object(event_object);
        payload.customer_reference
    } else if event_name == "checkout.session.completed"
        && str_field(event_object, "mode") == "subscription"
    {
        // Volle Subscription bevorzugen (wie apply_event), sonst Session-Metadaten.
        let from_sub = retrieved_subscription
            .map(subscription_payload_from_object)
            .map(|s| s.customer_reference)
            .filter(|s| !s.trim().is_empty());
        from_sub.unwrap_or_else(|| {
            let metadata = event_object.get("metadata").cloned().unwrap_or(Value::Null);
            let from_meta = str_field(&metadata, "customer_reference");
            if from_meta.is_empty() {
                str_field(event_object, "client_reference_id")
            } else {
                from_meta
            }
        })
    } else {
        return None;
    };
    let reference = reference.trim().to_string();
    if reference.is_empty() {
        None
    } else {
        Some(reference)
    }
}

/// Berechnet den Partner-Raid-Score für einen Login nach einer Billing-Änderung
/// sofort neu (P2.127/P2.128/P2.129; Port von
/// `_billing_refresh_partner_raid_score_cache`, billing_mixin.py:52-108).
///
/// Der Login wird über `twitch_streamers_partner_state` auf `twitch_user_id`
/// aufgelöst und an [`PartnerScoreRefresher::refresh_for_ids`] übergeben (recompute
/// + upsert in `twitch_partner_raid_scores`). Wie in Python ist das ein
///   **best-effort**-Hook: ist der Login unbekannt oder existiert keine
///   Partner-Zeile, passiert nichts (kein Fehler nach außen).
pub async fn refresh_partner_raid_score_for_login(
    pool: &PgPool,
    login: &str,
) -> Result<(), sqlx::Error> {
    let login = login.trim();
    if login.is_empty() {
        return Ok(());
    }
    let row = sqlx::query_scalar!(
        r#"SELECT twitch_user_id
           FROM twitch_streamers_partner_state
           WHERE LOWER(twitch_login) = LOWER($1)
           LIMIT 1"#,
        login
    )
    .fetch_optional(pool)
    .await?;
    let Some(Some(user_id)) = row else {
        return Ok(());
    };
    let user_id = user_id.trim().to_string();
    if user_id.is_empty() {
        return Ok(());
    }
    let refresher = PartnerScoreRefresher::new(pool.clone());
    refresher.refresh_for_ids(&[user_id], Utc::now()).await?;
    Ok(())
}

/// Wendet ein bereits verifiziertes und dedupliziertes Event auf den Zustand an.
///
/// 1:1 `_billing_apply_webhook_event`. Für `checkout.session.completed`
/// (mode=subscription) muss die volle Subscription vom Aufrufer nachgeladen und
/// als `retrieved_subscription` übergeben werden (analog Pythons
/// `stripe.Subscription.retrieve` mit `expand=items.data.price`); ist sie `None`,
/// wird nur der dünne Checkout-Zustand (`status=active` + Metadaten) erfasst.
///
/// `event_object` ist das `data.object` des Events.
pub async fn apply_event(
    tx: &mut sqlx::PgConnection,
    event_id: &str,
    event_type: &str,
    event_object: &Value,
    retrieved_subscription: Option<&Value>,
) -> Result<WebhookAction, sqlx::Error> {
    let event_name = event_type.trim();
    if event_name.is_empty() {
        return Ok(WebhookAction::IgnoredMissingType);
    }

    if let Some(_suffix) = event_name.strip_prefix("customer.subscription.") {
        let mut payload = subscription_payload_from_object(event_object);
        payload.last_event_id = event_id.to_string();
        upsert_subscription_state(tx, &payload).await?;
        return Ok(WebhookAction::SubscriptionStateUpdated);
    }

    if event_name == "checkout.session.completed" {
        let mode = str_field(event_object, "mode");
        if mode != "subscription" {
            return Ok(WebhookAction::CheckoutIgnoredNonSubscription);
        }
        let (payload, _bonus_months) =
            checkout_subscription_state_from_event(event_id, event_object, retrieved_subscription);

        upsert_subscription_state(tx, &payload).await?;
        return Ok(WebhookAction::CheckoutSubscriptionRecorded);
    }

    if event_name == "invoice.payment_succeeded" {
        let subscription_id = str_field(event_object, "subscription");
        if subscription_id.is_empty() {
            return Ok(WebhookAction::InvoiceIgnoredWithoutSubscription);
        }
        let state = SubscriptionState {
            stripe_subscription_id: subscription_id,
            stripe_customer_id: str_field(event_object, "customer"),
            status: "active".to_string(),
            last_event_id: event_id.to_string(),
            ..SubscriptionState::default()
        };
        upsert_subscription_state(tx, &state).await?;
        // Affiliate-Provision (30 % bei Zahlung): wert-identisch in
        // [`crate::affiliate_commission::process_commission`] portiert. Wie Pythons
        // Webhook-Route wird sie NICHT hier (in der DB-Transaktion) aufgerufen,
        // sondern vom Webhook-Handler NACH `apply_event` mit eigenem Pool/Lock +
        // Stripe-Client — die nötigen Felder (amount_paid/currency/invoice_id/
        // period_start/period_end aus `lines.data[0].period`) stehen im
        // event_object bereit. Wiring liegt in `tb-dashboard-api` (siehe handoff).
        return Ok(WebhookAction::InvoicePaymentRecorded);
    }

    if event_name == "invoice.payment_failed" {
        let subscription_id = str_field(event_object, "subscription");
        if subscription_id.is_empty() {
            return Ok(WebhookAction::InvoiceIgnoredWithoutSubscription);
        }
        let state = SubscriptionState {
            stripe_subscription_id: subscription_id,
            stripe_customer_id: str_field(event_object, "customer"),
            status: "past_due".to_string(),
            last_event_id: event_id.to_string(),
            ..SubscriptionState::default()
        };
        upsert_subscription_state(tx, &state).await?;
        return Ok(WebhookAction::InvoiceFailureRecorded);
    }

    Ok(WebhookAction::IgnoredUnsupportedEvent)
}

/// Idempotenter Dedup-Insert in `twitch_billing_events`.
///
/// Gibt `Ok(true)` zurück, wenn das Event NEU war (Insert erfolgreich),
/// `Ok(false)` bei Duplikat (`stripe_event_id` schon vorhanden — Stripe-Replay).
/// Port der Dedup-Logik aus `routes_billing.py:api_billing_stripe_webhook`.
pub async fn record_event_once(
    tx: &mut sqlx::PgConnection,
    event_id: &str,
    event_type: &str,
    object_id: &str,
    livemode: bool,
    payload: &str,
) -> Result<bool, sqlx::Error> {
    if event_id.trim().is_empty() {
        // Ohne Event-ID keine Dedup möglich → wie Python: trotzdem anwenden.
        return Ok(true);
    }
    let received_at = Utc::now().to_rfc3339();
    let result = sqlx::query!(
        r#"INSERT INTO twitch_billing_events
               (stripe_event_id, event_type, object_id, received_at, livemode, payload)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (stripe_event_id) DO NOTHING"#,
        event_id,
        event_type,
        object_id,
        &received_at,
        i32::from(livemode),
        payload
    )
    .execute(&mut *tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Test fixture for `twitch_billing_events`; canonical schema lives in migration
/// 20260630144000.
pub async fn ensure_event_table(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"CREATE TABLE IF NOT EXISTS twitch_billing_events (
               stripe_event_id TEXT PRIMARY KEY,
               event_type TEXT NOT NULL,
               object_id TEXT,
               received_at TEXT NOT NULL,
               livemode INTEGER NOT NULL DEFAULT 0,
               payload TEXT NOT NULL
           )"#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Plan-Name-Mapping (Geld-kritisch: nur 3 Pläne ≠ free) ───────────────
    #[test]
    fn plan_name_map_matches_python_oracle() {
        assert_eq!(plan_name_from_id("raid_boost"), "raid_boost");
        assert_eq!(plan_name_from_id("analysis_dashboard"), "analysis");
        assert_eq!(plan_name_from_id("bundle_analysis_raid_boost"), "bundle");
        // Alles andere → free (auch sonst bezahlte Pläne — Python-Engführung).
        assert_eq!(plan_name_from_id("chat_quiet"), "free");
        assert_eq!(plan_name_from_id("bundle_komplett"), "free");
        assert_eq!(plan_name_from_id("raid_free"), "free");
        assert_eq!(plan_name_from_id(""), "free");
        assert_eq!(plan_name_from_id("  raid_boost  "), "raid_boost");
    }

    // ── Subscription-Payload-Extraktion ─────────────────────────────────────
    #[test]
    fn payload_extraction_full_object() {
        let sub = json!({
            "id": "sub_123",
            "customer": "cus_abc",
            "status": "active",
            "metadata": { "customer_reference": "streamerlogin", "plan_id": "raid_boost" },
            "current_period_start": 1_700_000_000,
            "current_period_end": 1_702_000_000,
            "cancel_at_period_end": true,
            "items": { "data": [ {
                "quantity": 1,
                "price": {
                    "lookup_key": "deadlock_raid_boost_1m_net_v2",
                    "recurring": { "interval": "month", "interval_count": 1 },
                    "metadata": {}
                }
            } ] }
        });
        let state = subscription_payload_from_object(&sub);
        assert_eq!(state.stripe_subscription_id, "sub_123");
        assert_eq!(state.stripe_customer_id, "cus_abc");
        assert_eq!(state.customer_reference, "streamerlogin");
        assert_eq!(state.status, "active");
        assert_eq!(state.plan_id, "raid_boost");
        assert_eq!(state.cycle_months, 1);
        assert_eq!(state.quantity, 1);
        assert!(state.cancel_at_period_end);
        assert!(state.current_period_start.is_some());
        assert!(state.current_period_end.is_some());
    }

    #[test]
    fn payload_plan_id_resolution_order() {
        // metadata.plan_id leer → price.metadata.plan_id.
        let sub = json!({
            "id": "sub_1", "customer": "c", "status": "active", "metadata": {},
            "items": { "data": [ { "price": {
                "metadata": { "plan_id": "analysis_dashboard" },
                "lookup_key": "ignored_key",
                "recurring": { "interval": "year", "interval_count": 1 }
            } } ] }
        });
        let state = subscription_payload_from_object(&sub);
        assert_eq!(state.plan_id, "analysis_dashboard");
        // interval != month → cycle 1.
        assert_eq!(state.cycle_months, 1);

        // beide metadata leer → lookup_key.
        let sub2 = json!({
            "id": "s", "status": "active", "metadata": {},
            "items": { "data": [ { "price": {
                "metadata": {}, "lookup_key": "fallback_key",
                "recurring": { "interval": "month", "interval_count": 12 }
            } } ] }
        });
        let state2 = subscription_payload_from_object(&sub2);
        assert_eq!(state2.plan_id, "fallback_key");
        assert_eq!(state2.cycle_months, 12);
    }

    #[test]
    fn payload_empty_status_defaults_unknown() {
        let sub = json!({ "id": "s", "metadata": {} });
        let state = subscription_payload_from_object(&sub);
        assert_eq!(state.status, "unknown");
        assert_eq!(state.cycle_months, 1);
        assert_eq!(state.quantity, 1);
        assert!(state.current_period_end.is_none());
    }

    #[test]
    fn epoch_to_iso_handles_zero_and_missing() {
        let obj = json!({ "a": 0, "b": 1_700_000_000_i64 });
        assert!(epoch_to_iso(&obj, "a").is_none());
        assert!(epoch_to_iso(&obj, "missing").is_none());
        assert!(epoch_to_iso(&obj, "b").is_some());
    }

    #[test]
    fn action_status_strings_match_python() {
        assert_eq!(
            WebhookAction::IgnoredMissingType.as_str(),
            "ignored_missing_type"
        );
        assert_eq!(
            WebhookAction::SubscriptionStateUpdated.as_str(),
            "subscription_state_updated"
        );
        assert_eq!(
            WebhookAction::CheckoutSubscriptionRecorded.as_str(),
            "checkout_subscription_recorded"
        );
        assert_eq!(
            WebhookAction::CheckoutIgnoredNonSubscription.as_str(),
            "checkout_ignored_non_subscription"
        );
        assert_eq!(
            WebhookAction::InvoicePaymentRecorded.as_str(),
            "invoice_payment_recorded"
        );
        assert_eq!(
            WebhookAction::InvoiceFailureRecorded.as_str(),
            "invoice_failure_recorded"
        );
        assert_eq!(
            WebhookAction::InvoiceIgnoredWithoutSubscription.as_str(),
            "invoice_ignored_without_subscription"
        );
        assert_eq!(
            WebhookAction::IgnoredUnsupportedEvent.as_str(),
            "ignored_unsupported_event"
        );
    }

    // ── DB-Integration (skip ohne TB_TEST_DATABASE_URL) ─────────────────────
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        // Minimal-Schema: Abo-Tabelle, Event-Tabelle, Plan-Tabelle + Login-View-Ersatz.
        sqlx::query(
            r#"CREATE TABLE twitch_billing_subscriptions (
                   stripe_subscription_id TEXT PRIMARY KEY,
                   stripe_customer_id TEXT, customer_reference TEXT,
                   status TEXT NOT NULL DEFAULT 'unknown', plan_id TEXT,
                   cycle_months INTEGER NOT NULL DEFAULT 1, quantity INTEGER NOT NULL DEFAULT 1,
                   current_period_start TEXT, current_period_end TEXT,
                   cancel_at_period_end INTEGER NOT NULL DEFAULT 0,
                   canceled_at TEXT, ended_at TEXT, last_event_id TEXT, updated_at TEXT NOT NULL
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE streamer_plans (
                   twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
                   plan_name TEXT NOT NULL DEFAULT 'free', expires_at TEXT,
                   manual_plan_id TEXT, manual_plan_expires_at TEXT,
                   manual_plan_notes TEXT DEFAULT '', manual_plan_updated_at TEXT,
                   raid_boost_enabled INTEGER NOT NULL DEFAULT 0
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        // Die View ist in Prod read-only; im Test als Tabelle für den Login→UID-Join.
        sqlx::query(
            r#"CREATE TABLE twitch_streamers_partner_state (
                   twitch_login TEXT, twitch_user_id TEXT,
                   is_partner_active INTEGER NOT NULL DEFAULT 1
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        ensure_event_table(&pool).await.unwrap();
        Some(pool)
    }

    async fn sub_row(
        pool: &PgPool,
        sub_id: &str,
    ) -> Option<(String, Option<String>, Option<String>)> {
        sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT status, plan_id, last_event_id FROM twitch_billing_subscriptions WHERE stripe_subscription_id = $1",
        )
        .bind(sub_id)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn subscription_created_sets_plan() {
        let Some(pool) = pool_or_skip("wh_sub_created").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id) VALUES ('streamerlogin','42')")
            .execute(&pool).await.unwrap();
        let event_object = json!({
            "id": "sub_777", "customer": "cus_x", "status": "active",
            "metadata": { "customer_reference": "streamerlogin", "plan_id": "raid_boost" },
            "items": { "data": [ { "quantity": 1, "price": {
                "recurring": { "interval": "month", "interval_count": 1 }, "metadata": {}
            } } ] }
        });
        let mut tx = pool.acquire().await.unwrap();
        let action = apply_event(
            &mut tx,
            "evt_1",
            "customer.subscription.created",
            &event_object,
            None,
        )
        .await
        .unwrap();
        assert_eq!(action, WebhookAction::SubscriptionStateUpdated);
        drop(tx);

        let row = sub_row(&pool, "sub_777").await.unwrap();
        assert_eq!(row.0, "active");
        assert_eq!(row.1.as_deref(), Some("raid_boost"));
        assert_eq!(row.2.as_deref(), Some("evt_1"));
        // streamer_plans synchronisiert nachgelagert: raid_boost → plan_name "raid_boost".
        let sync =
            streamer_plan_sync_from_event("customer.subscription.created", &event_object, None)
                .expect("subscription sync");
        sync_plan_to_streamer_plans(&pool, &sync).await.unwrap();
        let plan: (String, Option<String>) = sqlx::query_as(
            "SELECT plan_name, expires_at FROM streamer_plans WHERE twitch_user_id = '42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(plan.0, "raid_boost");
        assert!(plan.1.is_none());
    }

    #[tokio::test]
    async fn subscription_deleted_clears_plan_to_free() {
        let Some(pool) = pool_or_skip("wh_sub_deleted").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id) VALUES ('login2','7')")
            .execute(&pool).await.unwrap();
        // Erst aktiv setzen.
        let active = json!({
            "id": "sub_d", "customer": "c", "status": "active",
            "metadata": { "customer_reference": "login2", "plan_id": "raid_boost" },
            "items": { "data": [ { "price": { "recurring": { "interval": "month", "interval_count": 1 } } } ] }
        });
        let mut tx = pool.acquire().await.unwrap();
        apply_event(
            &mut tx,
            "e1",
            "customer.subscription.created",
            &active,
            None,
        )
        .await
        .unwrap();
        drop(tx);
        let sync = streamer_plan_sync_from_event("customer.subscription.created", &active, None)
            .expect("active sync");
        sync_plan_to_streamer_plans(&pool, &sync).await.unwrap();
        // Dann löschen (status canceled) → free.
        let deleted = json!({
            "id": "sub_d", "customer": "c", "status": "canceled",
            "metadata": { "customer_reference": "login2", "plan_id": "raid_boost" },
            "items": { "data": [ { "price": { "recurring": { "interval": "month", "interval_count": 1 } } } ] }
        });
        let mut tx = pool.acquire().await.unwrap();
        apply_event(
            &mut tx,
            "e2",
            "customer.subscription.deleted",
            &deleted,
            None,
        )
        .await
        .unwrap();
        drop(tx);
        let sync = streamer_plan_sync_from_event("customer.subscription.deleted", &deleted, None)
            .expect("deleted sync");
        sync_plan_to_streamer_plans(&pool, &sync).await.unwrap();
        let plan: (String,) =
            sqlx::query_as("SELECT plan_name FROM streamer_plans WHERE twitch_user_id = '7'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plan.0, "free");
    }

    #[tokio::test]
    async fn dedup_prevents_double_apply() {
        let Some(pool) = pool_or_skip("wh_dedup").await else {
            return;
        };
        let mut tx = pool.acquire().await.unwrap();
        let first = record_event_once(
            &mut tx,
            "evt_dup",
            "customer.subscription.updated",
            "sub_z",
            false,
            "{}",
        )
        .await
        .unwrap();
        assert!(first, "erstes Event ist neu");
        let second = record_event_once(
            &mut tx,
            "evt_dup",
            "customer.subscription.updated",
            "sub_z",
            false,
            "{}",
        )
        .await
        .unwrap();
        assert!(!second, "Replay desselben event_id ist Duplikat");
    }

    #[tokio::test]
    async fn plan_sync_fehler_rollt_webhook_kern_nicht_zurueck() {
        let Some(pool) = pool_or_skip("wh_sync_err_no_rollback").await else {
            return;
        };
        sqlx::query("DROP TABLE twitch_streamers_partner_state")
            .execute(&pool)
            .await
            .unwrap();
        let event_object = json!({
            "id": "sub_sync_err", "customer": "cus_x", "status": "active",
            "metadata": { "customer_reference": "streamerlogin", "plan_id": "raid_boost" },
            "items": { "data": [ { "price": {
                "recurring": { "interval": "month", "interval_count": 1 }
            } } ] }
        });

        let mut tx = pool.begin().await.unwrap();
        let is_new = record_event_once(
            &mut tx,
            "evt_sync_err",
            "customer.subscription.created",
            "sub_sync_err",
            false,
            "{}",
        )
        .await
        .unwrap();
        assert!(is_new);
        apply_event(
            &mut tx,
            "evt_sync_err",
            "customer.subscription.created",
            &event_object,
            None,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let sync =
            streamer_plan_sync_from_event("customer.subscription.created", &event_object, None)
                .expect("sync payload");
        assert!(sync_plan_to_streamer_plans(&pool, &sync).await.is_err());
        assert!(sub_row(&pool, "sub_sync_err").await.is_some());
        let event_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_billing_events WHERE stripe_event_id = 'evt_sync_err'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(event_count, 1);
    }

    #[tokio::test]
    async fn unknown_event_type_is_noop() {
        let Some(pool) = pool_or_skip("wh_unknown").await else {
            return;
        };
        let mut tx = pool.acquire().await.unwrap();
        let action = apply_event(&mut tx, "e", "customer.created", &json!({"id": "x"}), None)
            .await
            .unwrap();
        assert_eq!(action, WebhookAction::IgnoredUnsupportedEvent);
        let action2 = apply_event(&mut tx, "e", "", &json!({}), None)
            .await
            .unwrap();
        assert_eq!(action2, WebhookAction::IgnoredMissingType);
    }

    #[tokio::test]
    async fn invoice_failed_sets_past_due_without_overwriting_plan() {
        let Some(pool) = pool_or_skip("wh_invoice_failed").await else {
            return;
        };
        // Vorab volles Abo.
        let active = json!({
            "id": "sub_inv", "customer": "c", "status": "active",
            "metadata": { "customer_reference": "l", "plan_id": "raid_boost" },
            "items": { "data": [ { "price": { "recurring": { "interval": "month", "interval_count": 1 } } } ] }
        });
        let mut tx = pool.acquire().await.unwrap();
        apply_event(
            &mut tx,
            "e1",
            "customer.subscription.created",
            &active,
            None,
        )
        .await
        .unwrap();
        drop(tx);
        // invoice.payment_failed (dünn) → status past_due, plan_id bleibt.
        let invoice = json!({ "id": "in_1", "subscription": "sub_inv", "customer": "c" });
        let mut tx = pool.acquire().await.unwrap();
        let action = apply_event(&mut tx, "e2", "invoice.payment_failed", &invoice, None)
            .await
            .unwrap();
        assert_eq!(action, WebhookAction::InvoiceFailureRecorded);
        drop(tx);
        let row = sub_row(&pool, "sub_inv").await.unwrap();
        assert_eq!(row.0, "past_due");
        assert_eq!(
            row.1.as_deref(),
            Some("raid_boost"),
            "dünnes Event darf plan_id nicht löschen"
        );
    }

    #[tokio::test]
    async fn invoice_without_subscription_ignored() {
        let Some(pool) = pool_or_skip("wh_invoice_nosub").await else {
            return;
        };
        let mut tx = pool.acquire().await.unwrap();
        let action = apply_event(
            &mut tx,
            "e",
            "invoice.payment_succeeded",
            &json!({"id": "in"}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(action, WebhookAction::InvoiceIgnoredWithoutSubscription);
    }

    // ── P2.127/P2.128: affected-login-Resolver (rein) ───────────────────────
    #[test]
    fn affected_login_for_subscription_event() {
        let sub = json!({
            "id": "sub_1", "status": "active",
            "metadata": { "customer_reference": "StreamerLogin", "plan_id": "raid_boost" }
        });
        assert_eq!(
            affected_login_for_billing_refresh("customer.subscription.updated", &sub, None)
                .as_deref(),
            Some("StreamerLogin")
        );
    }

    #[test]
    fn affected_login_for_checkout_prefers_subscription_then_client_ref() {
        // checkout ohne Metadata-Login, mit client_reference_id.
        let session = json!({
            "mode": "subscription", "subscription": "sub_x",
            "client_reference_id": "fallbacklogin", "metadata": {}
        });
        assert_eq!(
            affected_login_for_billing_refresh("checkout.session.completed", &session, None)
                .as_deref(),
            Some("fallbacklogin")
        );
        // mit nachgeladener Subscription, deren metadata.customer_reference gewinnt.
        let sub = json!({ "id": "sub_x", "status": "active",
            "metadata": { "customer_reference": "subwinner" } });
        assert_eq!(
            affected_login_for_billing_refresh("checkout.session.completed", &session, Some(&sub))
                .as_deref(),
            Some("subwinner")
        );
    }

    #[test]
    fn affected_login_none_for_unrelated_and_nonsubscription() {
        assert!(
            affected_login_for_billing_refresh("invoice.payment_succeeded", &json!({}), None)
                .is_none()
        );
        // checkout mit mode != subscription.
        let oneoff = json!({ "mode": "payment", "metadata": { "customer_reference": "x" } });
        assert!(
            affected_login_for_billing_refresh("checkout.session.completed", &oneoff, None)
                .is_none()
        );
        // subscription-Event ohne Login → None.
        assert!(affected_login_for_billing_refresh(
            "customer.subscription.created",
            &json!({ "id": "s", "metadata": {} }),
            None
        )
        .is_none());
    }

    // ── P1.50: bonus_months Annual-Grant ────────────────────────────────────
    #[tokio::test]
    async fn checkout_with_bonus_months_extends_manual_plan_expires_at() {
        let Some(pool) = pool_or_skip("wh_bonus_grant").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id) VALUES ('annuallogin','99')")
            .execute(&pool).await.unwrap();
        // Checkout-Session (mode=subscription) + nachgeladene Subscription mit
        // current_period_end und metadata.bonus_months=2.
        let period_end_epoch = 1_800_000_000_i64; // fester Zukunfts-Epoch
        let session = json!({
            "mode": "subscription", "subscription": "sub_bonus",
            "customer": "cus_b", "client_reference_id": "annuallogin",
            "metadata": { "plan_id": "analysis_dashboard" }
        });
        let retrieved = json!({
            "id": "sub_bonus", "customer": "cus_b", "status": "active",
            "metadata": { "customer_reference": "annuallogin", "plan_id": "analysis_dashboard",
                          "bonus_months": "2" },
            "current_period_end": period_end_epoch,
            "items": { "data": [ { "price": {
                "recurring": { "interval": "year", "interval_count": 1 } } } ] }
        });
        let mut tx = pool.acquire().await.unwrap();
        let action = apply_event(
            &mut tx,
            "evt_bonus",
            "checkout.session.completed",
            &session,
            Some(&retrieved),
        )
        .await
        .unwrap();
        assert_eq!(action, WebhookAction::CheckoutSubscriptionRecorded);
        drop(tx);
        let sync =
            streamer_plan_sync_from_event("checkout.session.completed", &session, Some(&retrieved))
                .expect("checkout sync");
        sync_plan_to_streamer_plans(&pool, &sync).await.unwrap();

        // manual_plan_expires_at = period_end + 2*31 Tage.
        let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT manual_plan_id, manual_plan_expires_at, manual_plan_notes FROM streamer_plans WHERE twitch_user_id='99'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("analysis_dashboard"));
        let expires = DateTime::parse_from_rfc3339(&row.1.expect("expires set")).unwrap();
        let period_end = DateTime::<Utc>::from_timestamp(period_end_epoch, 0).unwrap();
        let expected = period_end + chrono::Duration::days(2 * 31);
        assert_eq!(expires.with_timezone(&Utc), expected);
        assert!(row.2.unwrap().contains("bonus 2mo"));
    }

    #[tokio::test]
    async fn checkout_without_bonus_months_leaves_manual_plan_untouched() {
        let Some(pool) = pool_or_skip("wh_no_bonus").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id) VALUES ('plainlogin','11')")
            .execute(&pool).await.unwrap();
        let session = json!({
            "mode": "subscription", "subscription": "sub_plain",
            "customer": "c", "metadata": { "customer_reference": "plainlogin", "plan_id": "raid_boost" }
        });
        let retrieved = json!({
            "id": "sub_plain", "customer": "c", "status": "active",
            "metadata": { "customer_reference": "plainlogin", "plan_id": "raid_boost" },
            "current_period_end": 1_800_000_000_i64,
            "items": { "data": [ { "price": { "recurring": { "interval": "month", "interval_count": 1 } } } ] }
        });
        let mut tx = pool.acquire().await.unwrap();
        apply_event(
            &mut tx,
            "evt_p",
            "checkout.session.completed",
            &session,
            Some(&retrieved),
        )
        .await
        .unwrap();
        drop(tx);
        let sync =
            streamer_plan_sync_from_event("checkout.session.completed", &session, Some(&retrieved))
                .expect("checkout sync");
        sync_plan_to_streamer_plans(&pool, &sync).await.unwrap();
        // streamer_plans-Row existiert (sync), aber manual_plan_expires_at bleibt NULL.
        let row: (Option<String>,) = sqlx::query_as(
            "SELECT manual_plan_expires_at FROM streamer_plans WHERE twitch_user_id='11'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            row.0.is_none(),
            "ohne bonus_months kein manual_plan_expires_at"
        );
    }

    // ── P2.127/P2.128: Score-Refresh nach Billing-Änderung ──────────────────
    /// Eigenes Schema mit allen Tabellen, die PartnerScoreRefresher liest/schreibt.
    ///
    /// Wichtig: `search_path` wird per `after_connect` auf JEDER Pool-Verbindung
    /// gesetzt — der Refresher nutzt denselben Pool, dessen Verbindungen also alle
    /// auf das Test-Schema zeigen (sonst greift er auf `public` zu).
    async fn refresh_pool_or_skip(schema: &str) -> Option<PgPool> {
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
        let schema_owned = schema.to_string();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |conn, _| {
                let schema = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await
                        .map(|_| ())
                })
            })
            .connect(&dsn)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT, twitch_user_id TEXT, is_partner_active INTEGER NOT NULL DEFAULT 1)",
            "CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, raid_boost_enabled INTEGER NOT NULL DEFAULT 0)",
            "CREATE TABLE twitch_stream_sessions (streamer_login TEXT, started_at TIMESTAMPTZ, duration_seconds BIGINT)",
            "CREATE TABLE twitch_raid_history (from_broadcaster_id TEXT, to_broadcaster_id TEXT, executed_at TIMESTAMPTZ, success BOOLEAN)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT, is_live INTEGER, last_started_at TIMESTAMPTZ)",
            r#"CREATE TABLE twitch_partner_raid_scores (
                   twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
                   avg_duration_sec INTEGER, time_pattern_score_base DOUBLE PRECISION,
                   received_successful_raids_total INTEGER, is_new_partner_preferred INTEGER,
                   new_partner_multiplier DOUBLE PRECISION, raid_boost_multiplier DOUBLE PRECISION,
                   is_live INTEGER, current_started_at TEXT, current_uptime_sec INTEGER,
                   duration_score DOUBLE PRECISION, time_pattern_score DOUBLE PRECISION,
                   readiness_score DOUBLE PRECISION, fairness_score DOUBLE PRECISION,
                   base_score DOUBLE PRECISION, final_score DOUBLE PRECISION,
                   today_received_raids INTEGER, last_computed_at TEXT,
                   internal_sent_raids_30d INTEGER, internal_received_raids_7d INTEGER,
                   internal_received_raids_30d INTEGER,
                   courtesy_score DOUBLE PRECISION DEFAULT 1.0,
                   courtesy_class TEXT,
                   courtesy_observed INTEGER DEFAULT 0
               )"#,
            // Der Score-Refresh faltet die Raid-Etikette mit in den Base-Score.
            r#"CREATE TABLE twitch_raid_courtesy_events (
                   id BIGSERIAL PRIMARY KEY,
                   raid_history_id BIGINT,
                   from_broadcaster_id TEXT NOT NULL,
                   from_broadcaster_login TEXT NOT NULL,
                   to_broadcaster_id TEXT NOT NULL,
                   to_broadcaster_login TEXT NOT NULL,
                   observed_from TIMESTAMPTZ NOT NULL,
                   observed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                   courtesy_class TEXT NOT NULL,
                   message_count INTEGER NOT NULL DEFAULT 0,
                   message_span_sec INTEGER NOT NULL DEFAULT 0,
                   observation_source TEXT,
                   unknown_reason TEXT,
                   whisper_sent BOOLEAN NOT NULL DEFAULT FALSE
               )"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn refresh_recomputes_score_for_known_login() {
        let Some(pool) = refresh_pool_or_skip("wh_score_refresh").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active) VALUES ('boostlogin','555',1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, raid_boost_enabled) VALUES ('555','boostlogin',1)")
            .execute(&pool).await.unwrap();

        refresh_partner_raid_score_for_login(&pool, "BoostLogin")
            .await
            .unwrap();

        let row: (String, f64) = sqlx::query_as(
            "SELECT last_computed_at, raid_boost_multiplier FROM twitch_partner_raid_scores WHERE twitch_user_id='555'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!row.0.is_empty(), "last_computed_at gesetzt");
        // raid_boost_enabled=1 → Multiplikator > 1.0 (Boost greift sofort).
        assert!(
            row.1 > 1.0,
            "raid_boost_multiplier nach Refresh > 1.0, war {}",
            row.1
        );
    }

    #[tokio::test]
    async fn refresh_noop_for_unknown_login() {
        let Some(pool) = refresh_pool_or_skip("wh_score_refresh_unknown").await else {
            return;
        };
        // Kein Eintrag → kein Fehler, keine Score-Zeile.
        refresh_partner_raid_score_for_login(&pool, "gibtsnicht")
            .await
            .unwrap();
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*)::bigint FROM twitch_partner_raid_scores")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }
}

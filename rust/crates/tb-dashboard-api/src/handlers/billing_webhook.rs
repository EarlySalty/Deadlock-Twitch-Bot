//! Nativer Stripe-Webhook-Handler (B2-P0-stripe-webhook).
//!
//! `POST /twitch/api/billing/stripe/webhook` — Stripes Quelle der Wahrheit fürs
//! Bezahlt-Sein (Grillme Block 2A). Port von
//! `bot/dashboard/routes_billing.py:api_billing_stripe_webhook`.
//!
//! Ablauf (Python-Parität):
//! 1. Roher Body (byte-genau) + `Stripe-Signature` →
//!    [`tb_analytics::stripe::verify_signature`]. Ungültige Signatur → 400, KEINE
//!    Verarbeitung.
//! 2. Event-JSON parsen, idempotent gegen `stripe_event_id` deduplizieren
//!    ([`record_event_once`]); bei Replay kein erneutes Anwenden.
//! 3. Subscription-Lifecycle-Events auf `twitch_billing_subscriptions` anwenden
//!    ([`apply_event`]); `streamer_plans` danach best-effort syncen.
//! 4. 200 bei Erfolg (auch bei ignorierten Event-Typen — Stripe darf nicht
//!    retrien), 400 nur bei Signatur-/Parse-Fehler.
//!
//! **Secret:** `STRIPE_WEBHOOK_SECRET` (Infisical/Env), NIE geloggt, nie in
//! Fehlern transportiert. Ohne Config → 503 (wie Python `stripe_webhook_secret_missing`).
//! Die Route wird NATIV (vor dem Strangler-Fallback) registriert → kein 502.

use std::sync::Arc;

use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use sqlx::PgPool;

use tb_analytics::affiliate_commission::{process_commission, InvoicePayment};
use tb_analytics::stripe::webhook_apply::{
    apply_event, record_event_once, streamer_plan_sync_from_event, sync_plan_to_streamer_plans,
};
use tb_analytics::stripe::{verify_signature, StripeClient, DEFAULT_TOLERANCE_SECONDS};

/// Laufzeit-Konfiguration des Webhooks (als Extension injiziert).
///
/// `None` als Extension → Webhook nicht konfiguriert → 503. Der `StripeClient`
/// wird nur für das Nachladen der Subscription bei `checkout.session.completed`
/// gebraucht (Pythons `stripe.Subscription.retrieve`); fehlt er, wird nur der
/// dünne Checkout-Zustand erfasst.
#[derive(Clone)]
pub struct StripeWebhookConfig {
    /// Webhook-Signing-Secret (`whsec_…`). Wird NIE geloggt.
    pub webhook_secret: String,
    /// Optionaler Stripe-Client zum Nachladen der Subscription beim Checkout.
    pub client: Option<Arc<StripeClient>>,
}

/// Baut die Webhook-Config aus der Umgebung (Infisical/Env).
///
/// `STRIPE_WEBHOOK_SECRET` / `TWITCH_BILLING_STRIPE_WEBHOOK_SECRET` (Python-
/// Alias-Reihenfolge). Ohne Webhook-Secret → `None` (Route liefert 503). Der
/// Secret-Key (`STRIPE_SECRET_KEY`) ist optional — fehlt er, bleibt der Client
/// `None` (Checkout-Pfad erfasst dann nur den dünnen Zustand).
pub fn stripe_webhook_config_from_env() -> Option<StripeWebhookConfig> {
    let webhook_secret = non_empty_env(&["STRIPE_WEBHOOK_SECRET", "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET"])?;
    let client = non_empty_env(&["STRIPE_SECRET_KEY", "TWITCH_BILLING_STRIPE_SECRET_KEY"])
        .and_then(|key| StripeClient::new(key).ok())
        .map(Arc::new);
    Some(StripeWebhookConfig {
        webhook_secret,
        client,
    })
}

/// Erster nicht-leerer Env-Wert aus einer Alias-Liste (getrimmt).
fn non_empty_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// `POST /twitch/api/billing/stripe/webhook`
///
/// `body: axum::body::Bytes` als LETZTER Extractor liefert den Body byte-genau
/// (kein Reparsing) — Voraussetzung für die HMAC-Signaturprüfung.
pub async fn stripe_webhook_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<StripeWebhookConfig>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let Some(Extension(config)) = config else {
        // Python: 503 stripe_webhook_secret_missing.
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "stripe_webhook_secret_missing" })),
        )
            .into_response();
    };

    let signature = headers
        .get("Stripe-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string();
    if signature.is_empty() {
        // Python: 400 stripe_signature_missing.
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "stripe_signature_missing" })),
        )
            .into_response();
    }

    // Signatur über den ROHEN Body verifizieren. Ungültig → 400, KEINE DB-Schreibung.
    let now_unix = chrono::Utc::now().timestamp();
    if verify_signature(&body, &signature, &config.webhook_secret, now_unix, DEFAULT_TOLERANCE_SECONDS)
        .is_err()
    {
        // Python: 400 invalid_stripe_signature (generisch, kein Secret-Leak).
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_stripe_signature" })),
        )
            .into_response();
    }

    // Event parsen (nach erfolgreicher Signatur). Parse-Fehler → 400.
    let event: Value = match serde_json::from_slice(&body) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_stripe_payload" })),
            )
                .into_response();
        }
    };

    let event_id = str_field(&event, "id");
    let event_type = str_field(&event, "type");
    let event_object = event
        .get("data")
        .and_then(|d| d.get("object"))
        .cloned()
        .unwrap_or(Value::Null);
    let object_id = str_field(&event_object, "id");
    let livemode = event.get("livemode").and_then(Value::as_bool).unwrap_or(false);
    let payload_text = String::from_utf8_lossy(&body).into_owned();

    match process_event(
        &pool,
        &config,
        &event_id,
        &event_type,
        &event_object,
        &object_id,
        livemode,
        &payload_text,
    )
    .await
    {
        Ok((duplicate, action)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "status": if duplicate { "duplicate" } else { "processed" },
                "event_id": event_id,
                "event_type": event_type,
                "action": action,
            })),
        )
            .into_response(),
        Err(error) => {
            // Python: 500 stripe_webhook_processing_failed. Generisch geloggt.
            tracing::error!(%error, event_type = %event_type, "stripe webhook processing failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "stripe_webhook_processing_failed" })),
            )
                .into_response()
        }
    }
}

/// Transaktionaler Kern: Dedup + (falls neu) Subscription nachladen + anwenden.
/// Gibt `(duplicate, action_str)` zurück.
#[allow(clippy::too_many_arguments)]
async fn process_event(
    pool: &PgPool,
    config: &StripeWebhookConfig,
    event_id: &str,
    event_type: &str,
    event_object: &Value,
    object_id: &str,
    livemode: bool,
    payload_text: &str,
) -> Result<(bool, &'static str), sqlx::Error> {
    // checkout.session.completed: volle Subscription VOR der Transaktion nachladen
    // (HTTP-Call gehört nicht in eine offene DB-Transaktion).
    let retrieved_subscription = maybe_retrieve_subscription(config, event_type, event_object).await;

    let mut tx = pool.begin().await?;
    let is_new = record_event_once(&mut tx, event_id, event_type, object_id, livemode, payload_text).await?;

    let action = if is_new {
        apply_event(
            &mut tx,
            event_id,
            event_type,
            event_object,
            retrieved_subscription.as_ref(),
        )
        .await?
        .as_str()
    } else {
        "duplicate"
    };

    tx.commit().await?;

    // Plan-Sync NACH dem Commit: Dedup + Subscription-State bleiben atomar und
    // ein lokaler streamer_plans-Fehler erzeugt keinen Stripe-Retry.
    if is_new {
        if let Some(sync) =
            streamer_plan_sync_from_event(event_type, event_object, retrieved_subscription.as_ref())
        {
            if let Err(error) = sync_plan_to_streamer_plans(pool, &sync).await {
                tracing::warn!(
                    %error,
                    event_id,
                    event_type,
                    customer_reference = %sync.customer_reference,
                    plan_id = %sync.plan_id,
                    status = %sync.status,
                    "billing webhook streamer plan sync failed"
                );
            }
        }
    }

    // P2.127/P2.128: Partner-Raid-Score-Refresh NACH dem Commit (Python
    // fire-and-forget). Nur für frische Events; der betroffene Login wird aus
    // Event-Typ/-Objekt bzw. der nachgeladenen Subscription abgeleitet. Best-
    // effort — Fehler werden nur geloggt, damit Stripe kein Retry auslöst.
    if is_new {
        if let Some(login) = tb_analytics::stripe::affected_login_for_billing_refresh(
            event_type,
            event_object,
            retrieved_subscription.as_ref(),
        ) {
            if let Err(e) =
                tb_analytics::stripe::refresh_partner_raid_score_for_login(pool, &login).await
            {
                tracing::warn!(%e, "billing webhook raid-score refresh failed");
            }
        }
    }

    // Affiliate-Provision (30 %): NACH dem Commit, mit eigenem Pool/Advisory-Lock
    // (Python `affiliate_mixin._affiliate_process_commission` läuft ebenfalls
    // außerhalb der Webhook-DB-Transaktion). Nur für frische
    // `invoice.payment_succeeded`-Events; bei Replays hat `process_commission`
    // ohnehin seine eigene Idempotenz (UNIQUE auf stripe_event_id). Fehler werden
    // wie in Python geschluckt (Logeintrag), damit Stripe kein Retry auslöst.
    if is_new && event_type.trim() == "invoice.payment_succeeded" {
        if let Some(fields) = InvoiceFields::from_object(event_object) {
            let invoice = InvoicePayment {
                stripe_event_id: event_id,
                stripe_customer_id: &fields.customer,
                amount_paid_cents: fields.amount_paid_cents,
                currency: &fields.currency,
                invoice_id: &fields.invoice_id,
                period_start: &fields.period_start,
                period_end: &fields.period_end,
            };
            match process_commission(pool, config.client.as_deref(), &invoice).await {
                Ok(outcome) => tracing::info!(
                    event_id,
                    outcome = outcome.as_str(),
                    "affiliate commission processed"
                ),
                Err(error) => {
                    tracing::error!(%error, event_id, "affiliate commission processing failed")
                }
            }
        }
    }

    Ok((!is_new, action))
}

/// Aus dem Invoice-Event-Objekt gelöste, besessene Provisions-Felder
/// (Python `_affiliate_process_commission`): `amount_paid`/`currency`/`id`
/// (Invoice) direkt, Abrechnungszeitraum aus `lines.data[0].period.{start,end}`
/// (Unix-Epochs → ISO-8601). Owned, weil [`InvoicePayment`] geliehene Strings
/// erwartet und die ISO-Strings hier erst erzeugt werden.
struct InvoiceFields {
    customer: String,
    amount_paid_cents: i64,
    currency: String,
    invoice_id: String,
    period_start: String,
    period_end: String,
}

impl InvoiceFields {
    /// `None`, wenn kein Customer am Event hängt (ohne Customer keine Zuordnung).
    fn from_object(obj: &Value) -> Option<Self> {
        let customer = str_field(obj, "customer");
        if customer.is_empty() {
            return None;
        }
        let amount_paid_cents = obj.get("amount_paid").and_then(Value::as_i64).unwrap_or(0);
        let currency = {
            let c = str_field(obj, "currency");
            if c.is_empty() {
                "eur".to_string()
            } else {
                c
            }
        };
        // Abrechnungszeitraum aus der ersten Invoice-Line (Python `lines.data[0].period`).
        let period = obj
            .get("lines")
            .and_then(|l| l.get("data"))
            .and_then(Value::as_array)
            .and_then(|d| d.first())
            .and_then(|line| line.get("period"));
        let period_iso = |key: &str| {
            period
                .and_then(|p| p.get(key))
                .and_then(Value::as_i64)
                .and_then(epoch_to_iso)
                .unwrap_or_default()
        };
        Some(Self {
            customer,
            amount_paid_cents,
            currency,
            invoice_id: str_field(obj, "id"),
            period_start: period_iso("start"),
            period_end: period_iso("end"),
        })
    }
}

/// Unix-Epoch (Sekunden) → ISO-8601-String (UTC). `None` bei ungültigem Wert.
fn epoch_to_iso(epoch: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(epoch, 0)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
}

/// Lädt für `checkout.session.completed` (mode=subscription) die volle
/// Subscription via Stripe-API nach (Pythons `stripe.Subscription.retrieve`).
/// Fehler/fehlender Client → `None` (Aufrufer erfasst dann den dünnen Zustand).
async fn maybe_retrieve_subscription(
    config: &StripeWebhookConfig,
    event_type: &str,
    event_object: &Value,
) -> Option<Value> {
    if event_type.trim() != "checkout.session.completed" {
        return None;
    }
    if str_field(event_object, "mode") != "subscription" {
        return None;
    }
    let subscription_id = str_field(event_object, "subscription");
    if subscription_id.is_empty() {
        return None;
    }
    let client = config.client.as_ref()?;
    match client.retrieve_subscription(&subscription_id).await {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(%error, "stripe subscription retrieve fehlgeschlagen — dünner Zustand");
            None
        }
    }
}

/// Getrimmter String an `obj[key]` (sonst `""`).
fn str_field(obj: &Value, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::http::HeaderValue;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    const SECRET: &str = "whsec_test_secret";

    /// Baut einen gültigen `Stripe-Signature`-Header für `payload` zum Timestamp `ts`.
    fn sign(payload: &[u8], ts: i64, secret: &str) -> String {
        let mut signed = ts.to_string().into_bytes();
        signed.push(b'.');
        signed.extend_from_slice(payload);
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(&signed);
        let v1 = hex::encode(mac.finalize().into_bytes());
        format!("t={ts},v1={v1}")
    }

    fn cfg() -> StripeWebhookConfig {
        StripeWebhookConfig {
            webhook_secret: SECRET.to_string(),
            client: None,
        }
    }

    // ── Signatur-Logik (DB-frei: prüft die 400-Pfade vor jedem DB-Zugriff) ───

    #[test]
    fn valid_signature_verifies_over_raw_body() {
        let payload = br#"{"id":"evt_1","type":"customer.subscription.created","data":{"object":{}}}"#;
        let ts = chrono::Utc::now().timestamp();
        let header = sign(payload, ts, SECRET);
        assert!(
            verify_signature(payload, &header, SECRET, ts, DEFAULT_TOLERANCE_SECONDS).is_ok(),
            "frisch signierter Body muss verifizieren"
        );
    }

    #[test]
    fn tampered_body_fails_signature() {
        let payload = br#"{"id":"evt_1"}"#;
        let ts = chrono::Utc::now().timestamp();
        let header = sign(payload, ts, SECRET);
        let tampered = br#"{"id":"evt_2"}"#;
        assert!(
            verify_signature(tampered, &header, SECRET, ts, DEFAULT_TOLERANCE_SECONDS).is_err(),
            "manipulierter Body darf NICHT verifizieren"
        );
    }

    #[test]
    fn config_from_env_requires_webhook_secret() {
        // Ohne gesetzte Env-Variablen → None (Handler liefert 503).
        // (Test-Prozess-Env: die Keys sind hier nicht gesetzt.)
        for key in [
            "STRIPE_WEBHOOK_SECRET",
            "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET",
        ] {
            assert!(
                std::env::var(key).is_err() || std::env::var(key).unwrap().trim().is_empty(),
                "Test setzt voraus, dass {key} im Test-Env leer ist"
            );
        }
        assert!(stripe_webhook_config_from_env().is_none());
    }

    fn headers_with(sig: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(s) = sig {
            h.insert("Stripe-Signature", HeaderValue::from_str(s).unwrap());
        }
        h
    }

    // ── Voll-Handler-Tests (skip ohne TB_TEST_DATABASE_URL) ─────────────────
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_billing_subscriptions (
                   stripe_subscription_id TEXT PRIMARY KEY, stripe_customer_id TEXT,
                   customer_reference TEXT, status TEXT NOT NULL DEFAULT 'unknown', plan_id TEXT,
                   cycle_months INTEGER NOT NULL DEFAULT 1, quantity INTEGER NOT NULL DEFAULT 1,
                   current_period_start TEXT, current_period_end TEXT,
                   cancel_at_period_end INTEGER NOT NULL DEFAULT 0, canceled_at TEXT, ended_at TEXT,
                   last_event_id TEXT, updated_at TEXT NOT NULL
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE streamer_plans (
                   twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
                   plan_name TEXT NOT NULL DEFAULT 'free', expires_at TEXT
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT, twitch_user_id TEXT)"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_billing_events (
                   stripe_event_id TEXT PRIMARY KEY, event_type TEXT NOT NULL, object_id TEXT,
                   received_at TEXT NOT NULL, livemode INTEGER NOT NULL DEFAULT 0, payload TEXT NOT NULL
               )"#,
        ).execute(&pool).await.unwrap();
        // Affiliate-Provisions-Tabellen (für den invoice.payment_succeeded-Hook).
        sqlx::query(
            r#"CREATE TABLE affiliate_streamer_claims (
                   affiliate_twitch_login TEXT, claimed_streamer_login TEXT
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE affiliate_accounts (
                   twitch_login TEXT PRIMARY KEY, stripe_account_id TEXT
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE affiliate_commissions (
                   id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                   affiliate_twitch_login TEXT NOT NULL, streamer_login TEXT NOT NULL,
                   stripe_event_id TEXT UNIQUE NOT NULL, stripe_invoice_id TEXT,
                   stripe_customer_id TEXT, stripe_transfer_id TEXT,
                   brutto_cents INTEGER NOT NULL, commission_cents INTEGER NOT NULL,
                   currency TEXT NOT NULL DEFAULT 'eur', status TEXT NOT NULL DEFAULT 'pending',
                   period_start TEXT, period_end TEXT, created_at TEXT NOT NULL,
                   transferred_at TEXT, error_message TEXT
               )"#,
        ).execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn valid_subscription_created_sets_plan_and_200() {
        let Some(pool) = pool_or_skip("h_sub_created").await else { return };
        sqlx::query("INSERT INTO twitch_streamers_partner_state VALUES ('login','99')")
            .execute(&pool).await.unwrap();
        let payload = br#"{"id":"evt_h1","type":"customer.subscription.created","livemode":false,"data":{"object":{"id":"sub_h","customer":"cus","status":"active","metadata":{"customer_reference":"login","plan_id":"raid_boost"},"items":{"data":[{"price":{"recurring":{"interval":"month","interval_count":1}}}]}}}}"#;
        let ts = chrono::Utc::now().timestamp();
        let resp = stripe_webhook_handler(
            State(pool.clone()),
            Some(Extension(cfg())),
            headers_with(Some(&sign(payload, ts, SECRET))),
            Bytes::from_static(payload),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let plan: (String,) = sqlx::query_as("SELECT plan_name FROM streamer_plans WHERE twitch_user_id='99'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(plan.0, "raid_boost");
    }

    #[tokio::test]
    async fn invalid_signature_returns_400_no_db_write() {
        let Some(pool) = pool_or_skip("h_bad_sig").await else { return };
        let payload = br#"{"id":"evt_bad","type":"customer.subscription.created","data":{"object":{"id":"sub"}}}"#;
        let ts = chrono::Utc::now().timestamp();
        // Mit FALSCHEM Secret signiert → Verifikation schlägt fehl.
        let resp = stripe_webhook_handler(
            State(pool.clone()),
            Some(Extension(cfg())),
            headers_with(Some(&sign(payload, ts, "whsec_wrong"))),
            Bytes::from_static(payload),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // KEINE Event-Zeile geschrieben.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM twitch_billing_events")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 0, "ungültige Signatur darf nichts schreiben");
    }

    #[tokio::test]
    async fn replay_same_event_id_no_double_apply() {
        let Some(pool) = pool_or_skip("h_replay").await else { return };
        sqlx::query("INSERT INTO twitch_streamers_partner_state VALUES ('l','5')")
            .execute(&pool).await.unwrap();
        let payload = br#"{"id":"evt_replay","type":"customer.subscription.created","data":{"object":{"id":"sub_r","customer":"c","status":"active","metadata":{"customer_reference":"l","plan_id":"raid_boost"},"items":{"data":[{"price":{"recurring":{"interval":"month","interval_count":1}}}]}}}}"#;
        let ts = chrono::Utc::now().timestamp();
        let header = sign(payload, ts, SECRET);
        // 1. Mal → processed.
        let r1 = stripe_webhook_handler(State(pool.clone()), Some(Extension(cfg())), headers_with(Some(&header)), Bytes::from_static(payload)).await.into_response();
        assert_eq!(r1.status(), StatusCode::OK);
        // 2. Mal (Replay) → 200, aber duplicate.
        let r2 = stripe_webhook_handler(State(pool.clone()), Some(Extension(cfg())), headers_with(Some(&header)), Bytes::from_static(payload)).await.into_response();
        assert_eq!(r2.status(), StatusCode::OK);
        let body = axum::body::to_bytes(r2.into_body(), usize::MAX).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "duplicate");
        // Genau eine Event-Zeile.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM twitch_billing_events WHERE stripe_event_id='evt_replay'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn unknown_event_type_returns_200_noop() {
        let Some(pool) = pool_or_skip("h_unknown").await else { return };
        let payload = br#"{"id":"evt_u","type":"customer.created","data":{"object":{"id":"x"}}}"#;
        let ts = chrono::Utc::now().timestamp();
        let resp = stripe_webhook_handler(State(pool.clone()), Some(Extension(cfg())), headers_with(Some(&sign(payload, ts, SECRET))), Bytes::from_static(payload)).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["action"], "ignored_unsupported_event");
    }

    #[tokio::test]
    async fn missing_signature_returns_400() {
        let Some(pool) = pool_or_skip("h_nosig").await else { return };
        let payload = br#"{"id":"e","type":"x","data":{"object":{}}}"#;
        let resp = stripe_webhook_handler(State(pool.clone()), Some(Extension(cfg())), headers_with(None), Bytes::from_static(payload)).await.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unconfigured_returns_503() {
        let Some(pool) = pool_or_skip("h_unconfig").await else { return };
        let payload = br#"{}"#;
        let resp = stripe_webhook_handler(State(pool.clone()), None, headers_with(Some("t=1,v1=ab")), Bytes::from_static(payload)).await.into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // ── Affiliate-Provisions-Hook (B2-P1) ────────────────────────────────────

    #[test]
    fn invoice_fields_aus_event_objekt() {
        // amount_paid/currency/id direkt, Zeitraum aus lines.data[0].period.
        let obj: Value = serde_json::json!({
            "id": "in_1",
            "customer": "cus_1",
            "amount_paid": 1000,
            "currency": "eur",
            "lines": { "data": [ { "period": { "start": 1_700_000_000, "end": 1_702_000_000 } } ] }
        });
        let f = InvoiceFields::from_object(&obj).expect("Customer vorhanden → Some");
        assert_eq!(f.customer, "cus_1");
        assert_eq!(f.amount_paid_cents, 1000);
        assert_eq!(f.currency, "eur");
        assert_eq!(f.invoice_id, "in_1");
        assert!(f.period_start.starts_with("2023-"));
        assert!(f.period_end.starts_with("2023-"));
    }

    #[test]
    fn invoice_fields_ohne_customer_ist_none() {
        let obj: Value = serde_json::json!({ "id": "in_x", "amount_paid": 500 });
        assert!(InvoiceFields::from_object(&obj).is_none());
    }

    #[tokio::test]
    async fn invoice_payment_verbucht_affiliate_provision() {
        let Some(pool) = pool_or_skip("h_invoice_commission").await else { return };
        // Abo verknüpft Customer→Streamer, Streamer ist von einem Affiliate geworben.
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, stripe_customer_id, customer_reference, updated_at) VALUES ('sub_c','cus_c','StreamerX','now')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_streamer_login) VALUES ('aff1','streamerx')")
            .execute(&pool).await.unwrap();
        let payload = br#"{"id":"evt_inv","type":"invoice.payment_succeeded","livemode":false,"data":{"object":{"id":"in_42","customer":"cus_c","subscription":"sub_c","amount_paid":1000,"currency":"eur","lines":{"data":[{"period":{"start":1700000000,"end":1702000000}}]}}}}"#;
        let ts = chrono::Utc::now().timestamp();
        let resp = stripe_webhook_handler(
            State(pool.clone()),
            Some(Extension(cfg())),
            headers_with(Some(&sign(payload, ts, SECRET))),
            Bytes::from_static(payload),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        // 30 % von 1000 = 300, pending (kein Connect-Konto). `commission_cents` ist
        // INTEGER (INT4, echtes Schema) → als i32 dekodieren.
        let (status, commission): (String, i32) = sqlx::query_as(
            "SELECT status, commission_cents FROM affiliate_commissions WHERE stripe_event_id='evt_inv'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "pending");
        assert_eq!(commission, 300);
    }

    #[tokio::test]
    async fn invoice_replay_verbucht_keine_doppelte_provision() {
        let Some(pool) = pool_or_skip("h_invoice_replay_commission").await else { return };
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, stripe_customer_id, customer_reference, updated_at) VALUES ('sub_r','cus_r','StreamerY','now')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_streamer_login) VALUES ('aff2','streamery')")
            .execute(&pool).await.unwrap();
        let payload = br#"{"id":"evt_inv_r","type":"invoice.payment_succeeded","livemode":false,"data":{"object":{"id":"in_r","customer":"cus_r","subscription":"sub_r","amount_paid":1000,"currency":"eur","lines":{"data":[{"period":{"start":1700000000,"end":1702000000}}]}}}}"#;
        let ts = chrono::Utc::now().timestamp();
        let header = sign(payload, ts, SECRET);
        let r1 = stripe_webhook_handler(State(pool.clone()), Some(Extension(cfg())), headers_with(Some(&header)), Bytes::from_static(payload)).await.into_response();
        assert_eq!(r1.status(), StatusCode::OK);
        // Replay: Webhook-Dedup verhindert den 2. process_commission-Aufruf;
        // selbst ohne das wäre die UNIQUE auf stripe_event_id idempotent.
        let r2 = stripe_webhook_handler(State(pool.clone()), Some(Extension(cfg())), headers_with(Some(&header)), Bytes::from_static(payload)).await.into_response();
        assert_eq!(r2.status(), StatusCode::OK);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_commissions WHERE stripe_event_id='evt_inv_r'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "genau eine Provisions-Zeile trotz Replay");
    }
}

//! Nativer Abo-/Billing-Bezahlpfad (Block 2A, Stripe-hosted).
//!
//! Port von `bot/dashboard/routes_billing.py` + `abbo_billing_routes.py`. Deckt
//! den umsatzkritischen, NATIV (vor dem Strangler-Fallback) registrierten Teil ab:
//!
//! - `GET /twitch/abbo` (+ `/twitch/abo`, `/twitch/abos`) → 301 auf
//!   `/twitch/pricing` (Pythons `pricing_redirect`). Die eigentliche Abo-Status-/
//!   Verwaltungs-Seite ist die `/twitch/pricing`-SPA; sie liest ihren Zustand aus
//!   dem Katalog-JSON (s. u.), das den aktuellen Plan + buchbaren Katalog liefert.
//! - `GET /twitch/abbo/bezahlen` → erstellt eine **Stripe-Checkout-Session** über
//!   den nativen [`StripeClient`] und redirected (302) zur Stripe-hosted Checkout-
//!   URL. KEIN eigener Bezahl-Flow.
//! - `GET|POST /twitch/abbo/kündigen` → Stripe-Customer-Portal-Link (falls
//!   Customer-ID vorhanden), sonst Fallback `cancel_at_period_end` via
//!   [`StripeClient`]. Beides Stripe-hosted; keine eigene Kündigungs-Engine.
//! - `GET /twitch/api/billing/catalog` (+ `/twitch/api/v2/billing/catalog`) →
//!   Plan-Katalog als JSON inkl. `current_subscription` + Stripe-Price-Verfügbarkeit.
//! - `GET /twitch/api/billing/readiness` → Stripe-Readiness als JSON (keine Secrets).
//!
//! **Stripe-hosted (DROP eigene Engine, Grillme 2A):** Rechnungen/Invoices werden
//! NICHT selbst gerendert. Die Kündigung führt — wenn möglich — ins Stripe-
//! Customer-Portal (hosted Invoices/Rechnungsdownload dort). Affiliate-Provision
//! ist nicht Teil dieses Pfades (Block 2B).
//!
//! **Auth:** Nur der eingeloggte Partner (oder Admin/Localhost) sieht/verwaltet
//! sein eigenes Abo. Der [`DashboardAuthLevel`]-Extractor liefert Login + User-ID
//! aus der Server-Session; nicht authentifiziert → Redirect auf den Login bzw.
//! 401 bei den JSON-APIs (Python-Parität: `_check_v2_auth` / `auth_required`).

use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use tb_analytics::billing::{
    catalog_json, find_plan, is_paid_plan_id, normalize_billing_cycle, price_id_default,
};
use tb_analytics::plan::resolve_plan_snapshot;
use tb_analytics::stripe::StripeClient;

use crate::auth::level::DashboardAuthLevel;

/// Default-Public-Origin für success/cancel/return-URLs, wenn keine konfiguriert
/// ist (Pythons letzter `_billing_configured_public_origin`-Fallback).
const DEFAULT_PUBLIC_ORIGIN: &str = "https://admin.deutsche-deadlock-community.de";

/// Laufzeit-Konfiguration des nativen Bezahlpfades (als Extension injiziert).
///
/// `None` als Extension → Stripe nicht konfiguriert → Checkout/Cancel leiten auf
/// die Pricing-Seite mit `reason=...` um (Python-Parität: kein 500, sondern
/// Redirect mit Grund). Die JSON-APIs (Katalog/Readiness) funktionieren auch ohne
/// Config (sie melden dann `checkout_ready=false`).
#[derive(Clone)]
pub struct BillingPageConfig {
    /// Stripe-Client (Secret-Key intern, nie geloggt).
    pub client: Arc<StripeClient>,
    /// Öffentlicher Origin für success/cancel/return-URLs (z. B.
    /// `https://admin.deutsche-deadlock-community.de`), ohne Trailing-Slash.
    pub public_origin: String,
}

/// Baut die Billing-Page-Config aus der Umgebung (Infisical/Env).
///
/// Braucht `STRIPE_SECRET_KEY` (oder Alias) für den Client. Der Public-Origin
/// wird aus `STRIPE_CHECKOUT_SUCCESS_URL`/`STRIPE_CHECKOUT_CANCEL_URL` (Origin-
/// Teil) oder `TWITCH_ADMIN_PUBLIC_URL`/`MASTER_DASHBOARD_PUBLIC_URL` abgeleitet,
/// sonst der Default. Ohne Secret-Key → `None` (Checkout/Cancel redirecten dann
/// mit `reason=stripe_secret_key_missing`).
pub fn billing_page_config_from_env() -> Option<BillingPageConfig> {
    let secret = non_empty_env(&["STRIPE_SECRET_KEY", "TWITCH_BILLING_STRIPE_SECRET_KEY"])?;
    let client = StripeClient::new(secret).ok()?;
    let public_origin = resolve_public_origin();
    Some(BillingPageConfig {
        client: Arc::new(client),
        public_origin,
    })
}

/// Leitet den Public-Origin aus den konfigurierten URLs ab (Origin-Teil) oder
/// fällt auf den Default zurück. Spiegelt `_billing_configured_public_origin`.
fn resolve_public_origin() -> String {
    let from_url = |key: &str| -> Option<String> {
        std::env::var(key).ok().and_then(|raw| origin_of(&raw))
    };
    from_url("STRIPE_CHECKOUT_SUCCESS_URL")
        .or_else(|| from_url("STRIPE_CHECKOUT_CANCEL_URL"))
        .or_else(|| from_url("TWITCH_BILLING_CHECKOUT_SUCCESS_URL"))
        .or_else(|| from_url("TWITCH_BILLING_CHECKOUT_CANCEL_URL"))
        .or_else(|| non_empty_env(&["TWITCH_ADMIN_PUBLIC_URL"]).and_then(|u| origin_of(&u)))
        .or_else(|| non_empty_env(&["MASTER_DASHBOARD_PUBLIC_URL"]).and_then(|u| origin_of(&u)))
        .unwrap_or_else(|| DEFAULT_PUBLIC_ORIGIN.to_string())
}

/// Extrahiert `scheme://host[:port]` aus einer URL (ohne Trailing-Slash).
fn origin_of(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw.trim()).ok()?;
    let host = url.host_str()?;
    let scheme = url.scheme();
    let origin = match url.port() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    Some(origin.trim_end_matches('/').to_string())
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

// ── /twitch/abbo → /twitch/pricing ──────────────────────────────────────────

/// `GET /twitch/abbo` (+ `/twitch/abo`, `/twitch/abos`).
///
/// 301 auf `/twitch/pricing` (Query-String wird durchgereicht). Port von
/// `routes_billing.py:pricing_redirect`.
pub async fn abbo_redirect_handler(raw_query: axum::extract::RawQuery) -> Response {
    let target = match raw_query.0.filter(|q| !q.is_empty()) {
        Some(qs) => format!("/twitch/pricing?{qs}"),
        None => "/twitch/pricing".to_string(),
    };
    Redirect::permanent(&target).into_response()
}

// ── Checkout-Start ──────────────────────────────────────────────────────────

/// Query-Parameter des Checkout-Start-Links (`GET /twitch/abbo/bezahlen`).
#[derive(Debug, Deserialize, Default)]
pub struct CheckoutQuery {
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub cycle: Option<String>,
    #[serde(default)]
    pub quantity: Option<String>,
}

/// `GET /twitch/abbo/bezahlen` — erstellt eine Stripe-Checkout-Session und
/// redirected (302) zur hosted Checkout-URL.
///
/// Port von `abbo_billing_routes.py:abbo_pay`. Fehlerfälle leiten — wie Python —
/// auf `/twitch/pricing` (bzw. `/twitch/abbo`) mit `reason=...` um, statt 4xx/5xx
/// zu liefern. Nicht eingeloggt → Login-Redirect.
pub async fn checkout_start_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
    Query(params): Query<CheckoutQuery>,
) -> Response {
    // Auth-Gate: nur eingeloggter Partner/Admin/Localhost.
    let Some(reference) = customer_reference_for(&auth) else {
        return Redirect::to("/twitch/auth/login?next=%2Ftwitch%2Fpricing").into_response();
    };

    let plan_id = params.plan_id.unwrap_or_default();
    let plan_id = plan_id.trim();
    if plan_id.is_empty() {
        return Redirect::to("/twitch/pricing").into_response();
    }
    let cycle = normalize_billing_cycle(parse_u32(params.cycle.as_deref(), 1));

    // Plan muss existieren und kostenpflichtig sein (free → kein Stripe-Price).
    let Some(_plan) = find_plan(plan_id) else {
        return Redirect::to("/twitch/pricing").into_response();
    };
    if !is_paid_plan_id(plan_id) {
        return Redirect::to("/twitch/pricing").into_response();
    }

    let quantity = parse_u32(params.quantity.as_deref(), 1).clamp(1, 24);

    // Stripe muss konfiguriert sein (Client) und eine Price-ID für Plan/Zyklus haben.
    let Some(Extension(config)) = config else {
        return pricing_unavailable("stripe_secret_key_missing");
    };
    let Some(price_id) = price_id_default(plan_id, cycle) else {
        return pricing_unavailable("missing_stripe_price_id");
    };

    let base_url = config.public_origin.trim_end_matches('/');
    let success_url =
        format!("{base_url}/twitch/pricing?checkout=success&session_id={{CHECKOUT_SESSION_ID}}");
    let cancel_url = format!("{base_url}/twitch/pricing?checkout=cancelled");

    // Session-Payload (Python-Parität: subscription mode, hosted Checkout,
    // Steuer-/Adress-Erfassung, Metadaten für den Webhook-Sync).
    let mut session_payload = json!({
        "mode": "subscription",
        "success_url": success_url,
        "cancel_url": cancel_url,
        "line_items": [{ "price": price_id, "quantity": quantity }],
        "billing_address_collection": "required",
        "tax_id_collection": { "enabled": true },
        "consent_collection": { "terms_of_service": "required" },
        "client_reference_id": reference,
        "metadata": {
            "plan_id": plan_id,
            "cycle_months": cycle.to_string(),
            "quantity": quantity.to_string(),
            "source": "abbo_page_pay_link",
            "customer_reference": reference,
        },
    });

    // 30-Tage-Trial nur für analysis_dashboard im Monatszyklus; +2 Bonus-Monate
    // beim Jahreszyklus (vom Webhook via subscription_data.metadata verarbeitet).
    if plan_id == "analysis_dashboard" && cycle == 1 {
        session_payload["subscription_data"] = json!({ "trial_period_days": 30 });
    }
    if cycle == 12 {
        // Auf bestehende subscription_data mergen (für Jahreszyklus ist sie leer,
        // da der Trial nur bei Monatszyklus greift — Python-Parität).
        let mut sub_data = session_payload
            .get("subscription_data")
            .cloned()
            .unwrap_or_else(|| json!({}));
        sub_data["metadata"] = json!({ "bonus_months": "2" });
        session_payload["subscription_data"] = sub_data;
    }

    match config
        .client
        .create_checkout_session(&session_payload, None)
        .await
    {
        Ok(session) => {
            let url = session
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if url.is_empty() {
                pricing_unavailable("checkout_missing_url")
            } else {
                // 302 auf die Stripe-hosted Checkout-URL.
                Redirect::to(&url).into_response()
            }
        }
        Err(error) => {
            // Generisch geloggt (kein Secret-Leak — StripeError trägt keine Header).
            tracing::warn!(%error, "billing checkout redirect failed");
            pricing_unavailable("checkout_create_failed")
        }
    }
}

// ── Kündigen ────────────────────────────────────────────────────────────────

/// `GET|POST /twitch/abbo/kündigen` — Kündigung via Stripe-Customer-Portal,
/// Fallback `cancel_at_period_end`.
///
/// Port von `abbo_billing_routes.py:abbo_cancel`. Ermittelt die Stripe-Customer-/
/// Subscription-ID des eingeloggten Partners aus `twitch_billing_subscriptions`.
/// Hat der Customer eine Portal-Session → Redirect dorthin (Stripe-hosted, deckt
/// Kündigung + Rechnungen ab). Sonst direkter `cancel_at_period_end`-Fallback.
pub async fn cancel_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
    State(pool): State<PgPool>,
) -> Response {
    let Some(reference) = customer_reference_for(&auth) else {
        return Redirect::to("/twitch/auth/login?next=%2Ftwitch%2Fpricing").into_response();
    };
    let (twitch_login, twitch_user_id) = login_and_user_id(&auth);

    let record = match active_customer_record(&pool, &twitch_login, &twitch_user_id, &reference).await
    {
        Ok(rec) => rec,
        Err(error) => {
            tracing::error!(%error, "billing cancel: customer lookup failed");
            return Redirect::to("/twitch/pricing?cancel=error").into_response();
        }
    };
    let Some(record) = record else {
        return Redirect::to("/twitch/pricing?cancel=missing").into_response();
    };

    let Some(Extension(config)) = config else {
        return Redirect::to("/twitch/pricing?cancel=error").into_response();
    };

    let base_url = config.public_origin.trim_end_matches('/');

    // 1) Customer-Portal (Stripe-hosted) bevorzugen.
    if !record.stripe_customer_id.is_empty() {
        let return_url = format!("{base_url}/twitch/abbo?cancel=returned");
        match config
            .client
            .create_billing_portal_session(&record.stripe_customer_id, &return_url)
            .await
        {
            Ok(portal) => {
                let url = portal
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !url.is_empty() {
                    return Redirect::to(&url).into_response();
                }
            }
            Err(error) => {
                tracing::debug!(%error, "billing portal unavailable; trying direct cancel");
            }
        }
    }

    // 2) Fallback: cancel_at_period_end direkt setzen.
    if record.stripe_subscription_id.is_empty() {
        return Redirect::to("/twitch/pricing?cancel=missing").into_response();
    }
    match config
        .client
        .cancel_subscription_at_period_end(&record.stripe_subscription_id)
        .await
    {
        Ok(_) => Redirect::to("/twitch/pricing?cancel=scheduled").into_response(),
        Err(error) => {
            tracing::error!(%error, "billing cancel fallback failed");
            Redirect::to("/twitch/pricing?cancel=error").into_response()
        }
    }
}

// ── Katalog (JSON) ──────────────────────────────────────────────────────────

/// Query-Parameter des Katalog-Endpunkts.
#[derive(Debug, Deserialize, Default)]
pub struct CatalogQuery {
    #[serde(default)]
    pub cycle: Option<String>,
}

/// `GET /twitch/api/billing/catalog` (+ `/twitch/api/v2/billing/catalog`).
///
/// Plan-Katalog als JSON + Stripe-Price-Verfügbarkeit + aktueller Plan des
/// eingeloggten Partners. Port von `routes_billing.py:api_billing_catalog`.
/// Nicht authentifiziert → 401 `auth_required`.
pub async fn catalog_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
    State(pool): State<PgPool>,
    Query(params): Query<CatalogQuery>,
) -> Response {
    if !auth.is_authenticated() {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "auth_required" }))).into_response();
    }

    let cycle = normalize_billing_cycle(parse_u32(params.cycle.as_deref(), 1));
    let mut payload = catalog_json(cycle);
    let readiness = readiness_payload(config.as_ref().map(|Extension(c)| c));
    let checkout_ready = readiness["checkout_ready"].as_bool().unwrap_or(false);

    // Aktuellen Plan auflösen (Partner: eigener; Admin/Localhost: kein Partner-
    // Kontext → raid_free-Default).
    let (twitch_login, twitch_user_id) = login_and_user_id(&auth);
    let current = resolve_plan_snapshot(&pool, &twitch_login, &twitch_user_id)
        .await
        .unwrap_or_else(|_| {
            // Fail-safe: raid_free. resolve_plan_snapshot liefert das ohnehin als
            // Default; nur DB-Fehler landen hier.
            tb_analytics::plan::PlanSnapshot {
                plan_id: "raid_free",
                plan_name: "Free",
                tier: "free",
                is_extended: false,
                entitlements: vec!["analytics.daily"],
                expires_at: None,
                source: "default_basic",
            }
        });
    let current_plan_id = current.plan_id;

    // Pro Plan: is_current + Stripe-Price-Verfügbarkeit annotieren.
    if let Some(plans) = payload.get_mut("plans").and_then(Value::as_array_mut) {
        for plan in plans.iter_mut() {
            let pid = plan.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            plan["is_current"] = json!(pid == current_plan_id);
            if !is_paid_plan_id(&pid) {
                plan["checkout_available"] = json!(false);
                plan["stripe_price_id"] = Value::Null;
                continue;
            }
            let price_id = price_id_default(&pid, cycle);
            plan["stripe_price_id"] = match price_id {
                Some(id) => json!(id),
                None => Value::Null,
            };
            plan["checkout_available"] = json!(price_id.is_some() && checkout_ready);
        }
    }

    payload["current_subscription"] = json!({
        "plan_id": current.plan_id,
        "plan_name": current.plan_name,
        "tier": current.tier,
        "is_extended": current.is_extended,
        "entitlements": current.entitlements,
        "expires_at": current.expires_at,
        "source": current.source,
    });
    payload["payment"] = json!({
        "provider": "stripe",
        "catalog_path": "/twitch/api/billing/catalog",
        "readiness_path": "/twitch/api/billing/readiness",
        "checkout_path": "/twitch/abbo/bezahlen",
        "cancel_path": "/twitch/abbo/kündigen",
        "webhook_path": "/twitch/api/billing/stripe/webhook",
        "checkout_ready": checkout_ready,
    });

    Json(payload).into_response()
}

// ── Readiness (JSON) ────────────────────────────────────────────────────────

/// `GET /twitch/api/billing/readiness`.
///
/// Stripe-Readiness als JSON (keine Secrets). Port von
/// `routes_billing.py:api_billing_readiness`. Nicht authentifiziert → 401.
pub async fn readiness_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
) -> Response {
    if !auth.is_authenticated() {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "auth_required" }))).into_response();
    }
    Json(readiness_payload(config.as_ref().map(|Extension(c)| c))).into_response()
}

/// Baut die Readiness-Payload (Teilmenge von Pythons `_billing_stripe_readiness_payload`).
///
/// Bewusst KEINE Secret-Previews — der native Pfad liest Secrets aus Infisical
/// und gibt sie nie aus. `checkout_ready` = Stripe-Client konfiguriert;
/// `price_map_ready` = eingecheckte Price-ID-Defaults decken alle bezahlten
/// Pläne ab (sie tun es per Konstruktion, s. catalog::PRICE_ID_DEFAULTS).
fn readiness_payload(config: Option<&BillingPageConfig>) -> Value {
    let checkout_ready = config.is_some();
    let webhook_ready = non_empty_env(&[
        "STRIPE_WEBHOOK_SECRET",
        "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET",
    ])
    .is_some();
    // Eingecheckte Defaults decken alle bezahlten Pläne × {1,12} ab.
    let price_map_ready = true;
    json!({
        "provider": "stripe",
        "integration_state": if checkout_ready && price_map_ready { "live" } else { "planned" },
        "checkout_ready": checkout_ready,
        "webhook_ready": webhook_ready,
        "price_map_ready": price_map_ready,
        "ready_for_live": checkout_ready && webhook_ready && price_map_ready,
    })
}

// ── Hilfsfunktionen ─────────────────────────────────────────────────────────

/// Customer-Reference des eingeloggten Nutzers (Login bevorzugt, sonst User-ID).
/// `None`, wenn nicht authentifiziert.
fn customer_reference_for(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
        } => {
            let login = twitch_login.trim();
            if !login.is_empty() {
                Some(login.to_string())
            } else {
                let uid = twitch_user_id.trim();
                (!uid.is_empty()).then(|| uid.to_string())
            }
        }
        // Admin/Localhost haben keinen Partner-Kontext → kein eigenes Abo.
        DashboardAuthLevel::Admin | DashboardAuthLevel::Localhost => None,
        DashboardAuthLevel::None => None,
    }
}

/// `(twitch_login, twitch_user_id)` des eingeloggten Partners (sonst leer).
fn login_and_user_id(auth: &DashboardAuthLevel) -> (String, String) {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
        } => (twitch_login.clone(), twitch_user_id.clone()),
        _ => (String::new(), String::new()),
    }
}

/// Parst einen `u32` aus einem optionalen String, sonst Default.
fn parse_u32(raw: Option<&str>, default: u32) -> u32 {
    raw.and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(default)
}

/// Redirect auf die Pricing-Seite mit `checkout=unavailable&reason=...`.
fn pricing_unavailable(reason: &str) -> Response {
    Redirect::to(&format!("/twitch/pricing?checkout=unavailable&reason={reason}")).into_response()
}

/// Aktiver Stripe-Customer-/Subscription-Record des Nutzers.
#[derive(Debug, Default, Clone, sqlx::FromRow)]
struct CustomerRecord {
    stripe_customer_id: String,
    stripe_subscription_id: String,
}

/// Liest die beste (aktiv-ähnliche) Abo-Zeile für die Refs des Nutzers aus
/// `twitch_billing_subscriptions`. Port von
/// `_billing_customer_record_for_request`: matcht Login ODER User-ID ODER die
/// primäre Reference; priorisiert active/trialing/past_due, dann jüngstes Update.
async fn active_customer_record(
    pool: &PgPool,
    twitch_login: &str,
    twitch_user_id: &str,
    reference: &str,
) -> Result<Option<CustomerRecord>, sqlx::Error> {
    let login = twitch_login.trim().to_lowercase();
    let user_id = twitch_user_id.trim().to_lowercase();
    let reference = reference.trim().to_lowercase();

    let row: Option<CustomerRecord> = sqlx::query_as(
        r#"
        SELECT
            COALESCE(stripe_customer_id, '')     AS stripe_customer_id,
            COALESCE(stripe_subscription_id, '') AS stripe_subscription_id
        FROM twitch_billing_subscriptions
        WHERE (
                  LOWER(COALESCE(customer_reference, '')) = $1
               OR ($2 <> '' AND LOWER(COALESCE(customer_reference, '')) = $2)
               OR ($3 <> '' AND LOWER(COALESCE(customer_reference, '')) = $3)
              )
          AND TRIM(COALESCE(stripe_customer_id, '') || COALESCE(stripe_subscription_id, '')) <> ''
        ORDER BY
            CASE WHEN status IN ('active', 'trialing', 'past_due') THEN 0 ELSE 1 END,
            updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(&login)
    .bind(&user_id)
    .bind(&reference)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partner(login: &str, uid: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: uid.to_string(),
        }
    }

    // ── Reine Logik (DB-/HTTP-frei) ─────────────────────────────────────────

    #[test]
    fn origin_of_strips_path_and_trailing_slash() {
        assert_eq!(
            origin_of("https://admin.example.com/twitch/pricing?x=1").as_deref(),
            Some("https://admin.example.com")
        );
        assert_eq!(
            origin_of("http://127.0.0.1:8767/").as_deref(),
            Some("http://127.0.0.1:8767")
        );
        assert_eq!(origin_of("not a url"), None);
    }

    #[test]
    fn customer_reference_prefers_login() {
        assert_eq!(customer_reference_for(&partner("Streamer", "42")).as_deref(), Some("Streamer"));
        assert_eq!(customer_reference_for(&partner("  ", "42")).as_deref(), Some("42"));
        assert_eq!(customer_reference_for(&partner("", "")), None);
        assert_eq!(customer_reference_for(&DashboardAuthLevel::Admin), None);
        assert_eq!(customer_reference_for(&DashboardAuthLevel::None), None);
    }

    #[test]
    fn parse_u32_handles_garbage() {
        assert_eq!(parse_u32(Some("3"), 1), 3);
        assert_eq!(parse_u32(Some(" 12 "), 1), 12);
        assert_eq!(parse_u32(Some("abc"), 1), 1);
        assert_eq!(parse_u32(None, 1), 1);
    }

    #[test]
    fn readiness_without_config_is_not_checkout_ready() {
        let payload = readiness_payload(None);
        assert_eq!(payload["provider"], "stripe");
        assert_eq!(payload["checkout_ready"], false);
        assert_eq!(payload["price_map_ready"], true);
        assert_eq!(payload["integration_state"], "planned");
        assert_eq!(payload["ready_for_live"], false);
    }

    #[tokio::test]
    async fn abbo_redirect_keeps_query_string() {
        let resp = abbo_redirect_handler(axum::extract::RawQuery(Some("cycle=12".to_string()))).await;
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing?cycle=12");
    }

    #[tokio::test]
    async fn abbo_redirect_without_query() {
        let resp = abbo_redirect_handler(axum::extract::RawQuery(None)).await;
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing");
    }

    // ── Checkout (wiremock — Stripe-HTTP, KEINE echten Secrets) ─────────────

    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg_with_base(base: &str) -> BillingPageConfig {
        BillingPageConfig {
            client: Arc::new(StripeClient::new("sk_test_dummy").unwrap().with_api_base(base)),
            public_origin: "https://admin.example.test".to_string(),
        }
    }

    /// Unauth → Login-Redirect, KEIN Stripe-Call.
    #[tokio::test]
    async fn checkout_unauthenticated_redirects_to_login() {
        let resp = checkout_start_handler(
            DashboardAuthLevel::None,
            None,
            Query(CheckoutQuery {
                plan_id: Some("raid_boost".into()),
                cycle: Some("1".into()),
                quantity: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.contains("/twitch/auth/login"), "got {loc}");
    }

    /// Eingeloggt + gültiger bezahlter Plan → Stripe-Checkout-Session wird
    /// erstellt UND auf die hosted URL redirected (302).
    #[tokio::test]
    async fn checkout_creates_session_and_redirects_to_hosted_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/checkout/sessions"))
            // Beleg: Plan/Cycle landen in den Metadaten der Session.
            .and(body_string_contains("subscription"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cs_test_abc",
                "url": "https://checkout.stripe.com/c/pay/cs_test_abc",
            })))
            .mount(&server)
            .await;

        let resp = checkout_start_handler(
            partner("streamerlogin", "42"),
            Some(Extension(cfg_with_base(&server.uri()))),
            Query(CheckoutQuery {
                plan_id: Some("raid_boost".into()),
                cycle: Some("1".into()),
                quantity: Some("1".into()),
            }),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "https://checkout.stripe.com/c/pay/cs_test_abc");
    }

    /// Free-Plan (raid_free) → kein Stripe-Price → Redirect auf Pricing, KEIN Call.
    #[tokio::test]
    async fn checkout_free_plan_redirects_to_pricing() {
        let resp = checkout_start_handler(
            partner("login", "1"),
            Some(Extension(cfg_with_base("http://unused.invalid"))),
            Query(CheckoutQuery {
                plan_id: Some("raid_free".into()),
                cycle: Some("1".into()),
                quantity: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing");
    }

    /// Stripe nicht konfiguriert (keine Config) → Redirect mit reason, KEIN 500.
    #[tokio::test]
    async fn checkout_without_config_redirects_with_reason() {
        let resp = checkout_start_handler(
            partner("login", "1"),
            None,
            Query(CheckoutQuery {
                plan_id: Some("raid_boost".into()),
                cycle: Some("1".into()),
                quantity: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.contains("reason=stripe_secret_key_missing"), "got {loc}");
    }

    /// Stripe-API-Fehler beim Erstellen → Redirect mit reason=checkout_create_failed.
    #[tokio::test]
    async fn checkout_stripe_error_redirects_with_reason() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/checkout/sessions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {"type": "invalid_request_error"}
            })))
            .mount(&server)
            .await;
        let resp = checkout_start_handler(
            partner("login", "1"),
            Some(Extension(cfg_with_base(&server.uri()))),
            Query(CheckoutQuery {
                plan_id: Some("raid_boost".into()),
                cycle: Some("1".into()),
                quantity: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.contains("reason=checkout_create_failed"), "got {loc}");
    }

    // ── Cancel + Katalog (DB — skip ohne TB_TEST_DATABASE_URL) ──────────────

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
                   plan_name TEXT NOT NULL DEFAULT 'free', expires_at TEXT,
                   manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_updated_at TEXT,
                   trial_ever_granted INTEGER DEFAULT 0, first_login_at TEXT
               )"#,
        ).execute(&pool).await.unwrap();
        Some(pool)
    }

    /// Cancel: keine aktive Subscription → Redirect cancel=missing.
    #[tokio::test]
    async fn cancel_without_subscription_redirects_missing() {
        let Some(pool) = pool_or_skip("bp_cancel_missing").await else { return };
        let server = MockServer::start().await;
        let resp = cancel_handler(
            partner("nobody", "9"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool.clone()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing?cancel=missing");
    }

    /// Cancel-Fallback: Subscription ohne Customer → cancel_at_period_end gesetzt,
    /// Redirect cancel=scheduled.
    #[tokio::test]
    async fn cancel_fallback_sets_cancel_at_period_end() {
        let Some(pool) = pool_or_skip("bp_cancel_fallback").await else { return };
        sqlx::query(
            "INSERT INTO twitch_billing_subscriptions \
             (stripe_subscription_id, stripe_customer_id, customer_reference, status, updated_at) \
             VALUES ('sub_x', '', 'login', 'active', '2026-06-01T00:00:00+00:00')",
        )
        .execute(&pool).await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/sub_x"))
            .and(body_string_contains("cancel_at_period_end"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sub_x", "cancel_at_period_end": true
            })))
            .mount(&server)
            .await;

        let resp = cancel_handler(
            partner("login", "5"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool.clone()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing?cancel=scheduled");
    }

    /// Cancel mit Customer-ID → Portal-Session, Redirect zur hosted Portal-URL.
    #[tokio::test]
    async fn cancel_with_customer_uses_portal() {
        let Some(pool) = pool_or_skip("bp_cancel_portal").await else { return };
        sqlx::query(
            "INSERT INTO twitch_billing_subscriptions \
             (stripe_subscription_id, stripe_customer_id, customer_reference, status, updated_at) \
             VALUES ('sub_p', 'cus_p', 'login', 'active', '2026-06-01T00:00:00+00:00')",
        )
        .execute(&pool).await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/billing_portal/sessions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "bps_1", "url": "https://billing.stripe.com/p/session/bps_1"
            })))
            .mount(&server)
            .await;

        let resp = cancel_handler(
            partner("login", "5"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool.clone()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "https://billing.stripe.com/p/session/bps_1");
    }

    /// Katalog: eingeloggt → 200 mit Plan-Status (KEIN 502/Proxy). Aktueller Plan
    /// wird aufgelöst; bezahlte Pläne tragen checkout_available + stripe_price_id.
    #[tokio::test]
    async fn catalog_returns_plan_status_not_proxy() {
        let Some(pool) = pool_or_skip("bp_catalog").await else { return };
        let server = MockServer::start().await;
        let resp = catalog_handler(
            partner("login", "5"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool.clone()),
            Query(CatalogQuery { cycle: Some("1".into()) }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["currency"], "EUR");
        let plans = v["plans"].as_array().unwrap();
        assert_eq!(plans.len(), 8);
        // raid_free = aktueller Default-Plan.
        assert_eq!(v["current_subscription"]["plan_id"], "raid_free");
        let raid_free = plans.iter().find(|p| p["id"] == "raid_free").unwrap();
        assert_eq!(raid_free["is_current"], true);
        assert_eq!(raid_free["checkout_available"], false);
        assert!(raid_free["stripe_price_id"].is_null());
        // Bezahlter Plan trägt Price-ID + checkout_available (Config ⇒ checkout_ready).
        let raid_boost = plans.iter().find(|p| p["id"] == "raid_boost").unwrap();
        assert_eq!(raid_boost["is_current"], false);
        assert!(raid_boost["stripe_price_id"].is_string());
        assert_eq!(raid_boost["checkout_available"], true);
    }

    /// Katalog ohne Auth → 401 auth_required.
    #[tokio::test]
    async fn catalog_unauthenticated_401() {
        let Some(pool) = pool_or_skip("bp_catalog_unauth").await else { return };
        let resp = catalog_handler(
            DashboardAuthLevel::None,
            None,
            State(pool.clone()),
            Query(CatalogQuery { cycle: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

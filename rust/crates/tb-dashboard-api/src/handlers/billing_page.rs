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
//! - `GET /twitch/abbo/rechnungen` → Stripe-Customer-Portal-Link (falls
//!   Customer-ID vorhanden), sonst `/twitch/pricing` mit Grund. Keine eigene
//!   Rechnungsseite.
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
    extract::{Extension, Query, RawForm, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use tb_analytics::billing::{
    catalog_json, find_plan, is_paid_plan_id, normalize_billing_cycle,
    price_id_map_from_env, resolved_price_id,
};
use tb_analytics::plan::resolve_plan_snapshot;
use tb_analytics::stripe::StripeClient;

use crate::auth::level::DashboardAuthLevel;

/// Default-Public-Origin für success/cancel/return-URLs, wenn keine konfiguriert
/// ist (Pythons letzter `_billing_configured_public_origin`-Fallback).
const DEFAULT_PUBLIC_ORIGIN: &str = "https://admin.deutsche-deadlock-community.de";

/// P2.103: User-sichtbarer AGB-/§356-Widerrufsrecht-Hinweis auf der Stripe-
/// Checkout-Seite (deutscher Rechtstext). PLATZHALTER — finaler Wortlaut wird von
/// Claude gesetzt (Markdown-Link zu AGB + Anerkennung des Widerrufsrecht-Verlusts
/// nach § 356 Abs. 5 BGB; Python-Vorlage: abbo_billing_routes.py:99-107).
const CHECKOUT_TOS_MESSAGE: &str = "Mit dem Kauf stimmst du unseren AGB zu. Du verlangst ausdrücklich, dass die Leistung sofort beginnt, und bestätigst, dass dein Widerrufsrecht mit der vollständigen Vertragserfüllung gemäß § 356 Abs. 5 BGB erlischt.";

/// P1.44: User-sichtbare Status-Meldungen der Checkout-Preview (vier Zweige aus
/// `routes_billing.py:api_billing_checkout_preview`). PLATZHALTER — finaler
/// deutscher Wortlaut wird von Claude gesetzt.
const PREVIEW_MSG_FREE: &str = "Dieser Plan ist kostenlos – ein Checkout ist nicht nötig.";
const PREVIEW_MSG_READY: &str = "Der Checkout ist startklar.";
const PREVIEW_MSG_MISSING_PRICE: &str = "Für diesen Plan ist noch keine Preis-ID hinterlegt.";
const PREVIEW_MSG_NOT_CONFIGURED: &str =
    "Der Bezahlvorgang ist noch nicht vollständig eingerichtet.";

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
    let from_url =
        |key: &str| -> Option<String> { std::env::var(key).ok().and_then(|raw| origin_of(&raw)) };
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
    State(pool): State<PgPool>,
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
    // P2.126: Env-Override (Vault-Map) vor dem Hardcode-Default konsumieren —
    // erlaubt pro Plan/Zyklus eine abweichende Stripe-Price-ID ohne Code-Build.
    let price_vault = price_id_map_from_env();
    let Some(price_id) = resolved_price_id(plan_id, cycle, &price_vault) else {
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
        // P2.103: AGB + §356-Widerrufsrecht-Hinweis auf der Stripe-Checkout-Seite.
        "custom_text": {
            "terms_of_service_acceptance": {
                "message": CHECKOUT_TOS_MESSAGE,
            },
        },
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

    let (profile, _) =
        crate::handlers::billing_profile::resolve_profile(&pool, &auth, Some(&config), None).await;
    if let Some(email) = profile
        .get("recipient_email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|email| !email.is_empty())
    {
        session_payload["customer_email"] = json!(email);
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

/// `GET|POST /twitch/abbo/kündigen` — method-bewusster Kündigungs-Einstieg
/// (P1.37 POST-only-Guard + P1.38 CSRF).
///
/// Die Route ist (in lib.rs) auf GET UND POST registriert; dieser eine Handler
/// trennt die Methoden:
/// - **GET** → KEINE state-ändernde Aktion (Prefetch/Link/Image-CSRF-Schutz),
///   Redirect `cancel=post_required` (Python `abbo_billing_routes.py:186-187`).
/// - **POST** → Form-Body-CSRF-Validierung (Localhost-Bypass); ungültig →
///   `cancel=csrf_invalid` (Python :188-191), sonst Kündigung via [`cancel_execute`].
///
/// Nicht eingeloggt → Login-Redirect (vor jedem Stripe-Kontakt).
pub async fn cancel_handler(
    method: axum::http::Method,
    auth: DashboardAuthLevel,
    auth_state: Option<Extension<crate::auth::session::DashboardAuthState>>,
    config: Option<Extension<BillingPageConfig>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Auth-Gate zuerst (CSRF-Validierung braucht eine Session).
    if customer_reference_for(&auth).is_none() {
        return Redirect::to("/twitch/auth/login?next=%2Ftwitch%2Fpricing").into_response();
    }

    // P1.37: GET darf nicht kündigen.
    if method != axum::http::Method::POST {
        return Redirect::to("/twitch/pricing?cancel=post_required").into_response();
    }

    // P1.38: CSRF aus dem Form-Body.
    let form = parse_form(&body);
    if !verify_cancel_csrf(auth_state.as_ref(), &headers, &form).await {
        return Redirect::to("/twitch/pricing?cancel=csrf_invalid").into_response();
    }

    cancel_execute(auth, config, State(pool)).await
}

pub async fn promo_message_handler(
    auth: DashboardAuthLevel,
    auth_state: Option<Extension<crate::auth::session::DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let DashboardAuthLevel::Partner {
        twitch_login,
        twitch_user_id,
        ..
    } = &auth
    else {
        return Redirect::to("/twitch/abbo").into_response();
    };
    let login = twitch_login.trim().to_lowercase();
    let user_id = twitch_user_id.trim().to_string();
    if login.is_empty() || user_id.is_empty() {
        return Redirect::to("/twitch/abbo").into_response();
    }

    let form = parse_form(&body);
    if !verify_cancel_csrf(auth_state.as_ref(), &headers, &form).await {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "csrf_token_invalid" })),
        )
            .into_response();
    }

    let promo_message = form_get(&form, "promo_message").trim().to_string();
    let issues = tb_analytics::promo_mode::validate_streamer_promo_message(&promo_message);
    if let Some(issue) = issues.first() {
        let code = issue
            .code
            .as_deref()
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .unwrap_or("invalid_placeholder");
        return Redirect::to(&format!("/twitch/abbo?promo_error={code}")).into_response();
    }

    match upsert_streamer_promo_message(&pool, &login, &user_id, &promo_message).await {
        Ok(()) => Redirect::to("/twitch/abbo?promo_saved=1").into_response(),
        Err(error) => {
            tracing::error!(%error, login, "promo_message update failed");
            Redirect::to("/twitch/abbo?promo_error=db").into_response()
        }
    }
}

/// Kündigungs-Ausführung via Stripe-Customer-Portal, Fallback `cancel_at_period_end`.
///
/// Port von `abbo_billing_routes.py:abbo_cancel` (ab Zeile 193). Ermittelt die
/// Stripe-Customer-/Subscription-ID des eingeloggten Partners aus
/// `twitch_billing_subscriptions`. Hat der Customer eine Portal-Session → Redirect
/// dorthin (Stripe-hosted, deckt Kündigung + Rechnungen ab). Sonst direkter
/// `cancel_at_period_end`-Fallback. CSRF/POST-Guard liegen in den HTTP-Handlern.
pub async fn cancel_execute(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
    State(pool): State<PgPool>,
) -> Response {
    let Some(reference) = customer_reference_for(&auth) else {
        return Redirect::to("/twitch/auth/login?next=%2Ftwitch%2Fpricing").into_response();
    };
    let (twitch_login, twitch_user_id) = login_and_user_id(&auth);

    let record =
        match active_customer_record(&pool, &twitch_login, &twitch_user_id, &reference).await {
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
        Ok(subscription) => {
            if let Err(error) = persist_cancelled_subscription(&pool, &subscription).await {
                tracing::error!(%error, "billing cancel fallback state persist failed");
                return Redirect::to("/twitch/pricing?cancel=error").into_response();
            }
            Redirect::to("/twitch/pricing?cancel=scheduled").into_response()
        }
        Err(error) => {
            tracing::error!(%error, "billing cancel fallback failed");
            Redirect::to("/twitch/pricing?cancel=error").into_response()
        }
    }
}

// ── Legacy-Rechnungslink ────────────────────────────────────────────────────

/// `GET /twitch/abbo/rechnungen` — obsolete eigene Rechnungsseite.
///
/// Grillme-2A-Drop: Rechnungen werden nicht mehr selbst gerendert, sondern über
/// das Stripe-hosted Customer-Portal angeboten. Ohne Session, Customer-ID oder
/// Stripe-Konfiguration landet der alte Link sauber auf `/twitch/pricing`.
pub async fn legacy_invoices_redirect_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
    State(pool): State<PgPool>,
) -> Response {
    let Some(reference) = customer_reference_for(&auth) else {
        return Redirect::to("/twitch/auth/login?next=%2Ftwitch%2Fabbo%2Frechnungen")
            .into_response();
    };
    let (twitch_login, twitch_user_id) = login_and_user_id(&auth);

    let record =
        match active_customer_record(&pool, &twitch_login, &twitch_user_id, &reference).await {
            Ok(rec) => rec,
            Err(error) => {
                tracing::error!(%error, "billing invoices legacy redirect: customer lookup failed");
                return Redirect::to("/twitch/pricing?invoice=error").into_response();
            }
        };
    let Some(record) = record else {
        return Redirect::to("/twitch/pricing?invoice=missing_customer").into_response();
    };
    if record.stripe_customer_id.is_empty() {
        return Redirect::to("/twitch/pricing?invoice=missing_customer").into_response();
    }

    let Some(Extension(config)) = config else {
        return Redirect::to("/twitch/pricing?invoice=error").into_response();
    };
    let return_url = format!(
        "{}/twitch/pricing?invoice=portal_returned",
        config.public_origin.trim_end_matches('/')
    );

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
            if url.is_empty() {
                Redirect::to("/twitch/pricing?invoice=portal_unavailable").into_response()
            } else {
                Redirect::to(&url).into_response()
            }
        }
        Err(error) => {
            tracing::warn!(%error, "billing invoices legacy redirect: portal unavailable");
            Redirect::to("/twitch/pricing?invoice=portal_unavailable").into_response()
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
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "auth_required" })),
        )
            .into_response();
    }

    let cycle = normalize_billing_cycle(parse_u32(params.cycle.as_deref(), 1));
    let mut payload = catalog_json(cycle);
    let price_vault = price_id_map_from_env();
    let readiness = readiness_payload(config.as_ref().map(|Extension(c)| c));
    let checkout_ready = readiness["checkout_ready"].as_bool().unwrap_or(false);

    // Aktuellen Plan auflösen (Partner: eigener; Admin/Localhost: kein Partner-
    // Kontext → raid_free-Default).
    let (twitch_login, twitch_user_id) = login_and_user_id(&auth);
    let current = resolve_plan_snapshot(&pool, &twitch_login, &twitch_user_id)
        .await
        .unwrap_or_else(|_| {
            // Fail-safe: raid_free. resolve_plan_snapshot liefert das ohnehin als
            // Default; nur DB-Fehler landen hier. Kanonischer Konstruktor (statt
            // Literal), damit künftige PlanSnapshot-Felder hier nicht brechen.
            let fallback_ref = if !twitch_login.is_empty() {
                twitch_login.clone()
            } else {
                twitch_user_id.clone()
            };
            tb_analytics::plan::PlanSnapshot::default_basic(&fallback_ref)
        });
    let current_plan_id = current.plan_id;

    // Pro Plan: is_current + Stripe-Price-Verfügbarkeit annotieren.
    if let Some(plans) = payload.get_mut("plans").and_then(Value::as_array_mut) {
        for plan in plans.iter_mut() {
            let pid = plan
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            plan["is_current"] = json!(pid == current_plan_id);
            if !is_paid_plan_id(&pid) {
                plan["checkout_available"] = json!(false);
                plan["stripe_price_id"] = Value::Null;
                continue;
            }
            // Dieselbe Aufloesung wie der Checkout: eingecheckter Default,
            // sonst Vault-Map. Vorher las der Katalog nur den Default und zeigte
            // deshalb `checkout_available: false`, obwohl der Checkout gegangen waere.
            let price_id = resolved_price_id(&pid, cycle, &price_vault);
            plan["checkout_available"] = json!(price_id.is_some() && checkout_ready);
            plan["stripe_price_id"] = match price_id {
                Some(id) => json!(id),
                None => Value::Null,
            };
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

    // B2-P1: Rechnungsempfänger-Profil (persistiert + Stripe-Customer-Prefill) für
    // die UI-Vorbelegung. Die Stripe-Customer-ID kommt aus dem aktiven Abo-Record;
    // ohne Config/Customer-ID liefert resolve_profile nur das persistierte/Default-
    // Profil (kein Stripe-Call).
    let stripe_customer_id = active_customer_record(
        &pool,
        &twitch_login,
        &twitch_user_id,
        &customer_reference_for(&auth).unwrap_or_default(),
    )
    .await
    .ok()
    .flatten()
    .map(|rec| rec.stripe_customer_id)
    .filter(|id| !id.is_empty());
    let (profile, imported_fields) = crate::handlers::billing_profile::resolve_profile(
        &pool,
        &auth,
        config.as_ref().map(|Extension(c)| c),
        stripe_customer_id.as_deref(),
    )
    .await;
    payload["billing_profile"] = profile;
    payload["billing_profile_imported_fields"] = json!(imported_fields);

    payload["payment"] = payment_section(&readiness);

    Json(payload).into_response()
}

// ── Checkout-Preview (JSON, P1.44) ───────────────────────────────────────────

/// Request-Body von `POST /twitch/api/billing/checkout-preview`.
#[derive(Debug, Deserialize, Default)]
pub struct CheckoutPreviewBody {
    #[serde(default)]
    pub plan_id: Option<String>,
    /// Zyklus als Zahl ODER String (Python akzeptiert beides) → über Helfer geparst.
    #[serde(default)]
    pub cycle_months: Option<Value>,
}

/// `POST /twitch/api/billing/checkout-preview` — validiert die Plan-Auswahl und
/// liefert Stripe-Ready-Metadaten für die Pre-Checkout-UX.
///
/// Port von `routes_billing.py:api_billing_checkout_preview`. Nicht authentifiziert
/// → 401; unbekannte `plan_id` → 404 `unknown_plan_id` (+ `available_plan_ids`);
/// sonst 200 mit `ready`/`integration_state`/`message`/`next_steps`. Die
/// `message`-Texte sind user-sichtbar → PLATZHALTER (Claude setzt den Wortlaut).
pub async fn checkout_preview_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
    Json(body): Json<CheckoutPreviewBody>,
) -> Response {
    if !auth.is_authenticated() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "auth_required" })),
        )
            .into_response();
    }

    let cycle = normalize_billing_cycle(parse_cycle_value(body.cycle_months.as_ref()));
    let catalog = catalog_json(cycle);
    let plans = catalog["plans"].as_array().cloned().unwrap_or_default();

    let selected_plan_id = body.plan_id.unwrap_or_default().trim().to_string();
    let Some(selected_plan) = plans
        .iter()
        .find(|p| p["id"].as_str() == Some(selected_plan_id.as_str()))
        .cloned()
    else {
        let available: Vec<&str> = plans.iter().filter_map(|p| p["id"].as_str()).collect();
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown_plan_id", "available_plan_ids": available })),
        )
            .into_response();
    };

    let total_cents = selected_plan["price"]["total_gross_cents"]
        .as_i64()
        .unwrap_or(0);
    let readiness = readiness_payload(config.as_ref().map(|Extension(c)| c));
    let checkout_ready = readiness["checkout_ready"].as_bool().unwrap_or(false);
    let price_map_ready = readiness["price_map_ready"].as_bool().unwrap_or(false);
    let price_id = if is_paid_plan_id(&selected_plan_id) {
        resolved_price_id(&selected_plan_id, cycle, &price_id_map_from_env())
    } else {
        None
    };
    let checkout_possible =
        total_cents > 0 && checkout_ready && price_map_ready && price_id.is_some();
    let ready = total_cents <= 0 || checkout_possible;

    // P1.44: user-sichtbare Status-Meldung — vier Zweige (Python). PLATZHALTER.
    let message = if total_cents <= 0 {
        PREVIEW_MSG_FREE
    } else if checkout_possible {
        PREVIEW_MSG_READY
    } else if checkout_ready && price_id.is_none() {
        PREVIEW_MSG_MISSING_PRICE
    } else {
        PREVIEW_MSG_NOT_CONFIGURED
    };

    let payload = json!({
        "ready": ready,
        "provider": "stripe",
        "integration_state": readiness["integration_state"].as_str().unwrap_or("planned"),
        "currency": catalog["currency"],
        "tax_mode": catalog["tax_mode"],
        "gross_available": catalog["gross_available"],
        "cycle_months": catalog["cycle_months"],
        "cycle_label": catalog["cycle_label"],
        "plan": selected_plan,
        "stripe_price_id": price_id.map(Value::from).unwrap_or(Value::Null),
        "invoice_preview_path": "/twitch/abbo/rechnungen",
        "invoice_page_path": "/twitch/abbo/rechnungen",
        "message": message,
        "stripe_docs_url": tb_analytics::billing::catalog::STRIPE_QUICKSTART_URL,
        "next_steps": [
            "stripe_product_price_ids_hinterlegen",
            "webhook_verarbeitung_fuer_abos_aktivieren",
        ],
    });
    Json(payload).into_response()
}

/// Parst `cycle_months` aus JSON (Zahl ODER String, sonst `1`).
fn parse_cycle_value(raw: Option<&Value>) -> u32 {
    match raw {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(1) as u32,
        Some(Value::String(s)) => s.trim().parse::<u32>().unwrap_or(1),
        _ => 1,
    }
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
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "auth_required" })),
        )
            .into_response();
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

/// Baut die `payment`-Sektion des Katalogs (P2.125-Parität).
///
/// Port von `billing_plans.py:build_billing_catalog payment` + den Pfad-Ergänzungen
/// aus `routes_billing.py:api_billing_catalog`. `integration_state`/`checkout_enabled`
/// werden aus der Readiness abgeleitet (`payment_state_from_readiness`); die übrigen
/// Felder sind statische Pfade/Metadaten. `checkout_ready` (Rust-Zusatz) bleibt
/// erhalten, damit bestehende Frontend-Konsumenten nicht brechen.
fn payment_section(readiness: &Value) -> Value {
    use tb_analytics::billing::catalog::{
        payment_state_from_readiness, STRIPE_QUICKSTART_URL, SUPPORTED_METHODS_PLANNED,
    };

    let checkout_ready = readiness["checkout_ready"].as_bool().unwrap_or(false);
    let price_map_ready = readiness["price_map_ready"].as_bool().unwrap_or(false);
    let integration_override = readiness["integration_state"].as_str();
    let state = payment_state_from_readiness(checkout_ready, price_map_ready, integration_override);

    json!({
        "provider": "stripe",
        "integration_state": state.integration_state,
        "checkout_enabled": state.checkout_enabled,
        "checkout_preview_enabled": true,
        "catalog_path": "/twitch/api/billing/catalog",
        "checkout_preview_path": "/twitch/api/billing/checkout-preview",
        "readiness_path": "/twitch/api/billing/readiness",
        "webhook_path": "/twitch/api/billing/stripe/webhook",
        "quickstart_url": STRIPE_QUICKSTART_URL,
        "supported_methods_planned": SUPPORTED_METHODS_PLANNED,
        // Pfad-Ergänzungen aus routes_billing.py:api_billing_catalog.
        "invoice_preview_path": "/twitch/abbo/rechnungen",
        "invoice_page_path": "/twitch/abbo/rechnungen",
        "stripe_sync_path": "/twitch/api/billing/stripe/sync-products",
        // Rust-native Zusätze (Bestandsschutz für bestehende Konsumenten).
        "checkout_path": "/twitch/abbo/bezahlen",
        "cancel_path": "/twitch/abbo/kündigen",
        "checkout_ready": checkout_ready,
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
            ..
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
        DashboardAuthLevel::Admin { .. } => None,
        DashboardAuthLevel::None => None,
    }
}

/// `(twitch_login, twitch_user_id)` des eingeloggten Partners (sonst leer).
fn login_and_user_id(auth: &DashboardAuthLevel) -> (String, String) {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => (twitch_login.clone(), twitch_user_id.clone()),
        _ => (String::new(), String::new()),
    }
}

/// Parst einen `u32` aus einem optionalen String, sonst Default.
fn parse_u32(raw: Option<&str>, default: u32) -> u32 {
    raw.and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

/// Parst einen URL-encoded Form-Body in Key/Value-Paare.
fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Liest einen Form-Wert (leer wenn nicht vorhanden).
fn form_get<'a>(form: &'a [(String, String)], key: &str) -> &'a str {
    form.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

/// Validiert das Form-Body-CSRF-Token der Kündigung gegen die Session
/// (Admin- vor Partner-Cookie). Spiegelt `billing_profile::verify_form_csrf`.
async fn verify_cancel_csrf(
    auth_state: Option<&Extension<crate::auth::session::DashboardAuthState>>,
    headers: &axum::http::HeaderMap,
    form: &[(String, String)],
) -> bool {
    use crate::auth::session::{ADMIN_COOKIE_NAME, PARTNER_COOKIE_NAME};
    let Some(Extension(state)) = auth_state else {
        return false;
    };
    let presented = form_get(form, "csrf_token").trim().to_string();
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let read = |name: &str| -> Option<String> {
        cookie_header.split(';').find_map(|pair| {
            let pair = pair.trim();
            pair.split_once('=')
                .filter(|(k, _)| k.trim() == name)
                .map(|(_, v)| v.trim().to_string())
        })
    };
    let (cookie, session_type) = if let Some(c) = read(ADMIN_COOKIE_NAME).filter(|s| !s.is_empty())
    {
        (c, "discord_admin")
    } else if let Some(c) = read(PARTNER_COOKIE_NAME).filter(|s| !s.is_empty()) {
        (c, "twitch")
    } else {
        return false;
    };
    state
        .validate_csrf(&cookie, session_type, &presented)
        .await
        .unwrap_or(false)
}

/// Redirect auf die Pricing-Seite mit `checkout=unavailable&reason=...`.
fn pricing_unavailable(reason: &str) -> Response {
    Redirect::to(&format!(
        "/twitch/pricing?checkout=unavailable&reason={reason}"
    ))
    .into_response()
}

async fn upsert_streamer_promo_message(
    pool: &PgPool,
    login: &str,
    user_id: &str,
    promo_message: &str,
) -> Result<(), sqlx::Error> {
    let message = promo_message.trim();
    let value: Option<String> = if message.is_empty() {
        None
    } else {
        Some(message.to_string())
    };
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_message) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (twitch_user_id) DO UPDATE SET \
             promo_message = EXCLUDED.promo_message, \
             twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)",
    )
    .bind(user_id)
    .bind(login)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
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

    let row: Option<CustomerRecord> = sqlx::query_as!(
        CustomerRecord,
        r#"
        SELECT
            COALESCE(stripe_customer_id, '')     AS "stripe_customer_id!",
            COALESCE(stripe_subscription_id, '') AS "stripe_subscription_id!"
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
        login,
        user_id,
        reference
    )
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

async fn persist_cancelled_subscription(
    pool: &PgPool,
    subscription: &Value,
) -> Result<(), sqlx::Error> {
    let subscription_id = subscription
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if subscription_id.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    let event_id = format!("dashboard_cancel_fallback:{subscription_id}");
    tb_analytics::stripe::webhook_apply::apply_event(
        &mut tx,
        &event_id,
        "customer.subscription.updated",
        subscription,
        None,
    )
    .await?;
    tx.commit().await?;
    if let Some(sync) = tb_analytics::stripe::webhook_apply::streamer_plan_sync_from_event(
        "customer.subscription.updated",
        subscription,
        None,
    ) {
        if let Err(error) =
            tb_analytics::stripe::webhook_apply::sync_plan_to_streamer_plans(pool, &sync).await
        {
            tracing::warn!(%error, "billing cancel fallback streamer plan sync failed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partner(login: &str, uid: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: uid.to_string(),
            display_name: String::new(),
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
        assert_eq!(
            customer_reference_for(&partner("Streamer", "42")).as_deref(),
            Some("Streamer")
        );
        assert_eq!(
            customer_reference_for(&partner("  ", "42")).as_deref(),
            Some("42")
        );
        assert_eq!(customer_reference_for(&partner("", "")), None);
        assert_eq!(customer_reference_for(&DashboardAuthLevel::admin()), None);
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

    /// P1.37: GET /twitch/abbo/kündigen löst KEINE Kündigung aus, sondern
    /// redirectet auf cancel=post_required. Pool/Config werden nie berührt
    /// (würde sonst beim leeren Pool/HTTP-Call panicen).
    #[tokio::test]
    async fn cancel_get_is_post_only_guard() {
        let Some(pool) = pool_or_skip("bp_cancel_get_guard").await else {
            return;
        };
        let resp = cancel_handler(
            axum::http::Method::GET,
            partner("login", "5"),
            None,
            None,
            State(pool),
            axum::http::HeaderMap::new(),
            axum::body::Bytes::new(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing?cancel=post_required");
    }

    /// P1.37: GET-Guard ohne Login → Login-Redirect (vor Pool/Config).
    #[tokio::test]
    async fn cancel_get_unauthenticated_redirects_login() {
        let Some(pool) = pool_or_skip("bp_cancel_get_unauth").await else {
            return;
        };
        let resp = cancel_handler(
            axum::http::Method::GET,
            DashboardAuthLevel::None,
            None,
            None,
            State(pool),
            axum::http::HeaderMap::new(),
            axum::body::Bytes::new(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.contains("/twitch/auth/login"), "got {loc}");
    }

    /// P1.38: POST ohne gültiges CSRF-Token (kein auth_state) → cancel=csrf_invalid,
    /// KEINE Kündigung (Stripe wird nie kontaktiert).
    #[tokio::test]
    async fn cancel_post_without_csrf_is_rejected() {
        let Some(pool) = pool_or_skip("bp_cancel_csrf").await else {
            return;
        };
        let resp = cancel_handler(
            axum::http::Method::POST,
            partner("login", "5"),
            None, // kein DashboardAuthState → CSRF schlägt fehl
            None,
            State(pool),
            axum::http::HeaderMap::new(),
            axum::body::Bytes::new(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing?cancel=csrf_invalid");
    }

    /// P2.125: Die served `payment`-Sektion trägt die zuvor fehlenden Keys.
    #[test]
    fn payment_section_emits_full_python_keys() {
        let readiness = readiness_payload(None); // checkout_ready=false → planned
        let payment = payment_section(&readiness);
        assert_eq!(payment["provider"], "stripe");
        assert_eq!(payment["integration_state"], "planned");
        assert_eq!(payment["checkout_enabled"], false);
        assert_eq!(payment["checkout_preview_enabled"], true);
        assert_eq!(
            payment["checkout_preview_path"],
            "/twitch/api/billing/checkout-preview"
        );
        assert_eq!(
            payment["quickstart_url"],
            "https://docs.stripe.com/billing/quickstart"
        );
        let methods = payment["supported_methods_planned"].as_array().unwrap();
        assert!(methods.iter().any(|m| m == "card"));
        assert!(methods.iter().any(|m| m == "sepa_debit"));
        assert!(methods.iter().any(|m| m == "paypal_via_wallet_if_enabled"));
    }

    /// P1.44: Preview ohne Auth → 401.
    #[tokio::test]
    async fn checkout_preview_unauthenticated_401() {
        let resp = checkout_preview_handler(
            DashboardAuthLevel::None,
            None,
            Json(CheckoutPreviewBody {
                plan_id: Some("plus".into()),
                cycle_months: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// P1.44: unbekannte plan_id → 404 unknown_plan_id + available_plan_ids.
    #[tokio::test]
    async fn checkout_preview_unknown_plan_404() {
        let resp = checkout_preview_handler(
            partner("login", "5"),
            None,
            Json(CheckoutPreviewBody {
                plan_id: Some("does_not_exist".into()),
                cycle_months: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "unknown_plan_id");
        assert!(v["available_plan_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "plus"));
    }

    /// P1.44: gültiger bezahlter Plan ohne Stripe-Config → 200 mit readiness/
    /// next_steps; ready=false (Checkout nicht konfiguriert), price_id=null.
    #[tokio::test]
    async fn checkout_preview_valid_plan_returns_readiness() {
        let resp = checkout_preview_handler(
            partner("login", "5"),
            None,
            Json(CheckoutPreviewBody {
                plan_id: Some("plus".into()),
                cycle_months: Some(serde_json::json!(1)),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["provider"], "stripe");
        assert_eq!(v["plan"]["id"], "plus");
        assert_eq!(v["plan"]["price"]["total_gross_cents"], 499);
        assert_eq!(v["tax_mode"], "small_business");
        assert_eq!(v["gross_available"], true);
        assert_eq!(v["integration_state"], "planned");
        assert_eq!(v["ready"], false);
        assert!(v["next_steps"].as_array().unwrap().len() == 2);
    }

    /// P1.44: Free-Plan → ready=true (kein Stripe nötig).
    #[tokio::test]
    async fn checkout_preview_free_plan_is_ready() {
        let resp = checkout_preview_handler(
            partner("login", "5"),
            None,
            Json(CheckoutPreviewBody {
                plan_id: Some("free".into()),
                cycle_months: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ready"], true);
        assert!(v["stripe_price_id"].is_null());
    }

    #[tokio::test]
    async fn abbo_redirect_keeps_query_string() {
        let resp =
            abbo_redirect_handler(axum::extract::RawQuery(Some("cycle=12".to_string()))).await;
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
            client: Arc::new(
                StripeClient::new("sk_test_dummy")
                    .unwrap()
                    .with_api_base(base),
            ),
            public_origin: "https://admin.example.test".to_string(),
        }
    }

    fn lazy_pool() -> PgPool {
        PgPoolOptions::new()
            .connect_lazy("postgres://postgres@127.0.0.1:1/unused")
            .unwrap()
    }

    /// Seit dem Umbau auf drei Stufen stehen keine Stripe-Price-IDs mehr im
    /// Code (die alten zeigten auf Netto-Preise). Produktiv liefert sie der
    /// Vault; im Test setzen wir dieselbe Env-Variable einmal prozessweit.
    fn test_price_map_setzen() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            std::env::set_var(
                "STRIPE_PRICE_ID_MAP",
                r#"{"plus":{"1":"price_test_plus_1m","12":"price_test_plus_12m"},
                    "pro":{"1":"price_test_pro_1m","12":"price_test_pro_12m"}}"#,
            );
        });
    }

    /// Unauth → Login-Redirect, KEIN Stripe-Call.
    #[tokio::test]
    async fn checkout_unauthenticated_redirects_to_login() {
        let resp = checkout_start_handler(
            DashboardAuthLevel::None,
            None,
            State(lazy_pool()),
            Query(CheckoutQuery {
                plan_id: Some("plus".into()),
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
        test_price_map_setzen();
        let Some(pool) = pool_or_skip("bp_checkout_session").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_billing_profiles \
             (customer_reference, recipient_name, recipient_email, country_code, updated_at) \
             VALUES ('streamerlogin', 'Streamer Login', 'billing@example.test', 'DE', 'now')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/checkout/sessions"))
            // Beleg: Plan/Cycle landen in den Metadaten der Session.
            .and(body_string_contains("subscription"))
            // P2.103: AGB/§356-custom_text wird mitgesendet.
            .and(body_string_contains("terms_of_service_acceptance"))
            .and(body_string_contains("customer_email"))
            .and(body_string_contains("billing%40example.test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cs_test_abc",
                "url": "https://checkout.stripe.com/c/pay/cs_test_abc",
            })))
            .mount(&server)
            .await;

        let resp = checkout_start_handler(
            partner("streamerlogin", "42"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool),
            Query(CheckoutQuery {
                plan_id: Some("plus".into()),
                cycle: Some("1".into()),
                quantity: Some("1".into()),
            }),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "https://checkout.stripe.com/c/pay/cs_test_abc");
    }

    /// Free-Stufe → kein Stripe-Price → Redirect auf Pricing, KEIN Call.
    #[tokio::test]
    async fn checkout_free_plan_redirects_to_pricing() {
        let resp = checkout_start_handler(
            partner("login", "1"),
            Some(Extension(cfg_with_base("http://unused.invalid"))),
            State(lazy_pool()),
            Query(CheckoutQuery {
                plan_id: Some("free".into()),
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
            State(lazy_pool()),
            Query(CheckoutQuery {
                plan_id: Some("plus".into()),
                cycle: Some("1".into()),
                quantity: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(
            loc.contains("reason=stripe_secret_key_missing"),
            "got {loc}"
        );
    }

    /// Stripe-API-Fehler beim Erstellen → Redirect mit reason=checkout_create_failed.
    #[tokio::test]
    async fn checkout_stripe_error_redirects_with_reason() {
        let Some(pool) = pool_or_skip("bp_checkout_stripe_error").await else {
            return;
        };
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
            State(pool),
            Query(CheckoutQuery {
                plan_id: Some("plus".into()),
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
        sqlx::query(
            r#"CREATE TABLE twitch_billing_subscriptions (
                   stripe_subscription_id TEXT PRIMARY KEY, stripe_customer_id TEXT,
                   customer_reference TEXT, status TEXT NOT NULL DEFAULT 'unknown', plan_id TEXT,
                   cycle_months INTEGER NOT NULL DEFAULT 1, quantity INTEGER NOT NULL DEFAULT 1,
                   current_period_start TEXT, current_period_end TEXT,
                   cancel_at_period_end INTEGER NOT NULL DEFAULT 0, canceled_at TEXT, ended_at TEXT,
                   last_event_id TEXT, updated_at TEXT NOT NULL
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE streamer_plans (
                   twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
                   plan_name TEXT NOT NULL DEFAULT 'free', expires_at TEXT,
                   manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_updated_at TEXT,
                   trial_ever_granted INTEGER DEFAULT 0, first_login_at TEXT, promo_message TEXT
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        // B2-P1: Rechnungsprofil-Tabelle (Katalog liest sie für die UI-Vorbelegung).
        sqlx::query(
            r#"CREATE TABLE twitch_billing_profiles (
                   customer_reference TEXT PRIMARY KEY,
                   recipient_name TEXT NOT NULL DEFAULT '', recipient_email TEXT NOT NULL DEFAULT '',
                   company_name TEXT NOT NULL DEFAULT '', street_line1 TEXT NOT NULL DEFAULT '',
                   postal_code TEXT NOT NULL DEFAULT '', city TEXT NOT NULL DEFAULT '',
                   country_code TEXT NOT NULL DEFAULT '', vat_id TEXT NOT NULL DEFAULT '',
                   updated_at TEXT NOT NULL DEFAULT ''
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE dashboard_sessions (
                   session_id TEXT PRIMARY KEY,
                   session_type TEXT NOT NULL,
                   payload_enc BYTEA NOT NULL,
                   created_at DOUBLE PRECISION NOT NULL,
                   expires_at DOUBLE PRECISION NOT NULL
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    /// Cancel: keine aktive Subscription → Redirect cancel=missing.
    #[tokio::test]
    async fn cancel_without_subscription_redirects_missing() {
        let Some(pool) = pool_or_skip("bp_cancel_missing").await else {
            return;
        };
        let server = MockServer::start().await;
        let resp = cancel_execute(
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
        let Some(pool) = pool_or_skip("bp_cancel_fallback").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_billing_subscriptions \
             (stripe_subscription_id, stripe_customer_id, customer_reference, status, updated_at) \
             VALUES ('sub_x', '', 'login', 'active', '2026-06-01T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/sub_x"))
            .and(body_string_contains("cancel_at_period_end"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sub_x", "cancel_at_period_end": true
            })))
            .mount(&server)
            .await;

        let resp = cancel_execute(
            partner("login", "5"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool.clone()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing?cancel=scheduled");
        let cancel_flag: i32 = sqlx::query_scalar(
            "SELECT cancel_at_period_end FROM twitch_billing_subscriptions WHERE stripe_subscription_id = 'sub_x'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cancel_flag, 1);
    }

    /// Cancel mit Customer-ID → Portal-Session, Redirect zur hosted Portal-URL.
    #[tokio::test]
    async fn cancel_with_customer_uses_portal() {
        let Some(pool) = pool_or_skip("bp_cancel_portal").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_billing_subscriptions \
             (stripe_subscription_id, stripe_customer_id, customer_reference, status, updated_at) \
             VALUES ('sub_p', 'cus_p', 'login', 'active', '2026-06-01T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/billing_portal/sessions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "bps_1", "url": "https://billing.stripe.com/p/session/bps_1"
            })))
            .mount(&server)
            .await;

        let resp = cancel_execute(
            partner("login", "5"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool.clone()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "https://billing.stripe.com/p/session/bps_1");
    }

    #[tokio::test]
    async fn promo_message_post_updates_db() {
        let Some(pool) = pool_or_skip("bp_promo_message").await else {
            return;
        };
        let auth_state = crate::auth::session::DashboardAuthState::new(
            pool.clone(),
            "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string(),
        );
        let created = auth_state
            .create_partner_session("nani", "42", "Nani")
            .await
            .unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            axum::http::HeaderValue::from_str(&format!(
                "{}={}",
                crate::auth::session::PARTNER_COOKIE_NAME,
                created.session_id
            ))
            .unwrap(),
        );
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        serializer
            .append_pair("csrf_token", &created.csrf_token)
            .append_pair("promo_message", "Join {invite}");
        let body = serializer.finish();

        let resp = promo_message_handler(
            partner("nani", "42"),
            Some(Extension(auth_state)),
            State(pool.clone()),
            headers,
            RawForm(axum::body::Bytes::from(body)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/abbo?promo_saved=1");
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT promo_message FROM streamer_plans WHERE twitch_user_id = '42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored.as_deref(), Some("Join {invite}"));
    }

    /// Katalog: eingeloggt → 200 mit Plan-Status (KEIN 502/Proxy). Aktueller Plan
    /// wird aufgelöst; bezahlte Pläne tragen checkout_available + stripe_price_id.
    #[tokio::test]
    async fn catalog_returns_plan_status_not_proxy() {
        test_price_map_setzen();
        let Some(pool) = pool_or_skip("bp_catalog").await else {
            return;
        };
        let server = MockServer::start().await;
        let resp = catalog_handler(
            partner("login", "5"),
            Some(Extension(cfg_with_base(&server.uri()))),
            State(pool.clone()),
            Query(CatalogQuery {
                cycle: Some("1".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["currency"], "EUR");
        assert_eq!(v["tax_mode"], "small_business");
        assert_eq!(v["gross_available"], true);
        let plans = v["plans"].as_array().unwrap();
        assert_eq!(plans.len(), 3);
        // Ohne Eintrag steht der Streamer auf dem Default raid_free (Stufe Free);
        // die Free-Karte ist deshalb keine der drei Katalog-Karten "is_current".
        assert_eq!(v["current_subscription"]["plan_id"], "raid_free");
        let free = plans.iter().find(|p| p["id"] == "free").unwrap();
        assert_eq!(free["checkout_available"], false);
        assert!(free["stripe_price_id"].is_null());
        assert_eq!(free["price"]["total_gross_cents"], 0);
        // Bezahlte Stufe trägt Price-ID + checkout_available (Config ⇒ checkout_ready).
        let plus = plans.iter().find(|p| p["id"] == "plus").unwrap();
        assert_eq!(plus["price"]["total_gross_cents"], 499);
        assert!(plus["stripe_price_id"].is_string());
        assert_eq!(plus["checkout_available"], true);
        // B2-P1: Katalog liefert das Rechnungsprofil (Default — noch nichts
        // persistiert; recipient_name fällt auf den Login zurück, country=DE).
        assert_eq!(v["billing_profile"]["customer_reference"], "login");
        assert_eq!(v["billing_profile"]["country_code"], "DE");
        assert!(v["billing_profile_imported_fields"].is_array());
    }

    /// Katalog ohne Auth → 401 auth_required.
    #[tokio::test]
    async fn catalog_unauthenticated_401() {
        let Some(pool) = pool_or_skip("bp_catalog_unauth").await else {
            return;
        };
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

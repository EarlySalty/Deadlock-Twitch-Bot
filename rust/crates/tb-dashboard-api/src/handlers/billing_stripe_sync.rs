//! Stripe Product/Price-Sync (B2-P1-stripe-sync-products).
//!
//! `POST /twitch/api/billing/stripe/sync-products` — legt für jeden bezahlten Plan
//! (× Zyklen {1, 12}) das Stripe-Produkt + die Preise an bzw. verwendet bestehende
//! wieder. Port von `bot/dashboard/routes_billing.py:api_billing_stripe_sync_products`
//! (Zeilen 375-590) + dem ID-Map-Layer aus `billing_mixin.py:147-235`.
//!
//! **Bewusste Abweichung (Migrationsbug-Fix, dokumentiert):** Python persistiert
//! die erzeugten Product-/Price-IDs in einen *Windows-Keyring-Vault*
//! (`_write_keyring_secret`). Dieser host-spezifische Secret-Writer existiert im
//! nativen Linux-Dashboard NICHT — Secrets kommen read-only aus Infisical. Der
//! native Pfad braucht ihn auch nicht: die in `tb_analytics::billing::catalog`
//! **eingecheckten** `PRICE_ID_DEFAULTS`/`PRODUCT_ID_DEFAULTS` sind die Quelle der
//! Wahrheit (Readiness meldet `price_map_ready=true`). Der Endpoint führt daher
//! create/reuse aus und liefert die resultierenden IDs im Operations-Report, meldet
//! aber `persisted_to_windows_vault=false` mit Grund. Das Schreiben des Runtime-
//! Overrides in einen Secret-Store ist ein crate-fremdes Folge-Ticket (Handoff).
//!
//! Auth: Admin/Localhost (`DashboardAuthLevel::is_privileged`). Body: optionaler
//! `dry_run` (JSON oder Form) — bei `true` werden KEINE Stripe-Objekte erzeugt,
//! nur der Plan (`would_create`/`reused`) berichtet.
//!
//! **Self-Heal (P2.113):** Eingecheckte Default-IDs werden im Live-Lauf erst gegen
//! Stripe verifiziert, bevor sie als `reused` gelten. Eine *Price*-Default-ID via
//! `retrieve_price` (`routes_billing.py:498-504`), eine *Product*-Default-ID via
//! `retrieve_product` + `deleted`-Flag (`routes_billing.py:439-449`). Schlägt der
//! Retrieve fehl (transienter 5xx) ODER ist das Objekt gelöscht, wird die ID
//! verworfen, NICHT als `reused` gezählt, und der Lookup-/Create-Pfad legt neu an —
//! genau wie Pythons Verify-Zyklus, der jede Exception schluckt (ein 5xx killt den
//! Sync nicht). Im `dry_run` wird nicht verifiziert; die Defaults gelten dort
//! unverändert als `reused`.
//!
//! **Response-Parität (P2.114):** Der Payload führt wieder `product_id_map`
//! (plan_id → product_id), `price_id_map` (plan_id → {cycle → price_id}) und
//! `readiness` (Stripe-Readiness, keine Secrets) wie Python
//! (`routes_billing.py:578-592`).

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use sqlx::PgPool;

use tb_analytics::billing::{
    catalog_json, is_paid_plan_id, price_id_default, product_id_default,
};

use crate::auth::level::DashboardAuthLevel;
use crate::handlers::billing_page::BillingPageConfig;

/// Abrechnungszyklen, für die Preise angelegt werden (Python `billing_cycle_discounts`).
const SYNC_CYCLES: [u32; 2] = [1, 12];

/// `POST /twitch/api/billing/stripe/sync-products`.
pub async fn sync_products_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<BillingPageConfig>>,
    State(_pool): State<PgPool>,
    body: axum::body::Bytes,
) -> Response {
    if !auth.is_privileged() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "admin_required" })),
        )
            .into_response();
    }

    let dry_run = parse_dry_run(&body);

    // Stripe-Client erforderlich (außer im dry_run — dort wird nichts erstellt,
    // wir können den Plan trotzdem berichten).
    let client = config.as_ref().map(|Extension(c)| c.client.clone());
    if client.is_none() && !dry_run {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "stripe_secret_key_missing",
                "missing": ["stripe_secret_key"],
            })),
        )
            .into_response();
    }

    let mut operations: Vec<Value> = Vec::new();
    let mut created_products = 0u32;
    let mut reused_products = 0u32;
    let mut created_prices = 0u32;
    let mut reused_prices = 0u32;
    // P2.114: ID-Maps für den Response-Payload akkumulieren.
    let mut product_id_map: Map<String, Value> = Map::new();
    let mut price_id_map: Map<String, Value> = Map::new();

    // Bezahlte Pläne aus dem 1-Monats-Katalog (Plan-Stammdaten sind zyklus-stabil).
    let base_catalog = catalog_json(1);
    let plans = base_catalog
        .get("plans")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for plan in plans.iter() {
        let plan_id = plan.get("id").and_then(Value::as_str).unwrap_or("");
        if !is_paid_plan_id(plan_id) {
            continue;
        }
        let plan_name = plan
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(plan_id);
        let plan_description = plan.get("description").and_then(Value::as_str).unwrap_or("");

        // ── Produkt: eingecheckter Default → gegen Stripe verifizieren (P2.113) ──
        //    Live: retrieve_product + deleted-Flag prüfen; schlägt der Retrieve fehl
        //    ODER ist das Produkt gelöscht, wird die ID verworfen, NICHT als reused
        //    gezählt → der Create-Pfad legt neu an (Self-Heal). Ein transienter 5xx
        //    killt den Sync NICHT (Python schluckt jede Exception).
        //    dry_run: kein Stripe-Call, Default gilt unverifiziert als reused.
        let mut product_id = product_id_default(plan_id).unwrap_or("").to_string();
        if !product_id.is_empty() && !dry_run {
            if let Some(client) = client.as_ref() {
                match client.retrieve_product(&product_id).await {
                    Ok(obj) if !is_stripe_deleted(&obj) => {}
                    Ok(_) => {
                        tracing::warn!(plan_id, "stripe product is deleted; recreating");
                        product_id.clear();
                    }
                    Err(error) => {
                        tracing::warn!(%error, plan_id, "stripe product retrieve failed; recreating");
                        product_id.clear();
                    }
                }
            }
        }
        let product_status = if !product_id.is_empty() {
            reused_products += 1;
            "reused"
        } else if dry_run {
            "would_create"
        } else {
            match client
                .as_ref()
                .unwrap()
                .create_product(&json!({
                    "name": plan_name,
                    "description": (!plan_description.is_empty()).then_some(plan_description),
                    "metadata": {
                        "plan_id": plan_id,
                        "source": "deutsche-deadlock-community.de",
                        "billing": "subscriptions",
                    },
                }))
                .await
            {
                Ok(obj) => {
                    product_id = obj.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                    if product_id.is_empty() {
                        return stripe_fail("stripe_product_id_missing", plan_id, None);
                    }
                    created_products += 1;
                    "created"
                }
                Err(error) => {
                    tracing::warn!(%error, plan_id, "stripe product create failed");
                    return stripe_fail("stripe_product_create_failed", plan_id, None);
                }
            }
        };

        if !product_id.is_empty() {
            product_id_map.insert(plan_id.to_string(), json!(product_id));
        }

        let mut cycle_price_map: Map<String, Value> = Map::new();
        let mut price_reports: Vec<Value> = Vec::new();
        for &cycle in SYNC_CYCLES.iter() {
            let cycle_catalog = catalog_json(cycle);
            let amount_cents = cycle_plan_total_net_cents(&cycle_catalog, plan_id);
            if amount_cents <= 0 {
                continue;
            }
            let lookup_key = format!("deadlock_{plan_id}_{cycle}m_net_v2");

            // 1) Eingecheckter Price-Default → gegen Stripe verifizieren (P2.113).
            //    Live: retrieve_price; schlägt er fehl (gelöscht/ungültig), wird die
            //    ID verworfen und NICHT als reused gezählt → Lookup/Create heilt.
            //    dry_run: kein Stripe-Call, Default gilt unverifiziert als reused.
            let mut price_id = price_id_default(plan_id, cycle).unwrap_or("").to_string();
            let mut price_status = "missing";
            if !price_id.is_empty() {
                if dry_run {
                    reused_prices += 1;
                    price_status = "reused";
                } else if let Some(client) = client.as_ref() {
                    match client.retrieve_price(&price_id).await {
                        Ok(_) => {
                            reused_prices += 1;
                            price_status = "reused";
                        }
                        Err(error) => {
                            tracing::warn!(%error, plan_id, cycle, "stripe price retrieve failed; recreating");
                            price_id.clear();
                        }
                    }
                } else {
                    // Kein Client (nur theoretisch außerhalb dry_run) → nicht verifizierbar.
                    reused_prices += 1;
                    price_status = "reused";
                }
            }

            // 2) Per Lookup-Key suchen (nur live + wenn noch keine ID).
            if price_id.is_empty() && !dry_run {
                if let Some(client) = client.as_ref() {
                    if let Ok(found) = client.list_prices_by_lookup_key(&lookup_key).await {
                        if let Some(first) = found.first() {
                            if let Some(id) = first.get("id").and_then(Value::as_str) {
                                if !id.is_empty() {
                                    price_id = id.to_string();
                                    reused_prices += 1;
                                    price_status = "reused_lookup";
                                }
                            }
                        }
                    }
                }
            }

            // 3) Neu anlegen.
            if price_id.is_empty() {
                if dry_run {
                    price_status = "would_create";
                } else {
                    match client
                        .as_ref()
                        .unwrap()
                        .create_price(&json!({
                            "currency": "eur",
                            "product": product_id,
                            "unit_amount": amount_cents,
                            "recurring": { "interval": "month", "interval_count": cycle },
                            "lookup_key": lookup_key,
                            "metadata": {
                                "plan_id": plan_id,
                                "cycle_months": cycle.to_string(),
                                "source": "deutsche-deadlock-community.de",
                            },
                        }))
                        .await
                    {
                        Ok(obj) => {
                            price_id = obj.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                            if price_id.is_empty() {
                                return stripe_fail("stripe_price_id_missing", plan_id, Some(cycle));
                            }
                            created_prices += 1;
                            price_status = "created";
                        }
                        Err(error) => {
                            tracing::warn!(%error, plan_id, cycle, "stripe price create failed");
                            return stripe_fail("stripe_price_create_failed", plan_id, Some(cycle));
                        }
                    }
                }
            }

            if !price_id.is_empty() {
                cycle_price_map.insert(cycle.to_string(), json!(price_id.clone()));
            }

            price_reports.push(json!({
                "cycle_months": cycle,
                "amount_net_cents": amount_cents,
                "price_id": (!price_id.is_empty()).then_some(price_id),
                "lookup_key": lookup_key,
                "status": price_status,
            }));
        }

        if !cycle_price_map.is_empty() {
            price_id_map.insert(plan_id.to_string(), Value::Object(cycle_price_map));
        }

        operations.push(json!({
            "plan_id": plan_id,
            "name": plan_name,
            "product": {
                "id": (!product_id.is_empty()).then_some(product_id),
                "status": product_status,
            },
            "prices": price_reports,
        }));
    }

    let payload = json!({
        "ok": true,
        "provider": "stripe",
        "dry_run": dry_run,
        // Siehe Modul-Doku: kein host-seitiger Secret-Writer im nativen Pfad; die
        // eingecheckten Defaults sind die Quelle der Wahrheit.
        "persisted_to_windows_vault": false,
        "persist_skipped_reason": "runtime_vault_unavailable_using_checked_in_defaults",
        "created_products": created_products,
        "reused_products": reused_products,
        "created_prices": created_prices,
        "reused_prices": reused_prices,
        "operations": operations,
        // P2.114: ID-Maps + Readiness wieder im Payload (Python-Parität).
        "product_id_map": Value::Object(product_id_map),
        "price_id_map": Value::Object(price_id_map),
        "readiness": readiness_payload(client.is_some()),
    });
    Json(payload).into_response()
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

/// Liest `total_net_cents` des Plans aus dem Zyklus-Katalog (Python
/// `cycle_plan["price"]["total_net_cents"]`).
fn cycle_plan_total_net_cents(cycle_catalog: &Value, plan_id: &str) -> i64 {
    cycle_catalog
        .get("plans")
        .and_then(Value::as_array)
        .and_then(|plans| plans.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(plan_id)))
        .and_then(|p| p.get("price"))
        .and_then(|price| price.get("total_net_cents"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// Baut die Stripe-Readiness für den Sync-Payload (Teilmenge von Pythons
/// `_billing_stripe_readiness_payload`, identisch zu `billing_page::readiness_payload`).
///
/// Keine Secrets. `checkout_ready` = Stripe-Client konfiguriert; `price_map_ready`
/// ist per Konstruktion `true` (eingecheckte Defaults decken alle bezahlten Pläne
/// × {1,12} ab); `webhook_ready` aus dem Vorhandensein des Webhook-Secrets.
fn readiness_payload(checkout_ready: bool) -> Value {
    let webhook_ready = std::env::var("STRIPE_WEBHOOK_SECRET")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("TWITCH_BILLING_STRIPE_WEBHOOK_SECRET")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .is_some();
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

/// Liest das `deleted`-Flag eines Stripe-Objekts robust (Python
/// `bool(_billing_stripe_obj_get(obj, "deleted", False))`): fehlend → `false`,
/// sonst entscheidet die Truthiness des Werts (`true`, jede Zahl ≠ 0, jeder
/// nicht-leere String/Array/Objekt → `true`; `false`/`0`/`""`/`null` → `false`).
fn is_stripe_deleted(obj: &Value) -> bool {
    match obj.get("deleted") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Parst das `dry_run`-Flag aus JSON- ODER Form-Body (Python akzeptiert beides).
fn parse_dry_run(body: &[u8]) -> bool {
    let truthy = |s: &str| matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
    // JSON?
    if let Ok(v) = serde_json::from_slice::<Value>(body) {
        if let Some(b) = v.get("dry_run") {
            return match b {
                Value::Bool(b) => *b,
                Value::String(s) => truthy(s),
                Value::Number(n) => n.as_i64().map(|i| i != 0).unwrap_or(false),
                _ => false,
            };
        }
    }
    // Form-encoded?
    url::form_urlencoded::parse(body)
        .find(|(k, _)| k == "dry_run")
        .map(|(_, v)| truthy(&v))
        .unwrap_or(false)
}

fn stripe_fail(error: &str, plan_id: &str, cycle: Option<u32>) -> Response {
    let mut obj = json!({ "error": error, "plan_id": plan_id, "message": error });
    if let Some(cycle) = cycle {
        obj["cycle_months"] = json!(cycle);
    }
    (StatusCode::BAD_GATEWAY, Json(obj)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_aus_json_und_form() {
        assert!(parse_dry_run(br#"{"dry_run": true}"#));
        assert!(parse_dry_run(br#"{"dry_run": "yes"}"#));
        assert!(!parse_dry_run(br#"{"dry_run": false}"#));
        assert!(parse_dry_run(b"dry_run=1"));
        assert!(parse_dry_run(b"dry_run=on"));
        assert!(!parse_dry_run(b"dry_run=0"));
        assert!(!parse_dry_run(b""));
    }

    #[test]
    fn total_net_cents_aus_katalog() {
        let cat = catalog_json(1);
        // raid_boost: 199 ct/Monat × 1 Monat.
        assert_eq!(cycle_plan_total_net_cents(&cat, "raid_boost"), 199);
        // raid_free ist nicht bezahlt → 0.
        assert_eq!(cycle_plan_total_net_cents(&cat, "raid_free"), 0);
        // Unbekannt → 0.
        assert_eq!(cycle_plan_total_net_cents(&cat, "gibtsnicht"), 0);
    }

    /// dry_run als Admin ohne Stripe-Config: erzeugt nichts, liefert pro bezahltem
    /// Plan einen Operations-Eintrag mit reused/would_create-Status.
    #[tokio::test]
    async fn dry_run_admin_liefert_operations_ohne_stripe_call() {
        use sqlx::postgres::PgPoolOptions;
        // Kein echter Pool nötig — State wird nicht gelesen. Lazy-Connect.
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .unwrap();
        let resp = sync_products_handler(
            DashboardAuthLevel::admin(),
            None,
            State(pool),
            axum::body::Bytes::from_static(br#"{"dry_run": true}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["persisted_to_windows_vault"], false);
        let ops = v["operations"].as_array().unwrap();
        // Genau die 7 bezahlten Pläne (8 Katalog-Pläne minus raid_free).
        assert_eq!(ops.len(), 7);
        // Jeder bezahlte Plan hat Preis-Reports für beide Zyklen.
        for op in ops {
            assert!(!op["prices"].as_array().unwrap().is_empty());
        }
        // P2.114: ID-Maps + Readiness sind im Payload (Python-Parität).
        assert!(v["product_id_map"].is_object(), "product_id_map fehlt");
        assert!(v["price_id_map"].is_object(), "price_id_map fehlt");
        assert!(v["readiness"].is_object(), "readiness fehlt");
        assert_eq!(v["readiness"]["provider"], "stripe");
        // dry_run hat keinen Stripe-Client → checkout_ready=false.
        assert_eq!(v["readiness"]["checkout_ready"], false);
        // Eingecheckte Defaults füllen beide Maps. chat_quiet hat sowohl Product-
        // als auch Price-Default; raid_boost nur Price-Default (kein Product).
        let pmap = v["product_id_map"].as_object().unwrap();
        assert!(pmap.contains_key("chat_quiet"), "product_id_map ohne chat_quiet");
        let prmap = v["price_id_map"].as_object().unwrap();
        let boost_cycles = prmap["raid_boost"].as_object().unwrap();
        assert!(boost_cycles.contains_key("1"), "price_id_map ohne raid_boost/1m");
        assert!(boost_cycles.contains_key("12"), "price_id_map ohne raid_boost/12m");
    }

    /// P2.113: Live-Sync, bei dem `retrieve_price` für eine eingecheckte Default-ID
    /// 404 liefert (gelöschter Preis). Die ID darf NICHT als `reused` zählen; der
    /// Plan muss über Lookup/Create geheilt werden (Status `created`/`reused_lookup`),
    /// und am Ende dürfen `reused_prices` kleiner sein als die Default-Abdeckung.
    #[tokio::test]
    async fn live_recreates_when_price_retrieve_404() {
        use std::sync::Arc;
        use tb_analytics::stripe::StripeClient;
        use wiremock::matchers::{method, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        // Jeder Price-Retrieve (GET /v1/prices/{id}) schlägt mit 404 fehl
        // → ID wird verworfen, nicht als reused gezählt.
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/prices/price_.*"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": {"type": "invalid_request_error", "message": "No such price"}
            })))
            .mount(&server)
            .await;
        // Lookup-Suche liefert nichts → Create-Pfad.
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/prices$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
            .mount(&server)
            .await;
        // Default-Produkte existieren und sind nicht gelöscht → bleiben reused.
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/products/prod_.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "prod_default", "deleted": false })))
            .mount(&server)
            .await;
        // Product-Create (Fallback; bei intakten Defaults nicht aufgerufen).
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/products$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "prod_new" })))
            .mount(&server)
            .await;
        // Price-Create → neuer Preis (Self-Heal).
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/prices$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "price_recreated" })))
            .mount(&server)
            .await;

        let config = BillingPageConfig {
            client: Arc::new(
                StripeClient::new("sk_test_x")
                    .unwrap()
                    .with_api_base(server.uri()),
            ),
            public_origin: "https://example.test".to_string(),
        };

        use sqlx::postgres::PgPoolOptions;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .unwrap();

        let resp = sync_products_handler(
            DashboardAuthLevel::admin(),
            Some(Extension(config)),
            State(pool),
            axum::body::Bytes::from_static(br#"{"dry_run": false}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();

        // Kein Preis darf als 'reused' (verifizierter Default) gezählt werden, da
        // jeder Retrieve fehlschlug.
        assert_eq!(v["reused_prices"], 0, "fehlgeschlagener retrieve zählte als reused");
        // Alle Preise wurden neu angelegt.
        assert!(v["created_prices"].as_u64().unwrap() > 0, "kein Preis recreated");
        // Statt 'reused' steht in den Reports 'created'.
        for op in v["operations"].as_array().unwrap() {
            for price in op["prices"].as_array().unwrap() {
                assert_ne!(price["status"], "reused", "Default galt trotz 404 als reused");
            }
        }
        // Maps + readiness sind weiterhin vorhanden, checkout_ready=true (Client da).
        assert!(v["price_id_map"].is_object());
        assert_eq!(v["readiness"]["checkout_ready"], true);
    }

    #[test]
    fn deleted_flag_robust_gelesen() {
        // Fehlend / null / falsy → nicht gelöscht.
        assert!(!is_stripe_deleted(&json!({})));
        assert!(!is_stripe_deleted(&json!({ "deleted": null })));
        assert!(!is_stripe_deleted(&json!({ "deleted": false })));
        assert!(!is_stripe_deleted(&json!({ "deleted": 0 })));
        assert!(!is_stripe_deleted(&json!({ "deleted": "" })));
        // Truthy → gelöscht.
        assert!(is_stripe_deleted(&json!({ "deleted": true })));
        assert!(is_stripe_deleted(&json!({ "deleted": 1 })));
        assert!(is_stripe_deleted(&json!({ "deleted": "true" })));
    }

    /// P2.113 (Product-Seite): Live-Sync, bei dem `retrieve_product` für eine
    /// eingecheckte Default-Product-ID ein gelöschtes Objekt (`deleted: true`)
    /// liefert. Die ID darf NICHT als `reused` zählen; das Produkt muss über den
    /// Create-Pfad neu angelegt werden (`status: created`).
    #[tokio::test]
    async fn live_recreates_when_product_deleted() {
        use std::sync::Arc;
        use tb_analytics::stripe::StripeClient;
        use wiremock::matchers::{method, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        // Default-Produkt-Retrieve → deleted:true → Default verworfen.
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/products/prod_.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "prod_x", "deleted": true })))
            .mount(&server)
            .await;
        // Product-Create → neues Produkt (Self-Heal).
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/products$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "prod_recreated" })))
            .mount(&server)
            .await;
        // Preise: Default-Retrieve OK → bleiben reused (Preis-Pfad nicht im Fokus).
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/prices/price_.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "price_ok" })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/prices$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/prices$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "price_created" })))
            .mount(&server)
            .await;

        let config = BillingPageConfig {
            client: Arc::new(
                StripeClient::new("sk_test_x")
                    .unwrap()
                    .with_api_base(server.uri()),
            ),
            public_origin: "https://example.test".to_string(),
        };

        use sqlx::postgres::PgPoolOptions;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .unwrap();

        let resp = sync_products_handler(
            DashboardAuthLevel::admin(),
            Some(Extension(config)),
            State(pool),
            axum::body::Bytes::from_static(br#"{"dry_run": false}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();

        // Kein Produkt darf als verifizierter Default 'reused' zählen.
        assert_eq!(v["reused_products"], 0, "gelöschter Default zählte als reused");
        // Die Default-Produkte (4 Pläne) wurden neu angelegt.
        assert!(
            v["created_products"].as_u64().unwrap() >= 4,
            "gelöschte Produkte nicht recreated"
        );
        // Kein Operations-Eintrag trägt product.status == reused.
        for op in v["operations"].as_array().unwrap() {
            assert_ne!(op["product"]["status"], "reused", "Default galt trotz deleted als reused");
        }
    }

    /// P2.113 (Product-Seite): Ein transienter 5xx bei `retrieve_product` darf den
    /// Sync NICHT abbrechen — die ID wird geleert und der Create-Pfad heilt.
    #[tokio::test]
    async fn live_recreates_when_product_retrieve_errors() {
        use std::sync::Arc;
        use tb_analytics::stripe::StripeClient;
        use wiremock::matchers::{method, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        // Default-Produkt-Retrieve → 500 (transient) → ID geleert, kein Abbruch.
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/products/prod_.*"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": {"type": "api_error", "message": "boom"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/products$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "prod_recreated" })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/prices/price_.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "price_ok" })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/prices$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/prices$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "price_created" })))
            .mount(&server)
            .await;

        let config = BillingPageConfig {
            client: Arc::new(
                StripeClient::new("sk_test_x")
                    .unwrap()
                    .with_api_base(server.uri()),
            ),
            public_origin: "https://example.test".to_string(),
        };

        use sqlx::postgres::PgPoolOptions;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .unwrap();

        let resp = sync_products_handler(
            DashboardAuthLevel::admin(),
            Some(Extension(config)),
            State(pool),
            axum::body::Bytes::from_static(br#"{"dry_run": false}"#),
        )
        .await;
        // Kein Abbruch: weiterhin 200 OK trotz 5xx beim Retrieve.
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["reused_products"], 0, "transienter Fehler zählte als reused");
        assert!(
            v["created_products"].as_u64().unwrap() >= 4,
            "Produkte nach Retrieve-Fehler nicht recreated"
        );
    }

    #[tokio::test]
    async fn nicht_admin_401() {
        use sqlx::postgres::PgPoolOptions;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .unwrap();
        let resp = sync_products_handler(
            DashboardAuthLevel::None,
            None,
            State(pool),
            axum::body::Bytes::from_static(b"{}"),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

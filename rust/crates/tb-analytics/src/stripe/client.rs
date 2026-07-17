//! Nativer Stripe-HTTP-Client (Ersatz für das `stripe`-Python-SDK).
//!
//! Deckt die vom Billing-/Affiliate-Pfad genutzten Endpunkte ab: Checkout-Session,
//! Subscription (retrieve/cancel), Customer-Portal, Customer, Product/Price und
//! die Stripe-Connect-OAuth-/Transfer-Oberfläche (MVP). Alle Schreib-Calls gehen
//! formularkodiert mit Stripes Bracket-Notation (`recurring[interval]=month`),
//! exakt wie Pythons REST-Fallback in `routes_mixin._billing_form_pairs`.
//!
//! Der Secret-Key wird nur im `Authorization`-Header mitgesendet und niemals
//! geloggt oder in [`StripeError`] transportiert.

use serde_json::Value;
use std::time::Duration;

const DEFAULT_API_BASE: &str = "https://api.stripe.com";
const DEFAULT_CONNECT_BASE: &str = "https://connect.stripe.com";
const DEFAULT_TIMEOUT_SECS: u64 = 25;

/// Fehler des Stripe-Clients. Enthält **keine** Secrets.
#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    /// Kein Secret-Key konfiguriert.
    #[error("stripe secret key missing")]
    SecretKeyMissing,
    /// HTTP-/Transport-Fehler (reqwest leakt keine Header).
    #[error("stripe transport error")]
    Transport(#[from] reqwest::Error),
    /// Stripe antwortete mit Nicht-2xx. `stripe_type` ist der `error.type` der Antwort.
    #[error("stripe api error: http {status}")]
    Api {
        /// HTTP-Statuscode der Antwort.
        status: u16,
        /// `error.type` aus dem Stripe-Fehler-Payload, falls vorhanden.
        stripe_type: Option<String>,
    },
    /// Antwort war kein JSON-Objekt bzw. nicht parsebar.
    #[error("invalid stripe response")]
    InvalidResponse,
    /// Erfolgs-Antwort enthielt keine `id`.
    #[error("stripe response missing id")]
    MissingId,
}

/// Stripe-HTTP-Client. Klonbar (teilt den `reqwest::Client`-Pool).
#[derive(Clone)]
pub struct StripeClient {
    http: reqwest::Client,
    api_base: String,
    connect_base: String,
    secret_key: String,
}

impl StripeClient {
    /// Erzeugt einen Client mit Default-Endpunkten und 25 s Timeout.
    ///
    /// Gibt [`StripeError::SecretKeyMissing`] zurück, wenn `secret_key` leer ist.
    pub fn new(secret_key: impl Into<String>) -> Result<Self, StripeError> {
        let secret_key = secret_key.into();
        if secret_key.trim().is_empty() {
            return Err(StripeError::SecretKeyMissing);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()?;
        Ok(Self {
            http,
            api_base: DEFAULT_API_BASE.to_string(),
            connect_base: DEFAULT_CONNECT_BASE.to_string(),
            secret_key,
        })
    }

    /// Überschreibt die API-Basis-URL (`https://api.stripe.com`) — für Tests (wiremock).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into().trim_end_matches('/').to_string();
        self
    }

    /// Überschreibt die Connect-Basis-URL (`https://connect.stripe.com`) — für Tests.
    pub fn with_connect_base(mut self, base: impl Into<String>) -> Self {
        self.connect_base = base.into().trim_end_matches('/').to_string();
        self
    }

    // -- gemeinsame Transport-Helfer ------------------------------------- //

    async fn post_form(
        &self,
        url: &str,
        params: &[(String, String)],
        idempotency_key: Option<&str>,
    ) -> Result<Value, StripeError> {
        let mut req = self
            .http
            .post(url)
            .bearer_auth(&self.secret_key)
            .form(params);
        if let Some(key) = idempotency_key.filter(|k| !k.is_empty()) {
            req = req.header("Idempotency-Key", key);
        }
        let resp = req.send().await?;
        Self::parse_response(resp).await
    }

    async fn get(&self, url: &str, query: &[(String, String)]) -> Result<Value, StripeError> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.secret_key)
            .query(query)
            .send()
            .await?;
        Self::parse_response(resp).await
    }

    async fn parse_response(resp: reqwest::Response) -> Result<Value, StripeError> {
        let status = resp.status();
        let text = resp.text().await?;
        let value: Value = serde_json::from_str(&text).map_err(|_| StripeError::InvalidResponse)?;
        if !status.is_success() {
            let stripe_type = value
                .get("error")
                .and_then(|e| e.get("type"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
            return Err(StripeError::Api {
                status: status.as_u16(),
                stripe_type,
            });
        }
        Ok(value)
    }

    fn ensure_id(value: Value) -> Result<Value, StripeError> {
        match value.get("id").and_then(|v| v.as_str()) {
            Some(id) if !id.trim().is_empty() => Ok(value),
            _ => Err(StripeError::MissingId),
        }
    }

    // -- Checkout --------------------------------------------------------- //

    /// Erstellt eine Checkout-Session (`POST /v1/checkout/sessions`).
    ///
    /// `params` ist das Session-Objekt als JSON; es wird in Stripes
    /// Bracket-Notation formularkodiert. Die zurückgegebene Session muss eine
    /// `id` tragen.
    pub async fn create_checkout_session(
        &self,
        params: &Value,
        idempotency_key: Option<&str>,
    ) -> Result<Value, StripeError> {
        let url = format!("{}/v1/checkout/sessions", self.api_base);
        let value = self
            .post_form(&url, &form_pairs(params), idempotency_key)
            .await?;
        Self::ensure_id(value)
    }

    // -- Subscription ----------------------------------------------------- //

    /// Liest eine Subscription (`GET /v1/subscriptions/{id}`).
    pub async fn retrieve_subscription(&self, subscription_id: &str) -> Result<Value, StripeError> {
        let url = format!("{}/v1/subscriptions/{subscription_id}", self.api_base);
        self.get(&url, &[]).await
    }

    /// Markiert eine Subscription zum Kündigen am Periodenende
    /// (`POST /v1/subscriptions/{id}`, `cancel_at_period_end=true`,
    /// `proration_behavior=none`).
    pub async fn cancel_subscription_at_period_end(
        &self,
        subscription_id: &str,
    ) -> Result<Value, StripeError> {
        let url = format!("{}/v1/subscriptions/{subscription_id}", self.api_base);
        let params = vec![
            ("cancel_at_period_end".to_string(), "true".to_string()),
            ("proration_behavior".to_string(), "none".to_string()),
        ];
        self.post_form(&url, &params, None).await
    }

    // -- Customer / Portal ------------------------------------------------ //

    /// Liest einen Customer (`GET /v1/customers/{id}`).
    pub async fn retrieve_customer(&self, customer_id: &str) -> Result<Value, StripeError> {
        let url = format!("{}/v1/customers/{customer_id}", self.api_base);
        self.get(&url, &[]).await
    }

    /// Erstellt eine Customer-Portal-Session (`POST /v1/billing_portal/sessions`).
    pub async fn create_billing_portal_session(
        &self,
        customer_id: &str,
        return_url: &str,
    ) -> Result<Value, StripeError> {
        let url = format!("{}/v1/billing_portal/sessions", self.api_base);
        let params = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("return_url".to_string(), return_url.to_string()),
        ];
        let value = self.post_form(&url, &params, None).await?;
        Self::ensure_id(value)
    }

    // -- Product / Price -------------------------------------------------- //

    /// Erstellt ein Produkt (`POST /v1/products`). `params` als JSON-Objekt.
    pub async fn create_product(&self, params: &Value) -> Result<Value, StripeError> {
        let url = format!("{}/v1/products", self.api_base);
        let value = self.post_form(&url, &form_pairs(params), None).await?;
        Self::ensure_id(value)
    }

    /// Liest ein Produkt (`GET /v1/products/{id}`).
    pub async fn retrieve_product(&self, product_id: &str) -> Result<Value, StripeError> {
        let url = format!("{}/v1/products/{product_id}", self.api_base);
        self.get(&url, &[]).await
    }

    /// Erstellt einen Preis (`POST /v1/prices`). `params` als JSON-Objekt
    /// (inkl. `recurring`, `metadata`, `lookup_key`).
    pub async fn create_price(&self, params: &Value) -> Result<Value, StripeError> {
        let url = format!("{}/v1/prices", self.api_base);
        let value = self.post_form(&url, &form_pairs(params), None).await?;
        Self::ensure_id(value)
    }

    /// Liest einen Preis (`GET /v1/prices/{id}`).
    pub async fn retrieve_price(&self, price_id: &str) -> Result<Value, StripeError> {
        let url = format!("{}/v1/prices/{price_id}", self.api_base);
        self.get(&url, &[]).await
    }

    /// Listet aktive Preise zu einem Lookup-Key (`GET /v1/prices`, limit 1).
    /// Liefert das `data`-Array der Antwort.
    pub async fn list_prices_by_lookup_key(
        &self,
        lookup_key: &str,
    ) -> Result<Vec<Value>, StripeError> {
        let url = format!("{}/v1/prices", self.api_base);
        let query = vec![
            ("active".to_string(), "true".to_string()),
            ("lookup_keys[0]".to_string(), lookup_key.to_string()),
            ("limit".to_string(), "1".to_string()),
        ];
        let value = self.get(&url, &query).await?;
        Ok(value
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default())
    }

    // -- Connect-OAuth + Transfer (MVP-Stubs) ----------------------------- //

    /// Baut die Stripe-Connect-Authorize-URL (reine String-Erzeugung, kein HTTP).
    /// Entspricht `STRIPE_CONNECT_AUTHORIZE_URL` mit `scope=read_write`.
    pub fn connect_authorize_url(
        &self,
        client_id: &str,
        redirect_uri: &str,
        state: &str,
    ) -> String {
        let query = form_urlencode(&[
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("scope", "read_write"),
            ("state", state),
        ]);
        format!("{}/oauth/authorize?{}", self.connect_base, query)
    }

    /// Tauscht einen Connect-OAuth-`code` gegen Account-Daten
    /// (`POST {connect_base}/oauth/token`). MVP-Stub: liefert das rohe
    /// Antwort-Objekt (Caller liest `stripe_user_id`).
    pub async fn exchange_connect_oauth_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<Value, StripeError> {
        let url = format!("{}/oauth/token", self.connect_base);
        let params = vec![
            ("client_secret".to_string(), self.secret_key.clone()),
            ("code".to_string(), code.to_string()),
            ("grant_type".to_string(), "authorization_code".to_string()),
            ("redirect_uri".to_string(), redirect_uri.to_string()),
        ];
        self.post_form(&url, &params, None).await
    }

    /// Erstellt einen Connect-Transfer (`POST /v1/transfers`). MVP-Stub für die
    /// Affiliate-Provisionsauszahlung; `transfer_group` korreliert mit dem Event.
    pub async fn create_transfer(
        &self,
        amount_cents: u64,
        currency: &str,
        destination_account: &str,
        transfer_group: &str,
        idempotency_key: Option<&str>,
    ) -> Result<Value, StripeError> {
        let url = format!("{}/v1/transfers", self.api_base);
        let params = vec![
            ("amount".to_string(), amount_cents.to_string()),
            ("currency".to_string(), currency.to_string()),
            ("destination".to_string(), destination_account.to_string()),
            ("transfer_group".to_string(), transfer_group.to_string()),
        ];
        let value = self.post_form(&url, &params, idempotency_key).await?;
        Self::ensure_id(value)
    }
}

/// Flacht ein JSON-Objekt in Stripes Form-Encoding-Paare ab (Bracket-Notation).
///
/// Portierung von `routes_mixin._billing_form_pairs`: Dicts → `parent[key]`,
/// Listen → `parent[index]`, Bools → `"true"`/`"false"`, Zahlen/Strings → ihr
/// String-Wert. `null` und leere Keys werden ausgelassen.
pub fn form_pairs(value: &Value) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    flatten(value, "", &mut pairs);
    pairs
}

fn flatten(value: &Value, prefix: &str, pairs: &mut Vec<(String, String)>) {
    match value {
        Value::Null => {}
        Value::Object(map) => {
            for (key, child) in map {
                let key = key.trim();
                if key.is_empty() {
                    continue;
                }
                let child_prefix = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}[{key}]")
                };
                flatten(child, &child_prefix, pairs);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                let child_prefix = format!("{prefix}[{index}]");
                flatten(child, &child_prefix, pairs);
            }
        }
        _ => {
            if prefix.is_empty() {
                return;
            }
            let rendered = match value {
                Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            pairs.push((prefix.to_string(), rendered));
        }
    }
}

fn form_urlencode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Minimales `application/x-www-form-urlencoded`-Component-Encoding (RFC 3986
/// unreserved bleiben, Rest wird `%XX`-kodiert, Space → `%20`).
fn encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn missing_secret_key_is_rejected() {
        assert!(matches!(
            StripeClient::new(""),
            Err(StripeError::SecretKeyMissing)
        ));
        assert!(matches!(
            StripeClient::new("   "),
            Err(StripeError::SecretKeyMissing)
        ));
        assert!(StripeClient::new("sk_test_x").is_ok());
    }

    #[test]
    fn form_pairs_flattens_nested_objects_and_arrays() {
        let params = json!({
            "mode": "subscription",
            "success_url": "https://example.test/ok",
            "recurring": {"interval": "month", "interval_count": 12},
            "line_items": [{"price": "price_123", "quantity": 1}],
            "automatic_tax": {"enabled": true},
            "ignored_null": null,
        });
        let pairs = form_pairs(&params);
        let has = |k: &str, v: &str| pairs.iter().any(|(pk, pv)| pk == k && pv == v);

        assert!(has("mode", "subscription"));
        assert!(has("recurring[interval]", "month"));
        assert!(has("recurring[interval_count]", "12"));
        assert!(has("line_items[0][price]", "price_123"));
        assert!(has("line_items[0][quantity]", "1"));
        assert!(has("automatic_tax[enabled]", "true"));
        assert!(!pairs.iter().any(|(k, _)| k.starts_with("ignored_null")));
    }

    #[tokio::test]
    async fn create_checkout_session_posts_form_and_returns_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/checkout/sessions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cs_test_123",
                "object": "checkout.session",
                "url": "https://checkout.stripe.com/c/pay/cs_test_123",
            })))
            .mount(&server)
            .await;

        let client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_api_base(server.uri());

        let params = json!({
            "mode": "subscription",
            "line_items": [{"price": "price_123", "quantity": 1}],
            "success_url": "https://example.test/ok",
            "cancel_url": "https://example.test/cancel",
        });
        let session = client
            .create_checkout_session(&params, Some("idem-key-1"))
            .await
            .expect("checkout session created");

        assert_eq!(
            session.get("id").and_then(|v| v.as_str()),
            Some("cs_test_123")
        );
        assert_eq!(
            session.get("url").and_then(|v| v.as_str()),
            Some("https://checkout.stripe.com/c/pay/cs_test_123")
        );
    }

    #[tokio::test]
    async fn create_checkout_session_maps_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/checkout/sessions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {"type": "invalid_request_error", "message": "boom"}
            })))
            .mount(&server)
            .await;

        let client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_api_base(server.uri());

        let err = client
            .create_checkout_session(&json!({"mode": "subscription"}), None)
            .await
            .expect_err("should fail");
        match err {
            StripeError::Api {
                status,
                stripe_type,
            } => {
                assert_eq!(status, 400);
                assert_eq!(stripe_type.as_deref(), Some("invalid_request_error"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancel_subscription_sends_period_end_flags() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/sub_123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sub_123",
                "cancel_at_period_end": true,
            })))
            .mount(&server)
            .await;

        let client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_api_base(server.uri());

        let sub = client
            .cancel_subscription_at_period_end("sub_123")
            .await
            .expect("cancel ok");
        assert_eq!(
            sub.get("cancel_at_period_end").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn connect_authorize_url_contains_scope_and_state() {
        let client = StripeClient::new("sk_test_secret").unwrap();
        let url = client.connect_authorize_url("ca_client", "https://app.test/cb", "state123");
        assert!(url.starts_with("https://connect.stripe.com/oauth/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=ca_client"));
        assert!(url.contains("scope=read_write"));
        assert!(url.contains("state=state123"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fapp.test%2Fcb"));
    }
}

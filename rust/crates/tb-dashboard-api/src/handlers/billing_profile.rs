//! Rechnungsempfänger-Profil: Persistenz + Stripe-Customer-Prefill (B2-P1).
//!
//! Port von `bot/dashboard/abbo_billing_routes.py:abbo_profile_save` (Write) +
//! `billing_mixin.py:_billing_profile_for_request` /
//! `_billing_profile_from_stripe_customer` / `_billing_prefill_profile_from_stripe`
//! (Read/Prefill, Zeilen 893-1108).
//!
//! - `POST /twitch/abbo/rechnungsdaten` — speichert das Rechnungsprofil des
//!   eingeloggten Partners in `twitch_billing_profiles` und redirected auf
//!   `/twitch/pricing?cycle={cycle}&profile={saved|invalid|error}`. Form-POST mit
//!   `csrf_token` im Body (Legacy-Client) → in-handler CSRF-Validierung.
//! - [`resolve_profile`] — lädt das persistierte Profil (oder den Session-Default)
//!   und füllt leere Felder aus dem Stripe-Customer (Name/E-Mail/Adresse/VAT) auf;
//!   wird vom Katalog-Endpoint (billing_page) für die UI-Vorbelegung genutzt.

use axum::{
    extract::{Extension, RawForm, State},
    response::{IntoResponse, Redirect, Response},
};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME, PARTNER_COOKIE_NAME};
use crate::handlers::billing_page::BillingPageConfig;

use tb_analytics::billing::normalize_billing_cycle;

/// Feld-Längengrenzen (Python `abbo_profile_save`).
const MAX_NAME: usize = 180;
const MAX_EMAIL: usize = 180;
const MAX_COMPANY: usize = 200;
const MAX_STREET: usize = 200;
const MAX_POSTAL: usize = 32;
const MAX_CITY: usize = 120;
const MAX_VAT: usize = 60;

/// `POST /twitch/abbo/rechnungsdaten` — Rechnungsprofil speichern.
pub async fn profile_save_handler(
    auth: DashboardAuthLevel,
    auth_state: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    let cycle = normalize_billing_cycle(
        form_get(&form, "cycle").trim().parse::<u32>().unwrap_or(1),
    );

    // Auth-Gate: nur eingeloggter Partner/Admin.
    let Some(customer_reference) = customer_reference_for(&auth) else {
        return Redirect::to("/twitch/auth/login?next=%2Ftwitch%2Fpricing").into_response();
    };

    // CSRF aus dem Form-Body.
    if !verify_form_csrf(auth_state.as_ref(), &headers, &form).await {
        return profile_redirect(cycle, "error");
    }

    let recipient_name = clip(form_get(&form, "recipient_name"), MAX_NAME);
    let recipient_email = clip(form_get(&form, "recipient_email"), MAX_EMAIL);
    let company_name = clip(form_get(&form, "company_name"), MAX_COMPANY);
    let street_line1 = clip(form_get(&form, "street_line1"), MAX_STREET);
    let postal_code = clip(form_get(&form, "postal_code"), MAX_POSTAL);
    let city = clip(form_get(&form, "city"), MAX_CITY);
    let country_code = clip(form_get(&form, "country_code"), 2).to_uppercase();
    let vat_id = clip(form_get(&form, "vat_id"), MAX_VAT);

    // Pflichtfelder (Python: alle außer company_name + vat_id).
    if recipient_name.is_empty()
        || recipient_email.is_empty()
        || street_line1.is_empty()
        || postal_code.is_empty()
        || city.is_empty()
        || country_code.is_empty()
    {
        return profile_redirect(cycle, "invalid");
    }

    let profile = BillingProfile {
        customer_reference: customer_reference.clone(),
        recipient_name,
        recipient_email,
        company_name,
        street_line1,
        postal_code,
        city,
        country_code,
        vat_id,
    };

    match upsert_profile(&pool, &profile).await {
        Ok(()) => profile_redirect(cycle, "saved"),
        Err(error) => {
            tracing::error!(%error, "billing profile save failed");
            profile_redirect(cycle, "error")
        }
    }
}

// ── Persistiertes Profil (Read/Upsert) ───────────────────────────────────────

/// Rechnungsempfänger-Profil (Python `_billing_profile_for_request`-Shape).
#[derive(Debug, Clone, Default)]
pub struct BillingProfile {
    pub customer_reference: String,
    pub recipient_name: String,
    pub recipient_email: String,
    pub company_name: String,
    pub street_line1: String,
    pub postal_code: String,
    pub city: String,
    pub country_code: String,
    pub vat_id: String,
}

impl BillingProfile {
    /// JSON-Shape für die Katalog-/UI-Antwort.
    pub fn to_json(&self) -> Value {
        json!({
            "customer_reference": self.customer_reference,
            "recipient_name": self.recipient_name,
            "recipient_email": self.recipient_email,
            "company_name": self.company_name,
            "street_line1": self.street_line1,
            "postal_code": self.postal_code,
            "city": self.city,
            "country_code": self.country_code,
            "vat_id": self.vat_id,
        })
    }
}

/// Upsert in `twitch_billing_profiles` (Python `_billing_upsert_profile`).
pub async fn upsert_profile(pool: &PgPool, profile: &BillingProfile) -> Result<(), sqlx::Error> {
    let reference = profile.customer_reference.trim();
    if reference.is_empty() {
        return Ok(());
    }
    let now_iso = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
    sqlx::query(
        r#"
        INSERT INTO twitch_billing_profiles (
            customer_reference, recipient_name, recipient_email, company_name,
            street_line1, postal_code, city, country_code, vat_id, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (customer_reference) DO UPDATE SET
            recipient_name = EXCLUDED.recipient_name,
            recipient_email = EXCLUDED.recipient_email,
            company_name = EXCLUDED.company_name,
            street_line1 = EXCLUDED.street_line1,
            postal_code = EXCLUDED.postal_code,
            city = EXCLUDED.city,
            country_code = EXCLUDED.country_code,
            vat_id = EXCLUDED.vat_id,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(reference)
    .bind(profile.recipient_name.trim())
    .bind(profile.recipient_email.trim())
    .bind(profile.company_name.trim())
    .bind(profile.street_line1.trim())
    .bind(profile.postal_code.trim())
    .bind(profile.city.trim())
    .bind(profile.country_code.trim().to_uppercase())
    .bind(profile.vat_id.trim())
    .bind(&now_iso)
    .execute(pool)
    .await?;
    Ok(())
}

/// Lädt das persistierte Profil für eine Customer-Reference (Python
/// `_billing_profile_for_request` DB-Teil). `None` → noch kein Profil gespeichert.
pub async fn load_profile(
    pool: &PgPool,
    customer_reference: &str,
) -> Result<Option<BillingProfile>, sqlx::Error> {
    let reference = customer_reference.trim();
    if reference.is_empty() {
        return Ok(None);
    }
    // 9 TEXT-Spalten der Profil-Zeile (reference + 8 Felder).
    type ProfileRow = (String, String, String, String, String, String, String, String, String);
    let row: Option<ProfileRow> =
        sqlx::query_as(
            r#"
            SELECT customer_reference, recipient_name, recipient_email, company_name,
                   street_line1, postal_code, city, country_code, vat_id
            FROM twitch_billing_profiles
            WHERE LOWER(customer_reference) = LOWER($1)
            LIMIT 1
            "#,
        )
        .bind(reference)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| BillingProfile {
        customer_reference: r.0,
        recipient_name: r.1,
        recipient_email: r.2,
        company_name: r.3,
        street_line1: r.4,
        postal_code: r.5,
        city: r.6,
        country_code: if r.7.trim().is_empty() { "DE".into() } else { r.7.to_uppercase() },
        vat_id: r.8,
    }))
}

/// Auflösung des UI-Profils inkl. Stripe-Customer-Prefill (Python
/// `_billing_profile_for_request` + `_billing_prefill_profile_from_stripe`).
///
/// Reihenfolge:
/// 1. Default aus dem Session-Kontext (`recipient_name` = Anzeigename/Login).
/// 2. Persistiertes Profil aus `twitch_billing_profiles` überschreibt Default.
/// 3. Leere Felder werden aus dem Stripe-Customer (falls Customer-ID + Config
///    vorhanden) aufgefüllt. Belegte Felder bleiben unverändert (kein Overwrite).
///
/// Gibt `(profile_json, imported_fields)` zurück — `imported_fields` listet die
/// aus Stripe übernommenen Schlüssel (für einen UI-Hinweis).
pub async fn resolve_profile(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    config: Option<&BillingPageConfig>,
    stripe_customer_id: Option<&str>,
) -> (Value, Vec<String>) {
    let customer_reference = customer_reference_for(auth).unwrap_or_default();
    let default_name = display_name_for(auth);

    let mut profile = match load_profile(pool, &customer_reference).await {
        Ok(Some(mut p)) => {
            if p.recipient_name.trim().is_empty() {
                p.recipient_name = default_name;
            }
            p
        }
        _ => BillingProfile {
            customer_reference: customer_reference.clone(),
            recipient_name: default_name,
            country_code: "DE".into(),
            ..Default::default()
        },
    };

    // Stripe-Prefill nur, wenn Customer-ID + Stripe-Client vorhanden.
    let mut imported: Vec<String> = Vec::new();
    if let (Some(customer_id), Some(cfg)) =
        (stripe_customer_id.map(str::trim).filter(|s| !s.is_empty()), config)
    {
        if let Ok(stripe_profile) = fetch_stripe_profile(cfg, customer_id).await {
            imported = prefill_from_stripe(&mut profile, &stripe_profile);
        }
    }

    (profile.to_json(), imported)
}

/// Liest rechnungsrelevante Felder aus dem Stripe-Customer
/// (Python `_billing_profile_from_stripe_customer`).
async fn fetch_stripe_profile(
    config: &BillingPageConfig,
    customer_id: &str,
) -> Result<BillingProfile, ()> {
    let customer = config
        .client
        .retrieve_customer(customer_id)
        .await
        .map_err(|error| {
            tracing::debug!(%error, "billing stripe customer lookup failed");
        })?;

    let str_at = |obj: &Value, key: &str| -> String {
        obj.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string()
    };
    let address = {
        let billing = customer.get("address").cloned().unwrap_or(Value::Null);
        if billing.is_object() {
            billing
        } else {
            customer
                .get("shipping")
                .and_then(|s| s.get("address"))
                .cloned()
                .unwrap_or(Value::Null)
        }
    };
    let metadata = customer.get("metadata").cloned().unwrap_or(Value::Null);
    let shipping_name = customer
        .get("shipping")
        .map(|s| str_at(s, "name"))
        .unwrap_or_default();

    let country = str_at(&address, "country");
    Ok(BillingProfile {
        customer_reference: String::new(),
        recipient_name: {
            let n = str_at(&customer, "name");
            if n.is_empty() { shipping_name } else { n }
        },
        recipient_email: str_at(&customer, "email"),
        company_name: {
            let c = str_at(&metadata, "company_name");
            if c.is_empty() { str_at(&metadata, "company") } else { c }
        },
        street_line1: str_at(&address, "line1"),
        postal_code: str_at(&address, "postal_code"),
        city: str_at(&address, "city"),
        country_code: if country.is_empty() { "DE".into() } else { country.to_uppercase() },
        vat_id: String::new(),
    })
}

/// Füllt leere Profilfelder aus dem Stripe-Profil (Python
/// `_billing_prefill_profile_from_stripe`). Gibt die importierten Schlüssel zurück.
fn prefill_from_stripe(profile: &mut BillingProfile, stripe: &BillingProfile) -> Vec<String> {
    let mut imported = Vec::new();
    macro_rules! fill {
        ($field:ident, $key:literal) => {
            if profile.$field.trim().is_empty() && !stripe.$field.trim().is_empty() {
                profile.$field = stripe.$field.trim().to_string();
                imported.push($key.to_string());
            }
        };
    }
    fill!(recipient_name, "recipient_name");
    fill!(recipient_email, "recipient_email");
    fill!(company_name, "company_name");
    fill!(street_line1, "street_line1");
    fill!(postal_code, "postal_code");
    fill!(city, "city");
    // country_code: nur überschreiben, wenn aktuell der reine Default "DE" steht
    // und Stripe ein abweichendes Land liefert (Python prüft auf Leere; unser
    // Default ist "DE", daher zusätzlich auf den Default-Wert prüfen).
    if (profile.country_code.trim().is_empty() || profile.country_code == "DE")
        && !stripe.country_code.trim().is_empty()
        && stripe.country_code != "DE"
    {
        profile.country_code = stripe.country_code.clone();
        imported.push("country_code".to_string());
    }
    fill!(vat_id, "vat_id");
    imported
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

/// Customer-Reference des eingeloggten Nutzers (Login bevorzugt, sonst User-ID).
fn customer_reference_for(auth: &DashboardAuthLevel) -> Option<String> {
    if let DashboardAuthLevel::Partner { twitch_login, twitch_user_id, .. } = auth {
        let login = twitch_login.trim();
        if !login.is_empty() {
            return Some(login.to_string());
        }
        let uid = twitch_user_id.trim();
        if !uid.is_empty() {
            return Some(uid.to_string());
        }
    }
    None
}

/// Anzeigename für den Profil-Default (Python: display_name → login → Fallback).
fn display_name_for(auth: &DashboardAuthLevel) -> String {
    if let DashboardAuthLevel::Partner { twitch_login, display_name, .. } = auth {
        let dn = display_name.trim();
        if !dn.is_empty() {
            return dn.to_string();
        }
        let login = twitch_login.trim();
        if !login.is_empty() {
            return login.to_string();
        }
    }
    "Streamer Partner".to_string()
}

/// Validiert das Form-Body-CSRF-Token gegen die Session (Admin- vor Partner-Cookie).
async fn verify_form_csrf(
    auth_state: Option<&Extension<DashboardAuthState>>,
    headers: &axum::http::HeaderMap,
    form: &[(String, String)],
) -> bool {
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
    let (cookie, session_type) = if let Some(c) = read(ADMIN_COOKIE_NAME).filter(|s| !s.is_empty()) {
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

fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn form_get<'a>(form: &'a [(String, String)], key: &str) -> &'a str {
    form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
}

/// Trimmt + kürzt auf `max` Zeichen (Python `str(...).strip()[:max]`).
fn clip(raw: &str, max: usize) -> String {
    raw.trim().chars().take(max).collect()
}

fn profile_redirect(cycle: u32, status: &str) -> Response {
    Redirect::to(&format!("/twitch/pricing?cycle={cycle}&profile={status}")).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: "42".into(),
            display_name: "Nani".into(),
        }
    }

    #[test]
    fn clip_trimmt_und_kuerzt() {
        assert_eq!(clip("  hallo  ", 10), "hallo");
        assert_eq!(clip("abcdef", 3), "abc");
    }

    #[test]
    fn reference_und_default_name() {
        assert_eq!(customer_reference_for(&partner("nani")).as_deref(), Some("nani"));
        assert_eq!(display_name_for(&partner("nani")), "Nani");
        assert_eq!(display_name_for(&DashboardAuthLevel::admin()), "Streamer Partner");
        assert_eq!(customer_reference_for(&DashboardAuthLevel::admin()), None);
    }

    #[test]
    fn prefill_fuellt_nur_leere_felder() {
        let mut profile = BillingProfile {
            recipient_name: "Bestehend".into(),
            country_code: "DE".into(),
            ..Default::default()
        };
        let stripe = BillingProfile {
            recipient_name: "Stripe Name".into(),
            recipient_email: "a@b.de".into(),
            city: "Berlin".into(),
            country_code: "AT".into(),
            ..Default::default()
        };
        let imported = prefill_from_stripe(&mut profile, &stripe);
        // recipient_name war belegt → bleibt.
        assert_eq!(profile.recipient_name, "Bestehend");
        // leere Felder aufgefüllt.
        assert_eq!(profile.recipient_email, "a@b.de");
        assert_eq!(profile.city, "Berlin");
        // country DE→AT übernommen.
        assert_eq!(profile.country_code, "AT");
        assert!(imported.contains(&"recipient_email".to_string()));
        assert!(imported.contains(&"country_code".to_string()));
        assert!(!imported.contains(&"recipient_name".to_string()));
    }

    #[test]
    fn redirect_traegt_cycle_und_status() {
        let resp = profile_redirect(12, "saved");
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/twitch/pricing?cycle=12&profile=saved");
    }
}

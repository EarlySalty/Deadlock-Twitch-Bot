//! Öffentliche Absender- und Rechnungsstellerdaten; SMTP-Zugangsdaten bleiben im Secretloader.
use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AffiliateMailOptions {
    pub host: Option<String>,
    pub port: u16,
    pub from_email: Option<String>,
    pub from_name: String,
    pub starttls: bool,
    pub use_ssl: bool,
}
impl Default for AffiliateMailOptions {
    fn default() -> Self {
        Self {
            host: None,
            port: 587,
            from_email: None,
            from_name: "Deadlock Partner Network".into(),
            starttls: true,
            use_ssl: false,
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AffiliateSellerOptions {
    pub name: String,
    pub company: String,
    pub street: String,
    pub postal_code: String,
    pub city: String,
    pub country: String,
    pub email: String,
    pub website: Option<String>,
    pub tax_id: String,
}
impl Default for AffiliateSellerOptions {
    fn default() -> Self {
        Self {
            name: "[STEUERBERATER: Firmenname]".into(),
            company: "[STEUERBERATER: Firmierung]".into(),
            street: "[STEUERBERATER: Adresse]".into(),
            postal_code: String::new(),
            city: String::new(),
            country: "DE".into(),
            email: "billing@example.invalid".into(),
            website: None,
            tax_id: "[STEUERBERATER: Steuernummer/USt-IdNr.]".into(),
        }
    }
}

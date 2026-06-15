//! Affiliate-PII (verschlüsselte Stammdaten) + Gutschrift-Readiness.
//!
//! Port von `bot/dashboard/affiliate/affiliate_pii.py:AffiliatePII.load_pii` +
//! `gutschrift.py:build_readiness`. Die Stammdaten (Name/E-Mail/Adresse/Steuer)
//! liegen als AES-GCM-verschlüsselte BYTEA-Spalten in `affiliate_pii`;
//! entschlüsselt via [`tb_crypto::FieldCipher`] (Schema identisch zu Pythons
//! `FieldCrypto`, DB_MASTER_KEY_V1) mit AAD `affiliate_pii|<field>|<login>`.
//!
//! Verwendet vom Affiliate-Detail-Endpoint nur für `ust_status` + die
//! Readiness-Prüfung (welche Pflichtfelder fehlen) — die entschlüsselten Werte
//! selbst werden NICHT ausgegeben.

use serde_json::{json, Value};
use sqlx::PgPool;
use tb_crypto::FieldCipher;

const REQUIRED_GUTSCHRIFT_FIELDS: [&str; 6] =
    ["full_name", "email", "address_line1", "address_city", "address_zip", "address_country"];
const VALID_UST_STATUS: [&str; 3] = ["kleinunternehmer", "regelbesteuert", "unknown"];

fn field_label(field: &str) -> &'static str {
    match field {
        "full_name" => "Vollstaendiger Name",
        "email" => "Kontakt-E-Mail",
        "address_line1" => "Strasse",
        "address_city" => "Ort",
        "address_zip" => "PLZ",
        "address_country" => "Land",
        "tax_id" => "Steuernummer oder USt-IdNr.",
        "vat_id" => "USt-IdNr.",
        "ust_status" => "USt-Status",
        _ => "",
    }
}

fn pii_aad(field: &str, login: &str) -> String {
    format!("affiliate_pii|{field}|{login}")
}

fn normalize_ust_status(value: &str) -> String {
    let n = value.trim().to_lowercase();
    if VALID_UST_STATUS.contains(&n.as_str()) {
        n
    } else {
        "unknown".to_string()
    }
}

fn normalize_country(value: &str) -> String {
    let n = value.trim().to_uppercase();
    if n.is_empty() {
        "DE".to_string()
    } else {
        n
    }
}

/// tax_id_enc-Klartext → (tax_id, vat_id) (Python `_deserialize_tax_bundle`).
fn deserialize_tax_bundle(raw: &str) -> (String, String) {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return (String::new(), String::new());
    }
    if normalized.starts_with('{') {
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(normalized) {
            let tax = m.get("tax_id").and_then(Value::as_str).unwrap_or("").trim().to_string();
            let vat = m.get("vat_id").and_then(Value::as_str).unwrap_or("").trim().to_string();
            return (tax, vat);
        }
    }
    (normalized.to_string(), String::new())
}

/// Entschlüsselte PII-Stammdaten (Python `AffiliatePII`-Payload).
#[derive(Debug, Clone)]
pub struct PiiPayload {
    pub full_name: String,
    pub email: String,
    pub address_line1: String,
    pub address_city: String,
    pub address_zip: String,
    pub tax_id: String,
    pub vat_id: String,
    pub address_country: String,
    pub ust_status: String,
    pub updated_at: Option<String>,
}

impl PiiPayload {
    /// Leere Defaults (Python `_default_payload`).
    pub fn default_payload() -> Self {
        Self {
            full_name: String::new(),
            email: String::new(),
            address_line1: String::new(),
            address_city: String::new(),
            address_zip: String::new(),
            tax_id: String::new(),
            vat_id: String::new(),
            address_country: "DE".to_string(),
            ust_status: "unknown".to_string(),
            updated_at: None,
        }
    }

    fn get(&self, field: &str) -> &str {
        match field {
            "full_name" => &self.full_name,
            "email" => &self.email,
            "address_line1" => &self.address_line1,
            "address_city" => &self.address_city,
            "address_zip" => &self.address_zip,
            "address_country" => &self.address_country,
            "tax_id" => &self.tax_id,
            "vat_id" => &self.vat_id,
            "ust_status" => &self.ust_status,
            _ => "",
        }
    }
}

/// Fehler beim PII-Laden.
#[derive(Debug)]
pub enum PiiError {
    Db(sqlx::Error),
    Decrypt(String),
}

#[derive(sqlx::FromRow)]
struct PiiRow {
    full_name_enc: Option<Vec<u8>>,
    email_enc: Option<Vec<u8>>,
    address_line1_enc: Option<Vec<u8>>,
    address_city_enc: Option<Vec<u8>>,
    address_zip_enc: Option<Vec<u8>>,
    tax_id_enc: Option<Vec<u8>>,
    address_country: Option<String>,
    ust_status: Option<String>,
    updated_at: Option<String>,
}

fn decrypt_blob(
    cipher: &FieldCipher,
    blob: Option<Vec<u8>>,
    field: &str,
    login: &str,
) -> Result<String, PiiError> {
    match blob.filter(|b| !b.is_empty()) {
        Some(b) => cipher
            .decrypt_field(&b, &pii_aad(field, login))
            .map_err(|e| PiiError::Decrypt(e.to_string())),
        None => Ok(String::new()),
    }
}

/// Lädt + entschlüsselt die PII eines Affiliates (Python `load_pii`).
/// `login` muss bereits normalisiert sein (AAD-Bindung). Keine Zeile → Defaults.
pub async fn load_affiliate_pii(
    pool: &PgPool,
    cipher: &FieldCipher,
    login: &str,
) -> Result<PiiPayload, PiiError> {
    let row: Option<PiiRow> = sqlx::query_as(
        "SELECT full_name_enc, email_enc, address_line1_enc, address_city_enc, address_zip_enc, \
                tax_id_enc, address_country, ust_status, updated_at \
         FROM affiliate_pii WHERE twitch_login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
    .map_err(PiiError::Db)?;

    let Some(r) = row else {
        return Ok(PiiPayload::default_payload());
    };

    let (tax_id, vat_id) = {
        let raw = decrypt_blob(cipher, r.tax_id_enc, "tax_id", login)?;
        deserialize_tax_bundle(&raw)
    };

    Ok(PiiPayload {
        full_name: decrypt_blob(cipher, r.full_name_enc, "full_name", login)?,
        email: decrypt_blob(cipher, r.email_enc, "email", login)?,
        address_line1: decrypt_blob(cipher, r.address_line1_enc, "address_line1", login)?,
        address_city: decrypt_blob(cipher, r.address_city_enc, "address_city", login)?,
        address_zip: decrypt_blob(cipher, r.address_zip_enc, "address_zip", login)?,
        tax_id,
        vat_id,
        address_country: normalize_country(&r.address_country.unwrap_or_default()),
        ust_status: normalize_ust_status(&r.ust_status.unwrap_or_default()),
        updated_at: r.updated_at,
    })
}

/// Fehlende Pflichtfelder für die Gutschrift-Generierung (Python `missing_gutschrift_fields`).
pub fn missing_gutschrift_fields(pii: &PiiPayload) -> Vec<String> {
    let mut missing = Vec::new();
    for field in REQUIRED_GUTSCHRIFT_FIELDS {
        if pii.get(field).trim().is_empty() {
            missing.push(field.to_string());
        }
    }
    if pii.tax_id.trim().is_empty() && pii.vat_id.trim().is_empty() {
        missing.push("tax_id".to_string());
    }
    missing
}

/// Blocker (USt-Status + fehlende Felder) (Python `gutschrift_blockers`).
fn gutschrift_blockers(pii: &PiiPayload) -> Vec<String> {
    let mut blockers = Vec::new();
    if normalize_ust_status(&pii.ust_status) == "unknown" {
        blockers.push("USt-Status noch nicht angegeben.".to_string());
    }
    for field in missing_gutschrift_fields(pii) {
        blockers.push(format!("{} fehlt.", field_label(&field)));
    }
    blockers
}

/// Readiness-Objekt fürs Dashboard (Python `build_readiness`).
pub fn build_readiness(pii: &PiiPayload) -> Value {
    let blockers = gutschrift_blockers(pii);
    let mut warnings: Vec<String> = Vec::new();
    if normalize_ust_status(&pii.ust_status) == "regelbesteuert" && pii.vat_id.trim().is_empty() {
        warnings.push(
            "USt-IdNr. ist leer. Bitte nur dann leer lassen, wenn keine vergeben wurde.".to_string(),
        );
    }
    let ust = if pii.ust_status.trim().is_empty() { "unknown".to_string() } else { pii.ust_status.clone() };
    json!({
        "can_generate": blockers.is_empty(),
        "blockers": blockers,
        "warnings": warnings,
        "missing_fields": missing_gutschrift_fields(pii),
        "ust_status": ust,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn test_cipher() -> FieldCipher {
        FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap()
    }

    #[test]
    fn readiness_blocker_bei_unvollstaendig() {
        let pii = PiiPayload::default_payload(); // alles leer, ust unknown
        let r = build_readiness(&pii);
        assert_eq!(r["can_generate"], false);
        let blockers = r["blockers"].as_array().unwrap();
        assert!(blockers.iter().any(|b| b == "USt-Status noch nicht angegeben."));
        // address_country ist "DE" (default) → NICHT in missing.
        let missing = r["missing_fields"].as_array().unwrap();
        assert!(missing.iter().any(|m| m == "full_name"));
        assert!(!missing.iter().any(|m| m == "address_country"));
    }

    #[test]
    fn readiness_vollstaendig_kann_generieren() {
        let pii = PiiPayload {
            full_name: "Nani".into(),
            email: "a@b.de".into(),
            address_line1: "Str 1".into(),
            address_city: "Ort".into(),
            address_zip: "12345".into(),
            tax_id: "DE123".into(),
            vat_id: String::new(),
            address_country: "DE".into(),
            ust_status: "kleinunternehmer".into(),
            updated_at: None,
        };
        let r = build_readiness(&pii);
        assert_eq!(r["can_generate"], true);
        assert_eq!(r["blockers"].as_array().unwrap().len(), 0);
        assert_eq!(r["missing_fields"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn readiness_warnung_regelbesteuert_ohne_vat() {
        let pii = PiiPayload {
            full_name: "Nani".into(), email: "a@b.de".into(), address_line1: "Str 1".into(),
            address_city: "Ort".into(), address_zip: "12345".into(), tax_id: "DE123".into(),
            vat_id: String::new(), address_country: "DE".into(), ust_status: "regelbesteuert".into(),
            updated_at: None,
        };
        let r = build_readiness(&pii);
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("USt-IdNr")));
    }

    #[test]
    fn tax_bundle_parsing() {
        assert_eq!(deserialize_tax_bundle(""), (String::new(), String::new()));
        assert_eq!(deserialize_tax_bundle("DE12345"), ("DE12345".to_string(), String::new()));
        assert_eq!(
            deserialize_tax_bundle(r#"{"tax_id":"DE1","vat_id":"DE9"}"#),
            ("DE1".to_string(), "DE9".to_string())
        );
    }

    async fn connect(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, full_name_enc BYTEA, email_enc BYTEA, \
             address_line1_enc BYTEA, address_city_enc BYTEA, address_zip_enc BYTEA, tax_id_enc BYTEA, \
             address_country TEXT, ust_status TEXT, updated_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn load_pii_entschluesselt_round_trip() {
        let Some(pool) = connect("t_pii_load").await else { return };
        let cipher = test_cipher();
        let login = "nani";
        // Mit derselben AAD verschlüsseln wie Python (affiliate_pii|<field>|<login>).
        let name_blob = cipher.encrypt_field("Nani Mustermann", &pii_aad("full_name", login)).unwrap();
        let mail_blob = cipher.encrypt_field("a@b.de", &pii_aad("email", login)).unwrap();
        let tax_blob = cipher.encrypt_field(r#"{"tax_id":"DE1","vat_id":"DE9"}"#, &pii_aad("tax_id", login)).unwrap();
        sqlx::query("INSERT INTO affiliate_pii (twitch_login, full_name_enc, email_enc, tax_id_enc, address_country, ust_status) VALUES ($1, $2, $3, $4, 'de', 'REGELBESTEUERT')")
            .bind(login).bind(&name_blob).bind(&mail_blob).bind(&tax_blob)
            .execute(&pool).await.unwrap();

        let pii = load_affiliate_pii(&pool, &cipher, login).await.unwrap();
        assert_eq!(pii.full_name, "Nani Mustermann");
        assert_eq!(pii.email, "a@b.de");
        assert_eq!(pii.tax_id, "DE1");
        assert_eq!(pii.vat_id, "DE9");
        assert_eq!(pii.address_country, "DE"); // upper-normalisiert
        assert_eq!(pii.ust_status, "regelbesteuert"); // lower-normalisiert
        // address_line1/city/zip leer (keine Blobs) → missing.
        let missing = missing_gutschrift_fields(&pii);
        assert!(missing.contains(&"address_line1".to_string()));
    }

    #[tokio::test]
    async fn load_pii_keine_zeile_default() {
        let Some(pool) = connect("t_pii_default").await else { return };
        let pii = load_affiliate_pii(&pool, &test_cipher(), "ghost").await.unwrap();
        assert_eq!(pii.ust_status, "unknown");
        assert_eq!(pii.address_country, "DE");
        assert!(pii.full_name.is_empty());
    }
}

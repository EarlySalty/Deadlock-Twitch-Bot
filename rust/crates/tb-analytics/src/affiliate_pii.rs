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
use sqlx::{PgPool, Row};
use tb_crypto::FieldCipher;

const REQUIRED_GUTSCHRIFT_FIELDS: [&str; 6] = [
    "full_name",
    "email",
    "address_line1",
    "address_city",
    "address_zip",
    "address_country",
];
pub const VALID_UST_STATUS: [&str; 3] = ["kleinunternehmer", "regelbesteuert", "unknown"];

pub fn is_valid_ust_status(value: &str) -> bool {
    VALID_UST_STATUS.contains(&value.trim().to_lowercase().as_str())
}

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
    if is_valid_ust_status(&n) {
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
            let tax = m
                .get("tax_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let vat = m
                .get("vat_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            return (tax, vat);
        }
    }
    (normalized.to_string(), String::new())
}

/// (tax_id, vat_id) → Klartext-Bundle (Python `_serialize_tax_bundle`).
///
/// Ohne vat_id wird nur die rohe tax_id gespeichert; mit vat_id ein kompaktes
/// JSON `{"tax_id":...,"vat_id":...}` (ASCII, ohne Whitespace) — exakt das
/// Format, das [`deserialize_tax_bundle`] wieder erwartet.
fn serialize_tax_bundle(tax_id: &str, vat_id: &str) -> String {
    let tax = tax_id.trim();
    let vat = vat_id.trim();
    if vat.is_empty() {
        return tax.to_string();
    }
    // serde_json::json! erzeugt ASCII + separators (",",":") (compact).
    json!({ "tax_id": tax, "vat_id": vat }).to_string()
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
    let normalized_login = login.trim().to_lowercase();
    let row: Option<PiiRow> = sqlx::query_as::<_, PiiRow>(
        r#"
        SELECT full_name_enc,
               email_enc,
               address_line1_enc,
               address_city_enc,
               address_zip_enc,
               tax_id_enc,
               address_country,
               ust_status,
               updated_at
        FROM affiliate_pii
        WHERE LOWER(twitch_login) = LOWER($1)
        ORDER BY CASE WHEN twitch_login = $1 THEN 0 ELSE 1 END
        LIMIT 1
        "#,
    )
    .bind(&normalized_login)
    .fetch_optional(pool)
    .await
    .map_err(PiiError::Db)?;

    let Some(r) = row else {
        return Ok(PiiPayload::default_payload());
    };

    let (tax_id, vat_id) = {
        let raw = decrypt_blob(cipher, r.tax_id_enc, "tax_id", &normalized_login)?;
        deserialize_tax_bundle(&raw)
    };

    Ok(PiiPayload {
        full_name: decrypt_blob(cipher, r.full_name_enc, "full_name", &normalized_login)?,
        email: decrypt_blob(cipher, r.email_enc, "email", &normalized_login)?,
        address_line1: decrypt_blob(
            cipher,
            r.address_line1_enc,
            "address_line1",
            &normalized_login,
        )?,
        address_city: decrypt_blob(
            cipher,
            r.address_city_enc,
            "address_city",
            &normalized_login,
        )?,
        address_zip: decrypt_blob(cipher, r.address_zip_enc, "address_zip", &normalized_login)?,
        tax_id,
        vat_id,
        address_country: normalize_country(&r.address_country.unwrap_or_default()),
        ust_status: normalize_ust_status(&r.ust_status.unwrap_or_default()),
        updated_at: r.updated_at,
    })
}

/// Teil-Update der PII (Python `save_pii(data: dict)`).
///
/// **Semantik mirror Pythons `field in payload`-Idiom:** `None` = Feld NICHT im
/// Payload → bestehender Wert bleibt; `Some(value)` = Feld gesetzt (auch leerer
/// String → Spalte wird geleert). So überschreibt ein partielles Profil-Update
/// nie versehentlich nicht-mitgeschickte Felder.
#[derive(Debug, Default, Clone)]
pub struct PiiInput {
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub address_line1: Option<String>,
    pub address_city: Option<String>,
    pub address_zip: Option<String>,
    pub tax_id: Option<String>,
    pub vat_id: Option<String>,
    pub address_country: Option<String>,
    pub ust_status: Option<String>,
}

/// Roh-Bestand (verschlüsselte Blobs + Klar-Spalten) für den Save-Merge.
#[derive(sqlx::FromRow)]
struct PiiExistingRow {
    twitch_login: String,
    full_name_enc: Option<Vec<u8>>,
    email_enc: Option<Vec<u8>>,
    address_line1_enc: Option<Vec<u8>>,
    address_city_enc: Option<Vec<u8>>,
    address_zip_enc: Option<Vec<u8>>,
    tax_id_enc: Option<Vec<u8>>,
    address_country: Option<String>,
    ust_status: Option<String>,
}

/// Verschlüsselt einen Klartext-Feldwert oder gibt den Bestand zurück (Python
/// `save_pii`-Schleife): Feld nicht im Payload → Bestand; leer → `None`; sonst
/// AES-GCM-verschlüsseln mit feld-/login-gebundener AAD.
fn encrypt_or_keep(
    cipher: &FieldCipher,
    value: Option<&String>,
    existing: Option<Vec<u8>>,
    field: &str,
    login: &str,
) -> Result<Option<Vec<u8>>, PiiError> {
    match value {
        None => Ok(existing),
        Some(v) if v.trim().is_empty() => Ok(None),
        Some(v) => cipher
            .encrypt_field(v.trim(), &pii_aad(field, login))
            .map(Some)
            .map_err(|e| PiiError::Decrypt(e.to_string())),
    }
}

/// Persistiert (UPSERT) die verschlüsselte PII eines Affiliates (Python `save_pii`).
///
/// `login` muss bereits normalisiert sein (AAD-Bindung, identisch zu
/// [`load_affiliate_pii`]). Klartext-Felder werden vor dem Schreiben AES-GCM-
/// verschlüsselt; nicht im [`PiiInput`] gesetzte Felder behalten ihren Bestand.
/// `updated_at` wird stets neu gesetzt (UTC-ISO-8601).
pub async fn save_affiliate_pii(
    pool: &PgPool,
    cipher: &FieldCipher,
    login: &str,
    input: &PiiInput,
) -> Result<(), PiiError> {
    save_affiliate_pii_with_storage_login(pool, cipher, login, login, input).await
}

async fn save_affiliate_pii_with_storage_login(
    pool: &PgPool,
    cipher: &FieldCipher,
    storage_login_hint: &str,
    aad_login: &str,
    input: &PiiInput,
) -> Result<(), PiiError> {
    let normalized_login = aad_login.trim().to_lowercase();
    let storage_login_hint = storage_login_hint.trim();
    let existing: Option<PiiExistingRow> = sqlx::query_as::<_, PiiExistingRow>(
        r#"
        SELECT twitch_login,
               full_name_enc,
               email_enc,
               address_line1_enc,
               address_city_enc,
               address_zip_enc,
               tax_id_enc,
               address_country,
               ust_status
        FROM affiliate_pii
        WHERE LOWER(twitch_login) = LOWER($1)
        ORDER BY CASE WHEN twitch_login = $2 THEN 0 WHEN twitch_login = $1 THEN 1 ELSE 2 END
        LIMIT 1
        "#,
    )
    .bind(&normalized_login)
    .bind(storage_login_hint)
    .fetch_optional(pool)
    .await
    .map_err(PiiError::Db)?;
    let storage_login = existing
        .as_ref()
        .map(|row| row.twitch_login.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if storage_login_hint.is_empty() {
                normalized_login.clone()
            } else {
                storage_login_hint.to_string()
            }
        });

    // Encrypted Stamm-Felder mergen (Bestand-Fallback bei fehlendem Key).
    let take = |b: &Option<PiiExistingRow>, f: fn(&PiiExistingRow) -> &Option<Vec<u8>>| {
        b.as_ref().and_then(|r| f(r).clone())
    };
    let full_name_enc = encrypt_or_keep(
        cipher,
        input.full_name.as_ref(),
        take(&existing, |r| &r.full_name_enc),
        "full_name",
        &normalized_login,
    )?;
    let email_enc = encrypt_or_keep(
        cipher,
        input.email.as_ref(),
        take(&existing, |r| &r.email_enc),
        "email",
        &normalized_login,
    )?;
    let address_line1_enc = encrypt_or_keep(
        cipher,
        input.address_line1.as_ref(),
        take(&existing, |r| &r.address_line1_enc),
        "address_line1",
        &normalized_login,
    )?;
    let address_city_enc = encrypt_or_keep(
        cipher,
        input.address_city.as_ref(),
        take(&existing, |r| &r.address_city_enc),
        "address_city",
        &normalized_login,
    )?;
    let address_zip_enc = encrypt_or_keep(
        cipher,
        input.address_zip.as_ref(),
        take(&existing, |r| &r.address_zip_enc),
        "address_zip",
        &normalized_login,
    )?;

    // Tax-Bundle (tax_id + vat_id) zusammenführen — Bestand entschlüsseln, mit
    // gesetzten Keys überschreiben, neu serialisieren/verschlüsseln (Python:
    // _normalize_tax_bundle + _serialize_tax_bundle).
    let existing_tax_blob = take(&existing, |r| &r.tax_id_enc);
    let tax_id_enc = if input.tax_id.is_some() || input.vat_id.is_some() {
        let (mut tax, mut vat) = match &existing_tax_blob {
            Some(b) if !b.is_empty() => {
                let raw = cipher
                    .decrypt_field(b, &pii_aad("tax_id", &normalized_login))
                    .map_err(|e| PiiError::Decrypt(e.to_string()))?;
                deserialize_tax_bundle(&raw)
            }
            _ => (String::new(), String::new()),
        };
        if let Some(v) = &input.tax_id {
            tax = v.trim().to_string();
        }
        if let Some(v) = &input.vat_id {
            vat = v.trim().to_string();
        }
        let serialized = serialize_tax_bundle(&tax, &vat);
        if serialized.is_empty() {
            None
        } else {
            Some(
                cipher
                    .encrypt_field(&serialized, &pii_aad("tax_id", &normalized_login))
                    .map_err(|e| PiiError::Decrypt(e.to_string()))?,
            )
        }
    } else {
        existing_tax_blob
    };

    // Klar-Spalten: gesetzt → normalisieren; sonst Bestand; sonst Default.
    let address_country = match &input.address_country {
        Some(v) => normalize_country(v),
        None => existing
            .as_ref()
            .and_then(|r| r.address_country.as_deref())
            .map(normalize_country)
            .unwrap_or_else(|| "DE".to_string()),
    };
    let ust_status = match &input.ust_status {
        Some(v) => normalize_ust_status(v),
        None => existing
            .as_ref()
            .and_then(|r| r.ust_status.as_deref())
            .map(normalize_ust_status)
            .unwrap_or_else(|| "unknown".to_string()),
    };
    let updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false);

    sqlx::query!(
        r#"
        INSERT INTO affiliate_pii
            (twitch_login, full_name_enc, email_enc, address_line1_enc,
             address_city_enc, address_zip_enc, tax_id_enc, address_country, ust_status, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (twitch_login) DO UPDATE SET
            full_name_enc = excluded.full_name_enc,
            email_enc = excluded.email_enc,
            address_line1_enc = excluded.address_line1_enc,
            address_city_enc = excluded.address_city_enc,
            address_zip_enc = excluded.address_zip_enc,
            tax_id_enc = excluded.tax_id_enc,
            address_country = excluded.address_country,
            ust_status = excluded.ust_status,
            updated_at = excluded.updated_at
        "#,
        &storage_login,
        full_name_enc,
        email_enc,
        address_line1_enc,
        address_city_enc,
        address_zip_enc,
        tax_id_enc,
        &address_country,
        &ust_status,
        &updated_at
    )
    .execute(pool)
    .await
    .map_err(PiiError::Db)?;
    Ok(())
}

/// Migriert alte Klartext-PII aus `affiliate_accounts` nach `affiliate_pii`
/// und leert anschließend die Klartext-Spalten (Python `migrate_from_plaintext`).
pub async fn migrate_from_plaintext(pool: &PgPool, cipher: &FieldCipher) -> Result<u64, PiiError> {
    let rows = sqlx::query(
        r#"
        SELECT a.twitch_login, a.email, a.full_name, a.address_line1, a.address_city,
               a.address_zip, a.address_country
        FROM affiliate_accounts a
        LEFT JOIN affiliate_pii p
          ON LOWER(p.twitch_login) = LOWER(a.twitch_login)
        WHERE p.twitch_login IS NULL
          AND (
            TRIM(COALESCE(a.email, '')) <> ''
            OR TRIM(COALESCE(a.full_name, '')) <> ''
            OR TRIM(COALESCE(a.address_line1, '')) <> ''
            OR TRIM(COALESCE(a.address_city, '')) <> ''
            OR TRIM(COALESCE(a.address_zip, '')) <> ''
            OR TRIM(COALESCE(a.address_country, '')) <> ''
          )
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(PiiError::Db)?;

    let mut migrated = 0_u64;
    for row in rows {
        let account_login = row
            .try_get::<Option<String>, _>("twitch_login")
            .map_err(PiiError::Db)?
            .unwrap_or_default()
            .trim()
            .to_string();
        if account_login.is_empty() {
            continue;
        }
        let normalized_login = account_login.to_lowercase();
        let value = |name: &str| -> Result<String, PiiError> {
            Ok(row
                .try_get::<Option<String>, _>(name)
                .map_err(PiiError::Db)?
                .unwrap_or_default())
        };
        let input = PiiInput {
            email: Some(value("email")?),
            full_name: Some(value("full_name")?),
            address_line1: Some(value("address_line1")?),
            address_city: Some(value("address_city")?),
            address_zip: Some(value("address_zip")?),
            address_country: Some(value("address_country")?),
            ..PiiInput::default()
        };
        save_affiliate_pii_with_storage_login(
            pool,
            cipher,
            &account_login,
            &normalized_login,
            &input,
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE affiliate_accounts
            SET email = '',
                full_name = '',
                address_line1 = '',
                address_city = '',
                address_zip = '',
                address_country = ''
            WHERE twitch_login = $1
            "#,
        )
        .bind(&account_login)
        .execute(pool)
        .await
        .map_err(PiiError::Db)?;
        migrated += 1;
    }
    Ok(migrated)
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
            "USt-IdNr. ist leer. Bitte nur dann leer lassen, wenn keine vergeben wurde."
                .to_string(),
        );
    }
    let ust = if pii.ust_status.trim().is_empty() {
        "unknown".to_string()
    } else {
        pii.ust_status.clone()
    };
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
        assert!(blockers
            .iter()
            .any(|b| b == "USt-Status noch nicht angegeben."));
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
            full_name: "Nani".into(),
            email: "a@b.de".into(),
            address_line1: "Str 1".into(),
            address_city: "Ort".into(),
            address_zip: "12345".into(),
            tax_id: "DE123".into(),
            vat_id: String::new(),
            address_country: "DE".into(),
            ust_status: "regelbesteuert".into(),
            updated_at: None,
        };
        let r = build_readiness(&pii);
        assert!(r["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("USt-IdNr")));
    }

    #[test]
    fn tax_bundle_parsing() {
        assert_eq!(deserialize_tax_bundle(""), (String::new(), String::new()));
        assert_eq!(
            deserialize_tax_bundle("DE12345"),
            ("DE12345".to_string(), String::new())
        );
        assert_eq!(
            deserialize_tax_bundle(r#"{"tax_id":"DE1","vat_id":"DE9"}"#),
            ("DE1".to_string(), "DE9".to_string())
        );
    }

    async fn connect(schema: &str) -> Option<PgPool> {
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
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
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
        let Some(pool) = connect("t_pii_load").await else {
            return;
        };
        let cipher = test_cipher();
        let login = "nani";
        // Mit derselben AAD verschlüsseln wie Python (affiliate_pii|<field>|<login>).
        let name_blob = cipher
            .encrypt_field("Nani Mustermann", &pii_aad("full_name", login))
            .unwrap();
        let mail_blob = cipher
            .encrypt_field("a@b.de", &pii_aad("email", login))
            .unwrap();
        let tax_blob = cipher
            .encrypt_field(
                r#"{"tax_id":"DE1","vat_id":"DE9"}"#,
                &pii_aad("tax_id", login),
            )
            .unwrap();
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
        let Some(pool) = connect("t_pii_default").await else {
            return;
        };
        let pii = load_affiliate_pii(&pool, &test_cipher(), "ghost")
            .await
            .unwrap();
        assert_eq!(pii.ust_status, "unknown");
        assert_eq!(pii.address_country, "DE");
        assert!(pii.full_name.is_empty());
    }

    #[tokio::test]
    async fn migrate_from_plaintext_verschiebt_und_leert_account_spalten() {
        let Some(pool) = connect("t_pii_migrate").await else {
            return;
        };
        sqlx::query(
            r#"
            CREATE TABLE affiliate_accounts (
                twitch_login TEXT PRIMARY KEY,
                email TEXT NOT NULL,
                full_name TEXT NOT NULL,
                address_line1 TEXT NOT NULL,
                address_city TEXT NOT NULL,
                address_zip TEXT NOT NULL,
                address_country TEXT NOT NULL DEFAULT ''
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, email, full_name, address_line1, address_city, address_zip, address_country)
            VALUES ('Affiliate_One', 'legacy@example.com', 'Legacy Partner', 'Altbau 5', 'Hamburg', '20095', 'DE')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let cipher = test_cipher();
        assert_eq!(migrate_from_plaintext(&pool, &cipher).await.unwrap(), 1);
        let pii = load_affiliate_pii(&pool, &cipher, "affiliate_one")
            .await
            .unwrap();
        assert_eq!(pii.email, "legacy@example.com");
        assert_eq!(pii.full_name, "Legacy Partner");
        assert_eq!(pii.address_line1, "Altbau 5");
        assert_eq!(pii.address_city, "Hamburg");
        assert_eq!(pii.address_zip, "20095");
        assert_eq!(pii.address_country, "DE");
        let pii_key: String = sqlx::query_scalar(
            "SELECT twitch_login FROM affiliate_pii WHERE LOWER(twitch_login) = LOWER('affiliate_one')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pii_key, "Affiliate_One");
        let row = sqlx::query(
            "SELECT email, full_name, address_line1, address_city, address_zip, address_country FROM affiliate_accounts WHERE twitch_login = 'Affiliate_One'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        for column in [
            "email",
            "full_name",
            "address_line1",
            "address_city",
            "address_zip",
            "address_country",
        ] {
            assert_eq!(row.try_get::<String, _>(column).unwrap(), "");
        }
        assert_eq!(migrate_from_plaintext(&pool, &cipher).await.unwrap(), 0);
    }

    // ── Save-Pfad (B2-P1-affiliate-pii-write) ───────────────────────────────

    #[test]
    fn tax_bundle_serialize_roundtrip() {
        // Ohne vat_id → rohe tax_id.
        assert_eq!(serialize_tax_bundle("DE123", ""), "DE123");
        assert_eq!(serialize_tax_bundle("", ""), "");
        // Mit vat_id → kompaktes JSON, das deserialize wieder versteht.
        let s = serialize_tax_bundle("DE1", "DE9");
        assert_eq!(s, r#"{"tax_id":"DE1","vat_id":"DE9"}"#);
        assert_eq!(
            deserialize_tax_bundle(&s),
            ("DE1".to_string(), "DE9".to_string())
        );
    }

    fn input_full() -> PiiInput {
        PiiInput {
            full_name: Some("Nani Mustermann".into()),
            email: Some("a@b.de".into()),
            address_line1: Some("Str 1".into()),
            address_city: Some("Ort".into()),
            address_zip: Some("12345".into()),
            tax_id: Some("DE1".into()),
            vat_id: Some("DE9".into()),
            address_country: Some("de".into()),
            ust_status: Some("REGELBESTEUERT".into()),
        }
    }

    #[tokio::test]
    async fn save_then_load_round_trip() {
        let Some(pool) = connect("t_pii_save").await else {
            return;
        };
        let cipher = test_cipher();
        let login = "nani";
        save_affiliate_pii(&pool, &cipher, login, &input_full())
            .await
            .unwrap();

        let pii = load_affiliate_pii(&pool, &cipher, login).await.unwrap();
        assert_eq!(pii.full_name, "Nani Mustermann");
        assert_eq!(pii.email, "a@b.de");
        assert_eq!(pii.address_zip, "12345");
        assert_eq!(pii.tax_id, "DE1");
        assert_eq!(pii.vat_id, "DE9");
        assert_eq!(pii.address_country, "DE"); // upper-normalisiert
        assert_eq!(pii.ust_status, "regelbesteuert"); // lower-normalisiert
        assert!(pii.updated_at.is_some());
        // Readiness vollständig.
        assert_eq!(build_readiness(&pii)["can_generate"], true);
    }

    #[tokio::test]
    async fn partial_update_behaelt_ungesetzte_felder() {
        let Some(pool) = connect("t_pii_partial").await else {
            return;
        };
        let cipher = test_cipher();
        let login = "nani";
        save_affiliate_pii(&pool, &cipher, login, &input_full())
            .await
            .unwrap();

        // Nur email ändern; alle anderen Felder None → Bestand bleibt.
        let patch = PiiInput {
            email: Some("neu@b.de".into()),
            ..PiiInput::default()
        };
        save_affiliate_pii(&pool, &cipher, login, &patch)
            .await
            .unwrap();

        let pii = load_affiliate_pii(&pool, &cipher, login).await.unwrap();
        assert_eq!(pii.email, "neu@b.de"); // geändert
        assert_eq!(pii.full_name, "Nani Mustermann"); // unverändert
        assert_eq!(pii.tax_id, "DE1"); // tax-bundle unverändert
        assert_eq!(pii.vat_id, "DE9");
        assert_eq!(pii.ust_status, "regelbesteuert");
    }

    #[tokio::test]
    async fn leeres_feld_leert_spalte() {
        let Some(pool) = connect("t_pii_clear").await else {
            return;
        };
        let cipher = test_cipher();
        let login = "nani";
        save_affiliate_pii(&pool, &cipher, login, &input_full())
            .await
            .unwrap();

        // address_line1 explizit leeren (Some("")) → Spalte NULL.
        let patch = PiiInput {
            address_line1: Some(String::new()),
            ..PiiInput::default()
        };
        save_affiliate_pii(&pool, &cipher, login, &patch)
            .await
            .unwrap();

        let pii = load_affiliate_pii(&pool, &cipher, login).await.unwrap();
        assert!(pii.address_line1.is_empty());
        assert!(missing_gutschrift_fields(&pii).contains(&"address_line1".to_string()));
        // andere bleiben.
        assert_eq!(pii.address_city, "Ort");
    }

    #[tokio::test]
    async fn vat_id_entfernen_faellt_auf_rohe_tax_id() {
        let Some(pool) = connect("t_pii_vat").await else {
            return;
        };
        let cipher = test_cipher();
        let login = "nani";
        save_affiliate_pii(&pool, &cipher, login, &input_full())
            .await
            .unwrap();

        // vat_id leeren → Bundle = nur tax_id (roh, kein JSON).
        let patch = PiiInput {
            vat_id: Some(String::new()),
            ..PiiInput::default()
        };
        save_affiliate_pii(&pool, &cipher, login, &patch)
            .await
            .unwrap();

        let pii = load_affiliate_pii(&pool, &cipher, login).await.unwrap();
        assert_eq!(pii.tax_id, "DE1");
        assert!(pii.vat_id.is_empty());
    }
}

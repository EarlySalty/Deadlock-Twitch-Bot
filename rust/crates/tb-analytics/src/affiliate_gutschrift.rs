//! Affiliate-Gutschriften: Generierung, Nummernkreis, PDF und SMTP-Versand.
//!
//! Treuer Port von `bot/dashboard/affiliate/gutschrift.py` und
//! `bot/dashboard/affiliate/affiliate_email.py`. Admin-Routen bleiben in Welle D;
//! dieses Modul stellt die aufrufbaren Funktionen dafuer bereit.

use std::collections::BTreeSet;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use lettre::message::{header::ContentType, Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, Message, SmtpTransport, Transport};
use printpdf::{
    BuiltinFont, Color, Mm, Op, PaintMode, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions,
    Point, Pt, Rect, Rgb, TextItem,
};
use serde::Serialize;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, Row, Transaction};
use tb_crypto::FieldCipher;
use tb_domain::normalize_twitch_login;
use thiserror::Error;

use crate::affiliate_pii::{build_readiness, load_affiliate_pii, PiiError, PiiPayload};

const TRANSFERRED_STATUS: &str = "transferred";
const STATUS_BLOCKED: &str = "blocked";
const STATUS_GENERATED: &str = "generated";
const STATUS_EMAILED: &str = "emailed";
const STATUS_EMAIL_FAILED: &str = "email_failed";
const STATUS_EXISTING: &str = "existing";
const VAT_RATE_PERCENT: i64 = 19;
const GUTSCHRIFT_LOCK_NAMESPACE: i32 = 1_129_067_203;

const MONTH_LABELS: [&str; 13] = [
    "",
    "Januar",
    "Februar",
    "Maerz",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

const SELECT_GUTSCHRIFT_COLUMNS: &str = "\
    id::bigint AS id, \
    gutschrift_number, \
    affiliate_twitch_login, \
    period_year::bigint AS period_year, \
    period_month::bigint AS period_month, \
    net_amount_cents::bigint AS net_amount_cents, \
    vat_amount_cents::bigint AS vat_amount_cents, \
    gross_amount_cents::bigint AS gross_amount_cents, \
    affiliate_ust_status, \
    pdf_blob, \
    pdf_generated_at, \
    email_sent_at, \
    email_error, \
    commission_ids, \
    created_at";

#[derive(Debug, Error)]
pub enum GutschriftError {
    #[error("affiliate_login is required")]
    InvalidLogin,
    #[error("invalid period")]
    InvalidPeriod,
    #[error("invalid year_month")]
    InvalidYearMonth,
    #[error("affiliate_gutschrift_counter schema is incompatible")]
    CounterSchema,
    #[error("could not allocate gutschrift number")]
    CounterAllocation,
    #[error("stored gutschrift row missing after upsert")]
    StoredRowMissing,
    #[error("amount overflow")]
    AmountOverflow,
    #[error("PII database error: {0}")]
    PiiDb(#[source] sqlx::Error),
    #[error("PII decrypt error: {0}")]
    PiiDecrypt(String),
    #[error("PDF generation failed: {0}")]
    Pdf(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

impl From<PiiError> for GutschriftError {
    fn from(error: PiiError) -> Self {
        match error {
            PiiError::Db(error) => Self::PiiDb(error),
            PiiError::Decrypt(error) => Self::PiiDecrypt(error),
        }
    }
}

fn is_missing_schema_error(error: &sqlx::Error) -> bool {
    let text = error.to_string().to_lowercase();
    [
        "does not exist",
        "no such table",
        "undefined table",
        "no such column",
        "undefined column",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[derive(Debug, Error)]
pub enum AffiliateEmailError {
    #[error("message build failed: {0}")]
    Message(String),
    #[error("smtp transport failed: {0}")]
    Transport(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffiliateEmailSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_email: String,
    pub from_name: String,
    pub starttls: bool,
    pub use_ssl: bool,
    pub timeout_seconds: u64,
}

impl AffiliateEmailSettings {
    pub fn from_secret_loader<F>(mut loader: F) -> Option<Self>
    where
        F: FnMut(&[&str]) -> Option<String>,
    {
        let host = load_secret(
            &mut loader,
            &["AFFILIATE_GUTSCHRIFT_SMTP_HOST", "SMTP_HOST"],
        );
        if host.trim().is_empty() {
            return None;
        }
        let port = load_secret(
            &mut loader,
            &["AFFILIATE_GUTSCHRIFT_SMTP_PORT", "SMTP_PORT"],
        )
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(587);
        let from_email = load_secret(
            &mut loader,
            &[
                "AFFILIATE_GUTSCHRIFT_SMTP_FROM",
                "AFFILIATE_GUTSCHRIFT_FROM_EMAIL",
                "SMTP_FROM",
            ],
        );
        if from_email.trim().is_empty() {
            return None;
        }

        let from_name = load_secret(
            &mut loader,
            &[
                "AFFILIATE_GUTSCHRIFT_SMTP_FROM_NAME",
                "AFFILIATE_GUTSCHRIFT_FROM_NAME",
            ],
        );

        Some(Self {
            host: host.trim().to_string(),
            port,
            username: load_secret(
                &mut loader,
                &["AFFILIATE_GUTSCHRIFT_SMTP_USERNAME", "SMTP_USERNAME"],
            )
            .trim()
            .to_string(),
            password: load_secret(
                &mut loader,
                &["AFFILIATE_GUTSCHRIFT_SMTP_PASSWORD", "SMTP_PASSWORD"],
            )
            .trim()
            .to_string(),
            from_email: from_email.trim().to_string(),
            from_name: from_name
                .trim()
                .if_empty("Deadlock Partner Network")
                .to_string(),
            starttls: normalize_bool(
                &load_secret(
                    &mut loader,
                    &["AFFILIATE_GUTSCHRIFT_SMTP_STARTTLS", "SMTP_STARTTLS"],
                ),
                true,
            ),
            use_ssl: normalize_bool(
                &load_secret(&mut loader, &["AFFILIATE_GUTSCHRIFT_SMTP_SSL", "SMTP_SSL"]),
                false,
            ),
            timeout_seconds: 20,
        })
    }
}

pub trait AffiliateGutschriftEmailSender: Send + Sync {
    fn send_gutschrift(
        &self,
        message: &AffiliateGutschriftEmail,
    ) -> Result<(), AffiliateEmailError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffiliateGutschriftEmail {
    pub recipient_email: String,
    pub recipient_name: String,
    pub gutschrift_number: String,
    pub period_label: String,
    pub gross_amount_label: String,
    pub pdf_bytes: Vec<u8>,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct SmtpAffiliateEmailSender {
    settings: AffiliateEmailSettings,
}

impl SmtpAffiliateEmailSender {
    pub fn new(settings: AffiliateEmailSettings) -> Self {
        Self { settings }
    }

    pub fn from_secret_loader<F>(loader: F) -> Option<Self>
    where
        F: FnMut(&[&str]) -> Option<String>,
    {
        AffiliateEmailSettings::from_secret_loader(loader).map(Self::new)
    }
}

impl AffiliateGutschriftEmailSender for SmtpAffiliateEmailSender {
    fn send_gutschrift(
        &self,
        message: &AffiliateGutschriftEmail,
    ) -> Result<(), AffiliateEmailError> {
        let display_name = if message.recipient_name.trim().is_empty() {
            "Affiliate".to_string()
        } else {
            message.recipient_name.trim().to_string()
        };
        let body = [
            format!("Hallo {display_name},"),
            String::new(),
            format!(
                "anbei deine Gutschrift {} fuer den Zeitraum {}.",
                message.gutschrift_number, message.period_label
            ),
            format!("Auszahlungsbetrag brutto: {}", message.gross_amount_label),
            String::new(),
            "Die PDF ist dieser E-Mail beigefuegt.".to_string(),
        ]
        .join("\n");

        let from = mailbox(
            non_empty_name(&self.settings.from_name),
            &self.settings.from_email,
        )?;
        let to = mailbox(non_empty_name(&display_name), &message.recipient_email)?;
        let pdf_content_type = ContentType::parse("application/pdf")
            .map_err(|error| AffiliateEmailError::Message(error.to_string()))?;
        let email = Message::builder()
            .from(from)
            .to(to)
            .subject(format!(
                "Gutschrift {} fuer {}",
                message.gutschrift_number, message.period_label
            ))
            .multipart(
                MultiPart::mixed()
                    .singlepart(SinglePart::plain(body))
                    .singlepart(
                        Attachment::new(message.filename.clone())
                            .body(message.pdf_bytes.clone(), pdf_content_type),
                    ),
            )
            .map_err(|error| AffiliateEmailError::Message(error.to_string()))?;

        let mut builder = if self.settings.use_ssl {
            SmtpTransport::relay(&self.settings.host)
        } else if self.settings.starttls {
            SmtpTransport::starttls_relay(&self.settings.host)
        } else {
            Ok(SmtpTransport::builder_dangerous(&self.settings.host))
        }
        .map_err(|error| AffiliateEmailError::Transport(error.to_string()))?
        .port(self.settings.port)
        .timeout(Some(Duration::from_secs(
            self.settings.timeout_seconds.max(1),
        )));

        if !self.settings.username.trim().is_empty() {
            builder = builder.credentials(Credentials::new(
                self.settings.username.clone(),
                self.settings.password.clone(),
            ));
        }

        builder
            .build()
            .send(&email)
            .map_err(|error| AffiliateEmailError::Transport(error.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffiliateGutschriftSeller {
    pub name: String,
    pub company: String,
    pub street: String,
    pub postal_code: String,
    pub city: String,
    pub country: String,
    pub email: String,
    pub website: String,
    pub tax_id: String,
}

impl Default for AffiliateGutschriftSeller {
    fn default() -> Self {
        Self {
            name: "[STEUERBERATER: Firmenname]".to_string(),
            company: "[STEUERBERATER: Firmierung]".to_string(),
            street: "[STEUERBERATER: Adresse]".to_string(),
            postal_code: String::new(),
            city: String::new(),
            country: "DE".to_string(),
            email: "billing@example.invalid".to_string(),
            website: String::new(),
            tax_id: "[STEUERBERATER: Steuernummer/USt-IdNr.]".to_string(),
        }
    }
}

impl AffiliateGutschriftSeller {
    pub fn from_secret_loader<F>(mut loader: F, public_url: Option<&str>) -> Self
    where
        F: FnMut(&[&str]) -> Option<String>,
    {
        let default = Self::default();
        let website_fallback = public_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("https://deutsche-deadlock-community.de");
        Self {
            name: load_or_default(
                &mut loader,
                &["AFFILIATE_GUTSCHRIFT_SELLER_NAME"],
                &default.name,
            ),
            company: load_or_default(
                &mut loader,
                &["AFFILIATE_GUTSCHRIFT_SELLER_COMPANY"],
                &default.company,
            ),
            street: load_or_default(
                &mut loader,
                &["AFFILIATE_GUTSCHRIFT_SELLER_STREET"],
                &default.street,
            ),
            postal_code: load_secret(&mut loader, &["AFFILIATE_GUTSCHRIFT_SELLER_POSTAL_CODE"])
                .trim()
                .to_string(),
            city: load_secret(&mut loader, &["AFFILIATE_GUTSCHRIFT_SELLER_CITY"])
                .trim()
                .to_string(),
            country: load_or_default(
                &mut loader,
                &["AFFILIATE_GUTSCHRIFT_SELLER_COUNTRY"],
                &default.country,
            )
            .to_uppercase(),
            email: load_or_default(
                &mut loader,
                &[
                    "AFFILIATE_GUTSCHRIFT_SELLER_EMAIL",
                    "AFFILIATE_GUTSCHRIFT_FROM_EMAIL",
                ],
                &default.email,
            ),
            website: load_or_default(
                &mut loader,
                &["AFFILIATE_GUTSCHRIFT_SELLER_WEBSITE"],
                website_fallback,
            ),
            tax_id: load_or_default(
                &mut loader,
                &[
                    "AFFILIATE_GUTSCHRIFT_SELLER_TAX_ID",
                    "AFFILIATE_GUTSCHRIFT_SELLER_VAT_ID",
                ],
                &default.tax_id,
            ),
        }
    }

    fn seller_name(&self) -> String {
        let company = self.company.trim();
        let name = self.name.trim();
        if !company.is_empty() {
            company.to_string()
        } else if !name.is_empty() {
            name.to_string()
        } else {
            Self::default().name
        }
    }

    fn seller_address(&self) -> String {
        combine_address(&self.street, &self.postal_code, &self.city, &self.country)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GutschriftMetadata {
    pub id: i64,
    pub period_year: i32,
    pub period_month: i32,
    pub period_label: String,
    pub gutschrift_number: String,
    pub status: String,
    pub net_amount_cents: i64,
    pub vat_amount_cents: i64,
    pub gross_amount_cents: i64,
    pub commission_count: usize,
    pub commission_ids: Vec<i64>,
    pub note_text: String,
    pub last_error: String,
    pub generated_at: Option<String>,
    pub emailed_at: Option<String>,
    pub created_at: Option<String>,
    pub download_path: Option<String>,
    pub has_pdf: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GenerateGutschriftResult {
    pub ok: bool,
    pub document: Option<GutschriftMetadata>,
    pub action: Option<String>,
    pub status: Option<String>,
    pub affiliate_login: String,
    pub period_year: i32,
    pub period_month: i32,
    pub readiness: Value,
}

#[derive(Debug, Clone)]
struct StoredGutschriftRow {
    id: i64,
    period_year: i32,
    period_month: i32,
    gutschrift_number: String,
    net_amount_cents: i64,
    vat_amount_cents: i64,
    gross_amount_cents: i64,
    commission_ids: Option<String>,
    affiliate_ust_status: String,
    pdf_blob: Option<Vec<u8>>,
    pdf_generated_at: Option<String>,
    email_sent_at: Option<String>,
    email_error: Option<String>,
    created_at: Option<String>,
}

impl StoredGutschriftRow {
    fn from_row(row: PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get::<Option<i64>, _>("id")?.unwrap_or_default(),
            period_year: row
                .try_get::<Option<i64>, _>("period_year")?
                .unwrap_or_default() as i32,
            period_month: row
                .try_get::<Option<i64>, _>("period_month")?
                .unwrap_or_default() as i32,
            gutschrift_number: row
                .try_get::<Option<String>, _>("gutschrift_number")?
                .unwrap_or_default(),
            net_amount_cents: row
                .try_get::<Option<i64>, _>("net_amount_cents")?
                .unwrap_or_default(),
            vat_amount_cents: row
                .try_get::<Option<i64>, _>("vat_amount_cents")?
                .unwrap_or_default(),
            gross_amount_cents: row
                .try_get::<Option<i64>, _>("gross_amount_cents")?
                .unwrap_or_default(),
            commission_ids: row.try_get("commission_ids")?,
            affiliate_ust_status: row
                .try_get::<Option<String>, _>("affiliate_ust_status")?
                .unwrap_or_default(),
            pdf_blob: row.try_get("pdf_blob")?,
            pdf_generated_at: row.try_get("pdf_generated_at")?,
            email_sent_at: row.try_get("email_sent_at")?,
            email_error: row.try_get("email_error")?,
            created_at: row.try_get("created_at")?,
        })
    }

    fn has_pdf(&self) -> bool {
        self.pdf_blob.as_ref().is_some_and(|blob| !blob.is_empty())
    }
}

#[derive(Debug, Clone)]
struct CommissionRow {
    id: i64,
    commission_cents: i64,
    currency: String,
}

#[derive(Debug, Clone)]
struct GutschriftPdfData {
    gutschrift_number: String,
    issue_date_label: String,
    period_label: String,
    net_amount_label: String,
    vat_rate_label: String,
    vat_amount_label: String,
    gross_amount_label: String,
    affiliate_name: String,
    affiliate_address: String,
    affiliate_tax_id: String,
    affiliate_ust_status: String,
    issuer_name: String,
    issuer_address: String,
    issuer_tax_id: String,
}

pub fn period_label(year: i32, month: i32) -> Result<String, GutschriftError> {
    let (year, month) = normalize_year_month(year, month)?;
    Ok(format!("{} {}", MONTH_LABELS[month as usize], year))
}

pub fn vat_amount_cents(net_amount_cents: i64, ust_status: &str) -> Result<i64, GutschriftError> {
    if !ust_status.trim().eq_ignore_ascii_case("regelbesteuert") {
        return Ok(0);
    }
    let numerator = i128::from(net_amount_cents)
        .checked_mul(i128::from(VAT_RATE_PERCENT))
        .ok_or(GutschriftError::AmountOverflow)?;
    let rounded = if numerator >= 0 {
        (numerator + 50) / 100
    } else {
        (numerator - 50) / 100
    };
    i64::try_from(rounded).map_err(|_| GutschriftError::AmountOverflow)
}

pub async fn due_periods(
    pool: &PgPool,
    as_of: Option<DateTime<Utc>>,
) -> Result<Vec<(String, i32, i32)>, GutschriftError> {
    let now_utc = as_of.unwrap_or_else(Utc::now);
    let current_period_start = Utc
        .with_ymd_and_hms(now_utc.year(), now_utc.month(), 1, 0, 0, 0)
        .single()
        .ok_or(GutschriftError::InvalidPeriod)?;
    let rows = sqlx::query(
        r#"
        SELECT affiliate_twitch_login, created_at
        FROM affiliate_commissions
        WHERE status = $1
        "#,
    )
    .bind(TRANSFERRED_STATUS)
    .fetch_all(pool)
    .await?;

    let mut periods = BTreeSet::new();
    for row in rows {
        let raw_login = row
            .try_get::<Option<String>, _>("affiliate_twitch_login")?
            .unwrap_or_default();
        let Some(login) = normalize_twitch_login(&raw_login) else {
            continue;
        };
        let created_at_raw = row
            .try_get::<Option<String>, _>("created_at")?
            .unwrap_or_default();
        let Some(created_at) = parse_created_at(&created_at_raw) else {
            continue;
        };
        if created_at >= current_period_start {
            continue;
        }
        periods.insert((created_at.year(), created_at.month() as i32, login));
    }
    Ok(periods
        .into_iter()
        .map(|(year, month, login)| (login, year, month))
        .collect())
}

pub async fn run_pending(
    pool: &PgPool,
    cipher: &FieldCipher,
    email_sender: Option<&dyn AffiliateGutschriftEmailSender>,
    seller: Option<&AffiliateGutschriftSeller>,
    as_of: Option<DateTime<Utc>>,
    limit: usize,
) -> Result<Vec<GenerateGutschriftResult>, GutschriftError> {
    let mut results = Vec::new();
    let due = due_periods(pool, as_of).await?;
    for (affiliate_login, year, month) in due.into_iter().take(limit.max(1)) {
        results.push(
            generate_for_period(
                pool,
                cipher,
                &affiliate_login,
                year,
                month,
                email_sender,
                seller,
                false,
            )
            .await?,
        );
    }
    Ok(results)
}

pub async fn list_for_affiliate(
    pool: &PgPool,
    affiliate_login: &str,
) -> Result<Vec<GutschriftMetadata>, GutschriftError> {
    let login = normalize_login(affiliate_login)?;
    let sql = format!(
        "SELECT {SELECT_GUTSCHRIFT_COLUMNS} \
         FROM affiliate_gutschriften \
         WHERE affiliate_twitch_login = $1 \
         ORDER BY period_year DESC, period_month DESC, id DESC"
    );
    let rows = sqlx::query(&sql).bind(&login).fetch_all(pool).await?;
    rows.into_iter()
        .map(StoredGutschriftRow::from_row)
        .map(|row| row.map(|row| row_to_metadata(&row, false)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(GutschriftError::Db)
}

pub async fn get_pdf(
    pool: &PgPool,
    affiliate_login: &str,
    gutschrift_id: i64,
) -> Result<Option<(GutschriftMetadata, Vec<u8>)>, GutschriftError> {
    let login = normalize_login(affiliate_login)?;
    let sql = format!(
        "SELECT {SELECT_GUTSCHRIFT_COLUMNS} \
         FROM affiliate_gutschriften \
         WHERE id::bigint = $1 AND affiliate_twitch_login = $2"
    );
    let row = match sqlx::query(&sql)
        .bind(gutschrift_id)
        .bind(&login)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(error) if is_missing_schema_error(&error) => {
            return get_pdf_minimal(pool, &login, gutschrift_id).await;
        }
        Err(error) => return Err(GutschriftError::Db(error)),
    };
    let Some(row) = row else {
        return Ok(None);
    };
    let row = match StoredGutschriftRow::from_row(row) {
        Ok(row) => row,
        Err(error) if is_missing_schema_error(&error) => {
            return get_pdf_minimal(pool, &login, gutschrift_id).await;
        }
        Err(error) => return Err(GutschriftError::Db(error)),
    };
    let Some(pdf_blob) = row.pdf_blob.clone().filter(|blob| !blob.is_empty()) else {
        return Ok(None);
    };
    Ok(Some((row_to_metadata(&row, false), pdf_blob)))
}

async fn get_pdf_minimal(
    pool: &PgPool,
    affiliate_login: &str,
    gutschrift_id: i64,
) -> Result<Option<(GutschriftMetadata, Vec<u8>)>, GutschriftError> {
    let row = sqlx::query(
        r#"
        SELECT id::bigint AS id, gutschrift_number, pdf_blob
        FROM affiliate_gutschriften
        WHERE id::bigint = $1 AND affiliate_twitch_login = $2
        "#,
    )
    .bind(gutschrift_id)
    .bind(affiliate_login)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let pdf_blob = row
        .try_get::<Option<Vec<u8>>, _>("pdf_blob")?
        .filter(|blob| !blob.is_empty());
    let Some(pdf_blob) = pdf_blob else {
        return Ok(None);
    };
    let number = row
        .try_get::<Option<String>, _>("gutschrift_number")?
        .unwrap_or_default();
    let id = row.try_get::<i64, _>("id").unwrap_or(gutschrift_id);
    let metadata = GutschriftMetadata {
        id,
        period_year: 0,
        period_month: 0,
        period_label: String::new(),
        gutschrift_number: number,
        status: STATUS_GENERATED.to_string(),
        net_amount_cents: 0,
        vat_amount_cents: 0,
        gross_amount_cents: 0,
        commission_count: 0,
        commission_ids: Vec::new(),
        note_text: String::new(),
        last_error: String::new(),
        generated_at: None,
        emailed_at: None,
        created_at: None,
        download_path: Some(format!("/twitch/api/affiliate/gutschriften/{id}/pdf")),
        has_pdf: true,
    };
    Ok(Some((metadata, pdf_blob)))
}

#[allow(clippy::too_many_arguments)]
pub async fn generate_for_period(
    pool: &PgPool,
    cipher: &FieldCipher,
    affiliate_login: &str,
    year: i32,
    month: i32,
    email_sender: Option<&dyn AffiliateGutschriftEmailSender>,
    seller: Option<&AffiliateGutschriftSeller>,
    force: bool,
) -> Result<GenerateGutschriftResult, GutschriftError> {
    let normalized_login = normalize_login(affiliate_login)?;
    let (year, month) = normalize_year_month(year, month)?;
    let period_start = period_start(year, month)?;
    let next_period_start = next_period_start(year, month)?;
    let profile = load_affiliate_pii(pool, cipher, &normalized_login).await?;
    let readiness = build_readiness(&profile);

    let mut tx = pool.begin().await?;
    lock_generation_period(&mut tx, &normalized_login, year, month).await?;
    let existing = load_existing(&mut tx, &normalized_login, year, month).await?;

    if existing.as_ref().is_some_and(StoredGutschriftRow::has_pdf) && !force {
        let mut current_row = existing.ok_or(GutschriftError::StoredRowMissing)?;
        let mut action = STATUS_EXISTING.to_string();
        if email_sender.is_some()
            && current_row
                .email_sent_at
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            && !profile.email.trim().is_empty()
        {
            let (updated, new_action) = send_email_for_row(
                &mut tx,
                email_sender,
                &profile.email,
                &profile.full_name,
                current_row,
                "eur",
            )
            .await?;
            current_row = updated;
            action = new_action;
        }
        let document = row_to_metadata(&current_row, false);
        tx.commit().await?;
        return Ok(GenerateGutschriftResult {
            ok: true,
            document: Some(document),
            action: Some(action),
            status: None,
            affiliate_login: normalized_login,
            period_year: year,
            period_month: month,
            readiness,
        });
    }

    let commission_rows = load_commissions(
        &mut tx,
        &normalized_login,
        &iso_period(period_start),
        &iso_period(next_period_start),
    )
    .await?;
    if commission_rows.is_empty() {
        tx.commit().await?;
        return Ok(GenerateGutschriftResult {
            ok: false,
            document: None,
            action: None,
            status: Some("no_commissions".to_string()),
            affiliate_login: normalized_login,
            period_year: year,
            period_month: month,
            readiness,
        });
    }
    if readiness
        .get("blockers")
        .and_then(Value::as_array)
        .is_some_and(|blockers| !blockers.is_empty())
    {
        tx.commit().await?;
        return Ok(GenerateGutschriftResult {
            ok: false,
            document: None,
            action: None,
            status: Some(STATUS_BLOCKED.to_string()),
            affiliate_login: normalized_login,
            period_year: year,
            period_month: month,
            readiness,
        });
    }

    let commission_ids: Vec<i64> = commission_rows
        .iter()
        .filter_map(|row| (row.id > 0).then_some(row.id))
        .collect();
    let net_amount_cents = commission_rows
        .iter()
        .try_fold(0_i64, |acc, row| acc.checked_add(row.commission_cents))
        .ok_or(GutschriftError::AmountOverflow)?;
    let currency = commission_rows
        .first()
        .map(|row| row.currency.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "eur".to_string());
    let ust_status = profile.ust_status.trim().to_lowercase();
    let vat_amount_cents = vat_amount_cents(net_amount_cents, &ust_status)?;
    let gross_amount_cents = net_amount_cents
        .checked_add(vat_amount_cents)
        .ok_or(GutschriftError::AmountOverflow)?;

    let effective_seller = seller.cloned().unwrap_or_default();
    let year_month = format!("{year:04}{month:02}");
    let mut gutschrift_number = existing
        .as_ref()
        .map(|row| row.gutschrift_number.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    if gutschrift_number.is_empty() {
        gutschrift_number = next_gutschrift_number(&mut tx, &year_month).await?;
    }

    let affiliate_name = profile.full_name.trim().to_string();
    let affiliate_address = affiliate_address(&profile);
    let affiliate_tax_id = affiliate_tax_id(&profile);
    let issuer_name = effective_seller.seller_name();
    let issuer_address = effective_seller.seller_address();
    let issuer_tax_id = effective_seller.tax_id.trim().to_string();
    let pdf_generated_at = now_iso();
    let created_at = existing
        .as_ref()
        .and_then(|row| row.created_at.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| pdf_generated_at.clone());
    let issue_date = next_period_start.date_naive();
    let pdf_bytes = generate_gutschrift_pdf(&GutschriftPdfData {
        gutschrift_number: gutschrift_number.clone(),
        issue_date_label: format_issue_date(issue_date),
        period_label: period_label(year, month)?,
        net_amount_label: format_eur_cents(net_amount_cents, &currency),
        vat_rate_label: "19 %".to_string(),
        vat_amount_label: format_eur_cents(vat_amount_cents, &currency),
        gross_amount_label: format_eur_cents(gross_amount_cents, &currency),
        affiliate_name,
        affiliate_address,
        affiliate_tax_id,
        affiliate_ust_status: ust_status.clone(),
        issuer_name,
        issuer_address,
        issuer_tax_id,
    })?;

    let stored = store_gutschrift(
        &mut tx,
        &normalized_login,
        year,
        month,
        &gutschrift_number,
        net_amount_cents,
        vat_amount_cents,
        gross_amount_cents,
        &profile,
        &effective_seller,
        pdf_bytes,
        &pdf_generated_at,
        &commission_ids,
        &created_at,
    )
    .await?;

    let mut action = STATUS_GENERATED.to_string();
    let mut row = stored;
    if email_sender.is_some() && !profile.email.trim().is_empty() {
        let (updated, new_action) = send_email_for_row(
            &mut tx,
            email_sender,
            &profile.email,
            &profile.full_name,
            row,
            &currency,
        )
        .await?;
        row = updated;
        action = new_action;
    }

    let document = row_to_metadata(&row, false);
    tx.commit().await?;
    Ok(GenerateGutschriftResult {
        ok: true,
        document: Some(document),
        action: Some(action),
        status: None,
        affiliate_login: normalized_login,
        period_year: year,
        period_month: month,
        readiness,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn generate_monthly_gutschriften(
    pool: &PgPool,
    cipher: &FieldCipher,
    year: i32,
    month: i32,
    email_sender: Option<&dyn AffiliateGutschriftEmailSender>,
    seller: Option<&AffiliateGutschriftSeller>,
    affiliate_login: Option<&str>,
    force: bool,
) -> Result<Vec<GenerateGutschriftResult>, GutschriftError> {
    let (year, month) = normalize_year_month(year, month)?;
    if let Some(login) = affiliate_login {
        let normalized_login = normalize_login(login)?;
        return Ok(vec![
            generate_for_period(
                pool,
                cipher,
                &normalized_login,
                year,
                month,
                email_sender,
                seller,
                force,
            )
            .await?,
        ]);
    }

    let period_start = period_start(year, month)?;
    let next_period_start = next_period_start(year, month)?;
    let rows = sqlx::query(
        r#"
        SELECT DISTINCT affiliate_twitch_login
        FROM affiliate_commissions
        WHERE status = $1
          AND created_at >= $2
          AND created_at < $3
        ORDER BY affiliate_twitch_login ASC
        "#,
    )
    .bind(TRANSFERRED_STATUS)
    .bind(iso_period(period_start))
    .bind(iso_period(next_period_start))
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
        let raw_login = row
            .try_get::<Option<String>, _>("affiliate_twitch_login")?
            .unwrap_or_default();
        let Some(login) = normalize_twitch_login(&raw_login) else {
            continue;
        };
        results.push(
            generate_for_period(
                pool,
                cipher,
                &login,
                year,
                month,
                email_sender,
                seller,
                force,
            )
            .await?,
        );
    }
    Ok(results)
}

fn normalize_login(value: &str) -> Result<String, GutschriftError> {
    normalize_twitch_login(value).ok_or(GutschriftError::InvalidLogin)
}

fn normalize_year_month(year: i32, month: i32) -> Result<(i32, i32), GutschriftError> {
    if year < 2000 || !(1..=12).contains(&month) {
        return Err(GutschriftError::InvalidPeriod);
    }
    Ok((year, month))
}

fn period_start(year: i32, month: i32) -> Result<DateTime<Utc>, GutschriftError> {
    normalize_year_month(year, month)?;
    Utc.with_ymd_and_hms(year, month as u32, 1, 0, 0, 0)
        .single()
        .ok_or(GutschriftError::InvalidPeriod)
}

fn next_period_start(year: i32, month: i32) -> Result<DateTime<Utc>, GutschriftError> {
    normalize_year_month(year, month)?;
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Utc.with_ymd_and_hms(next_year, next_month as u32, 1, 0, 0, 0)
        .single()
        .ok_or(GutschriftError::InvalidPeriod)
}

fn iso_period(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false)
}

fn format_issue_date(value: NaiveDate) -> String {
    value.format("%d.%m.%Y").to_string()
}

fn format_eur_cents(cents: i64, currency: &str) -> String {
    let cents_i128 = i128::from(cents);
    let sign = if cents_i128 < 0 { "-" } else { "" };
    let abs = cents_i128.abs();
    format!(
        "{sign}{},{:02} {}",
        abs / 100,
        abs % 100,
        currency.trim().if_empty("EUR").to_uppercase()
    )
}

fn combine_address(street: &str, postal_code: &str, city: &str, country: &str) -> String {
    let mut lines = Vec::new();
    let street = street.trim();
    if !street.is_empty() {
        lines.push(street.to_string());
    }
    let postal_city = [postal_code.trim(), city.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !postal_city.is_empty() {
        lines.push(postal_city);
    }
    let country = country.trim().to_uppercase();
    if !country.is_empty() {
        lines.push(country);
    }
    lines.join("\n")
}

fn affiliate_address(profile: &PiiPayload) -> String {
    combine_address(
        &profile.address_line1,
        &profile.address_zip,
        &profile.address_city,
        &profile.address_country,
    )
}

fn affiliate_tax_id(profile: &PiiPayload) -> String {
    let mut lines = Vec::new();
    let tax_id = profile.tax_id.trim();
    let vat_id = profile.vat_id.trim();
    if !tax_id.is_empty() {
        lines.push(format!("Steuernummer: {tax_id}"));
    }
    if !vat_id.is_empty() {
        lines.push(format!("USt-IdNr.: {vat_id}"));
    }
    lines.join("\n")
}

fn note_text(ust_status: &str) -> String {
    if ust_status.trim().eq_ignore_ascii_case("kleinunternehmer") {
        "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet.".to_string()
    } else {
        String::new()
    }
}

fn row_status(row: &StoredGutschriftRow) -> String {
    if row
        .pdf_generated_at
        .as_deref()
        .filter(|value| !value.is_empty())
        .is_none()
    {
        STATUS_BLOCKED.to_string()
    } else if row
        .email_error
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        STATUS_EMAIL_FAILED.to_string()
    } else if row
        .email_sent_at
        .as_deref()
        .filter(|value| !value.is_empty())
        .is_some()
    {
        STATUS_EMAILED.to_string()
    } else {
        STATUS_GENERATED.to_string()
    }
}

fn row_to_metadata(row: &StoredGutschriftRow, admin_download_path: bool) -> GutschriftMetadata {
    let commission_ids = parse_commission_ids(row.commission_ids.as_deref());
    let period_label = period_label(row.period_year, row.period_month).unwrap_or_default();
    let download_path = if row.id > 0 {
        if admin_download_path {
            Some(format!(
                "/twitch/api/admin/affiliates/gutschriften/{}/pdf",
                row.id
            ))
        } else {
            Some(format!("/twitch/api/affiliate/gutschriften/{}/pdf", row.id))
        }
    } else {
        None
    };
    GutschriftMetadata {
        id: row.id,
        period_year: row.period_year,
        period_month: row.period_month,
        period_label,
        gutschrift_number: row.gutschrift_number.clone(),
        status: row_status(row),
        net_amount_cents: row.net_amount_cents,
        vat_amount_cents: row.vat_amount_cents,
        gross_amount_cents: row.gross_amount_cents,
        commission_count: commission_ids.len(),
        commission_ids,
        note_text: note_text(&row.affiliate_ust_status),
        last_error: row.email_error.clone().unwrap_or_default(),
        generated_at: row.pdf_generated_at.clone(),
        emailed_at: row.email_sent_at.clone(),
        created_at: row.created_at.clone(),
        download_path,
        has_pdf: row.has_pdf(),
    }
}

fn parse_commission_ids(raw: Option<&str>) -> Vec<i64> {
    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(values)) => values
            .iter()
            .filter_map(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

async fn lock_generation_period(
    tx: &mut Transaction<'_, Postgres>,
    login: &str,
    year: i32,
    month: i32,
) -> Result<(), sqlx::Error> {
    let lock_key = format!("{login}:{year:04}{month:02}");
    sqlx::query("SELECT pg_advisory_xact_lock($1, hashtext($2))")
        .bind(GUTSCHRIFT_LOCK_NAMESPACE)
        .bind(lock_key)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn load_existing(
    tx: &mut Transaction<'_, Postgres>,
    affiliate_login: &str,
    year: i32,
    month: i32,
) -> Result<Option<StoredGutschriftRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {SELECT_GUTSCHRIFT_COLUMNS} \
         FROM affiliate_gutschriften \
         WHERE affiliate_twitch_login = $1 AND period_year = $2 AND period_month = $3"
    );
    let row = sqlx::query(&sql)
        .bind(affiliate_login)
        .bind(year)
        .bind(month)
        .fetch_optional(&mut **tx)
        .await?;
    row.map(StoredGutschriftRow::from_row).transpose()
}

async fn load_by_id(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
) -> Result<StoredGutschriftRow, GutschriftError> {
    let sql = format!(
        "SELECT {SELECT_GUTSCHRIFT_COLUMNS} FROM affiliate_gutschriften WHERE id::bigint = $1"
    );
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(GutschriftError::StoredRowMissing)?;
    StoredGutschriftRow::from_row(row).map_err(GutschriftError::Db)
}

async fn load_commissions(
    tx: &mut Transaction<'_, Postgres>,
    affiliate_login: &str,
    period_start: &str,
    next_period_start: &str,
) -> Result<Vec<CommissionRow>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id::bigint AS id, commission_cents::bigint AS commission_cents, currency
        FROM affiliate_commissions
        WHERE affiliate_twitch_login = $1
          AND status = $2
          AND created_at >= $3
          AND created_at < $4
        ORDER BY id ASC
        "#,
    )
    .bind(affiliate_login)
    .bind(TRANSFERRED_STATUS)
    .bind(period_start)
    .bind(next_period_start)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(CommissionRow {
                id: row.try_get("id")?,
                commission_cents: row.try_get("commission_cents")?,
                currency: row
                    .try_get::<Option<String>, _>("currency")?
                    .unwrap_or_else(|| "eur".to_string()),
            })
        })
        .collect()
}

async fn next_gutschrift_number(
    tx: &mut Transaction<'_, Postgres>,
    year_month: &str,
) -> Result<String, GutschriftError> {
    // Schema garantiert durch Migration 20260617030000 (year_month/last_seq); Pythons Legacy-counter_year/last_counter-Introspektion entfällt.
    if year_month.len() != 6 || !year_month.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(GutschriftError::InvalidYearMonth);
    }
    sqlx::query(
        r#"
        INSERT INTO affiliate_gutschrift_counter (year_month, last_seq)
        VALUES ($1, 0)
        ON CONFLICT(year_month) DO NOTHING
        "#,
    )
    .bind(year_month)
    .execute(&mut **tx)
    .await?;
    let row = sqlx::query(
        r#"
        UPDATE affiliate_gutschrift_counter
        SET last_seq = last_seq + 1
        WHERE year_month = $1
        RETURNING last_seq
        "#,
    )
    .bind(year_month)
    .fetch_optional(&mut **tx)
    .await?;
    let next_seq = row
        .map(|row| row.try_get::<i32, _>("last_seq"))
        .transpose()?
        .unwrap_or_default();
    if next_seq <= 0 {
        return Err(GutschriftError::CounterAllocation);
    }
    Ok(format!("GS-{year_month}-{next_seq:04}"))
}

#[allow(clippy::too_many_arguments)]
async fn store_gutschrift(
    tx: &mut Transaction<'_, Postgres>,
    affiliate_login: &str,
    year: i32,
    month: i32,
    gutschrift_number: &str,
    net_amount_cents: i64,
    vat_amount_cents: i64,
    gross_amount_cents: i64,
    profile: &PiiPayload,
    seller: &AffiliateGutschriftSeller,
    pdf_blob: Vec<u8>,
    pdf_generated_at: &str,
    commission_ids: &[i64],
    created_at: &str,
) -> Result<StoredGutschriftRow, GutschriftError> {
    let affiliate_name = profile.full_name.trim().to_string();
    let affiliate_address = affiliate_address(profile);
    let affiliate_tax_id = affiliate_tax_id(profile);
    let affiliate_ust_status = profile.ust_status.trim().to_lowercase();
    let vat_rate_percent = if affiliate_ust_status == "regelbesteuert" {
        "19.00"
    } else {
        "0.00"
    };
    let issuer_name = seller.seller_name();
    let issuer_address = seller.seller_address();
    let issuer_tax_id = seller.tax_id.trim().to_string();
    let commission_ids_json = serde_json::to_string(commission_ids)
        .map_err(|error| GutschriftError::Pdf(error.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO affiliate_gutschriften (
            gutschrift_number,
            affiliate_twitch_login,
            period_year,
            period_month,
            net_amount_cents,
            vat_rate_percent,
            vat_amount_cents,
            gross_amount_cents,
            affiliate_name,
            affiliate_address,
            affiliate_tax_id,
            affiliate_ust_status,
            issuer_name,
            issuer_address,
            issuer_tax_id,
            pdf_blob,
            pdf_generated_at,
            email_sent_at,
            email_error,
            commission_ids,
            created_at
        ) VALUES ($1, $2, $3, $4, $5, $6::numeric(5,2), $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, NULL, NULL, $18, $19)
        ON CONFLICT(affiliate_twitch_login, period_year, period_month) DO UPDATE SET
            gutschrift_number = excluded.gutschrift_number,
            net_amount_cents = excluded.net_amount_cents,
            vat_rate_percent = excluded.vat_rate_percent,
            vat_amount_cents = excluded.vat_amount_cents,
            gross_amount_cents = excluded.gross_amount_cents,
            affiliate_name = excluded.affiliate_name,
            affiliate_address = excluded.affiliate_address,
            affiliate_tax_id = excluded.affiliate_tax_id,
            affiliate_ust_status = excluded.affiliate_ust_status,
            issuer_name = excluded.issuer_name,
            issuer_address = excluded.issuer_address,
            issuer_tax_id = excluded.issuer_tax_id,
            pdf_blob = excluded.pdf_blob,
            pdf_generated_at = excluded.pdf_generated_at,
            email_sent_at = excluded.email_sent_at,
            email_error = excluded.email_error,
            commission_ids = excluded.commission_ids
        "#,
    )
    .bind(gutschrift_number)
    .bind(affiliate_login)
    .bind(year)
    .bind(month)
    .bind(net_amount_cents)
    .bind(vat_rate_percent)
    .bind(vat_amount_cents)
    .bind(gross_amount_cents)
    .bind(affiliate_name)
    .bind(affiliate_address)
    .bind(if affiliate_tax_id.trim().is_empty() {
        None
    } else {
        Some(affiliate_tax_id)
    })
    .bind(affiliate_ust_status)
    .bind(issuer_name)
    .bind(issuer_address)
    .bind(issuer_tax_id)
    .bind(pdf_blob)
    .bind(pdf_generated_at)
    .bind(commission_ids_json)
    .bind(created_at)
    .execute(&mut **tx)
    .await?;

    load_existing(tx, affiliate_login, year, month)
        .await?
        .ok_or(GutschriftError::StoredRowMissing)
}

async fn send_email_for_row(
    tx: &mut Transaction<'_, Postgres>,
    email_sender: Option<&dyn AffiliateGutschriftEmailSender>,
    recipient_email: &str,
    recipient_name: &str,
    row: StoredGutschriftRow,
    currency: &str,
) -> Result<(StoredGutschriftRow, String), GutschriftError> {
    let Some(sender) = email_sender else {
        return Ok((row, STATUS_GENERATED.to_string()));
    };
    let normalized_email = recipient_email.trim();
    if normalized_email.is_empty() {
        return Ok((row, STATUS_GENERATED.to_string()));
    }
    let Some(pdf_blob) = row.pdf_blob.clone().filter(|blob| !blob.is_empty()) else {
        return Ok((row, STATUS_GENERATED.to_string()));
    };

    let period = period_label(row.period_year, row.period_month)?;
    let filename = format!(
        "{}.pdf",
        row.gutschrift_number
            .trim()
            .if_empty("gutschrift")
            .replace('"', "")
    );
    let message = AffiliateGutschriftEmail {
        recipient_email: normalized_email.to_string(),
        recipient_name: recipient_name.trim().to_string(),
        gutschrift_number: row.gutschrift_number.clone(),
        period_label: period,
        gross_amount_label: format_eur_cents(row.gross_amount_cents, currency),
        pdf_bytes: pdf_blob,
        filename,
    };

    if let Err(error) = sender.send_gutschrift(&message) {
        let error_text = truncate_chars(&error.to_string(), 500);
        sqlx::query(
            r#"
            UPDATE affiliate_gutschriften
            SET email_sent_at = NULL, email_error = $1
            WHERE id::bigint = $2
            "#,
        )
        .bind(error_text)
        .bind(row.id)
        .execute(&mut **tx)
        .await?;
        let updated = load_by_id(tx, row.id).await?;
        return Ok((updated, STATUS_EMAIL_FAILED.to_string()));
    }

    let sent_at = now_iso();
    sqlx::query(
        r#"
        UPDATE affiliate_gutschriften
        SET email_sent_at = $1, email_error = NULL
        WHERE id::bigint = $2
        "#,
    )
    .bind(sent_at)
    .bind(row.id)
    .execute(&mut **tx)
    .await?;
    let updated = load_by_id(tx, row.id).await?;
    Ok((updated, STATUS_EMAILED.to_string()))
}

fn generate_gutschrift_pdf(data: &GutschriftPdfData) -> Result<Vec<u8>, GutschriftError> {
    let mut ops = Vec::new();
    draw_box(&mut ops, 20.0, 221.0, 170.0, 25.0);
    draw_box(&mut ops, 20.0, 178.0, 170.0, 25.0);
    draw_box(&mut ops, 20.0, 59.0, 170.0, 16.0);

    push_text(
        &mut ops,
        20.0,
        280.0,
        18.0,
        BuiltinFont::HelveticaBold,
        "GUTSCHRIFT",
    );
    push_text(
        &mut ops,
        150.0,
        280.0,
        10.0,
        BuiltinFont::Helvetica,
        &data.gutschrift_number,
    );
    push_text(
        &mut ops,
        20.0,
        270.0,
        10.0,
        BuiltinFont::Helvetica,
        &format!("Datum: {}", data.issue_date_label),
    );

    push_text(
        &mut ops,
        20.0,
        255.0,
        10.0,
        BuiltinFont::HelveticaBold,
        "Leistungsempfaenger (Aussteller):",
    );
    push_multiline(
        &mut ops,
        23.0,
        241.0,
        5.0,
        10.0,
        BuiltinFont::Helvetica,
        &join_non_empty(&[&data.issuer_name, &data.issuer_address, &data.issuer_tax_id]),
    );

    push_text(
        &mut ops,
        20.0,
        212.0,
        10.0,
        BuiltinFont::HelveticaBold,
        "Leistender (Empfaenger der Gutschrift):",
    );
    push_multiline(
        &mut ops,
        23.0,
        198.0,
        5.0,
        10.0,
        BuiltinFont::Helvetica,
        &join_non_empty(&[
            &data.affiliate_name,
            &data.affiliate_address,
            &data.affiliate_tax_id,
        ]),
    );

    push_text(
        &mut ops,
        20.0,
        166.0,
        10.0,
        BuiltinFont::Helvetica,
        &format!("Leistungszeitraum: {}", data.period_label),
    );
    push_multiline(
        &mut ops,
        20.0,
        154.0,
        6.0,
        10.0,
        BuiltinFont::Helvetica,
        &format!(
            "Vermittlungsleistung (Provision 30 %) fuer {}",
            data.period_label
        ),
    );
    push_text(
        &mut ops,
        20.0,
        137.0,
        10.0,
        BuiltinFont::Helvetica,
        &format!("Nettobetrag: {}", data.net_amount_label),
    );
    if data
        .affiliate_ust_status
        .trim()
        .eq_ignore_ascii_case("regelbesteuert")
    {
        push_text(
            &mut ops,
            20.0,
            128.0,
            10.0,
            BuiltinFont::Helvetica,
            &format!(
                "USt {}: {}",
                data.vat_rate_label.if_empty("19 %"),
                data.vat_amount_label
            ),
        );
    } else {
        push_text(
            &mut ops,
            20.0,
            128.0,
            10.0,
            BuiltinFont::Helvetica,
            "Gem. § 19 UStG: keine USt",
        );
    }
    push_text(
        &mut ops,
        20.0,
        116.0,
        11.0,
        BuiltinFont::HelveticaBold,
        &format!("Gesamtbetrag: {}", data.gross_amount_label),
    );
    push_multiline(
        &mut ops,
        23.0,
        70.0,
        6.0,
        10.0,
        BuiltinFont::Helvetica,
        "Diese Gutschrift gilt als Rechnung im Sinne des § 14 Abs. 2 Satz 2 UStG.",
    );

    let mut doc = PdfDocument::new("Gutschrift");
    let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
    let mut warnings = Vec::new();
    let bytes = doc
        .with_pages(vec![page])
        .save(&PdfSaveOptions::default(), &mut warnings);
    if bytes.is_empty() {
        return Err(GutschriftError::Pdf("empty PDF output".to_string()));
    }
    Ok(bytes)
}

fn draw_box(ops: &mut Vec<Op>, x: f32, y: f32, width: f32, height: f32) {
    ops.push(Op::SetOutlineColor { col: black() });
    ops.push(Op::SetOutlineThickness { pt: Pt(0.5) });
    let mut rect = Rect::from_xywh(
        Mm(x).into(),
        Mm(y).into(),
        Mm(width).into(),
        Mm(height).into(),
    );
    rect.mode = Some(PaintMode::Stroke);
    ops.push(Op::DrawPolygon {
        polygon: rect.to_polygon(),
    });
}

fn push_text(ops: &mut Vec<Op>, x: f32, y: f32, size: f32, font: BuiltinFont, text: &str) {
    ops.push(Op::StartTextSection);
    ops.push(Op::SetTextCursor {
        pos: Point::new(Mm(x), Mm(y)),
    });
    ops.push(Op::SetFont {
        font: PdfFontHandle::Builtin(font),
        size: Pt(size),
    });
    ops.push(Op::SetFillColor { col: black() });
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(pdf_safe(text))],
    });
    ops.push(Op::EndTextSection);
}

fn push_multiline(
    ops: &mut Vec<Op>,
    x: f32,
    y: f32,
    line_height: f32,
    size: f32,
    font: BuiltinFont,
    text: &str,
) {
    for (idx, line) in text.lines().enumerate() {
        push_text(ops, x, y - (idx as f32 * line_height), size, font, line);
    }
}

fn black() -> Color {
    Color::Rgb(Rgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        icc_profile: None,
    })
}

fn pdf_safe(value: &str) -> String {
    value
        .chars()
        .map(|ch| if u32::from(ch) <= 0xFF { ch } else { '?' })
        .collect()
}

fn join_non_empty(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_created_at(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(value) = DateTime::parse_from_rfc3339(raw) {
        return Some(value.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|value| Utc.from_utc_datetime(&value))
}

fn mailbox(name: Option<String>, email: &str) -> Result<Mailbox, AffiliateEmailError> {
    let address = email
        .trim()
        .parse::<Address>()
        .map_err(|error| AffiliateEmailError::Message(error.to_string()))?;
    Ok(Mailbox::new(name, address))
}

fn non_empty_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn load_secret<F>(loader: &mut F, keys: &[&str]) -> String
where
    F: FnMut(&[&str]) -> Option<String>,
{
    loader(keys).unwrap_or_default()
}

fn load_or_default<F>(loader: &mut F, keys: &[&str], default: &str) -> String
where
    F: FnMut(&[&str]) -> Option<String>,
{
    let value = load_secret(loader, keys);
    value.trim().if_empty(default).to_string()
}

fn normalize_bool(value: &str, default: bool) -> bool {
    let raw = value.trim().to_lowercase();
    if raw.is_empty() {
        default
    } else {
        matches!(raw.as_str(), "1" | "true" | "yes" | "on")
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

trait IfEmpty {
    fn if_empty<'a>(&'a self, default: &'a str) -> &'a str;
}

impl IfEmpty for str {
    fn if_empty<'a>(&'a self, default: &'a str) -> &'a str {
        if self.trim().is_empty() {
            default
        } else {
            self
        }
    }
}

impl IfEmpty for String {
    fn if_empty<'a>(&'a self, default: &'a str) -> &'a str {
        self.as_str().if_empty(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::Mutex;

    use crate::affiliate_pii::{save_affiliate_pii, PiiInput};

    fn test_cipher() -> FieldCipher {
        FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap()
    }

    async fn connect(schema: &str) -> Option<PgPool> {
        let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => dsn,
            Err(_) => {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                return None;
            }
        };
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
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        create_tables(&pool).await;
        Some(pool)
    }

    async fn create_tables(pool: &PgPool) {
        for ddl in [
            "CREATE TABLE affiliate_accounts (\
                twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT NOT NULL, display_name TEXT, \
                email TEXT NOT NULL, full_name TEXT NOT NULL, address_line1 TEXT NOT NULL, \
                address_city TEXT NOT NULL, address_zip TEXT NOT NULL, address_country TEXT NOT NULL DEFAULT 'DE', \
                stripe_account_id TEXT, stripe_connected_at TEXT, stripe_connect_status TEXT DEFAULT 'pending', \
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL, is_active INTEGER NOT NULL DEFAULT 1)",
            "CREATE TABLE affiliate_pii (\
                twitch_login TEXT PRIMARY KEY, full_name_enc BYTEA, email_enc BYTEA, \
                address_line1_enc BYTEA, address_city_enc BYTEA, address_zip_enc BYTEA, tax_id_enc BYTEA, \
                address_country TEXT NOT NULL DEFAULT 'DE', ust_status TEXT NOT NULL DEFAULT 'unknown', updated_at TEXT NOT NULL)",
            "CREATE TABLE affiliate_commissions (\
                id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT NOT NULL, \
                streamer_login TEXT NOT NULL, stripe_event_id TEXT UNIQUE NOT NULL, stripe_invoice_id TEXT, \
                stripe_customer_id TEXT, stripe_transfer_id TEXT, brutto_cents INTEGER NOT NULL, \
                commission_cents INTEGER NOT NULL, currency TEXT NOT NULL DEFAULT 'eur', status TEXT NOT NULL DEFAULT 'pending', \
                period_start TEXT, period_end TEXT, created_at TEXT NOT NULL, transferred_at TEXT, error_message TEXT)",
            "CREATE TABLE affiliate_gutschrift_counter (year_month TEXT PRIMARY KEY, last_seq INTEGER NOT NULL DEFAULT 0)",
            "CREATE TABLE affiliate_gutschriften (\
                id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, gutschrift_number TEXT UNIQUE NOT NULL, \
                affiliate_twitch_login TEXT NOT NULL, period_year INTEGER NOT NULL, period_month INTEGER NOT NULL, \
                net_amount_cents INTEGER NOT NULL, vat_rate_percent NUMERIC(5,2) NOT NULL DEFAULT 0, \
                vat_amount_cents INTEGER NOT NULL DEFAULT 0, gross_amount_cents INTEGER NOT NULL, \
                affiliate_name TEXT NOT NULL, affiliate_address TEXT NOT NULL, affiliate_tax_id TEXT, \
                affiliate_ust_status TEXT NOT NULL, issuer_name TEXT NOT NULL, issuer_address TEXT NOT NULL, \
                issuer_tax_id TEXT NOT NULL, pdf_blob BYTEA, pdf_generated_at TEXT, email_sent_at TEXT, \
                email_error TEXT, commission_ids TEXT, created_at TEXT NOT NULL, \
                UNIQUE (affiliate_twitch_login, period_year, period_month))",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    async fn insert_affiliate(pool: &PgPool, login: &str, user_id: &str, display_name: &str) {
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts (
                twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                address_city, address_zip, address_country, stripe_connect_status,
                created_at, updated_at, is_active
            ) VALUES ($1, $2, $3, '', '', '', '', '', '', 'pending', '2026-01-01T10:00:00+00:00', '2026-01-01T10:00:00+00:00', 1)
            "#,
        )
        .bind(login)
        .bind(user_id)
        .bind(display_name)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_commission(
        pool: &PgPool,
        affiliate_login: &str,
        event_id: &str,
        commission_cents: i64,
        created_at: &str,
        status: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO affiliate_commissions (
                affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id,
                stripe_customer_id, stripe_transfer_id, brutto_cents, commission_cents,
                currency, status, period_start, period_end, created_at
            ) VALUES ($1, 'streamer_one', $2, $3, 'cus_123', NULL, $4, $5, 'eur', $6, '2026-02-01T00:00:00+00:00', '2026-02-28T23:59:59+00:00', $7)
            "#,
        )
        .bind(affiliate_login)
        .bind(event_id)
        .bind(format!("in_{event_id}"))
        .bind(commission_cents * 3)
        .bind(commission_cents)
        .bind(status)
        .bind(created_at)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn save_profile(
        pool: &PgPool,
        cipher: &FieldCipher,
        login: &str,
        ust_status: &str,
        vat_id: Option<&str>,
    ) {
        let input = PiiInput {
            full_name: Some(match login {
                "affiliate_two" => "Affiliate Two".to_string(),
                _ => "Affiliate One".to_string(),
            }),
            email: Some(match login {
                "affiliate_two" => "affiliate2@example.com".to_string(),
                _ => "affiliate@example.com".to_string(),
            }),
            address_line1: Some(match login {
                "affiliate_two" => "Musterweg 2".to_string(),
                _ => "Musterstr. 1".to_string(),
            }),
            address_city: Some(match login {
                "affiliate_two" => "Koeln".to_string(),
                _ => "Berlin".to_string(),
            }),
            address_zip: Some(match login {
                "affiliate_two" => "50667".to_string(),
                _ => "10115".to_string(),
            }),
            address_country: Some("DE".to_string()),
            tax_id: Some(match login {
                "affiliate_two" => "98/765/43210".to_string(),
                _ => "12/345/67890".to_string(),
            }),
            vat_id: vat_id.map(ToOwned::to_owned),
            ust_status: Some(ust_status.to_string()),
        };
        save_affiliate_pii(pool, cipher, login, &input)
            .await
            .unwrap();
    }

    fn seller() -> AffiliateGutschriftSeller {
        AffiliateGutschriftSeller {
            name: "Deadlock Partner Network".to_string(),
            company: "EarlySalty GmbH".to_string(),
            street: "Issuer Street 9".to_string(),
            postal_code: "40213".to_string(),
            city: "Duesseldorf".to_string(),
            country: "DE".to_string(),
            email: String::new(),
            website: String::new(),
            tax_id: "DE999999999".to_string(),
        }
    }

    #[derive(Default)]
    struct RecordingEmailSender {
        calls: Mutex<Vec<AffiliateGutschriftEmail>>,
    }

    impl AffiliateGutschriftEmailSender for RecordingEmailSender {
        fn send_gutschrift(
            &self,
            message: &AffiliateGutschriftEmail,
        ) -> Result<(), AffiliateEmailError> {
            self.calls.lock().unwrap().push(message.clone());
            Ok(())
        }
    }

    #[test]
    fn period_label_und_vat_half_up() {
        assert_eq!(period_label(2026, 3).unwrap(), "Maerz 2026");
        assert_eq!(vat_amount_cents(1000, "regelbesteuert").unwrap(), 190);
        assert_eq!(vat_amount_cents(999, "regelbesteuert").unwrap(), 190);
        assert_eq!(vat_amount_cents(1000, "kleinunternehmer").unwrap(), 0);
    }

    #[test]
    fn smtp_settings_loader_paritaet() {
        let settings = AffiliateEmailSettings::from_secret_loader(|keys| match keys[0] {
            "AFFILIATE_GUTSCHRIFT_SMTP_HOST" => Some("smtp.example.test".into()),
            "AFFILIATE_GUTSCHRIFT_SMTP_FROM" => Some("billing@example.test".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(settings.port, 587);
        assert_eq!(settings.from_name, "Deadlock Partner Network");
        assert!(settings.starttls);
        assert!(!settings.use_ssl);
    }

    #[test]
    fn pdf_enthaelt_pdf_header() {
        let bytes = generate_gutschrift_pdf(&GutschriftPdfData {
            gutschrift_number: "GS-202602-0001".into(),
            issue_date_label: "01.03.2026".into(),
            period_label: "Februar 2026".into(),
            net_amount_label: "10,00 EUR".into(),
            vat_rate_label: "19 %".into(),
            vat_amount_label: "1,90 EUR".into(),
            gross_amount_label: "11,90 EUR".into(),
            affiliate_name: "Affiliate One".into(),
            affiliate_address: "Musterstr. 1\n10115 Berlin\nDE".into(),
            affiliate_tax_id: "Steuernummer: 12/345/67890".into(),
            affiliate_ust_status: "regelbesteuert".into(),
            issuer_name: "EarlySalty GmbH".into(),
            issuer_address: "Issuer Street 9\n40213 Duesseldorf\nDE".into(),
            issuer_tax_id: "DE999999999".into(),
        })
        .unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[tokio::test]
    async fn generate_for_period_blocks_when_ust_status_is_unknown() {
        let Some(pool) = connect("aff_gs_blocked").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let cipher = test_cipher();
        insert_affiliate(&pool, "affiliate_one", "1001", "Affiliate One").await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_blocked",
            1500,
            "2026-02-10T12:00:00+00:00",
            "transferred",
        )
        .await;
        save_profile(&pool, &cipher, "affiliate_one", "unknown", None).await;

        let result = generate_for_period(
            &pool,
            &cipher,
            "affiliate_one",
            2026,
            2,
            None,
            Some(&seller()),
            false,
        )
        .await
        .unwrap();

        assert!(!result.ok);
        assert_eq!(result.status.as_deref(), Some("blocked"));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_gutschriften")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn generate_for_period_creates_snapshot_with_monthly_number() {
        let Some(pool) = connect("aff_gs_snapshot").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let cipher = test_cipher();
        insert_affiliate(&pool, "affiliate_one", "1001", "Affiliate One").await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_small_1",
            1200,
            "2026-02-10T12:00:00+00:00",
            "transferred",
        )
        .await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_small_2",
            800,
            "2026-02-15T08:00:00+00:00",
            "transferred",
        )
        .await;
        save_profile(&pool, &cipher, "affiliate_one", "kleinunternehmer", None).await;

        let result = generate_for_period(
            &pool,
            &cipher,
            "affiliate_one",
            2026,
            2,
            None,
            Some(&seller()),
            false,
        )
        .await
        .unwrap();

        let document = result.document.unwrap();
        assert_eq!(document.status, "generated");
        assert_eq!(document.net_amount_cents, 2000);
        assert_eq!(document.vat_amount_cents, 0);
        assert_eq!(document.gross_amount_cents, 2000);
        assert_eq!(document.gutschrift_number, "GS-202602-0001");
        assert_eq!(document.commission_count, 2);
        assert_eq!(
            document.note_text,
            "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet."
        );

        let row = sqlx::query(
            "SELECT affiliate_name, affiliate_address, affiliate_tax_id, affiliate_ust_status, \
             issuer_name, issuer_address, issuer_tax_id, commission_ids, pdf_generated_at, \
             email_sent_at, email_error FROM affiliate_gutschriften \
             WHERE affiliate_twitch_login = 'affiliate_one' AND period_year = 2026 AND period_month = 2",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.try_get::<String, _>("affiliate_name").unwrap(),
            "Affiliate One"
        );
        assert_eq!(
            row.try_get::<String, _>("affiliate_address").unwrap(),
            "Musterstr. 1\n10115 Berlin\nDE"
        );
        assert_eq!(
            row.try_get::<String, _>("affiliate_tax_id").unwrap(),
            "Steuernummer: 12/345/67890"
        );
        assert_eq!(
            row.try_get::<String, _>("affiliate_ust_status").unwrap(),
            "kleinunternehmer"
        );
        assert_eq!(
            row.try_get::<String, _>("issuer_name").unwrap(),
            "EarlySalty GmbH"
        );
        assert_eq!(
            row.try_get::<String, _>("issuer_address").unwrap(),
            "Issuer Street 9\n40213 Duesseldorf\nDE"
        );
        assert_eq!(
            row.try_get::<String, _>("issuer_tax_id").unwrap(),
            "DE999999999"
        );
        assert_eq!(row.try_get::<String, _>("commission_ids").unwrap(), "[1,2]");
        assert!(row
            .try_get::<Option<String>, _>("pdf_generated_at")
            .unwrap()
            .is_some());
        assert!(row
            .try_get::<Option<String>, _>("email_sent_at")
            .unwrap()
            .is_none());
        assert!(row
            .try_get::<Option<String>, _>("email_error")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn generate_monthly_numbers_are_sequential_per_month() {
        let Some(pool) = connect("aff_gs_sequence").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let cipher = test_cipher();
        insert_affiliate(&pool, "affiliate_one", "1001", "Affiliate One").await;
        insert_affiliate(&pool, "affiliate_two", "1002", "Affiliate Two").await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_seq_1",
            1000,
            "2026-02-05T12:00:00+00:00",
            "transferred",
        )
        .await;
        insert_commission(
            &pool,
            "affiliate_two",
            "evt_seq_2",
            900,
            "2026-02-18T12:00:00+00:00",
            "transferred",
        )
        .await;
        save_profile(&pool, &cipher, "affiliate_one", "kleinunternehmer", None).await;
        save_profile(&pool, &cipher, "affiliate_two", "kleinunternehmer", None).await;

        let results = generate_monthly_gutschriften(
            &pool,
            &cipher,
            2026,
            2,
            None,
            Some(&seller()),
            None,
            false,
        )
        .await
        .unwrap();
        let mut numbers = results
            .into_iter()
            .filter_map(|result| result.document.map(|doc| doc.gutschrift_number))
            .collect::<Vec<_>>();
        numbers.sort();
        assert_eq!(numbers, vec!["GS-202602-0001", "GS-202602-0002"]);
    }

    #[tokio::test]
    async fn due_periods_skips_invalid_legacy_login_rows() {
        let Some(pool) = connect("aff_gs_due_skip").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        insert_affiliate(&pool, "affiliate_one", "1001", "Affiliate One").await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_valid_due",
            1000,
            "2026-02-05T12:00:00+00:00",
            "transferred",
        )
        .await;
        insert_commission(
            &pool,
            "https://example.com/not-twitch",
            "evt_invalid_due",
            900,
            "2026-02-06T12:00:00+00:00",
            "transferred",
        )
        .await;

        let periods = due_periods(
            &pool,
            Some(Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(periods, vec![("affiliate_one".to_string(), 2026, 2)]);
    }

    #[tokio::test]
    async fn due_periods_and_run_pending_sort_by_period_then_login() {
        let Some(pool) = connect("aff_gs_due_sort").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let cipher = test_cipher();
        insert_affiliate(&pool, "affiliate_alpha", "1001", "Affiliate Alpha").await;
        insert_affiliate(&pool, "affiliate_beta", "1002", "Affiliate Beta").await;
        insert_affiliate(&pool, "affiliate_zulu", "1003", "Affiliate Zulu").await;
        insert_commission(
            &pool,
            "affiliate_alpha",
            "evt_sort_alpha_feb",
            1000,
            "2026-02-05T12:00:00+00:00",
            "transferred",
        )
        .await;
        insert_commission(
            &pool,
            "affiliate_zulu",
            "evt_sort_zulu_jan",
            900,
            "2026-01-10T12:00:00+00:00",
            "transferred",
        )
        .await;
        insert_commission(
            &pool,
            "affiliate_beta",
            "evt_sort_beta_jan",
            800,
            "2026-01-15T12:00:00+00:00",
            "transferred",
        )
        .await;
        save_profile(&pool, &cipher, "affiliate_beta", "kleinunternehmer", None).await;
        save_profile(&pool, &cipher, "affiliate_zulu", "kleinunternehmer", None).await;

        let periods = due_periods(
            &pool,
            Some(Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(
            periods,
            vec![
                ("affiliate_beta".to_string(), 2026, 1),
                ("affiliate_zulu".to_string(), 2026, 1),
                ("affiliate_alpha".to_string(), 2026, 2),
            ]
        );

        let results = run_pending(
            &pool,
            &cipher,
            None,
            Some(&seller()),
            Some(Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap()),
            2,
        )
        .await
        .unwrap();
        let processed = results
            .iter()
            .map(|result| {
                (
                    result.affiliate_login.as_str(),
                    result.period_year,
                    result.period_month,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            processed,
            vec![("affiliate_beta", 2026, 1), ("affiliate_zulu", 2026, 1)]
        );
    }

    #[tokio::test]
    async fn generate_monthly_skips_invalid_legacy_login_rows() {
        let Some(pool) = connect("aff_gs_monthly_skip").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let cipher = test_cipher();
        insert_affiliate(&pool, "affiliate_one", "1001", "Affiliate One").await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_valid_monthly",
            1000,
            "2026-02-05T12:00:00+00:00",
            "transferred",
        )
        .await;
        insert_commission(
            &pool,
            "https://example.com/not-twitch",
            "evt_invalid_monthly",
            900,
            "2026-02-06T12:00:00+00:00",
            "transferred",
        )
        .await;
        save_profile(&pool, &cipher, "affiliate_one", "kleinunternehmer", None).await;

        let results = generate_monthly_gutschriften(
            &pool,
            &cipher,
            2026,
            2,
            None,
            Some(&seller()),
            None,
            false,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].document.as_ref().unwrap().gutschrift_number,
            "GS-202602-0001"
        );
    }

    #[tokio::test]
    async fn generate_for_period_applies_19_percent_vat_and_sends_email() {
        let Some(pool) = connect("aff_gs_vat_email").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let cipher = test_cipher();
        let sender = RecordingEmailSender::default();
        insert_affiliate(&pool, "affiliate_one", "1001", "Affiliate One").await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_regular",
            1000,
            "2026-02-20T10:00:00+00:00",
            "transferred",
        )
        .await;
        save_profile(
            &pool,
            &cipher,
            "affiliate_one",
            "regelbesteuert",
            Some("DE123456789"),
        )
        .await;

        let result = generate_for_period(
            &pool,
            &cipher,
            "affiliate_one",
            2026,
            2,
            Some(&sender),
            Some(&seller()),
            false,
        )
        .await
        .unwrap();

        let document = result.document.unwrap();
        assert_eq!(document.status, "emailed");
        assert_eq!(document.net_amount_cents, 1000);
        assert_eq!(document.vat_amount_cents, 190);
        assert_eq!(document.gross_amount_cents, 1190);
        assert_eq!(document.gutschrift_number, "GS-202602-0001");
        assert_eq!(sender.calls.lock().unwrap().len(), 1);
        let row = sqlx::query(
            "SELECT email_sent_at, email_error FROM affiliate_gutschriften WHERE id::bigint = $1",
        )
        .bind(document.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row
            .try_get::<Option<String>, _>("email_sent_at")
            .unwrap()
            .is_some());
        assert!(row
            .try_get::<Option<String>, _>("email_error")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn generate_for_period_is_idempotent_without_force() {
        let Some(pool) = connect("aff_gs_idempotent").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let cipher = test_cipher();
        insert_affiliate(&pool, "affiliate_one", "1001", "Affiliate One").await;
        insert_commission(
            &pool,
            "affiliate_one",
            "evt_idem",
            1000,
            "2026-02-20T10:00:00+00:00",
            "transferred",
        )
        .await;
        save_profile(&pool, &cipher, "affiliate_one", "kleinunternehmer", None).await;

        let first = generate_for_period(
            &pool,
            &cipher,
            "affiliate_one",
            2026,
            2,
            None,
            Some(&seller()),
            false,
        )
        .await
        .unwrap();
        let second = generate_for_period(
            &pool,
            &cipher,
            "affiliate_one",
            2026,
            2,
            None,
            Some(&seller()),
            false,
        )
        .await
        .unwrap();

        assert_eq!(
            first.document.as_ref().unwrap().gutschrift_number,
            second.document.as_ref().unwrap().gutschrift_number
        );
        assert_eq!(second.action.as_deref(), Some("existing"));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_gutschriften")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let seq: i32 = sqlx::query_scalar(
            "SELECT last_seq FROM affiliate_gutschrift_counter WHERE year_month = '202602'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(seq, 1);
    }
}

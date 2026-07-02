//! Read-Only-Datenschicht für die Admin-Affiliate-Übersichten.
//!
//! Port von `bot/analytics/admin_affiliate_queries.py`. Aggregiert das
//! Affiliate-Programm (Konten, Claims, Provisionen, Gutschriften) für das
//! Admin-Dashboard. Alle Timestamp-Spalten sind in Prod TEXT (ISO-Strings),
//! Geldbeträge in Cent (INTEGER).
//!
//! Quellen-Status: stats portiert; list/detail/gutschriften folgen als Teil 2+.

use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_crypto::FieldCipher;

use crate::affiliate_gutschrift::{self, GutschriftError};
use crate::affiliate_pii::{build_readiness, load_affiliate_pii, PiiError, PiiPayload};

/// `true` wenn der DB-Fehler auf fehlendes Schema deutet (→ Nullwerte statt 500).
fn is_missing_schema_error(e: &sqlx::Error) -> bool {
    let s = e.to_string().to_lowercase();
    [
        "does not exist",
        "no such table",
        "undefined table",
        "no such column",
        "undefined column",
    ]
    .iter()
    .any(|m| s.contains(m))
}

fn cents_to_amount(cents: i64) -> f64 {
    (cents as f64) / 100.0
}

fn zero_stats() -> Value {
    json!({
        "total_affiliates": 0,
        "active_affiliates": 0,
        "total_claims": 0,
        "total_provision": 0.0,
        "this_month_claims": 0,
        "this_month_provision": 0.0,
        "total_gutschriften": 0,
        "total_gutschrift_amount": 0.0,
        "pending_email_gutschriften": 0,
    })
}

async fn load_admin_pii(
    pool: &PgPool,
    cipher: &FieldCipher,
    login: &str,
) -> Result<PiiPayload, PiiError> {
    match load_affiliate_pii(pool, cipher, login).await {
        Ok(pii) => Ok(pii),
        Err(PiiError::Db(e)) if is_missing_schema_error(&e) => Ok(PiiPayload::default_payload()),
        Err(e) => Err(e),
    }
}

/// Aggregierte Affiliate-Programm-Statistik (Python `load_admin_affiliate_stats`).
/// Fehlendes Schema → Nullwerte (1:1 Python-`except`), sonst durchgereichter Fehler.
pub async fn load_affiliate_stats(
    pool: &PgPool,
    month_start_iso: &str,
) -> Result<Value, sqlx::Error> {
    match stats_inner(pool, month_start_iso).await {
        Ok(v) => Ok(v),
        Err(e) if is_missing_schema_error(&e) => Ok(zero_stats()),
        Err(e) => Err(e),
    }
}

async fn stats_inner(pool: &PgPool, month_start_iso: &str) -> Result<Value, sqlx::Error> {
    let stats = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "total_affiliates!",
               COALESCE(SUM(CASE WHEN is_active = 1 THEN 1 ELSE 0 END), 0)::bigint AS "active_affiliates!"
        FROM affiliate_accounts
        "#,
    )
    .fetch_one(pool)
    .await?;

    let claims = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "total_claims!",
               COALESCE(SUM(CASE WHEN claimed_at >= $1 THEN 1 ELSE 0 END), 0)::bigint AS "this_month_claims!"
        FROM affiliate_streamer_claims
        "#,
        month_start_iso
    )
    .fetch_one(pool)
    .await?;

    let commission = sqlx::query!(
        r#"
        SELECT COALESCE(SUM(commission_cents), 0)::bigint AS "total_provision_cents!",
               COALESCE(
                   SUM(CASE
                       WHEN created_at >= $1 AND status IN ('pending', 'transferred')
                       THEN commission_cents ELSE 0
                   END),
                   0
               )::bigint AS "this_month_provision_cents!"
        FROM affiliate_commissions
        WHERE status IN ('pending', 'transferred')
        "#,
        month_start_iso
    )
    .fetch_one(pool)
    .await?;

    // Gutschrift-Summary (Python `_admin_affiliate_gutschriften_summary` — hier nur
    // die drei von stats genutzten Felder).
    let gutschriften = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "total_gutschriften!",
               COALESCE(SUM(gross_amount_cents), 0)::bigint AS "total_gutschrift_cents!",
               COALESCE(SUM(CASE WHEN pdf_generated_at IS NOT NULL AND email_sent_at IS NULL THEN 1 ELSE 0 END), 0)::bigint AS "pending_email!"
        FROM affiliate_gutschriften
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(json!({
        "total_affiliates": stats.total_affiliates,
        "active_affiliates": stats.active_affiliates,
        "total_claims": claims.total_claims,
        "total_provision": cents_to_amount(commission.total_provision_cents),
        "this_month_claims": claims.this_month_claims,
        "this_month_provision": cents_to_amount(commission.this_month_provision_cents),
        "total_gutschriften": gutschriften.total_gutschriften,
        "total_gutschrift_amount": cents_to_amount(gutschriften.total_gutschrift_cents),
        "pending_email_gutschriften": gutschriften.pending_email,
    }))
}

/// Eine Zeile der Affiliate-Liste.
type ListRow = (
    Option<String>, // twitch_login
    Option<String>, // display_name
    i64,            // is_active
    Option<String>, // created_at
    i64,            // total_claims
    i64,            // total_provision_cents
    Option<String>, // last_claim_at
    Option<String>, // ust_status
    i64,            // has_pii
);

const LIST_CLAIM_COMM_JOINS: &str = "\
    LEFT JOIN (SELECT affiliate_twitch_login, COUNT(*) AS total_claims, MAX(claimed_at) AS last_claim_at \
               FROM affiliate_streamer_claims GROUP BY affiliate_twitch_login) claim_stats \
      ON claim_stats.affiliate_twitch_login = a.twitch_login \
    LEFT JOIN (SELECT affiliate_twitch_login, \
                      SUM(CASE WHEN status IN ('pending', 'transferred') THEN commission_cents ELSE 0 END) AS total_provision \
               FROM affiliate_commissions GROUP BY affiliate_twitch_login) comm_stats \
      ON comm_stats.affiliate_twitch_login = a.twitch_login \
    ORDER BY a.created_at DESC";

fn map_list_rows(rows: Vec<ListRow>) -> Value {
    let affiliates: Vec<Value> = rows
        .into_iter()
        .map(
            |(
                login,
                display_name,
                is_active,
                created_at,
                total_claims,
                total_provision_cents,
                last_claim_at,
                ust_status,
                has_pii,
            )| {
                let ust = ust_status.unwrap_or_default().trim().to_string();
                json!({
                    "login": login.unwrap_or_default().trim(),
                    "display_name": display_name,
                    "active": is_active != 0,
                    "total_claims": total_claims,
                    "total_provision": cents_to_amount(total_provision_cents),
                    "created_at": created_at,
                    "last_claim_at": last_claim_at,
                    "ust_status": if ust.is_empty() { "unknown".to_string() } else { ust },
                    "has_pii": has_pii != 0,
                })
            },
        )
        .collect();
    json!({ "affiliates": affiliates })
}

/// Affiliate-Liste mit Claims- + Provisions-Summen (Python `load_admin_affiliates_list`).
/// Zwei-stufiger Fallback: Vollquery (mit affiliate_pii) → ohne PII → leer.
pub async fn load_affiliates_list(pool: &PgPool) -> Result<Value, sqlx::Error> {
    let full_sql = format!(
        "SELECT a.twitch_login, a.display_name, a.is_active::bigint, a.created_at, \
                COALESCE(claim_stats.total_claims, 0)::bigint, COALESCE(comm_stats.total_provision, 0)::bigint, \
                claim_stats.last_claim_at, COALESCE(pii.ust_status, 'unknown'), \
                (CASE WHEN pii.twitch_login IS NOT NULL THEN 1 ELSE 0 END)::bigint \
         FROM affiliate_accounts a \
         LEFT JOIN affiliate_pii pii ON pii.twitch_login = a.twitch_login {LIST_CLAIM_COMM_JOINS}"
    );
    match sqlx::query_as::<_, ListRow>(&full_sql)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => return Ok(map_list_rows(rows)),
        Err(e) if !is_missing_schema_error(&e) => return Err(e),
        Err(_) => {} // fehlendes Schema (z. B. affiliate_pii) → Fallback ohne PII.
    }

    let no_pii_sql = format!(
        "SELECT a.twitch_login, a.display_name, a.is_active::bigint, a.created_at, \
                COALESCE(claim_stats.total_claims, 0)::bigint, COALESCE(comm_stats.total_provision, 0)::bigint, \
                claim_stats.last_claim_at, 'unknown', 0::bigint \
         FROM affiliate_accounts a {LIST_CLAIM_COMM_JOINS}"
    );
    match sqlx::query_as::<_, ListRow>(&no_pii_sql)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => Ok(map_list_rows(rows)),
        Err(e) if is_missing_schema_error(&e) => Ok(json!({ "affiliates": [] })),
        Err(e) => Err(e),
    }
}

// ── Gutschriften-Liste ────────────────────────────────────────────────────────

/// Deutsche Monatsnamen (Index 1–12; 0 = leer). ASCII „Maerz" wie Python.
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

#[derive(sqlx::FromRow)]
struct GutschriftRow {
    id: i64,
    affiliate_twitch_login: Option<String>,
    period_year: i64,
    period_month: i64,
    gutschrift_number: Option<String>,
    net_amount_cents: i64,
    vat_amount_cents: i64,
    gross_amount_cents: i64,
    commission_ids: Option<String>,
    affiliate_ust_status: Option<String>,
    email_error: Option<String>,
    pdf_generated_at: Option<String>,
    email_sent_at: Option<String>,
    created_at: Option<String>,
    has_pdf: Option<i64>,
    display_name: Option<String>,
    is_active: i64,
    ust_status: Option<String>,
    has_pii: i64,
}

/// Status-Ableitung (Python `_status_from_row`).
fn gutschrift_status(r: &GutschriftRow) -> &'static str {
    if r.pdf_generated_at
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_none()
    {
        "blocked"
    } else if r
        .email_error
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        "email_failed"
    } else if r
        .email_sent_at
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        "emailed"
    } else {
        "generated"
    }
}

/// §19-UStG-Hinweis bei Kleinunternehmern (Python `_note_text`).
fn gutschrift_note_text(ust_status: Option<&str>) -> &'static str {
    if ust_status
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("kleinunternehmer")
    {
        "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet."
    } else {
        ""
    }
}

/// commission_ids-JSON-String → Liste von ints (Python `_commission_ids_from_row`).
fn parse_commission_ids(raw: Option<&str>) -> Vec<i64> {
    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Basis-Metadaten einer Gutschrift (Python `_row_to_metadata`, OHNE die
/// Konto-/PII-Join-Felder). Von der for-login-Liste direkt genutzt; die globale
/// Liste hängt die Join-Felder an.
fn gutschrift_base_value(r: &GutschriftRow) -> Value {
    let commission_ids = parse_commission_ids(r.commission_ids.as_deref());
    let period_label = if r.period_year >= 2000 && (1..=12).contains(&r.period_month) {
        format!(
            "{} {}",
            MONTH_LABELS[r.period_month as usize], r.period_year
        )
    } else {
        String::new()
    };
    let download_path = if r.id > 0 {
        Some(format!(
            "/twitch/api/admin/affiliates/gutschriften/{}/pdf",
            r.id
        ))
    } else {
        None
    };
    json!({
        "id": r.id,
        "period_year": r.period_year,
        "period_month": r.period_month,
        "period_label": period_label,
        "gutschrift_number": r.gutschrift_number.clone().unwrap_or_default(),
        "status": gutschrift_status(r),
        "net_amount_cents": r.net_amount_cents,
        "vat_amount_cents": r.vat_amount_cents,
        "gross_amount_cents": r.gross_amount_cents,
        "commission_count": commission_ids.len(),
        "commission_ids": commission_ids,
        "note_text": gutschrift_note_text(r.affiliate_ust_status.as_deref()),
        "last_error": r.email_error.clone().unwrap_or_default(),
        "generated_at": r.pdf_generated_at,
        "emailed_at": r.email_sent_at,
        "created_at": r.created_at,
        "download_path": download_path,
        "has_pdf": r.has_pdf.is_some(),
    })
}

/// Globale Liste: Basis-Metadaten + Konto-/PII-Join-Felder.
fn gutschrift_payload(r: GutschriftRow) -> Value {
    let mut value = gutschrift_base_value(&r);
    if let Some(obj) = value.as_object_mut() {
        let ust = r.ust_status.unwrap_or_default().trim().to_string();
        obj.insert(
            "affiliate_login".to_string(),
            json!(r.affiliate_twitch_login.unwrap_or_default().trim()),
        );
        obj.insert("display_name".to_string(), json!(r.display_name));
        obj.insert("active".to_string(), json!(r.is_active != 0));
        obj.insert(
            "ust_status".to_string(),
            json!(if ust.is_empty() {
                "unknown".to_string()
            } else {
                ust
            }),
        );
        obj.insert("has_pii".to_string(), json!(r.has_pii != 0));
    }
    value
}

/// Nur die `g.*`-Spalten (für GutschriftRow); Konto-/PII-Felder hängt der Aufrufer an.
const GUTSCHRIFT_G_COLUMNS: &str = "\
    g.id::bigint AS id, g.affiliate_twitch_login AS affiliate_twitch_login, \
    g.period_year::bigint AS period_year, g.period_month::bigint AS period_month, \
    g.gutschrift_number AS gutschrift_number, g.net_amount_cents::bigint AS net_amount_cents, \
    g.vat_amount_cents::bigint AS vat_amount_cents, g.gross_amount_cents::bigint AS gross_amount_cents, \
    g.commission_ids AS commission_ids, g.affiliate_ust_status AS affiliate_ust_status, \
    g.email_error AS email_error, g.pdf_generated_at AS pdf_generated_at, \
    g.email_sent_at AS email_sent_at, g.created_at AS created_at, \
    (CASE WHEN g.pdf_blob IS NOT NULL THEN 1 ELSE NULL END)::bigint AS has_pdf";

const GUTSCHRIFT_ACCOUNT_COLUMNS: &str =
    "a.display_name AS display_name, a.is_active::bigint AS is_active";

const GUTSCHRIFT_ORDER: &str = "ORDER BY g.period_year DESC, g.period_month DESC, g.id DESC";

/// Globale Gutschriften-Liste (Python `load_admin_affiliate_gutschriften`).
/// Zwei-stufiger Fallback: mit affiliate_pii → ohne PII → leer.
pub async fn load_affiliate_gutschriften(pool: &PgPool) -> Result<Value, sqlx::Error> {
    let full_sql = format!(
        "SELECT {GUTSCHRIFT_G_COLUMNS}, {GUTSCHRIFT_ACCOUNT_COLUMNS}, COALESCE(pii.ust_status, 'unknown') AS ust_status, \
                (CASE WHEN pii.twitch_login IS NOT NULL THEN 1 ELSE 0 END)::bigint AS has_pii \
         FROM affiliate_gutschriften g \
         JOIN affiliate_accounts a ON a.twitch_login = g.affiliate_twitch_login \
         LEFT JOIN affiliate_pii pii ON pii.twitch_login = g.affiliate_twitch_login {GUTSCHRIFT_ORDER}"
    );
    let rows = match sqlx::query_as::<_, GutschriftRow>(&full_sql)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) if !is_missing_schema_error(&e) => return Err(e),
        Err(_) => {
            let no_pii_sql = format!(
                "SELECT {GUTSCHRIFT_G_COLUMNS}, {GUTSCHRIFT_ACCOUNT_COLUMNS}, 'unknown' AS ust_status, 0::bigint AS has_pii \
                 FROM affiliate_gutschriften g \
                 JOIN affiliate_accounts a ON a.twitch_login = g.affiliate_twitch_login {GUTSCHRIFT_ORDER}"
            );
            match sqlx::query_as::<_, GutschriftRow>(&no_pii_sql)
                .fetch_all(pool)
                .await
            {
                Ok(rows) => rows,
                Err(e) if is_missing_schema_error(&e) => {
                    return Ok(json!({ "gutschriften": [], "count": 0 }))
                }
                Err(e) => return Err(e),
            }
        }
    };

    let documents: Vec<Value> = rows.into_iter().map(gutschrift_payload).collect();
    let count = documents.len();
    Ok(json!({ "gutschriften": documents, "count": count }))
}

/// Volle Gutschrift-Summary für einen Affiliate (Python
/// `_admin_affiliate_gutschriften_summary` mit affiliate_login). Fehlendes Schema
/// → Nullwerte.
async fn gutschrift_summary_for(pool: &PgPool, login: &str) -> Result<Value, sqlx::Error> {
    let res = sqlx::query!(
        r#"
        SELECT COUNT(*)::bigint AS "total!",
               COALESCE(SUM(gross_amount_cents), 0)::bigint AS "total_cents!",
               COALESCE(SUM(CASE WHEN pdf_generated_at IS NOT NULL AND email_sent_at IS NULL THEN 1 ELSE 0 END), 0)::bigint AS "pending_email!",
               MAX(pdf_generated_at) AS last_generated,
               MAX(email_sent_at) AS last_emailed
        FROM affiliate_gutschriften
        WHERE affiliate_twitch_login = $1
        "#,
        login
    )
    .fetch_one(pool)
    .await;
    match res {
        Ok(row) => Ok(json!({
            "total_gutschriften": row.total,
            "total_gutschrift_amount_cents": row.total_cents,
            "total_gutschrift_amount": cents_to_amount(row.total_cents),
            "pending_email_gutschriften": row.pending_email,
            "last_generated_at": row.last_generated,
            "last_emailed_at": row.last_emailed,
        })),
        Err(e) if is_missing_schema_error(&e) => Ok(json!({
            "total_gutschriften": 0,
            "total_gutschrift_amount_cents": 0,
            "total_gutschrift_amount": 0.0,
            "pending_email_gutschriften": 0,
            "last_generated_at": Value::Null,
            "last_emailed_at": Value::Null,
        })),
        Err(e) => Err(e),
    }
}

/// Fehler beim for-login-Gutschriften-Read.
#[derive(Debug)]
pub enum ForLoginError {
    NotFound,
    Db(sqlx::Error),
    Decrypt(String),
}

/// Gutschriften eines Affiliates inkl. Konto + PII-Readiness + Summary (Python
/// `load_admin_affiliate_gutschriften_for_login`). Kein Konto/Schema → NotFound.
pub async fn load_gutschriften_for_login(
    pool: &PgPool,
    cipher: &FieldCipher,
    login: &str,
) -> Result<Value, ForLoginError> {
    let account = match sqlx::query!(
        r#"
        SELECT twitch_login, display_name, is_active, created_at, updated_at
        FROM affiliate_accounts
        WHERE twitch_login = $1
        "#,
        login
    )
    .fetch_optional(pool)
    .await
    {
        Ok(v) => v,
        Err(e) if is_missing_schema_error(&e) => return Err(ForLoginError::NotFound),
        Err(e) => return Err(ForLoginError::Db(e)),
    };
    let Some(account) = account else {
        return Err(ForLoginError::NotFound);
    };

    let pii = load_admin_pii(pool, cipher, login)
        .await
        .map_err(|e| match e {
            PiiError::Db(e) => ForLoginError::Db(e),
            PiiError::Decrypt(s) => ForLoginError::Decrypt(s),
        })?;
    let readiness = build_readiness(&pii);
    let summary = gutschrift_summary_for(pool, login)
        .await
        .map_err(ForLoginError::Db)?;

    // Gutschriften des Affiliates (nur g.*; Join-Felder als Dummies → base_value ignoriert sie).
    let list_sql = format!(
        "SELECT {GUTSCHRIFT_G_COLUMNS}, NULL::text AS display_name, 0::bigint AS is_active, \
                NULL::text AS ust_status, 0::bigint AS has_pii \
         FROM affiliate_gutschriften g WHERE g.affiliate_twitch_login = $1 {GUTSCHRIFT_ORDER}"
    );
    let documents = match sqlx::query_as::<_, GutschriftRow>(&list_sql)
        .bind(login)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows.iter().map(gutschrift_base_value).collect::<Vec<_>>(),
        Err(e) if is_missing_schema_error(&e) => Vec::new(), // Python: list_for_affiliate missing-schema → []
        Err(e) => return Err(ForLoginError::Db(e)),
    };

    Ok(json!({
        "affiliate": {
            "login": account.twitch_login.trim(),
            "display_name": account.display_name,
            "active": account.is_active != 0,
            "created_at": account.created_at,
            "updated_at": account.updated_at,
        },
        "ust_status": if pii.ust_status.trim().is_empty() { "unknown".to_string() } else { pii.ust_status.clone() },
        "readiness": readiness,
        "gutschriften_summary": summary,
        "gutschriften": documents,
    }))
}

// ── Toggle (Write) ────────────────────────────────────────────────────────────

/// Fehler beim Affiliate-Toggle (Python: `AdminAffiliateNotFoundError` → 404).
#[derive(Debug)]
pub enum ToggleError {
    /// Kein Konto (oder fehlendes Schema) → 404.
    NotFound,
    Db(sqlx::Error),
}

/// Flippt `is_active` eines Affiliates (Python `toggle_admin_affiliate`).
/// Fehlende Zeile oder fehlendes Schema → [`ToggleError::NotFound`].
pub async fn toggle_affiliate(pool: &PgPool, login: &str) -> Result<Value, ToggleError> {
    let current = match sqlx::query_scalar!(
        "SELECT is_active FROM affiliate_accounts WHERE twitch_login = $1",
        login
    )
    .fetch_optional(pool)
    .await
    {
        Ok(v) => v,
        Err(e) if is_missing_schema_error(&e) => return Err(ToggleError::NotFound),
        Err(e) => return Err(ToggleError::Db(e)),
    };
    let Some(current) = current else {
        return Err(ToggleError::NotFound);
    };

    // Python: new_status = 0 if current else 1.
    let new_status: i32 = if current != 0 { 0 } else { 1 };
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false);
    if let Err(e) = sqlx::query!(
        "UPDATE affiliate_accounts SET is_active = $1, updated_at = $2 WHERE twitch_login = $3",
        new_status,
        &now,
        login
    )
    .execute(pool)
    .await
    {
        return Err(if is_missing_schema_error(&e) {
            ToggleError::NotFound
        } else {
            ToggleError::Db(e)
        });
    }

    Ok(json!({ "login": login, "active": new_status != 0 }))
}

/// Lädt das gespeicherte PDF einer Gutschrift (Python
/// `load_admin_affiliate_gutschrift_pdf` + `AffiliateGutschriftService.get_pdf`).
/// Gibt `(Dateiname-Basis, PDF-Bytes)` zurück, oder `None` wenn die Gutschrift
/// fehlt, kein PDF gespeichert ist oder das Schema fehlt. Reiner Read des
/// `pdf_blob`-BYTEA (kein Generieren). Der Login-Round-Trip aus Python ist
/// redundant (id ist PK) und wird zur Einzelabfrage zusammengezogen.
pub async fn load_gutschrift_pdf(
    pool: &PgPool,
    gutschrift_id: i64,
) -> Result<Option<(String, Vec<u8>)>, GutschriftError> {
    let row = match sqlx::query_as::<_, (Option<String>,)>(
        "SELECT affiliate_twitch_login FROM affiliate_gutschriften WHERE id::bigint = $1",
    )
    .bind(gutschrift_id)
    .fetch_optional(pool)
    .await
    {
        Ok(v) => v,
        Err(e) if is_missing_schema_error(&e) => return Ok(None),
        Err(e) => return Err(GutschriftError::Db(e)),
    };
    let Some(row) = row else {
        return Ok(None);
    };
    let login = row.0.unwrap_or_default();
    let Some(login) = tb_domain::login::normalize_twitch_login(&login) else {
        return Ok(None);
    };

    match affiliate_gutschrift::get_pdf(pool, &login, gutschrift_id).await {
        Ok(Some((metadata, bytes))) => {
            let number = metadata.gutschrift_number.trim();
            let filename_base = if number.is_empty() {
                format!("gutschrift-{gutschrift_id}")
            } else {
                number.to_string()
            };
            Ok(Some((filename_base, bytes)))
        }
        Ok(None) => Ok(None),
        Err(GutschriftError::Db(e)) if is_missing_schema_error(&e) => Ok(None),
        Err(GutschriftError::InvalidLogin) => Ok(None),
        Err(e) => Err(e),
    }
}

// ── Detail (Read mit PII-Readiness) ───────────────────────────────────────────

/// Fehler beim Affiliate-Detail-Laden.
#[derive(Debug)]
pub enum DetailError {
    /// Kein Konto (oder fehlendes Schema) → 404.
    NotFound,
    Db(sqlx::Error),
    Decrypt(String),
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Volles Affiliate-Detail (Python `load_admin_affiliate_detail`): Konto + Claims
/// (mit Provisions-Summen) + Statistik + PII-Readiness + Gutschrift-Summary.
pub async fn load_affiliate_detail(
    pool: &PgPool,
    cipher: &FieldCipher,
    login: &str,
) -> Result<Value, DetailError> {
    let account = match sqlx::query!(
        r#"
        SELECT twitch_login, display_name, is_active, created_at, email,
               stripe_connect_status, stripe_account_id, updated_at
        FROM affiliate_accounts
        WHERE twitch_login = $1
        "#,
        login
    )
    .fetch_optional(pool)
    .await
    {
        Ok(v) => v,
        Err(e) if is_missing_schema_error(&e) => return Err(DetailError::NotFound),
        Err(e) => return Err(DetailError::Db(e)),
    };
    let Some(account) = account else {
        return Err(DetailError::NotFound);
    };

    // Claims mit Provisions-Summe je Claim (nur Umsatz-Status).
    let claim_rows = sqlx::query!(
        r#"
        SELECT c.id::bigint AS "id!",
               c.claimed_streamer_login,
               c.claimed_at,
               COALESCE(SUM(co.commission_cents), 0)::bigint AS "commission_cents!",
               COUNT(co.id)::bigint AS "commission_count!"
        FROM affiliate_streamer_claims c
        LEFT JOIN affiliate_commissions co
          ON co.affiliate_twitch_login = c.affiliate_twitch_login
         AND co.streamer_login = c.claimed_streamer_login
         AND co.status IN ('pending', 'transferred')
        WHERE c.affiliate_twitch_login = $1
        GROUP BY c.id, c.claimed_streamer_login, c.claimed_at
        ORDER BY c.claimed_at DESC
        "#,
        login
    )
    .fetch_all(pool)
    .await
    .map_err(DetailError::Db)?;

    let total_claims: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM affiliate_streamer_claims WHERE affiliate_twitch_login = $1"#,
        login
    )
    .fetch_one(pool)
    .await
    .map_err(DetailError::Db)?;

    let provision = sqlx::query!(
        r#"
        SELECT COALESCE(SUM(commission_cents), 0)::bigint AS "total_provision_cents!",
               COUNT(DISTINCT streamer_login)::bigint AS "active_customers!"
        FROM affiliate_commissions
        WHERE affiliate_twitch_login = $1 AND status IN ('pending', 'transferred')
        "#,
        login
    )
    .fetch_one(pool)
    .await
    .map_err(DetailError::Db)?;

    let pii = load_admin_pii(pool, cipher, login)
        .await
        .map_err(|e| match e {
            PiiError::Db(e) => DetailError::Db(e),
            PiiError::Decrypt(s) => DetailError::Decrypt(s),
        })?;
    let readiness = build_readiness(&pii);

    let gutschriften = match sqlx::query!(
        r#"
        SELECT COUNT(*)::bigint AS "count!",
               COALESCE(SUM(gross_amount_cents), 0)::bigint AS "total_cents!"
        FROM affiliate_gutschriften
        WHERE affiliate_twitch_login = $1
        "#,
        login
    )
    .fetch_one(pool)
    .await
    {
        Ok(row) => (row.count, row.total_cents),
        Err(e) if is_missing_schema_error(&e) => (0, 0),
        Err(e) => return Err(DetailError::Db(e)),
    };

    // Stripe-Account-ID maskieren (>12 Zeichen → erste 8 … letzte 4).
    let stripe_id = account.stripe_account_id.unwrap_or_default();
    let masked = if stripe_id.chars().count() > 12 {
        let chars: Vec<char> = stripe_id.chars().collect();
        let first: String = chars[..8].iter().collect();
        let last: String = chars[chars.len() - 4..].iter().collect();
        format!("{first}...{last}")
    } else {
        stripe_id
    };

    let claims: Vec<Value> = claim_rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.id,
                "customer_login": row.claimed_streamer_login.trim(),
                "claimed_at": row.claimed_at,
                "commission_cents": row.commission_cents,
                "commission_count": row.commission_count,
            })
        })
        .collect();

    let avg_provision = if total_claims > 0 {
        round2((provision.total_provision_cents as f64 / total_claims.max(1) as f64) / 100.0)
    } else {
        0.0
    };

    Ok(json!({
        "affiliate": {
            "login": account.twitch_login.trim(),
            "display_name": account.display_name,
            "active": account.is_active != 0,
            "created_at": account.created_at,
            "email": account.email,
            "stripe_connect_status": account.stripe_connect_status,
            "stripe_account_id": if masked.is_empty() { Value::Null } else { json!(masked) },
            "updated_at": account.updated_at,
        },
        "claims": claims,
        "stats": {
            "total_claims": total_claims,
            "total_provision": cents_to_amount(provision.total_provision_cents),
            "avg_provision": avg_provision,
            "active_customers": provision.active_customers,
        },
        "ust_status": pii.ust_status,
        "pii_readiness": readiness,
        "gutschriften_summary": { "count": gutschriften.0, "total_gross_cents": gutschriften.1 },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

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
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    async fn with_tables(pool: &PgPool) {
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT)",
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, ust_status TEXT NOT NULL DEFAULT 'unknown')",
            "CREATE TABLE affiliate_streamer_claims (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, commission_cents INTEGER, status TEXT, created_at TEXT)",
            "CREATE TABLE affiliate_gutschriften (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, gross_amount_cents INTEGER, pdf_generated_at TEXT, email_sent_at TEXT)",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn list_aggregiert_und_pii() {
        let Some(pool) = connect("t_aff_list").await else {
            return;
        };
        with_tables(&pool).await;
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, display_name, is_active, created_at) VALUES ('nani','Nani',1,'2026-06-01T00:00:00+00:00'), ('foo',NULL,0,'2026-05-01T00:00:00+00:00')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_pii (twitch_login, ust_status) VALUES ('nani','kleinunternehmer')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_at) VALUES ('nani','2026-06-10T00:00:00+00:00'), ('nani','2026-06-12T00:00:00+00:00')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_commissions (affiliate_twitch_login, commission_cents, status, created_at) VALUES ('nani', 2000, 'pending', '2026-06-05T00:00:00+00:00'), ('nani', 999, 'refunded', '2026-06-06T00:00:00+00:00')").execute(&pool).await.unwrap();

        let v = load_affiliates_list(&pool).await.unwrap();
        let items = v["affiliates"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        // ORDER BY created_at DESC → nani (Juni) zuerst.
        let nani = &items[0];
        assert_eq!(nani["login"], "nani");
        assert_eq!(nani["display_name"], "Nani");
        assert_eq!(nani["active"], true);
        assert_eq!(nani["total_claims"], 2);
        assert_eq!(nani["total_provision"], 20.0); // refunded ausgeschlossen
        assert_eq!(nani["last_claim_at"], "2026-06-12T00:00:00+00:00");
        assert_eq!(nani["ust_status"], "kleinunternehmer");
        assert_eq!(nani["has_pii"], true);
        // foo: kein PII, inaktiv, keine Claims/Provision.
        let foo = &items[1];
        assert_eq!(foo["active"], false);
        assert!(foo["display_name"].is_null());
        assert_eq!(foo["total_claims"], 0);
        assert_eq!(foo["total_provision"], 0.0);
        assert_eq!(foo["ust_status"], "unknown");
        assert_eq!(foo["has_pii"], false);
    }

    #[tokio::test]
    async fn list_ohne_pii_tabelle_fallback() {
        let Some(pool) = connect("t_aff_list_nopii").await else {
            return;
        };
        // Nur die Kern-Tabellen, KEIN affiliate_pii → Fallback-Query.
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT)",
            "CREATE TABLE affiliate_streamer_claims (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, commission_cents INTEGER, status TEXT, created_at TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, is_active, created_at) VALUES ('nani',1,'2026-06-01T00:00:00+00:00')").execute(&pool).await.unwrap();
        let v = load_affiliates_list(&pool).await.unwrap();
        let items = v["affiliates"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["ust_status"], "unknown"); // Fallback ohne PII
        assert_eq!(items[0]["has_pii"], false);
    }

    #[tokio::test]
    async fn list_ohne_tabellen_leer() {
        let Some(pool) = connect("t_aff_list_empty").await else {
            return;
        };
        let v = load_affiliates_list(&pool).await.unwrap();
        assert_eq!(v["affiliates"], json!([]));
    }

    #[test]
    fn pure_helfer() {
        assert_eq!(parse_commission_ids(Some("[1, 2, \"3\"]")), vec![1, 2, 3]);
        assert_eq!(parse_commission_ids(Some("kein json")), Vec::<i64>::new());
        assert_eq!(parse_commission_ids(None), Vec::<i64>::new());
        assert_eq!(
            gutschrift_note_text(Some("Kleinunternehmer")),
            "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet."
        );
        assert_eq!(gutschrift_note_text(Some("regular")), "");
    }

    async fn with_gutschrift_tables(pool: &PgPool) {
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT)",
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, ust_status TEXT NOT NULL DEFAULT 'unknown')",
            "CREATE TABLE affiliate_gutschriften (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, gutschrift_number TEXT, affiliate_twitch_login TEXT, period_year INTEGER, period_month INTEGER, net_amount_cents INTEGER, vat_amount_cents INTEGER, gross_amount_cents INTEGER, affiliate_ust_status TEXT, email_error TEXT, pdf_blob BYTEA, pdf_generated_at TEXT, email_sent_at TEXT, commission_ids TEXT, created_at TEXT)",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn gutschriften_liste_payload() {
        let Some(pool) = connect("t_aff_gut").await else {
            return;
        };
        with_gutschrift_tables(&pool).await;
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, display_name, is_active, created_at) VALUES ('nani','Nani',1,'2026-06-01T00:00:00+00:00')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_pii (twitch_login, ust_status) VALUES ('nani','kleinunternehmer')").execute(&pool).await.unwrap();
        // PDF generiert + Email versendet → status 'emailed'; pdf_blob gesetzt → has_pdf.
        sqlx::query("INSERT INTO affiliate_gutschriften (gutschrift_number, affiliate_twitch_login, period_year, period_month, net_amount_cents, vat_amount_cents, gross_amount_cents, affiliate_ust_status, pdf_blob, pdf_generated_at, email_sent_at, commission_ids, created_at) VALUES ('GS-2026-06-001','nani',2026,6,1000,0,1000,'kleinunternehmer',E'\\\\x00','2026-06-30T00:00:00+00:00','2026-07-01T00:00:00+00:00','[10, 11]','2026-06-30T00:00:00+00:00')")
            .execute(&pool).await.unwrap();

        let v = load_affiliate_gutschriften(&pool).await.unwrap();
        assert_eq!(v["count"], 1);
        let g = &v["gutschriften"][0];
        assert_eq!(g["gutschrift_number"], "GS-2026-06-001");
        assert_eq!(g["period_label"], "Juni 2026");
        assert_eq!(g["status"], "emailed");
        assert_eq!(g["has_pdf"], true);
        assert_eq!(g["commission_count"], 2);
        assert_eq!(g["commission_ids"], json!([10, 11]));
        assert!(g["note_text"].as_str().unwrap().contains("§ 19"));
        assert_eq!(
            g["download_path"],
            "/twitch/api/admin/affiliates/gutschriften/1/pdf"
        );
        assert_eq!(g["affiliate_login"], "nani");
        assert_eq!(g["display_name"], "Nani");
        assert_eq!(g["ust_status"], "kleinunternehmer");
        assert_eq!(g["has_pii"], true);
    }

    #[tokio::test]
    async fn gutschriften_ohne_tabellen_leer() {
        let Some(pool) = connect("t_aff_gut_empty").await else {
            return;
        };
        let v = load_affiliate_gutschriften(&pool).await.unwrap();
        assert_eq!(v["count"], 0);
        assert_eq!(v["gutschriften"], json!([]));
    }

    #[tokio::test]
    async fn toggle_flippt_und_not_found() {
        let Some(pool) = connect("t_aff_toggle").await else {
            return;
        };
        sqlx::query("CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, is_active INTEGER NOT NULL DEFAULT 1, updated_at TEXT)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, is_active) VALUES ('nani', 1)")
            .execute(&pool)
            .await
            .unwrap();

        // 1 → 0.
        let v = toggle_affiliate(&pool, "nani").await.unwrap();
        assert_eq!(v["login"], "nani");
        assert_eq!(v["active"], false);
        let stored: i32 = sqlx::query_scalar(
            "SELECT is_active FROM affiliate_accounts WHERE twitch_login='nani'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, 0);
        let ts: Option<String> = sqlx::query_scalar(
            "SELECT updated_at FROM affiliate_accounts WHERE twitch_login='nani'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(ts.is_some(), "updated_at gesetzt");

        // 0 → 1.
        let v = toggle_affiliate(&pool, "nani").await.unwrap();
        assert_eq!(v["active"], true);

        // unbekannter Login → NotFound.
        assert!(matches!(
            toggle_affiliate(&pool, "ghost").await,
            Err(ToggleError::NotFound)
        ));
    }

    #[tokio::test]
    async fn toggle_fehlendes_schema_not_found() {
        let Some(pool) = connect("t_aff_toggle_empty").await else {
            return;
        };
        assert!(matches!(
            toggle_affiliate(&pool, "nani").await,
            Err(ToggleError::NotFound)
        ));
    }

    #[tokio::test]
    async fn for_login_gutschriften_und_not_found() {
        let Some(pool) = connect("t_aff_forlogin").await else {
            return;
        };
        let cipher = FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap();
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT, updated_at TEXT)",
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, full_name_enc BYTEA, email_enc BYTEA, address_line1_enc BYTEA, address_city_enc BYTEA, address_zip_enc BYTEA, tax_id_enc BYTEA, address_country TEXT NOT NULL DEFAULT 'DE', ust_status TEXT NOT NULL DEFAULT 'unknown', updated_at TEXT NOT NULL DEFAULT '2026-06-30T00:00:00+00:00')",
            "CREATE TABLE affiliate_gutschriften (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, gutschrift_number TEXT, affiliate_twitch_login TEXT, period_year INTEGER, period_month INTEGER, net_amount_cents INTEGER, vat_amount_cents INTEGER, gross_amount_cents INTEGER, affiliate_ust_status TEXT, email_error TEXT, pdf_blob BYTEA, pdf_generated_at TEXT, email_sent_at TEXT, commission_ids TEXT, created_at TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, display_name, is_active, created_at, updated_at) VALUES ('nani','Nani',1,'2026-06-01T00:00:00+00:00','2026-06-02T00:00:00+00:00')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_pii (twitch_login, ust_status) VALUES ('nani','kleinunternehmer')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_gutschriften (gutschrift_number, affiliate_twitch_login, period_year, period_month, net_amount_cents, vat_amount_cents, gross_amount_cents, pdf_blob, pdf_generated_at, created_at) VALUES ('GS-2026-06-001','nani',2026,6,1000,0,1000,E'\\\\x25','2026-06-30T00:00:00+00:00','2026-06-30T00:00:00+00:00')").execute(&pool).await.unwrap();

        let v = load_gutschriften_for_login(&pool, &cipher, "nani")
            .await
            .unwrap();
        assert_eq!(v["affiliate"]["login"], "nani");
        assert_eq!(v["affiliate"]["display_name"], "Nani");
        assert_eq!(v["ust_status"], "kleinunternehmer");
        assert!(v["readiness"]["blockers"].is_array()); // unvollständig (kein full_name) → blockers
        assert_eq!(v["gutschriften_summary"]["total_gutschriften"], 1);
        assert_eq!(
            v["gutschriften_summary"]["last_generated_at"],
            "2026-06-30T00:00:00+00:00"
        );
        let docs = v["gutschriften"].as_array().unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0]["gutschrift_number"], "GS-2026-06-001");
        assert_eq!(docs[0]["period_label"], "Juni 2026");
        assert_eq!(docs[0]["has_pdf"], true);
        // for-login-Items haben KEINE Join-Felder (affiliate_login/has_pii).
        assert!(docs[0].get("affiliate_login").is_none());

        assert!(matches!(
            load_gutschriften_for_login(&pool, &cipher, "ghost").await,
            Err(ForLoginError::NotFound)
        ));
    }

    #[tokio::test]
    async fn pdf_download_und_none_faelle() {
        let Some(pool) = connect("t_aff_pdf").await else {
            return;
        };
        // fehlendes Schema → None.
        assert!(load_gutschrift_pdf(&pool, 1).await.unwrap().is_none());

        sqlx::query("CREATE TABLE affiliate_gutschriften (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, period_year INTEGER, period_month INTEGER, gutschrift_number TEXT NOT NULL, net_amount_cents INTEGER, vat_amount_cents INTEGER, gross_amount_cents INTEGER, affiliate_ust_status TEXT, pdf_blob BYTEA, pdf_generated_at TEXT, email_sent_at TEXT, email_error TEXT, commission_ids TEXT, created_at TEXT)").execute(&pool).await.unwrap();
        // mit PDF → (filename, bytes).
        sqlx::query("INSERT INTO affiliate_gutschriften (id, affiliate_twitch_login, period_year, period_month, gutschrift_number, net_amount_cents, vat_amount_cents, gross_amount_cents, affiliate_ust_status, pdf_blob) VALUES (5, 'nani', 2026, 6, 'GS-2026-06-001', 1000, 0, 1000, 'kleinunternehmer', E'\\\\x255044462d')").execute(&pool).await.unwrap();
        // ohne PDF → None.
        sqlx::query("INSERT INTO affiliate_gutschriften (id, affiliate_twitch_login, period_year, period_month, gutschrift_number, net_amount_cents, vat_amount_cents, gross_amount_cents, affiliate_ust_status, pdf_blob) VALUES (6, 'nani', 2026, 6, 'GS-2026-06-002', 1000, 0, 1000, 'kleinunternehmer', NULL)").execute(&pool).await.unwrap();
        // ohne Nummer → Fallback-Dateiname.
        sqlx::query("INSERT INTO affiliate_gutschriften (id, affiliate_twitch_login, period_year, period_month, gutschrift_number, net_amount_cents, vat_amount_cents, gross_amount_cents, affiliate_ust_status, pdf_blob) VALUES (7, 'nani', 2026, 6, '', 1000, 0, 1000, 'kleinunternehmer', E'\\\\x255044')").execute(&pool).await.unwrap();

        let (name, bytes) = load_gutschrift_pdf(&pool, 5).await.unwrap().unwrap();
        assert_eq!(name, "GS-2026-06-001");
        assert_eq!(bytes, vec![0x25, 0x50, 0x44, 0x46, 0x2d]); // %PDF-
        assert!(load_gutschrift_pdf(&pool, 6).await.unwrap().is_none()); // NULL-Blob
        assert!(load_gutschrift_pdf(&pool, 99).await.unwrap().is_none()); // fehlende Zeile
        let (name7, _) = load_gutschrift_pdf(&pool, 7).await.unwrap().unwrap();
        assert_eq!(name7, "gutschrift-7"); // Fallback
    }

    #[tokio::test]
    async fn for_login_ohne_pii_und_gutschrift_schema_nutzt_defaults() {
        let Some(pool) = connect("t_aff_forlogin_defaults").await else {
            return;
        };
        let cipher = FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap();
        sqlx::query("CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT, updated_at TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, display_name, is_active, created_at, updated_at) VALUES ('nani','Nani',1,'2026-06-01T00:00:00+00:00','2026-06-02T00:00:00+00:00')")
            .execute(&pool)
            .await
            .unwrap();

        let v = load_gutschriften_for_login(&pool, &cipher, "nani")
            .await
            .unwrap();
        assert_eq!(v["affiliate"]["login"], "nani");
        assert_eq!(v["ust_status"], "unknown");
        assert_eq!(v["readiness"]["can_generate"], false);
        assert_eq!(v["gutschriften_summary"]["total_gutschriften"], 0);
        assert_eq!(v["gutschriften"], json!([]));
    }

    #[tokio::test]
    async fn detail_vollstaendig_und_not_found() {
        let Some(pool) = connect("t_aff_detail").await else {
            return;
        };
        let cipher = FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap();
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT, email TEXT, stripe_connect_status TEXT, stripe_account_id TEXT, updated_at TEXT)",
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, full_name_enc BYTEA, email_enc BYTEA, address_line1_enc BYTEA, address_city_enc BYTEA, address_zip_enc BYTEA, tax_id_enc BYTEA, address_country TEXT NOT NULL DEFAULT 'DE', ust_status TEXT NOT NULL DEFAULT 'unknown', updated_at TEXT NOT NULL DEFAULT '2026-06-30T00:00:00+00:00')",
            "CREATE TABLE affiliate_streamer_claims (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, claimed_streamer_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, streamer_login TEXT, commission_cents INTEGER, status TEXT)",
            "CREATE TABLE affiliate_gutschriften (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, gross_amount_cents INTEGER)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, display_name, is_active, created_at, email, stripe_connect_status, stripe_account_id, updated_at) VALUES ('nani','Nani',1,'2026-06-01T00:00:00+00:00','a@b.de','active','acct_1234567890XY','2026-06-02T00:00:00+00:00')").execute(&pool).await.unwrap();
        // PII vollständig → can_generate.
        let enc = |field: &str, v: &str| {
            cipher
                .encrypt_field(v, &format!("affiliate_pii|{field}|nani"))
                .unwrap()
        };
        sqlx::query("INSERT INTO affiliate_pii (twitch_login, full_name_enc, email_enc, address_line1_enc, address_city_enc, address_zip_enc, tax_id_enc, address_country, ust_status) VALUES ('nani',$1,$2,$3,$4,$5,$6,'DE','kleinunternehmer')")
            .bind(enc("full_name","Nani M")).bind(enc("email","a@b.de")).bind(enc("address_line1","Str 1"))
            .bind(enc("address_city","Ort")).bind(enc("address_zip","12345")).bind(enc("tax_id","DE123"))
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_streamer_login, claimed_at) VALUES ('nani','streamerx','2026-06-01T00:00:00+00:00')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_commissions (affiliate_twitch_login, streamer_login, commission_cents, status) VALUES ('nani','streamerx',2000,'pending')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_gutschriften (affiliate_twitch_login, gross_amount_cents) VALUES ('nani',1500)").execute(&pool).await.unwrap();

        let v = load_affiliate_detail(&pool, &cipher, "nani").await.unwrap();
        assert_eq!(v["affiliate"]["login"], "nani");
        assert_eq!(v["affiliate"]["email"], "a@b.de"); // Plaintext aus accounts
        assert_eq!(v["affiliate"]["stripe_account_id"], "acct_123...90XY"); // maskiert (>12)
        assert_eq!(v["claims"].as_array().unwrap().len(), 1);
        assert_eq!(v["claims"][0]["commission_cents"], 2000);
        assert_eq!(v["stats"]["total_claims"], 1);
        assert_eq!(v["stats"]["total_provision"], 20.0);
        assert_eq!(v["stats"]["avg_provision"], 20.0);
        assert_eq!(v["stats"]["active_customers"], 1);
        assert_eq!(v["ust_status"], "kleinunternehmer");
        assert_eq!(v["pii_readiness"]["can_generate"], true);
        assert_eq!(v["gutschriften_summary"]["count"], 1);
        assert_eq!(v["gutschriften_summary"]["total_gross_cents"], 1500);

        // unbekannt → NotFound.
        assert!(matches!(
            load_affiliate_detail(&pool, &cipher, "ghost").await,
            Err(DetailError::NotFound)
        ));
    }

    #[tokio::test]
    async fn detail_ohne_pii_und_gutschrift_schema_nutzt_defaults() {
        let Some(pool) = connect("t_aff_detail_defaults").await else {
            return;
        };
        let cipher = FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap();
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT, email TEXT, stripe_connect_status TEXT, stripe_account_id TEXT, updated_at TEXT)",
            "CREATE TABLE affiliate_streamer_claims (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, claimed_streamer_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, streamer_login TEXT, commission_cents INTEGER, status TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, display_name, is_active, created_at, email, stripe_connect_status, stripe_account_id, updated_at) VALUES ('nani','Nani',1,'2026-06-01T00:00:00+00:00','a@b.de','active','acct_123', '2026-06-02T00:00:00+00:00')")
            .execute(&pool)
            .await
            .unwrap();

        let v = load_affiliate_detail(&pool, &cipher, "nani").await.unwrap();
        assert_eq!(v["affiliate"]["login"], "nani");
        assert_eq!(v["ust_status"], "unknown");
        assert_eq!(v["pii_readiness"]["can_generate"], false);
        assert_eq!(v["gutschriften_summary"]["count"], 0);
        assert_eq!(v["gutschriften_summary"]["total_gross_cents"], 0);
    }

    #[tokio::test]
    async fn fehlendes_schema_gibt_nullwerte() {
        let Some(pool) = connect("t_aff_empty").await else {
            return;
        };
        // Keine Tabellen → missing-schema → Nullwerte.
        let v = load_affiliate_stats(&pool, "2026-06-01T00:00:00+00:00")
            .await
            .unwrap();
        assert_eq!(v["total_affiliates"], 0);
        assert_eq!(v["total_provision"], 0.0);
        assert_eq!(v["pending_email_gutschriften"], 0);
    }

    #[tokio::test]
    async fn stats_aggregiert_korrekt() {
        let Some(pool) = connect("t_aff_stats").await else {
            return;
        };
        with_tables(&pool).await;
        let month = "2026-06-01T00:00:00+00:00";

        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, is_active) VALUES ('a', 1), ('b', 1), ('c', 0)").execute(&pool).await.unwrap();
        // 2 Claims, eins diesen Monat.
        sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_at) VALUES ('a', '2026-06-10T00:00:00+00:00'), ('a', '2026-05-01T00:00:00+00:00')").execute(&pool).await.unwrap();
        // Provisionen: 1000 (pending, diesen Monat) + 500 (transferred, alt) + 999 (refunded → zählt NICHT).
        sqlx::query("INSERT INTO affiliate_commissions (affiliate_twitch_login, commission_cents, status, created_at) VALUES ('a', 1000, 'pending', '2026-06-05T00:00:00+00:00'), ('a', 500, 'transferred', '2026-05-01T00:00:00+00:00'), ('a', 999, 'refunded', '2026-06-05T00:00:00+00:00')").execute(&pool).await.unwrap();
        // Gutschriften: 1 mit PDF aber ohne Email (pending_email).
        sqlx::query("INSERT INTO affiliate_gutschriften (affiliate_twitch_login, gross_amount_cents, pdf_generated_at, email_sent_at) VALUES ('a', 1500, '2026-06-01T00:00:00+00:00', NULL)").execute(&pool).await.unwrap();

        let v = load_affiliate_stats(&pool, month).await.unwrap();
        assert_eq!(v["total_affiliates"], 3);
        assert_eq!(v["active_affiliates"], 2);
        assert_eq!(v["total_claims"], 2);
        assert_eq!(v["this_month_claims"], 1);
        // total_provision = (1000 + 500) / 100 = 15.0 (refunded ausgeschlossen).
        assert_eq!(v["total_provision"], 15.0);
        // this_month_provision = 1000 / 100 = 10.0.
        assert_eq!(v["this_month_provision"], 10.0);
        assert_eq!(v["total_gutschriften"], 1);
        assert_eq!(v["total_gutschrift_amount"], 15.0);
        assert_eq!(v["pending_email_gutschriften"], 1);
    }
}

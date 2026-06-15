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

use crate::affiliate_pii::{build_readiness, load_affiliate_pii, PiiError};

/// Provisions-Status, die als Umsatz zählen (Python `_AFFILIATE_REVENUE_STATUSES`).
const REVENUE_STATUSES: &str = "'pending', 'transferred'";

/// `true` wenn der DB-Fehler auf fehlendes Schema deutet (→ Nullwerte statt 500).
fn is_missing_schema_error(e: &sqlx::Error) -> bool {
    let s = e.to_string().to_lowercase();
    ["does not exist", "no such table", "undefined table", "no such column", "undefined column"]
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

/// Aggregierte Affiliate-Programm-Statistik (Python `load_admin_affiliate_stats`).
/// Fehlendes Schema → Nullwerte (1:1 Python-`except`), sonst durchgereichter Fehler.
pub async fn load_affiliate_stats(pool: &PgPool, month_start_iso: &str) -> Result<Value, sqlx::Error> {
    match stats_inner(pool, month_start_iso).await {
        Ok(v) => Ok(v),
        Err(e) if is_missing_schema_error(&e) => Ok(zero_stats()),
        Err(e) => Err(e),
    }
}

async fn stats_inner(pool: &PgPool, month_start_iso: &str) -> Result<Value, sqlx::Error> {
    let (total_affiliates, active_affiliates): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN is_active = 1 THEN 1 ELSE 0 END), 0) FROM affiliate_accounts",
    )
    .fetch_one(pool)
    .await?;

    let (total_claims, this_month_claims): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN claimed_at >= $1 THEN 1 ELSE 0 END), 0) \
         FROM affiliate_streamer_claims",
    )
    .bind(month_start_iso)
    .fetch_one(pool)
    .await?;

    // Nur Umsatz-Status (pending/transferred) zählen — Konstanten, kein User-Input.
    let commission_sql = format!(
        "SELECT COALESCE(SUM(commission_cents), 0), \
                COALESCE(SUM(CASE WHEN created_at >= $1 AND status IN ({REVENUE_STATUSES}) THEN commission_cents ELSE 0 END), 0) \
         FROM affiliate_commissions WHERE status IN ({REVENUE_STATUSES})"
    );
    let (total_provision_cents, this_month_provision_cents): (i64, i64) =
        sqlx::query_as(&commission_sql).bind(month_start_iso).fetch_one(pool).await?;

    // Gutschrift-Summary (Python `_admin_affiliate_gutschriften_summary` — hier nur
    // die drei von stats genutzten Felder).
    let (total_gutschriften, total_gutschrift_cents, pending_email): (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(gross_amount_cents), 0), \
                COALESCE(SUM(CASE WHEN pdf_generated_at IS NOT NULL AND email_sent_at IS NULL THEN 1 ELSE 0 END), 0) \
         FROM affiliate_gutschriften",
    )
    .fetch_one(pool)
    .await?;

    Ok(json!({
        "total_affiliates": total_affiliates,
        "active_affiliates": active_affiliates,
        "total_claims": total_claims,
        "total_provision": cents_to_amount(total_provision_cents),
        "this_month_claims": this_month_claims,
        "this_month_provision": cents_to_amount(this_month_provision_cents),
        "total_gutschriften": total_gutschriften,
        "total_gutschrift_amount": cents_to_amount(total_gutschrift_cents),
        "pending_email_gutschriften": pending_email,
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
        .map(|(login, display_name, is_active, created_at, total_claims, total_provision_cents, last_claim_at, ust_status, has_pii)| {
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
        })
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
    match sqlx::query_as::<_, ListRow>(&full_sql).fetch_all(pool).await {
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
    match sqlx::query_as::<_, ListRow>(&no_pii_sql).fetch_all(pool).await {
        Ok(rows) => Ok(map_list_rows(rows)),
        Err(e) if is_missing_schema_error(&e) => Ok(json!({ "affiliates": [] })),
        Err(e) => Err(e),
    }
}

// ── Gutschriften-Liste ────────────────────────────────────────────────────────

/// Deutsche Monatsnamen (Index 1–12; 0 = leer). ASCII „Maerz" wie Python.
const MONTH_LABELS: [&str; 13] = [
    "", "Januar", "Februar", "Maerz", "April", "Mai", "Juni", "Juli", "August", "September",
    "Oktober", "November", "Dezember",
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
    if r.pdf_generated_at.as_deref().filter(|s| !s.is_empty()).is_none() {
        "blocked"
    } else if r.email_error.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
        "email_failed"
    } else if r.email_sent_at.as_deref().filter(|s| !s.is_empty()).is_some() {
        "emailed"
    } else {
        "generated"
    }
}

/// §19-UStG-Hinweis bei Kleinunternehmern (Python `_note_text`).
fn gutschrift_note_text(ust_status: Option<&str>) -> &'static str {
    if ust_status.unwrap_or("").trim().eq_ignore_ascii_case("kleinunternehmer") {
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
            .filter_map(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
            .collect(),
        _ => Vec::new(),
    }
}

fn gutschrift_payload(r: GutschriftRow) -> Value {
    let commission_ids = parse_commission_ids(r.commission_ids.as_deref());
    let period_label = if r.period_year >= 2000 && (1..=12).contains(&r.period_month) {
        format!("{} {}", MONTH_LABELS[r.period_month as usize], r.period_year)
    } else {
        String::new()
    };
    let status = gutschrift_status(&r);
    let note_text = gutschrift_note_text(r.affiliate_ust_status.as_deref());
    let download_path = if r.id > 0 {
        Some(format!("/twitch/api/admin/affiliates/gutschriften/{}/pdf", r.id))
    } else {
        None
    };
    let ust = r.ust_status.unwrap_or_default().trim().to_string();
    json!({
        "id": r.id,
        "period_year": r.period_year,
        "period_month": r.period_month,
        "period_label": period_label,
        "gutschrift_number": r.gutschrift_number.unwrap_or_default(),
        "status": status,
        "net_amount_cents": r.net_amount_cents,
        "vat_amount_cents": r.vat_amount_cents,
        "gross_amount_cents": r.gross_amount_cents,
        "commission_count": commission_ids.len(),
        "commission_ids": commission_ids,
        "note_text": note_text,
        "last_error": r.email_error.unwrap_or_default(),
        "generated_at": r.pdf_generated_at,
        "emailed_at": r.email_sent_at,
        "created_at": r.created_at,
        "download_path": download_path,
        "has_pdf": r.has_pdf.is_some(),
        "affiliate_login": r.affiliate_twitch_login.unwrap_or_default().trim(),
        "display_name": r.display_name,
        "active": r.is_active != 0,
        "ust_status": if ust.is_empty() { "unknown".to_string() } else { ust },
        "has_pii": r.has_pii != 0,
    })
}

const GUTSCHRIFT_COLUMNS: &str = "\
    g.id::bigint AS id, g.affiliate_twitch_login AS affiliate_twitch_login, \
    g.period_year::bigint AS period_year, g.period_month::bigint AS period_month, \
    g.gutschrift_number AS gutschrift_number, g.net_amount_cents::bigint AS net_amount_cents, \
    g.vat_amount_cents::bigint AS vat_amount_cents, g.gross_amount_cents::bigint AS gross_amount_cents, \
    g.commission_ids AS commission_ids, g.affiliate_ust_status AS affiliate_ust_status, \
    g.email_error AS email_error, g.pdf_generated_at AS pdf_generated_at, \
    g.email_sent_at AS email_sent_at, g.created_at AS created_at, \
    (CASE WHEN g.pdf_blob IS NOT NULL THEN 1 ELSE NULL END)::bigint AS has_pdf, \
    a.display_name AS display_name, a.is_active::bigint AS is_active";

const GUTSCHRIFT_ORDER: &str = "ORDER BY g.period_year DESC, g.period_month DESC, g.id DESC";

/// Globale Gutschriften-Liste (Python `load_admin_affiliate_gutschriften`).
/// Zwei-stufiger Fallback: mit affiliate_pii → ohne PII → leer.
pub async fn load_affiliate_gutschriften(pool: &PgPool) -> Result<Value, sqlx::Error> {
    let full_sql = format!(
        "SELECT {GUTSCHRIFT_COLUMNS}, COALESCE(pii.ust_status, 'unknown') AS ust_status, \
                (CASE WHEN pii.twitch_login IS NOT NULL THEN 1 ELSE 0 END)::bigint AS has_pii \
         FROM affiliate_gutschriften g \
         JOIN affiliate_accounts a ON a.twitch_login = g.affiliate_twitch_login \
         LEFT JOIN affiliate_pii pii ON pii.twitch_login = g.affiliate_twitch_login {GUTSCHRIFT_ORDER}"
    );
    let rows = match sqlx::query_as::<_, GutschriftRow>(&full_sql).fetch_all(pool).await {
        Ok(rows) => rows,
        Err(e) if !is_missing_schema_error(&e) => return Err(e),
        Err(_) => {
            let no_pii_sql = format!(
                "SELECT {GUTSCHRIFT_COLUMNS}, 'unknown' AS ust_status, 0::bigint AS has_pii \
                 FROM affiliate_gutschriften g \
                 JOIN affiliate_accounts a ON a.twitch_login = g.affiliate_twitch_login {GUTSCHRIFT_ORDER}"
            );
            match sqlx::query_as::<_, GutschriftRow>(&no_pii_sql).fetch_all(pool).await {
                Ok(rows) => rows,
                Err(e) if is_missing_schema_error(&e) => return Ok(json!({ "gutschriften": [], "count": 0 })),
                Err(e) => return Err(e),
            }
        }
    };

    let documents: Vec<Value> = rows.into_iter().map(gutschrift_payload).collect();
    let count = documents.len();
    Ok(json!({ "gutschriften": documents, "count": count }))
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
    let current: Option<i32> = match sqlx::query_scalar::<_, i32>(
        "SELECT is_active FROM affiliate_accounts WHERE twitch_login = $1",
    )
    .bind(login)
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
    if let Err(e) = sqlx::query("UPDATE affiliate_accounts SET is_active = $1, updated_at = $2 WHERE twitch_login = $3")
        .bind(new_status)
        .bind(&now)
        .bind(login)
        .execute(pool)
        .await
    {
        return Err(if is_missing_schema_error(&e) { ToggleError::NotFound } else { ToggleError::Db(e) });
    }

    Ok(json!({ "login": login, "active": new_status != 0 }))
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
    type Account = (
        Option<String>, // twitch_login
        Option<String>, // display_name
        i32,            // is_active
        Option<String>, // created_at
        Option<String>, // email
        Option<String>, // stripe_connect_status
        Option<String>, // stripe_account_id
        Option<String>, // updated_at
    );
    let account: Option<Account> = match sqlx::query_as(
        "SELECT twitch_login, display_name, is_active, created_at, email, \
                stripe_connect_status, stripe_account_id, updated_at \
         FROM affiliate_accounts WHERE twitch_login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
    {
        Ok(v) => v,
        Err(e) if is_missing_schema_error(&e) => return Err(DetailError::NotFound),
        Err(e) => return Err(DetailError::Db(e)),
    };
    let Some((twitch_login, display_name, is_active, created_at, email, stripe_status, stripe_account_id, updated_at)) =
        account
    else {
        return Err(DetailError::NotFound);
    };

    // Claims mit Provisions-Summe je Claim (nur Umsatz-Status).
    let claim_rows: Vec<(i64, Option<String>, Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT c.id::bigint, c.claimed_streamer_login, c.claimed_at, \
                COALESCE(SUM(co.commission_cents), 0)::bigint, COUNT(co.id)::bigint \
         FROM affiliate_streamer_claims c \
         LEFT JOIN affiliate_commissions co \
           ON co.affiliate_twitch_login = c.affiliate_twitch_login \
          AND co.streamer_login = c.claimed_streamer_login \
          AND co.status IN ('pending', 'transferred') \
         WHERE c.affiliate_twitch_login = $1 \
         GROUP BY c.id, c.claimed_streamer_login, c.claimed_at \
         ORDER BY c.claimed_at DESC",
    )
    .bind(login)
    .fetch_all(pool)
    .await
    .map_err(DetailError::Db)?;

    let total_claims: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_streamer_claims WHERE affiliate_twitch_login = $1")
            .bind(login)
            .fetch_one(pool)
            .await
            .map_err(DetailError::Db)?;

    let (total_provision_cents, active_customers): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(commission_cents), 0)::bigint, COUNT(DISTINCT streamer_login)::bigint \
         FROM affiliate_commissions WHERE affiliate_twitch_login = $1 AND status IN ('pending', 'transferred')",
    )
    .bind(login)
    .fetch_one(pool)
    .await
    .map_err(DetailError::Db)?;

    let pii = load_affiliate_pii(pool, cipher, login).await.map_err(|e| match e {
        PiiError::Db(e) => DetailError::Db(e),
        PiiError::Decrypt(s) => DetailError::Decrypt(s),
    })?;
    let readiness = build_readiness(&pii);

    let (gut_count, gut_total_cents): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COALESCE(SUM(gross_amount_cents), 0)::bigint \
         FROM affiliate_gutschriften WHERE affiliate_twitch_login = $1",
    )
    .bind(login)
    .fetch_one(pool)
    .await
    .map_err(DetailError::Db)?;

    // Stripe-Account-ID maskieren (>12 Zeichen → erste 8 … letzte 4).
    let stripe_id = stripe_account_id.unwrap_or_default();
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
        .map(|(id, customer, claimed_at, comm_cents, comm_count)| {
            json!({
                "id": id,
                "customer_login": customer.unwrap_or_default().trim(),
                "claimed_at": claimed_at,
                "commission_cents": comm_cents,
                "commission_count": comm_count,
            })
        })
        .collect();

    let avg_provision = if total_claims > 0 {
        round2((total_provision_cents as f64 / total_claims.max(1) as f64) / 100.0)
    } else {
        0.0
    };

    Ok(json!({
        "affiliate": {
            "login": twitch_login.unwrap_or_default().trim(),
            "display_name": display_name,
            "active": is_active != 0,
            "created_at": created_at,
            "email": email,
            "stripe_connect_status": stripe_status,
            "stripe_account_id": if masked.is_empty() { Value::Null } else { json!(masked) },
            "updated_at": updated_at,
        },
        "claims": claims,
        "stats": {
            "total_claims": total_claims,
            "total_provision": cents_to_amount(total_provision_cents),
            "avg_provision": avg_provision,
            "active_customers": active_customers,
        },
        "ust_status": pii.ust_status,
        "pii_readiness": readiness,
        "gutschriften_summary": { "count": gut_count, "total_gross_cents": gut_total_cents },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn connect(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        Some(PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap())
    }

    async fn with_tables(pool: &PgPool) {
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT)",
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, ust_status TEXT NOT NULL DEFAULT 'unknown')",
            "CREATE TABLE affiliate_streamer_claims (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, commission_cents INTEGER, status TEXT, created_at TEXT)",
            "CREATE TABLE affiliate_gutschriften (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, gross_amount_cents INTEGER, pdf_generated_at TEXT, email_sent_at TEXT)",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn list_aggregiert_und_pii() {
        let Some(pool) = connect("t_aff_list").await else { return };
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
        let Some(pool) = connect("t_aff_list_nopii").await else { return };
        // Nur die Kern-Tabellen, KEIN affiliate_pii → Fallback-Query.
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT)",
            "CREATE TABLE affiliate_streamer_claims (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, commission_cents INTEGER, status TEXT, created_at TEXT)",
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
        let Some(pool) = connect("t_aff_list_empty").await else { return };
        let v = load_affiliates_list(&pool).await.unwrap();
        assert_eq!(v["affiliates"], json!([]));
    }

    #[test]
    fn pure_helfer() {
        assert_eq!(parse_commission_ids(Some("[1, 2, \"3\"]")), vec![1, 2, 3]);
        assert_eq!(parse_commission_ids(Some("kein json")), Vec::<i64>::new());
        assert_eq!(parse_commission_ids(None), Vec::<i64>::new());
        assert_eq!(gutschrift_note_text(Some("Kleinunternehmer")), "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet.");
        assert_eq!(gutschrift_note_text(Some("regular")), "");
    }

    async fn with_gutschrift_tables(pool: &PgPool) {
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT)",
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, ust_status TEXT NOT NULL DEFAULT 'unknown')",
            "CREATE TABLE affiliate_gutschriften (id BIGSERIAL PRIMARY KEY, gutschrift_number TEXT, affiliate_twitch_login TEXT, period_year INTEGER, period_month INTEGER, net_amount_cents INTEGER, vat_amount_cents INTEGER, gross_amount_cents INTEGER, affiliate_ust_status TEXT, email_error TEXT, pdf_blob BYTEA, pdf_generated_at TEXT, email_sent_at TEXT, commission_ids TEXT, created_at TEXT)",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn gutschriften_liste_payload() {
        let Some(pool) = connect("t_aff_gut").await else { return };
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
        assert_eq!(g["download_path"], "/twitch/api/admin/affiliates/gutschriften/1/pdf");
        assert_eq!(g["affiliate_login"], "nani");
        assert_eq!(g["display_name"], "Nani");
        assert_eq!(g["ust_status"], "kleinunternehmer");
        assert_eq!(g["has_pii"], true);
    }

    #[tokio::test]
    async fn gutschriften_ohne_tabellen_leer() {
        let Some(pool) = connect("t_aff_gut_empty").await else { return };
        let v = load_affiliate_gutschriften(&pool).await.unwrap();
        assert_eq!(v["count"], 0);
        assert_eq!(v["gutschriften"], json!([]));
    }

    #[tokio::test]
    async fn toggle_flippt_und_not_found() {
        let Some(pool) = connect("t_aff_toggle").await else { return };
        sqlx::query("CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, is_active INTEGER NOT NULL DEFAULT 1, updated_at TEXT)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, is_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();

        // 1 → 0.
        let v = toggle_affiliate(&pool, "nani").await.unwrap();
        assert_eq!(v["login"], "nani");
        assert_eq!(v["active"], false);
        let stored: i32 = sqlx::query_scalar("SELECT is_active FROM affiliate_accounts WHERE twitch_login='nani'").fetch_one(&pool).await.unwrap();
        assert_eq!(stored, 0);
        let ts: Option<String> = sqlx::query_scalar("SELECT updated_at FROM affiliate_accounts WHERE twitch_login='nani'").fetch_one(&pool).await.unwrap();
        assert!(ts.is_some(), "updated_at gesetzt");

        // 0 → 1.
        let v = toggle_affiliate(&pool, "nani").await.unwrap();
        assert_eq!(v["active"], true);

        // unbekannter Login → NotFound.
        assert!(matches!(toggle_affiliate(&pool, "ghost").await, Err(ToggleError::NotFound)));
    }

    #[tokio::test]
    async fn toggle_fehlendes_schema_not_found() {
        let Some(pool) = connect("t_aff_toggle_empty").await else { return };
        assert!(matches!(toggle_affiliate(&pool, "nani").await, Err(ToggleError::NotFound)));
    }

    #[tokio::test]
    async fn detail_vollstaendig_und_not_found() {
        let Some(pool) = connect("t_aff_detail").await else { return };
        let cipher = FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap();
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT, email TEXT, stripe_connect_status TEXT, stripe_account_id TEXT, updated_at TEXT)",
            "CREATE TABLE affiliate_pii (twitch_login TEXT PRIMARY KEY, full_name_enc BYTEA, email_enc BYTEA, address_line1_enc BYTEA, address_city_enc BYTEA, address_zip_enc BYTEA, tax_id_enc BYTEA, address_country TEXT, ust_status TEXT, updated_at TEXT)",
            "CREATE TABLE affiliate_streamer_claims (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, claimed_streamer_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, streamer_login TEXT, commission_cents INTEGER, status TEXT)",
            "CREATE TABLE affiliate_gutschriften (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, gross_amount_cents INTEGER)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, display_name, is_active, email, stripe_connect_status, stripe_account_id) VALUES ('nani','Nani',1,'a@b.de','active','acct_1234567890XY')").execute(&pool).await.unwrap();
        // PII vollständig → can_generate.
        let enc = |field: &str, v: &str| cipher.encrypt_field(v, &format!("affiliate_pii|{field}|nani")).unwrap();
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
        assert!(matches!(load_affiliate_detail(&pool, &cipher, "ghost").await, Err(DetailError::NotFound)));
    }

    #[tokio::test]
    async fn fehlendes_schema_gibt_nullwerte() {
        let Some(pool) = connect("t_aff_empty").await else { return };
        // Keine Tabellen → missing-schema → Nullwerte.
        let v = load_affiliate_stats(&pool, "2026-06-01T00:00:00+00:00").await.unwrap();
        assert_eq!(v["total_affiliates"], 0);
        assert_eq!(v["total_provision"], 0.0);
        assert_eq!(v["pending_email_gutschriften"], 0);
    }

    #[tokio::test]
    async fn stats_aggregiert_korrekt() {
        let Some(pool) = connect("t_aff_stats").await else { return };
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

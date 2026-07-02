//! Affiliate-Provisions-Verbuchung (30 % bei bezahlter Stripe-Invoice).
//!
//! Port von `bot/dashboard/affiliate/affiliate_mixin.py:_affiliate_process_commission`
//! (+ `_affiliate_replay_pending_commissions`, `_affiliate_transfer_commission`).
//!
//! # Ablauf (Python-Parität)
//! Bei `invoice.payment_succeeded` (siehe Hook im Stripe-Webhook):
//! 1. Streamer aus `twitch_billing_subscriptions` per `stripe_customer_id` lösen
//!    (Identifier ist `customer_reference`, ein Twitch-Login).
//! 2. Werbenden Affiliate aus `affiliate_streamer_claims` lösen.
//! 3. 30 % des bezahlten Brutto als `commission_cents` (Floor) berechnen.
//! 4. Unter Affiliate-Advisory-Lock idempotent in `affiliate_commissions`
//!    einfügen und committen (UNIQUE auf `stripe_event_id` → Replays sind
//!    `duplicate`).
//! 5. Erst danach in einer separaten Transaktion ausstehende Provisionen per
//!    Stripe-Transfer auszahlen; sonst bleibt der Eintrag `pending` (bzw.
//!    `skipped`, wenn die offene Summe den Sicherheitsdeckel überschreitet).
//!
//! Der Lock-Schlüssel ist bit-identisch zu Pythons `zlib.crc32(login)` (signed
//! i32) im Namespace `1_103_151_689`, sodass Rust- und Python-Pfad während des
//! Cutovers denselben Postgres-Advisory-Lock teilen.

use sqlx::{PgPool, Postgres, Transaction};

use crate::affiliate_claim_window::sql_claim_window_predicate;
use crate::stripe::StripeClient;

/// 30 % Provision auf das bezahlte Brutto (Python `_COMMISSION_RATE`).
const COMMISSION_RATE_NUM: i64 = 30;
const COMMISSION_RATE_DEN: i64 = 100;
/// Sicherheitsdeckel offener Provisionen ohne Connect-Konto (Python
/// `_MAX_PENDING_COMMISSION_CENTS`): darüber wird neu Erfasstes `skipped`.
const MAX_PENDING_COMMISSION_CENTS: i64 = 5000;
/// Lock-Namespace (Python `_AFFILIATE_COMMISSION_LOCK_NAMESPACE`).
const LOCK_NAMESPACE: i32 = 1_103_151_689;

/// Eingehende Invoice-Daten aus dem `invoice.payment_succeeded`-Event.
#[derive(Debug, Clone)]
pub struct InvoicePayment<'a> {
    pub stripe_event_id: &'a str,
    pub stripe_customer_id: &'a str,
    pub amount_paid_cents: i64,
    pub currency: &'a str,
    pub invoice_id: &'a str,
    pub period_start: &'a str,
    pub period_end: &'a str,
}

/// Ergebnis der Verbuchung — stabile Strings (Python-Parität).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommissionOutcome {
    /// Kein Streamer zum `stripe_customer_id`.
    NoStreamer,
    /// Streamer hat keinen werbenden Affiliate.
    NoAffiliate,
    /// Betrag ≤ 0 oder Provision ≤ 0 → nichts verbucht.
    Skipped,
    /// `stripe_event_id` bereits verbucht (Stripe-Replay).
    Duplicate,
    /// Verbucht; finaler Provisions-Status (`pending`/`skipped`/`transferred`).
    Recorded(String),
}

impl CommissionOutcome {
    /// Stabiler Status-String (identisch zu Pythons Rückgabewerten).
    pub fn as_str(&self) -> &str {
        match self {
            CommissionOutcome::NoStreamer => "no_streamer",
            CommissionOutcome::NoAffiliate => "no_affiliate",
            CommissionOutcome::Skipped => "skipped",
            CommissionOutcome::Duplicate => "duplicate",
            CommissionOutcome::Recorded(status) => status,
        }
    }
}

/// CRC-32 (IEEE) eines Byte-Slices — bit-identisch zu Pythons `zlib.crc32`.
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// (namespace, lock_key) für `pg_advisory_*_lock` (Python
/// `_affiliate_commission_lock_key`): `crc32(lower(login))` als signed i32.
fn commission_lock_key(affiliate_login: &str) -> (i32, i32) {
    let normalized = affiliate_login.trim().to_lowercase();
    (LOCK_NAMESPACE, crc32_ieee(normalized.as_bytes()) as i32)
}

fn cents_to_int4(value: i64, field: &str) -> Result<i32, sqlx::Error> {
    i32::try_from(value)
        .map_err(|_| sqlx::Error::InvalidArgument(format!("{field} out of int4 range: {value}")))
}

async fn existing_commission_for_event(
    pool: &PgPool,
    stripe_event_id: &str,
) -> Result<Option<(i32, String, String)>, sqlx::Error> {
    sqlx::query_as::<_, (i32, String, String)>(
        r#"
        SELECT id, affiliate_twitch_login, status
        FROM affiliate_commissions
        WHERE stripe_event_id = $1
        "#,
    )
    .bind(stripe_event_id)
    .fetch_optional(pool)
    .await
}

async fn acquire_commission_lock(
    tx: &mut Transaction<'_, Postgres>,
    affiliate_login: &str,
) -> Result<(), sqlx::Error> {
    let (lock_ns, lock_key) = commission_lock_key(affiliate_login);
    // Transaktions-Advisory-Lock: löst beim Commit/Rollback automatisch aus
    // (Python nimmt einen Session-Lock + explizites Unlock — selber Schlüssel,
    // damit Python- und Rust-Pfad sich gegenseitig serialisieren).
    sqlx::query!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock($1, $2)) AS _lock"#,
        lock_ns,
        lock_key
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(())
}

/// Speichert die Stripe-Connect-Account-ID und replayt danach pending/failed
/// Provisionen unter demselben Affiliate-Advisory-Lock.
///
/// Port von `affiliate_mixin.py:_affiliate_connect_stripe_sync`: erst
/// `stripe_account_id`/Connect-Status aktualisieren, dann
/// `_affiliate_replay_pending_commissions(..., commit=False)`.
pub async fn connect_account_and_replay(
    pool: &PgPool,
    stripe: Option<&StripeClient>,
    affiliate_login: &str,
    stripe_account_id: &str,
) -> Result<(), sqlx::Error> {
    let affiliate_login = affiliate_login.trim().to_lowercase();
    let stripe_account_id = stripe_account_id.trim();
    if affiliate_login.is_empty() || stripe_account_id.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
    let mut tx = pool.begin().await?;
    acquire_commission_lock(&mut tx, &affiliate_login).await?;

    sqlx::query(
        r#"
        UPDATE affiliate_accounts
        SET stripe_account_id = $1,
            stripe_connected_at = $2,
            stripe_connect_status = 'connected',
            updated_at = $3
        WHERE twitch_login = $4
        "#,
    )
    .bind(stripe_account_id)
    .bind(&now)
    .bind(&now)
    .bind(&affiliate_login)
    .execute(&mut *tx)
    .await?;

    replay_pending_commissions_in_tx(&mut tx, stripe, stripe_account_id, &affiliate_login).await?;
    tx.commit().await?;
    Ok(())
}

/// Verbucht (und zahlt ggf. aus) die Affiliate-Provision für eine bezahlte
/// Invoice. `stripe` ist optional — ohne Client bleibt eine fällige Auszahlung
/// `pending` (genau wie Pythons Pfad ohne Connect-Konto/Client).
pub async fn process_commission(
    pool: &PgPool,
    stripe: Option<&StripeClient>,
    invoice: &InvoicePayment<'_>,
) -> Result<CommissionOutcome, sqlx::Error> {
    if let Some((_id, existing_affiliate_login, _status)) =
        existing_commission_for_event(pool, invoice.stripe_event_id).await?
    {
        replay_pending_commissions_for_affiliate(pool, stripe, &existing_affiliate_login).await?;
        return Ok(CommissionOutcome::Duplicate);
    }

    // 1. Streamer aus dem Abo lösen. Der Streamer-Identifier ist
    // `customer_reference` (ein Twitch-Login, siehe stripe/webhook_apply.rs:351);
    // eine `twitch_login`-Spalte hat `twitch_billing_subscriptions` nie gehabt.
    // Normalisierung wie Python: `str(...).strip().lower()`.
    let streamer_login: Option<String> = sqlx::query_scalar!(
        r#"
        SELECT LOWER(TRIM(COALESCE(customer_reference, ''))) AS "streamer_login!"
        FROM twitch_billing_subscriptions
        WHERE stripe_customer_id = $1
        "#,
        invoice.stripe_customer_id
    )
    .fetch_optional(pool)
    .await?;
    let Some(streamer_login) = streamer_login.filter(|s| !s.is_empty()) else {
        return Ok(CommissionOutcome::NoStreamer);
    };

    // 2. Werbenden Affiliate und Aktivierungsanker lösen. Provisionsberechtigt
    // ist der Claim nur im zentralen Claim-Zeitfenster relativ zu `partnered_at`.
    let claim_row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        r#"
        SELECT c.affiliate_twitch_login,
               c.claimed_at,
               ps.created_at AS partnered_at
        FROM affiliate_streamer_claims c
        LEFT JOIN twitch_streamers_partner_state ps
          ON LOWER(ps.twitch_login) = LOWER(c.claimed_streamer_login)
         AND COALESCE(ps.is_partner_active, 0) = 1
        WHERE LOWER(c.claimed_streamer_login) = LOWER($1)
        ORDER BY COALESCE(ps.is_partner_active, 0) DESC
        LIMIT 1
        "#,
    )
    .bind(&streamer_login)
    .fetch_optional(pool)
    .await?;
    let Some((affiliate_login, claimed_at, partnered_at)) = claim_row else {
        return Ok(CommissionOutcome::NoAffiliate);
    };
    let affiliate_login = affiliate_login.trim().to_lowercase();
    if affiliate_login.is_empty() {
        return Ok(CommissionOutcome::NoAffiliate);
    }
    let Some(claimed_at) = claimed_at
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
    else {
        tracing::warn!(streamer_login = %streamer_login, "Affiliate-Claim ohne claimed_at; keine Provision");
        return Ok(CommissionOutcome::NoAffiliate);
    };
    let Some(partnered_at) = partnered_at
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
    else {
        tracing::warn!(streamer_login = %streamer_login, "Affiliate-Claim ohne partnered_at; keine Provision");
        return Ok(CommissionOutcome::NoAffiliate);
    };
    let window_predicate = sql_claim_window_predicate("$1", "$2");
    let window_sql = format!("SELECT {window_predicate} AS eligible");
    let eligible: bool = sqlx::query_scalar(&window_sql)
        .bind(&claimed_at)
        .bind(&partnered_at)
        .fetch_one(pool)
        .await?;
    if !eligible {
        return Ok(CommissionOutcome::NoAffiliate);
    }

    // 3. Provision (Floor von 30 %). Arithmetik in i64 (wie Pythons unbeschränktes
    // int), gespeichert wird in INTEGER-Spalten (siehe `commission_cents_i32`).
    if invoice.amount_paid_cents <= 0 {
        return Ok(CommissionOutcome::Skipped);
    }
    let commission_cents = invoice.amount_paid_cents * COMMISSION_RATE_NUM / COMMISSION_RATE_DEN;
    if commission_cents <= 0 {
        return Ok(CommissionOutcome::Skipped);
    }
    // `brutto_cents`/`commission_cents` sind INTEGER (INT4) → für den Bind nach
    // i32 verengen. Cent-Beträge realer Invoices passen weit in i32.
    let brutto_cents_i32 = cents_to_int4(invoice.amount_paid_cents, "brutto_cents")?;
    let commission_cents_i32 = cents_to_int4(commission_cents, "commission_cents")?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false);

    // 4. Ledger-Phase: unter Advisory-Lock idempotent einfügen und committen,
    // BEVOR ein Stripe-Transfer versucht wird. Crash nach externem Transfer darf
    // die Commission-ID nie wieder verschwinden lassen, weil sie den
    // Stripe-Idempotency-Key stabilisiert.
    let mut tx = pool.begin().await?;
    acquire_commission_lock(&mut tx, &affiliate_login).await?;

    let stripe_account_id: String = sqlx::query_scalar!(
        "SELECT stripe_account_id FROM affiliate_accounts WHERE twitch_login = $1",
        &affiliate_login
    )
    .fetch_optional(&mut *tx)
    .await?
    .flatten()
    .unwrap_or_default()
    .trim()
    .to_string();

    // Ohne Connect-Konto: Deckel auf offene Provisionssumme anwenden.
    let mut initial_status = "pending";
    if stripe_account_id.is_empty() {
        let pending_total: i64 = sqlx::query_scalar!(
            r#"
            SELECT COALESCE(SUM(commission_cents), 0)::bigint AS "pending_total!"
            FROM affiliate_commissions
            WHERE affiliate_twitch_login = $1 AND status = 'pending'
            "#,
            &affiliate_login
        )
        .fetch_one(&mut *tx)
        .await?;
        if pending_total + commission_cents > MAX_PENDING_COMMISSION_CENTS {
            initial_status = "skipped";
        }
    }

    let inserted_row: Option<(i32, String, String)> = sqlx::query_as(
        r#"
        INSERT INTO affiliate_commissions
            (affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id,
             stripe_customer_id, brutto_cents, commission_cents, currency,
             status, period_start, period_end, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT (stripe_event_id) DO NOTHING
        RETURNING id, affiliate_twitch_login, status
        "#,
    )
    .bind(&affiliate_login)
    .bind(&streamer_login)
    .bind(invoice.stripe_event_id)
    .bind(invoice.invoice_id)
    .bind(invoice.stripe_customer_id)
    .bind(brutto_cents_i32)
    .bind(commission_cents_i32)
    .bind(invoice.currency)
    .bind(initial_status)
    .bind(invoice.period_start)
    .bind(invoice.period_end)
    .bind(&now)
    .fetch_optional(&mut *tx)
    .await?;

    let (commission_id, ledger_affiliate_login, ledger_status, inserted_new) =
        if let Some((id, affiliate, status)) = inserted_row {
            (id, affiliate, status, true)
        } else {
            let (id, affiliate, status): (i32, String, String) = sqlx::query_as(
                r#"
                SELECT id, affiliate_twitch_login, status
                FROM affiliate_commissions
                WHERE stripe_event_id = $1
                "#,
            )
            .bind(invoice.stripe_event_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
            (id, affiliate, status, false)
        };

    tx.commit().await?;

    if inserted_new && stripe_account_id.is_empty() {
        return Ok(CommissionOutcome::Recorded(ledger_status));
    }

    replay_pending_commissions_for_affiliate(pool, stripe, &ledger_affiliate_login).await?;

    if !inserted_new {
        return Ok(CommissionOutcome::Duplicate);
    }

    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM affiliate_commissions WHERE id = $1")
            .bind(commission_id)
            .fetch_optional(pool)
            .await?;
    Ok(CommissionOutcome::Recorded(status.unwrap_or(ledger_status)))
}

/// Eine offene/fehlgeschlagene Provision (für Replay-Transfer).
#[derive(sqlx::FromRow)]
struct PendingCommission {
    id: i32,
    stripe_event_id: Option<String>,
    // INTEGER-Spalte (INT4) → i32.
    commission_cents: Option<i32>,
    currency: Option<String>,
}

/// Zahlt alle offenen Provisionen eines Affiliates per Stripe-Transfer aus
/// (Python `_affiliate_replay_pending_commissions`). Reihenfolge wie Python:
/// `created_at ASC, id ASC`.
pub async fn replay_pending_commissions(
    pool: &PgPool,
    stripe: Option<&StripeClient>,
    stripe_account_id: &str,
    affiliate_login: &str,
) -> Result<(), sqlx::Error> {
    let affiliate_login = affiliate_login.trim().to_lowercase();
    if affiliate_login.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    acquire_commission_lock(&mut tx, &affiliate_login).await?;
    replay_pending_commissions_in_tx(&mut tx, stripe, stripe_account_id, &affiliate_login).await?;
    tx.commit().await?;
    Ok(())
}

async fn replay_pending_commissions_for_affiliate(
    pool: &PgPool,
    stripe: Option<&StripeClient>,
    affiliate_login: &str,
) -> Result<(), sqlx::Error> {
    let affiliate_login = affiliate_login.trim().to_lowercase();
    if affiliate_login.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    acquire_commission_lock(&mut tx, &affiliate_login).await?;
    let stripe_account_id: String = sqlx::query_scalar!(
        "SELECT stripe_account_id FROM affiliate_accounts WHERE twitch_login = $1",
        &affiliate_login
    )
    .fetch_optional(&mut *tx)
    .await?
    .flatten()
    .unwrap_or_default()
    .trim()
    .to_string();
    if !stripe_account_id.is_empty() {
        replay_pending_commissions_in_tx(&mut tx, stripe, &stripe_account_id, &affiliate_login)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn replay_pending_commissions_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    stripe: Option<&StripeClient>,
    stripe_account_id: &str,
    affiliate_login: &str,
) -> Result<(), sqlx::Error> {
    let pending: Vec<PendingCommission> = sqlx::query_as!(
        PendingCommission,
        r#"
        SELECT id AS "id!",
               stripe_event_id AS "stripe_event_id?",
               commission_cents AS "commission_cents?",
               currency AS "currency?"
        FROM affiliate_commissions
        WHERE affiliate_twitch_login = $1
          AND status IN ('pending', 'failed')
          AND stripe_transfer_id IS NULL
          AND transferred_at IS NULL
        ORDER BY created_at ASC, id ASC
        "#,
        affiliate_login
    )
    .fetch_all(&mut **tx)
    .await?;

    for row in pending {
        transfer_commission_in_tx(
            tx,
            stripe,
            stripe_account_id,
            row.id,
            &row.stripe_event_id.unwrap_or_default(),
            row.commission_cents.unwrap_or(0),
            &row.currency.unwrap_or_else(|| "eur".to_string()),
        )
        .await?;
    }
    Ok(())
}

/// Führt einen einzelnen Provisions-Transfer aus (Python
/// `_affiliate_transfer_commission`). Ohne Client/bei Transfer-Fehler bleibt die
/// Provision `pending` (mit `error_message`); bei Erfolg `transferred`.
async fn transfer_commission_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    stripe: Option<&StripeClient>,
    stripe_account_id: &str,
    commission_id: i32,
    stripe_event_id: &str,
    commission_cents: i32,
    currency: &str,
) -> Result<(), sqlx::Error> {
    let idempotency_key = format!("affiliate-transfer:{commission_id}");

    let amount = u64::try_from(commission_cents).unwrap_or(0);
    let transfer = match stripe {
        Some(client) if amount > 0 => {
            client
                .create_transfer(
                    amount,
                    currency,
                    stripe_account_id,
                    stripe_event_id,
                    Some(&idempotency_key),
                )
                .await
        }
        // Kein Client (oder 0-Betrag): wie Pythons Fehlerpfad → pending bleiben.
        _ => Err(crate::stripe::StripeError::SecretKeyMissing),
    };

    match transfer {
        Ok(value) => {
            let transfer_id = value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let transferred_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
            sqlx::query(
                r#"
                UPDATE affiliate_commissions
                SET status = 'transferred',
                    stripe_transfer_id = $1,
                    transferred_at = $2,
                    error_message = NULL
                WHERE id = $3
                  AND stripe_transfer_id IS NULL
                  AND transferred_at IS NULL
                "#,
            )
            .bind(transfer_id)
            .bind(&transferred_at)
            .bind(commission_id)
            .execute(&mut **tx)
            .await?;
        }
        Err(error) => {
            tracing::warn!(%error, commission_id, "Affiliate Stripe-Transfer fehlgeschlagen");
            let mut msg = error.to_string();
            msg.truncate(500);
            sqlx::query(
                r#"
                UPDATE affiliate_commissions
                SET status = 'pending',
                    stripe_transfer_id = NULL,
                    transferred_at = NULL,
                    error_message = $1
                WHERE id = $2
                  AND stripe_transfer_id IS NULL
                  AND transferred_at IS NULL
                "#,
            )
            .bind(&msg)
            .bind(commission_id)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── crc32 / Lock-Schlüssel: bit-identisch zu Python zlib.crc32 ──────────

    #[test]
    fn crc32_konkrete_referenzwerte() {
        // zlib.crc32(b"123456789") == 0xCBF43926 (CRC-32/IEEE Standard-Vektor).
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
        // zlib.crc32(b"") == 0.
        assert_eq!(crc32_ieee(b""), 0);
    }

    #[test]
    fn lock_key_namespace_und_signed_cast() {
        let (ns, _key) = commission_lock_key("Nani");
        assert_eq!(ns, 1_103_151_689);
        // Lowercase-Normalisierung: gleiche Schlüssel für "Nani"/"nani".
        assert_eq!(commission_lock_key("Nani"), commission_lock_key("  nani  "));
        // Großer crc32 wird als signed i32 negativ (zlib.crc32 → >=2^31).
        // "123456789" → 0xCBF43926 > 2^31 → negativ.
        let (_ns, key) = commission_lock_key("123456789");
        assert!(key < 0, "großer CRC muss als negativer i32 landen: {key}");
        assert_eq!(key, 0xCBF4_3926_u32 as i32);
    }

    #[test]
    fn outcome_status_strings() {
        assert_eq!(CommissionOutcome::NoStreamer.as_str(), "no_streamer");
        assert_eq!(CommissionOutcome::NoAffiliate.as_str(), "no_affiliate");
        assert_eq!(CommissionOutcome::Skipped.as_str(), "skipped");
        assert_eq!(CommissionOutcome::Duplicate.as_str(), "duplicate");
        assert_eq!(
            CommissionOutcome::Recorded("pending".into()).as_str(),
            "pending"
        );
        assert_eq!(
            CommissionOutcome::Recorded("transferred".into()).as_str(),
            "transferred"
        );
    }

    // ── DB-Integration (skip ohne TB_TEST_DATABASE_URL) ─────────────────────

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
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        // Schema TREU zur echten Migration:
        //   - twitch_billing_subscriptions: Streamer-Identifier ist `customer_reference`
        //     (kein `twitch_login`!), Quelle migrations/20260601000000_baseline_schema.sql.
        //   - affiliate_*: Spalten/Typen aus migrations/20260617030000_baseline_missing_tables.sql
        //     (commission_cents/brutto_cents = INTEGER, id = INTEGER IDENTITY,
        //     stripe_event_id UNIQUE). FK-REFERENCES auf affiliate_accounts hier
        //     weggelassen — orthogonal zum Geld-Pfad und unnötige Fixture-Last.
        sqlx::query(
            "CREATE TABLE twitch_billing_subscriptions (\
                stripe_subscription_id TEXT PRIMARY KEY, stripe_customer_id TEXT, \
                customer_reference TEXT, status TEXT NOT NULL DEFAULT 'unknown', \
                updated_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE affiliate_streamer_claims (\
                id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, \
                affiliate_twitch_login TEXT NOT NULL, claimed_streamer_login TEXT NOT NULL, \
                claimed_at TEXT, UNIQUE (claimed_streamer_login))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_streamers_partner_state (\
                twitch_login TEXT, is_partner_active INTEGER NOT NULL DEFAULT 1, \
                created_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE affiliate_accounts (\
                twitch_login TEXT PRIMARY KEY, stripe_account_id TEXT, \
                stripe_connected_at TEXT, stripe_connect_status TEXT, updated_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE affiliate_commissions (\
                id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, \
                affiliate_twitch_login TEXT NOT NULL, streamer_login TEXT NOT NULL, \
                stripe_event_id TEXT UNIQUE NOT NULL, stripe_invoice_id TEXT, \
                stripe_customer_id TEXT, stripe_transfer_id TEXT, \
                brutto_cents INTEGER NOT NULL, commission_cents INTEGER NOT NULL, \
                currency TEXT NOT NULL DEFAULT 'eur', status TEXT NOT NULL DEFAULT 'pending', \
                period_start TEXT, period_end TEXT, created_at TEXT NOT NULL, \
                transferred_at TEXT, error_message TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn invoice<'a>(event_id: &'a str, customer: &'a str, amount: i64) -> InvoicePayment<'a> {
        InvoicePayment {
            stripe_event_id: event_id,
            stripe_customer_id: customer,
            amount_paid_cents: amount,
            currency: "eur",
            invoice_id: "in_1",
            period_start: "2026-06-01",
            period_end: "2026-07-01",
        }
    }

    struct DedupeStripeServer {
        base_url: String,
        effective_transfers: Arc<AtomicUsize>,
        requests: Arc<AtomicUsize>,
        _task: tokio::task::JoinHandle<()>,
    }

    fn header_value(request: &str, name: &str) -> Option<String> {
        request.lines().find_map(|line| {
            let (header_name, value) = line.split_once(':')?;
            header_name
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }

    fn request_complete(request: &[u8]) -> bool {
        let Some(header_end) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = header_value(&headers, "content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        request.len() >= header_end + 4 + content_length
    }

    async fn start_dedupe_stripe_server(
        gate: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
    ) -> DedupeStripeServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(HashMap::<String, String>::new()));
        let effective_transfers = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Mutex::new(gate));
        let task = {
            let seen = Arc::clone(&seen);
            let effective_transfers = Arc::clone(&effective_transfers);
            let requests = Arc::clone(&requests);
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _addr)) = listener.accept().await else {
                        break;
                    };
                    let seen = Arc::clone(&seen);
                    let effective_transfers = Arc::clone(&effective_transfers);
                    let requests = Arc::clone(&requests);
                    let gate = Arc::clone(&gate);
                    tokio::spawn(async move {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let mut request = Vec::new();
                        let mut buf = [0_u8; 1024];
                        loop {
                            let n = match stream.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => n,
                            };
                            request.extend_from_slice(&buf[..n]);
                            if request_complete(&request) {
                                break;
                            }
                        }
                        let request_text = String::from_utf8_lossy(&request);
                        let idempotency_key = header_value(&request_text, "idempotency-key")
                            .unwrap_or_else(|| "missing".to_string());
                        let transfer_id = {
                            let mut seen = seen.lock().unwrap();
                            if let Some(id) = seen.get(&idempotency_key) {
                                id.clone()
                            } else {
                                effective_transfers.fetch_add(1, Ordering::SeqCst);
                                let id = format!("tr_effective_{}", seen.len() + 1);
                                seen.insert(idempotency_key, id.clone());
                                id
                            }
                        };
                        let gate_pair = { gate.lock().unwrap().take() };
                        if let Some((seen_tx, release_rx)) = gate_pair {
                            let _ = seen_tx.send(());
                            let _ = release_rx.await;
                        }
                        let body = format!(r#"{{"id":"{transfer_id}"}}"#);
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                    });
                }
            })
        };
        DedupeStripeServer {
            base_url,
            effective_transfers,
            requests,
            _task: task,
        }
    }

    async fn seed_claim(pool: &PgPool) {
        seed_claim_with_times(
            pool,
            "2026-06-01T00:00:00+00:00",
            Some("2026-06-03T00:00:00+00:00"),
        )
        .await;
    }

    async fn seed_claim_with_times(pool: &PgPool, claimed_at: &str, partnered_at: Option<&str>) {
        // customer_reference trägt den Streamer-Login (gemischte Groß-/Kleinschreibung
        // → der Code normalisiert per LOWER/TRIM auf 'streamerx').
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, stripe_customer_id, customer_reference, updated_at) VALUES ('sub_1', 'cus_1', 'StreamerX', '2026-06-01')")
            .execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO affiliate_streamer_claims \
                (affiliate_twitch_login, claimed_streamer_login, claimed_at) \
             VALUES ('aff1', 'streamerx', $1)",
        )
        .bind(claimed_at)
        .execute(pool)
        .await
        .unwrap();
        if let Some(partnered_at) = partnered_at {
            sqlx::query(
                "INSERT INTO twitch_streamers_partner_state \
                    (twitch_login, is_partner_active, created_at) \
                 VALUES ('streamerx', 1, $1)",
            )
            .bind(partnered_at)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn no_streamer_when_customer_unknown() {
        let Some(pool) = connect("comm_nostreamer").await else {
            return;
        };
        let out = process_commission(&pool, None, &invoice("evt", "cus_unknown", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::NoStreamer);
    }

    #[tokio::test]
    async fn no_affiliate_when_unclaimed() {
        let Some(pool) = connect("comm_noaff").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, stripe_customer_id, customer_reference, updated_at) VALUES ('sub_1', 'cus_1', 'StreamerX', '2026-06-01')")
            .execute(&pool).await.unwrap();
        let out = process_commission(&pool, None, &invoice("evt", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::NoAffiliate);
    }

    #[tokio::test]
    async fn records_pending_thirty_percent_without_account() {
        let Some(pool) = connect("comm_pending").await else {
            return;
        };
        seed_claim(&pool).await;
        // 1000 Cent → 30 % = 300 Cent, kein Connect-Konto → pending.
        let out = process_commission(&pool, None, &invoice("evt_1", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::Recorded("pending".into()));
        // commission_cents ist INTEGER (INT4) → i32.
        let (status, commission): (String, i32) = sqlx::query_as(
            "SELECT status, commission_cents FROM affiliate_commissions WHERE stripe_event_id = 'evt_1'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "pending");
        assert_eq!(commission, 300);
    }

    #[tokio::test]
    async fn commission_claim_innerhalb_fenster_wird_attribuiert() {
        let Some(pool) = connect("comm_claim_window_in").await else {
            return;
        };
        seed_claim_with_times(
            &pool,
            "2026-06-01T00:00:00+00:00",
            Some("2026-06-03T00:00:00+00:00"),
        )
        .await;

        let out = process_commission(&pool, None, &invoice("evt_win", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::Recorded("pending".into()));
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM affiliate_commissions WHERE stripe_event_id = 'evt_win'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn commission_claim_vor_fenster_ist_no_affiliate() {
        let Some(pool) = connect("comm_claim_window_before").await else {
            return;
        };
        seed_claim_with_times(
            &pool,
            "2026-05-29T23:59:59+00:00",
            Some("2026-06-03T00:00:00+00:00"),
        )
        .await;

        let out = process_commission(&pool, None, &invoice("evt_before", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::NoAffiliate);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_commissions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn commission_claim_nach_fenster_ist_no_affiliate() {
        let Some(pool) = connect("comm_claim_window_after").await else {
            return;
        };
        seed_claim_with_times(
            &pool,
            "2026-06-04T00:00:01+00:00",
            Some("2026-06-03T00:00:00+00:00"),
        )
        .await;

        let out = process_commission(&pool, None, &invoice("evt_after", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::NoAffiliate);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_commissions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn commission_ohne_partnered_at_ist_no_affiliate() {
        let Some(pool) = connect("comm_claim_window_missing_partnered").await else {
            return;
        };
        seed_claim_with_times(&pool, "2026-06-01T00:00:00+00:00", None).await;

        let out = process_commission(&pool, None, &invoice("evt_missing", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::NoAffiliate);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_commissions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn floor_rounding_thirty_percent() {
        let Some(pool) = connect("comm_floor").await else {
            return;
        };
        seed_claim(&pool).await;
        // 999 Cent → 30 % = 299.7 → Floor 299.
        process_commission(&pool, None, &invoice("evt_f", "cus_1", 999))
            .await
            .unwrap();
        // commission_cents ist INTEGER (INT4) → i32.
        let commission: i32 = sqlx::query_scalar(
            "SELECT commission_cents FROM affiliate_commissions WHERE stripe_event_id = 'evt_f'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(commission, 299);
    }

    #[tokio::test]
    async fn zero_or_negative_amount_skipped() {
        let Some(pool) = connect("comm_zero").await else {
            return;
        };
        seed_claim(&pool).await;
        assert_eq!(
            process_commission(&pool, None, &invoice("e0", "cus_1", 0))
                .await
                .unwrap(),
            CommissionOutcome::Skipped
        );
        // commission floor 0 (1 Cent → 0.3 → 0) ist ebenfalls skipped.
        assert_eq!(
            process_commission(&pool, None, &invoice("e1", "cus_1", 1))
                .await
                .unwrap(),
            CommissionOutcome::Skipped
        );
    }

    #[tokio::test]
    async fn duplicate_event_is_idempotent() {
        let Some(pool) = connect("comm_dup").await else {
            return;
        };
        seed_claim(&pool).await;
        let first = process_commission(&pool, None, &invoice("evt_dup", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(first, CommissionOutcome::Recorded("pending".into()));
        let second = process_commission(&pool, None, &invoice("evt_dup", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(second, CommissionOutcome::Duplicate);
        // Nur eine Zeile.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM affiliate_commissions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn pending_cap_marks_skipped_without_account() {
        let Some(pool) = connect("comm_cap").await else {
            return;
        };
        seed_claim(&pool).await;
        // Bestehende offene Provision knapp unter dem Deckel (5000).
        sqlx::query(
            "INSERT INTO affiliate_commissions \
                (affiliate_twitch_login, streamer_login, stripe_event_id, brutto_cents, commission_cents, status, created_at) \
             VALUES ('aff1', 'streamerx', 'old', 16000, 4800, 'pending', '2026-06-01')",
        ).execute(&pool).await.unwrap();
        // Neue 300 → 4800+300 = 5100 > 5000 → skipped.
        let out = process_commission(&pool, None, &invoice("evt_cap", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::Recorded("skipped".into()));
        let status: String = sqlx::query_scalar(
            "SELECT status FROM affiliate_commissions WHERE stripe_event_id = 'evt_cap'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "skipped");
    }

    #[tokio::test]
    async fn with_account_but_no_client_stays_pending_with_error() {
        let Some(pool) = connect("comm_acct_noclient").await else {
            return;
        };
        seed_claim(&pool).await;
        // Affiliate hat ein Connect-Konto, aber wir übergeben keinen Stripe-Client
        // → Transfer schlägt fehl → bleibt pending mit error_message.
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, stripe_account_id) VALUES ('aff1', 'acct_123')")
            .execute(&pool).await.unwrap();
        let out = process_commission(&pool, None, &invoice("evt_acct", "cus_1", 1000))
            .await
            .unwrap();
        assert_eq!(out, CommissionOutcome::Recorded("pending".into()));
        let (status, err): (String, Option<String>) = sqlx::query_as(
            "SELECT status, error_message FROM affiliate_commissions WHERE stripe_event_id = 'evt_acct'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "pending");
        assert!(err.is_some(), "Transfer-Fehler muss error_message setzen");
    }

    #[tokio::test]
    async fn commission_ledger_is_pending_durable_before_transfer() {
        let Some(pool) = connect("comm_ledger_before_transfer").await else {
            return;
        };
        seed_claim(&pool).await;
        sqlx::query(
            "INSERT INTO affiliate_accounts (twitch_login, stripe_account_id) VALUES ('aff1', 'acct_123')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let (seen_tx, seen_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = start_dedupe_stripe_server(Some((seen_tx, release_rx))).await;
        let client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_api_base(server.base_url.clone());

        let task_pool = pool.clone();
        let task_client = client.clone();
        let task = tokio::spawn(async move {
            process_commission(
                &task_pool,
                Some(&task_client),
                &invoice("evt_ledger", "cus_1", 1000),
            )
            .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(5), seen_rx)
            .await
            .unwrap()
            .unwrap();
        let (status, transfer_id): (String, Option<String>) = sqlx::query_as(
            "SELECT status, stripe_transfer_id FROM affiliate_commissions WHERE stripe_event_id = 'evt_ledger'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "pending");
        assert!(
            transfer_id.is_none(),
            "Transfer darf vor Stripe-Antwort noch nicht markiert sein"
        );

        release_tx.send(()).unwrap();
        let outcome = task.await.unwrap().unwrap();
        assert_eq!(outcome, CommissionOutcome::Recorded("transferred".into()));
        assert_eq!(server.effective_transfers.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn successful_transfer_then_lost_commit_retries_same_idempotency_key() {
        let Some(pool) = connect("comm_transfer_lost_commit").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO affiliate_accounts (twitch_login, stripe_account_id) VALUES ('aff1', 'acct_123')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO affiliate_commissions \
                (affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id, \
                 stripe_customer_id, brutto_cents, commission_cents, currency, status, created_at) \
             VALUES ('aff1', 'streamerx', 'evt_lost_commit', 'in_1', 'cus_1', 1000, 300, 'eur', \
                     'pending', '2026-06-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let server = start_dedupe_stripe_server(None).await;
        let client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_api_base(server.base_url.clone());

        let mut tx = pool.begin().await.unwrap();
        acquire_commission_lock(&mut tx, "aff1").await.unwrap();
        transfer_commission_in_tx(
            &mut tx,
            Some(&client),
            "acct_123",
            1,
            "evt_lost_commit",
            300,
            "eur",
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let (status_after_rollback, transfer_after_rollback): (String, Option<String>) =
            sqlx::query_as(
                "SELECT status, stripe_transfer_id FROM affiliate_commissions WHERE stripe_event_id = 'evt_lost_commit'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status_after_rollback, "pending");
        assert!(transfer_after_rollback.is_none());

        replay_pending_commissions(&pool, Some(&client), "acct_123", "aff1")
            .await
            .unwrap();

        let (status, transfer_id): (String, Option<String>) = sqlx::query_as(
            "SELECT status, stripe_transfer_id FROM affiliate_commissions WHERE stripe_event_id = 'evt_lost_commit'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "transferred");
        assert_eq!(transfer_id.as_deref(), Some("tr_effective_1"));
        assert_eq!(
            server.requests.load(Ordering::SeqCst),
            2,
            "Retry muss Stripe erneut mit demselben Key fragen"
        );
        assert_eq!(
            server.effective_transfers.load(Ordering::SeqCst),
            1,
            "Stripe-Dedupe darf nur einen effektiven Transfer ausführen"
        );
    }

    #[tokio::test]
    async fn replay_transfer_uses_idempotency_group_and_is_not_repeated() {
        let Some(pool) = connect("comm_transfer_idem").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO affiliate_accounts (twitch_login, stripe_account_id) VALUES ('aff1', 'acct_123')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO affiliate_commissions \
                (affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id, \
                 stripe_customer_id, brutto_cents, commission_cents, currency, status, created_at) \
             VALUES ('aff1', 'streamerx', 'evt_transfer', 'in_1', 'cus_1', 1000, 300, 'eur', \
                     'pending', '2026-06-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/transfers"))
            .and(header("Idempotency-Key", "affiliate-transfer:1"))
            .and(body_string_contains("amount=300"))
            .and(body_string_contains("currency=eur"))
            .and(body_string_contains("destination=acct_123"))
            .and(body_string_contains("transfer_group=evt_transfer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "tr_123"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_api_base(server.uri());

        replay_pending_commissions(&pool, Some(&client), "acct_123", "aff1")
            .await
            .unwrap();
        replay_pending_commissions(&pool, Some(&client), "acct_123", "aff1")
            .await
            .unwrap();

        let (status, transfer_id): (String, Option<String>) = sqlx::query_as(
            "SELECT status, stripe_transfer_id FROM affiliate_commissions WHERE stripe_event_id = 'evt_transfer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "transferred");
        assert_eq!(transfer_id.as_deref(), Some("tr_123"));
    }

    #[tokio::test]
    async fn connect_account_update_and_replay_share_helper() {
        let Some(pool) = connect("comm_connect_replay").await else {
            return;
        };
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login) VALUES ('aff1')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO affiliate_commissions \
                (affiliate_twitch_login, streamer_login, stripe_event_id, brutto_cents, \
                 commission_cents, currency, status, created_at) \
             VALUES ('aff1', 'streamerx', 'evt_connect', 1000, 300, 'eur', 'pending', \
                     '2026-06-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/transfers"))
            .and(header("Idempotency-Key", "affiliate-transfer:1"))
            .and(body_string_contains("destination=acct_456"))
            .and(body_string_contains("transfer_group=evt_connect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "tr_connect"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_api_base(server.uri());

        connect_account_and_replay(&pool, Some(&client), "Aff1", "acct_456")
            .await
            .unwrap();

        let (account_id, connect_status, status, transfer_id): (
            Option<String>,
            Option<String>,
            String,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT a.stripe_account_id, a.stripe_connect_status, c.status, c.stripe_transfer_id \
             FROM affiliate_accounts a \
             JOIN affiliate_commissions c ON c.affiliate_twitch_login = a.twitch_login \
             WHERE a.twitch_login = 'aff1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(account_id.as_deref(), Some("acct_456"));
        assert_eq!(connect_status.as_deref(), Some("connected"));
        assert_eq!(status, "transferred");
        assert_eq!(transfer_id.as_deref(), Some("tr_connect"));
    }
}

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
//!    einfügen (UNIQUE auf `stripe_event_id` → Replays sind `duplicate`).
//! 5. Hat der Affiliate ein Stripe-Connect-Konto, ausstehende Provisionen per
//!    Stripe-Transfer auszahlen; sonst bleibt der Eintrag `pending` (bzw.
//!    `skipped`, wenn die offene Summe den Sicherheitsdeckel überschreitet).
//!
//! Der Lock-Schlüssel ist bit-identisch zu Pythons `zlib.crc32(login)` (signed
//! i32) im Namespace `1_103_151_689`, sodass Rust- und Python-Pfad während des
//! Cutovers denselben Postgres-Advisory-Lock teilen.

use sqlx::PgPool;

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

/// `true`, wenn der Fehler auf eine UNIQUE-/Duplicate-Verletzung deutet (Python:
/// `"unique" in msg or "duplicate" in msg`).
fn is_duplicate_error(e: &sqlx::Error) -> bool {
    let msg = e.to_string().to_lowercase();
    msg.contains("unique") || msg.contains("duplicate")
}

fn cents_to_int4(value: i64, field: &str) -> Result<i32, sqlx::Error> {
    i32::try_from(value)
        .map_err(|_| sqlx::Error::InvalidArgument(format!("{field} out of int4 range: {value}")))
}

/// Verbucht (und zahlt ggf. aus) die Affiliate-Provision für eine bezahlte
/// Invoice. `stripe` ist optional — ohne Client bleibt eine fällige Auszahlung
/// `pending` (genau wie Pythons Pfad ohne Connect-Konto/Client).
pub async fn process_commission(
    pool: &PgPool,
    stripe: Option<&StripeClient>,
    invoice: &InvoicePayment<'_>,
) -> Result<CommissionOutcome, sqlx::Error> {
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

    // 2. Werbenden Affiliate lösen.
    let affiliate_login: Option<String> = sqlx::query_scalar!(
        r#"
        SELECT affiliate_twitch_login
        FROM affiliate_streamer_claims
        WHERE claimed_streamer_login = $1
        "#,
        &streamer_login
    )
    .fetch_optional(pool)
    .await?;
    let Some(affiliate_login) = affiliate_login.filter(|s| !s.trim().is_empty()) else {
        return Ok(CommissionOutcome::NoAffiliate);
    };

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

    // 4. Unter Advisory-Lock idempotent einfügen.
    let (lock_ns, lock_key) = commission_lock_key(&affiliate_login);
    let mut tx = pool.begin().await?;
    // Transaktions-Advisory-Lock: löst beim Commit/Rollback automatisch aus
    // (Python nimmt einen Session-Lock + explizites Unlock — selber Schlüssel,
    // damit Python- und Rust-Pfad sich gegenseitig serialisieren).
    sqlx::query!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock($1, $2)) AS _lock"#,
        lock_ns,
        lock_key
    )
    .fetch_one(&mut *tx)
    .await?;

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

    let insert = sqlx::query!(
        r#"
        INSERT INTO affiliate_commissions
            (affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id,
             stripe_customer_id, brutto_cents, commission_cents, currency,
             status, period_start, period_end, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
        &affiliate_login,
        &streamer_login,
        invoice.stripe_event_id,
        invoice.invoice_id,
        invoice.stripe_customer_id,
        brutto_cents_i32,
        commission_cents_i32,
        invoice.currency,
        initial_status,
        invoice.period_start,
        invoice.period_end,
        &now
    )
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert {
        if is_duplicate_error(&e) {
            // Lock löst beim Drop/Rollback aus.
            return Ok(CommissionOutcome::Duplicate);
        }
        return Err(e);
    }
    tx.commit().await?;

    // 5. Ohne Connect-Konto bleibt es beim erfassten Status.
    if stripe_account_id.is_empty() {
        return Ok(CommissionOutcome::Recorded(initial_status.to_string()));
    }

    // Auszahlung: alle offenen Provisionen des Affiliates per Stripe-Transfer.
    replay_pending_commissions(pool, stripe, &stripe_account_id, &affiliate_login).await?;

    // Finaler Status der gerade verbuchten Provision.
    let status: Option<String> = sqlx::query_scalar!(
        "SELECT status FROM affiliate_commissions WHERE stripe_event_id = $1",
        invoice.stripe_event_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(CommissionOutcome::Recorded(
        status.unwrap_or_else(|| "pending".to_string()),
    ))
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
async fn replay_pending_commissions(
    pool: &PgPool,
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
    .fetch_all(pool)
    .await?;

    for row in pending {
        transfer_commission(
            pool,
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
async fn transfer_commission(
    pool: &PgPool,
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
            sqlx::query!(
                r#"
                UPDATE affiliate_commissions
                SET status = 'transferred',
                    stripe_transfer_id = $1,
                    transferred_at = $2,
                    error_message = NULL
                WHERE id = $3
                "#,
                transfer_id,
                &transferred_at,
                commission_id
            )
            .execute(pool)
            .await?;
        }
        Err(error) => {
            tracing::warn!(%error, commission_id, "Affiliate Stripe-Transfer fehlgeschlagen");
            let mut msg = error.to_string();
            msg.truncate(500);
            sqlx::query!(
                r#"
                UPDATE affiliate_commissions
                SET status = 'pending',
                    stripe_transfer_id = NULL,
                    transferred_at = NULL,
                    error_message = $1
                WHERE id = $2
                "#,
                &msg,
                commission_id
            )
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

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
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, stripe_account_id TEXT)",
        ).execute(&pool).await.unwrap();
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

    async fn seed_claim(pool: &PgPool) {
        // customer_reference trägt den Streamer-Login (gemischte Groß-/Kleinschreibung
        // → der Code normalisiert per LOWER/TRIM auf 'streamerx').
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, stripe_customer_id, customer_reference, updated_at) VALUES ('sub_1', 'cus_1', 'StreamerX', '2026-06-01')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_streamer_login, claimed_at) VALUES ('aff1', 'streamerx', '2026-06-01')")
            .execute(pool).await.unwrap();
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
}

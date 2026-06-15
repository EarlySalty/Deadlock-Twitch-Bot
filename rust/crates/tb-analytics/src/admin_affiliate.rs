//! Read-Only-Datenschicht für die Admin-Affiliate-Übersichten.
//!
//! Port von `bot/analytics/admin_affiliate_queries.py`. Aggregiert das
//! Affiliate-Programm (Konten, Claims, Provisionen, Gutschriften) für das
//! Admin-Dashboard. Alle Timestamp-Spalten sind in Prod TEXT (ISO-Strings),
//! Geldbeträge in Cent (INTEGER).
//!
//! Quellen-Status: stats portiert; list/detail/gutschriften folgen als Teil 2+.

use serde_json::{json, Value};
use sqlx::PgPool;

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
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, is_active INTEGER NOT NULL DEFAULT 1)",
            "CREATE TABLE affiliate_streamer_claims (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, commission_cents INTEGER, status TEXT, created_at TEXT)",
            "CREATE TABLE affiliate_gutschriften (id BIGSERIAL PRIMARY KEY, affiliate_twitch_login TEXT, gross_amount_cents INTEGER, pdf_generated_at TEXT, email_sent_at TEXT)",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
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

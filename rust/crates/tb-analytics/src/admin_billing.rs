//! Read-Only-Datenschicht für die Admin-Billing-Übersichten.
//!
//! Port von `bot/analytics/admin_config_queries.py:load_admin_billing_subscriptions`
//! + `load_admin_billing_affiliates`. Reine SELECTs über
//!   `twitch_billing_subscriptions` (+ `streamer_plans`-Join) bzw.
//!   `affiliate_accounts`; alle Timestamp-Spalten sind in Prod TEXT (ISO-Strings).

use serde_json::{json, Value};
use sqlx::PgPool;

/// `{ items: [...], count }` der Stripe-Abos + manueller Plan-Overrides.
pub async fn load_billing_subscriptions(pool: &PgPool) -> Result<Value, sqlx::Error> {
    // Spaltennamen/-reihenfolge wie Python; current_period_start/canceled_at/
    // ended_at werden dort SELECTed, aber NICHT in die Antwort übernommen → hier
    // weggelassen.
    let rows = sqlx::query!(
        r#"
        SELECT b.customer_reference, b.plan_id, b.status, b.current_period_end, b.updated_at,
               sp.manual_plan_id, sp.manual_plan_expires_at
        FROM twitch_billing_subscriptions b
        LEFT JOIN streamer_plans sp ON LOWER(sp.twitch_login) = LOWER(b.customer_reference)
        ORDER BY b.updated_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            // login = customer_reference.strip().lower() or None.
            let login = row
                .customer_reference
                .as_deref()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty());
            json!({
                "login": login,
                "customerReference": row.customer_reference,
                "planId": row.plan_id,
                "status": row.status,
                "trialEndsAt": Value::Null,
                "currentPeriodEnd": row.current_period_end,
                "updatedAt": row.updated_at,
                "manualPlanId": row.manual_plan_id,
                "manualPlanExpiresAt": row.manual_plan_expires_at,
            })
        })
        .collect();

    let count = items.len();
    Ok(json!({ "items": items, "count": count }))
}

/// `{ items: [...], count }` der Affiliate-Konten.
pub async fn load_billing_affiliates(pool: &PgPool) -> Result<Value, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT twitch_login, email, stripe_account_id, stripe_connect_status, updated_at, created_at
        FROM affiliate_accounts
        ORDER BY COALESCE(updated_at, created_at) DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            // updatedAt = updated_at OR created_at (Pythons Truthiness: leer → Fallback).
            let updated = if row.updated_at.is_empty() {
                row.created_at
            } else {
                row.updated_at
            };
            json!({
                "twitchLogin": row.twitch_login,
                "stripeAccountId": row.stripe_account_id,
                "status": row.stripe_connect_status,
                "payoutEmail": row.email,
                // affiliate_accounts hat keine commission_rate-Spalte (Python: None).
                "commissionRate": Value::Null,
                "updatedAt": updated,
            })
        })
        .collect();

    let count = items.len();
    Ok(json!({ "items": items, "count": count }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
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
        for ddl in [
            "CREATE TABLE twitch_billing_subscriptions (stripe_subscription_id TEXT PRIMARY KEY, customer_reference TEXT, plan_id TEXT, status TEXT, current_period_end TEXT, updated_at TEXT)",
            "CREATE TABLE streamer_plans (twitch_login TEXT PRIMARY KEY, manual_plan_id TEXT, manual_plan_expires_at TEXT)",
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, email TEXT, stripe_account_id TEXT, stripe_connect_status TEXT, updated_at TEXT, created_at TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn subscriptions_join_und_login_lower() {
        let Some(pool) = make_pool("t_admbill_subs").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, customer_reference, plan_id, status, current_period_end, updated_at) VALUES ('sub_1', 'Nani', 'raid_plus', 'active', '2026-07-01T00:00:00+00:00', '2026-06-01T00:00:00+00:00')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO streamer_plans (twitch_login, manual_plan_id, manual_plan_expires_at) VALUES ('nani', 'raid_extended', '2026-12-31T00:00:00+00:00')")
            .execute(&pool).await.unwrap();

        let v = load_billing_subscriptions(&pool).await.unwrap();
        assert_eq!(v["count"], 1);
        let item = &v["items"][0];
        assert_eq!(item["login"], "nani"); // lowercased
        assert_eq!(item["customerReference"], "Nani"); // roh
        assert_eq!(item["planId"], "raid_plus");
        assert_eq!(item["status"], "active");
        assert!(item["trialEndsAt"].is_null());
        assert_eq!(item["currentPeriodEnd"], "2026-07-01T00:00:00+00:00");
        assert_eq!(item["manualPlanId"], "raid_extended"); // aus Join
    }

    #[tokio::test]
    async fn affiliates_updated_fallback_created() {
        let Some(pool) = make_pool("t_admbill_aff").await else {
            return;
        };
        // updated_at leer → Fallback created_at.
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, email, stripe_account_id, stripe_connect_status, updated_at, created_at) VALUES ('nani', 'a@b.de', 'acct_1', 'active', '', '2026-05-01T00:00:00+00:00')")
            .execute(&pool).await.unwrap();

        let v = load_billing_affiliates(&pool).await.unwrap();
        assert_eq!(v["count"], 1);
        let item = &v["items"][0];
        assert_eq!(item["twitchLogin"], "nani");
        assert_eq!(item["stripeAccountId"], "acct_1");
        assert_eq!(item["status"], "active");
        assert_eq!(item["payoutEmail"], "a@b.de");
        assert!(item["commissionRate"].is_null());
        assert_eq!(item["updatedAt"], "2026-05-01T00:00:00+00:00"); // created_at-Fallback
    }

    #[tokio::test]
    async fn leere_tabellen_count_null() {
        let Some(pool) = make_pool("t_admbill_empty").await else {
            return;
        };
        assert_eq!(load_billing_subscriptions(&pool).await.unwrap()["count"], 0);
        assert_eq!(load_billing_affiliates(&pool).await.unwrap()["count"], 0);
    }
}

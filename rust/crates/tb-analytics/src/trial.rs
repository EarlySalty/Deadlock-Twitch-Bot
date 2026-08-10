//! Analytics-Trial (14 Tage, zweimal pro Streamer).
//!
//! Port von `bot/dashboard/billing/billing_mixin.py:_billing_start_trial_for_user`
//! (Self-Claim) plus der Onboarding-Variante („Mitbringsel" für neue Partner).
//!
//! Modell: Der Trial ist ein manueller Plan `analytics_trial` mit
//! `manual_plan_expires_at = jetzt + 14 Tage` in `streamer_plans`. Der Plan
//! trägt das konsolidierte `analytics`-Entitlement (siehe [`crate::plan`]); das
//! vorhandene Ablauf-Gate in `tb_dashboard_api::auth` sperrt nach Ablauf
//! automatisch.
//!
//! Pricing-Umbau 2026-08-09: aus „30 Tage einmalig" wird „14 Tage automatisch
//! beim ersten Login, danach einmalig weitere 14 Tage auf Wunsch". Gezählt wird
//! in der Spalte `trials_granted`; der alte Boolean `trial_ever_granted` bleibt
//! bestehen und wird weiterhin mitgeschrieben, damit Altleser
//! (`tb_db::rows::StreamerPlanRow`, Python) nicht brechen. Gelesen wird immer
//! `GREATEST(trials_granted, trial_ever_granted)` — so zählt eine Zeile, die
//! nur den Boolean trägt, als eine verbrauchte Einlösung, auch ohne Backfill.

use chrono::{Duration, Utc};
use sqlx::PgPool;

/// Plan-ID des Trials (Python `catalog.ANALYTICS_TRIAL_PLAN_ID`).
pub const ANALYTICS_TRIAL_PLAN_ID: &str = "analytics_trial";
/// Trial-Dauer in Tagen. Zweite Stelle: `trial_period_days` in der
/// Stripe-Checkout-Session (`tb_dashboard_api::handlers::billing_page`).
pub const TRIAL_DURATION_DAYS: i64 = 14;
/// Wie oft ein Streamer den Trial insgesamt bekommen kann: einmal automatisch
/// beim ersten Login, danach genau eine weitere Einlösung. Der dritte Versuch
/// wird abgelehnt.
pub const MAX_TRIAL_GRANTS: i32 = 2;

/// Bezahlpläne, die einen Self-Claim-Trial ausschließen
/// (Python `_billing_start_trial_for_user`, `all_paid_plan_ids`).
/// `premium` ist der Plan aus dem Pricing-Umbau 2026-08-09; ohne ihn würde ein
/// zahlender Kunde seinen Plan per Trial-Knopf gegen `analytics_trial` tauschen.
const PAID_PLAN_IDS: &[&str] = &[
    "premium",
    "chat_quiet",
    "raid_boost",
    "analysis_dashboard",
    "bundle_chat_quiet_raid_boost",
    "bundle_werbefrei_analyse",
    "bundle_komplett",
    "bundle_analysis_raid_boost",
];

/// Ergebnis eines Trial-Grant-Versuchs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialOutcome {
    /// Trial wurde frisch gewährt.
    Granted,
    /// Der Streamer hat alle Einlösungen verbraucht
    /// (`GREATEST(trials_granted, trial_ever_granted) >= MAX_TRIAL_GRANTS`).
    AlreadyUsed,
    /// Es liegt bereits ein bezahlter Plan vor (Billing-Abo oder manuell).
    HasPaidPlan,
    /// DB-Fehler.
    Error,
}

impl TrialOutcome {
    /// Stabiler Status-String für die HTTP-Antwort (Python-Parität).
    pub fn as_str(self) -> &'static str {
        match self {
            TrialOutcome::Granted => "granted",
            TrialOutcome::AlreadyUsed => "already_used",
            TrialOutcome::HasPaidPlan => "has_paid_plan",
            TrialOutcome::Error => "error",
        }
    }
}

/// Self-Claim: löst eine Trial-Periode ein (Port von
/// `_billing_start_trial_for_user`). Das ist seit dem Pricing-Umbau der Weg für
/// die *zweiten* 14 Tage; die ersten kommen automatisch beim ersten Login.
pub async fn start_trial_for_user(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> TrialOutcome {
    grant_trial(
        pool,
        twitch_user_id,
        twitch_login,
        "14-day trial started by user",
    )
    .await
}

/// Onboarding-„Mitbringsel": gewährt neuen Partnern automatisch die ersten
/// 14 Tage. Idempotent über den Zähler (kein Doppel-Grant), überschreibt
/// keinen bestehenden Bezahlplan. Das Ergebnis wird nur geloggt.
pub async fn grant_trial_at_onboarding(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> TrialOutcome {
    let outcome = grant_trial(
        pool,
        twitch_user_id,
        twitch_login,
        "Trial-Mitbringsel beim Partner-Onboarding",
    )
    .await;
    tracing::info!(
        login = %twitch_login,
        outcome = outcome.as_str(),
        "Onboarding-Trial geprüft"
    );
    outcome
}

async fn grant_trial(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
    notes: &str,
) -> TrialOutcome {
    match grant_trial_inner(pool, twitch_user_id, twitch_login, notes).await {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::error!(%error, login = %twitch_login, "Trial-Grant fehlgeschlagen");
            TrialOutcome::Error
        }
    }
}

async fn grant_trial_inner(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
    notes: &str,
) -> Result<TrialOutcome, sqlx::Error> {
    // Aktives, bezahltes Billing-Abo? Tolerant: die Stripe-Tabelle ist evtl.
    // Python-only — fehlt sie, gilt „kein Abo" (kein Abbruch des Trials).
    let paid_billing = has_active_paid_billing_sub(pool, twitch_user_id, twitch_login).await;

    let mut tx = pool.begin().await?;

    // Zähler: wie viele Trials sind schon verbraucht? Eine Bestandszeile, die
    // nur den alten Boolean trägt, zählt als eine Einlösung.
    let trial_row = sqlx::query_scalar!(
        r#"SELECT GREATEST(COALESCE(trials_granted, 0), COALESCE(trial_ever_granted, 0))
                  AS "used!"
           FROM streamer_plans
           WHERE TRIM(COALESCE(twitch_user_id,'')) = $1
              OR LOWER(COALESCE(twitch_login,'')) = LOWER($2)
           LIMIT 1"#,
        twitch_user_id,
        twitch_login
    )
    .fetch_optional(&mut *tx)
    .await?;
    let already_granted = trial_row.unwrap_or(0) >= MAX_TRIAL_GRANTS;

    // Manueller Bezahlplan gesetzt?
    let manual_row = sqlx::query_scalar!(
        r#"SELECT manual_plan_id FROM streamer_plans
           WHERE TRIM(COALESCE(twitch_user_id,'')) = $1
              OR LOWER(COALESCE(twitch_login,'')) = LOWER($2)
           LIMIT 1"#,
        twitch_user_id,
        twitch_login
    )
    .fetch_optional(&mut *tx)
    .await?;
    let manual_paid = manual_row
        .flatten()
        .map(|p| PAID_PLAN_IDS.contains(&p.trim()))
        .unwrap_or(false);

    let outcome = if already_granted {
        TrialOutcome::AlreadyUsed
    } else if paid_billing || manual_paid {
        TrialOutcome::HasPaidPlan
    } else {
        let now = Utc::now();
        let now_iso = now.to_rfc3339();
        let expires_iso = (now + Duration::days(TRIAL_DURATION_DAYS)).to_rfc3339();
        sqlx::query!(
            r#"INSERT INTO streamer_plans
                   (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at,
                    trial_ever_granted, trials_granted, manual_plan_notes, manual_plan_updated_at)
               VALUES ($1, $2, $3, $4, 1, 1, $5, $6)
               ON CONFLICT (twitch_user_id) DO UPDATE SET
                   manual_plan_id = EXCLUDED.manual_plan_id,
                   manual_plan_expires_at = EXCLUDED.manual_plan_expires_at,
                   trial_ever_granted = 1,
                   trials_granted = GREATEST(
                       COALESCE(streamer_plans.trials_granted, 0),
                       COALESCE(streamer_plans.trial_ever_granted, 0)
                   ) + 1,
                   manual_plan_notes = EXCLUDED.manual_plan_notes,
                   manual_plan_updated_at = EXCLUDED.manual_plan_updated_at"#,
            twitch_user_id,
            twitch_login,
            ANALYTICS_TRIAL_PLAN_ID,
            &expires_iso,
            notes,
            &now_iso
        )
        .execute(&mut *tx)
        .await?;
        TrialOutcome::Granted
    };

    tx.commit().await?;
    Ok(outcome)
}

/// Prüft tolerant, ob ein aktives/„trialing" Billing-Abo mit Bezahlplan existiert.
/// Fehlt die Tabelle (Python-only), wird `false` angenommen.
async fn has_active_paid_billing_sub(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> bool {
    has_active_paid_billing_sub_in(pool, twitch_user_id, twitch_login, PAID_PLAN_IDS).await
}

/// Wie [`has_active_paid_billing_sub`], aber mit konfigurierbarer Bezahlplan-
/// Menge — der 24h-Auto-Grant prüft eine kleinere Menge als der Self-Claim.
async fn has_active_paid_billing_sub_in(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
    allowed: &[&str],
) -> bool {
    let user_id = twitch_user_id.trim();
    let login = twitch_login.trim();
    let row = sqlx::query_scalar!(
        r#"SELECT plan_id FROM twitch_billing_subscriptions
           WHERE customer_reference IS NOT NULL
             AND TRIM(customer_reference) <> ''
             AND (($1 <> '' AND LOWER(customer_reference) = LOWER($1))
                  OR ($2 <> '' AND LOWER(customer_reference) = LOWER($2)))
             AND status IN ('active','trialing')
           LIMIT 1"#,
        login,
        user_id
    )
    .fetch_optional(pool)
    .await;

    match row {
        Ok(Some(Some(plan))) => allowed.contains(&plan.trim()),
        _ => false,
    }
}

/// Grace-Periode (Stunden) vor dem Auto-Grant. Pricing-Umbau 2026-08-09: der
/// Trial startet „automatisch beim ersten Login", also ohne Wartezeit. Vorher
/// waren es 24 Stunden (Python `grace_period_hours = 24`). Der Knopf bleibt
/// stehen, falls der Start doch wieder verzögert werden soll.
const TRIAL_GRACE_PERIOD_HOURS: f64 = 0.0;

/// Bezahlpläne, die den Auto-Grant ausschließen — Python-Auto-Grant
/// `paid_plan_ids` (kleinere Menge als der Self-Claim). `premium` kommt aus dem
/// Pricing-Umbau: ohne ihn würde ein zahlender Kunde auf `analytics_trial`
/// zurückgesetzt.
const AUTO_GRANT_PAID_PLAN_IDS: &[&str] = &[
    "premium",
    "raid_boost",
    "analysis_dashboard",
    "bundle_analysis_raid_boost",
];

/// Auto-Grant der ersten 14 Tage beim ersten Login (Python
/// `_billing_check_and_grant_trial_eligibility`). Wird aus der Plan-Resolution
/// für authentifizierte User aufgerufen. Grantet NUR wenn: noch gar kein Trial
/// verbraucht (`GREATEST(trials_granted, trial_ever_granted) = 0`),
/// `first_login_at` gesetzt und alt genug (siehe [`TRIAL_GRACE_PERIOD_HOURS`]),
/// KEIN aktives bezahltes Billing-Abo und KEIN manueller Bezahlplan
/// (≠ `free`/`raid_free`). Die zweite Einlösung passiert bewusst NICHT
/// automatisch, sondern nur über [`start_trial_for_user`]. Idempotent über den
/// Zähler; Fehler werden geschluckt (Python try/except). `true` = frisch gewährt.
pub async fn check_and_grant_trial_eligibility(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> bool {
    match check_and_grant_inner(pool, twitch_user_id, twitch_login).await {
        Ok(granted) => granted,
        Err(error) => {
            tracing::debug!(%error, login = %twitch_login, "Trial-Auto-Grant fehlgeschlagen");
            false
        }
    }
}

async fn check_and_grant_inner(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<bool, sqlx::Error> {
    // Billing-Abo tolerant VOR der Transaktion prüfen (Tabelle evtl. Python-only).
    let paid_billing = has_active_paid_billing_sub_in(
        pool,
        twitch_user_id,
        twitch_login,
        AUTO_GRANT_PAID_PLAN_IDS,
    )
    .await;

    let mut tx = pool.begin().await?;
    // Flag + manueller Plan + Stunden seit first_login_at in einer Query.
    // first_login_at::timestamptz toleriert ISO/Date-only; NULL/unparsebar → NULL.
    let row = sqlx::query!(
        r#"SELECT
               GREATEST(COALESCE(trials_granted, 0), COALESCE(trial_ever_granted, 0)) AS "used!",
               manual_plan_id,
               (EXTRACT(EPOCH FROM (NOW() - first_login_at::timestamptz)) / 3600.0)::float8 AS hours_since
           FROM streamer_plans
           WHERE TRIM(COALESCE(twitch_user_id,'')) = $1
              OR LOWER(COALESCE(twitch_login,'')) = LOWER($2)
           LIMIT 1"#,
        twitch_user_id,
        twitch_login
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        return Ok(false); // kein streamer_plans-Eintrag → kein first_login → nein
    };
    // Schon eine Einlösung verbraucht → der Automatismus ist durch. Die zweiten
    // 14 Tage holt sich der Streamer selbst (`start_trial_for_user`).
    if row.used > 0 {
        return Ok(false);
    }
    // first_login_at fehlt/unparsebar oder zu frisch → kein Grant.
    let Some(hours) = row.hours_since else {
        return Ok(false);
    };
    if hours < TRIAL_GRACE_PERIOD_HOURS {
        return Ok(false);
    }
    // Manueller Bezahlplan (≠ Free) → kein Grant. Seit dem Pricing-Umbau
    // 2026-08-09 heisst der Gratis-Plan `free`; `raid_free` bleibt fuer
    // Bestandszeilen gueltig.
    if let Some(mp) = row.manual_plan_id.as_deref().map(str::trim) {
        if !mp.is_empty() && mp != "raid_free" && mp != "free" {
            return Ok(false);
        }
    }
    if paid_billing {
        return Ok(false);
    }

    // Grant — UPSERT mit Zähler (Boolean wird mitgeschrieben, siehe Modulkopf).
    let now = Utc::now();
    let now_iso = now.to_rfc3339();
    let expires_iso = (now + Duration::days(TRIAL_DURATION_DAYS)).to_rfc3339();
    sqlx::query!(
        r#"INSERT INTO streamer_plans
               (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at,
                trial_ever_granted, trials_granted, manual_plan_notes, manual_plan_updated_at)
           VALUES ($1, $2, $3, $4, 1, 1, $5, $6)
           ON CONFLICT (twitch_user_id) DO UPDATE SET
               manual_plan_id = EXCLUDED.manual_plan_id,
               manual_plan_expires_at = EXCLUDED.manual_plan_expires_at,
               trial_ever_granted = 1,
               trials_granted = GREATEST(
                   COALESCE(streamer_plans.trials_granted, 0),
                   COALESCE(streamer_plans.trial_ever_granted, 0)
               ) + 1,
               manual_plan_notes = EXCLUDED.manual_plan_notes,
               manual_plan_updated_at = EXCLUDED.manual_plan_updated_at"#,
        twitch_user_id,
        twitch_login,
        ANALYTICS_TRIAL_PLAN_ID,
        &expires_iso,
        "Trial automatisch beim ersten Login",
        &now_iso
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod auto_grant_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            // Prod speichert die Zeitfelder als TEXT (Python schreibt ISO-Strings;
            // der bestehende grant_trial_inner bindet ebenfalls Strings).
            r#"CREATE TABLE streamer_plans (
                   twitch_user_id TEXT PRIMARY KEY,
                   twitch_login TEXT,
                   manual_plan_id TEXT,
                   manual_plan_expires_at TEXT,
                   trial_ever_granted INTEGER DEFAULT 0,
                   trials_granted INTEGER NOT NULL DEFAULT 0,
                   manual_plan_notes TEXT,
                   manual_plan_updated_at TEXT,
                   first_login_at TEXT
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_billing_subscriptions (
                   customer_reference TEXT,
                   plan_id TEXT,
                   status TEXT
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    async fn flag_and_plan(pool: &PgPool, user_id: &str) -> (i32, Option<String>) {
        sqlx::query_as::<_, (Option<i32>, Option<String>)>(
            "SELECT trial_ever_granted, manual_plan_id FROM streamer_plans WHERE twitch_user_id = $1",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map(|(f, p)| (f.unwrap_or(0), p))
        .unwrap()
    }

    /// Verbrauchte Einlösungen laut Zähler.
    async fn trials_used(pool: &PgPool, user_id: &str) -> i32 {
        sqlx::query_scalar::<_, i32>(
            "SELECT trials_granted FROM streamer_plans WHERE twitch_user_id = $1",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// Verbleibende Tage bis `manual_plan_expires_at`, gerundet.
    async fn tage_bis_ablauf(pool: &PgPool, user_id: &str) -> i64 {
        let expires: String = sqlx::query_scalar(
            "SELECT manual_plan_expires_at FROM streamer_plans WHERE twitch_user_id = $1",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(&expires).unwrap();
        (parsed.with_timezone(&Utc) - Utc::now()).num_hours() / 24
    }

    #[tokio::test]
    async fn erster_trial_kommt_automatisch_beim_ersten_login() {
        let Some(pool) = pool_or_skip("t6e_trial_grant").await else {
            return;
        };
        // Login gerade eben, kein Zähler, kein Plan → Grant ohne Wartezeit.
        // Vor dem Pricing-Umbau brauchte es 24 h Grace.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) VALUES ('10','streamer', NOW()::text)")
            .execute(&pool).await.unwrap();
        assert!(check_and_grant_trial_eligibility(&pool, "10", "streamer").await);
        let (flag, plan) = flag_and_plan(&pool, "10").await;
        assert_eq!(flag, 1, "der alte Boolean wird weiter mitgeschrieben");
        assert_eq!(plan.as_deref(), Some(ANALYTICS_TRIAL_PLAN_ID));
        assert_eq!(trials_used(&pool, "10").await, 1);
        assert_eq!(
            tage_bis_ablauf(&pool, "10").await,
            13,
            "14 Tage minus ein paar Sekunden Laufzeit"
        );
        // Idempotent: der Automatismus grantet nicht erneut, auch nicht die
        // zweiten 14 Tage — die holt sich der Streamer selbst.
        assert!(!check_and_grant_trial_eligibility(&pool, "10", "streamer").await);
        assert_eq!(trials_used(&pool, "10").await, 1);
    }

    #[tokio::test]
    async fn zweiter_trial_ist_einloesbar() {
        let Some(pool) = pool_or_skip("t6e_trial_zweiter").await else {
            return;
        };
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) VALUES ('11','streamer', NOW()::text)")
            .execute(&pool).await.unwrap();
        assert!(check_and_grant_trial_eligibility(&pool, "11", "streamer").await);
        // Erster Trial abgelaufen, Plan steht wieder auf free.
        sqlx::query("UPDATE streamer_plans SET manual_plan_id = 'free', manual_plan_expires_at = NULL WHERE twitch_user_id = '11'")
            .execute(&pool).await.unwrap();

        let outcome = start_trial_for_user(&pool, "11", "streamer").await;
        assert_eq!(outcome, TrialOutcome::Granted);
        assert_eq!(trials_used(&pool, "11").await, 2);
        let (_, plan) = flag_and_plan(&pool, "11").await;
        assert_eq!(plan.as_deref(), Some(ANALYTICS_TRIAL_PLAN_ID));
        assert_eq!(tage_bis_ablauf(&pool, "11").await, 13);
    }

    #[tokio::test]
    async fn dritter_trial_versuch_wird_abgelehnt() {
        let Some(pool) = pool_or_skip("t6e_trial_dritter").await else {
            return;
        };
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) VALUES ('12','streamer', NOW()::text)")
            .execute(&pool).await.unwrap();
        assert!(check_and_grant_trial_eligibility(&pool, "12", "streamer").await);
        assert_eq!(
            start_trial_for_user(&pool, "12", "streamer").await,
            TrialOutcome::Granted
        );
        let ablauf_nach_zwei: Option<String> = sqlx::query_scalar(
            "SELECT manual_plan_expires_at FROM streamer_plans WHERE twitch_user_id = '12'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Dritter Versuch: abgelehnt, und die Zeile bleibt unangetastet.
        assert_eq!(
            start_trial_for_user(&pool, "12", "streamer").await,
            TrialOutcome::AlreadyUsed
        );
        assert_eq!(trials_used(&pool, "12").await, 2);
        let ablauf_nach_drei: Option<String> = sqlx::query_scalar(
            "SELECT manual_plan_expires_at FROM streamer_plans WHERE twitch_user_id = '12'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ablauf_nach_zwei, ablauf_nach_drei);
        // Auch der Automatismus greift nicht mehr.
        assert!(!check_and_grant_trial_eligibility(&pool, "12", "streamer").await);
    }

    #[tokio::test]
    async fn bestandsnutzer_mit_altem_boolean_hat_genau_eine_einloesung() {
        let Some(pool) = pool_or_skip("t6e_trial_bestand").await else {
            return;
        };
        // (a) Backfill gelaufen: Boolean 1, Zähler 1.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, trial_ever_granted, trials_granted, first_login_at) VALUES ('13','backfill','free',1,1, (NOW() - INTERVAL '90 days')::text)")
            .execute(&pool).await.unwrap();
        assert_eq!(
            start_trial_for_user(&pool, "13", "backfill").await,
            TrialOutcome::Granted
        );
        assert_eq!(trials_used(&pool, "13").await, 2);
        assert_eq!(
            start_trial_for_user(&pool, "13", "backfill").await,
            TrialOutcome::AlreadyUsed
        );

        // (b) Zeile ohne Backfill: nur der Boolean steht. GREATEST rechnet sie
        // trotzdem als eine verbrauchte Einlösung — sonst bekäme sie drei.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, trial_ever_granted, trials_granted, first_login_at) VALUES ('14','ohnebackfill','free',1,0, (NOW() - INTERVAL '90 days')::text)")
            .execute(&pool).await.unwrap();
        assert!(
            !check_and_grant_trial_eligibility(&pool, "14", "ohnebackfill").await,
            "der Automatismus ist fuer diese Zeile durch"
        );
        assert_eq!(
            start_trial_for_user(&pool, "14", "ohnebackfill").await,
            TrialOutcome::Granted
        );
        assert_eq!(trials_used(&pool, "14").await, 2);
        assert_eq!(
            start_trial_for_user(&pool, "14", "ohnebackfill").await,
            TrialOutcome::AlreadyUsed
        );
    }

    #[tokio::test]
    async fn kein_trial_fuer_premium_kunden() {
        let Some(pool) = pool_or_skip("t6e_trial_premium").await else {
            return;
        };
        // Manueller Premium-Plan: weder Selbstbedienung noch Automatismus
        // duerfen ihn gegen analytics_trial tauschen.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, first_login_at) VALUES ('15','zahler','premium', NOW()::text)")
            .execute(&pool).await.unwrap();
        assert_eq!(
            start_trial_for_user(&pool, "15", "zahler").await,
            TrialOutcome::HasPaidPlan
        );
        assert!(!check_and_grant_trial_eligibility(&pool, "15", "zahler").await);
        let (_, plan) = flag_and_plan(&pool, "15").await;
        assert_eq!(plan.as_deref(), Some("premium"));

        // Aktives Premium-Abo bei Stripe, keine manuelle Zeile.
        sqlx::query("INSERT INTO twitch_billing_subscriptions (customer_reference, plan_id, status) VALUES ('abonnent', 'premium', 'active')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) VALUES ('16','abonnent', NOW()::text)")
            .execute(&pool).await.unwrap();
        assert_eq!(
            start_trial_for_user(&pool, "16", "abonnent").await,
            TrialOutcome::HasPaidPlan
        );
        assert!(!check_and_grant_trial_eligibility(&pool, "16", "abonnent").await);
        assert_eq!(trials_used(&pool, "16").await, 0);
    }

    #[tokio::test]
    async fn self_claim_blockt_login_referenziertes_paid_abo() {
        let Some(pool) = pool_or_skip("t6e_trial_paid_login").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_billing_subscriptions (customer_reference, plan_id, status) \
             VALUES ('streamer', 'analysis_dashboard', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let outcome = start_trial_for_user(&pool, "42", "streamer").await;
        assert_eq!(outcome, TrialOutcome::HasPaidPlan);
        let rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM streamer_plans WHERE twitch_user_id = '42'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            rows, 0,
            "Paid-Abo via Login darf keinen Trial-Grant erzeugen"
        );
    }

    #[tokio::test]
    async fn leere_billing_referenz_matcht_keinen_leeren_login() {
        let Some(pool) = pool_or_skip("t6e_trial_empty_paid_ref").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_billing_subscriptions (customer_reference, plan_id, status) \
             VALUES (NULL, 'analysis_dashboard', 'active'), ('', 'analysis_dashboard', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(!has_active_paid_billing_sub(&pool, "77", "").await);
        let outcome = start_trial_for_user(&pool, "77", "").await;
        assert_eq!(outcome, TrialOutcome::Granted);
        let (flag, plan) = flag_and_plan(&pool, "77").await;
        assert_eq!(flag, 1);
        assert_eq!(plan.as_deref(), Some(ANALYTICS_TRIAL_PLAN_ID));
    }

    #[tokio::test]
    async fn kein_grant_ohne_first_login() {
        let Some(pool) = pool_or_skip("t6e_trial_nograce").await else {
            return;
        };
        // Gegenprobe zur abgeschafften Grace-Periode: 5h alter Login grantet
        // jetzt sofort, frueher war er zu frisch.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) VALUES ('20','frisch', (NOW() - INTERVAL '5 hours')::text)")
            .execute(&pool).await.unwrap();
        assert!(check_and_grant_trial_eligibility(&pool, "20", "frisch").await);
        // kein first_login_at → kein Grant.
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login) VALUES ('21','ohnelogin')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(!check_and_grant_trial_eligibility(&pool, "21", "ohnelogin").await);
        // gar kein Eintrag → kein Grant.
        assert!(!check_and_grant_trial_eligibility(&pool, "99", "unbekannt").await);
    }

    #[tokio::test]
    async fn kein_grant_bei_manuellem_bezahlplan() {
        let Some(pool) = pool_or_skip("t6e_trial_paid").await else {
            return;
        };
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, first_login_at) VALUES ('30','zahler','raid_boost', (NOW() - INTERVAL '48 hours')::text)")
            .execute(&pool).await.unwrap();
        assert!(!check_and_grant_trial_eligibility(&pool, "30", "zahler").await);
        // raid_free blockt NICHT (zählt als gratis).
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, first_login_at) VALUES ('31','gratis','raid_free', (NOW() - INTERVAL '48 hours')::text)")
            .execute(&pool).await.unwrap();
        assert!(check_and_grant_trial_eligibility(&pool, "31", "gratis").await);
        // `free` ebenfalls nicht (der Nachfolger von raid_free).
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, first_login_at) VALUES ('32','freeplan','free', (NOW() - INTERVAL '48 hours')::text)")
            .execute(&pool).await.unwrap();
        assert!(check_and_grant_trial_eligibility(&pool, "32", "freeplan").await);
    }
}

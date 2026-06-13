//! Analytics-Trial (30 Tage, einmalig pro Streamer).
//!
//! Port von `bot/dashboard/billing/billing_mixin.py:_billing_start_trial_for_user`
//! (Self-Claim) plus der Onboarding-Variante („Mitbringsel" für neue Partner).
//!
//! Modell (unverändert zu Python): Der Trial ist ein manueller Plan
//! `analytics_trial` mit `manual_plan_expires_at = jetzt + 30 Tage` in
//! `streamer_plans`. Der unveränderliche Flag `trial_ever_granted` (INTEGER 0/1)
//! garantiert „genau einmal pro Streamer" — Self-Claim und Onboarding-Grant
//! teilen sich denselben Flag. Der Plan `analytics_trial` trägt das
//! `analytics.extended`-Entitlement (siehe [`crate::plan`]); das vorhandene
//! Ablauf-Gate in `tb_dashboard_api::auth` sperrt nach 30 Tagen automatisch.

use chrono::{Duration, Utc};
use sqlx::PgPool;

/// Plan-ID des Trials (Python `catalog.ANALYTICS_TRIAL_PLAN_ID`).
pub const ANALYTICS_TRIAL_PLAN_ID: &str = "analytics_trial";
/// Trial-Dauer in Tagen (Python `catalog.TRIAL_DURATION_DAYS`).
pub const TRIAL_DURATION_DAYS: i64 = 30;

/// Bezahlpläne, die einen Self-Claim-Trial ausschließen
/// (Python `_billing_start_trial_for_user`, `all_paid_plan_ids`).
const PAID_PLAN_IDS: &[&str] = &[
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
    /// Der Streamer hatte schon einmal einen Trial (`trial_ever_granted = 1`).
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

/// Self-Claim: startet den 30-Tage-Trial für einen Streamer (Port von
/// `_billing_start_trial_for_user`).
pub async fn start_trial_for_user(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> TrialOutcome {
    grant_trial(
        pool,
        twitch_user_id,
        twitch_login,
        "30-day trial started by user",
    )
    .await
}

/// Onboarding-„Mitbringsel": gewährt neuen Partnern automatisch den einmaligen
/// Trial. Idempotent über `trial_ever_granted` (kein Doppel-Grant), überschreibt
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
    let paid_billing = has_active_paid_billing_sub(pool, twitch_user_id).await;

    let mut tx = pool.begin().await?;

    // Einmal-Flag: bereits ein Trial gewährt?
    let trial_row: Option<(Option<i32>,)> = sqlx::query_as(
        r#"SELECT trial_ever_granted FROM streamer_plans
           WHERE TRIM(COALESCE(twitch_user_id,'')) = $1
              OR LOWER(COALESCE(twitch_login,'')) = LOWER($2)
           LIMIT 1"#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(&mut *tx)
    .await?;
    let already_granted = trial_row.and_then(|r| r.0).unwrap_or(0) == 1;

    // Manueller Bezahlplan gesetzt?
    let manual_row: Option<(Option<String>,)> = sqlx::query_as(
        r#"SELECT manual_plan_id FROM streamer_plans
           WHERE TRIM(COALESCE(twitch_user_id,'')) = $1
              OR LOWER(COALESCE(twitch_login,'')) = LOWER($2)
           LIMIT 1"#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(&mut *tx)
    .await?;
    let manual_paid = manual_row
        .and_then(|r| r.0)
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
        sqlx::query(
            r#"INSERT INTO streamer_plans
                   (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at,
                    trial_ever_granted, manual_plan_notes, manual_plan_updated_at)
               VALUES ($1, $2, $3, $4, 1, $5, $6)
               ON CONFLICT (twitch_user_id) DO UPDATE SET
                   manual_plan_id = EXCLUDED.manual_plan_id,
                   manual_plan_expires_at = EXCLUDED.manual_plan_expires_at,
                   trial_ever_granted = 1,
                   manual_plan_notes = EXCLUDED.manual_plan_notes,
                   manual_plan_updated_at = EXCLUDED.manual_plan_updated_at"#,
        )
        .bind(twitch_user_id)
        .bind(twitch_login)
        .bind(ANALYTICS_TRIAL_PLAN_ID)
        .bind(&expires_iso)
        .bind(notes)
        .bind(&now_iso)
        .execute(&mut *tx)
        .await?;
        TrialOutcome::Granted
    };

    tx.commit().await?;
    Ok(outcome)
}

/// Prüft tolerant, ob ein aktives/„trialing" Billing-Abo mit Bezahlplan existiert.
/// Fehlt die Tabelle (Python-only), wird `false` angenommen.
async fn has_active_paid_billing_sub(pool: &PgPool, twitch_user_id: &str) -> bool {
    let row: Result<Option<(Option<String>,)>, _> = sqlx::query_as(
        r#"SELECT plan_id FROM twitch_billing_subscriptions
           WHERE LOWER(customer_reference) = LOWER($1)
             AND status IN ('active','trialing')
           LIMIT 1"#,
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await;

    match row {
        Ok(Some((Some(plan),))) => PAID_PLAN_IDS.contains(&plan.trim()),
        _ => false,
    }
}

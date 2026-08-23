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
//! konsolidierte `analytics`-Entitlement (siehe [`crate::plan`]); das vorhandene
//! Ablauf-Gate in `tb_dashboard_api::auth` sperrt nach 30 Tagen automatisch.

use chrono::{Duration, Utc};
use sqlx::PgPool;

/// Plan-ID des Trials (Python `catalog.ANALYTICS_TRIAL_PLAN_ID`).
pub const ANALYTICS_TRIAL_PLAN_ID: &str = "analytics_trial";
/// Trial-Dauer in Tagen (Python `catalog.TRIAL_DURATION_DAYS`).
pub const TRIAL_DURATION_DAYS: i64 = 30;

/// Plan-IDs, die kein eigenes Recht darstellen: darauf darf der Trial buchen.
///
/// Leer heisst "keine Zeile", `free`/`raid_free` sind die Gratis-Stufen. Alles
/// andere ist ein Recht, das jemand vergeben hat.
fn ist_gratis_platzhalter(plan_id: &str) -> bool {
    matches!(plan_id.trim(), "" | "free" | "raid_free")
}

/// `true`, wenn hier ein Manual-Recht **ohne Ablaufdatum** steht.
///
/// Sicherung gegen Datenverlust, unabhaengig von jeder Plan-Erkennung: ein
/// Admin-Geschenk auf Dauer wird vom Trial niemals ueberschrieben, auch wenn
/// die Plan-ID unbekannt ist. Der Trial setzt `manual_plan_id` und
/// `manual_plan_expires_at` per UPSERT neu; ein unbefristetes Recht waere danach
/// unwiederbringlich weg. Lieber einen Trial ablehnen als ein Geschenk loeschen.
fn ist_unbefristetes_manual_recht(plan_id: &str, expires_at: Option<&str>) -> bool {
    let id = plan_id.trim();
    if ist_gratis_platzhalter(id) || id == ANALYTICS_TRIAL_PLAN_ID {
        return false;
    }
    expires_at.is_none_or(|wert| wert.trim().is_empty())
}

/// `true`, wenn die Plan-ID einen bezahlten Zugang bezeichnet.
///
/// Kommt aus dem Katalog ([`crate::stufe::ist_bezahlter_plan`]), nicht aus einer
/// Liste in dieser Datei. Vorher standen hier zwei handgepflegte Namenslisten,
/// die `plus` und `pro` nicht kannten. Seit `admin_manual_plan::set_manual_plan`
/// nur noch Katalog-IDs annimmt, ist jedes kuenftige Admin-Geschenk `plus` oder
/// `pro` und waere fuer diese Listen unsichtbar gewesen. Eine neue Stufe im
/// Katalog wird hier automatisch mitgezaehlt.
fn ist_bezahlt(plan_id: &str) -> bool {
    crate::stufe::ist_bezahlter_plan(plan_id)
}

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
    let paid_billing = has_active_paid_billing_sub(pool, twitch_user_id, twitch_login).await;

    let mut tx = pool.begin().await?;

    // Einmal-Flag: bereits ein Trial gewährt?
    let trial_row = sqlx::query_scalar!(
        r#"SELECT trial_ever_granted FROM streamer_plans
           WHERE TRIM(COALESCE(twitch_user_id,'')) = $1
              OR LOWER(COALESCE(twitch_login,'')) = LOWER($2)
           LIMIT 1"#,
        twitch_user_id,
        twitch_login
    )
    .fetch_optional(&mut *tx)
    .await?;
    let already_granted = trial_row.unwrap_or(0) == 1;

    // Manuelles Recht gesetzt? Plan-ID UND Ablauf lesen: die Sicherung gegen
    // das Ueberschreiben eines unbefristeten Geschenks haengt am Ablauf, nicht
    // an der Plan-Erkennung.
    let manual_row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT manual_plan_id, manual_plan_expires_at::text FROM streamer_plans \
           WHERE TRIM(COALESCE(twitch_user_id,'')) = $1 \
              OR LOWER(COALESCE(twitch_login,'')) = LOWER($2) \
           LIMIT 1",
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(&mut *tx)
    .await?;
    let (manual_plan, manual_expires) = manual_row
        .map(|(plan, expires)| (plan.unwrap_or_default(), expires))
        .unwrap_or_default();
    let manual_paid = ist_bezahlt(&manual_plan);
    let manual_unbefristet =
        ist_unbefristetes_manual_recht(&manual_plan, manual_expires.as_deref());

    let outcome = if already_granted {
        TrialOutcome::AlreadyUsed
    } else if paid_billing || manual_paid || manual_unbefristet {
        // `HasPaidPlan` deckt beides ab: bezahlter Zugang und jedes andere
        // unbefristete Manual-Recht. Der Streamer verliert nur den Trial, nicht
        // sein Recht.
        TrialOutcome::HasPaidPlan
    } else {
        let now = Utc::now();
        let now_iso = now.to_rfc3339();
        let expires_iso = (now + Duration::days(TRIAL_DURATION_DAYS)).to_rfc3339();
        sqlx::query!(
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
///
/// Self-Claim und 24h-Auto-Grant fragen dieselbe Menge ab, und die kommt aus dem
/// Katalog. Vorher hatte der Auto-Grant eine eigene, kleinere Liste ohne
/// `plus`/`pro`: ein zahlender Stripe-Kunde ohne Manual-Zeile bekam beim Login
/// den Trial-Override uebergestuelpt und verlor 30 Tage lang, was er bezahlt hat.
async fn has_active_paid_billing_sub(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
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
        Ok(Some(Some(plan))) => ist_bezahlt(&plan),
        _ => false,
    }
}

/// Grace-Periode (Stunden) vor dem 24h-Auto-Grant (Python `grace_period_hours = 24`).
const TRIAL_GRACE_PERIOD_HOURS: f64 = 24.0;

/// Auto-Grant des 30-Tage-Trials nach 24h-Grace (Python
/// `_billing_check_and_grant_trial_eligibility`). Wird aus der Plan-Resolution
/// für authentifizierte User aufgerufen. Grantet NUR wenn: noch nie gewährt
/// (`trial_ever_granted`), `first_login_at` ≥ 24 h her, KEIN aktives bezahltes
/// Billing-Abo (Bezahlplan laut Katalog, siehe [`ist_bezahlt`]) und KEIN
/// gesetztes Manual-Recht ausser den Gratis-Stufen. Idempotent über `trial_ever_granted`;
/// Fehler werden geschluckt (Python try/except). `true` = frisch gewährt.
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
    let paid_billing = has_active_paid_billing_sub(pool, twitch_user_id, twitch_login).await;

    let mut tx = pool.begin().await?;
    // Flag + manueller Plan + Stunden seit first_login_at in einer Query.
    // first_login_at::timestamptz toleriert ISO/Date-only; NULL/unparsebar → NULL.
    let row = sqlx::query!(
        r#"SELECT
               trial_ever_granted,
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
    if row.trial_ever_granted == 1 {
        return Ok(false);
    }
    // first_login_at fehlt/unparsebar oder < 24 h her → kein Grant.
    let Some(hours) = row.hours_since else {
        return Ok(false);
    };
    if hours < TRIAL_GRACE_PERIOD_HOURS {
        return Ok(false);
    }
    // Jedes gesetzte Manual-Recht ausser den Gratis-Stufen blockt den Grant:
    // der UPSERT unten wuerde es ueberschreiben.
    if !ist_gratis_platzhalter(row.manual_plan_id.as_deref().unwrap_or("")) {
        return Ok(false);
    }
    if paid_billing {
        return Ok(false);
    }

    // Grant — UPSERT mit Einmal-Flag (Python-Notiz beibehalten).
    let now = Utc::now();
    let now_iso = now.to_rfc3339();
    let expires_iso = (now + Duration::days(TRIAL_DURATION_DAYS)).to_rfc3339();
    sqlx::query!(
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
        twitch_user_id,
        twitch_login,
        ANALYTICS_TRIAL_PLAN_ID,
        &expires_iso,
        "Trial granted after 24h grace period",
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

    #[tokio::test]
    async fn grant_nach_24h_grace() {
        let Some(pool) = pool_or_skip("t6e_trial_grant").await else {
            return;
        };
        // first_login 30h her, kein Flag, kein Plan → Grant.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) VALUES ('10','streamer', (NOW() - INTERVAL '30 hours')::text)")
            .execute(&pool).await.unwrap();
        assert!(check_and_grant_trial_eligibility(&pool, "10", "streamer").await);
        let (flag, plan) = flag_and_plan(&pool, "10").await;
        assert_eq!(flag, 1);
        assert_eq!(plan.as_deref(), Some(ANALYTICS_TRIAL_PLAN_ID));
        // Idempotent: zweiter Aufruf grantet nicht erneut.
        assert!(!check_and_grant_trial_eligibility(&pool, "10", "streamer").await);
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
    async fn kein_grant_innerhalb_grace_oder_ohne_first_login() {
        let Some(pool) = pool_or_skip("t6e_trial_nograce").await else {
            return;
        };
        // first_login erst 5h her → noch nicht eligible.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) VALUES ('20','frisch', (NOW() - INTERVAL '5 hours')::text)")
            .execute(&pool).await.unwrap();
        assert!(!check_and_grant_trial_eligibility(&pool, "20", "frisch").await);
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

    /// Ein unbefristetes Admin-Geschenk auf einer Katalog-Stufe darf der
    /// Self-Claim-Trial nicht ueberschreiben. Vorher kannte die Bezahl-Liste
    /// `plus`/`pro` nicht, der UPSERT hat das Geschenk geloescht.
    #[tokio::test]
    async fn trial_ueberschreibt_kein_unbefristetes_geschenk() {
        let Some(pool) = pool_or_skip("t6e_trial_geschenk").await else {
            return;
        };
        for (user, login, plan) in [
            ("40", "plusgeschenk", "plus"),
            ("41", "progeschenk", "pro"),
            // Auch eine ID, die der Katalog gar nicht kennt: ohne Ablauf ist es
            // ein Recht, das jemand vergeben hat.
            ("42", "unbekannt", "geschenk_2027"),
        ] {
            sqlx::query(
                "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id) \
                 VALUES ($1, $2, $3)",
            )
            .bind(user)
            .bind(login)
            .bind(plan)
            .execute(&pool)
            .await
            .unwrap();

            let outcome = start_trial_for_user(&pool, user, login).await;
            assert_eq!(
                outcome,
                TrialOutcome::HasPaidPlan,
                "{plan}: Trial haette abgelehnt werden muessen"
            );
            let (flag, gespeichert) = flag_and_plan(&pool, user).await;
            assert_eq!(gespeichert.as_deref(), Some(plan), "{plan} wurde geloescht");
            assert_eq!(flag, 0, "{plan}: Einmal-Flag darf nicht verbraucht werden");
            let expires: Option<String> = sqlx::query_scalar(
                "SELECT manual_plan_expires_at FROM streamer_plans WHERE twitch_user_id = $1",
            )
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(expires.is_none(), "{plan}: Ablauf wurde gesetzt");
        }
    }

    /// Gegenprobe zur Sicherung: was kein dauerhaftes Recht ist, blockt auch
    /// nicht. Sonst haette die Sicherung den Trial fuer alle zugemacht.
    #[tokio::test]
    async fn trial_laeuft_bei_gratis_und_befristetem_unbekannten_recht() {
        let Some(pool) = pool_or_skip("t6e_trial_gegenprobe").await else {
            return;
        };
        // Gratis-Stufen sind kein Recht: der Trial darf drueber.
        for (user, login, plan) in [
            ("50", "gratis_alt", Some("raid_free")),
            ("51", "gratis_neu", Some("free")),
            ("52", "leer", None),
        ] {
            sqlx::query(
                "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id) \
                 VALUES ($1, $2, $3)",
            )
            .bind(user)
            .bind(login)
            .bind(plan)
            .execute(&pool)
            .await
            .unwrap();
            assert_eq!(
                start_trial_for_user(&pool, user, login).await,
                TrialOutcome::Granted,
                "{plan:?}: Trial haette laufen muessen"
            );
            let (_, gespeichert) = flag_and_plan(&pool, user).await;
            assert_eq!(gespeichert.as_deref(), Some(ANALYTICS_TRIAL_PLAN_ID));
        }

        // Unbekannte ID MIT Ablauf ist kein dauerhaftes Recht und nicht bezahlt.
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at) \
             VALUES ('53', 'befristet', 'aktion_maerz', (NOW() + INTERVAL '10 days')::text)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            start_trial_for_user(&pool, "53", "befristet").await,
            TrialOutcome::Granted
        );
    }

    /// Ein laufendes Stripe-Abo auf einer Katalog-Stufe schliesst den
    /// Auto-Grant aus. Vorher kannte die Auto-Grant-Liste nur Alt-IDs, ein
    /// `pro`-Kunde bekam beim Login den Trial-Override und verlor 30 Tage lang
    /// `social.auto_post`.
    #[tokio::test]
    async fn auto_grant_weicht_bezahltem_abo_aus() {
        let Some(pool) = pool_or_skip("t6e_trial_stripe_katalog").await else {
            return;
        };
        for (user, login, plan, erwartet_grant) in [
            ("60", "prokunde", "pro", false),
            ("61", "pluskunde", "plus", false),
            // Gegenprobe: gratis oder unbekannt blockt nicht.
            ("62", "freikunde", "free", true),
            ("63", "krams", "kein_plan", true),
        ] {
            sqlx::query(
                "INSERT INTO twitch_billing_subscriptions (customer_reference, plan_id, status) \
                 VALUES ($1, $2, 'active')",
            )
            .bind(login)
            .bind(plan)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at) \
                 VALUES ($1, $2, (NOW() - INTERVAL '48 hours')::text)",
            )
            .bind(user)
            .bind(login)
            .execute(&pool)
            .await
            .unwrap();
            assert_eq!(
                check_and_grant_trial_eligibility(&pool, user, login).await,
                erwartet_grant,
                "{plan}: Auto-Grant-Entscheidung falsch"
            );
        }
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
    }
}

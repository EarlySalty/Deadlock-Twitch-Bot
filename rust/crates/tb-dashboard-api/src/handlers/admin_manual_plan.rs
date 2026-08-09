//! Admin Manual-Plan-Override (B2-P1-admin-manual-plan, Block 2B/16).
//!
//! Port von `bot/dashboard/live/live.py:admin_manual_plan_save/clear` +
//! `bot/dashboard/billing/billing_mixin.py:_billing_admin_set_manual_plan` /
//! `_billing_admin_clear_manual_plan` (Zeilen 1412-1543).
//!
//! Zwei Form-POST-Routen, die der Admin aus der Streamer-Detail-Ansicht aufruft:
//! - `POST /twitch/admin/manual-plan`        — Override setzen (login, plan_id,
//!   expires_at, notes)
//! - `POST /twitch/admin/manual-plan/clear`  — Override entfernen (login)
//!
//! **Vertrag (Python-Parität):** Der Legacy-Admin-Client (`submitLegacyAction`,
//! `client.ts`) sendet `application/x-www-form-urlencoded` inkl. `csrf_token` im
//! Body und folgt dem Redirect; er liest den `?ok=`/`?err=`-Query der finalen URL.
//! Daher antworten beide Handler mit `302` auf `/twitch/admin?ok=…` bzw. `?err=…`
//! (statt JSON). Auth: Admin/Localhost; CSRF wird gegen das sessiongebundene
//! Token aus dem Form-Body geprüft (Localhost-Bypass wie überall).
//!
//! Geschrieben wird `streamer_plans` (Spalten `manual_plan_id`,
//! `manual_plan_expires_at`, `manual_plan_notes`, `manual_plan_updated_at`),
//! aufgelöst über die `twitch_user_id` aus der View
//! `twitch_streamers_partner_state`.

use axum::{
    extract::{Extension, RawForm, State},
    response::{IntoResponse, Redirect, Response},
};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::DashboardAuthState;
use crate::handlers::legacy_form::validate_form_csrf;

/// Maximale Notizlänge (Python `notes_value[:1000]`).
const MAX_NOTES_LEN: usize = 1000;

/// Ziel-Pfad für alle Redirects (Python `default_path="/twitch/admin"`).
const ADMIN_PATH: &str = "/twitch/admin";

/// `POST /twitch/admin/manual-plan` — manuellen Plan-Override setzen.
pub async fn save_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form).await {
        return resp;
    }

    let login = form_get(&form, "login").trim().to_string();
    let plan_id = form_get(&form, "plan_id").trim().to_string();
    let expires_at = form_get(&form, "expires_at").trim().to_string();
    let notes = form_get(&form, "notes").trim().to_string();

    match set_manual_plan(&pool, &login, &plan_id, &expires_at, &notes).await {
        Ok(effective_plan_id) => redirect_ok(&format!(
            "Manueller Plan für {login} gesetzt ({effective_plan_id})"
        )),
        Err(err) => redirect_err(&err.user_message(&login)),
    }
}

/// `POST /twitch/admin/manual-plan/clear` — manuellen Plan-Override entfernen.
pub async fn clear_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form).await {
        return resp;
    }

    let login = form_get(&form, "login").trim().to_string();
    match clear_manual_plan(&pool, &login).await {
        Ok(effective_plan_id) => redirect_ok(&format!(
            "Manueller Override für {login} entfernt ({effective_plan_id})"
        )),
        Err(err) => redirect_err(&err.user_message(&login)),
    }
}

// ── Auth + CSRF-Gate ─────────────────────────────────────────────────────────

/// Admin-Auth (Localhost/Admin) + CSRF aus dem Form-Body. Gibt `Some(redirect)`
/// zurück, wenn der Request abzulehnen ist, sonst `None`.
async fn gate(
    auth: &DashboardAuthLevel,
    config: Option<&Extension<DashboardAuthState>>,
    headers: &axum::http::HeaderMap,
    form: &[(String, String)],
) -> Option<Response> {
    if !auth.is_privileged() {
        return Some(redirect_err("Nicht autorisiert."));
    }
    // Admin (Cookie-Session): CSRF-Token aus dem Form-Body gegen die Session prüfen.
    let presented = form_get(form, "csrf_token").trim().to_string();
    let Some(Extension(state)) = config else {
        // Kein Auth-State → kein Validierungspfad → fail-closed.
        return Some(redirect_err("CSRF-Prüfung nicht verfügbar."));
    };
    match validate_form_csrf(state, headers, &presented).await {
        Some(true) => None,
        Some(false) => Some(redirect_err("Ungültiges CSRF-Token.")),
        None => Some(redirect_err("Sitzung fehlt.")),
    }
}

// ── DB-Logik ─────────────────────────────────────────────────────────────────

/// Fehlerfälle (Python `ValueError`-Codes → Nutzer-Texte).
#[derive(Debug)]
enum ManualPlanError {
    LoginRequired,
    UnknownPlanId,
    UnknownStreamer,
    UserIdMissing,
    SaveFailed,
}

impl ManualPlanError {
    fn user_message(&self, login: &str) -> String {
        let login_label = if login.is_empty() { "—" } else { login };
        match self {
            Self::LoginRequired => "Bitte einen Twitch-Login angeben".to_string(),
            Self::UnknownPlanId => "Unbekannte Plan-ID".to_string(),
            Self::UnknownStreamer => format!("Streamer {login_label} nicht gefunden"),
            Self::UserIdMissing => format!("Für {login_label} fehlt die Twitch User-ID"),
            Self::SaveFailed => "Manueller Plan konnte nicht gespeichert werden".to_string(),
        }
    }
}

/// Setzt den manuellen Plan-Override; gibt die effektive Plan-ID zurück.
async fn set_manual_plan(
    pool: &PgPool,
    login: &str,
    plan_id: &str,
    expires_at: &str,
    notes: &str,
) -> Result<String, ManualPlanError> {
    let normalized_login = login.trim().to_lowercase();
    if normalized_login.is_empty() {
        return Err(ManualPlanError::LoginRequired);
    }
    // Python `_billing_normalize_plan_id`: nur Plan-IDs aus dem Billing-Katalog
    // (`_BILLING_PLANS`) sind gültig. `find_plan` liefert die kanonische
    // `&'static str`-ID (exakter Match, kein Lowercasing — Python-Parität).
    let normalized_plan_id = tb_analytics::billing::find_plan(plan_id.trim())
        .map(|plan| plan.id)
        .ok_or(ManualPlanError::UnknownPlanId)?;
    let expires_at_iso = parse_datetime_value(expires_at);
    let notes_value: String = notes.trim().chars().take(MAX_NOTES_LEN).collect();
    let updated_at_iso = now_iso();

    let (twitch_user_id, canonical_login) = resolve_streamer(pool, &normalized_login).await?;

    // Upsert-Identität (Python INSERT … ON CONFLICT) + Override-Update.
    sqlx::query!(
        r#"
        INSERT INTO streamer_plans (twitch_user_id, twitch_login)
        VALUES ($1, $2)
        ON CONFLICT (twitch_user_id) DO UPDATE SET twitch_login = EXCLUDED.twitch_login
        "#,
        &twitch_user_id,
        &canonical_login
    )
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("manual_plan upsert identity Fehler: {e}");
        ManualPlanError::SaveFailed
    })?;

    sqlx::query!(
        r#"
        UPDATE streamer_plans
        SET twitch_login = $1,
            manual_plan_id = $2,
            manual_plan_expires_at = $3,
            manual_plan_notes = $4,
            manual_plan_updated_at = $5
        WHERE twitch_user_id = $6
        "#,
        &canonical_login,
        normalized_plan_id,
        expires_at_iso.as_deref(),
        &notes_value,
        &updated_at_iso,
        &twitch_user_id
    )
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("manual_plan update Fehler: {e}");
        ManualPlanError::SaveFailed
    })?;

    // P2.129: Partner-Raid-Score sofort neu berechnen (raid_boost-Tier ist
    // plan-abhängig). Best-effort wie Pythons fire-and-forget Hook
    // (_billing_refresh_partner_raid_score_cache): Fehler werden geloggt, nicht
    // an den Admin-Response durchgereicht.
    refresh_partner_raid_score(pool, &canonical_login).await;

    Ok(effective_plan_id(pool, &canonical_login, &twitch_user_id).await)
}

/// Entfernt den manuellen Plan-Override; gibt die effektive Plan-ID zurück.
async fn clear_manual_plan(pool: &PgPool, login: &str) -> Result<String, ManualPlanError> {
    let normalized_login = login.trim().to_lowercase();
    if normalized_login.is_empty() {
        return Err(ManualPlanError::LoginRequired);
    }
    let updated_at_iso = now_iso();
    let (twitch_user_id, canonical_login) = resolve_streamer(pool, &normalized_login).await?;

    sqlx::query!(
        r#"
        UPDATE streamer_plans
        SET manual_plan_id = NULL,
            manual_plan_expires_at = NULL,
            manual_plan_notes = '',
            manual_plan_updated_at = $1
        WHERE twitch_user_id = $2
        "#,
        &updated_at_iso,
        &twitch_user_id
    )
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("manual_plan clear Fehler: {e}");
        ManualPlanError::SaveFailed
    })?;

    // P2.129: Score-Refresh auch nach dem Entfernen (entfernter Boost darf den
    // Score nicht weiter aufblähen).
    refresh_partner_raid_score(pool, &canonical_login).await;

    Ok(effective_plan_id(pool, &canonical_login, &twitch_user_id).await)
}

/// Best-effort Partner-Raid-Score-Refresh nach einer Plan-Änderung (P2.129).
///
/// Delegiert an [`tb_analytics::stripe::refresh_partner_raid_score_for_login`]
/// (gemeinsame Funktion mit dem Webhook-Pfad P2.127/P2.128). Fehler werden nur
/// geloggt — der Admin-Response hängt nicht davon ab.
async fn refresh_partner_raid_score(pool: &PgPool, login: &str) {
    if let Err(err) = tb_analytics::stripe::refresh_partner_raid_score_for_login(pool, login).await
    {
        tracing::warn!("manual_plan raid-score refresh fehlgeschlagen für {login}: {err}");
    }
}

/// Löst `(twitch_user_id, canonical_login)` aus `twitch_streamers_partner_state`.
async fn resolve_streamer(
    pool: &PgPool,
    normalized_login: &str,
) -> Result<(String, String), ManualPlanError> {
    let row = sqlx::query!(
        r#"
        SELECT twitch_user_id, twitch_login
        FROM twitch_streamers_partner_state
        WHERE LOWER(twitch_login) = LOWER($1)
        LIMIT 1
        "#,
        normalized_login
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("manual_plan streamer lookup Fehler: {e}");
        ManualPlanError::SaveFailed
    })?;

    let Some(row) = row else {
        return Err(ManualPlanError::UnknownStreamer);
    };
    let twitch_user_id = row.twitch_user_id.unwrap_or_default().trim().to_string();
    if twitch_user_id.is_empty() {
        return Err(ManualPlanError::UserIdMissing);
    }
    let canonical_login = row
        .twitch_login
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| normalized_login.to_string());
    Ok((twitch_user_id, canonical_login))
}

/// Effektive Plan-ID nach dem Schreiben (Python liest sie aus den Plan-Rows). Wir
/// nutzen den kanonischen Resolver; bei Fehler `free` (Gratis-Plan seit 2026-08-09).
async fn effective_plan_id(pool: &PgPool, login: &str, user_id: &str) -> String {
    match tb_analytics::plan::resolve_plan_snapshot(pool, login, user_id).await {
        Ok(snapshot) => snapshot.plan_id.to_string(),
        Err(_) => "free".to_string(),
    }
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

/// Parst `application/x-www-form-urlencoded` in Key/Value-Paare.
fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn form_get<'a>(form: &'a [(String, String)], key: &str) -> &'a str {
    form.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

/// ISO-8601-UTC-Zeitstempel (Python `datetime.now(UTC).isoformat()`).
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

/// Parst einen Datums-/Zeit-Eingabewert auf einen ISO-UTC-String, sonst `None`.
///
/// Port von `entitlements/repository.py:parse_datetime_value`: ein reines
/// `YYYY-MM-DD` wird auf `…T23:59:59+00:00` (Tagesende UTC) gehoben; `Z` →
/// `+00:00`; ungültig/leer → `None`.
fn parse_datetime_value(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    // Reines Datum (YYYY-MM-DD) → Tagesende UTC.
    let bytes = text.as_bytes();
    let date_only = text.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-';
    let candidate = if date_only {
        format!("{text}T23:59:59+00:00")
    } else {
        text.replace('Z', "+00:00")
    };
    chrono::DateTime::parse_from_rfc3339(&candidate)
        .ok()
        .map(|dt| {
            dt.with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
        })
}

fn redirect_to(query_key: &str, message: &str) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(message.as_bytes()).collect();
    Redirect::to(&format!("{ADMIN_PATH}?{query_key}={encoded}")).into_response()
}

fn redirect_ok(message: &str) -> Response {
    redirect_to("ok", message)
}

fn redirect_err(message: &str) -> Response {
    redirect_to("err", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_datetime_date_only_wird_tagesende() {
        let iso = parse_datetime_value("2026-12-31").unwrap();
        assert!(iso.starts_with("2026-12-31T23:59:59"));
        assert!(iso.ends_with("+00:00"));
    }

    #[test]
    fn parse_datetime_full_und_z_normalisiert() {
        let iso = parse_datetime_value("2026-06-15T10:00:00Z").unwrap();
        assert!(iso.starts_with("2026-06-15T10:00:00"));
        assert_eq!(parse_datetime_value(""), None);
        assert_eq!(parse_datetime_value("kaputt"), None);
    }

    #[test]
    fn form_parse_und_get() {
        let form = parse_form(b"login=Nani&plan_id=raid_boost&notes=hi%20there");
        assert_eq!(form_get(&form, "login"), "Nani");
        assert_eq!(form_get(&form, "plan_id"), "raid_boost");
        assert_eq!(form_get(&form, "notes"), "hi there");
        assert_eq!(form_get(&form, "fehlt"), "");
    }

    #[test]
    fn redirect_kodiert_message() {
        let resp = redirect_ok("Plan für nani gesetzt (raid_boost)");
        assert_eq!(resp.status(), axum::http::StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/admin?ok="));
        assert!(loc.contains("raid_boost"));
    }

    #[test]
    fn error_messages_parität() {
        assert_eq!(
            ManualPlanError::UnknownStreamer.user_message("nani"),
            "Streamer nani nicht gefunden"
        );
        assert_eq!(
            ManualPlanError::LoginRequired.user_message(""),
            "Bitte einen Twitch-Login angeben"
        );
        assert_eq!(
            ManualPlanError::UnknownPlanId.user_message(""),
            "Unbekannte Plan-ID"
        );
    }

    // ── DB-Logik (env-gated über TB_TEST_DATABASE_URL) ──────────────────────
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
        // Minimal-Schema: View-Ersatz als Tabelle (Lookup-Quelle) + streamer_plans
        // + twitch_billing_subscriptions (resolve_plan_snapshot liest beide).
        for ddl in [
            "CREATE TABLE twitch_streamers_partner_state (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, \
                 manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT DEFAULT '', \
                 manual_plan_updated_at TEXT)",
            "CREATE TABLE twitch_billing_subscriptions (stripe_subscription_id TEXT PRIMARY KEY, \
                 customer_reference TEXT, status TEXT, plan_id TEXT, updated_at TEXT, \
                 current_period_end TEXT, cancel_at_period_end INTEGER DEFAULT 0)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_user_id, twitch_login) VALUES ('42', 'NaniStream')")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn set_dann_clear_schreibt_streamer_plans() {
        let Some(pool) = make_pool("t_manplan").await else {
            return;
        };

        // SET: gültiger bezahlter Plan + Ablaufdatum.
        let plan = set_manual_plan(
            &pool,
            "nanistream",
            "premium",
            "2026-12-31",
            "VIP",
        )
        .await
        .expect("set ok");
        // Effektiver Plan = der manuell gesetzte (aktiv, nicht abgelaufen).
        assert_eq!(plan, "premium");

        let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT manual_plan_id, manual_plan_notes, manual_plan_expires_at FROM streamer_plans WHERE twitch_user_id='42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("premium"));
        assert_eq!(row.1.as_deref(), Some("VIP"));
        assert!(row.2.unwrap().starts_with("2026-12-31T23:59:59"));

        // CLEAR: Override entfernen → effektiver Plan fällt auf free.
        let plan = clear_manual_plan(&pool, "nanistream")
            .await
            .expect("clear ok");
        assert_eq!(plan, "free");
        let cleared: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT manual_plan_id, manual_plan_notes FROM streamer_plans WHERE twitch_user_id='42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(cleared.0.is_none());
        assert_eq!(cleared.1.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn set_unbekannter_streamer_fehler() {
        let Some(pool) = make_pool("t_manplan_unknown").await else {
            return;
        };
        let err = set_manual_plan(&pool, "gibtsnicht", "premium", "", "")
            .await
            .unwrap_err();
        assert!(matches!(err, ManualPlanError::UnknownStreamer));
    }

    #[tokio::test]
    async fn set_unbekannte_plan_id_fehler() {
        let Some(pool) = make_pool("t_manplan_badplan").await else {
            return;
        };
        let err = set_manual_plan(&pool, "nanistream", "Premium_XXL", "", "")
            .await
            .unwrap_err();
        assert!(matches!(err, ManualPlanError::UnknownPlanId));
    }

    /// Pool mit dem VOLLEN Schema, das der Score-Refresher braucht (P2.129).
    /// `search_path` per `after_connect`, damit der vom Refresher genutzte Pool
    /// (derselbe) auf das Test-Schema zeigt.
    async fn make_pool_with_scores(schema: &str) -> Option<PgPool> {
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
        let schema_owned = schema.to_string();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |conn, _| {
                let schema = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await
                        .map(|_| ())
                })
            })
            .connect(&dsn)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_streamers_partner_state (twitch_user_id TEXT, twitch_login TEXT, is_partner_active INTEGER NOT NULL DEFAULT 1)",
            "CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, \
                 manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT DEFAULT '', \
                 manual_plan_updated_at TEXT, raid_boost_enabled INTEGER NOT NULL DEFAULT 0)",
            "CREATE TABLE twitch_billing_subscriptions (stripe_subscription_id TEXT PRIMARY KEY, \
                 customer_reference TEXT, status TEXT, plan_id TEXT, updated_at TEXT, \
                 current_period_end TEXT, cancel_at_period_end INTEGER DEFAULT 0)",
            "CREATE TABLE twitch_stream_sessions (streamer_login TEXT, started_at TIMESTAMPTZ, duration_seconds BIGINT)",
            "CREATE TABLE twitch_raid_history (from_broadcaster_id TEXT, to_broadcaster_id TEXT, executed_at TIMESTAMPTZ, success BOOLEAN)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT, is_live INTEGER, last_started_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_partner_raid_scores ( \
                 twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, avg_duration_sec INTEGER, \
                 time_pattern_score_base DOUBLE PRECISION, received_successful_raids_total INTEGER, \
                 is_new_partner_preferred INTEGER, new_partner_multiplier DOUBLE PRECISION, \
                 raid_boost_multiplier DOUBLE PRECISION, is_live INTEGER, current_started_at TEXT, \
                 current_uptime_sec INTEGER, duration_score DOUBLE PRECISION, time_pattern_score DOUBLE PRECISION, \
                 readiness_score DOUBLE PRECISION, fairness_score DOUBLE PRECISION, base_score DOUBLE PRECISION, \
                 final_score DOUBLE PRECISION, today_received_raids INTEGER, last_computed_at TEXT, \
                 internal_sent_raids_30d INTEGER, internal_received_raids_7d INTEGER, internal_received_raids_30d INTEGER)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_user_id, twitch_login, is_partner_active) VALUES ('77','BoostStreamer',1)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    /// P2.129: Setzt man einen Plan mit Raid-Prio, muss der Partner-Raid-Score sofort
    /// neu berechnet werden (Boost-Multiplikator > 1.0 sofort wirksam).
    #[tokio::test]
    async fn set_manual_plan_refreshes_raid_score() {
        let Some(pool) = make_pool_with_scores("t_manplan_score").await else {
            return;
        };
        // Der Boost greift im Refresher über streamer_plans.raid_boost_enabled.
        // set_manual_plan setzt manual_plan_id; für den Boost-Flag im Refresher
        // setzen wir raid_boost_enabled direkt (Entitlement-Auflösung ist eigene
        // Slice, siehe partner_score_refresh.rs:load_boost_flag).
        let plan = set_manual_plan(&pool, "booststreamer", "premium", "", "")
            .await
            .expect("set ok");
        assert_eq!(plan, "premium");
        sqlx::query("UPDATE streamer_plans SET raid_boost_enabled = 1 WHERE twitch_user_id = '77'")
            .execute(&pool)
            .await
            .unwrap();
        // Refresh erneut auslösen (clear→set würde Score erneut schreiben); wir
        // rufen den Refresh-Pfad direkt über einen zweiten set auf.
        let _ = set_manual_plan(&pool, "booststreamer", "premium", "", "")
            .await
            .unwrap();

        let row: (String, f64) = sqlx::query_as(
            "SELECT last_computed_at, raid_boost_multiplier FROM twitch_partner_raid_scores WHERE twitch_user_id='77'",
        )
        .fetch_one(&pool)
        .await
        .expect("score row geschrieben (Refresh lief)");
        assert!(!row.0.is_empty());
        assert!(row.1 > 1.0, "raid_boost_multiplier > 1.0, war {}", row.1);
    }
}

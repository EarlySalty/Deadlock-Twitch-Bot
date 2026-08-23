//! Native Admin-Streamer-Verwaltung als Legacy-Form-POST-Routen (B-Welle-2-A1).
//!
//! Port der Python-Dashboard-Admin-Aktionen aus `bot/dashboard/live/live.py`,
//! die der Admin-React-Client (`submitLegacyAction`, `admin_dashboard/.../client.ts`)
//! als `application/x-www-form-urlencoded` aufruft:
//!
//! - `POST /twitch/add_streamer` — `add_streamer` (live.py:1728-1788): Streamer
//!   anlegen + optional Discord-Profil setzen (P0.1).
//! - `POST /twitch/add_url`, `POST /twitch/add_login`, `POST /twitch/add_any` —
//!   die Add-Shortcuts (live.py:1680-1726): nur `_do_add`, kein Discord-Profil
//!   (P1.46).
//! - `POST /twitch/remove` — `remove` (live.py:2031-2048): Streamer entfernen /
//!   Partner departnern (P1.46).
//! - `POST /twitch/discord_link` — `discord_link` (live.py:2000-2029): Discord-
//!   Profil (id/display_name/member_flag) für einen Streamer setzen (P2.112/P2.121).
//!
//! **Nativität (statt Strangler-Proxy):** Die DB-Mutationen laufen direkt über
//! die geteilte Domänen-Schicht [`tb_analytics::streamers_crud`] — exakt die
//! Funktionen, die auch die `tb-internal-api`-Handler (`streamers.rs`) nutzen
//! (`add_streamer`/`departner_streamer`/`remove_streamer`/`set_discord_profile`).
//! Die Twitch-User-ID wird (wie Python `_do_add`/`_discord_profile`) best-effort
//! über Helix aufgelöst; ohne App-Credentials läuft der Add ohne ID weiter.
//!
//! **Vertrag (Python-Parität):** `302`-Redirect auf `/twitch/admin?ok=…`/`?err=…`;
//! der Client folgt dem Redirect und liest den Status aus dem Query. Auth:
//! Admin/Localhost; CSRF aus dem Form-Body (Localhost-Bypass) — daher KEIN
//! Header-CSRF-Layer auf diesen Routen.

use axum::{
    extract::{Extension, RawForm, State},
    response::Response,
};
use sqlx::PgPool;
use std::sync::Arc;
use tb_analytics::streamers_crud::{
    add_streamer, departner_streamer, remove_streamer, set_discord_profile, AddStreamerResult,
    RemoveStreamerResult,
};
use tb_domain::login::normalize_twitch_login;
use tb_transport_twitch::{HelixClient, HelixConfig};

use super::legacy_form::{form_get, gate, parse_form, redirect_with};
use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::DashboardAuthState;

/// Ziel-Pfad aller Admin-Streamer-Redirects (Python `default_path="/twitch/admin"`).
const ADMIN_PATH: &str = "/twitch/admin";

fn redirect_ok(message: &str) -> Response {
    redirect_with(ADMIN_PATH, "ok", message)
}

fn redirect_err(message: &str) -> Response {
    redirect_with(ADMIN_PATH, "err", message)
}

// ── POST /twitch/add_streamer (P0.1) ──────────────────────────────────────────

/// `POST /twitch/add_streamer` — Streamer anlegen (+ optional Discord-Profil).
pub async fn add_streamer_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form, redirect_err).await {
        return resp;
    }

    let raw_login = form_get(&form, "login").trim();
    let discord_user_id = form_get(&form, "discord_user_id").trim().to_string();
    let discord_display_name = form_get(&form, "discord_display_name").trim().to_string();
    let mark_member = parse_member_flag(form_get(&form, "member_flag"));

    let Some(login) = normalize_twitch_login(raw_login) else {
        return redirect_err("Bitte einen Twitch-Login angeben");
    };

    let add_message = match do_add(&pool, &login).await {
        Ok(msg) => msg,
        Err(_) => return redirect_err("Twitch-Streamer konnte nicht hinzugefügt werden"),
    };

    // Optional: Discord-Profil setzen (Python `should_update_discord`).
    let should_update_discord =
        !discord_user_id.is_empty() || !discord_display_name.is_empty() || mark_member;
    let profile_message = if should_update_discord {
        match save_discord_profile(
            &pool,
            &login,
            opt(&discord_user_id),
            opt(&discord_display_name),
            mark_member,
        )
        .await
        {
            Ok(msg) => msg,
            Err(DiscordProfileError::Validation(text)) => return redirect_err(&text),
            Err(DiscordProfileError::Db) => {
                return redirect_err("Discord-Daten konnten nicht gespeichert werden")
            }
        }
    } else {
        String::new()
    };

    // Meldungen zusammenführen (Python `" – ".join(dict.fromkeys(...))`).
    let mut messages: Vec<String> = Vec::new();
    for m in [add_message, profile_message] {
        if !m.is_empty() && !messages.contains(&m) {
            messages.push(m);
        }
    }
    let ok_message = if messages.is_empty() {
        "Gespeichert".to_string()
    } else {
        messages.join(" – ")
    };
    redirect_ok(&ok_message)
}

// ── POST /twitch/add_url|add_login|add_any (P1.46) ─────────────────────────────

/// `POST /twitch/add_url` / `add_login` / `add_any` — Add-Shortcut ohne Discord.
///
/// Python liest den Login aus unterschiedlichen Feldern (`url`/`login`/`any`),
/// alle landen über `_do_add` im selben Pfad. Wir akzeptieren alle drei Felder.
pub async fn add_any_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form, redirect_err).await {
        return resp;
    }

    let raw = first_nonempty(&form, &["url", "login", "any", "streamer", "twitch_login"]);
    let Some(login) = normalize_twitch_login(raw.trim()) else {
        return redirect_err("Bitte einen Twitch-Login angeben");
    };

    match do_add(&pool, &login).await {
        Ok(msg) => redirect_ok(&msg),
        Err(_) => redirect_err("Twitch-Streamer konnte nicht hinzugefügt werden"),
    }
}

// ── POST /twitch/remove (P1.46) ───────────────────────────────────────────────

/// `POST /twitch/remove` — aktiven Partner departnern bzw. Streamer löschen.
///
/// Spiegelt den vollen Departner-Lifecycle der `tb-internal-api` (`remove_handler`):
/// zuerst aktiven Partner über [`departner_streamer`] departnern (Status-Wechsel,
/// Raid-Auth-Disable, Identity-Upsert); existiert kein aktiver Partner, fällt es
/// auf [`remove_streamer`] (Archivieren/Löschen + Live-State-Cleanup). Das
/// Discord-Rollen-Removal gehört in Prod in den Master-Broker-Pfad
/// (`tb-bot`/`tb-internal-api`) und ist hier — wie in [`tb_analytics`] dokumentiert
/// — ein bewusster Handoff; die DB-Departnerung ist vollständig.
pub async fn remove_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form, redirect_err).await {
        return resp;
    }

    let raw_login = form_get(&form, "login").trim();
    let Some(login) = normalize_twitch_login(raw_login) else {
        return redirect_err("could not remove");
    };

    match do_remove(&pool, &login).await {
        Ok(msg) => redirect_ok(&msg),
        Err(_) => redirect_err("could not remove"),
    }
}

// ── POST /twitch/discord_link (P2.112/P2.121) ─────────────────────────────────

/// `POST /twitch/discord_link` — Discord-Profil (id/display_name/member) setzen.
pub async fn discord_link_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form, redirect_err).await {
        return resp;
    }

    let raw_login = form_get(&form, "login").trim();
    let discord_user_id = form_get(&form, "discord_user_id").trim().to_string();
    let discord_display_name = form_get(&form, "discord_display_name").trim().to_string();
    let mark_member = parse_member_flag(form_get(&form, "member_flag"));

    let Some(login) = normalize_twitch_login(raw_login) else {
        return redirect_err("Bitte einen Twitch-Login angeben");
    };

    match save_discord_profile(
        &pool,
        &login,
        opt(&discord_user_id),
        opt(&discord_display_name),
        mark_member,
    )
    .await
    {
        Ok(msg) => redirect_ok(&msg),
        Err(DiscordProfileError::Validation(text)) => redirect_err(&text),
        Err(DiscordProfileError::Db) => {
            redirect_err("Discord-Daten konnten nicht gespeichert werden")
        }
    }
}

// ── Domänen-Logik ─────────────────────────────────────────────────────────────

/// Fehler beim Discord-Profil-Write.
#[derive(Debug)]
enum DiscordProfileError {
    /// Eingabe-Validierung (Python `ValueError` → Nutzer-Text).
    Validation(String),
    /// DB-/Lookup-Fehler.
    Db,
}

/// `_do_add`-Äquivalent: legt einen Streamer an, gibt die Erfolgsmeldung zurück.
///
/// Best-effort Helix-Lookup der Twitch-User-ID (Python `_add` → Helix); ohne
/// App-Credentials oder bei Helix-Fehler wird ohne ID angelegt (`add_streamer`
/// upsertet die ID später nach, sobald sie bekannt ist).
async fn do_add(pool: &PgPool, login: &str) -> Result<String, sqlx::Error> {
    let user_id = resolve_user_id(login).await;
    match add_streamer(pool, login, user_id.as_deref()).await? {
        AddStreamerResult::AlreadyExists => Ok(format!("{login} ist bereits aktiv")),
        AddStreamerResult::Added => Ok(format!("{login} hinzugefügt")),
    }
}

/// `_remove`-Äquivalent: departnert aktiven Partner bzw. löscht den Streamer.
async fn do_remove(pool: &PgPool, login: &str) -> Result<String, sqlx::Error> {
    if let Some(outcome) = departner_streamer(pool, login, false).await? {
        return Ok(format!("{} operativ deaktiviert", outcome.twitch_login));
    }
    match remove_streamer(pool, login).await? {
        RemoveStreamerResult::Archived => Ok(format!("{login} archiviert")),
        RemoveStreamerResult::Deleted => Ok(format!("{login} removed")),
        RemoveStreamerResult::NotFound => Ok(format!("{login} removed")),
    }
}

/// `_discord_profile`-Äquivalent: validiert die Eingaben (Python-Vertrag:
/// numerische Discord-ID, 120-Zeichen-Cap für den Display-Name), löst die
/// Twitch-User-ID auf (Raid-Auth → Helix) und schreibt das Profil.
async fn save_discord_profile(
    pool: &PgPool,
    login: &str,
    discord_user_id: Option<&str>,
    discord_display_name: Option<&str>,
    mark_member: bool,
) -> Result<String, DiscordProfileError> {
    // discord_user_id: numerisch erzwingen (Python `isdigit`-Check).
    if let Some(did) = discord_user_id {
        if !did.chars().all(|c| c.is_ascii_digit()) {
            return Err(DiscordProfileError::Validation(
                "Discord-ID muss numerisch sein".to_string(),
            ));
        }
    }
    // display_name auf 120 Zeichen kürzen (Python-Vertrag).
    let display_name_capped: Option<String> = discord_display_name.map(|s| {
        if s.chars().count() > 120 {
            s.chars().take(120).collect()
        } else {
            s.to_string()
        }
    });

    // Twitch-User-ID auflösen: erst aus twitch_raid_auth, sonst über Helix
    // (Python `_dashboard_save_discord_profile`).
    let mut twitch_user_id =
        tb_analytics::streamers_crud::load_twitch_user_id_from_raid_auth(pool, login)
            .await
            .map_err(|e| {
                tracing::error!("discord_link raid-auth lookup Fehler: {e}");
                DiscordProfileError::Db
            })?;
    if twitch_user_id.is_none() {
        twitch_user_id = resolve_user_id(login).await;
    }

    let updated = set_discord_profile(
        pool,
        login,
        discord_user_id,
        display_name_capped.as_deref(),
        mark_member,
        twitch_user_id.as_deref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("discord_link set_discord_profile Fehler: {e}");
        DiscordProfileError::Db
    })?;

    if updated {
        Ok(format!("Discord-Daten für {login} gespeichert"))
    } else {
        Err(DiscordProfileError::Validation(format!(
            "Für {login} fehlt die Twitch User-ID"
        )))
    }
}

// ── Hilfsfunktionen ───────────────────────────────────────────────────────────

/// Löst die Twitch-User-ID best-effort über Helix auf (`None` ohne Credentials
/// oder bei Lookup-Fehler — der Streamer wird dann ohne ID angelegt).
async fn resolve_user_id(login: &str) -> Option<String> {
    let helix = build_helix()?;
    match helix.get_users(&[login]).await {
        Ok(map) => map
            .get(login)
            .map(|u| u.id.clone())
            .filter(|s| !s.is_empty()),
        Err(e) => {
            tracing::warn!("Helix-Lookup für {login} fehlgeschlagen: {e}");
            None
        }
    }
}

/// Baut einen Helix-Client aus den Twitch-App-Credentials (`None`, wenn nicht
/// konfiguriert).
fn build_helix() -> Option<Arc<HelixClient>> {
    let client_id = std::env::var("TWITCH_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    let client_secret = std::env::var("TWITCH_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())?;
    HelixClient::new(HelixConfig::new(client_id, client_secret))
        .ok()
        .map(Arc::new)
}

/// Member-Flag-Parsing (Python `member_raw in {"1","true","on","yes"}`).
fn parse_member_flag(raw: &str) -> bool {
    matches!(
        raw.trim().to_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// Leerer String → `None`.
fn opt(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Erster nicht-leerer Form-Wert aus einer Schlüsselliste.
fn first_nonempty<'a>(form: &'a [(String, String)], keys: &[&str]) -> &'a str {
    for key in keys {
        let v = form_get(form, key);
        if !v.trim().is_empty() {
            return v;
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_flag_parsing() {
        for yes in ["1", "true", "On", "YES", " yes "] {
            assert!(parse_member_flag(yes), "{yes} sollte true sein");
        }
        for no in ["", "0", "false", "nope"] {
            assert!(!parse_member_flag(no), "{no} sollte false sein");
        }
    }

    #[test]
    fn first_nonempty_picks_url_then_login() {
        let form = parse_form(b"url=&login=Nani&any=x");
        assert_eq!(first_nonempty(&form, &["url", "login", "any"]), "Nani");
        let form2 = parse_form(b"url=Foo&login=Bar");
        assert_eq!(first_nonempty(&form2, &["url", "login"]), "Foo");
    }

    #[test]
    fn redirect_ok_und_err_kodieren() {
        let ok = redirect_ok("nani hinzugefügt");
        assert_eq!(ok.status(), axum::http::StatusCode::SEE_OTHER);
        let loc = ok.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/admin?ok="));
        let err = redirect_err("could not remove");
        let loc = err.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/admin?err="));
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
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            // Zeitspalten als TEXT — die geteilte Domänen-Schicht
            // (departner_streamer/set_discord_profile) bindet ISO-Strings, exakt
            // wie die Prod-Tabellen sie speichern (Repo-Konvention, vgl.
            // admin_manual_plan-Tests).
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT, created_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, \
                 discord_user_id TEXT, discord_display_name TEXT, is_on_discord INTEGER DEFAULT 0, \
                 created_at TEXT, updated_at TEXT)",
            "CREATE TABLE twitch_partners (id BIGSERIAL PRIMARY KEY, twitch_login TEXT, twitch_user_id TEXT, \
                 status TEXT DEFAULT 'active', admin_archived_at TEXT, departnered_at TEXT, \
                 manual_partner_opt_out INTEGER DEFAULT 0, raid_bot_enabled INTEGER DEFAULT 1, \
                 technical_pause_reason TEXT)",
            "CREATE TABLE twitch_live_state (streamer_login TEXT PRIMARY KEY)",
            "CREATE TABLE twitch_raid_auth (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT, raid_enabled BOOLEAN DEFAULT TRUE)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn add_streamer_legt_nativen_eintrag_an() {
        let Some(pool) = make_pool("t_legacy_add").await else {
            return;
        };
        let msg = do_add(&pool, "nanistream").await.expect("add ok");
        assert_eq!(msg, "nanistream hinzugefügt");
        let exists: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM twitch_streamers WHERE twitch_login = 'nanistream'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists.0, 1);

        // Idempotent: zweiter Add meldet "bereits aktiv", legt nichts Neues an.
        let msg2 = do_add(&pool, "nanistream").await.expect("add2 ok");
        assert_eq!(msg2, "nanistream ist bereits aktiv");
    }

    #[tokio::test]
    async fn remove_departnert_aktiven_partner() {
        let Some(pool) = make_pool("t_legacy_remove").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('part', '77')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_partners (twitch_login, twitch_user_id, status) VALUES ('part', '77', 'active')")
            .execute(&pool)
            .await
            .unwrap();

        let msg = do_remove(&pool, "part").await.expect("remove ok");
        assert!(msg.contains("operativ deaktiviert"), "msg={msg}");
        let status: (String,) =
            sqlx::query_as("SELECT status FROM twitch_partners WHERE twitch_login = 'part'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.0, "departnered");
    }

    #[tokio::test]
    async fn remove_loescht_streamer_ohne_partner() {
        let Some(pool) = make_pool("t_legacy_remove_plain").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('solo', '9')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let msg = do_remove(&pool, "solo").await.expect("remove ok");
        assert_eq!(msg, "solo removed");
        let cnt: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_streamers WHERE twitch_login = 'solo'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cnt.0, 0);
    }

    #[tokio::test]
    async fn discord_profile_persistiert_id_und_member() {
        let Some(pool) = make_pool("t_legacy_discord").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('linkme', '123')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let msg = save_discord_profile(
            &pool,
            "linkme",
            Some("662995601738170389"),
            Some("Owner"),
            true,
        )
        .await
        .expect("profile ok");
        assert!(msg.contains("linkme"), "msg={msg}");

        let row: (Option<String>, Option<String>, i32) = sqlx::query_as(
            "SELECT discord_user_id, discord_display_name, is_on_discord \
             FROM twitch_streamer_identities WHERE twitch_user_id = '123'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("662995601738170389"));
        assert_eq!(row.1.as_deref(), Some("Owner"));
        assert_eq!(row.2, 1);
    }

    #[tokio::test]
    async fn discord_profile_lehnt_nicht_numerische_id_ab() {
        let Some(pool) = make_pool("t_legacy_discord_bad").await else {
            return;
        };
        let err = save_discord_profile(&pool, "x", Some("abc"), None, false)
            .await
            .unwrap_err();
        assert!(matches!(err, DiscordProfileError::Validation(_)));
    }

    // ── Route-Test (self-contained Router, Localhost-Bypass) ────────────────
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use tower::ServiceExt;

    fn router(pool: PgPool) -> Router {
        Router::new()
            .route("/twitch/add_streamer", post(add_streamer_handler))
            .route("/twitch/remove", post(remove_handler))
            .route("/twitch/discord_link", post(discord_link_handler))
            .route("/twitch/add_any", post(add_any_handler))
            .with_state(pool)
    }

    /// Loopback ohne Admin-Session → fail-closed, kein Write.
    #[tokio::test]
    async fn route_add_streamer_loopback_ohne_auth_fail_closed_302() {
        let Some(pool) = make_pool("t_legacy_route_add").await else {
            return;
        };
        let app = router(pool.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/twitch/add_streamer")
            .header("host", "127.0.0.1:8769")
            .header("content-type", "application/x-www-form-urlencoded")
            .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                12345,
            ))))
            .body(Body::from("login=RouteStreamer"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/admin?err="), "loc={loc}");

        let cnt: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM twitch_streamers WHERE twitch_login = 'routestreamer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt.0, 0);
    }

    /// Nicht-Localhost ohne Auth-State → fail-closed → 302 err (kein Write).
    #[tokio::test]
    async fn route_add_streamer_ohne_auth_abgelehnt() {
        let Some(pool) = make_pool("t_legacy_route_noauth").await else {
            return;
        };
        let app = router(pool.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/twitch/add_streamer")
            .header("host", "dashboard.example.com")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("login=Nope"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/admin?err="), "loc={loc}");
        let cnt: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_streamers WHERE twitch_login = 'nope'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cnt.0, 0);
    }
}

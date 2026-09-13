//! JSON-API für das Engagement-Dashboard (`/twitch/api/v2/engagement/*`).
//!
//! Port von `bot/engagement/dashboard_api.py` (CRUD-Teil). Vier Endpoints:
//! - `GET  …/engagement/settings` — Liste (Admin: alle, User: eigener Kanal).
//! - `POST …/engagement/toggle`   — enabled an/aus pro Kanal.
//! - `POST …/engagement/update`   — steam_id / persona_override / tabu_topics.
//! - `GET  …/engagement/log`      — Decision-Log eines Kanals.
//!
//! Permission-Modell (Python `_resolve_actor`):
//! - **Admin** = Admin-Auth-Level ODER `super_mod` (twitch_admin_roles)
//!   → sieht/togglet ALLE Kanäle. Ein per Twitch-OAuth eingeloggter Admin
//!   (`earlysalty`) behält seine Session-Identität für die Audit-Attribution
//!   (`enabled_by`); Discord-Admin ohne Twitch-Actor hat keine (senderauth-01).
//! - **Partner** (normaler User) → nur den eigenen Kanal (Session-`twitch_login`).
//! - **None** → 401.
//!
//! Der OAuth-Onboarding-Teil (sender-auth/callback) folgt separat.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, PgPool, Row};
use tb_crypto::FieldCipher;
use tb_engagement::sender_auth::{SenderAuthStore, SENDER_LOGIN};

use crate::auth::level::DashboardAuthLevel;

/// Aufgelöster Akteur: wer ist es, und darf er alle Kanäle steuern?
struct Actor {
    actor_id: Option<String>,
    actor_login: Option<String>,
    admin: bool,
}

/// `super_mod`-Rolle in `twitch_admin_roles` (Python `engagement.admin.is_super_mod`).
async fn is_super_mod(pool: &PgPool, user_id: &str) -> bool {
    if user_id.is_empty() {
        return false;
    }
    sqlx::query_scalar!(
        "SELECT 1 AS \"one!\" FROM twitch_admin_roles WHERE twitch_user_id = $1 AND role = 'super_mod' LIMIT 1",
        user_id
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
    .is_some()
}

/// Akteur aus dem Auth-Level ableiten. Admin → admin=true; die Session-Identität
/// (actor_id/actor_login) wird beibehalten, falls vorhanden — ein per Twitch-OAuth
/// eingeloggter Admin (`earlysalty`) trägt sie für die Audit-Attribution
/// (`enabled_by`). Python (`dashboard_api.py:214`) extrahiert die Session-Identität
/// IMMER zuerst, auch bei auth_level='admin'. Partner → eigener Login/ID, admin nur
/// wenn super_mod. None → 401.
async fn resolve_actor(auth: &DashboardAuthLevel, pool: &PgPool) -> Result<Actor, Response> {
    match auth {
        DashboardAuthLevel::Admin { actor } => Ok(Actor {
            actor_id: actor.as_ref().map(|a| a.twitch_user_id.clone()),
            actor_login: actor.as_ref().map(|a| a.twitch_login.clone()),
            admin: true,
        }),
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => {
            let admin = is_super_mod(pool, twitch_user_id).await;
            Ok(Actor {
                actor_id: Some(twitch_user_id.clone()),
                actor_login: Some(twitch_login.to_lowercase()),
                admin,
            })
        }
        DashboardAuthLevel::None => Err(err(StatusCode::UNAUTHORIZED, "Authentication required.")),
    }
}

fn err(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn db_error(error: sqlx::Error, ctx: &str) -> Response {
    tracing::error!(%error, "engagement-dashboard {ctx} DB-Fehler");
    err(StatusCode::INTERNAL_SERVER_ERROR, "db")
}

/// ISO-8601 mit UTC-Offset (mirror von Pythons `_iso`).
fn iso(ts: Option<chrono::DateTime<chrono::Utc>>) -> Value {
    match ts {
        Some(t) => json!(t.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)),
        None => Value::Null,
    }
}

/// Serialisiert eine `twitch_engagement_settings`-Zeile (camelCase wie Python).
fn serialize_settings(row: &PgRow) -> Value {
    let tabu: Option<Vec<String>> = row.try_get("tabu_topics").unwrap_or(None);
    json!({
        "channelLogin": row.try_get::<String, _>("channel_login").unwrap_or_default(),
        "enabled": row.try_get::<bool, _>("enabled").unwrap_or(false),
        "steamId": row.try_get::<Option<String>, _>("steam_id").unwrap_or(None),
        "personaOverride": row.try_get::<Option<String>, _>("persona_override").unwrap_or(None),
        "tabuTopics": tabu.unwrap_or_default(),
        "enabledAt": iso(row.try_get("enabled_at").unwrap_or(None)),
        "enabledBy": row.try_get::<Option<String>, _>("enabled_by").unwrap_or(None),
        "updatedAt": iso(row.try_get("updated_at").unwrap_or(None)),
    })
}

const SETTINGS_COLS: &str = "channel_login, enabled, steam_id, persona_override, tabu_topics, \
                             enabled_at, enabled_by, updated_at";

async fn load_one(pool: &PgPool, channel: &str) -> Result<Vec<Value>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {SETTINGS_COLS} FROM twitch_engagement_settings WHERE channel_login = $1"
    ))
    .bind(channel)
    .fetch_optional(pool)
    .await?;
    Ok(row.iter().map(serialize_settings).collect())
}

async fn load_all(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {SETTINGS_COLS} FROM twitch_engagement_settings ORDER BY channel_login"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(serialize_settings).collect())
}

#[derive(Deserialize, Default)]
pub struct ChannelQuery {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub limit: Option<String>,
}

/// `GET …/engagement/settings`.
pub async fn get_settings_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<ChannelQuery>,
) -> Response {
    let actor = match resolve_actor(&auth, &pool).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let channel = q
        .channel
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let result = match channel {
        Some(ch) => {
            if !actor.admin && Some(&ch) != actor.actor_login.as_ref() {
                return err(
                    StatusCode::FORBIDDEN,
                    "Du darfst nur deinen eigenen Channel sehen.",
                );
            }
            load_one(&pool, &ch).await
        }
        None if actor.admin => load_all(&pool).await,
        None => match &actor.actor_login {
            Some(login) => load_one(&pool, login).await,
            None => Ok(vec![]),
        },
    };

    match result {
        Ok(settings) => Json(json!({
            "settings": settings,
            "isSuperMod": actor.admin,
            "actorLogin": actor.actor_login,
        }))
        .into_response(),
        Err(e) => db_error(e, "settings"),
    }
}

/// `POST …/engagement/toggle` — Body `{ channelLogin, enabled }`.
pub async fn post_toggle_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(payload): Json<Value>,
) -> Response {
    let actor = match resolve_actor(&auth, &pool).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let channel = payload
        .get("channelLogin")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let enabled = payload.get("enabled").and_then(Value::as_bool);
    let (channel, enabled) = match (channel.is_empty(), enabled) {
        (false, Some(en)) => (channel, en),
        _ => {
            return err(
                StatusCode::BAD_REQUEST,
                "channelLogin (str) und enabled (bool) erforderlich.",
            )
        }
    };
    if !actor.admin && Some(&channel) != actor.actor_login.as_ref() {
        return err(
            StatusCode::FORBIDDEN,
            "Du darfst nur deinen eigenen Channel toggeln.",
        );
    }

    if let Err(e) = update_settings(
        &pool,
        &channel,
        Some(enabled),
        None,
        None,
        None,
        actor.actor_id.as_deref(),
    )
    .await
    {
        return db_error(e, "toggle");
    }
    settings_response(&pool, &channel).await
}

/// `POST …/engagement/update` — Body mit optionalen `steamId` /
/// `personaOverride` / `tabuTopics`. Feld vorhanden → schreiben (null/leer →
/// NULL/[]); Feld fehlt → unberührt.
pub async fn post_update_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(payload): Json<Value>,
) -> Response {
    let actor = match resolve_actor(&auth, &pool).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let channel = payload
        .get("channelLogin")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if channel.is_empty() {
        return err(StatusCode::BAD_REQUEST, "channelLogin erforderlich.");
    }
    if !actor.admin && Some(&channel) != actor.actor_login.as_ref() {
        return err(
            StatusCode::FORBIDDEN,
            "Du darfst nur deinen eigenen Channel verändern.",
        );
    }

    // Feld vorhanden? null → Clear-Marker (""), string → roh. Sonst 400.
    let steam_id = match payload.get("steamId") {
        None => None,
        Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "steamId muss string oder null sein.",
            )
        }
    };
    let persona = match payload.get("personaOverride") {
        None => None,
        Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "personaOverride muss string oder null sein.",
            )
        }
    };
    let tabu = match payload.get("tabuTopics") {
        None => None,
        Some(Value::Null) => Some(Vec::new()),
        Some(Value::Array(a)) => Some(
            a.iter()
                .filter_map(Value::as_str)
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>(),
        ),
        Some(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "tabuTopics muss array oder null sein.",
            )
        }
    };

    if let Err(e) = update_settings(
        &pool,
        &channel,
        None,
        steam_id,
        persona,
        tabu,
        actor.actor_id.as_deref(),
    )
    .await
    {
        return db_error(e, "update");
    }
    settings_response(&pool, &channel).await
}

/// Antwortet mit der frisch geladenen Settings-Zeile (oder `null`).
async fn settings_response(pool: &PgPool, channel: &str) -> Response {
    match load_one(pool, channel).await {
        Ok(mut list) => {
            Json(json!({ "settings": list.pop().unwrap_or(Value::Null) })).into_response()
        }
        Err(e) => db_error(e, "reload"),
    }
}

/// `GET …/engagement/log?channel=&limit=`.
pub async fn get_log_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<ChannelQuery>,
) -> Response {
    let actor = match resolve_actor(&auth, &pool).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let channel = q.channel.as_deref().unwrap_or("").trim().to_lowercase();
    if channel.is_empty() {
        return err(StatusCode::BAD_REQUEST, "channel query-param erforderlich.");
    }
    if !actor.admin && Some(&channel) != actor.actor_login.as_ref() {
        return err(
            StatusCode::FORBIDDEN,
            "Du darfst nur deinen eigenen Log sehen.",
        );
    }
    let limit = q
        .limit
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(25)
        .clamp(1, 200);

    let rows = sqlx::query!(
        "SELECT decision, response_text, model, prompt_tokens, completion_tokens, \
                cost_usd_estimate::float8 AS \"cost_usd_estimate?\", latency_ms, ts \
         FROM twitch_engagement_log WHERE channel_login = $1 ORDER BY ts DESC LIMIT $2",
        &channel,
        limit
    )
    .fetch_all(&pool)
    .await;

    match rows {
        Ok(rows) => {
            let entries: Vec<Value> = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "decision": row.decision,
                        "responseText": row.response_text,
                        "model": row.model,
                        "promptTokens": row.prompt_tokens,
                        "completionTokens": row.completion_tokens,
                        "costUsdEstimate": row.cost_usd_estimate,
                        "latencyMs": row.latency_ms,
                        "ts": iso(Some(row.ts)),
                    })
                })
                .collect();
            Json(json!({ "channelLogin": channel, "entries": entries })).into_response()
        }
        Err(e) => db_error(e, "log"),
    }
}

/// Dashboard-Wahl atomar speichern, auch wenn ein Smalltalk-Test seine
/// provisorische Settings-Zeile gleichzeitig aufräumt. Nur ausdrücklich
/// gesetzte Felder ändern sich; der An/Aus-Schalter steuert auch Lese- und
/// Versandmodus. Reine Profiländerungen behalten den Laufzustand bei.
async fn update_settings(
    pool: &PgPool,
    channel: &str,
    enabled: Option<bool>,
    steam_id: Option<String>,
    persona: Option<String>,
    tabu: Option<Vec<String>>,
    actor_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // Derselbe Lock wie Start/Ende der Smalltalk-Sitzung: Profiländerungen
    // dürfen nicht zwischen deren Settings-Snapshot und Aufräumen verloren gehen.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('tb-engagement:smalltalk-loop-global', 0))",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO twitch_engagement_settings AS current
            (channel_login, enabled, steam_id, persona_override, tabu_topics,
             enabled_at, enabled_by, updated_at, irc_read, output_mode)
         VALUES ($1, COALESCE($2, FALSE), NULLIF(BTRIM($3), ''), NULLIF(BTRIM($4), ''),
                 COALESCE($5, ARRAY[]::text[]), CASE WHEN $2 THEN NOW() ELSE NULL END,
                 $6, NOW(), COALESCE($2, FALSE), CASE WHEN $2 THEN 'live' ELSE 'off' END)
         ON CONFLICT (channel_login) DO UPDATE SET
             enabled = COALESCE($2, current.enabled),
             irc_read = CASE WHEN $2 IS NULL THEN current.irc_read ELSE $2 END,
             output_mode = CASE WHEN $2 IS NULL THEN current.output_mode
                                WHEN $2 THEN 'live' ELSE 'off' END,
             steam_id = CASE WHEN $3 IS NULL THEN current.steam_id ELSE EXCLUDED.steam_id END,
             persona_override = CASE WHEN $4 IS NULL THEN current.persona_override ELSE EXCLUDED.persona_override END,
             tabu_topics = CASE WHEN $5 IS NULL THEN current.tabu_topics ELSE EXCLUDED.tabu_topics END,
             enabled_at = CASE WHEN $2 THEN NOW() ELSE current.enabled_at END,
             enabled_by = CASE WHEN $2 THEN COALESCE($6, current.enabled_by) ELSE current.enabled_by END,
             updated_at = NOW()",
    )
    .bind(channel)
    .bind(enabled)
    .bind(steam_id)
    .bind(persona)
    .bind(tabu)
    .bind(actor_id)
    .execute(&mut *tx)
    .await?;
    // Auch eine ursprünglich provisorische Zeile gehört nach explizitem
    // Speichern dem Nutzer. Das Testende stellt nur die Laufwerte zurück,
    // statt die gerade gespeicherten Profilfelder mit der Zeile zu löschen.
    sqlx::query("UPDATE twitch_smalltalk_sessions SET settings_existed = TRUE WHERE channel_login = $1 AND ended_at IS NULL")
        .bind(channel).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

// === OAuth-Onboarding des Engagement-Sende-Accounts (Smoke-Account) ===

/// Baut den SenderAuthStore aus Env (DB_MASTER_KEY_V1 + TWITCH_CLIENT_ID/SECRET).
/// `None`, wenn Krypto-Key oder App-Credentials fehlen.
fn build_sender_store(pool: PgPool) -> Option<SenderAuthStore> {
    let cipher = Arc::new(FieldCipher::from_env().ok()?);
    SenderAuthStore::from_env(pool, cipher)
}

/// `GET …/engagement/sender-auth` — Admin-only: erzeugt den Authorize-Link für
/// den Sende-Account (Port von `_handle_sender_auth_start`).
pub async fn sender_auth_start_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    let actor = match resolve_actor(&auth, &pool).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    if !actor.admin {
        return err(
            StatusCode::FORBIDDEN,
            "Nur Admins dürfen den Sende-Account autorisieren.",
        );
    }
    let Some(store) = build_sender_store(pool) else {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Sende-Account-Setup nicht verfügbar (DB_MASTER_KEY_V1/TWITCH_CLIENT_ID fehlt).",
        );
    };
    match store.build_authorize_url().await {
        Ok(url) => Json(json!({
            "authorizeUrl": url,
            "senderLogin": SENDER_LOGIN,
            "hint": "In einem separaten Browser/Inkognito als der Sende-Account einloggen, \
                     dann diesen Link öffnen und Authorize klicken.",
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "engagement sender-auth: Link-Erzeugung fehlgeschlagen");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Link-Erzeugung fehlgeschlagen.",
            )
        }
    }
}

#[derive(Deserialize, Default)]
pub struct CallbackQuery {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub error: String,
}

/// `GET …/engagement/sender-callback` + `/callback/engagement-sender` —
/// öffentlicher OAuth-Callback (Sicherheit über den State-Token; keine
/// Session-Auth). Port von `_handle_sender_auth_callback`. Liefert eine
/// HTML-Seite.
pub async fn sender_auth_callback_handler(
    State(pool): State<PgPool>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if !q.error.is_empty() {
        return page(
            "Autorisierung abgebrochen",
            &format!("Twitch meldete: {}", esc(&q.error)),
            StatusCode::BAD_REQUEST,
        );
    }
    if q.code.is_empty() || q.state.is_empty() {
        return page(
            "Ungültige Anfrage",
            "Code oder State fehlt.",
            StatusCode::BAD_REQUEST,
        );
    }
    let Some(store) = build_sender_store(pool) else {
        return page(
            "Setup nicht verfügbar",
            "Krypto-Key oder App-Credentials fehlen auf dem Dashboard-Dienst.",
            StatusCode::INTERNAL_SERVER_ERROR,
        );
    };
    match store.handle_callback(&q.code, &q.state).await {
        Ok(result) => page(
            "Sende-Account verbunden ✓",
            &format!(
                "Der Engagement-Account <b>{}</b> ist jetzt autorisiert. \
                 Du kannst dieses Fenster schließen.",
                esc(&result.login)
            ),
            StatusCode::OK,
        ),
        Err(error) => {
            tracing::error!(%error, "engagement sender-auth callback fehlgeschlagen");
            page(
                "Autorisierung fehlgeschlagen",
                &esc(&error),
                StatusCode::BAD_REQUEST,
            )
        }
    }
}

/// Minimale HTML-Antwortseite (mirror von Pythons `_page`).
fn page(title: &str, body: &str, status: StatusCode) -> Response {
    let html = format!(
        "<!doctype html><html><head><meta charset='utf-8'><title>{t}</title></head>\
         <body style='font-family:sans-serif;max-width:560px;margin:40px auto'>\
         <h2>{t}</h2><p>{body}</p></body></html>",
        t = esc(title)
    );
    (status, Html(html)).into_response()
}

/// Minimales HTML-Escaping für eingebettete dynamische Strings.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[tokio::test]
    async fn admin_toggle_schaltet_versand_und_lesepfad_atomar_profil_bleibt_erhalten() {
        let db = crate::test_postgres::TestPostgres::start().await;
        sqlx::raw_sql(
            "CREATE TABLE twitch_engagement_settings (
            channel_login TEXT PRIMARY KEY, channel_user_id TEXT,
            enabled BOOLEAN NOT NULL DEFAULT FALSE, steam_id TEXT, persona_override TEXT,
            tabu_topics TEXT[], enabled_at TIMESTAMPTZ, enabled_by TEXT,
            irc_read BOOLEAN NOT NULL DEFAULT FALSE, output_mode TEXT NOT NULL DEFAULT 'off',
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260727150000_twitch_smalltalk_loop.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::raw_sql("CREATE TABLE twitch_partner_outreach (streamer_login TEXT PRIMARY KEY, cooldown_until TEXT)")
            .execute(&db.pool).await.unwrap();
        for existed in [false, true] {
            sqlx::query("DELETE FROM twitch_engagement_settings")
                .execute(&db.pool)
                .await
                .unwrap();
            if existed {
                sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, irc_read, output_mode) VALUES ('nani', TRUE, TRUE, 'test')")
                    .execute(&db.pool).await.unwrap();
            }
            for enabled in [true, false, true] {
                let response = post_toggle_handler(
                    DashboardAuthLevel::admin(),
                    State(db.pool.clone()),
                    Json(json!({"channelLogin":"nani", "enabled":enabled})),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let state: (bool, bool, String) = sqlx::query_as("SELECT enabled, irc_read, output_mode FROM twitch_engagement_settings WHERE channel_login='nani'")
                    .fetch_one(&db.pool).await.unwrap();
                assert_eq!(
                    state,
                    (
                        enabled,
                        enabled,
                        if enabled { "live" } else { "off" }.into()
                    )
                );
                let runtime = tb_engagement::gate::load_settings(&db.pool, "nani", None)
                    .await
                    .unwrap();
                assert_eq!(runtime.enabled, enabled);
                assert_eq!(
                    runtime.output_mode,
                    if enabled {
                        tb_engagement::types::OutputMode::Live
                    } else {
                        tb_engagement::types::OutputMode::Off
                    }
                );
            }
            // Profiländerung und Toggle konkurrieren: beide müssen erhalten bleiben.
            let (profile, toggle) = tokio::join!(
                update_settings(
                    &db.pool,
                    "nani",
                    None,
                    Some(" 123 ".into()),
                    Some("frech".into()),
                    None,
                    None
                ),
                update_settings(&db.pool, "nani", Some(false), None, None, None, None),
            );
            profile.unwrap();
            toggle.unwrap();
            let state: (bool, bool, String, String, String) = sqlx::query_as("SELECT enabled, irc_read, output_mode, steam_id, persona_override FROM twitch_engagement_settings WHERE channel_login='nani'")
                .fetch_one(&db.pool).await.unwrap();
            assert_eq!(
                state,
                (false, false, "off".into(), "123".into(), "frech".into())
            );
        }
        // Ein Profilupdate während eines Tests behält den Testmodus bis zum
        // Ende, muss danach aber in der ursprünglich provisorischen Zeile bleiben.
        sqlx::query("UPDATE twitch_engagement_settings SET enabled=TRUE, irc_read=TRUE, output_mode='test' WHERE channel_login='nani'")
            .execute(&db.pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_smalltalk_sessions (id,channel_login,streamer_user_id,started_at,settings_existed,previous_enabled,previous_irc_read,previous_output_mode) VALUES ('00000000-0000-0000-0000-000000000001','nani','42',NOW(),FALSE,FALSE,FALSE,'off')")
            .execute(&db.pool).await.unwrap();
        update_settings(
            &db.pool,
            "nani",
            None,
            None,
            Some("gespeichertes Profil".into()),
            None,
            None,
        )
        .await
        .unwrap();
        let store = tb_engagement::smalltalk_loop_store::SmalltalkLoopStore::new(db.pool.clone());
        store
            .close_active_session("session_timeout", chrono::Utc::now())
            .await
            .unwrap();
        let state: (bool, bool, String, String) = sqlx::query_as("SELECT enabled,irc_read,output_mode,persona_override FROM twitch_engagement_settings WHERE channel_login='nani'")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(
            state,
            (false, false, "off".into(), "gespeichertes Profil".into())
        );
    }

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
        sqlx::query(
            "CREATE TABLE twitch_engagement_settings (channel_login TEXT PRIMARY KEY, \
             enabled BOOLEAN NOT NULL DEFAULT FALSE, steam_id TEXT, persona_override TEXT, \
             tabu_topics TEXT[], enabled_at TIMESTAMPTZ, enabled_by TEXT, \
             irc_read BOOLEAN NOT NULL DEFAULT FALSE, output_mode TEXT NOT NULL DEFAULT 'off', \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_admin_roles (twitch_user_id TEXT, role TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_smalltalk_sessions (channel_login TEXT, ended_at TIMESTAMPTZ, settings_existed BOOLEAN)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn toggle_legt_an_und_setzt_enabled() {
        let Some(pool) = make_pool("t_eng_dash_toggle").await else {
            return;
        };
        // Neue Zeile via toggle on.
        update_settings(&pool, "nani", Some(true), None, None, None, Some("42"))
            .await
            .unwrap();
        let list = load_one(&pool, "nani").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["enabled"], json!(true));
        assert_eq!(list[0]["enabledBy"], json!("42"));
        assert!(list[0]["enabledAt"].is_string()); // gesetzt weil enabled

        // toggle off lässt enabled_by/at stehen, setzt enabled=false.
        update_settings(&pool, "nani", Some(false), None, None, None, Some("99"))
            .await
            .unwrap();
        let list = load_one(&pool, "nani").await.unwrap();
        assert_eq!(list[0]["enabled"], json!(false));
        assert_eq!(list[0]["enabledBy"], json!("42")); // COALESCE: nicht überschrieben (off)
    }

    #[tokio::test]
    async fn update_felder_und_clear() {
        let Some(pool) = make_pool("t_eng_dash_update").await else {
            return;
        };
        update_settings(&pool, "nani", Some(true), None, None, None, Some("1"))
            .await
            .unwrap();
        // steam_id + persona + tabu setzen. tabu kommt vom Handler bereits
        // gefiltert (leere Elemente raus) — update_settings speichert verbatim.
        update_settings(
            &pool,
            "nani",
            None,
            Some("  76561  ".to_string()),
            Some("frech".to_string()),
            Some(vec!["politik".to_string()]),
            Some("1"),
        )
        .await
        .unwrap();
        let list = load_one(&pool, "nani").await.unwrap();
        assert_eq!(list[0]["steamId"], json!("76561")); // getrimmt
        assert_eq!(list[0]["personaOverride"], json!("frech"));
        assert_eq!(list[0]["tabuTopics"], json!(["politik"]));
        assert_eq!(list[0]["enabled"], json!(true)); // unberührt

        // steam_id clearen (leerer String → NULL).
        update_settings(
            &pool,
            "nani",
            None,
            Some(String::new()),
            None,
            None,
            Some("1"),
        )
        .await
        .unwrap();
        let list = load_one(&pool, "nani").await.unwrap();
        assert_eq!(list[0]["steamId"], Value::Null);
        assert_eq!(list[0]["personaOverride"], json!("frech")); // nicht angefasst
    }

    #[tokio::test]
    async fn super_mod_erkennung() {
        let Some(pool) = make_pool("t_eng_dash_supermod").await else {
            return;
        };
        assert!(!is_super_mod(&pool, "7").await);
        sqlx::query(
            "INSERT INTO twitch_admin_roles (twitch_user_id, role) VALUES ('7', 'super_mod')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(is_super_mod(&pool, "7").await);
        assert!(!is_super_mod(&pool, "").await); // leere ID
    }

    // === senderauth-01: Admin-Actor-Attribution ===

    /// Ein per Twitch-OAuth eingeloggter Admin (Login z. B. `earlysalty`) wird vom
    /// Extractor zu `Admin { actor: Some(..) }` promoted. `resolve_actor` MUSS die
    /// Session-Identität (actor_id/actor_login) behalten, damit Audit-Spalten wie
    /// `enabled_by` den realen Admin tragen — Python (`dashboard_api.py:214`)
    /// extrahiert die Session-Identität IMMER, auch bei auth_level='admin'.
    #[tokio::test]
    async fn twitch_admin_behaelt_actor_attribution() {
        let Some(pool) = make_pool("t_eng_dash_admin_attr").await else {
            return;
        };
        let auth = DashboardAuthLevel::Admin {
            actor: Some(crate::auth::level::AdminActor {
                twitch_user_id: "555".into(),
                twitch_login: "earlysalty".into(),
            }),
        };
        let actor = resolve_actor(&auth, &pool).await.expect("resolve ok");
        assert!(actor.admin);
        assert_eq!(actor.actor_id.as_deref(), Some("555"));
        assert_eq!(actor.actor_login.as_deref(), Some("earlysalty")); // bereits klein
    }

    /// Discord-Admin ohne Twitch-Session-Identität → admin, aber
    /// keine Attribution (Python: `_extract_session_user({})` → None,None).
    #[tokio::test]
    async fn discord_admin_ohne_attribution() {
        let Some(pool) = make_pool("t_eng_dash_admin_noattr").await else {
            return;
        };
        let admin = resolve_actor(&DashboardAuthLevel::Admin { actor: None }, &pool)
            .await
            .expect("resolve ok");
        assert!(admin.admin);
        assert_eq!(admin.actor_id, None);
        assert_eq!(admin.actor_login, None);
    }
}

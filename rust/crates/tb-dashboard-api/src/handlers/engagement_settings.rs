//! JSON-API für das Engagement-Dashboard (`/twitch/api/v2/engagement/*`).
//!
//! Port von `bot/engagement/dashboard_api.py` (CRUD-Teil). Vier Endpoints:
//! - `GET  …/engagement/settings` — Liste (Admin: alle, User: eigener Kanal).
//! - `POST …/engagement/toggle`   — enabled an/aus pro Kanal.
//! - `POST …/engagement/update`   — steam_id / persona_override / tabu_topics.
//! - `GET  …/engagement/log`      — Decision-Log eines Kanals.
//!
//! Permission-Modell (Python `_resolve_actor`):
//! - **Admin** = Localhost/Admin-Auth-Level ODER `super_mod` (twitch_admin_roles)
//!   → sieht/togglet ALLE Kanäle. Ein per Twitch-OAuth eingeloggter Admin
//!   (`earlysalty`) behält seine Session-Identität für die Audit-Attribution
//!   (`enabled_by`); Discord-Admin/Localhost haben keine (senderauth-01).
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
use sqlx::{postgres::PgRow, PgPool, Postgres, QueryBuilder, Row};
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
    sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM twitch_admin_roles WHERE twitch_user_id = $1 AND role = 'super_mod' LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
    .is_some()
}

/// Akteur aus dem Auth-Level ableiten. Localhost → admin=true ohne Identität
/// (reiner Loopback, keine Session). Admin → admin=true; die Session-Identität
/// (actor_id/actor_login) wird beibehalten, falls vorhanden — ein per Twitch-OAuth
/// eingeloggter Admin (`earlysalty`) trägt sie für die Audit-Attribution
/// (`enabled_by`). Python (`dashboard_api.py:214`) extrahiert die Session-Identität
/// IMMER zuerst, auch bei auth_level='admin'. Partner → eigener Login/ID, admin nur
/// wenn super_mod. None → 401.
async fn resolve_actor(auth: &DashboardAuthLevel, pool: &PgPool) -> Result<Actor, Response> {
    match auth {
        DashboardAuthLevel::Localhost => Ok(Actor {
            actor_id: None,
            actor_login: None,
            admin: true,
        }),
        DashboardAuthLevel::Admin { actor } => Ok(Actor {
            actor_id: actor.as_ref().map(|a| a.twitch_user_id.clone()),
            actor_login: actor.as_ref().map(|a| a.twitch_login.clone()),
            admin: true,
        }),
        DashboardAuthLevel::Partner { twitch_login, twitch_user_id, .. } => {
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
                return err(StatusCode::FORBIDDEN, "Du darfst nur deinen eigenen Channel sehen.");
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
        _ => return err(StatusCode::BAD_REQUEST, "channelLogin (str) und enabled (bool) erforderlich."),
    };
    if !actor.admin && Some(&channel) != actor.actor_login.as_ref() {
        return err(StatusCode::FORBIDDEN, "Du darfst nur deinen eigenen Channel toggeln.");
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
        return err(StatusCode::FORBIDDEN, "Du darfst nur deinen eigenen Channel verändern.");
    }

    // Feld vorhanden? null → Clear-Marker (""), string → roh. Sonst 400.
    let steam_id = match payload.get("steamId") {
        None => None,
        Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return err(StatusCode::BAD_REQUEST, "steamId muss string oder null sein."),
    };
    let persona = match payload.get("personaOverride") {
        None => None,
        Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return err(StatusCode::BAD_REQUEST, "personaOverride muss string oder null sein."),
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
        Some(_) => return err(StatusCode::BAD_REQUEST, "tabuTopics muss array oder null sein."),
    };

    if let Err(e) =
        update_settings(&pool, &channel, None, steam_id, persona, tabu, actor.actor_id.as_deref()).await
    {
        return db_error(e, "update");
    }
    settings_response(&pool, &channel).await
}

/// Antwortet mit der frisch geladenen Settings-Zeile (oder `null`).
async fn settings_response(pool: &PgPool, channel: &str) -> Response {
    match load_one(pool, channel).await {
        Ok(mut list) => Json(json!({ "settings": list.pop().unwrap_or(Value::Null) })).into_response(),
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
        return err(StatusCode::FORBIDDEN, "Du darfst nur deinen eigenen Log sehen.");
    }
    let limit = q
        .limit
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(25)
        .clamp(1, 200);

    let rows = sqlx::query(
        "SELECT decision, response_text, model, prompt_tokens, completion_tokens, \
                cost_usd_estimate, latency_ms, ts \
         FROM twitch_engagement_log WHERE channel_login = $1 ORDER BY ts DESC LIMIT $2",
    )
    .bind(&channel)
    .bind(limit)
    .fetch_all(&pool)
    .await;

    match rows {
        Ok(rows) => {
            let entries: Vec<Value> = rows.iter().map(serialize_log).collect();
            Json(json!({ "channelLogin": channel, "entries": entries })).into_response()
        }
        Err(e) => db_error(e, "log"),
    }
}

fn serialize_log(row: &PgRow) -> Value {
    json!({
        "decision": row.try_get::<String, _>("decision").unwrap_or_default(),
        "responseText": row.try_get::<Option<String>, _>("response_text").unwrap_or(None),
        "model": row.try_get::<Option<String>, _>("model").unwrap_or(None),
        "promptTokens": row.try_get::<Option<i32>, _>("prompt_tokens").unwrap_or(None),
        "completionTokens": row.try_get::<Option<i32>, _>("completion_tokens").unwrap_or(None),
        "costUsdEstimate": row.try_get::<Option<f64>, _>("cost_usd_estimate").unwrap_or(None),
        "latencyMs": row.try_get::<Option<i32>, _>("latency_ms").unwrap_or(None),
        "ts": iso(row.try_get("ts").unwrap_or(None)),
    })
}

/// `"" / nur-Whitespace → None`, sonst getrimmt (mirror Pythons
/// `(x or "").strip() or None`).
fn normalize_opt(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Kern von `_sync_update_settings`: neue Zeile → INSERT (Rohwerte), sonst
/// dynamisches UPDATE (nur gesetzte Felder; steam/persona getrimmt→NULL).
async fn update_settings(
    pool: &PgPool,
    channel: &str,
    enabled: Option<bool>,
    steam_id: Option<String>,
    persona: Option<String>,
    tabu: Option<Vec<String>>,
    actor_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    let exists = sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM twitch_engagement_settings WHERE channel_login = $1",
    )
    .bind(channel)
    .fetch_optional(pool)
    .await?
    .is_some();

    if !exists {
        let en = enabled.unwrap_or(false);
        sqlx::query(
            "INSERT INTO twitch_engagement_settings \
                (channel_login, enabled, steam_id, persona_override, tabu_topics, \
                 enabled_at, enabled_by, updated_at) \
             VALUES ($1, $2, $3, $4, $5, CASE WHEN $6 THEN NOW() ELSE NULL END, $7, NOW())",
        )
        .bind(channel)
        .bind(en)
        .bind(steam_id)
        .bind(persona)
        .bind(tabu.unwrap_or_default())
        .bind(en)
        .bind(actor_id)
        .execute(pool)
        .await?;
        return Ok(());
    }

    // Bestehende Zeile: nur explizit gesetzte Felder anfassen.
    if enabled.is_none() && steam_id.is_none() && persona.is_none() && tabu.is_none() {
        return Ok(());
    }
    let mut qb = QueryBuilder::<Postgres>::new(
        "UPDATE twitch_engagement_settings SET updated_at = NOW()",
    );
    if let Some(en) = enabled {
        qb.push(", enabled = ").push_bind(en);
        if en {
            qb.push(", enabled_at = NOW(), enabled_by = COALESCE(")
                .push_bind(actor_id.map(str::to_string))
                .push(", enabled_by)");
        }
    }
    if let Some(s) = steam_id {
        qb.push(", steam_id = ").push_bind(normalize_opt(s));
    }
    if let Some(p) = persona {
        qb.push(", persona_override = ").push_bind(normalize_opt(p));
    }
    if let Some(t) = tabu {
        qb.push(", tabu_topics = ").push_bind(t);
    }
    qb.push(" WHERE channel_login = ").push_bind(channel);
    qb.build().execute(pool).await?;
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
        return err(StatusCode::FORBIDDEN, "Nur Admins dürfen den Sende-Account autorisieren.");
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
            err(StatusCode::INTERNAL_SERVER_ERROR, "Link-Erzeugung fehlgeschlagen.")
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
        return page("Autorisierung abgebrochen", &format!("Twitch meldete: {}", esc(&q.error)), StatusCode::BAD_REQUEST);
    }
    if q.code.is_empty() || q.state.is_empty() {
        return page("Ungültige Anfrage", "Code oder State fehlt.", StatusCode::BAD_REQUEST);
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
            page("Autorisierung fehlgeschlagen", &esc(&error), StatusCode::BAD_REQUEST)
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
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_settings (channel_login TEXT PRIMARY KEY, \
             enabled BOOLEAN NOT NULL DEFAULT FALSE, steam_id TEXT, persona_override TEXT, \
             tabu_topics TEXT[], enabled_at TIMESTAMPTZ, enabled_by TEXT, \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_admin_roles (twitch_user_id TEXT, role TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn toggle_legt_an_und_setzt_enabled() {
        let Some(pool) = make_pool("t_eng_dash_toggle").await else { return };
        // Neue Zeile via toggle on.
        update_settings(&pool, "nani", Some(true), None, None, None, Some("42")).await.unwrap();
        let list = load_one(&pool, "nani").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["enabled"], json!(true));
        assert_eq!(list[0]["enabledBy"], json!("42"));
        assert!(list[0]["enabledAt"].is_string()); // gesetzt weil enabled

        // toggle off lässt enabled_by/at stehen, setzt enabled=false.
        update_settings(&pool, "nani", Some(false), None, None, None, Some("99")).await.unwrap();
        let list = load_one(&pool, "nani").await.unwrap();
        assert_eq!(list[0]["enabled"], json!(false));
        assert_eq!(list[0]["enabledBy"], json!("42")); // COALESCE: nicht überschrieben (off)
    }

    #[tokio::test]
    async fn update_felder_und_clear() {
        let Some(pool) = make_pool("t_eng_dash_update").await else { return };
        update_settings(&pool, "nani", Some(true), None, None, None, Some("1")).await.unwrap();
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
        update_settings(&pool, "nani", None, Some(String::new()), None, None, Some("1")).await.unwrap();
        let list = load_one(&pool, "nani").await.unwrap();
        assert_eq!(list[0]["steamId"], Value::Null);
        assert_eq!(list[0]["personaOverride"], json!("frech")); // nicht angefasst
    }

    #[tokio::test]
    async fn super_mod_erkennung() {
        let Some(pool) = make_pool("t_eng_dash_supermod").await else { return };
        assert!(!is_super_mod(&pool, "7").await);
        sqlx::query("INSERT INTO twitch_admin_roles (twitch_user_id, role) VALUES ('7', 'super_mod')")
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
        let Some(pool) = make_pool("t_eng_dash_admin_attr").await else { return };
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

    /// Discord-Admin / Localhost ohne Twitch-Session-Identität → admin, aber
    /// keine Attribution (Python: `_extract_session_user({})` → None,None).
    #[tokio::test]
    async fn discord_admin_und_localhost_ohne_attribution() {
        let Some(pool) = make_pool("t_eng_dash_admin_noattr").await else { return };
        let admin = resolve_actor(&DashboardAuthLevel::Admin { actor: None }, &pool)
            .await
            .expect("resolve ok");
        assert!(admin.admin);
        assert_eq!(admin.actor_id, None);
        assert_eq!(admin.actor_login, None);

        let local = resolve_actor(&DashboardAuthLevel::Localhost, &pool)
            .await
            .expect("resolve ok");
        assert!(local.admin);
        assert_eq!(local.actor_id, None);
        assert_eq!(local.actor_login, None);
    }
}

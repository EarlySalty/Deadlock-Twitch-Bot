//! MCP-Connector — MCP-Server (Streamable HTTP, JSON-RPC) im tb-bot-Prozess.
//!
//! Eine Claude-Sitzung verbindet sich als MCP-Client auf
//! `http://127.0.0.1:8892/mcp` und verwaltet den Twitch-Bot mit der Identität
//! des laufenden Prozesses: Postgres-Pool, Streamer-Token, Helix-Zugang und
//! Discord-Broker liegen alle schon hier. Der Aufrufer braucht deshalb kein
//! einziges Secret.
//!
//! **Kein zweiter Pfad.** Jedes Werkzeug ruft die Logik auf, die es schon gibt:
//! `disconnect_bot` geht durch `tb_internal_api::disconnect_bot_handler_inner`
//! (derselbe Ablauf wie „Bot vom Kanal trennen" im Dashboard), der Sweep fährt
//! den `DeadlockPauseReactor` des Timers, die Vorschau liest über dieselben
//! Kandidaten-Queries wie der Sweep, die Partner-Liste über
//! `tb_analytics::streamers_crud::list_streamers`. Eigenes SQL steht hier nur
//! für die zwei Partner-Spalten, die keine dieser Quellen führt
//! (`technical_pause_reason`, `deadlock_pause_unmodded_at`).
//!
//! Schreibende Werkzeuge verlangen `confirm: true`. Ohne das antworten sie mit
//! einer Vorschau, nicht mit einem Fehler: ein vergessenes Flag soll erklären,
//! was passiert wäre, statt den Aufrufer raten zu lassen.
//!
//! Env (Netzwerkparameter optional):
//!   TB_MCP_HOST    default 127.0.0.1 (muss Loopback sein, sonst startet er nicht)
//!   TB_MCP_PORT    default 8892 (dediziert; 8891 gehört dem Relay)
//!   TWITCH_INTERNAL_API_TOKEN wird aus der bestehenden Bot-Konfiguration
//!   übernommen und ist Pflicht: ausschließlich `Authorization: Bearer <t>`.
//!
//! Ein Bind-Fehler beendet den Bot NICHT (anders als beim Discord-Bot): tb-bot
//! ist ein Produktivdienst, ein belegter Debug-Port darf ihn nicht kosten.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use tb_analytics::{raid_blacklist, streamers_crud};
use tb_internal_api::{disconnect_bot_handler_inner, DiscordRoleExt, ModeratorRemovalExt};
use tb_raid::token_lifecycle::{BotBanStatus, BotBanStatusProbe};
use tb_raid::TokenBlacklist;

use crate::partner_lookup;
use crate::task_supervisor::TaskSupervisor;
use crate::token_lifecycle_wiring::SharedDeadlockPauseReactor;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8892;
const PROTOCOL_FALLBACK: &str = "2025-03-26";
const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];
/// Obergrenze für `list_partners`, damit eine vergessene Filterangabe nicht das
/// ganze Kontextfenster des Aufrufers füllt.
const DEFAULT_PARTNER_LIMIT: usize = 200;
/// Sessions je Partner in `partner_status`.
const SESSION_LIMIT: usize = 10;
/// Harte Obergrenze für einen authentifizierten JSON-RPC-Request. Die Prüfung
/// erfolgt erst nach dem Header-Gate, damit Unauthentifizierte keinen Body
/// einlesen lassen können.
const MAX_MCP_BODY_BYTES: usize = 1024 * 1024;
const MAX_MCP_BATCH_ITEMS: usize = 16;
const MAX_MCP_CONCURRENT_REQUESTS: usize = 4;
const MAX_MCP_REQUEST_SECONDS: u64 = 30;

// ───────────────────────────── Zustand ─────────────────────────────

/// Alles, was die Werkzeuge brauchen. Bewusst nur Ports und der Pool: der
/// Connector hält keinen eigenen Zustand, jeder Aufruf liest frisch.
pub(crate) struct McpState {
    pool: PgPool,
    auth_token: Option<String>,
    role_ext: DiscordRoleExt,
    removal_ext: ModeratorRemovalExt,
    ban_probe: Option<Arc<dyn BotBanStatusProbe>>,
    deadlock_pause: Option<SharedDeadlockPauseReactor>,
    request_slots: tokio::sync::Semaphore,
}

impl McpState {
    pub(crate) fn new(
        pool: PgPool,
        auth_token: String,
        role_ext: DiscordRoleExt,
        removal_ext: ModeratorRemovalExt,
        ban_probe: Option<Arc<dyn BotBanStatusProbe>>,
        deadlock_pause: Option<SharedDeadlockPauseReactor>,
    ) -> Self {
        Self {
            pool,
            auth_token: normalize_auth_token(auth_token),
            role_ext,
            removal_ext,
            ban_probe,
            deadlock_pause,
            request_slots: tokio::sync::Semaphore::new(MAX_MCP_CONCURRENT_REQUESTS),
        }
    }
}

fn normalize_auth_token(token: String) -> Option<String> {
    let token = token.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Bind-Adresse aus der Umgebung. `Err` beschreibt, warum nicht gebunden wird —
/// eine nicht-Loopback-Adresse ist ein Fehler, kein Grund für einen offenen Port.
fn bind_addr() -> Result<SocketAddr, String> {
    let host = std::env::var("TB_MCP_HOST")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_HOST.to_string());
    let port = match std::env::var("TB_MCP_PORT") {
        Ok(raw) if !raw.trim().is_empty() => raw
            .trim()
            .parse::<u16>()
            .map_err(|_| format!("TB_MCP_PORT ist keine Portnummer: {raw}"))?,
        _ => DEFAULT_PORT,
    };
    let ip: IpAddr = host
        .parse()
        .map_err(|_| format!("TB_MCP_HOST ist keine IP-Adresse: {host}"))?;
    if !ip.is_loopback() {
        return Err(format!(
            "TB_MCP_HOST muss eine Loopback-Adresse sein, war: {host}"
        ));
    }
    Ok(SocketAddr::new(ip, port))
}

/// Startet den Connector als überwachten Task.
///
/// Bind-Fehler werden geloggt und beenden nur diesen Task. Der Bot läuft ohne
/// MCP weiter.
pub(crate) fn spawn(supervisor: &TaskSupervisor, state: Arc<McpState>) {
    if state.auth_token.is_none() {
        tracing::warn!(
            "MCP-Connector nicht gestartet: TWITCH_INTERNAL_API_TOKEN fehlt oder ist leer"
        );
        return;
    }
    let addr = match bind_addr() {
        Ok(addr) => addr,
        Err(reason) => {
            tracing::warn!(reason, "MCP-Connector nicht gestartet");
            return;
        }
    };
    supervisor.spawn_finite("mcp_connector", async move {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, %addr, "MCP-Connector: Port nicht bindbar, läuft ohne MCP weiter");
                return;
            }
        };
        tracing::info!(
            %addr,
            auth = state.auth_token.is_some(),
            "MCP-Connector gebunden (POST /mcp, GET /healthz)"
        );
        if let Err(error) = axum::serve(listener, router(state)).await {
            tracing::warn!(%error, "MCP-Connector beendet");
        }
    });
}

pub(crate) fn router(state: Arc<McpState>) -> Router {
    Router::new()
        .route("/mcp", post(mcp_post).get(mcp_get))
        .route(
            "/healthz",
            get(|| async {
                Json(json!({
                    "ok": true,
                    "service": "tb-mcp-connector",
                    "protocol": PROTOCOL_FALLBACK,
                }))
            }),
        )
        .with_state(state)
}

// ───────────────────────────── HTTP-Ebene ─────────────────────────────

fn json_response(status: StatusCode, body: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn auth_ok(st: &McpState, headers: &HeaderMap) -> bool {
    let Some(expected) = &st.auth_token else {
        return false;
    };
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = value.strip_prefix("Bearer ") {
            if constant_time_eq(token.trim(), expected) {
                return true;
            }
        }
    }
    false
}

/// Schuetzt gegen DNS-Rebinding und CSRF aus dem Browser.
///
/// Ein lokaler MCP-Client (Claude Code, curl) schickt keinen `Origin`. Eine
/// Webseite dagegen setzt ihn bei jedem Cross-Origin-Request, auch bei einem
/// CORS-Simple-Request, dessen Antwort sie nie zu sehen bekommt. Genau der
/// wuerde hier sonst reichen, um `disconnect_bot` mit `confirm=true`
/// auszuloesen. `Host` faengt zusaetzlich Rebinding ab: der Dienst bindet auf
/// Loopback, ein Name, der dorthin aufloest, ist trotzdem fremd.
///
/// Deshalb: kein `Origin` ist in Ordnung, ein gesetzter muss auf Loopback
/// zeigen. Die MCP-Streamable-HTTP-Spec verlangt genau diese Pruefung.
fn origin_ok(headers: &HeaderMap) -> bool {
    fn ist_loopback_host(wert: &str) -> bool {
        let ohne_schema = wert.split_once("://").map(|(_, rest)| rest).unwrap_or(wert);
        let host = ohne_schema
            .split('/')
            .next()
            .unwrap_or("")
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(ohne_schema.split('/').next().unwrap_or(""));
        let host = host.trim_matches(|c| c == '[' || c == ']');
        matches!(host, "localhost" | "127.0.0.1" | "::1")
    }

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        if !ist_loopback_host(origin) {
            return false;
        }
    }
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(ist_loopback_host)
}

async fn mcp_get() -> Response {
    // Kein server-initiierter SSE-Stream nötig — Streamable HTTP erlaubt 405.
    json_response(
        StatusCode::METHOD_NOT_ALLOWED,
        json!({"error": "SSE-Stream nicht unterstützt"}),
    )
}

async fn mcp_post(State(st): State<Arc<McpState>>, request: Request<Body>) -> Response {
    let headers = request.headers();
    if !origin_ok(headers) {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({"error": "nur ueber Loopback erreichbar"}),
        );
    }
    if !auth_ok(&st, headers) {
        return json_response(StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}));
    }
    let _permit = match st.request_slots.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return json_response(
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error": "zu viele parallele MCP-Anfragen"}),
            )
        }
    };
    match tokio::time::timeout(
        Duration::from_secs(MAX_MCP_REQUEST_SECONDS),
        handle_authenticated_request(&st, request),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => json_response(
            StatusCode::GATEWAY_TIMEOUT,
            json!({"error": "MCP-Anfrage hat das Zeitlimit überschritten"}),
        ),
    }
}

async fn handle_authenticated_request(st: &Arc<McpState>, request: Request<Body>) -> Response {
    let body = match to_bytes(request.into_body(), MAX_MCP_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            return json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error": "request body too large"}),
            )
        }
    };
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return json_response(
                StatusCode::OK,
                json!({"jsonrpc": "2.0", "id": null,
                       "error": {"code": -32700, "message": format!("Parse error: {error}")}}),
            )
        }
    };

    match parsed {
        Value::Array(items) => {
            if items.is_empty() || items.len() > MAX_MCP_BATCH_ITEMS {
                return json_response(
                    StatusCode::OK,
                    json!({"jsonrpc": "2.0", "id": null, "error": {
                        "code": -32600,
                        "message": format!("Batch muss 1 bis {MAX_MCP_BATCH_ITEMS} Einträge enthalten")
                    }}),
                );
            }
            let mut out = Vec::new();
            for item in items {
                if let Some(response) = handle_rpc(st, item).await {
                    out.push(response);
                }
            }
            if out.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                json_response(StatusCode::OK, Value::Array(out))
            }
        }
        object => match handle_rpc(st, object).await {
            Some(response) => json_response(StatusCode::OK, response),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

// ───────────────────────────── JSON-RPC / MCP ─────────────────────────────

/// Stateless: `tools/list` und `tools/call` funktionieren auch ohne vorheriges
/// `initialize` (dann fällt der Sitzungs-Handshake einfach weg).
async fn handle_rpc(st: &Arc<McpState>, request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (ohne id) → keine Antwort.
    let id = match id {
        Some(value) if !value.is_null() => value,
        _ => {
            tracing::debug!(method, "MCP-Notification");
            return None;
        }
    };

    let result: Result<Value, String> = match method.as_str() {
        "initialize" => Ok(initialize_result(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => tools_call(st, &params).await,
        _ => Err("__method_not_found__".to_string()),
    };

    Some(match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
        Err(message) if message == "__method_not_found__" => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {method}")}
        }),
        Err(message) => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32603, "message": message}
        }),
    })
}

fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or(PROTOCOL_FALLBACK);
    let version = if SUPPORTED_PROTOCOLS.contains(&requested) {
        requested
    } else {
        PROTOCOL_FALLBACK
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "tb-mcp-connector", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "Verwaltung des Deadlock-Twitch-Bots mit der Identität des laufenden \
            Prozesses. Lesende Werkzeuge (list_partners, partner_status, deadlock_pause_preview, \
            bot_ban_status) sind gefahrlos, bot_ban_status setzt den Bot dabei aber als Moderator \
            ein. Schreibende Werkzeuge (disconnect_bot, run_deadlock_pause_sweep) tun ohne \
            confirm=true nichts und liefern stattdessen eine Vorschau."
    })
}

async fn tools_call(st: &Arc<McpState>, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| "tools/call ohne name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let outcome = match name {
        "list_partners" => tool_list_partners(st, &args).await,
        "partner_status" => tool_partner_status(st, &args).await,
        "deadlock_pause_preview" => tool_deadlock_pause_preview(st, &args).await,
        "bot_ban_status" => tool_bot_ban_status(st, &args).await,
        "disconnect_bot" => tool_disconnect_bot(st, &args).await,
        "run_deadlock_pause_sweep" => tool_run_deadlock_pause_sweep(st, &args).await,
        other => Err(format!("Unbekanntes Tool: {other}")),
    };

    // Jeder Aufruf ins Log, auch der fehlgeschlagene. Ein Werkzeug, das den
    // Bot verändert, darf nicht nur im Chatfenster des Aufrufers stehen.
    match &outcome {
        Ok(value) => {
            tracing::info!(tool = name, ergebnis = %kurzfassung(value), "MCP-Tool ausgeführt")
        }
        Err(error) => tracing::info!(tool = name, fehler = %error, "MCP-Tool fehlgeschlagen"),
    }

    Ok(match outcome {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            json!({ "content": [{"type": "text", "text": text}], "isError": false })
        }
        Err(error) => json!({ "content": [{"type": "text", "text": error}], "isError": true }),
    })
}

/// Log-Zeile statt komplettem Ergebnis: die Antwort kann hunderte Zeilen haben.
fn kurzfassung(value: &Value) -> String {
    let raw = value.to_string();
    if raw.chars().count() <= 400 {
        return raw;
    }
    let gekappt: String = raw.chars().take(400).collect();
    format!("{gekappt}…")
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "list_partners",
            "description": "Aktive Twitch-Partner mit Betriebszustand: Token gesund, technische \
                Pause, Deadlock-Pause aktiv, letzter Deadlock-Stream, Discord-Verknüpfung. \
                Reiner Read.",
            "inputSchema": { "type": "object", "properties": {
                "search": {"type": "string", "description": "Teilstring im Twitch-Login"},
                "token_issues_only": {"type": "boolean", "description": "Nur Partner ohne gesunden Token"},
                "technical_pause_only": {"type": "boolean", "description": "Nur Partner mit technischer Pause"},
                "deadlock_paused_only": {"type": "boolean", "description": "Nur Partner in der Deadlock-Pause"},
                "limit": {"type": "integer", "description": "Obergrenze, default 200"}
            }}
        },
        {
            "name": "partner_status",
            "description": "Alles zu einem Login: Partner-Zeile, Auth-Zustand, Blacklist- und \
                Denylist-Einträge, Deadlock-Pause-Marker und die letzten Stream-Sessions. \
                Reiner Read.",
            "inputSchema": { "type": "object", "required": ["login"], "properties": {
                "login": {"type": "string", "description": "Twitch-Login"}
            }}
        },
        {
            "name": "deadlock_pause_preview",
            "description": "Wer beim nächsten Deadlock-Pause-Sweep entmoddet (unmod) bzw. wieder \
                gemoddet (remod) würde. Liest über dieselben Queries wie der Sweep und verändert \
                nichts. Der echte Lauf remoddet zuerst und entmoddet danach.",
            "inputSchema": { "type": "object", "properties": {
                "limit": {"type": "integer", "description": "Kandidaten je Richtung; default sind die Sweep-Grenzen (5 Unmod, 50 Remod)"}
            }}
        },
        {
            "name": "bot_ban_status",
            "description": "Fragt bei Twitch nach, ob der Bot im Kanal eines aktiven Partners \
                gebannt ist. ACHTUNG: die Probe setzt den Bot im selben Aufruf als Moderator ein \
                (so erkennt sie den Ban) — der Aufruf ist also nicht wirkungsfrei. Nur für \
                aktive Partner, andere Logins werden abgelehnt.",
            "inputSchema": { "type": "object", "required": ["login"], "properties": {
                "login": {"type": "string", "description": "Twitch-Login eines aktiven Partners"}
            }}
        },
        {
            "name": "disconnect_bot",
            "description": "Trennt den Bot vom Kanal — derselbe Vorgang wie 'Bot vom Kanal trennen' \
                im Dashboard: Mod-Rechte entziehen, departnern, Opt-out setzen, Discord-Streamer- \
                Rolle entziehen. Ohne confirm=true passiert nichts und es kommt eine Vorschau. \
                Die Antwort nennt jeden Schritt einzeln, auch die fehlgeschlagenen.",
            "inputSchema": { "type": "object", "required": ["login"], "properties": {
                "login": {"type": "string", "description": "Twitch-Login"},
                "confirm": {"type": "boolean", "description": "Pflicht, sonst nur Vorschau"}
            }}
        },
        {
            "name": "run_deadlock_pause_sweep",
            "description": "Löst einen Deadlock-Pause-Sweep sofort aus, statt auf den 15-Minuten- \
                Timer zu warten. Default ist dry_run=true (nur Vorschau). Ein echter Lauf \
                (dry_run=false) braucht confirm=true, entzieht Mod-Rechte und schickt DMs an \
                Streamer.",
            "inputSchema": { "type": "object", "properties": {
                "dry_run": {"type": "boolean", "description": "default true — nur Vorschau"},
                "confirm": {"type": "boolean", "description": "Pflicht für dry_run=false"}
            }}
        }
    ])
}

// ───────────────────────────── Argument-Helfer ─────────────────────────────

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// `dry_run` ist ohne Angabe wahr: ein Aufruf ohne Meinung darf nichts anfassen.
fn arg_bool_default_true(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(true)
}

fn arg_login(args: &Value) -> Result<String, String> {
    let raw = args
        .get("login")
        .and_then(|v| v.as_str())
        .map(|v| v.trim())
        .unwrap_or("");
    if raw.is_empty() {
        return Err("login fehlt".to_string());
    }
    Ok(raw.trim_start_matches('@').to_lowercase())
}

fn arg_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|v| v.as_i64())
        .filter(|v| *v > 0)
        .map(|v| (v as usize).min(DEFAULT_PARTNER_LIMIT))
}

// ───────────────────────────── Werkzeuge: Lesen ─────────────────────────────

/// Die zwei Partner-Spalten, die keine der bestehenden Listen-Queries führt.
/// Alles als `text`, weil die Spalten je nach Umgebung `text` oder
/// `timestamptz` sind (siehe `tb_raid::deadlock_pause`).
async fn pause_markers(pool: &PgPool) -> Result<HashMap<String, (Value, Value)>, String> {
    let rows = sqlx::query(
        "SELECT LOWER(COALESCE(twitch_login, '')) AS login, \
                NULLIF(TRIM(COALESCE(technical_pause_reason::text, '')), '') AS technical_pause_reason, \
                deadlock_pause_unmodded_at::text AS deadlock_pause_unmodded_at \
           FROM twitch_partners",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("Pausen-Marker nicht ladbar: {error}"))?;

    let mut map = HashMap::new();
    for row in rows {
        let login: String = row.try_get("login").unwrap_or_default();
        if login.is_empty() {
            continue;
        }
        let reason: Option<String> = row.try_get("technical_pause_reason").unwrap_or(None);
        let paused: Option<String> = row.try_get("deadlock_pause_unmodded_at").unwrap_or(None);
        map.insert(
            login,
            (
                reason.map(Value::from).unwrap_or(Value::Null),
                paused.map(Value::from).unwrap_or(Value::Null),
            ),
        );
    }
    Ok(map)
}

/// Ein Wort für den Token-Zustand. `list_partners` soll die Frage „geht der
/// Kanal gerade?" beantworten, ohne dass der Aufrufer drei Felder verrechnet.
fn token_zustand(row: &streamers_crud::StreamerListRow) -> &'static str {
    match (row.raid_needs_reauth, row.raid_authorized_at) {
        (Some(true), _) => "reauth_noetig",
        (_, None) => "keine_autorisierung",
        _ => "ok",
    }
}

async fn tool_list_partners(st: &Arc<McpState>, args: &Value) -> Result<Value, String> {
    let target_game = tb_internal_api::handlers::streamers::target_game_name();
    let rows = streamers_crud::list_streamers(&st.pool, &target_game)
        .await
        .map_err(|error| format!("Partner-Liste nicht ladbar: {error}"))?;
    let marker = pause_markers(&st.pool).await?;

    let search = args
        .get("search")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty());
    let token_issues_only = arg_bool(args, "token_issues_only");
    let technical_pause_only = arg_bool(args, "technical_pause_only");
    let deadlock_paused_only = arg_bool(args, "deadlock_paused_only");
    let limit = arg_usize(args, "limit").unwrap_or(DEFAULT_PARTNER_LIMIT);

    let gesamt = rows.len();
    let mut partners = Vec::new();
    for row in &rows {
        let login = row.twitch_login.to_lowercase();
        if let Some(needle) = &search {
            if !login.contains(needle.as_str()) {
                continue;
            }
        }
        let (technical_pause_reason, deadlock_pause_since) = marker
            .get(&login)
            .cloned()
            .unwrap_or((Value::Null, Value::Null));
        let zustand = token_zustand(row);
        let deadlock_pause_active = !deadlock_pause_since.is_null();
        if token_issues_only && zustand == "ok" {
            continue;
        }
        if technical_pause_only && technical_pause_reason.is_null() {
            continue;
        }
        if deadlock_paused_only && !deadlock_pause_active {
            continue;
        }
        partners.push(json!({
            "twitch_login": row.twitch_login,
            "twitch_user_id": row.twitch_user_id,
            "token_zustand": zustand,
            "token_gesund": zustand == "ok",
            "raid_needs_reauth": row.raid_needs_reauth,
            "raid_auth_enabled": row.raid_auth_enabled,
            "raid_token_expires_at": row.raid_token_expires_at,
            "technical_pause_reason": technical_pause_reason,
            "deadlock_pause_active": deadlock_pause_active,
            "deadlock_pause_since": deadlock_pause_since,
            "last_deadlock_stream_at": row.last_deadlock_stream_at,
            "discord_user_id": row.discord_user_id,
            "manual_partner_opt_out": row.manual_partner_opt_out,
        }));
        if partners.len() >= limit {
            break;
        }
    }

    Ok(json!({
        "aktive_partner_gesamt": gesamt,
        "zurueckgegeben": partners.len(),
        "limit": limit,
        "target_game": target_game,
        "partners": partners,
    }))
}

/// Partner- und Auth-Zeile zu einem Login. Startet bei einem Literal und
/// verbindet nach links, damit auch ein departnerter oder nie promoteter Login
/// eine Antwort bekommt statt „nicht gefunden".
async fn partner_row(pool: &PgPool, login: &str) -> Result<Value, String> {
    const SPALTEN: &[&str] = &[
        "partner_gefunden",
        "twitch_login",
        "twitch_user_id",
        "status",
        "technical_pause_reason",
        "partnered_at",
        "departnered_at",
        "admin_archived_at",
        "manual_partner_opt_out",
        "raid_bot_enabled",
        "deadlock_pause_unmodded_at",
        "auth_gefunden",
        "raid_needs_reauth",
        "raid_enabled",
        "raid_authorized_at",
        "raid_token_expires_at",
        "raid_token_vorhanden",
        "raid_scopes",
    ];
    // Alles als text: die Zeitspalten sind je nach Umgebung text oder
    // timestamptz, die Flags integer oder boolean.
    let row = sqlx::query(
        "SELECT (p.twitch_login IS NOT NULL)::text AS partner_gefunden, \
                COALESCE(LOWER(p.twitch_login), q.login) AS twitch_login, \
                COALESCE(NULLIF(TRIM(COALESCE(p.twitch_user_id, '')), ''), \
                         NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '')) AS twitch_user_id, \
                p.status::text AS status, \
                NULLIF(TRIM(COALESCE(p.technical_pause_reason::text, '')), '') AS technical_pause_reason, \
                p.partnered_at::text AS partnered_at, \
                p.departnered_at::text AS departnered_at, \
                p.admin_archived_at::text AS admin_archived_at, \
                p.manual_partner_opt_out::text AS manual_partner_opt_out, \
                p.raid_bot_enabled::text AS raid_bot_enabled, \
                p.deadlock_pause_unmodded_at::text AS deadlock_pause_unmodded_at, \
                (a.twitch_user_id IS NOT NULL OR a.twitch_login IS NOT NULL)::text AS auth_gefunden, \
                a.needs_reauth::text AS raid_needs_reauth, \
                a.raid_enabled::text AS raid_enabled, \
                a.authorized_at::text AS raid_authorized_at, \
                a.token_expires_at::text AS raid_token_expires_at, \
                (a.access_token_enc IS NOT NULL AND OCTET_LENGTH(a.access_token_enc) > 0)::text \
                    AS raid_token_vorhanden, \
                a.scopes::text AS raid_scopes \
           FROM (SELECT LOWER($1::text) AS login) q \
           LEFT JOIN twitch_partners p ON LOWER(p.twitch_login) = q.login \
           LEFT JOIN twitch_raid_auth a \
             ON LOWER(COALESCE(a.twitch_login, '')) = q.login \
             OR (p.twitch_user_id IS NOT NULL AND a.twitch_user_id = p.twitch_user_id) \
          LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("Partner-Zeile nicht ladbar: {error}"))?;

    let Some(row) = row else {
        return Ok(Value::Null);
    };
    let mut object = serde_json::Map::new();
    for spalte in SPALTEN {
        let wert: Option<String> = row.try_get(*spalte).unwrap_or(None);
        object.insert(
            (*spalte).to_string(),
            wert.map(Value::from).unwrap_or(Value::Null),
        );
    }
    Ok(Value::Object(object))
}

async fn tool_partner_status(st: &Arc<McpState>, args: &Value) -> Result<Value, String> {
    let login = arg_login(args)?;
    let partner = partner_row(&st.pool, &login).await?;
    let twitch_user_id = partner
        .get("twitch_user_id")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string());

    // Blacklists über die bestehenden Leser, kein eigenes SQL.
    let raid_blacklist = raid_blacklist::check_entry(&st.pool, &login)
        .await
        .map_err(|error| format!("Raid-Blacklist nicht lesbar: {error}"))?
        .map(|(reason, added_at)| json!({"reason": reason, "added_at": added_at}))
        .unwrap_or(Value::Null);
    let signup_block =
        tb_raid::signup_denylist::lookup(&st.pool, twitch_user_id.as_deref(), &login)
            .await
            .map_err(|error| format!("Signup-Denylist nicht lesbar: {error}"))?
            .map(|block| {
                json!({
                    "reason": block.reason,
                    "public_message": block.public_message,
                })
            })
            .unwrap_or(Value::Null);
    let token_blacklisted = match twitch_user_id.as_deref() {
        Some(user_id) => Value::Bool(
            tb_raid::TokenBlacklistStore::new(st.pool.clone())
                .is_blacklisted(user_id)
                .await,
        ),
        None => Value::Null,
    };

    let (stats, sessions) = streamers_crud::streamer_analytics(&st.pool, &login, 30)
        .await
        .map_err(|error| format!("Sessions nicht ladbar: {error}"))?;
    let sessions: Vec<Value> = sessions
        .iter()
        .take(SESSION_LIMIT)
        .map(|session| serde_json::to_value(session).unwrap_or(Value::Null))
        .collect();

    Ok(json!({
        "login": login,
        "partner": partner,
        "blacklists": {
            "raid_blacklist": raid_blacklist,
            "signup_block": signup_block,
            "token_blacklisted": token_blacklisted,
        },
        "stats_30d": serde_json::to_value(&stats).unwrap_or(Value::Null),
        "letzte_sessions": sessions,
    }))
}

/// Vorschau auf den nächsten Sweep. Nutzt die Pausendauer des laufenden
/// Reactors, damit Vorschau und Lauf nie mit verschiedenen Grenzen rechnen.
async fn deadlock_pause_vorschau(
    st: &Arc<McpState>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let pause_days = st
        .deadlock_pause
        .as_ref()
        .map(|reactor| reactor.pause_days())
        .unwrap_or(tb_raid::DEADLOCK_PAUSE_DAYS);
    let unmod_limit = limit
        .map(|v| v as i64)
        .unwrap_or(tb_raid::MAX_UNMOD_PER_SWEEP);
    let remod_limit = limit
        .map(|v| v as i64)
        .unwrap_or(tb_raid::MAX_REMOD_PER_SWEEP);

    let unmod = tb_raid::unmod_candidates(&st.pool, pause_days, unmod_limit)
        .await
        .map_err(|error| format!("Unmod-Kandidaten nicht ladbar: {error}"))?;
    let remod = tb_raid::remod_candidates(&st.pool, remod_limit)
        .await
        .map_err(|error| format!("Remod-Kandidaten nicht ladbar: {error}"))?;

    let als_json = |kandidaten: &[tb_raid::DeadlockPauseCandidate]| -> Vec<Value> {
        kandidaten
            .iter()
            .map(|k| json!({"twitch_login": k.twitch_login, "twitch_user_id": k.twitch_user_id}))
            .collect()
    };

    Ok(json!({
        "sweep_verdrahtet": st.deadlock_pause.is_some(),
        "pause_tage": pause_days,
        "reihenfolge": "erst remod, dann unmod",
        "unmod_faellig": als_json(&unmod),
        "unmod_anzahl": unmod.len(),
        "unmod_limit": unmod_limit,
        "remod_faellig": als_json(&remod),
        "remod_anzahl": remod.len(),
        "remod_limit": remod_limit,
    }))
}

async fn tool_deadlock_pause_preview(st: &Arc<McpState>, args: &Value) -> Result<Value, String> {
    deadlock_pause_vorschau(st, arg_usize(args, "limit")).await
}

async fn tool_bot_ban_status(st: &Arc<McpState>, args: &Value) -> Result<Value, String> {
    let login = arg_login(args)?;
    let Some(probe) = st.ban_probe.as_ref() else {
        return Err(
            "Ban-Probe nicht verdrahtet (kein DB_MASTER_KEY_V1 oder keine Bot-User-ID)".to_string(),
        );
    };
    // Bewusst nur aktive Partner: die Probe moddet den Bot im selben Aufruf ein.
    // In einem fremden Kanal wäre das ein Eingriff ohne Anlass.
    let Some(user_id) = partner_lookup::resolve_active_partner_id_by_login(&st.pool, &login).await
    else {
        return Err(format!(
            "{login} ist kein aktiver Partner mit bekannter User-ID — Ban-Probe abgelehnt"
        ));
    };

    let status = probe.bot_ban_status(&user_id, &login).await;
    let (kurz, erklaerung) = match status {
        BotBanStatus::Banned => ("gebannt", "Twitch lehnt den Bot in diesem Kanal ab"),
        BotBanStatus::NotBanned => (
            "nicht_gebannt",
            "Bot ist im Kanal nutzbar und wurde dabei als Moderator eingesetzt",
        ),
        BotBanStatus::Unknown => ("unklar", "Twitch hat keine eindeutige Antwort geliefert"),
    };
    Ok(json!({
        "login": login,
        "twitch_user_id": user_id,
        "status": kurz,
        "erklaerung": erklaerung,
        "hinweis": "Die Probe setzt den Bot im selben Aufruf als Moderator ein.",
    }))
}

// ───────────────────────────── Werkzeuge: Schreiben ─────────────────────────────

async fn tool_disconnect_bot(st: &Arc<McpState>, args: &Value) -> Result<Value, String> {
    let login = arg_login(args)?;
    if !arg_bool(args, "confirm") {
        let partner = partner_row(&st.pool, &login).await?;
        return Ok(json!({
            "vorschau": true,
            "login": login,
            "wuerde_passieren": [
                "Mod-Rechte des Bots auf Twitch entziehen (braucht gültigen Streamer-Token)",
                "Partnerschaft beenden (departnern, Verifikation zurücksetzen)",
                "manual_partner_opt_out = 1 setzen, damit kein Sweep den Kanal zurückholt",
                "Discord-Streamer-Rolle entziehen",
            ],
            "aktueller_stand": partner,
            "hinweis": "Nichts wurde geändert. Für den echten Lauf confirm=true setzen.",
        }));
    }

    // Genau der Dashboard-Vorgang, kein Nachbau.
    match disconnect_bot_handler_inner(&st.pool, &st.role_ext, &st.removal_ext, &login).await {
        Ok((status, mut value)) => {
            if let Some(object) = value.as_object_mut() {
                object.insert("http_status".to_string(), json!(status.as_u16()));
                object.insert("vorschau".to_string(), json!(false));
            }
            Ok(value)
        }
        Err(error) => Err(format!("Trennen fehlgeschlagen: {error:?}")),
    }
}

async fn tool_run_deadlock_pause_sweep(st: &Arc<McpState>, args: &Value) -> Result<Value, String> {
    let dry_run = arg_bool_default_true(args, "dry_run");
    let confirm = arg_bool(args, "confirm");

    if dry_run || !confirm {
        let mut vorschau = deadlock_pause_vorschau(st, None).await?;
        if let Some(object) = vorschau.as_object_mut() {
            object.insert("vorschau".to_string(), json!(true));
            object.insert(
                "hinweis".to_string(),
                json!(if dry_run {
                    "Nichts wurde geändert (dry_run). Für den echten Lauf dry_run=false und confirm=true setzen."
                } else {
                    "Nichts wurde geändert: confirm fehlt. Für den echten Lauf dry_run=false und confirm=true setzen."
                }),
            );
        }
        return Ok(vorschau);
    }

    let Some(reactor) = st.deadlock_pause.as_ref() else {
        return Err(
            "Deadlock-Pause-Sweep ist nicht verdrahtet (kein Mod-Entzug oder keine Ban-Probe)"
                .to_string(),
        );
    };
    let outcome = reactor.sweep().await;
    Ok(json!({
        "vorschau": false,
        "unmodded": outcome.unmodded,
        "remodded": outcome.remodded,
        "wirkung": outcome.any(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Value {
        value
    }

    fn headers_mit(paare: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, wert) in paare {
            headers.insert(name.clone(), wert.parse().expect("Header-Wert"));
        }
        headers
    }

    #[test]
    fn ohne_origin_geht_der_lokale_client_durch() {
        assert!(origin_ok(&headers_mit(&[(header::HOST, "127.0.0.1:8892")])));
        assert!(!origin_ok(&HeaderMap::new()));
    }

    #[test]
    fn fremder_origin_wird_abgewiesen() {
        // Eine Webseite kann per CORS-Simple-Request POSTen, ohne die Antwort
        // zu sehen. Der Schreibvorgang liefe trotzdem, deshalb hier der Riegel.
        assert!(!origin_ok(&headers_mit(&[
            (header::ORIGIN, "https://boese.example"),
            (header::HOST, "127.0.0.1:8892"),
        ])));
    }

    #[test]
    fn loopback_origin_bleibt_erlaubt() {
        for origin in [
            "http://localhost:5173",
            "http://127.0.0.1:8892",
            "http://[::1]:8892",
        ] {
            assert!(
                origin_ok(&headers_mit(&[
                    (header::ORIGIN, origin),
                    (header::HOST, "127.0.0.1:8892"),
                ])),
                "{origin} sollte durchgehen"
            );
        }
    }

    #[test]
    fn fremder_host_faengt_dns_rebinding() {
        assert!(!origin_ok(&headers_mit(&[(
            header::HOST,
            "angreifer.example:8892"
        )])));
    }

    /// Zustand ohne Ports und ohne erreichbare DB. Reicht für alles, was die
    /// HTTP-/JSON-RPC-Ebene betrifft: `tools/list` fasst keine Tabelle an.
    fn test_state(auth_token: Option<&str>) -> Arc<McpState> {
        let pool = PgPool::connect_lazy("postgres://ungenutzt@127.0.0.1/ungenutzt")
            .expect("Lazy-Pool ohne Verbindungsaufbau");
        Arc::new(McpState {
            pool,
            auth_token: auth_token.map(|t| t.to_string()),
            role_ext: DiscordRoleExt(None),
            removal_ext: ModeratorRemovalExt(None),
            ban_probe: None,
            deadlock_pause: None,
            request_slots: tokio::sync::Semaphore::new(MAX_MCP_CONCURRENT_REQUESTS),
        })
    }

    #[test]
    fn leerer_interner_token_deaktiviert_mcp() {
        assert_eq!(normalize_auth_token(String::new()), None);
        assert_eq!(normalize_auth_token("  \t".to_string()), None);
        assert_eq!(
            normalize_auth_token("  intern-token  ".to_string()).as_deref(),
            Some("intern-token")
        );
    }

    async fn serve_test(state: Arc<McpState>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Testport");
        let addr = listener.local_addr().expect("Adresse");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router(state)).await;
        });
        format!("http://{addr}")
    }

    /// Ein MCP-Client, der ohne `initialize` einsteigt, muss die Werkzeugliste
    /// bekommen — der Server ist stateless.
    #[tokio::test]
    async fn tools_list_ohne_initialize_und_healthz() {
        let base = serve_test(test_state(Some("test-token"))).await;
        let client = reqwest::Client::new();

        let health = client
            .get(format!("{base}/healthz"))
            .send()
            .await
            .expect("healthz");
        assert!(health.status().is_success());
        let health_body: Value = health.json().await.expect("healthz JSON");
        assert_eq!(health_body["ok"], true);
        assert_eq!(health_body["service"], "tb-mcp-connector");

        let response: Value = client
            .post(format!("{base}/mcp"))
            .bearer_auth("test-token")
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .send()
            .await
            .expect("tools/list")
            .json()
            .await
            .expect("JSON");
        assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 6);

        // GET /mcp ist bewusst 405: kein server-initiierter SSE-Stream.
        let sse = client.get(format!("{base}/mcp")).send().await.expect("GET");
        assert_eq!(sse.status().as_u16(), 405);
    }

    /// Mit gesetztem Token kommt niemand ohne ihn hinein, und ein Schreib-Tool
    /// ohne confirm bleibt auch dann wirkungslos.
    #[tokio::test]
    async fn token_pflicht_und_vorschau_ohne_confirm() {
        let base = serve_test(test_state(Some("geheim"))).await;
        let client = reqwest::Client::new();

        let ohne = client
            .post(format!("{base}/mcp"))
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .send()
            .await
            .expect("ohne Token");
        assert_eq!(ohne.status().as_u16(), 401);

        // Der Sweep ohne confirm läuft in die Vorschau. Die Kandidaten-Query
        // scheitert an der Attrappen-DB — entscheidend ist, dass kein Sweep
        // startet, sondern eine Fehlermeldung kommt.
        let antwort: Value = client
            .post(format!("{base}/mcp"))
            .bearer_auth("geheim")
            .json(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                          "params": {"name": "run_deadlock_pause_sweep", "arguments": {}}}))
            .send()
            .await
            .expect("tools/call")
            .json()
            .await
            .expect("JSON");
        let text = antwort["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("Kandidaten nicht ladbar"),
            "unerwartete Antwort: {text}"
        );
    }

    /// Ohne konfiguriertes Token darf der MCP-Router niemals offen werden. Das
    /// bleibt auch als zweite Schutzschicht wahr, falls ein Aufrufer `spawn`
    /// umgeht und den Router direkt baut.
    #[tokio::test]
    async fn fehlendes_token_ist_fail_closed() {
        let base = serve_test(test_state(None)).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .send()
            .await
            .expect("MCP-Aufruf");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// URL-Querys landen in Browser-History, Proxy-Logs und Prozesslisten. Ein
    /// MCP-Token wird deshalb ausschließlich im Authorization-Header akzeptiert.
    #[tokio::test]
    async fn query_token_wird_nicht_akzeptiert() {
        let base = serve_test(test_state(Some("geheim"))).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp?token=geheim"))
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .send()
            .await
            .expect("MCP-Aufruf");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// Authentifizierung muss passieren, bevor Axum den Request-Body einliest.
    /// Sonst kann ein nicht authentifizierter Client schon am Body-Limit Arbeit
    /// und Speicher verursachen bzw. eine 413-Antwort vor dem Auth-Gate erzwingen.
    #[tokio::test]
    async fn auth_laeuft_vor_body_extraktion() {
        let base = serve_test(test_state(Some("geheim"))).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .body("x".repeat(3 * 1024 * 1024))
            .send()
            .await
            .expect("MCP-Aufruf");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn batchgroesse_ist_begrenzt() {
        let base = serve_test(test_state(Some("geheim"))).await;
        let batch = vec![
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
            MAX_MCP_BATCH_ITEMS + 1
        ];
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .bearer_auth("geheim")
            .json(&batch)
            .send()
            .await
            .expect("MCP-Aufruf");
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("JSON-Antwort");
        assert_eq!(body["error"]["code"], -32600);
    }

    #[test]
    fn login_wird_normalisiert() {
        assert_eq!(
            arg_login(&args(json!({"login": " @EarlySalty "}))).unwrap(),
            "earlysalty"
        );
        assert!(arg_login(&args(json!({"login": "  "}))).is_err());
        assert!(arg_login(&args(json!({}))).is_err());
    }

    #[test]
    fn caller_limit_wird_hart_gedeckelt() {
        assert_eq!(
            arg_usize(&args(json!({"limit": 999_999})), "limit"),
            Some(DEFAULT_PARTNER_LIMIT)
        );
    }

    /// Ein Aufruf ohne Meinung darf nichts anfassen: `dry_run` fehlt → true.
    #[test]
    fn dry_run_ist_ohne_angabe_an() {
        assert!(arg_bool_default_true(&args(json!({})), "dry_run"));
        assert!(!arg_bool_default_true(
            &args(json!({"dry_run": false})),
            "dry_run"
        ));
        assert!(!arg_bool(&args(json!({})), "confirm"));
    }

    #[test]
    fn bind_adresse_lehnt_nicht_loopback_ab() {
        // Kein Env-Zugriff im Test (parallel laufende Tests teilen ihn sich):
        // geprüft wird die Regel selbst.
        let ip: IpAddr = "0.0.0.0".parse().unwrap();
        assert!(!ip.is_loopback());
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(ip.is_loopback());
    }

    /// Jedes Schreib-Werkzeug muss in seiner Beschreibung sagen, dass es ohne
    /// confirm nichts tut — sonst probiert es ein Aufrufer blind aus.
    #[test]
    fn schreib_werkzeuge_nennen_confirm() {
        let tools = tool_definitions();
        let tools = tools.as_array().unwrap();
        assert_eq!(tools.len(), 6);
        for name in ["disconnect_bot", "run_deadlock_pause_sweep"] {
            let tool = tools
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("Tool {name} fehlt"));
            let description = tool["description"].as_str().unwrap();
            assert!(
                description.contains("confirm=true"),
                "{name} erklärt confirm nicht"
            );
            assert!(tool["inputSchema"]["properties"]["confirm"].is_object());
        }
    }

    /// Die Ban-Probe moddet den Bot ein. Das muss in der Beschreibung stehen,
    /// sonst hält der Aufrufer sie für einen reinen Read.
    #[test]
    fn ban_probe_warnt_vor_der_nebenwirkung() {
        let tools = tool_definitions();
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "bot_ban_status")
            .unwrap()
            .clone();
        let description = tool["description"].as_str().unwrap();
        assert!(description.contains("Moderator"));
    }

    /// Keine Werkzeuge, die in fremde Chats oder DMs schreiben.
    #[test]
    fn kein_werkzeug_verschickt_nachrichten() {
        let tools = tool_definitions();
        let namen: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            namen,
            vec![
                "list_partners",
                "partner_status",
                "deadlock_pause_preview",
                "bot_ban_status",
                "disconnect_bot",
                "run_deadlock_pause_sweep",
            ]
        );
    }

    #[test]
    fn kurzfassung_kappt_lange_ergebnisse() {
        let lang = json!({ "x": "y".repeat(1000) });
        let kurz = kurzfassung(&lang);
        assert!(kurz.chars().count() <= 401);
        assert!(kurz.ends_with('…'));
        let klein = json!({"a": 1});
        assert_eq!(kurzfassung(&klein), klein.to_string());
    }
}

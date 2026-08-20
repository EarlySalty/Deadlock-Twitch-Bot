//! Proxy vom Streamer-Dashboard zu rs-relay. Das Relay-Secret bleibt serverseitig.
//!
//! Der Browser sieht nur `/twitch/api/v2/uplink/...`. Jede Anfrage wird hier mit
//! dem Shared Secret versehen und an `127.0.0.1:8891` weitergereicht; die
//! Streamer-Identitaet kommt aus der bestehenden Dashboard-Session und nie aus
//! der Anfrage. Die Admin-Endpunkte sind zusaetzlich auf den Admin-Modus
//! begrenzt, killende Aufrufe verlangen `confirm=true`.

// Die Helfer geben eine fertige `Response` als Fehler zurueck, damit jeder
// Handler sie mit `?` weiterreichen kann. `Response` ist gross, und clippy
// meldet das fuer jede synchrone Funktion mit diesem Rueckgabetyp. Ein Boxen
// wuerde an jeder Aufrufstelle ein `*` erzwingen, ohne dass es dem Leser oder
// der Laufzeit etwas braechte.
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_crypto::FieldCipher;
use tb_raid::RaidAuthStore;

use crate::auth::level::DashboardAuthLevel;

/// Loopback-Adresse des Relays, Abschnitt 13 der Spezifikation.
const DEFAULT_RELAY_BASE: &str = "http://127.0.0.1:8891";

/// Push-Adresse von Twitch. Fest, weil Twitch fuer alle Kanaele dieselbe nutzt.
const TWITCH_RTMP_URL: &str = "rtmp://live.twitch.tv/app";

/// Scope, den Twitch fuer `GET /streams/key` verlangt.
const STREAM_KEY_SCOPE: &str = "channel:read:stream_key";

/// Zugang zum Relay. Ohne Secret gibt es keinen.
pub(crate) struct RelayClient {
    base: String,
    secret: String,
}

impl RelayClient {
    /// Baut den Zugang aus rohen Werten. Ohne Secret gibt es `None`, damit der
    /// Aufrufer 503 melden kann statt unauthentifiziert ins Leere zu rufen.
    pub(crate) fn from_parts(base: Option<String>, secret: Option<String>) -> Option<Self> {
        let secret = secret
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        let base = base
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| DEFAULT_RELAY_BASE.to_string());
        Some(Self {
            base: base.trim_end_matches('/').to_string(),
            secret,
        })
    }

    fn from_env() -> Option<Self> {
        Self::from_parts(
            std::env::var("RS_RELAY_BASE_URL").ok(),
            std::env::var("RS_RELAY_API_SECRET").ok(),
        )
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, Response> {
        let url = format!("{}{path}", self.base);
        let client = reqwest::Client::new();
        let mut req = client
            .request(method, url)
            .header("X-Relay-Auth", self.secret.clone())
            .header("Accept", "application/json");
        if let Some(body) = body {
            req = req.json(&body);
        }
        let antwort = req.send().await.map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "Uplink antwortet nicht." })),
            )
                .into_response()
        })?;
        let status = antwort.status();
        let wert = antwort.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            return Err((code, Json(fehlertext(code, wert))).into_response());
        }
        Ok(wert)
    }
}

/// Macht aus einer Relay-Absage etwas, das im Dashboard lesbar ist.
///
/// Das Relay antwortet auf Fehler mit leerem Rumpf. Ohne diese Uebersetzung
/// stuende im Dashboard nichts, und der Streamer saehe eine leere Karte.
///
/// Der Text kommt zu dem, was das Relay geschickt hat, statt es zu ersetzen:
/// beim abgelehnten Beenden steckt die Auskunft, ob der Stream noch laeuft,
/// genau in diesen Feldern, und die Oberflaeche braucht sie.
fn fehlertext(code: StatusCode, roh: Value) -> Value {
    if roh.get("error").and_then(Value::as_str).is_some() {
        return roh;
    }
    let text = if beenden_laeuft_noch(&roh) {
        "Der Stream läuft noch. Wir haben das Beenden angesagt und warten auf die Bestätigung."
    } else {
        match code {
            StatusCode::BAD_REQUEST => {
                "Die Angaben passen so nicht zusammen. Schau Adresse und Schlüssel noch einmal an."
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "Uplink hat den Zugang abgelehnt.",
            StatusCode::NOT_FOUND => "Dazu gibt es gerade nichts zu sehen.",
            StatusCode::CONFLICT => "Das lässt sich so nicht ändern.",
            StatusCode::SERVICE_UNAVAILABLE => "Uplink ist gerade nicht bereit.",
            _ => "Uplink konnte das nicht ausführen.",
        }
    };
    let mut wert = roh;
    match wert.as_object_mut() {
        Some(obj) => {
            obj.insert("error".into(), json!(text));
            wert
        }
        None => json!({ "error": text }),
    }
}

/// Erkennt die Absage auf ein Beenden, bei dem der Stream noch laeuft.
fn beenden_laeuft_noch(roh: &Value) -> bool {
    roh.get("stopped").and_then(Value::as_bool) == Some(false) && roh.get("ended").is_some()
}

fn relay_client() -> Result<RelayClient, Response> {
    RelayClient::from_env().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Uplink ist noch nicht verbunden." })),
        )
            .into_response()
    })
}

/// Twitch-User-ID der laufenden Dashboard-Session, roh wie gespeichert.
fn partner_user_id(auth: &DashboardAuthLevel) -> Result<String, Response> {
    let raw = match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => twitch_user_id.as_str(),
        DashboardAuthLevel::Admin {
            actor: Some(actor), ..
        } => actor.twitch_user_id.as_str(),
        DashboardAuthLevel::Admin { actor: None } => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Für diesen Zugang fehlt die Twitch-Identität." })),
            )
                .into_response());
        }
        DashboardAuthLevel::None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response());
        }
    };
    let getrimmt = raw.trim();
    if getrimmt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Für diesen Zugang fehlt die Twitch-Identität." })),
        )
            .into_response());
    }
    Ok(getrimmt.to_string())
}

fn partner_id(auth: &DashboardAuthLevel) -> Result<i64, Response> {
    partner_user_id(auth)?.parse::<i64>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Für diesen Zugang fehlt die Twitch-Identität." })),
        )
            .into_response()
    })
}

/// Admin-Gate fuer die Verwaltungsendpunkte.
///
/// Ohne Session ist es 401, mit Partner-Session 403: sonst haette ein
/// eingeloggter Streamer keine Auskunft darueber, warum nichts passiert.
fn require_admin(auth: &DashboardAuthLevel) -> Result<(), Response> {
    match auth {
        DashboardAuthLevel::Admin { .. } => Ok(()),
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()),
        DashboardAuthLevel::Partner { .. } => Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Dieser Bereich gehört der Verwaltung." })),
        )
            .into_response()),
    }
}

// ---------------------------------------------------------------------------
// Streamer-Endpunkte
// ---------------------------------------------------------------------------

pub async fn me_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let wert = relay_client()?
        .call(
            reqwest::Method::GET,
            &format!("/v1/me?streamer_id={id}"),
            None,
        )
        .await?;
    Ok(Json(wert))
}

pub async fn waitlist_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let wert = relay_client()?
        .call(
            reqwest::Method::POST,
            &format!("/v1/me/waitlist?streamer_id={id}"),
            Some(json!({})),
        )
        .await?;
    Ok(Json(wert))
}

pub async fn get_destinations_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let wert = relay_client()?
        .call(
            reqwest::Method::GET,
            &format!("/v1/me/destinations?streamer_id={id}"),
            None,
        )
        .await?;
    Ok(Json(wert))
}

/// Ein Ziel aus dem Dashboard. Adresse und Schluessel kommen zusammen oder gar
/// nicht; ohne beides aendert der Aufruf nur die Profilwerte.
#[derive(Deserialize)]
pub struct DestinationBody {
    pub platform: String,
    #[serde(default)]
    pub rtmp_url: Option<String>,
    #[serde(default)]
    pub stream_key: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub fps: Option<i32>,
    #[serde(default)]
    pub bitrate_kbps: Option<i32>,
}

/// Baut den Rumpf, den `PUT /v1/me/destinations` erwartet.
fn destination_payload(id: i64, body: &DestinationBody) -> Value {
    let mut eintrag = json!({ "platform": body.platform });
    let obj = eintrag.as_object_mut().expect("objekt");
    if let (Some(url), Some(key)) = (body.rtmp_url.as_deref(), body.stream_key.as_deref()) {
        obj.insert("rtmp_url".into(), json!(url));
        obj.insert("stream_key".into(), json!(key));
    }
    for (name, wert) in [
        ("width", body.width),
        ("height", body.height),
        ("fps", body.fps),
        ("bitrate_kbps", body.bitrate_kbps),
    ] {
        if let Some(wert) = wert {
            obj.insert(name.into(), json!(wert));
        }
    }
    if let Some(enabled) = body.enabled {
        obj.insert("enabled".into(), json!(enabled));
    }
    json!({ "streamer_id": id, "destinations": [eintrag] })
}

pub async fn put_destination_handler(
    auth: DashboardAuthLevel,
    Json(body): Json<DestinationBody>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let hat_url = body
        .rtmp_url
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let hat_key = body
        .stream_key
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if hat_url != hat_key {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Adresse und Schlüssel gehören zusammen." })),
        )
            .into_response());
    }
    let wert = relay_client()?
        .call(
            reqwest::Method::PUT,
            "/v1/me/destinations",
            Some(destination_payload(id, &body)),
        )
        .await?;
    Ok(Json(wert))
}

pub async fn get_schedule_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let wert = relay_client()?
        .call(
            reqwest::Method::GET,
            &format!("/v1/me/schedule?streamer_id={id}"),
            None,
        )
        .await?;
    Ok(Json(wert))
}

#[derive(Deserialize)]
pub struct ScheduleEntryBody {
    pub starts_at: String,
    pub ends_at: String,
}

#[derive(Deserialize)]
pub struct ScheduleBody {
    #[serde(default)]
    pub entries: Vec<ScheduleEntryBody>,
}

pub async fn put_schedule_handler(
    auth: DashboardAuthLevel,
    Json(body): Json<ScheduleBody>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let entries: Vec<Value> = body
        .entries
        .iter()
        .map(|e| json!({ "starts_at": e.starts_at, "ends_at": e.ends_at }))
        .collect();
    let wert = relay_client()?
        .call(
            reqwest::Method::PUT,
            "/v1/me/schedule",
            Some(json!({ "streamer_id": id, "entries": entries })),
        )
        .await?;
    Ok(Json(wert))
}

#[derive(Deserialize)]
pub struct MetricsQuery {
    pub session: i64,
}

pub async fn metrics_handler(
    auth: DashboardAuthLevel,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let session = query.session;
    let wert = relay_client()?
        .call(
            reqwest::Method::GET,
            &format!("/v1/me/metrics?streamer_id={id}&session={session}"),
            None,
        )
        .await?;
    Ok(Json(wert))
}

// ---------------------------------------------------------------------------
// Twitch-Schluessel aus Helix
// ---------------------------------------------------------------------------

/// Holt den Twitch-Stream-Schluessel per Helix und traegt ihn als Ziel ein.
///
/// Der Schluessel geht nie in den Browser: er wird hier geholt und direkt an
/// das Relay weitergereicht. Fehlt der Scope oder der Token, sagt die Antwort
/// genau das, damit die Seite den ehrlichen Hinweis zeigen kann.
pub async fn twitch_auto_destination_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&auth)?;
    let user_id = partner_user_id(&auth)?;

    let Ok(cipher) = FieldCipher::from_env() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "token_store_unavailable" })),
        )
            .into_response());
    };
    let store = RaidAuthStore::new(pool.clone(), Arc::new(cipher));
    let scopes = store.get_scopes(&user_id).await.unwrap_or_default();
    if !scopes.iter().any(|scope| scope == STREAM_KEY_SCOPE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "scope_missing", "required_scope": STREAM_KEY_SCOPE })),
        )
            .into_response());
    }
    let token = match store.load_decrypted_unrestricted(&user_id).await {
        Ok(Some(tokens)) if !tokens.needs_reauth => tokens.access_token,
        _ => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "reauth_required" })),
            )
                .into_response())
        }
    };
    let client_id = std::env::var("TWITCH_CLIENT_ID")
        .or_else(|_| std::env::var("TWITCH_BOT_CLIENT_ID"))
        .unwrap_or_default();
    if client_id.trim().is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "twitch_client_unavailable" })),
        )
            .into_response());
    }
    let base = std::env::var("TWITCH_HELIX_BASE_URL")
        .unwrap_or_else(|_| "https://api.twitch.tv/helix".to_string());
    let antwort = reqwest::Client::new()
        .get(format!("{}/streams/key", base.trim_end_matches('/')))
        .query(&[("broadcaster_id", user_id.as_str())])
        .header("Client-Id", client_id)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Twitch-Streamkey-Abruf fehlgeschlagen: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "twitch_unavailable" })),
            )
                .into_response()
        })?;
    let status = antwort.status();
    // Erst den Rumpf als Text holen, dann protokollieren, dann lesen. So steht
    // in jedem Fall Status und Groesse im Protokoll, auch wenn Twitch etwas
    // schickt, das gar kein JSON ist.
    let rumpf_text = antwort.text().await.unwrap_or_default();
    let meldung = helix_log(status.as_u16(), rumpf_text.len());
    if status == reqwest::StatusCode::UNAUTHORIZED {
        tracing::warn!("{meldung}");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "reauth_required" })),
        )
            .into_response());
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        tracing::warn!("{meldung}");
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "scope_missing", "required_scope": STREAM_KEY_SCOPE })),
        )
            .into_response());
    }
    if !status.is_success() {
        tracing::error!("{meldung}");
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "twitch_unavailable" })),
        )
            .into_response());
    }
    let rumpf = helix_rumpf_wert(&rumpf_text);
    let Some(stream_key) = helix_stream_key(&rumpf) else {
        // 200 ohne brauchbaren Schluessel bleibt ein Fehlschlag. Ein leeres Ziel
        // einzutragen waere schlimmer als die ehrliche Absage.
        tracing::warn!("{meldung}");
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "twitch_unavailable" })),
        )
            .into_response());
    };
    tracing::info!("{meldung}");

    let wert = relay_client()?
        .call(
            reqwest::Method::PUT,
            "/v1/me/destinations",
            Some(json!({
                "streamer_id": id,
                "destinations": [{
                    "platform": "twitch",
                    "rtmp_url": TWITCH_RTMP_URL,
                    "stream_key": stream_key,
                    "enabled": true,
                }],
            })),
        )
        .await?;
    Ok(Json(wert))
}

/// Baut die Protokollzeile fuer einen Helix-Abruf.
///
/// Hier steht bewusst nur der Statuscode und wie viele Bytes zurueckkamen.
/// Der Rumpf traegt den Stream-Schluessel, und der gehoert in kein Protokoll.
fn helix_log(status: u16, rumpf_laenge: usize) -> String {
    format!("Twitch-Streamkey-Abruf: Status {status}, Rumpf {rumpf_laenge} Bytes")
}

/// Macht aus dem rohen Antworttext einen Wert.
///
/// Antwortet Twitch mit HTML oder einem Bruchstueck, gibt es `Null`. Daraus
/// liest `helix_stream_key` nichts, und der Aufrufer meldet den Fehlschlag,
/// statt ein leeres Ziel einzutragen.
fn helix_rumpf_wert(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

/// Liest `data[0].stream_key` aus der Helix-Antwort.
fn helix_stream_key(rumpf: &Value) -> Option<String> {
    let key = rumpf
        .get("data")?
        .as_array()?
        .first()?
        .get("stream_key")?
        .as_str()?
        .trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

// ---------------------------------------------------------------------------
// Verwaltung
// ---------------------------------------------------------------------------

pub async fn admin_overview_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    require_admin(&auth)?;
    let wert = relay_client()?
        .call(reqwest::Method::GET, "/v1/admin/overview", None)
        .await?;
    Ok(Json(wert))
}

pub async fn admin_waitlist_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    require_admin(&auth)?;
    let wert = relay_client()?
        .call(reqwest::Method::GET, "/v1/admin/waitlist", None)
        .await?;
    Ok(Json(wert))
}

#[derive(Deserialize)]
pub struct AdminSettingsBody {
    #[serde(default)]
    pub max_points: Option<i32>,
    #[serde(default)]
    pub load_reject_threshold: Option<f32>,
}

pub async fn admin_settings_handler(
    auth: DashboardAuthLevel,
    Json(body): Json<AdminSettingsBody>,
) -> Result<Json<Value>, Response> {
    require_admin(&auth)?;
    let mut rumpf = json!({});
    let obj = rumpf.as_object_mut().expect("objekt");
    if let Some(wert) = body.max_points {
        obj.insert("max_points".into(), json!(wert));
    }
    if let Some(wert) = body.load_reject_threshold {
        obj.insert("load_reject_threshold".into(), json!(wert));
    }
    if obj.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Es gibt nichts zu speichern." })),
        )
            .into_response());
    }
    let wert = relay_client()?
        .call(reqwest::Method::PUT, "/v1/admin/settings", Some(rumpf))
        .await?;
    Ok(Json(wert))
}

/// `confirm=true`, Abschnitt 10: killende Aufrufe brauchen eine Bestaetigung.
#[derive(Deserialize, Default)]
pub struct ConfirmQuery {
    #[serde(default)]
    pub confirm: bool,
}

pub async fn admin_kill_session_handler(
    auth: DashboardAuthLevel,
    Path(session_id): Path<i64>,
    Query(query): Query<ConfirmQuery>,
) -> Result<Json<Value>, Response> {
    require_admin(&auth)?;
    if !query.confirm {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Bestätigung fehlt." })),
        )
            .into_response());
    }
    let wert = relay_client()?
        .call(
            reqwest::Method::POST,
            &format!("/v1/admin/sessions/{session_id}/kill?confirm=true"),
            Some(json!({})),
        )
        .await?;
    Ok(Json(wert))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Die Relay-Umgebungsvariablen sind global. Tests, die sie setzen, laufen
    /// nacheinander, sonst nimmt ein Test die Einstellung des anderen mit.
    static ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "earlysalty".into(),
            twitch_user_id: "123".into(),
            display_name: "Early".into(),
        }
    }

    fn status_von(antwort: Response) -> StatusCode {
        antwort.status()
    }

    #[test]
    fn partner_ohne_id_ist_fehler() {
        let auth = DashboardAuthLevel::None;
        assert!(partner_id(&auth).is_err());
    }

    #[test]
    fn partner_id_wird_gelesen() {
        assert_eq!(partner_id(&partner()).unwrap(), 123);
    }

    #[test]
    fn ohne_secret_gibt_es_keinen_zugang() {
        assert!(RelayClient::from_parts(None, None).is_none());
        assert!(RelayClient::from_parts(None, Some("   ".into())).is_none());
        let client = RelayClient::from_parts(None, Some("geheim".into())).expect("zugang");
        assert_eq!(client.base, DEFAULT_RELAY_BASE);
        let mit_basis =
            RelayClient::from_parts(Some("http://127.0.0.1:9/".into()), Some("geheim".into()))
                .expect("zugang");
        assert_eq!(mit_basis.base, "http://127.0.0.1:9");
    }

    #[tokio::test]
    async fn ohne_session_ist_es_401() {
        let fehler = me_handler(DashboardAuthLevel::None)
            .await
            .expect_err("ohne Session darf es keine Antwort geben");
        assert_eq!(status_von(fehler), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ohne_secret_ist_es_503() {
        let _guard = ENV.lock().await;
        std::env::remove_var("RS_RELAY_API_SECRET");
        std::env::remove_var("RS_RELAY_BASE_URL");
        let fehler = me_handler(partner())
            .await
            .expect_err("ohne Secret darf nichts durchgehen");
        assert_eq!(status_von(fehler), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn me_reicht_die_relay_antwort_durch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/me"))
            .and(query_param("streamer_id", "123"))
            .and(header("X-Relay-Auth", "geheim"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "enabled": true,
                "waitlisted": false,
                "ingest_key": "rsr_abc",
                "rtmp_url": "",
                "srt_hint": "srt://relay.example.org:8899?streamid=rsr_abc",
            })))
            .mount(&server)
            .await;

        let _guard = ENV.lock().await;
        std::env::set_var("RS_RELAY_API_SECRET", "geheim");
        std::env::set_var("RS_RELAY_BASE_URL", server.uri());
        let antwort = me_handler(partner()).await;
        let Json(wert) = antwort.map_err(|_| "relay-aufruf").expect("antwort");
        assert_eq!(wert["enabled"], json!(true));
        assert_eq!(wert["ingest_key"], json!("rsr_abc"));
        assert!(wert["srt_hint"]
            .as_str()
            .expect("srt")
            .contains("relay.example.org"));
    }

    #[tokio::test]
    async fn kill_ohne_bestaetigung_geht_nicht_ans_relay() {
        let server = MockServer::start().await;
        // Kein Mock: jeder Aufruf am Relay waere ein Treffer auf 404 und damit
        // ein Fehler mit anderem Code als 400.
        let _guard = ENV.lock().await;
        std::env::set_var("RS_RELAY_API_SECRET", "geheim");
        std::env::set_var("RS_RELAY_BASE_URL", server.uri());
        let fehler = admin_kill_session_handler(
            DashboardAuthLevel::admin(),
            Path(7),
            Query(ConfirmQuery { confirm: false }),
        )
        .await
        .expect_err("ohne Bestaetigung darf nichts sterben");
        assert_eq!(status_von(fehler), StatusCode::BAD_REQUEST);
        assert!(server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
    }

    #[tokio::test]
    async fn kill_mit_bestaetigung_erreicht_das_relay() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/admin/sessions/7/kill"))
            .and(query_param("confirm", "true"))
            .and(header("X-Relay-Auth", "geheim"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_id": 7, "ended": true, "end_reason": "admin_kill"
            })))
            .mount(&server)
            .await;

        let _guard = ENV.lock().await;
        std::env::set_var("RS_RELAY_API_SECRET", "geheim");
        std::env::set_var("RS_RELAY_BASE_URL", server.uri());
        let antwort = admin_kill_session_handler(
            DashboardAuthLevel::admin(),
            Path(7),
            Query(ConfirmQuery { confirm: true }),
        )
        .await;
        let Json(wert) = antwort.map_err(|_| "relay-aufruf").expect("antwort");
        assert_eq!(wert["ended"], json!(true));
    }

    #[tokio::test]
    async fn verwaltung_ist_fuer_partner_gesperrt() {
        let fehler = admin_overview_handler(partner())
            .await
            .expect_err("ein Partner darf hier nichts sehen");
        assert_eq!(status_von(fehler), StatusCode::FORBIDDEN);

        let ohne = admin_overview_handler(DashboardAuthLevel::None)
            .await
            .expect_err("ohne Session erst recht nicht");
        assert_eq!(status_von(ohne), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn halb_ausgefuelltes_ziel_wird_abgelehnt() {
        let payload = destination_payload(
            123,
            &DestinationBody {
                platform: "kick".into(),
                rtmp_url: None,
                stream_key: None,
                enabled: None,
                width: Some(1280),
                height: Some(720),
                fps: Some(60),
                bitrate_kbps: Some(4500),
            },
        );
        let ziel = &payload["destinations"][0];
        assert_eq!(payload["streamer_id"], json!(123));
        assert_eq!(ziel["platform"], json!("kick"));
        assert!(
            ziel.get("stream_key").is_none(),
            "Key darf nicht erfunden werden"
        );
        assert_eq!(ziel["width"], json!(1280));
    }

    #[test]
    fn weggelassene_profilfelder_bleiben_weg() {
        let payload = destination_payload(
            5,
            &DestinationBody {
                platform: "twitch".into(),
                rtmp_url: Some(TWITCH_RTMP_URL.into()),
                stream_key: Some("live_1".into()),
                enabled: Some(true),
                width: None,
                height: None,
                fps: None,
                bitrate_kbps: None,
            },
        );
        let ziel = &payload["destinations"][0];
        assert_eq!(ziel["stream_key"], json!("live_1"));
        assert!(ziel.get("width").is_none());
        assert!(ziel.get("fps").is_none());
        assert_eq!(ziel["enabled"], json!(true));
    }

    #[test]
    fn helix_antwort_wird_gelesen() {
        let gut = json!({ "data": [{ "stream_key": "live_42_abc" }] });
        assert_eq!(helix_stream_key(&gut).as_deref(), Some("live_42_abc"));
        assert!(helix_stream_key(&json!({ "data": [] })).is_none());
        assert!(helix_stream_key(&json!({ "data": [{ "stream_key": "  " }] })).is_none());
        assert!(helix_stream_key(&json!({})).is_none());
    }

    #[test]
    fn leere_relay_fehler_bekommen_lesbaren_text() {
        let wert = fehlertext(StatusCode::BAD_REQUEST, json!({}));
        assert!(wert["error"].as_str().expect("text").len() > 10);
        let durchgereicht = fehlertext(StatusCode::BAD_REQUEST, json!({ "error": "eigen" }));
        assert_eq!(durchgereicht["error"], json!("eigen"));
    }

    #[test]
    fn absage_behaelt_die_felder_der_kill_antwort() {
        let roh = json!({
            "session_id": 7,
            "ended": true,
            "end_reason": "admin_kill",
            "stopped": false,
        });
        let wert = fehlertext(StatusCode::SERVICE_UNAVAILABLE, roh);
        assert_eq!(wert["session_id"], json!(7));
        assert_eq!(wert["ended"], json!(true));
        assert_eq!(wert["end_reason"], json!("admin_kill"));
        assert_eq!(wert["stopped"], json!(false));
        assert_eq!(
            wert["error"],
            json!(
                "Der Stream läuft noch. Wir haben das Beenden angesagt und warten auf die Bestätigung."
            )
        );
    }

    #[test]
    fn absage_ohne_kill_felder_bleibt_beim_alten_text() {
        let wert = fehlertext(StatusCode::SERVICE_UNAVAILABLE, json!({}));
        assert_eq!(wert["error"], json!("Uplink ist gerade nicht bereit."));
        let mit_stopped = fehlertext(StatusCode::SERVICE_UNAVAILABLE, json!({ "stopped": true }));
        assert_eq!(
            mit_stopped["error"],
            json!("Uplink ist gerade nicht bereit.")
        );
        assert_eq!(mit_stopped["stopped"], json!(true));
    }

    #[test]
    fn eigener_fehlertext_des_relays_bleibt_unangetastet() {
        let roh = json!({ "error": "eigener Satz", "stopped": false, "ended": true });
        let wert = fehlertext(StatusCode::SERVICE_UNAVAILABLE, roh.clone());
        assert_eq!(wert, roh);
    }

    #[test]
    fn helix_log_nennt_status_und_laenge_ohne_inhalt() {
        let zeile = helix_log(200, 57);
        assert!(zeile.contains("200"), "Status fehlt: {zeile}");
        assert!(zeile.contains("57"), "Laenge fehlt: {zeile}");
        assert!(
            !zeile.contains("live_"),
            "Schluessel darf nicht auftauchen: {zeile}"
        );
        assert!(
            !zeile.contains("data"),
            "Rumpf darf nicht auftauchen: {zeile}"
        );
        assert!(helix_log(503, 0).contains("503"));
    }

    #[test]
    fn helix_ohne_brauchbaren_schluessel_gibt_nichts_zurueck() {
        for roh in [
            json!({}),
            json!({ "data": [] }),
            json!({ "data": [{}] }),
            json!({ "data": [{ "stream_key": "   " }] }),
        ] {
            assert!(
                helix_stream_key(&roh).is_none(),
                "{roh} darf keinen Schlüssel liefern"
            );
        }
        assert!(helix_stream_key(&helix_rumpf_wert("<html>kaputt</html>")).is_none());
        assert!(helix_stream_key(&helix_rumpf_wert("")).is_none());
        let gut = helix_rumpf_wert(r#"{"data":[{"stream_key":"live_42_abc"}]}"#);
        assert_eq!(helix_stream_key(&gut).as_deref(), Some("live_42_abc"));
    }

    #[test]
    fn admin_gate_laesst_admin_durch() {
        assert!(require_admin(&DashboardAuthLevel::admin()).is_ok());
        let fehler = require_admin(&partner()).expect_err("partner ist kein admin");
        assert_eq!(fehler.into_response().status(), StatusCode::FORBIDDEN);
    }
}

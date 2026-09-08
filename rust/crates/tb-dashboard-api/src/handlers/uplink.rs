//! Proxy vom Streamer-Dashboard zu rs-relay. Das Relay-Secret bleibt serverseitig.

// Axum-Responses sind hier absichtlich direkt im Result: ein Boxen wuerde
// jeden Handler und jede Aufrufstelle verkomplizieren, ohne Laufzeitgewinn.
#![allow(clippy::result_large_err)]

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use tb_transport_twitch::{HelixClient, TwitchUser};

use super::platform_token::{PlatformTokenConfig, PLATFORM_TWITCH};
use crate::auth::{
    level::{is_admin_login, DashboardAuthLevel},
    require_admin,
};

const RELAY_ADMIN_WAITLIST_PFAD: &str = "/v1/admin/waitlist";
const RELAY_ADMIN_USERS_PFAD: &str = "/v1/admin/users";

/// Der Name des Secrets, das zu diesem Pfad gehört.
///
/// Das Relay hängt seine Admin-Routen seit dem Ingest-Key-Umbau an ein
/// eigenes Shared Secret. Der Grund liegt hier: dieses Dashboard hält das
/// API-Secret, um die Nutzerpfade eines Streamers zu lesen und seine Ziele zu
/// setzen, und bis dahin öffnete genau dieser Wert auch Freischalten,
/// Löschen, fremde Ziele und Session-Kills auf dem Relay. Kein Rückfall auf
/// das API-Secret, wenn das Admin-Secret fehlt: der wäre unsichtbar und
/// hätte die Trennung wieder aufgehoben.
///
/// Nur der Name, nicht der Wert: so lässt sich die Zuordnung prüfen, ohne im
/// Test die Prozessumgebung anzufassen. `set_var` in einem Testbinary ist ein
/// Datenrennen auf `environ` mit jedem anderen Test, der dasselbe tut.
fn secret_name_fuer(path: &str) -> &'static str {
    if path.starts_with("/v1/admin/") {
        "RS_RELAY_ADMIN_SECRET"
    } else {
        "RS_RELAY_API_SECRET"
    }
}

/// Twitch-Identität der Session: Login und, falls die Session sie mitbringt,
/// die numerische User-ID.
///
/// Die Master-Session des Admin-Dashboards ist Discord-basiert und trägt gar
/// keine Twitch-User-ID (`master_session_auth` setzt sie leer). Ohne Fallback
/// scheiterte Uplink für genau diese Session an einem leeren Parse.
pub(crate) fn twitch_identitaet(auth: &DashboardAuthLevel) -> Result<(&str, &str), Response> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => Ok((twitch_login.as_str(), twitch_user_id.as_str())),
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            Ok((actor.twitch_login.as_str(), actor.twitch_user_id.as_str()))
        }
        DashboardAuthLevel::Admin { actor: None } => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "admin ohne twitch-identitaet" })),
        )
            .into_response()),
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()),
    }
}

/// Ausschließlich die bestätigte numerische Twitch-ID aus der Sitzung.
pub(crate) async fn partner_id(_pool: &PgPool, auth: &DashboardAuthLevel) -> Result<i64, Response> {
    let (_, raw) = twitch_identitaet(auth)?;
    raw.parse::<i64>().ok().filter(|id| *id > 0).ok_or_else(|| {
        (StatusCode::UNAUTHORIZED, Json(json!({"error":"Deine Twitch-Identität ist nicht bestätigt. Bitte erneut anmelden."}))).into_response()
    })
}

pub(crate) async fn relay_json(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, Response> {
    let runtime = crate::uplink_config::runtime().map_err(|_| {
        let message = if secret_name_fuer(path) == "RS_RELAY_ADMIN_SECRET" {
            "Uplink-Adminzugang ist nicht eingerichtet."
        } else {
            "Uplink ist noch nicht eingerichtet."
        };
        fehler(StatusCode::SERVICE_UNAVAILABLE, message)
    })?;
    relay_json_mit(&runtime.base, runtime.secret(path), method, path, body).await
}

/// Der Relay-Aufruf mit ausdruecklicher Basis-URL und Secret, damit der Weg im
/// Test gegen einen Wiremock laufen kann, ohne die Prozessumgebung anzufassen.
/// `set_var` in einem Testbinary ist ein Datenrennen mit jedem anderen Test,
/// der dasselbe tut.
pub(crate) async fn relay_json_mit(
    base: &str,
    secret: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, Response> {
    use crate::uplink_config::{bounded_body, http_client, local_origin};
    let fail = || {
        fehler(
            StatusCode::BAD_GATEWAY,
            "Uplink antwortet nicht vollständig. Bitte erneut versuchen.",
        )
    };
    let base = local_origin(base).map_err(|_| fail())?;
    if !path.starts_with("/v1/") || path.contains('\\') || path.contains('#') {
        return Err(fail());
    }
    let client = http_client().map_err(|_| fail())?;
    let mut credential = reqwest::header::HeaderValue::from_str(secret).map_err(|_| fail())?;
    credential.set_sensitive(true);
    let mut request = client
        .request(method, format!("{base}{path}"))
        .header("X-Relay-Auth", credential)
        .header("Accept", "application/json");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|_| fail())?;
    let status = response.status();
    if status.is_redirection() {
        return Err(fail());
    }
    if !status.is_success() {
        let message = if status == StatusCode::CONFLICT {
            "Die Verbindung wurde geändert oder ist noch in Betrieb. Lade den aktuellen Stand und beende gegebenenfalls zuerst den Stream."
        } else {
            "Der Uplink-Dienst konnte die Anfrage nicht bestätigen. Bitte erneut versuchen."
        };
        return Err(fehler(status, message));
    }
    let bytes = bounded_body(response, 1024 * 1024)
        .await
        .map_err(|_| fail())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| fail())?;
    Ok(value)
}

/// Wie lange ein Eintrag aus `twitch_live_state` als Aussage ueber jetzt gilt.
///
/// Der Poller schreibt alle Zeilen gemeinsam und liegt im Betrieb wenige
/// Sekunden zurueck. Fuenf Minuten sind grosszuegig genug, dass ein einzelner
/// verzoegerter Lauf niemanden aussperrt, und kurz genug, dass ein stehender
/// Poller nicht stundenlang ein "ist nicht live" behauptet.
const LIVE_FRISCHE: chrono::Duration = chrono::Duration::minutes(5);

/// Ob der Streamer gerade sendet.
///
/// Drei Antworten, nicht zwei: `"live"`, `"aus"` und `"unbekannt"`. Das
/// Unbekannt ist kein Zierrat. Steht der Poller, ist ein altes `is_live = 0`
/// keine Aussage ueber jetzt, und genau darauf soll die Oberflaeche nicht das
/// Aufdecken eines Schluessels stuetzen. Unbekannt wird dort wie live
/// behandelt: verdeckt bleiben kostet nur Komfort, faelschlich aufdecken kostet
/// den Kanal.
/// Bewertet eine Zeile aus `twitch_live_state`, ohne Datenbank und ohne Uhr.
///
/// `jetzt` kommt von aussen, damit die Frist pruefbar ist statt nur behauptet.
fn live_bewerten(
    zeile: Option<(i32, Option<&str>)>,
    jetzt: chrono::DateTime<chrono::Utc>,
) -> &'static str {
    // Keine Zeile heisst: dieser Streamer wird nicht beobachtet. Auch das ist
    // keine Aussage ueber jetzt.
    let Some((is_live, last_seen)) = zeile else {
        return "unbekannt";
    };

    // `last_seen_at` ist Text in der Datenbank. Was sich nicht lesen laesst,
    // ist keine Zeitangabe und damit kein Frischenachweis.
    let Some(gesehen) = last_seen
        .map(str::trim)
        .and_then(|roh| chrono::DateTime::parse_from_rfc3339(roh).ok())
    else {
        return "unbekannt";
    };

    // Auch ein Stand aus der Zukunft ist keiner: eine schiefe Uhr auf der
    // schreibenden Seite darf keine Frische vortaeuschen.
    let alter = jetzt.signed_duration_since(gesehen.with_timezone(&chrono::Utc));
    if alter > LIVE_FRISCHE || alter < -LIVE_FRISCHE {
        return "unbekannt";
    }
    match is_live {
        0 => "aus",
        _ => "live",
    }
}

pub(crate) async fn live_status(pool: &PgPool, streamer_id: i64) -> &'static str {
    let zeile: Option<(i32, Option<String>)> = sqlx::query_as(
        "SELECT COALESCE(is_live, 0), last_seen_at FROM twitch_live_state WHERE twitch_user_id = $1",
    )
    .bind(streamer_id.to_string())
    .fetch_optional(pool)
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("uplink: Live-Status fuer {streamer_id} nicht lesbar: {e}");
        None
    });

    live_bewerten(
        zeile.as_ref().map(|(l, g)| (*l, g.as_deref())),
        chrono::Utc::now(),
    )
}

/// Der Verbindungsstand einer Plattform, wie ihn `/uplink/me` meldet.
///
/// Drei Antworten, nicht zwei. `neu_verbinden` ist kein Zierrat: ein alter
/// Raid-Grant traegt gueltige Tokens, aber weder Chat- noch Stream-Key-Recht.
/// Ihn als "verbunden" zu zeigen hiesse, dem Streamer ein Dock zu versprechen,
/// das leer bleibt. Ihn als "getrennt" zu zeigen hiesse, seinen laufenden
/// Raid-Bot zu verschweigen.
///
/// Reine Funktion, damit die Zuordnung ohne Datenbank pruefbar ist.
pub fn verbindungs_status(hat_tokens: bool, needs_reauth: bool, scopes: &[String]) -> &'static str {
    if !hat_tokens {
        return "getrennt";
    }
    if needs_reauth {
        return "neu_verbinden";
    }
    if !tb_raid::scope_profiles::hat_alle_uplink_scopes(scopes) {
        return "rechte_ergaenzen";
    }
    "verbunden"
}

/// Der Verbindungsstand aller Plattformen.
///
/// Zwei Quellen, weil es zwei Arten von Plattform gibt. Twitch liegt in
/// `twitch_raid_auth`, weil der Streamer dort ohnehin autorisiert. Kick,
/// YouTube und TikTok haben keinen Raid-Bot, an dessen Grant sich etwas
/// anhaengen liesse; fuer sie bleibt `platform_connections` der Speicher. Was
/// in keiner der beiden Quellen steht, ergaenzt der Aufrufer als "getrennt".
///
/// Ohne Feldschlüssel bleibt der Zugang unbekannt.
pub(crate) async fn verbindungen_lesen(
    pool: &PgPool,
    config: Option<&PlatformTokenConfig>,
    streamer_id: i64,
) -> Vec<(String, &'static str)> {
    let Some(config) = config else {
        return PLATTFORMEN
            .iter()
            .map(|platform| (platform.to_string(), "zugang_unbekannt"))
            .collect();
    };
    let mut liste = twitch_verbindung(pool, config, streamer_id).await;
    // Die uebrigen Plattformen. Heute ist die Tabelle leer, also kommt hier
    // nichts zurueck; sobald die erste gebaut ist, steht ihr Stand ohne
    // weiteres Zutun in derselben Liste.
    let store =
        super::platform_store::PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
    match store.status_liste(streamer_id).await {
        Ok(weitere) => liste.extend(
            weitere
                .into_iter()
                .filter(|(plattform, _)| plattform != PLATFORM_TWITCH),
        ),
        Err(_) => {
            liste.extend(
                PLATTFORMEN
                    .iter()
                    .filter(|platform| **platform != PLATFORM_TWITCH)
                    .map(|platform| (platform.to_string(), "zugang_unbekannt")),
            );
        }
    }
    let states:Result<Vec<(String,bool,bool)>,sqlx::Error>=sqlx::query_as("SELECT platform,enabled,disconnect_pending FROM uplink_target_generations WHERE twitch_user_id=$1").bind(streamer_id.to_string()).fetch_all(pool).await;
    match states {
        Ok(states) => {
            for (platform, enabled, pending) in states {
                if !enabled {
                    liste.retain(|(p, _)| p != &platform);
                    liste.push((
                        platform,
                        if pending {
                            "trennung_offen"
                        } else {
                            "getrennt"
                        },
                    ));
                }
            }
        }
        Err(_) => {
            return PLATTFORMEN
                .iter()
                .map(|p| (p.to_string(), "zugang_unbekannt"))
                .collect()
        }
    }
    liste
}

/// Der Verbindungsstand von Twitch aus `twitch_raid_auth`.
async fn twitch_verbindung(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    streamer_id: i64,
) -> Vec<(String, &'static str)> {
    use super::platform_token::{gueltiger_twitch_token, TokenFehler};
    let status = match gueltiger_twitch_token(pool, config, streamer_id, chrono::Utc::now()).await {
        Ok((tokens, scopes)) => verbindungs_status(true, tokens.needs_reauth, &scopes),
        Err(TokenFehler::KeineVerbindung) => "getrennt",
        Err(TokenFehler::NeuVerbinden) => "neu_verbinden",
        Err(TokenFehler::NichtLieferbar) => "zugang_unbekannt",
    };
    vec![(PLATFORM_TWITCH.to_string(), status)]
}

pub async fn me_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<PlatformTokenConfig>>,
    auth: DashboardAuthLevel,
) -> Result<Json<Value>, Response> {
    let config = config.map(|Extension(c)| c);
    let id = partner_id(&pool, &auth).await?;
    let mut wert = relay_json(
        reqwest::Method::GET,
        &format!("/v1/me?streamer_id={id}"),
        None,
    )
    .await?;
    // Der Live-Status ist Wissen des Bots, nicht des Relays: er kommt aus der
    // Twitch-Beobachtung. Deshalb wird er hier angehaengt und nicht im Relay
    // nachgebaut. Gleiches gilt fuer die verbundenen Plattformen.
    let live = live_status(&pool, id).await;
    let verbindungen = verbindungen_lesen(&pool, config.as_ref(), id).await;
    // Ein fehlgeschlagener Zielabruf ist unbekannt, kein nachgewiesen fehlender Schlüssel.
    let ziele = ziel_plattformen(
        &relay_json(
            reqwest::Method::GET,
            &format!("{RELAY_ZIEL_PFAD}?streamer_id={id}"),
            None,
        )
        .await?,
    );
    let mut verbindbar: Vec<&str> = Vec::new();
    if config.is_some() {
        verbindbar.push("twitch");
    }
    if super::plattform_connect::kick_konfiguriert() {
        verbindbar.push("kick");
    }
    if super::plattform_connect::youtube_konfiguriert() {
        verbindbar.push("youtube");
    }
    me_anreichern(&mut wert, live, &verbindungen, &ziele, &verbindbar);
    Ok(Json(wert))
}

/// Plattformen, fuer die das Relay ein Ziel fuehrt (`GET /v1/me/destinations`).
fn ziel_plattformen(wert: &Value) -> Vec<String> {
    wert.get("destinations")
        .and_then(Value::as_array)
        .map(|liste| {
            liste
                .iter()
                .filter_map(|z| z.get("platform").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Haengt an die Relay-Antwort an, was nur der Bot weiss: Live-Status und den
/// Stand der Chat-Verbindungen je Plattform. Reine Funktion, damit sie ohne
/// Relay pruefbar ist.
///
/// Der Twitch-Login stand hier frueher mit, weil die Oberflaeche daraus die
/// Adresse des Twitch-Chat-Fensters baute. Diese Fenster gibt es nicht mehr;
/// der Login hatte danach keinen Abnehmer und ging trotzdem bei jedem Abruf
/// mit.
fn me_anreichern(
    wert: &mut Value,
    live: &str,
    verbindungen: &[(String, &'static str)],
    ziele: &[String],
    verbindbar: &[&str],
) {
    let Some(objekt) = wert.as_object_mut() else {
        return;
    };
    objekt.insert("live_status".to_string(), Value::String(live.to_string()));
    // Beide Dock-Felder kommen vom Relay und werden unveraendert
    // durchgereicht, nur auf eine feste Form gebracht.
    //
    // `dock_url_vorhanden` fehlt: dann gilt Nein, damit die Oberflaeche
    // "erzeugen" von "neu erzeugen" trennen kann. `dock_urls` fehlt: dann
    // steht dort `null` statt gar nichts, damit die Oberflaeche einen fehlenden
    // Schluessel nicht von einer leeren Antwort unterscheiden muss.
    //
    // Beides kann auseinanderfallen: ein Zugang, den das Relay nur als
    // Fingerabdruck kennt, ist vorhanden und trotzdem nicht anzeigbar.
    let dock_url_vorhanden = objekt
        .get("dock_url_vorhanden")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    objekt.insert(
        "dock_url_vorhanden".to_string(),
        Value::Bool(dock_url_vorhanden),
    );
    //
    // Was kein Objekt ist, wird zu `null`: die Oberflaeche liest die vier
    // Felder direkt. Ein String oder ein Array kaeme dort als vier fehlende
    // Felder an und die Karte bliebe stumm leer, waehrend sie daneben
    // "vorhanden" meldet. Auf `null` landet derselbe Fall auf dem Pfad, der
    // erklaert ist und einen Knopf anbietet.
    let dock_urls = objekt
        .get("dock_urls")
        .filter(|w| w.is_object())
        .cloned()
        .unwrap_or(Value::Null);
    objekt.insert("dock_urls".to_string(), dock_urls);
    // Jede bekannte Plattform bekommt einen Eintrag; was nicht gespeichert
    // ist, ist getrennt. So muss die Oberflaeche keine Luecken deuten.
    let liste: Vec<Value> = PLATTFORMEN
        .iter()
        .map(|plattform| {
            let status = verbindungen
                .iter()
                .find(|(p, _)| p == plattform)
                .map(|(_, s)| *s)
                .unwrap_or("getrennt");
            let stream_key_vorhanden = ziele.iter().any(|z| z == plattform);
            json!({
                "platform": plattform,
                "status": status,
                "stream_key_vorhanden": stream_key_vorhanden,
                "verbindbar": verbindbar.contains(plattform),
            })
        })
        .collect();
    objekt.insert("verbindungen".to_string(), Value::Array(liste));
}

/// `POST /twitch/api/v2/uplink/dock-token/rotate`: laesst das Relay neue
/// Dock-Adressen fuer den Streamer ausstellen. Die alten gelten danach nicht
/// mehr.
///
/// Die Antwort ist nicht die einzige Gelegenheit: `GET /uplink/me` traegt
/// dieselben vier Adressen bei jedem Abruf. Sie steht hier trotzdem, damit die
/// Oberflaeche direkt nach dem Klick etwas zeigen kann.
pub async fn dock_token_rotate_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&pool, &auth).await?;
    let wert = relay_json(reqwest::Method::POST, &dock_token_rotate_pfad(id), None).await?;
    Ok(Json(wert))
}

/// Das Relay liest den Streamer wie bei `/v1/me/waitlist` aus der Query
/// (`Query<MeQuery>` in rs-relay `src/api/chat.rs`), nicht aus einem Body.
fn dock_token_rotate_pfad(streamer_id: i64) -> String {
    format!("/v1/me/dock-token/rotate?streamer_id={streamer_id}")
}

pub async fn waitlist_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&pool, &auth).await?;
    let wert = relay_json(
        reqwest::Method::POST,
        &format!("/v1/me/waitlist?streamer_id={id}"),
        Some(json!({})),
    )
    .await?;
    Ok(Json(wert))
}

/// Gate für die Uplink-Wartelistenverwaltung.
///
/// Zugelassen ist jeder Admin (Discord-Master oder per Twitch-OAuth promoteter
/// Admin) und zusätzlich der Owner, der nur mit seiner Twitch-Identität
/// eingeloggt ist: eine Partner-Session mit Admin-Login (`is_admin_login`, also
/// `earlysalty`) verwaltet die Warteliste, ohne vorher den Admin-Modus
/// einzuschalten. Ein normaler Partner bleibt draußen.
fn admin_pruefen(auth: &DashboardAuthLevel) -> Result<(), Response> {
    if let DashboardAuthLevel::Partner { twitch_login, .. } = auth {
        if is_admin_login(twitch_login) {
            return Ok(());
        }
    }
    match require_admin(auth) {
        Some(fehler) => Err(fehler.into_response()),
        None => Ok(()),
    }
}

fn admin_actor_fuer_log(auth: &DashboardAuthLevel) -> (&str, Option<&str>) {
    match auth {
        DashboardAuthLevel::Admin { actor: Some(actor) } => (
            actor.twitch_login.as_str(),
            Some(actor.twitch_user_id.as_str()),
        ),
        DashboardAuthLevel::Admin { actor: None } => ("discord-admin", None),
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => (twitch_login.as_str(), Some(twitch_user_id.as_str())),
        DashboardAuthLevel::None => ("unauthenticated", None),
    }
}

/// Baut aus der tb-dashboard-Env einen Helix-Client (App-Token).
///
/// Dieselbe Secret-Quelle wie der AvatarCache (`TWITCH_CLIENT_ID`/`_SECRET`, aus
/// Infisical in die Prozess-Env geladen). Fehlt ein Secret oder scheitert die
/// Konstruktion, gibt es keinen Client; die Warteliste bleibt dann bei den
/// nackten IDs, statt den ganzen Abruf zu verlieren.
fn helix_aus_konfiguration() -> Option<HelixClient> {
    crate::uplink_config::runtime().ok()?.helix.clone()
}

/// Die `streamer_id` eines Eintrags als String, egal ob JSON-Zahl oder -String.
fn eintrag_id(eintrag: &Value) -> Option<String> {
    eintrag.get("streamer_id").and_then(|id| {
        id.as_i64()
            .map(|n| n.to_string())
            .or_else(|| id.as_str().map(str::to_string))
    })
}

/// Sammelt die `streamer_id` aller Einträge als Strings für den Helix-Abruf.
fn waitlist_ids(wert: &Value) -> Vec<String> {
    wert.get("entries")
        .and_then(Value::as_array)
        .map(|eintraege| eintraege.iter().filter_map(eintrag_id).collect())
        .unwrap_or_default()
}

/// Hängt an jeden Eintrag `twitch_login` und `display_name`, sofern Twitch die
/// ID auflöst. Nicht aufgelöste IDs bekommen `null`, damit das Frontend sauber
/// auf die nackte ID zurückfällt statt einen fremden Namen zu zeigen.
fn mit_namen(mut wert: Value, users: &HashMap<String, TwitchUser>) -> Value {
    if let Some(eintraege) = wert.get_mut("entries").and_then(Value::as_array_mut) {
        for eintrag in eintraege {
            let user = eintrag_id(eintrag).and_then(|id| users.get(&id));
            if let Some(obj) = eintrag.as_object_mut() {
                obj.insert(
                    "twitch_login".into(),
                    user.map(|u| json!(u.login)).unwrap_or(Value::Null),
                );
                obj.insert(
                    "display_name".into(),
                    user.map(|u| json!(u.display_name)).unwrap_or(Value::Null),
                );
            }
        }
    }
    wert
}

/// Wartende Uplink-Konten für die Admin-Box.
///
/// Gate ist `admin_pruefen`: jeder Admin und zusätzlich der Owner, der nur mit
/// seiner Twitch-Identität eingeloggt ist (Partner mit Admin-Login). So sieht
/// und bedient der Owner die Warteliste auch ohne aktivierten Admin-Modus; ein
/// normaler Partner bekommt weiterhin 403.
///
/// Die Einträge tragen nur die numerische `streamer_id`; für die Anzeige wird
/// jeder Name best-effort über Twitch-Helix nachgeladen. Scheitert das (kein
/// Secret, Twitch nicht erreichbar), bleibt die Liste mit den IDs erhalten.
pub async fn admin_waitlist_handler(auth: DashboardAuthLevel) -> Result<Json<Value>, Response> {
    admin_pruefen(&auth)?;
    let wert = relay_json(reqwest::Method::GET, RELAY_ADMIN_WAITLIST_PFAD, None).await?;

    let ids = waitlist_ids(&wert);
    if ids.is_empty() {
        return Ok(Json(wert));
    }
    let users = match helix_aus_konfiguration() {
        Some(helix) => {
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            match helix.get_users_by_id(&refs).await {
                Ok(map) => map,
                Err(fehler) => {
                    tracing::warn!(%fehler, "uplink: Wartelisten-Namen nicht aufloesbar");
                    HashMap::new()
                }
            }
        }
        None => HashMap::new(),
    };
    Ok(Json(mit_namen(wert, &users)))
}

#[derive(Deserialize)]
pub struct AdminFreischaltenBody {
    pub streamer_id: i64,
}

fn freischaltung_antwort(streamer_id: i64) -> Value {
    json!({
        "streamer_id": streamer_id,
        "enabled": true,
    })
}

/// Schaltet einen Wartelisteneintrag über den bestehenden Relay-Adminpfad frei.
///
/// Das Relay liefert dabei auch den neuen Ingest-Schlüssel. Diese Antwort wird
/// bewusst verworfen, damit der Schlüssel nicht im Browser landet.
pub async fn admin_freischalten_handler(
    auth: DashboardAuthLevel,
    Json(body): Json<AdminFreischaltenBody>,
) -> Result<Json<Value>, Response> {
    admin_pruefen(&auth)?;
    if body.streamer_id <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "ungültige Twitch-ID" })),
        )
            .into_response());
    }

    relay_json(
        reqwest::Method::POST,
        RELAY_ADMIN_USERS_PFAD,
        Some(json!({ "streamer_id": body.streamer_id })),
    )
    .await?;

    let (actor, actor_id) = admin_actor_fuer_log(&auth);
    tracing::info!(
        actor,
        actor_twitch_user_id = actor_id.unwrap_or("-"),
        target_streamer_id = body.streamer_id,
        "Uplink-Wartelisteneintrag freigeschaltet"
    );

    Ok(Json(freischaltung_antwort(body.streamer_id)))
}

fn admin_waitlist_eintrag_pfad(streamer_id: i64) -> String {
    format!("{RELAY_ADMIN_WAITLIST_PFAD}/{streamer_id}")
}

/// Lehnt einen Wartelisteneintrag ab: der Eintrag verschwindet, ein Zugang
/// entsteht nicht.
///
/// Kein Konto wird geloescht und keine Sperre gesetzt. Wer abgelehnt wurde,
/// sieht im Dashboard wieder den Knopf und kann sich erneut eintragen. Genau
/// deshalb steht der Knopf gleichberechtigt neben "Freischalten": die Liste
/// bleibt sonst voll mit Anfragen, die nie beantwortet werden.
pub async fn admin_ablehnen_handler(
    auth: DashboardAuthLevel,
    Path(streamer_id): Path<i64>,
) -> Result<Json<Value>, Response> {
    admin_pruefen(&auth)?;
    if streamer_id <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "ungültige Twitch-ID" })),
        )
            .into_response());
    }

    relay_json(
        reqwest::Method::DELETE,
        &admin_waitlist_eintrag_pfad(streamer_id),
        None,
    )
    .await?;

    let (actor, actor_id) = admin_actor_fuer_log(&auth);
    tracing::info!(
        actor,
        actor_twitch_user_id = actor_id.unwrap_or("-"),
        target_streamer_id = streamer_id,
        "Uplink-Wartelisteneintrag abgelehnt"
    );

    Ok(Json(json!({
        "streamer_id": streamer_id,
        "rejected": true,
    })))
}

/// Die gespeicherten Ziele, ohne Stream-Key.
///
/// Das Relay liefert den Key nicht mit, und das ist richtig so: er ist
/// verschluesselt abgelegt und wird nie wieder ausgegeben. Gerade deshalb
/// braucht die Oberflaeche diese Liste. Ohne sie sieht ein gespeichertes Ziel
/// aus wie ein leeres Formular, und der Streamer speichert ein zweites Mal.
pub async fn destinations_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&pool, &auth).await?;
    let wert = relay_json(
        reqwest::Method::GET,
        &format!("/v1/me/destinations?streamer_id={id}"),
        None,
    )
    .await?;
    Ok(Json(wert))
}

/// Einstellung fuer die Wartezeit nach einem unerwarteten Ingest-Abriss.
/// Ein normales OBS-Stoppen wird im Relay davon getrennt und sofort abgeraeumt.
#[derive(Deserialize)]
pub struct ReconnectWaitBody {
    pub reconnect_wait_s: i32,
}

pub async fn put_reconnect_wait_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Json(body): Json<ReconnectWaitBody>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&pool, &auth).await?;
    let wert = relay_json(
        reqwest::Method::PUT,
        &format!("/v1/me/reconnect-wait?streamer_id={id}"),
        Some(json!({ "reconnect_wait_s": body.reconnect_wait_s })),
    )
    .await?;
    Ok(Json(wert))
}

/// Erlaubte Profile fuer die Zielwahl im Dashboard.
///
/// Feste Stufen sind der bequeme Weg: ein Name, und die vier Zahlen dahinter
/// sind auf beiden Seiten dieselben. Daneben gibt es den manuellen Modus, der
/// die Zahlen direkt traegt. Der Katalog bleibt, weil die Auswahlliste ihn
/// braucht und weil "1080p60" als gespeicherter Wunsch leichter zu lesen ist
/// als vier Spalten.
///
/// 1440p ist bewusst nicht der Standard: Twitch unterstuetzt es ueber den
/// normalen Ingest offiziell nicht. Auf 1,78-mal so viele Pixel verteilt sind
/// dieselben Bits in einem Deadlock-Teamfight weniger wert als bei 1080p. Wer
/// es trotzdem will, soll es waehlen koennen; die Oberflaeche schreibt den
/// Haken dazu.
///
/// Die Reihenfolge hier ist die der Auswahlliste, absteigend von der besten
/// zur sparsamsten Stufe. `profil_aufloesen` sucht nach Namen, fuer die
/// Aufloesung spielt sie keine Rolle.
const PROFILE: [(&str, i32, i32, i32, i32); 5] = [
    ("1440p60", 2560, 1440, 60, 12000),
    ("1080p60-hoch", 1920, 1080, 60, 8000),
    ("1080p60", 1920, 1080, 60, 6000),
    ("720p60", 1280, 720, 60, 4500),
    ("480p30", 854, 480, 30, 1500),
];

/// Plattformen, die das Relay kennt (`platform` Check-Constraint in rs-relay).
///
/// Die Pruefung passiert hier und nicht erst im Relay, damit ein Tippfehler
/// eine lesbare 400 mit Text bekommt statt einer nackten vom Proxy dahinter.
const PLATTFORMEN: [&str; 4] = ["twitch", "kick", "youtube", "tiktok"];

/// Werte, die Twitch fuer 2K empfiehlt. Nur fuer den Katalogtest: der
/// manuelle Modus prueft nichts dagegen.
///
/// Der Ingest-Deckel, der hier frueher stand, ist mit der Klemmung in rs-relay
/// weggefallen. Er hat eine Eingabe abgelehnt, die das Relay angenommen
/// haette, und damit an einer Stelle entschieden, an der niemand nach dem
/// Grund suchen wuerde.
#[cfg(test)]
const TWITCH_EMPFOHLENE_BREITE: i32 = 2560;
#[cfg(test)]
const TWITCH_EMPFOHLENE_HOEHE: i32 = 1440;
#[cfg(test)]
const TWITCH_EMPFOHLENE_BITRATE: i32 = 12000;

/// Loest einen Profilnamen auf. `None` heisst: nicht im Katalog.
fn profil_aufloesen(name: &str) -> Option<(i32, i32, i32, i32)> {
    let gesucht = name.trim();
    PROFILE
        .iter()
        .find(|(n, ..)| *n == gesucht)
        .map(|(_, w, h, f, b)| (*w, *h, *f, *b))
}

/// Eine Zahl aus einem Formularfeld, so wie sie ankommt, ungeprueft.
///
/// Bewusst kein `i32`. Das Feld im Dashboard nimmt beliebig viele Ziffern an,
/// und wer sich beim Tippen vergreift, schickt eine Zahl, die nicht in einen
/// `i32` passt. `serde` bricht dann schon beim Deserialisieren ab, axum
/// antwortet mit 422 und einem Klartextkoerper, und die Oberflaeche zeigt
/// "Speichern hat nicht geklappt." ohne einen Hinweis darauf, welches der
/// vier Felder gemeint ist.
///
/// Deshalb nimmt dieser Typ jede JSON-Zahl an und merkt sich nur, ob sie als
/// ganze Zahl darstellbar war. Ueber alles Weitere entscheidet
/// [`manuell_pruefen`], mit Feldnamen und lesbarem Satz.
///
/// `None` heisst: keine ganze Zahl in Reichweite. Das trifft eine
/// Kommazahl genauso wie eine Zahl mit zwanzig Stellen, die `serde_json` als
/// Gleitkomma einliest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Formularzahl(Option<i64>);

impl Formularzahl {
    #[cfg(test)]
    pub fn neu(wert: i64) -> Self {
        Formularzahl(Some(wert))
    }
}

impl<'de> Deserialize<'de> for Formularzahl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let zahl = serde_json::Number::deserialize(d)?;
        Ok(Formularzahl(zahl.as_i64()))
    }
}

/// Freie Zahlen aus dem manuellen Modus.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManuellesProfil {
    pub width: Formularzahl,
    pub height: Formularzahl,
    pub fps: Formularzahl,
    pub bitrate_kbps: Formularzahl,
}

/// Prueft ein manuelles Profil auf das, was technisch nicht gehen kann, und
/// gibt die vier Zahlen so zurueck, wie das Relay sie speichert.
///
/// Nach oben wird nichts mehr geprueft. Frueher stand hier ein Ingest-Deckel,
/// und der hat eine Eingabe abgelehnt, die das Relay angenommen haette: der
/// Streamer trug 16000 ein, bekam "liegt über unserem Maximum" und keine
/// Erklaerung, wessen Maximum das ist. Was eine Plattform wirklich annimmt,
/// entscheidet die Plattform an ihrem Ingest. Was der Server traegt,
/// entscheidet das Punktebudget in rs-relay, und zwar mit einer Ablehnung
/// samt Grund statt hier mit einem Formularfehler.
///
/// Die Umrechnung nach `i32` ist keine Obergrenze und will keine sein. Sie ist
/// der Spaltentyp in rs-relay, und eine Zahl, die dort nicht hineinpasst, kann
/// niemand speichern. Frueher hat das `serde` erledigt, mit einer 422 ohne
/// Feldangabe; jetzt steht der Feldname und die getippte Zahl im Satz.
///
/// Rueckgabe ist sonst der Grund, warum es nicht geht, damit die Oberflaeche
/// ihn hinschreiben kann. Ein blosses "ungueltig" laesst den Streamer raten,
/// welches der vier Felder gemeint ist.
fn manuell_pruefen(p: ManuellesProfil) -> Result<(i32, i32, i32, i32), String> {
    let mut zahlen = [0i32; 4];
    for (platz, (name, feld)) in [
        ("Breite", p.width),
        ("Höhe", p.height),
        ("Bildrate", p.fps),
        ("Bitrate", p.bitrate_kbps),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(wert) = feld.0 else {
            return Err(format!(
                "{name}: das ist keine ganze Zahl. Prüf das Feld auf einen Tippfehler."
            ));
        };
        if wert <= 0 {
            return Err(format!("{name} muss größer als 0 sein."));
        }
        let Ok(wert) = i32::try_from(wert) else {
            return Err(format!(
                "{name}: {wert} ist zu groß für dieses Feld. Da hat sich wohl eine Ziffer verirrt."
            ));
        };
        zahlen[platz] = wert;
    }
    let [width, height, fps, bitrate_kbps] = zahlen;
    // Ungerade Kantenlaengen bringen den Encoder ins Straucheln: yuv420p
    // halbiert beide Achsen, und eine ungerade Zahl laesst sich nicht
    // halbieren. ffmpeg bricht dann beim Start ab, nicht beim Speichern.
    if width % 2 != 0 || height % 2 != 0 {
        return Err("Breite und Höhe müssen gerade Zahlen sein.".into());
    }
    Ok((width, height, fps, bitrate_kbps))
}

#[derive(Deserialize)]
pub struct DestinationBody {
    pub platform: String,
    /// Weggelassen heisst zusammen mit `stream_key`: nur das Profil aendern,
    /// das gespeicherte Ziel bleibt stehen. Genau das fehlte vorher, und
    /// deshalb liess sich eine Qualitaetsstufe nicht ohne erneutes Eintippen
    /// des Stream-Keys speichern.
    pub rtmp_url: Option<String>,
    pub stream_key: Option<String>,
    /// Name aus `PROFILE`. Schliesst `manuell` aus.
    pub profil: Option<String>,
    /// Freie Zahlen. Schliesst `profil` aus.
    pub manuell: Option<ManuellesProfil>,
    /// Ziel an- oder abschalten, ohne es zu loeschen.
    pub enabled: Option<bool>,
}

fn fehler(status: StatusCode, text: &str) -> Response {
    (status, Json(json!({ "error": text }))).into_response()
}

/// Baut den Zieleintrag fuer das Relay oder liefert den Grund, warum nicht.
fn ziel_nutzlast(body: &DestinationBody) -> Result<Value, Response> {
    if !PLATTFORMEN.contains(&body.platform.trim()) {
        return Err(fehler(StatusCode::BAD_REQUEST, "unbekannte Plattform"));
    }
    if body.profil.is_some() && body.manuell.is_some() {
        return Err(fehler(
            StatusCode::BAD_REQUEST,
            "Entweder eine Stufe oder eigene Werte, nicht beides.",
        ));
    }

    let mut eintrag = json!({ "platform": body.platform.trim() });
    let felder = eintrag.as_object_mut().expect("json! baut hier ein Objekt");

    let url = body.rtmp_url.as_deref().map(str::trim).unwrap_or_default();
    let key = body
        .stream_key
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    match (url.is_empty(), key.is_empty()) {
        // Beides da: Ziel anlegen oder ersetzen.
        (false, false) => {
            felder.insert("rtmp_url".into(), json!(url));
            felder.insert("stream_key".into(), json!(key));
        }
        // Beides leer: nur das Profil eines vorhandenen Ziels aendern.
        (true, true) => {}
        // Halb ausgefuellt ist immer ein Fehler in der Anfrage. Das Relay
        // lehnt es ebenfalls ab, aber ohne Text.
        _ => {
            return Err(fehler(
                StatusCode::BAD_REQUEST,
                "Adresse und Stream-Key gehören zusammen: entweder beide oder keins von beidem.",
            ))
        }
    }

    if let Some(enabled) = body.enabled {
        felder.insert("enabled".into(), json!(enabled));
    }

    let werte = match (&body.profil, body.manuell) {
        (Some(name), _) => Some(
            profil_aufloesen(name)
                .ok_or_else(|| fehler(StatusCode::BAD_REQUEST, "unbekanntes Profil"))?,
        ),
        (None, Some(manuell)) => Some(
            manuell_pruefen(manuell).map_err(|grund| fehler(StatusCode::BAD_REQUEST, &grund))?,
        ),
        (None, None) => None,
    };
    if let Some((w, h, f, b)) = werte {
        felder.insert("width".into(), json!(w));
        felder.insert("height".into(), json!(h));
        felder.insert("fps".into(), json!(f));
        felder.insert("bitrate_kbps".into(), json!(b));
    }

    // Ein Aufruf ohne Adresse, ohne Key und ohne Profil aendert nichts und
    // meldete trotzdem Erfolg. Das sieht in der Oberflaeche aus wie
    // gespeichert.
    if felder.len() == 1 {
        return Err(fehler(
            StatusCode::BAD_REQUEST,
            "Nichts zu speichern: weder Zugangsdaten noch Qualität angegeben.",
        ));
    }
    Ok(eintrag)
}

/// Die Grenzenkataloge des Relays, damit die Oberflaeche freie Zahlen
/// einordnen kann, statt sie doppelt zu pflegen.
pub async fn caps_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
) -> Result<Json<Value>, Response> {
    // Die Caps sind kein Geheimnis, aber der Weg zum Relay ist es: ohne
    // Sessionpruefung waere das ein offener Proxy auf ein internes Secret.
    partner_id(&pool, &auth).await?;
    let wert = relay_json(reqwest::Method::GET, "/v1/caps", None).await?;
    Ok(Json(wert))
}

/// Der Relay-Pfad zum Speichern eines Ziels.
///
/// `/v1/me/destinations` und nicht `/v1/admin/destinations`: nur dieser
/// Endpunkt nimmt eine Aenderung ohne Stream-Key an, und genau daran scheiterte
/// vorher jede Qualitaetsaenderung an einem eingerichteten Ziel.
pub(crate) const RELAY_ZIEL_PFAD: &str = "/v1/me/destinations";

/// Die Huelle, die `PutDestinations` in rs-relay erwartet
/// (`rs-relay/src/api/user.rs`): Streamer-ID und eine Liste von Zielen.
///
/// Eigene Funktion, damit die Form pruefbar ist und nicht nur im Handler
/// steht, wo sie ohne laufendes Relay niemand zu Gesicht bekommt.
fn nutzlast_fuer(streamer_id: i64, eintrag: Value) -> Value {
    json!({ "streamer_id": streamer_id, "destinations": [eintrag] })
}

/// Speichert ein Ziel: Zugangsdaten, Qualitaet oder beides.
///
/// Die Antwort ist die volle Zielliste samt `requested` und `effective`, also
/// genau das, was die Oberflaeche danach anzeigen will.
pub async fn put_destination_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Json(body): Json<DestinationBody>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&pool, &auth).await?;
    let mut eintrag = ziel_nutzlast(&body)?;
    let platform = eintrag["platform"]
        .as_str()
        .ok_or_else(|| fehler(StatusCode::BAD_REQUEST, "Plattform fehlt"))?
        .to_owned();
    let generation = if eintrag.get("stream_key").is_some() {
        tb_raid::target_generation::activate_manual(&pool, &id.to_string(), &platform).await
    } else {
        tb_raid::target_generation::snapshot(&pool, &id.to_string(), &platform)
            .await
            .and_then(|s| {
                if s.enabled && !s.disconnect_pending {
                    Ok(s.generation)
                } else {
                    Err(sqlx::Error::Protocol("Verbindung ist getrennt".into()))
                }
            })
    }
    .map_err(|_| {
        fehler(
            StatusCode::CONFLICT,
            "Die Verbindung wurde geändert. Bitte den aktuellen Stand laden.",
        )
    })?;
    eintrag["connection_generation"] = json!(generation);
    let wert = relay_json(
        reqwest::Method::PUT,
        RELAY_ZIEL_PFAD,
        Some(nutzlast_fuer(id, eintrag)),
    )
    .await?;
    if wert.get("ok").and_then(Value::as_bool) != Some(true)
        || wert
            .get("connection_generations")
            .and_then(|g| g.get(&platform))
            .and_then(Value::as_i64)
            != Some(generation)
    {
        return Err(fehler(
            StatusCode::BAD_GATEWAY,
            "Uplink hat das Speichern noch nicht bestätigt. Bitte den aktuellen Stand laden.",
        ));
    }
    Ok(Json(wert))
}

// ───────────────────────────────────────────────────────────────────────────
// Stream-Key hinterlegen und Trennen
// ───────────────────────────────────────────────────────────────────────────

/// Ingest-Adresse, unter der Twitch den Stream-Key erwartet. Twitch loest
/// `live.twitch.tv` selbst auf den naechsten Ingest auf.
pub const TWITCH_RTMP_URL: &str = "rtmp://live.twitch.tv/app";

/// Die Ingest-Adresse je Plattform. `None` heisst: fuer diese Plattform gibt
/// es noch keinen Stream-Key-Weg, das Ziel bleibt ein Eintrag von Hand.
fn rtmp_url_fuer(platform: &str) -> Option<&'static str> {
    (platform == PLATFORM_TWITCH).then_some(TWITCH_RTMP_URL)
}

/// Der Relay-Pfad zum Loeschen eines Ziels.
fn ziel_loesch_pfad(streamer_id: i64, platform: &str, generation: i64) -> String {
    format!(
        "{RELAY_ZIEL_PFAD}/{platform}?streamer_id={streamer_id}&connection_generation={generation}"
    )
}

/// Antworten des Relays in einen Fehlertext ohne Nutzlast verwandeln: die
/// Nutzlast koennte Feldnamen des Ziels tragen, der Text landet im Log.
fn relay_fehler(r: Response) -> String {
    format!("relay HTTP {}", r.status().as_u16())
}

/// Der Weg zu den Uplink-Zielen im Relay, als Trait, damit Nachlauf und
/// Trennen ohne laufendes Relay pruefbar sind.
#[async_trait::async_trait]
pub trait RelayZiele: Send + Sync {
    /// Legt das Ziel der Plattform an oder ersetzt Adresse und Key. Andere
    /// Ziele und die Qualitaetsstufe des Ziels bleiben stehen.
    async fn ziel_setzen(
        &self,
        streamer_id: i64,
        platform: &str,
        rtmp_url: &str,
        stream_key: &str,
        generation: i64,
    ) -> Result<(), String>;
    /// Entfernt das Ziel der Plattform. `false` heisst: es gab keins.
    async fn ziel_loeschen(
        &self,
        streamer_id: i64,
        platform: &str,
        generation: i64,
    ) -> Result<bool, String>;
}

/// Echte Anbindung ueber [`relay_json`] (`X-Relay-Auth`, Basis-URL aus der
/// Umgebung wie bei jedem anderen Relay-Aufruf des Dashboards).
#[derive(Default)]
pub struct HttpRelayZiele {
    /// Nur fuer Tests: (Basis-URL, Secret) fest statt aus der Umgebung.
    verbindung: Option<(String, String)>,
}

impl HttpRelayZiele {
    async fn aufruf(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        match &self.verbindung {
            Some((base, secret)) => relay_json_mit(base, secret, method, path, body).await,
            None => relay_json(method, path, body).await,
        }
        .map_err(relay_fehler)
    }
}

#[async_trait::async_trait]
impl RelayZiele for HttpRelayZiele {
    async fn ziel_setzen(
        &self,
        streamer_id: i64,
        platform: &str,
        rtmp_url: &str,
        stream_key: &str,
        generation: i64,
    ) -> Result<(), String> {
        let reply = self
            .aufruf(
                reqwest::Method::PUT,
                RELAY_ZIEL_PFAD,
                Some(json!({
                    "streamer_id": streamer_id,
                    "destinations": [{
                        "platform": platform,
                        "rtmp_url": rtmp_url,
                        "stream_key": stream_key,
                        "connection_generation":generation,
                    }],
                })),
            )
            .await?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true)
            || reply
                .get("connection_generations")
                .and_then(|g| g.get(platform))
                .and_then(Value::as_i64)
                != Some(generation)
        {
            return Err("Der Uplink-Dienst hat diese Zielgeneration nicht bestätigt.".into());
        }
        Ok(())
    }

    async fn ziel_loeschen(
        &self,
        streamer_id: i64,
        platform: &str,
        generation: i64,
    ) -> Result<bool, String> {
        match self
            .aufruf(
                reqwest::Method::DELETE,
                &ziel_loesch_pfad(streamer_id, platform, generation),
                None,
            )
            .await
        {
            Ok(wert)
                if wert.get("connection_generation").and_then(Value::as_i64)
                    == Some(generation) =>
            {
                wert.get("deleted").and_then(Value::as_bool).ok_or_else(|| {
                    "Der Uplink-Dienst hat die Entfernung nicht bestätigt.".to_string()
                })
            }
            Ok(_) => Err("Der Uplink-Dienst hat die Zielgeneration nicht bestätigt.".into()),
            Err(fehler) => Err(fehler),
        }
    }
}

/// Der Weg zum Stream-Key und zum Widerruf bei Twitch, als Trait fuer die Tests.
#[async_trait::async_trait]
pub trait PlattformKonto: Send + Sync {
    /// Stream-Key des Broadcasters. Der Rueckgabewert ist ein Geheimnis und
    /// darf nie in ein Log.
    async fn stream_key(&self, access_token: &str, broadcaster_id: &str) -> Result<String, String>;
    /// Token bei der Plattform zuruecknehmen.
    async fn widerrufen(&self, access_token: &str) -> Result<(), String>;
}

/// Echte Twitch-Anbindung ueber den vorhandenen Helix-Client.
pub struct HelixKonto {
    helix: tb_transport_twitch::HelixClient,
}

impl HelixKonto {
    /// `None`, wenn Client-ID oder Secret fehlen: dann bleibt der Nachlauf aus,
    /// statt gegen einen halb gebauten Client zu laufen.
    pub fn aus_konfiguration() -> Option<Self> {
        helix_aus_konfiguration().map(|helix| Self { helix })
    }
}

/// Ein Twitch-Fehler als Logtext. Bewusst ohne Antwortkoerper: bei einem
/// erfolgreichen Stream-Key-Abruf staende der Key darin, und ein Fehlerkoerper
/// von Twitch traegt keine Information, die der Statuscode nicht schon hat.
fn fehlertext(error: tb_transport_twitch::user_token::UserTokenError) -> String {
    use tb_transport_twitch::user_token::UserTokenError as E;
    match error {
        E::InvalidClient => "invalid_client".into(),
        E::InvalidGrant => "invalid_grant".into(),
        E::Other(_) => "Die Plattform konnte den Zugang gerade nicht bestätigen.".into(),
    }
}

#[async_trait::async_trait]
impl PlattformKonto for HelixKonto {
    async fn stream_key(&self, access_token: &str, broadcaster_id: &str) -> Result<String, String> {
        self.helix
            .fetch_stream_key(access_token, broadcaster_id)
            .await
            .map_err(fehlertext)
    }
    async fn widerrufen(&self, access_token: &str) -> Result<(), String> {
        self.helix
            .revoke_user_token(access_token)
            .await
            .map_err(fehlertext)
    }
}

/// Ergebnis des Stream-Key-Nachlaufs.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamKeyStand {
    /// Key geholt und als Uplink-Ziel hinterlegt.
    Hinterlegt,
    /// Es gibt keine nutzbare Verbindung: der Streamer muss erst verbinden.
    KeineVerbindung,
    /// Der Grant traegt das Stream-Key-Recht nicht (alter Raid-Grant).
    RechtFehlt,
    /// Twitch oder das Relay haben nicht mitgespielt. Die Verbindung bleibt
    /// stehen; der Streamer kann es erneut versuchen oder den Key eintragen.
    Fehlgeschlagen,
}

/// Holt den Stream-Key bei Twitch und legt ihn als Uplink-Ziel im Relay ab.
///
/// Der Key wandert von Twitch direkt ins Relay und wird nirgends geloggt, auch
/// nicht im Fehlerfall: ein Fehlertext von Twitch traegt keine Information, die
/// der Statuscode nicht schon hat, und bei Erfolg staende der Key im Rumpf.
pub async fn stream_key_hinterlegen(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    konto: &dyn PlattformKonto,
    relay: &dyn RelayZiele,
    streamer_id: i64,
    platform: &str,
) -> StreamKeyStand {
    let Some(rtmp_url) = rtmp_url_fuer(platform) else {
        return StreamKeyStand::KeineVerbindung;
    };
    let uid = streamer_id.to_string();
    // Ueber den gemeinsamen Token-Weg, nicht direkt aus der Zeile: der
    // gespeicherte Access-Token haelt vier Stunden, und der Hintergrund-Refresh
    // im Bot fasst nur raid-aktivierte Streamer an. Wer Raids aus hat, haette
    // hier sonst nach kurzer Zeit ein totes Token und bekaeme bei jedem Klick
    // dieselbe Bitte, es noch einmal zu versuchen.
    let (tokens, scopes) = match super::platform_token::gueltiger_twitch_token(
        pool,
        config,
        streamer_id,
        chrono::Utc::now(),
    )
    .await
    {
        Ok(v) => v,
        Err(super::platform_token::TokenFehler::KeineVerbindung)
        | Err(super::platform_token::TokenFehler::NeuVerbinden) => {
            return StreamKeyStand::KeineVerbindung
        }
        Err(super::platform_token::TokenFehler::NichtLieferbar) => {
            return StreamKeyStand::Fehlgeschlagen
        }
    };
    if !scopes.iter().any(|s| s.trim() == "channel:read:stream_key") {
        return StreamKeyStand::RechtFehlt;
    }
    let key = match konto.stream_key(&tokens.access_token, &uid).await {
        Ok(k) => k,
        Err(error) => {
            tracing::warn!(streamer_id, platform, %error, "uplink: Stream-Key nicht abrufbar");
            return StreamKeyStand::Fehlgeschlagen;
        }
    };
    if !matches!(
        tb_raid::target_generation::check_current(
            pool,
            &uid,
            platform,
            tokens.connection_generation
        )
        .await,
        Ok(true)
    ) {
        return StreamKeyStand::KeineVerbindung;
    }
    match relay
        .ziel_setzen(
            streamer_id,
            platform,
            rtmp_url,
            &key,
            tokens.connection_generation,
        )
        .await
    {
        Ok(()) => StreamKeyStand::Hinterlegt,
        Err(error) => {
            tracing::warn!(streamer_id, platform, %error, "uplink: Uplink-Ziel nicht gespeichert");
            StreamKeyStand::Fehlgeschlagen
        }
    }
}

/// Ergebnis des Trennens.
#[derive(Debug, PartialEq, Eq)]
pub enum TrennenErgebnis {
    /// Ziel dieser Generation entfernt; Uplink-Zugang beendet. Gemeinsame Grants bleiben.
    Getrennt,
    /// Fuer diese Plattform gibt es noch keinen Verbinden-Weg, also auch
    /// nichts zu trennen. Ein `ok` waere hier eine Falschaussage: es hat
    /// niemand etwas getan.
    KeinWeg,
    /// Relay-Abschluss unbestätigt; reservierte Trennung bleibt dauerhaft offen.
    RelayFehler,
    /// Lokaler Abschluss fehlgeschlagen oder inzwischen durch neue Verbindung überholt.
    SpeicherFehler,
}

/// Uplink-Trennung: bestätigtes Ziel-DELETE, danach atomarer Uplink-Intent.
/// Gemeinsame Twitch-Tokens für Bot und Raids bleiben erhalten.
pub async fn trennen(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    _konto: &dyn PlattformKonto,
    relay: &dyn RelayZiele,
    streamer_id: i64,
    platform: &str,
) -> TrennenErgebnis {
    if rtmp_url_fuer(platform).is_none() {
        return TrennenErgebnis::KeinWeg;
    }
    let generation = match tb_raid::target_generation::begin_disconnect(
        pool,
        &streamer_id.to_string(),
        platform,
    )
    .await
    {
        Ok(g) => g,
        Err(_) => return TrennenErgebnis::SpeicherFehler,
    };
    if !matches!(
        relay.ziel_loeschen(streamer_id, platform, generation).await,
        Ok(true)
    ) {
        return TrennenErgebnis::RelayFehler;
    }
    let writer = tb_raid::auth_writer::AuthWriter::new(pool.clone(), config.cipher.clone());
    if !matches!(
        writer
            .finish_uplink_disconnect(&streamer_id.to_string(), generation)
            .await,
        Ok(true)
    ) {
        return TrennenErgebnis::SpeicherFehler;
    }
    TrennenErgebnis::Getrennt
}

/// Prueft den Plattformnamen aus dem Pfad.
fn plattform_pruefen(roh: &str) -> Result<String, Response> {
    let platform = roh.trim().to_lowercase();
    if !PLATTFORMEN.contains(&platform.as_str()) {
        return Err(fehler(StatusCode::BAD_REQUEST, "unbekannte Plattform"));
    }
    Ok(platform)
}

fn nicht_eingerichtet() -> Response {
    fehler(
        StatusCode::SERVICE_UNAVAILABLE,
        "Der Uplink ist auf diesem Server noch nicht eingerichtet.",
    )
}

/// `POST /twitch/api/v2/uplink/connect/{platform}/disconnect`: Cookie-Session
/// plus CSRF wie jede Schreibroute des Dashboards.
pub async fn disconnect_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<PlatformTokenConfig>>,
    auth: DashboardAuthLevel,
    Path(platform): Path<String>,
) -> Response {
    let platform = match plattform_pruefen(&platform) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(Extension(config)) = config else {
        return nicht_eingerichtet();
    };
    let Some(konto) = HelixKonto::aus_konfiguration() else {
        return nicht_eingerichtet();
    };
    let id = match partner_id(&pool, &auth).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if platform == "kick" || platform == "youtube" {
        return plattform_trennen_antwort(&pool, &config, id, &platform).await;
    }
    match trennen(
        &pool,
        &config,
        &konto,
        &HttpRelayZiele::default(),
        id,
        &platform,
    )
    .await
    {
        TrennenErgebnis::Getrennt => Json(json!({ "ok": true })).into_response(),
        TrennenErgebnis::KeinWeg => fehler(
            StatusCode::CONFLICT,
            "Diese Plattform lässt sich noch nicht verbinden, also gibt es auch nichts zu trennen.",
        ),
        TrennenErgebnis::RelayFehler => fehler(
            StatusCode::BAD_GATEWAY,
            "Die Trennung ist noch nicht bestätigt. Bitte erneut auf Trennen klicken.",
        ),
        TrennenErgebnis::SpeicherFehler => fehler(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Verbindung konnte nicht getrennt werden. Bitte später noch einmal versuchen.",
        ),
    }
}

async fn plattform_trennen_antwort(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    streamer_id: i64,
    platform: &str,
) -> Response {
    use super::plattform_connect::{plattform_trennen, PlattformTrennenErgebnis};
    use super::plattform_oauth::{GoogleOAuth, KickApi, KickOAuth, YouTubeApi};

    let kick = KickOAuth::aus_konfiguration();
    let youtube = GoogleOAuth::aus_konfiguration();
    let ergebnis = plattform_trennen(
        pool,
        config,
        &HttpRelayZiele::default(),
        kick.as_ref().map(|c| c as &dyn KickApi),
        youtube.as_ref().map(|c| c as &dyn YouTubeApi),
        streamer_id,
        platform,
    )
    .await;
    match ergebnis {
        PlattformTrennenErgebnis::Getrennt => Json(json!({ "ok": true })).into_response(),
        PlattformTrennenErgebnis::RelayFehler => fehler(
            StatusCode::BAD_GATEWAY,
            "Die Trennung ist noch nicht bestätigt. Bitte erneut auf Trennen klicken.",
        ),
        PlattformTrennenErgebnis::SpeicherFehler => fehler(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Verbindung konnte nicht getrennt werden. Bitte später noch einmal versuchen.",
        ),
    }
}

/// `POST /twitch/api/v2/uplink/connect/{platform}/streamkey`: holt den
/// Stream-Key nach und legt ihn als Uplink-Ziel ab. Die Oberflaeche ruft das
/// nach der Rueckkehr aus dem Twitch-Dialog und ueber "Stream-Key erneut holen".
pub async fn streamkey_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<PlatformTokenConfig>>,
    auth: DashboardAuthLevel,
    Path(platform): Path<String>,
) -> Response {
    let platform = match plattform_pruefen(&platform) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(Extension(config)) = config else {
        return nicht_eingerichtet();
    };
    let id = match partner_id(&pool, &auth).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if platform == "kick" || platform == "youtube" {
        let stand = super::plattform_connect::plattform_stream_key_hinterlegen(
            &pool,
            &config,
            &HttpRelayZiele::default(),
            id,
            &platform,
            chrono::Utc::now(),
        )
        .await;
        return streamkey_antwort(stand);
    }
    let Some(konto) = HelixKonto::aus_konfiguration() else {
        return nicht_eingerichtet();
    };
    streamkey_antwort(
        stream_key_hinterlegen(
            &pool,
            &config,
            &konto,
            &HttpRelayZiele::default(),
            id,
            &platform,
        )
        .await,
    )
}

fn streamkey_antwort(stand: StreamKeyStand) -> Response {
    match stand {
        StreamKeyStand::Hinterlegt => Json(json!({ "ok": true })).into_response(),
        StreamKeyStand::KeineVerbindung => fehler(
            StatusCode::CONFLICT,
            "Für diese Plattform gibt es noch keine Verbindung. Erst verbinden, dann klappt auch der Stream-Key.",
        ),
        StreamKeyStand::RechtFehlt => fehler(
            StatusCode::CONFLICT,
            "Deine Verbindung ist älter und darf den Stream-Key noch nicht lesen. Einmal neu verbinden, dann geht es.",
        ),
        StreamKeyStand::Fehlgeschlagen => fehler(
            StatusCode::BAD_GATEWAY,
            "Der Stream-Key kam gerade nicht durch. Bitte noch einmal versuchen; du kannst ihn auch von Hand eintragen.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;

    #[tokio::test]
    async fn proxy_rejects_successful_but_invalid_json() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        assert!(relay_json_mit(
            &server.uri(),
            "synthetic-api",
            reqwest::Method::GET,
            "/v1/me",
            None
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn proxy_does_not_forward_its_credential_to_a_redirect() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
        let first = MockServer::start().await;
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(0)
            .mount(&target)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", target.uri()))
            .mount(&first)
            .await;
        assert!(relay_json_mit(
            &first.uri(),
            "synthetic-api",
            reqwest::Method::GET,
            "/v1/me",
            None
        )
        .await
        .is_err());
    }

    #[test]
    fn bekannte_profile_loesen_auf() {
        assert_eq!(profil_aufloesen("1080p60"), Some((1920, 1080, 60, 6000)));
        assert_eq!(profil_aufloesen("480p30"), Some((854, 480, 30, 1500)));
    }

    #[test]
    fn unbekannte_profile_werden_abgelehnt() {
        assert_eq!(profil_aufloesen("2160p60"), None);
        assert_eq!(profil_aufloesen(""), None);
    }

    #[test]
    fn das_hoechste_profil_ist_1440p() {
        assert_eq!(profil_aufloesen("1440p60"), Some((2560, 1440, 60, 12000)));
    }

    #[test]
    fn leerraum_um_den_namen_stoert_nicht() {
        assert_eq!(profil_aufloesen("  720p60 "), Some((1280, 720, 60, 4500)));
    }

    #[test]
    fn keine_fertige_stufe_geht_ueber_die_twitch_empfehlung() {
        // Der manuelle Modus darf jede Zahl tragen, das ist der Sinn der
        // Sache. Die fertigen Stufen sind etwas anderes: sie sind unser
        // Vorschlag, und ein Vorschlag ueber dem, was Twitch selbst nennt,
        // waere keiner. Wer hier absichtlich hoeher gehen will, nimmt den
        // manuellen Modus.
        for (name, w, h, _f, b) in PROFILE {
            assert!(
                w <= TWITCH_EMPFOHLENE_BREITE,
                "{name} ist breiter als Twitch empfiehlt"
            );
            assert!(
                h <= TWITCH_EMPFOHLENE_HOEHE,
                "{name} ist hoeher als Twitch empfiehlt"
            );
            assert!(
                b <= TWITCH_EMPFOHLENE_BITRATE,
                "{name} liegt ueber der Twitch-Empfehlung"
            );
        }
    }

    #[test]
    fn die_namen_im_katalog_sind_eindeutig() {
        // Zwei gleiche Namen: `find` nimmt den ersten, der zweite waere tot,
        // und in der Auswahlliste staende derselbe Eintrag zweimal.
        let mut namen: Vec<&str> = PROFILE.iter().map(|(n, ..)| *n).collect();
        namen.sort_unstable();
        let vorher = namen.len();
        namen.dedup();
        assert_eq!(namen.len(), vorher);
    }

    /// Vier Zahlen, wie sie aus dem Formular kaemen. Spart in jedem Test die
    /// vier `Formularzahl::neu`-Huellen.
    fn manuell(width: i64, height: i64, fps: i64, bitrate_kbps: i64) -> ManuellesProfil {
        ManuellesProfil {
            width: Formularzahl::neu(width),
            height: Formularzahl::neu(height),
            fps: Formularzahl::neu(fps),
            bitrate_kbps: Formularzahl::neu(bitrate_kbps),
        }
    }

    fn body(platform: &str) -> DestinationBody {
        DestinationBody {
            platform: platform.into(),
            rtmp_url: None,
            stream_key: None,
            profil: None,
            manuell: None,
            enabled: None,
        }
    }

    /// Der Kern der Sache: eine Qualitaetsstufe laesst sich aendern, ohne den
    /// Stream-Key erneut einzutippen. Vorher ging das nicht, und deshalb sah
    /// es aus, als wuerde die Auswahl nicht gespeichert.
    #[test]
    fn nur_das_profil_aendern_geht_ohne_stream_key() {
        let mut b = body("twitch");
        b.profil = Some("720p60".into());
        let wert = ziel_nutzlast(&b).expect("darf durchgehen");
        assert_eq!(wert["platform"], "twitch");
        assert_eq!(wert["height"], 720);
        assert_eq!(wert["bitrate_kbps"], 4500);
        assert!(wert.get("stream_key").is_none());
    }

    #[test]
    fn manuelle_werte_gehen_durch() {
        let mut b = body("youtube");
        b.manuell = Some(manuell(2560, 1440, 60, 18000));
        let wert = ziel_nutzlast(&b).expect("darf durchgehen");
        assert_eq!(wert["width"], 2560);
        assert_eq!(wert["bitrate_kbps"], 18000);
    }

    /// Die Umkehrung des alten Tests: hohe Werte gehen durch.
    ///
    /// Hier stand `manuelle_werte_ueber_dem_ingest_deckel_fallen_auf` und hat
    /// 4K und 60000 kbps abgelehnt. Beides lehnt jetzt niemand mehr ab. Ob die
    /// Plattform das annimmt, entscheidet die Plattform; ob der Server das
    /// traegt, entscheidet das Punktebudget in rs-relay, und das antwortet mit
    /// einem Grund statt mit einem Formularfehler.
    #[test]
    fn hohe_werte_werden_nicht_mehr_abgelehnt() {
        assert!(manuell_pruefen(manuell(3840, 2160, 60, 6000)).is_ok());
        assert!(manuell_pruefen(manuell(1920, 1080, 60, 60000)).is_ok());
        // Und der Fall aus dem Betrieb: 16000 kbps an Twitch. Genau diese Zahl
        // hat der Streamer eingetragen und "liegt über unserem Maximum"
        // gelesen, ohne zu erfahren, wessen Maximum gemeint war.
        assert!(manuell_pruefen(manuell(2560, 1440, 60, 16000)).is_ok());
    }

    /// Der Weg, auf dem der Fehler entstanden ist: nicht der Aufruf von
    /// `manuell_pruefen`, sondern das Deserialisieren davor.
    ///
    /// Mit `i32`-Feldern brach `serde` hier ab, axum machte daraus eine 422
    /// mit Klartextkoerper, und die Oberflaeche zeigte "Speichern hat nicht
    /// geklappt." ohne zu sagen, welches Feld gemeint ist.
    ///
    /// Bewusst aus Text und nicht aus `json!`: `serde_json` liest die Zahl
    /// hier genauso ein wie im Betrieb, samt der Stelle, an der eine
    /// zwanzigstellige Zahl zu Gleitkomma wird.
    fn aus_json(bitrate: &str) -> Result<DestinationBody, serde_json::Error> {
        serde_json::from_str(&format!(
            r#"{{"platform":"twitch","manuell":{{"width":1920,"height":1080,"fps":60,"bitrate_kbps":{bitrate}}}}}"#
        ))
    }

    #[test]
    fn ein_vertipper_sprengt_das_deserialisieren_nicht_mehr() {
        for zahl in [
            "9999999999",
            "99999999999999999999999",
            "6000.5",
            "-9999999999",
        ] {
            let b = aus_json(zahl).expect("darf nicht schon am Typ scheitern");
            let antwort = ziel_nutzlast(&b).expect_err("und wird danach abgelehnt");
            assert_eq!(antwort.status(), StatusCode::BAD_REQUEST, "{zahl}");
        }
    }

    /// Die Gegenprobe: eine ganz normale Zahl geht denselben Weg durch.
    #[test]
    fn eine_normale_zahl_geht_durch_dieselbe_huelle() {
        let b = aus_json("16000").expect("gueltig");
        let wert = ziel_nutzlast(&b).expect("darf durchgehen");
        assert_eq!(wert["bitrate_kbps"], 16000);
    }

    #[test]
    fn die_meldung_zu_einem_vertipper_nennt_das_feld() {
        // Ohne Feldnamen sucht der Streamer in vier Feldern nach dem Fehler.
        let fehler = manuell_pruefen(manuell(1920, 1080, 60, 9_999_999_999))
            .expect_err("passt nicht in den Spaltentyp des Relays");
        assert!(fehler.starts_with("Bitrate"), "{fehler}");
        let fehler = manuell_pruefen(ManuellesProfil {
            bitrate_kbps: Formularzahl(None),
            ..manuell(1920, 1080, 60, 6000)
        })
        .expect_err("keine ganze Zahl");
        assert!(fehler.starts_with("Bitrate"), "{fehler}");
    }

    /// Die Umrechnung nach `i32` ist der Spaltentyp des Relays und keine
    /// wieder eingefuehrte Obergrenze: alles, was dort hineinpasst, geht durch.
    #[test]
    fn der_groesste_speicherbare_wert_geht_weiterhin_durch() {
        assert!(manuell_pruefen(manuell(1920, 1080, 60, i32::MAX as i64)).is_ok());
    }

    /// yuv420p halbiert beide Achsen. Eine ungerade Kante laesst ffmpeg erst
    /// beim Start sterben, nicht beim Speichern, und dann steht der Stream.
    #[test]
    fn ungerade_kanten_werden_abgelehnt() {
        assert!(manuell_pruefen(manuell(1921, 1080, 60, 6000)).is_err());
    }

    #[test]
    fn null_und_negativ_sind_keine_werte() {
        for (w, h, f, b) in [
            (0, 1080, 60, 6000),
            (1920, 0, 60, 6000),
            (1920, 1080, 0, 6000),
            (1920, 1080, 60, -1),
        ] {
            assert!(manuell_pruefen(manuell(w, h, f, b)).is_err());
        }
    }

    #[test]
    fn stufe_und_eigene_werte_zusammen_sind_ein_fehler() {
        let mut b = body("twitch");
        b.profil = Some("1080p60".into());
        b.manuell = Some(manuell(1920, 1080, 60, 6000));
        assert!(ziel_nutzlast(&b).is_err());
    }

    #[test]
    fn halb_ausgefuellte_zugangsdaten_sind_ein_fehler() {
        let mut b = body("twitch");
        b.rtmp_url = Some("rtmp://live.twitch.tv/app".into());
        assert!(ziel_nutzlast(&b).is_err());
    }

    /// Ohne diese Pruefung meldete ein Aufruf, der nichts traegt, Erfolg. In
    /// der Oberflaeche sieht das aus wie gespeichert.
    #[test]
    fn ein_aufruf_ohne_inhalt_ist_ein_fehler() {
        assert!(ziel_nutzlast(&body("twitch")).is_err());
    }

    #[test]
    fn unbekannte_plattformen_kommen_nicht_durch() {
        let mut b = body("facebook");
        b.profil = Some("1080p60".into());
        assert!(ziel_nutzlast(&b).is_err());
    }

    #[test]
    fn alle_vier_plattformen_sind_waehlbar() {
        for platform in PLATTFORMEN {
            let mut b = body(platform);
            b.profil = Some("1080p60".into());
            assert!(ziel_nutzlast(&b).is_ok(), "{platform} kommt nicht durch");
        }
    }

    #[test]
    fn abschalten_geht_ohne_qualitaetsangabe() {
        let mut b = body("kick");
        b.enabled = Some(false);
        let wert = ziel_nutzlast(&b).expect("darf durchgehen");
        assert_eq!(wert["enabled"], false);
        assert!(wert.get("width").is_none());
    }

    /// Die Form, die rs-relay erwartet. Ohne diesen Test faellt ein
    /// vertippter Feldname erst im Betrieb auf, und zwar als nackte 400 vom
    /// Relay ohne Hinweis darauf, welches Feld gemeint war.
    #[test]
    fn die_nutzlast_hat_die_huelle_des_relays() {
        let mut b = body("twitch");
        b.profil = Some("1080p60".into());
        let nutzlast = nutzlast_fuer(4711, ziel_nutzlast(&b).expect("darf durchgehen"));
        assert_eq!(nutzlast["streamer_id"], 4711);
        let ziele = nutzlast["destinations"].as_array().expect("Liste");
        assert_eq!(ziele.len(), 1, "eine Anfrage traegt genau ein Ziel");
        assert_eq!(ziele[0]["platform"], "twitch");
        assert_eq!(ziele[0]["height"], 1080);
    }

    /// Der Weg ueber den Admin-Endpunkt war der Grund, warum sich eine
    /// Qualitaetsstufe ohne Stream-Key nicht speichern liess.
    #[test]
    fn gespeichert_wird_ueber_den_nutzer_endpunkt() {
        assert_eq!(RELAY_ZIEL_PFAD, "/v1/me/destinations");
    }

    #[test]
    fn ohne_session_gibt_es_keine_identitaet() {
        assert!(twitch_identitaet(&DashboardAuthLevel::None).is_err());
    }

    #[test]
    fn wartelistenverwaltung_erlaubt_admin_und_owner_ohne_admin_modus() {
        let admin = DashboardAuthLevel::Admin { actor: None };
        // Owner nur mit Twitch-Identität, ohne aktiven Admin-Modus.
        let owner_partner = DashboardAuthLevel::Partner {
            twitch_login: "earlysalty".into(),
            twitch_user_id: "42".into(),
            display_name: "Early".into(),
        };
        // Normaler Partner bleibt draußen.
        let fremder_partner = DashboardAuthLevel::Partner {
            twitch_login: "someone".into(),
            twitch_user_id: "7".into(),
            display_name: "Someone".into(),
        };

        assert!(admin_pruefen(&admin).is_ok());
        assert!(admin_pruefen(&owner_partner).is_ok());
        assert_eq!(
            admin_pruefen(&fremder_partner).unwrap_err().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            admin_pruefen(&DashboardAuthLevel::None)
                .unwrap_err()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn waitlist_namen_werden_angehaengt_und_fehlende_bleiben_null() {
        let wert = json!({
            "entries": [
                {"streamer_id": 1367527782_i64, "requested_at": "x", "enabled": false},
                {"streamer_id": 99_i64, "requested_at": "y", "enabled": false}
            ]
        });
        let mut users = HashMap::new();
        users.insert(
            "1367527782".to_string(),
            TwitchUser {
                id: "1367527782".into(),
                login: "coolstreamer".into(),
                display_name: "CoolStreamer".into(),
                profile_image_url: None,
            },
        );

        assert_eq!(waitlist_ids(&wert), vec!["1367527782", "99"]);

        let angereichert = mit_namen(wert, &users);
        let e = &angereichert["entries"];
        // Aufgelöste ID trägt Login und Anzeigename.
        assert_eq!(e[0]["twitch_login"], "coolstreamer");
        assert_eq!(e[0]["display_name"], "CoolStreamer");
        // Unbekannte ID bleibt null, das Frontend fällt auf die ID zurück.
        assert_eq!(e[1]["twitch_login"], Value::Null);
        assert_eq!(e[1]["display_name"], Value::Null);
    }

    #[test]
    fn freischaltantwort_enthaelt_keinen_ingest_schluessel() {
        let antwort = freischaltung_antwort(42);
        assert_eq!(antwort["streamer_id"], 42);
        assert_eq!(antwort["enabled"], true);
        assert!(antwort.get("ingest_key").is_none());
        assert!(antwort.get("srt_hint").is_none());
    }

    #[test]
    fn ablehnen_laeuft_ueber_den_admin_pfad_und_traegt_die_id() {
        let pfad = admin_waitlist_eintrag_pfad(4711);
        assert_eq!(pfad, "/v1/admin/waitlist/4711");
        // Der Pfad entscheidet ueber das Secret. Rutschte er aus
        // `/v1/admin/` heraus, ginge das Ablehnen mit dem API-Secret hinaus
        // und das Relay wiese es ab, ohne dass hier etwas auffiele.
        assert_eq!(secret_name_fuer(&pfad), "RS_RELAY_ADMIN_SECRET");
    }

    #[test]
    fn admin_audit_label_ist_auch_ohne_twitch_actor_eindeutig() {
        assert_eq!(
            admin_actor_fuer_log(&DashboardAuthLevel::Admin { actor: None }),
            ("discord-admin", None)
        );
    }

    #[test]
    fn partner_id_wird_gelesen() {
        let auth = DashboardAuthLevel::Partner {
            twitch_login: "earlysalty".into(),
            twitch_user_id: "123".into(),
            display_name: "Early".into(),
        };
        let (login, id) = twitch_identitaet(&auth).unwrap();
        assert_eq!(login, "earlysalty");
        assert_eq!(id.parse::<i64>().unwrap(), 123);
    }

    /// Die Master-Session des Admin-Dashboards kommt genau so an: Login da,
    /// User-ID leer. Frueher endete das direkt im Fehler "twitch user id
    /// fehlt"; jetzt bleibt der Login fuer den DB-Lookup uebrig.
    #[test]
    fn master_session_behaelt_den_login_ohne_id() {
        let auth = DashboardAuthLevel::Partner {
            twitch_login: "earlysalty".into(),
            twitch_user_id: String::new(),
            display_name: "earlysalty".into(),
        };
        let (login, id) = twitch_identitaet(&auth).unwrap();
        assert_eq!(login, "earlysalty");
        assert!(id.trim().parse::<i64>().is_err());
    }

    fn zeit(roh: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(roh)
            .expect("Testzeit")
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn frischer_stand_entscheidet_live_oder_aus() {
        let jetzt = zeit("2026-08-22T12:00:00+00:00");
        let gerade = Some("2026-08-22T11:59:50+00:00");
        assert_eq!(live_bewerten(Some((1, gerade)), jetzt), "live");
        assert_eq!(live_bewerten(Some((0, gerade)), jetzt), "aus");
    }

    /// Der Kern der Sache: ein stehender Poller darf kein "ist nicht live"
    /// behaupten, auf das die Oberflaeche ein Aufdecken stuetzt.
    #[test]
    fn alter_stand_ist_unbekannt_statt_aus() {
        let jetzt = zeit("2026-08-22T12:00:00+00:00");
        let alt = Some("2026-08-22T11:50:00+00:00");
        assert_eq!(live_bewerten(Some((0, alt)), jetzt), "unbekannt");
        assert_eq!(live_bewerten(Some((1, alt)), jetzt), "unbekannt");
    }

    #[test]
    fn ohne_zeile_oder_ohne_zeit_bleibt_es_unbekannt() {
        let jetzt = zeit("2026-08-22T12:00:00+00:00");
        assert_eq!(live_bewerten(None, jetzt), "unbekannt");
        assert_eq!(live_bewerten(Some((1, None)), jetzt), "unbekannt");
        assert_eq!(
            live_bewerten(Some((1, Some("gestern"))), jetzt),
            "unbekannt"
        );
    }

    /// Eine schiefe Uhr auf der schreibenden Seite darf keine Frische
    /// vortaeuschen, sonst reichte ein Stand aus der Zukunft als Freibrief.
    #[test]
    fn stand_aus_der_zukunft_ist_unbekannt() {
        let jetzt = zeit("2026-08-22T12:00:00+00:00");
        let zukunft = Some("2026-08-22T12:30:00+00:00");
        assert_eq!(live_bewerten(Some((0, zukunft)), jetzt), "unbekannt");
    }

    /// Genau an der Grenze zaehlt der Stand noch, eine Sekunde darueber nicht.
    #[test]
    fn die_frist_gilt_genau() {
        let jetzt = zeit("2026-08-22T12:00:00+00:00");
        assert_eq!(
            live_bewerten(Some((0, Some("2026-08-22T11:55:00+00:00"))), jetzt),
            "aus"
        );
        assert_eq!(
            live_bewerten(Some((0, Some("2026-08-22T11:54:59+00:00"))), jetzt),
            "unbekannt"
        );
    }

    /// Admin-Pfade greifen zum Admin-Secret, alles andere zum API-Secret.
    ///
    /// Geprüft wird die Namenswahl, nicht ein gesetzter Wert. Ein Test, der
    /// dafür `set_var` benutzt, wäre ein Datenrennen auf `environ` mit jedem
    /// anderen Test im selben Binary, der die Umgebung anfasst, und davon gibt
    /// es in dieser Crate mehrere.
    #[test]
    fn me_traegt_dock_urls_und_verbindungen() {
        // Die vier Adressen kommen vom Relay und gehen unveraendert weiter.
        // Der Bot baut hier nichts nach; er weiss den Zugang gar nicht.
        let mut wert = json!({
            "freigeschaltet": true,
            "dock_url_vorhanden": true,
            "dock_urls": {
                "chat": "https://relay.test/dock/chat?t=abc",
                "activity": "https://relay.test/dock/activity?t=abc",
                "stream_info": "https://relay.test/dock/stream-info?t=abc",
                "points": "https://relay.test/dock/points?t=abc"
            }
        });
        me_anreichern(
            &mut wert,
            "live",
            &[("twitch".to_string(), "verbunden")],
            &["twitch".to_string()],
            &["twitch", "kick"],
        );
        assert_eq!(wert["live_status"], "live");
        assert_eq!(wert["dock_url_vorhanden"], true);
        assert_eq!(
            wert["dock_urls"]["chat"],
            "https://relay.test/dock/chat?t=abc"
        );
        assert_eq!(
            wert["dock_urls"]["points"],
            "https://relay.test/dock/points?t=abc"
        );
        let liste = wert["verbindungen"].as_array().unwrap();
        assert_eq!(liste.len(), PLATTFORMEN.len());
        assert_eq!(
            liste[0],
            json!({ "platform": "twitch", "status": "verbunden", "stream_key_vorhanden": true, "verbindbar": true })
        );
        assert!(liste[1..].iter().all(|e| e["status"] == "getrennt"));
        assert!(liste[1..]
            .iter()
            .all(|e| e["stream_key_vorhanden"] == false));
        let kick = liste.iter().find(|e| e["platform"] == "kick").unwrap();
        assert_eq!(kick["verbindbar"], true);
        let youtube = liste.iter().find(|e| e["platform"] == "youtube").unwrap();
        assert_eq!(youtube["verbindbar"], false);
    }

    #[test]
    fn ziel_plattformen_kommen_aus_der_relay_liste() {
        let wert = json!({ "destinations": [
            { "platform": "twitch", "rtmp_url": "rtmp://live.twitch.tv/app" },
            { "platform": "kick" }
        ]});
        assert_eq!(ziel_plattformen(&wert), vec!["twitch", "kick"]);
        assert!(ziel_plattformen(&json!({})).is_empty());
    }

    #[test]
    fn me_ohne_dock_url_meldet_keine_adresse() {
        let mut wert = json!({ "freigeschaltet": false });
        me_anreichern(&mut wert, "unbekannt", &[], &[], &[]);
        assert_eq!(wert["dock_url_vorhanden"], false);
        // `null`, nicht "Feld fehlt": die Oberflaeche soll einen aelteren
        // Relay nicht von einem Streamer ohne Adressen unterscheiden muessen.
        assert_eq!(wert["dock_urls"], Value::Null);
        assert!(wert.as_object().unwrap().contains_key("dock_urls"));
        assert!(wert["verbindungen"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["status"] == "getrennt"));

        let mut nein = json!({ "dock_url_vorhanden": false });
        me_anreichern(&mut nein, "aus", &[], &[], &[]);
        assert_eq!(nein["dock_url_vorhanden"], false);

        // Ein Zugang, den das Relay nur als Fingerabdruck kennt: vorhanden,
        // aber nicht anzeigbar. Beides muss nebeneinander stehen bleiben.
        let mut ohne_enc = json!({ "dock_url_vorhanden": true, "dock_urls": Value::Null });
        me_anreichern(&mut ohne_enc, "aus", &[], &[], &[]);
        assert_eq!(ohne_enc["dock_url_vorhanden"], true);
        assert_eq!(ohne_enc["dock_urls"], Value::Null);

        // Was kein Objekt ist, faellt auf `null`. Ein String kaeme in der
        // Oberflaeche als vier fehlende Felder an: leere Karte neben der
        // Meldung "vorhanden", ohne dass irgendwo stuende, warum.
        for kaputt in [json!("https://relay.test/dock/chat"), json!([]), json!(7)] {
            let mut wert = json!({ "dock_url_vorhanden": true, "dock_urls": kaputt });
            me_anreichern(&mut wert, "aus", &[], &[], &[]);
            assert_eq!(wert["dock_urls"], Value::Null);
        }
    }

    #[test]
    fn dock_token_rotate_traegt_streamer_in_der_query() {
        let pfad = dock_token_rotate_pfad(4242);
        assert_eq!(pfad, "/v1/me/dock-token/rotate?streamer_id=4242");
        assert_eq!(secret_name_fuer(&pfad), "RS_RELAY_API_SECRET");
    }

    #[test]
    fn admin_pfade_nehmen_das_admin_secret() {
        for pfad in [
            "/v1/admin/users",
            RELAY_ADMIN_WAITLIST_PFAD,
            RELAY_ADMIN_USERS_PFAD,
            "/v1/admin/sessions/7/kill?confirm=true",
        ] {
            assert_eq!(secret_name_fuer(pfad), "RS_RELAY_ADMIN_SECRET", "{pfad}");
        }
        for pfad in [
            "/v1/me",
            "/v1/caps",
            "/v1/me/destinations",
            "/v1/me/key/rotate",
            // Kein Admin-Pfad, auch wenn "admin" darin vorkommt: das Relay
            // kennt nur `/v1/admin/...`.
            "/v1/me/adminwunsch",
        ] {
            assert_eq!(secret_name_fuer(pfad), "RS_RELAY_API_SECRET", "{pfad}");
        }
    }

    #[test]
    fn admin_mit_actor_nutzt_dessen_identitaet() {
        let auth = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "42".into(),
                twitch_login: "earlysalty".into(),
            }),
        };
        assert_eq!(twitch_identitaet(&auth).unwrap(), ("earlysalty", "42"));
    }

    // ── Verbindungsstand ───────────────────────────────────────────────────

    fn scopes(liste: &[&str]) -> Vec<String> {
        liste.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn status_verbunden_mit_allen_uplink_scopes() {
        let voll = scopes(tb_raid::scope_profiles::UPLINK_SCOPES);
        assert_eq!(verbindungs_status(true, false, &voll), "verbunden");
    }

    /// Der Fall aus dem Betrieb: ein Streamer hat den Raid-Bot laengst
    /// autorisiert. Seine Tokens sind gueltig, aber sie duerfen weder den Chat
    /// lesen noch den Stream-Key holen. "Verbunden" waere hier eine
    /// Falschaussage: das Dock bliebe leer.
    #[test]
    fn status_neu_verbinden_bei_altem_raid_grant() {
        let alt = scopes(tb_raid::scope_profiles::FULL_STREAMER_SCOPES);
        assert_eq!(verbindungs_status(true, false, &alt), "rechte_ergaenzen");
        let basis = scopes(tb_raid::scope_profiles::BASE_STREAMER_SCOPES);
        assert_eq!(verbindungs_status(true, false, &basis), "rechte_ergaenzen");
    }

    #[test]
    fn status_getrennt_ohne_zeile() {
        // Ohne Tokens gibt es nichts zu erneuern; "neu verbinden" waere hier
        // der falsche Knopf, weil noch nie etwas verbunden war.
        assert_eq!(verbindungs_status(false, false, &[]), "getrennt");
        let voll = scopes(tb_raid::scope_profiles::UPLINK_SCOPES);
        assert_eq!(verbindungs_status(false, true, &voll), "getrennt");
    }

    #[test]
    fn status_neu_verbinden_bei_needs_reauth() {
        let voll = scopes(tb_raid::scope_profiles::UPLINK_SCOPES);
        assert_eq!(verbindungs_status(true, true, &voll), "neu_verbinden");
    }

    // ── Relay-Pfade ────────────────────────────────────────────────────────

    #[test]
    fn der_loesch_pfad_traegt_plattform_und_streamer() {
        assert_eq!(
            ziel_loesch_pfad(4242, "twitch", 1),
            "/v1/me/destinations/twitch?streamer_id=4242&connection_generation=1"
        );
        // Und er greift zum API-Secret, nicht zum Admin-Secret.
        assert_eq!(
            secret_name_fuer(&ziel_loesch_pfad(4242, "twitch", 1)),
            "RS_RELAY_API_SECRET"
        );
    }

    #[test]
    fn nur_twitch_hat_einen_stream_key_weg() {
        assert_eq!(rtmp_url_fuer("twitch"), Some(TWITCH_RTMP_URL));
        for fremd in ["kick", "youtube", "tiktok", ""] {
            assert_eq!(rtmp_url_fuer(fremd), None, "{fremd}");
        }
    }

    #[test]
    fn unbekannte_plattformen_kommen_gar_nicht_erst_durch() {
        assert_eq!(plattform_pruefen("  TWITCH ").unwrap(), "twitch");
        for fremd in ["rumble", "", "twitch2"] {
            let antwort = plattform_pruefen(fremd).expect_err("darf nicht durchgehen");
            assert_eq!(antwort.status(), StatusCode::BAD_REQUEST, "{fremd}");
        }
    }

    /// Der einzige Weg mit Aussenwirkung: Methode, Pfad, Header und Rumpf, die
    /// wirklich beim Relay ankommen. Die Attrappen darueber pruefen nur die
    /// Trait-Ebene.
    #[tokio::test]
    async fn http_relay_setzt_und_loescht_das_ziel_auf_den_richtigen_pfaden() {
        use wiremock::matchers::{body_partial_json, header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/v1/me/destinations"))
            .and(header("X-Relay-Auth", "geheim"))
            .and(body_partial_json(json!({
                "streamer_id": 4242,
                "destinations": [{
                    "platform": "twitch",
                    "rtmp_url": TWITCH_RTMP_URL,
                    "stream_key": "sk-1", "connection_generation":0
                }]
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "ok":true,"connection_generations":{"twitch":0} })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/v1/me/destinations/twitch"))
            .and(query_param("streamer_id", "4242"))
            .and(query_param("connection_generation", "1"))
            .and(header("X-Relay-Auth", "geheim"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "deleted": true,"connection_generation":1 })),
            )
            .expect(1)
            .mount(&server)
            .await;
        let relay = HttpRelayZiele {
            verbindung: Some((server.uri(), "geheim".into())),
        };
        relay
            .ziel_setzen(4242, "twitch", TWITCH_RTMP_URL, "sk-1", 0)
            .await
            .unwrap();
        assert!(relay.ziel_loeschen(4242, "twitch", 1).await.unwrap());
        // Eine unbekannte Route bestätigt keine Entfernung.
        assert!(relay.ziel_loeschen(4242, "kick", 1).await.is_err());
    }

    #[tokio::test]
    async fn ziel_loeschen_verlangt_eine_explizite_bestaetigung() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
        for reply in [
            json!({}),
            json!({"deleted":"true","connection_generation":1}),
            json!({"deleted":true,"connection_generation":2}),
            json!({"error":"synthetic-secret"}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("DELETE"))
                .respond_with(ResponseTemplate::new(200).set_body_json(reply))
                .mount(&server)
                .await;
            let relay = HttpRelayZiele {
                verbindung: Some((server.uri(), "synthetic-key".into())),
            };
            assert!(relay.ziel_loeschen(4242, "twitch", 1).await.is_err());
        }
    }

    /// Die Gegenprobe zum 404: ein echter Ausfall bleibt ein Fehler. Sonst
    /// wuerde das Trennen die Tokens wegwerfen, waehrend das Ziel im Relay
    /// weitersendet.
    #[tokio::test]
    async fn ein_relay_ausfall_beim_loeschen_bleibt_ein_fehler() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/me/destinations/twitch"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let relay = HttpRelayZiele {
            verbindung: Some((server.uri(), "geheim".into())),
        };
        assert_eq!(
            relay.ziel_loeschen(4242, "twitch", 1).await.unwrap_err(),
            "relay HTTP 503"
        );
    }

    // ── Attrappen fuer Nachlauf und Trennen ────────────────────────────────

    #[derive(Default)]
    struct FakeRelay {
        gesetzt: std::sync::Mutex<Vec<(i64, String, String, String)>>,
        geloescht: std::sync::Mutex<Vec<(i64, String)>>,
        kaputt: bool,
    }

    #[async_trait::async_trait]
    impl RelayZiele for FakeRelay {
        async fn ziel_setzen(
            &self,
            streamer_id: i64,
            platform: &str,
            rtmp_url: &str,
            stream_key: &str,
            _generation: i64,
        ) -> Result<(), String> {
            if self.kaputt {
                return Err("relay HTTP 503".into());
            }
            self.gesetzt.lock().unwrap().push((
                streamer_id,
                platform.into(),
                rtmp_url.into(),
                stream_key.into(),
            ));
            Ok(())
        }
        async fn ziel_loeschen(
            &self,
            streamer_id: i64,
            platform: &str,
            _generation: i64,
        ) -> Result<bool, String> {
            if self.kaputt {
                return Err("relay HTTP 503".into());
            }
            self.geloescht
                .lock()
                .unwrap()
                .push((streamer_id, platform.into()));
            Ok(true)
        }
    }

    struct FakeKonto {
        key: std::sync::Mutex<Result<String, String>>,
        widerrufen: std::sync::Mutex<Vec<String>>,
    }

    impl FakeKonto {
        fn neu() -> Self {
            Self {
                key: std::sync::Mutex::new(Ok("sk-geheim".into())),
                widerrufen: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn ohne_key(self) -> Self {
            *self.key.lock().unwrap() = Err("stream key HTTP 401".into());
            self
        }
    }

    #[async_trait::async_trait]
    impl PlattformKonto for FakeKonto {
        async fn stream_key(&self, _t: &str, _b: &str) -> Result<String, String> {
            self.key.lock().unwrap().clone()
        }
        async fn widerrufen(&self, access_token: &str) -> Result<(), String> {
            self.widerrufen
                .lock()
                .unwrap()
                .push(access_token.to_string());
            Ok(())
        }
    }

    // ── mit DB ─────────────────────────────────────────────────────────────

    const TEST_KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn config() -> super::PlatformTokenConfig {
        super::PlatformTokenConfig {
            cipher: std::sync::Arc::new(
                tb_crypto::FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").unwrap(),
            ),
            token_client: std::sync::Arc::new(StummerRefresh),
            kick: None,
            youtube: None,
        }
    }

    /// Kein Refresh in diesen Tests: die Zeilen sind absichtlich frisch.
    struct StummerRefresh;

    #[async_trait::async_trait]
    impl tb_raid::token_refresher::TwitchTokenClient for StummerRefresh {
        async fn refresh(
            &self,
            _t: &str,
        ) -> Result<tb_raid::token_refresher::TokenResponse, tb_raid::token_refresher::RefreshError>
        {
            unreachable!("kein Refresh in diesem Test")
        }
        async fn exchange_code(
            &self,
            _c: &str,
        ) -> Result<tb_raid::token_refresher::TokenResponse, tb_raid::token_refresher::RefreshError>
        {
            unreachable!()
        }
        async fn token_owner(
            &self,
            _t: &str,
        ) -> Result<tb_raid::token_refresher::TokenOwnerInfo, tb_raid::token_refresher::RefreshError>
        {
            unreachable!()
        }
    }

    async fn test_pool() -> (PgPool, crate::test_postgres::TestPostgres) {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260908220000_uplink_target_generations.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260908210000_twitch_uplink_intent.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT NOT NULL PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                access_token TEXT DEFAULT 'ENC',
                refresh_token TEXT DEFAULT 'ENC',
                token_expires_at TIMESTAMPTZ NOT NULL,
                scopes TEXT NOT NULL,
                authorized_at TIMESTAMPTZ DEFAULT NOW(),
                last_refreshed_at TIMESTAMPTZ,
                raid_enabled BOOLEAN DEFAULT TRUE,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                needs_reauth BOOLEAN DEFAULT FALSE,
                reauth_notified_at TIMESTAMPTZ,
                access_token_enc BYTEA,
                refresh_token_enc BYTEA,
                enc_version INTEGER DEFAULT 1,
                enc_kid TEXT DEFAULT 'v1',
                enc_migrated_at TIMESTAMPTZ
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, database)
    }

    async fn zeile_anlegen(
        pool: &PgPool,
        cipher: &tb_crypto::FieldCipher,
        uid: &str,
        satz: &[&str],
    ) {
        let access = cipher
            .encrypt_field(
                "acc-alt",
                &tb_crypto::aad::raid_auth("access_token", uid, 1),
            )
            .unwrap();
        let refresh = cipher
            .encrypt_field(
                "ref-alt",
                &tb_crypto::aad::raid_auth("refresh_token", uid, 1),
            )
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth \
             (twitch_user_id, twitch_login, access_token, refresh_token, \
              access_token_enc, refresh_token_enc, enc_version, enc_kid, \
              token_expires_at, scopes, authorized_at, raid_enabled, needs_reauth) \
             VALUES ($1, 'streamerin', 'ENC', 'ENC', $2, $3, 1, 'v1', \
                     NOW() + INTERVAL '3 hours', $4, NOW(), TRUE, FALSE)",
        )
        .bind(uid)
        .bind(&access)
        .bind(&refresh)
        .bind(satz.join(" "))
        .execute(pool)
        .await
        .unwrap();
    }

    struct NichtErreichbarerRefresh;
    #[async_trait::async_trait]
    impl tb_raid::token_refresher::TwitchTokenClient for NichtErreichbarerRefresh {
        async fn refresh(
            &self,
            _: &str,
        ) -> Result<tb_raid::token_refresher::TokenResponse, tb_raid::token_refresher::RefreshError>
        {
            Err(tb_raid::token_refresher::RefreshError::Other(
                "synthetischer Netzausfall".into(),
            ))
        }
        async fn exchange_code(
            &self,
            _: &str,
        ) -> Result<tb_raid::token_refresher::TokenResponse, tb_raid::token_refresher::RefreshError>
        {
            unreachable!()
        }
        async fn token_owner(
            &self,
            _: &str,
        ) -> Result<tb_raid::token_refresher::TokenOwnerInfo, tb_raid::token_refresher::RefreshError>
        {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn abgelaufener_zugang_mit_refreshfehler_erscheint_nicht_verbunden() {
        let (pool, _database) = test_pool().await;
        let mut config = config();
        config.token_client = std::sync::Arc::new(NichtErreichbarerRefresh);
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6001",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        sqlx::query("UPDATE twitch_raid_auth SET token_expires_at=NOW()-INTERVAL '1 hour'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            twitch_verbindung(&pool, &config, 6001).await,
            vec![("twitch".into(), "zugang_unbekannt")]
        );
    }

    #[tokio::test]
    async fn callback_holt_stream_key_und_setzt_uplink_ziel() {
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6001",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let relay = FakeRelay::default();
        let konto = FakeKonto::neu();
        assert_eq!(
            stream_key_hinterlegen(&pool, &config, &konto, &relay, 6001, "twitch").await,
            StreamKeyStand::Hinterlegt
        );
        assert_eq!(
            relay.gesetzt.lock().unwrap().as_slice(),
            &[(
                6001,
                "twitch".to_string(),
                TWITCH_RTMP_URL.to_string(),
                "sk-geheim".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn callback_ohne_stream_key_recht_verlangt_neu_verbinden() {
        let (pool, _database) = test_pool().await;
        let config = config();
        // Alter Raid-Grant: Tokens gueltig, aber kein channel:read:stream_key.
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6002",
            tb_raid::scope_profiles::FULL_STREAMER_SCOPES,
        )
        .await;
        let relay = FakeRelay::default();
        assert_eq!(
            stream_key_hinterlegen(&pool, &config, &FakeKonto::neu(), &relay, 6002, "twitch").await,
            StreamKeyStand::RechtFehlt
        );
        assert!(relay.gesetzt.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn callback_bei_kaputtem_relay_meldet_fehler_und_setzt_kein_ziel() {
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6003",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let relay = FakeRelay {
            kaputt: true,
            ..FakeRelay::default()
        };
        assert_eq!(
            stream_key_hinterlegen(&pool, &config, &FakeKonto::neu(), &relay, 6003, "twitch").await,
            StreamKeyStand::Fehlgeschlagen
        );
        // Und die Verbindung selbst bleibt unangetastet.
        let store = tb_raid::token_store::RaidAuthStore::new(pool.clone(), config.cipher.clone());
        let t = store.load_decrypted_unrestricted("6003").await.unwrap();
        assert!(!t.unwrap().needs_reauth);
    }

    #[tokio::test]
    async fn callback_ohne_twitch_antwort_behaelt_die_verbindung() {
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6004",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let relay = FakeRelay::default();
        assert_eq!(
            stream_key_hinterlegen(
                &pool,
                &config,
                &FakeKonto::neu().ohne_key(),
                &relay,
                6004,
                "twitch"
            )
            .await,
            StreamKeyStand::Fehlgeschlagen
        );
        assert!(relay.gesetzt.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn clear_tokens_setzt_needs_reauth_und_leert_blobs() {
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6005",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let writer = tb_raid::auth_writer::AuthWriter::new(pool.clone(), config.cipher.clone());
        writer
            .clear_tokens("6005", chrono::Utc::now())
            .await
            .unwrap();

        /// Die vier Spalten, an denen sich das Leeren ablesen laesst.
        #[derive(sqlx::FromRow)]
        struct GeleerteZeile {
            access_token_enc: Option<Vec<u8>>,
            refresh_token_enc: Option<Vec<u8>>,
            needs_reauth: Option<bool>,
            reauth_notified_at: Option<chrono::DateTime<chrono::Utc>>,
        }

        let zeile: GeleerteZeile = sqlx::query_as(
            "SELECT access_token_enc, refresh_token_enc, needs_reauth, reauth_notified_at \
                 FROM twitch_raid_auth WHERE twitch_user_id = '6005'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            zeile.access_token_enc.is_none(),
            "access_token_enc muss leer sein"
        );
        assert!(
            zeile.refresh_token_enc.is_none(),
            "refresh_token_enc muss leer sein"
        );
        assert_eq!(zeile.needs_reauth, Some(true));
        // Ohne den Stempel mahnt der Watchdog jemanden, der gerade selbst
        // getrennt hat.
        assert!(
            zeile.reauth_notified_at.is_some(),
            "reauth_notified_at muss gesetzt sein"
        );

        // Die Zeile bleibt: raid_enabled und die Partnerhistorie haengen daran.
        let anzahl: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM twitch_raid_auth WHERE twitch_user_id = '6005'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(anzahl, 1);

        // Und der Lesepfad meldet den Streamer jetzt als getrennt.
        let store = tb_raid::token_store::RaidAuthStore::new(pool.clone(), config.cipher.clone());
        assert!(store
            .load_decrypted_unrestricted("6005")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn trennen_entfernt_nur_uplink_und_behaelt_raid_grant() {
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6006",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let relay = FakeRelay::default();
        let konto = FakeKonto::neu();
        assert_eq!(
            trennen(&pool, &config, &konto, &relay, 6006, "twitch").await,
            TrennenErgebnis::Getrennt
        );
        assert_eq!(
            relay.geloescht.lock().unwrap().as_slice(),
            &[(6006, "twitch".to_string())]
        );
        assert!(konto.widerrufen.lock().unwrap().is_empty());
        let store = tb_raid::token_store::RaidAuthStore::new(pool.clone(), config.cipher.clone());
        let raid = store
            .load_decrypted("6006")
            .await
            .unwrap()
            .expect("Raid bleibt verbunden");
        assert_eq!(raid.access_token, "acc-alt");
        assert!(!raid.needs_reauth);
        let (uplink, scopes) = store
            .load_decrypted_with_scopes("6006")
            .await
            .unwrap()
            .unwrap();
        assert!(uplink.uplink_disconnected);
        assert!(!scopes.is_empty());
        assert_eq!(
            twitch_verbindung(&pool, &config, 6006).await,
            vec![("twitch".into(), "getrennt")]
        );
        assert!(matches!(
            super::super::platform_token::gueltiger_twitch_token(
                &pool,
                &config,
                6006,
                chrono::Utc::now()
            )
            .await,
            Err(super::super::platform_token::TokenFehler::KeineVerbindung)
        ));

        // Noch einmal: wiederholbar, nichts bricht.
        assert_eq!(
            trennen(&pool, &config, &konto, &relay, 6006, "twitch").await,
            TrennenErgebnis::Getrennt
        );
    }

    /// Bleibt das Ziel im Relay stehen, sendet der Uplink weiter an einen
    /// Kanal, den der Streamer gerade abgeklemmt hat. Also bricht das Trennen
    /// hier ab, statt die Tokens schon wegzuwerfen.
    #[tokio::test]
    async fn trennen_bei_kaputtem_relay_laesst_alles_stehen() {
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6007",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let relay = FakeRelay {
            kaputt: true,
            ..FakeRelay::default()
        };
        let konto = FakeKonto::neu();
        assert_eq!(
            trennen(&pool, &config, &konto, &relay, 6007, "twitch").await,
            TrennenErgebnis::RelayFehler
        );
        assert!(konto.widerrufen.lock().unwrap().is_empty());
        let store = tb_raid::token_store::RaidAuthStore::new(pool.clone(), config.cipher.clone());
        assert!(store
            .load_decrypted_unrestricted("6007")
            .await
            .unwrap()
            .is_some());
    }

    /// Kick hat noch keinen Verbinden-Weg. Ein `ok` waere die Behauptung,
    /// etwas getrennt zu haben, das nie verbunden war.
    #[tokio::test]
    async fn trennen_ohne_verbinden_weg_meldet_keinen_erfolg() {
        let (pool, _database) = test_pool().await;
        let config = config();
        assert_eq!(
            trennen(
                &pool,
                &config,
                &FakeKonto::neu(),
                &FakeRelay::default(),
                6008,
                "kick"
            )
            .await,
            TrennenErgebnis::KeinWeg
        );
    }
    #[tokio::test]
    async fn review_started_stream_key_followup_cannot_recreate_disconnected_destination() {
        struct PausedKonto {
            started: tokio::sync::Notify,
            resume: tokio::sync::Notify,
        }
        #[async_trait::async_trait]
        impl PlattformKonto for PausedKonto {
            async fn stream_key(&self, _: &str, _: &str) -> Result<String, String> {
                self.started.notify_one();
                self.resume.notified().await;
                Ok("synthetic-late-key".into())
            }
            async fn widerrufen(&self, _: &str) -> Result<(), String> {
                panic!("Uplink-only disconnect must not revoke a shared token")
            }
        }
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6010",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let relay = FakeRelay::default();
        let konto = PausedKonto {
            started: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        };
        let late = stream_key_hinterlegen(&pool, &config, &konto, &relay, 6010, "twitch");
        let disconnect = async {
            konto.started.notified().await;
            assert_eq!(
                trennen(&pool, &config, &konto, &relay, 6010, "twitch").await,
                TrennenErgebnis::Getrennt
            );
            let store =
                tb_raid::token_store::RaidAuthStore::new(pool.clone(), config.cipher.clone());
            assert!(
                store
                    .load_decrypted_with_scopes("6010")
                    .await
                    .unwrap()
                    .unwrap()
                    .0
                    .uplink_disconnected
            );
            konto.resume.notify_one();
        };
        let (outcome, ()) = tokio::join!(late, disconnect);
        assert_eq!(relay.geloescht.lock().unwrap().len(), 1);
        assert!(relay.gesetzt.lock().unwrap().is_empty(), "A followup resumed after confirmed DELETE and durable disconnect, then issued a new destination PUT; returned {outcome:?}");
    }
    #[tokio::test]
    async fn lost_delete_reply_remains_pending_and_normal_retry_reuses_generation() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (pool, _database) = test_pool().await;
        let config = config();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "6011",
            tb_raid::scope_profiles::UPLINK_SCOPES,
        )
        .await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = requests.clone();
        let server = tokio::spawn(async move {
            for round in 0..2 {
                let (mut stream, _) =
                    tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    assert!(header.len() < 8192);
                    header.push(
                        tokio::time::timeout(std::time::Duration::from_secs(2), stream.read_u8())
                            .await
                            .unwrap()
                            .unwrap(),
                    );
                }
                let head = String::from_utf8(header).unwrap();
                let path = head
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
                let generation = url
                    .query_pairs()
                    .find(|(key, _)| key == "connection_generation")
                    .unwrap()
                    .1
                    .parse::<i64>()
                    .unwrap();
                captured.lock().unwrap().push(generation);
                // Remote DELETE has been accepted. Only its response is lost.
                let response = if round == 0 {
                    "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{"
                        .to_owned()
                } else {
                    let body =
                        format!(r#"{{"deleted":true,"connection_generation":{generation}}}"#);
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        let relay = HttpRelayZiele {
            verbindung: Some((format!("http://{address}"), "synthetic".into())),
        };
        let konto = FakeKonto::neu();
        assert_eq!(
            trennen(&pool, &config, &konto, &relay, 6011, "twitch").await,
            TrennenErgebnis::RelayFehler
        );
        let pending = tb_raid::target_generation::snapshot(&pool, "6011", "twitch")
            .await
            .unwrap();
        assert!(pending.disconnect_pending && !pending.enabled);
        // The ordinary status path must expose the incomplete operation.
        assert!(verbindungen_lesen(&pool, Some(&config), 6011)
            .await
            .contains(&("twitch".into(), "trennung_offen")));
        assert_eq!(
            trennen(&pool, &config, &konto, &relay, 6011, "twitch").await,
            TrennenErgebnis::Getrennt
        );
        server.await.unwrap();
        assert_eq!(
            *requests.lock().unwrap(),
            vec![pending.generation, pending.generation]
        );
        let done = tb_raid::target_generation::snapshot(&pool, "6011", "twitch")
            .await
            .unwrap();
        assert!(!done.enabled && !done.disconnect_pending);
        let token = tb_raid::RaidAuthStore::new(pool.clone(), config.cipher.clone())
            .load_decrypted_unrestricted("6011")
            .await
            .unwrap()
            .unwrap();
        assert!(
            !token.access_token.is_empty(),
            "Shared Raid grant remains intact"
        );
    }
}

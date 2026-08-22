//! Proxy vom Streamer-Dashboard zu rs-relay. Das Relay-Secret bleibt serverseitig.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

fn relay_base() -> String {
    std::env::var("RS_RELAY_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8891".into())
}

fn relay_secret() -> Option<String> {
    std::env::var("RS_RELAY_API_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Twitch-Identität der Session: Login und, falls die Session sie mitbringt,
/// die numerische User-ID.
///
/// Die Master-Session des Admin-Dashboards ist Discord-basiert und trägt gar
/// keine Twitch-User-ID (`master_session_auth` setzt sie leer). Ohne Fallback
/// scheiterte Uplink für genau diese Session an einem leeren Parse.
fn twitch_identitaet(auth: &DashboardAuthLevel) -> Result<(&str, &str), Response> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => Ok((twitch_login.as_str(), twitch_user_id.as_str())),
        DashboardAuthLevel::Admin {
            actor: Some(actor),
        } => Ok((actor.twitch_login.as_str(), actor.twitch_user_id.as_str())),
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

/// Streamer-ID für das Relay. Bringt die Session keine numerische User-ID mit,
/// wird sie über den Login aus der Datenbank aufgelöst (`tb_twitch_user_id`,
/// dieselbe Quelle wie im übrigen Dashboard).
async fn partner_id(pool: &PgPool, auth: &DashboardAuthLevel) -> Result<i64, Response> {
    let (login, roh) = twitch_identitaet(auth)?;
    if let Ok(id) = roh.trim().parse::<i64>() {
        return Ok(id);
    }

    let login = login.trim().to_lowercase();
    let aufgeloest: Option<String> = sqlx::query_scalar("SELECT tb_twitch_user_id($1)")
        .bind(&login)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            tracing::warn!("uplink: Lookup der Twitch-User-ID für {login} fehlgeschlagen: {e}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "twitch-identitaet nicht abrufbar" })),
            )
                .into_response()
        })?;

    aufgeloest
        .as_deref()
        .and_then(|wert| wert.trim().parse::<i64>().ok())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "twitch user id fehlt" })),
            )
                .into_response()
        })
}

async fn relay_json(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, Response> {
    let secret = relay_secret().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Uplink ist noch nicht verbunden." })),
        )
            .into_response()
    })?;
    let url = format!("{}{path}", relay_base().trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req = client
        .request(method, url)
        .header("X-Relay-Auth", secret)
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
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(wert),
        )
            .into_response());
    }
    Ok(wert)
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

async fn live_status(pool: &PgPool, streamer_id: i64) -> &'static str {
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

pub async fn me_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&pool, &auth).await?;
    let mut wert = relay_json(
        reqwest::Method::GET,
        &format!("/v1/me?streamer_id={id}"),
        None,
    )
    .await?;
    // Der Live-Status ist Wissen des Bots, nicht des Relays: er kommt aus der
    // Twitch-Beobachtung. Deshalb wird er hier angehaengt und nicht im Relay
    // nachgebaut.
    if let Some(objekt) = wert.as_object_mut() {
        objekt.insert(
            "live_status".to_string(),
            Value::String(live_status(&pool, id).await.to_string()),
        );
    }
    Ok(Json(wert))
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

#[derive(Deserialize)]
pub struct DestinationBody {
    pub platform: String,
    pub rtmp_url: String,
    pub stream_key: String,
}

pub async fn put_destination_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    Json(body): Json<DestinationBody>,
) -> Result<Json<Value>, Response> {
    let id = partner_id(&pool, &auth).await?;
    if body.stream_key.trim().is_empty() || body.rtmp_url.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "rtmp_url und stream_key braucht es" })),
        )
            .into_response());
    }
    let wert = relay_json(
        reqwest::Method::PUT,
        "/v1/admin/destinations",
        Some(json!({
            "streamer_id": id,
            "platform": body.platform,
            "rtmp_url": body.rtmp_url,
            "stream_key": body.stream_key,
        })),
    )
    .await?;
    Ok(Json(wert))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;

    #[test]
    fn ohne_session_gibt_es_keine_identitaet() {
        assert!(twitch_identitaet(&DashboardAuthLevel::None).is_err());
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
        assert_eq!(live_bewerten(Some((1, Some("gestern"))), jetzt), "unbekannt");
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
        assert_eq!(live_bewerten(Some((0, Some("2026-08-22T11:55:00+00:00"))), jetzt), "aus");
        assert_eq!(
            live_bewerten(Some((0, Some("2026-08-22T11:54:59+00:00"))), jetzt),
            "unbekannt"
        );
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
}

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
        // Der Login geht mit, damit die Oberflaeche die OBS-Dock-Adressen
        // fertig hinschreiben kann. Wer auf "Benutzerdefiniert" umstellt,
        // verliert in OBS Chat und Aktivitaet, und ein Platzhalter zum
        // Selbstersetzen ist genau die Stelle, an der Leute steckenbleiben.
        if let Ok((login, _)) = twitch_identitaet(&auth) {
            objekt.insert(
                "twitch_login".to_string(),
                Value::String(login.trim().to_lowercase()),
            );
        }
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

/// Freie Zahlen aus dem manuellen Modus.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManuellesProfil {
    pub width: i32,
    pub height: i32,
    pub fps: i32,
    pub bitrate_kbps: i32,
}

/// Prueft ein manuelles Profil auf das, was technisch nicht gehen kann.
///
/// Nach oben wird nichts mehr geprueft. Frueher stand hier ein Ingest-Deckel,
/// und der hat eine Eingabe abgelehnt, die das Relay angenommen haette: der
/// Streamer trug 16000 ein, bekam "liegt über unserem Maximum" und keine
/// Erklaerung, wessen Maximum das ist. Was eine Plattform wirklich annimmt,
/// entscheidet die Plattform an ihrem Ingest. Was der Server traegt,
/// entscheidet das Punktebudget in rs-relay, und zwar mit einer Ablehnung
/// samt Grund statt hier mit einem Formularfehler.
///
/// Rueckgabe ist der Grund, warum es nicht geht, damit die Oberflaeche ihn
/// hinschreiben kann. Ein blosses "ungueltig" laesst den Streamer raten,
/// welches der vier Felder gemeint ist.
fn manuell_pruefen(p: ManuellesProfil) -> Result<(), String> {
    for (name, wert) in [
        ("Breite", p.width),
        ("Höhe", p.height),
        ("Bildrate", p.fps),
        ("Bitrate", p.bitrate_kbps),
    ] {
        if wert <= 0 {
            return Err(format!("{name} muss größer als 0 sein."));
        }
    }
    // Ungerade Kantenlaengen bringen den Encoder ins Straucheln: yuv420p
    // halbiert beide Achsen, und eine ungerade Zahl laesst sich nicht
    // halbieren. ffmpeg bricht dann beim Start ab, nicht beim Speichern.
    if p.width % 2 != 0 || p.height % 2 != 0 {
        return Err("Breite und Höhe müssen gerade Zahlen sein.".into());
    }
    Ok(())
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
    let key = body.stream_key.as_deref().map(str::trim).unwrap_or_default();
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
        (Some(name), _) => Some(profil_aufloesen(name).ok_or_else(|| {
            fehler(StatusCode::BAD_REQUEST, "unbekanntes Profil")
        })?),
        (None, Some(manuell)) => {
            manuell_pruefen(manuell).map_err(|grund| fehler(StatusCode::BAD_REQUEST, &grund))?;
            Some((
                manuell.width,
                manuell.height,
                manuell.fps,
                manuell.bitrate_kbps,
            ))
        }
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
const RELAY_ZIEL_PFAD: &str = "/v1/me/destinations";

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
    let eintrag = ziel_nutzlast(&body)?;
    let wert = relay_json(
        reqwest::Method::PUT,
        RELAY_ZIEL_PFAD,
        Some(nutzlast_fuer(id, eintrag)),
    )
    .await?;
    Ok(Json(wert))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;

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
        b.manuell = Some(ManuellesProfil {
            width: 2560,
            height: 1440,
            fps: 60,
            bitrate_kbps: 18000,
        });
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
        assert!(manuell_pruefen(ManuellesProfil {
            width: 3840,
            height: 2160,
            fps: 60,
            bitrate_kbps: 6000,
        })
        .is_ok());
        assert!(manuell_pruefen(ManuellesProfil {
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 60000,
        })
        .is_ok());
        // Und der Fall aus dem Betrieb: 16000 kbps an Twitch. Genau diese Zahl
        // hat der Streamer eingetragen und "liegt über unserem Maximum"
        // gelesen, ohne zu erfahren, wessen Maximum gemeint war.
        assert!(manuell_pruefen(ManuellesProfil {
            width: 2560,
            height: 1440,
            fps: 60,
            bitrate_kbps: 16000,
        })
        .is_ok());
    }

    /// yuv420p halbiert beide Achsen. Eine ungerade Kante laesst ffmpeg erst
    /// beim Start sterben, nicht beim Speichern, und dann steht der Stream.
    #[test]
    fn ungerade_kanten_werden_abgelehnt() {
        assert!(manuell_pruefen(ManuellesProfil {
            width: 1921,
            height: 1080,
            fps: 60,
            bitrate_kbps: 6000,
        })
        .is_err());
    }

    #[test]
    fn null_und_negativ_sind_keine_werte() {
        for (w, h, f, b) in [(0, 1080, 60, 6000), (1920, 0, 60, 6000), (1920, 1080, 0, 6000), (1920, 1080, 60, -1)] {
            assert!(manuell_pruefen(ManuellesProfil {
                width: w,
                height: h,
                fps: f,
                bitrate_kbps: b,
            })
            .is_err());
        }
    }

    #[test]
    fn stufe_und_eigene_werte_zusammen_sind_ein_fehler() {
        let mut b = body("twitch");
        b.profil = Some("1080p60".into());
        b.manuell = Some(ManuellesProfil {
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 6000,
        });
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

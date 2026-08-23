//! `GET /obs/ws` — der Socket, an dem die eigenen OBS-Docks haengen.
//!
//! Ablauf eines Verbindungsaufbaus:
//!
//! 1. **Auth vor dem Upgrade.** Der bestehende [`DashboardAuthLevel`]-Extractor
//!    entscheidet. Ohne Session gibt es 401 und ausdruecklich **keinen**
//!    Redirect: ein Dock-Socket soll nicht auf eine Login-HTML-Seite umgebogen
//!    werden, das Dock-HTML hat den Redirect schon vorher erledigt.
//! 2. **Kanalbindung.** [`resolve_streamer_scope`] bindet einen Partner an
//!    seinen eigenen Kanal; ein fremder `?streamer=` gibt 403. Admin darf
//!    jeden Kanal, muss ihn aber nennen.
//! 3. **Kanalkennung.** Der Bus sortiert nach `channel_id`, und das ist bei
//!    Twitch die numerische User-ID, nicht der Login. Fuer einen Partner steht
//!    sie schon in der Session, sonst kommt sie aus `twitch_partners`.
//! 4. **Anmelden vor dem Nachlauf.** Der Socket abonniert den Kanal, bevor er
//!    den Nachlauf aus `obs_dock_events` liest. Live-Ereignisse sammeln sich
//!    solange im Puffer, und [`Auslieferung`] wirft danach genau die weg, die
//!    der Nachlauf schon abgedeckt hat. Ohne diese Reihenfolge entstuende
//!    genau im Uebergang eine Luecke.
//!
//! Betrieb: Ping alle 30 s, Leerlauf-Deckel, Nachpruefung der Session im
//! Minutentakt. Ein Dock laeuft acht Stunden, deshalb schliesst der Server nie
//! stumm, sondern immer mit Code und Grund aus [`SchliessGrund`].
//!
//! Der Client darf ausschliesslich `{"typ":"ping"}` senden und bekommt darauf
//! `{"typ":"pong"}`. Jeder andere Rahmen wird verworfen; die Gegenrichtung
//! traegt nur `tb_platform_core::PlatformEvent`.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{debug, warn};

use crate::auth::level::DashboardAuthLevel;
use crate::auth::resolve_streamer_scope;
use crate::auth::session::{DashboardAuthState, PARTNER_ACCESS_COOKIE_NAME, PARTNER_COOKIE_NAME};
use crate::obs::bus::{
    Auslieferung, ObsDockBus, SchliessGrund, NACHLAUF_DECKEL, VORLAUF_OHNE_SEIT,
};

/// Abstand der Server-Pings.
const HEARTBEAT: Duration = Duration::from_secs(30);

/// Nach so langer Funkstille gilt der Socket als tot. Vier verpasste Pings.
const LEERLAUF_DECKEL: Duration = Duration::from_secs(120);

/// Abstand der Session-Nachpruefung.
const SESSION_PRUEFUNG: Duration = Duration::from_secs(60);

/// Antwort auf `{"typ":"ping"}`.
const PONG: &str = r#"{"typ":"pong"}"#;

/// Query-Parameter von `GET /obs/ws`.
///
/// Beide Werte kommen als Text herein und werden hier von Hand gelesen, damit
/// ein leeres `?seit=` nicht in einer 400 endet. Ein Dock, das nach einem
/// Neustart noch keine `id` kennt, schickt genau das.
#[derive(Debug, Default, Deserialize)]
pub struct ObsWsAbfrage {
    /// Twitch-Login des Kanals. Partner duerfen nur den eigenen nennen.
    #[serde(default)]
    pub streamer: Option<String>,
    /// Letzte `obs_dock_events.id`, die das Dock gesehen hat.
    #[serde(default)]
    pub seit: Option<String>,
}

impl ObsWsAbfrage {
    /// `?seit=` als Zahl. Leer oder unlesbar heisst "kein Nachlauf gewuenscht".
    fn seit(&self) -> Option<i64> {
        self.seit
            .as_deref()
            .map(str::trim)
            .filter(|wert| !wert.is_empty())
            .and_then(|wert| wert.parse::<i64>().ok())
            .filter(|wert| *wert > 0)
    }
}

fn fehler(status: StatusCode, code: &'static str, text: &'static str) -> Response {
    (status, Json(json!({ "error": code, "message": text }))).into_response()
}

/// 401 ohne Redirect (Plan Abschnitt 2.3).
fn nicht_angemeldet() -> Response {
    fehler(
        StatusCode::UNAUTHORIZED,
        "nicht_angemeldet",
        "Fuer den Dock-Socket braucht es eine angemeldete Sitzung.",
    )
}

/// Entscheidet, welchen Kanal diese Sitzung mitlesen darf.
///
/// Eigene Funktion, damit die Entscheidung ohne Socket und ohne Datenbank
/// pruefbar ist. Rueckgabe ist der Twitch-Login in Kleinschreibung.
#[allow(clippy::result_large_err)]
pub(crate) fn kanal_freigabe(
    auth: &DashboardAuthLevel,
    angefragt: Option<&str>,
) -> Result<String, Response> {
    if matches!(auth, DashboardAuthLevel::None) {
        return Err(nicht_angemeldet());
    }
    // `required = true`: Admin darf jeden Kanal, muss ihn aber nennen; ein
    // Partner mit fremdem `?streamer=` faellt hier auf 403.
    match resolve_streamer_scope(auth, angefragt, true)? {
        Some(login) => Ok(login),
        None => Err(fehler(
            StatusCode::BAD_REQUEST,
            "streamer_fehlt",
            "Der Parameter streamer fehlt.",
        )),
    }
}

/// Loest den Twitch-Login auf die Kanalkennung auf, nach der der Bus sortiert.
///
/// Fuer einen Partner steht sie schon in der Session; nur der Sonderfall der
/// Discord-Admin-Session ohne Twitch-Identitaet und der Admin-Zugriff auf einen
/// fremden Kanal brauchen die Tabelle.
async fn kanal_kennung(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    login: &str,
) -> Result<String, Response> {
    if let DashboardAuthLevel::Partner {
        twitch_login,
        twitch_user_id,
        ..
    } = auth
    {
        if twitch_login.to_lowercase() == login && !twitch_user_id.trim().is_empty() {
            return Ok(twitch_user_id.clone());
        }
    }

    let gefunden: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id
           FROM twitch_partners
          WHERE lower(twitch_login) = $1
            AND COALESCE(twitch_user_id, '') <> ''
          ORDER BY id
          LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
    .unwrap_or_else(|db_fehler| {
        warn!(%db_fehler, "OBS-Dock: Kanalkennung nicht ermittelbar");
        None
    })
    .flatten();

    gefunden.ok_or_else(|| {
        fehler(
            StatusCode::NOT_FOUND,
            "kanal_unbekannt",
            "Zu diesem Kanal ist keine Twitch-Kennung hinterlegt.",
        )
    })
}

/// Prueft im laufenden Betrieb, ob die Sitzung noch traegt.
///
/// Ein Dock haengt acht Stunden am Stueck. Laeuft die Session in der Zeit aus,
/// darf der Socket nicht stumm sterben, sondern muss dem Dock den Grund
/// nennen, damit es "Bitte neu anmelden" anzeigen kann.
enum SessionWaechter {
    /// Admin oder interner Token: laeuft im Sinne dieses Sockets nicht ab.
    Ohne,
    /// Partner-Cookie, wird nachgeprueft.
    Partner {
        state: DashboardAuthState,
        sitzung: Option<String>,
        dauersitzung: Option<String>,
        user_agent: String,
    },
}

impl SessionWaechter {
    fn bauen(
        auth: &DashboardAuthLevel,
        state: Option<DashboardAuthState>,
        headers: &HeaderMap,
    ) -> Self {
        let DashboardAuthLevel::Partner { .. } = auth else {
            return Self::Ohne;
        };
        let Some(state) = state else {
            return Self::Ohne;
        };
        let cookie = |name: &str| {
            crate::auth::level::cookie_values(headers, name)
                .into_iter()
                .find(|wert| !wert.is_empty())
                .map(str::to_string)
        };
        let sitzung = cookie(PARTNER_COOKIE_NAME);
        let dauersitzung = cookie(PARTNER_ACCESS_COOKIE_NAME);
        if sitzung.is_none() && dauersitzung.is_none() {
            // Partner-Level ohne Partner-Cookie gibt es nur ueber den
            // Loopback-Pfad; dort ist nichts nachzupruefen.
            return Self::Ohne;
        }
        Self::Partner {
            state,
            sitzung,
            dauersitzung,
            user_agent: headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|wert| wert.to_str().ok())
                .unwrap_or("")
                .to_string(),
        }
    }

    /// `true`, solange die Sitzung traegt. Ein Datenbankfehler zaehlt bewusst
    /// als "traegt": ein kurzer Aussetzer soll kein Dock aus dem Stream werfen.
    async fn gueltig(&self) -> bool {
        let Self::Partner {
            state,
            sitzung,
            dauersitzung,
            user_agent,
        } = self
        else {
            return true;
        };

        if let Some(id) = sitzung {
            match state.load_partner_session(id).await {
                Ok(Some(_)) => return true,
                Err(db_fehler) => {
                    warn!(%db_fehler, "OBS-Dock: Session-Nachpruefung fehlgeschlagen");
                    return true;
                }
                Ok(None) => {}
            }
        }
        if let Some(id) = dauersitzung {
            match state.load_partner_access_session(id, user_agent).await {
                Ok(Some(_)) => return true,
                Err(db_fehler) => {
                    warn!(%db_fehler, "OBS-Dock: Session-Nachpruefung fehlgeschlagen");
                    return true;
                }
                Ok(None) => {}
            }
        }
        false
    }
}

/// `GET /obs/ws`.
///
/// `WebSocketUpgrade` steht bewusst als `Option`: ein gewoehnlicher GET ohne
/// Upgrade-Header soll die Auth-Antwort sehen (401/403) und nicht die
/// Upgrade-Abweisung des Extractors. Erst wenn die Auth traegt, wird der
/// fehlende Upgrade-Header zum Fehler.
pub async fn obs_ws_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Extension(bus): Extension<Arc<ObsDockBus>>,
    auth_state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    Query(abfrage): Query<ObsWsAbfrage>,
    upgrade: Option<WebSocketUpgrade>,
) -> Response {
    let login = match kanal_freigabe(&auth, abfrage.streamer.as_deref()) {
        Ok(login) => login,
        Err(antwort) => return antwort,
    };
    let channel_id = match kanal_kennung(&pool, &auth, &login).await {
        Ok(kennung) => kennung,
        Err(antwort) => return antwort,
    };
    let Some(upgrade) = upgrade else {
        return fehler(
            StatusCode::UPGRADE_REQUIRED,
            "upgrade_noetig",
            "Dieser Endpunkt spricht nur WebSocket.",
        );
    };

    let waechter =
        SessionWaechter::bauen(&auth, auth_state.map(|Extension(state)| state), &headers);
    let seit = abfrage.seit();

    // Der Listener startet beim ersten Dock des Prozesses, nicht beim
    // Router-Bau: ohne offenes Dock soll keine Postgres-Verbindung liegen.
    bus.listener_sicherstellen();
    // Abonnieren vor dem Nachlauf, sonst reisst genau im Uebergang eine Luecke.
    let anmeldung = bus.anmelden(&channel_id);

    upgrade.on_upgrade(move |socket| async move {
        let anmeldung = anmeldung;
        socket_bedienen(socket, pool, channel_id, seit, anmeldung, waechter).await;
    })
}

/// Liest den Nachlauf aus `obs_dock_events`.
///
/// Mit `seit` alles danach (Deckel [`NACHLAUF_DECKEL`]), ohne `seit` die
/// letzten [`VORLAUF_OHNE_SEIT`] Zeilen des Kanals, beide aufsteigend sortiert.
async fn nachlauf_lesen(
    pool: &PgPool,
    channel_id: &str,
    seit: Option<i64>,
    deckel: i64,
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    match seit {
        Some(seit) => {
            sqlx::query_as(
                "SELECT id, payload::text
                   FROM obs_dock_events
                  WHERE channel_id = $1
                    AND id > $2
                  ORDER BY id
                  LIMIT $3",
            )
            .bind(channel_id)
            .bind(seit)
            .bind(deckel)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as(
                "SELECT id, payload
                   FROM (
                          SELECT id, payload::text AS payload
                            FROM obs_dock_events
                           WHERE channel_id = $1
                           ORDER BY id DESC
                           LIMIT $2
                        ) letzte
                  ORDER BY id",
            )
            .bind(channel_id)
            .bind(deckel)
            .fetch_all(pool)
            .await
        }
    }
}

/// Der Lebenslauf eines offenen Docks.
async fn socket_bedienen(
    socket: WebSocket,
    pool: PgPool,
    channel_id: String,
    seit: Option<i64>,
    anmeldung: crate::obs::bus::Anmeldung,
    waechter: SessionWaechter,
) {
    let crate::obs::bus::Anmeldung {
        waechter: _anmeldewaechter,
        mut rahmen,
        mut abbruch,
    } = anmeldung;

    let (mut schreiber, mut leser) = socket.split();
    let mut buch = Auslieferung::neu(seit);

    // 1. Nachlauf. Die Live-Rahmen warten solange im Puffer.
    let deckel = if seit.is_some() {
        NACHLAUF_DECKEL
    } else {
        VORLAUF_OHNE_SEIT
    };
    match nachlauf_lesen(&pool, &channel_id, seit, deckel).await {
        Ok(zeilen) => {
            for (id, json) in zeilen {
                if schreiber.send(Message::Text(json)).await.is_err() {
                    return;
                }
                buch.nachlauf(id);
            }
        }
        Err(db_fehler) => {
            // Kein Abbruch: live weiterzumachen ist besser als gar nichts.
            warn!(%db_fehler, channel_id, "OBS-Dock: Nachlauf nicht lesbar");
        }
    }

    // 2. Live.
    let mut herzschlag = tokio::time::interval(HEARTBEAT);
    herzschlag.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut sitzungstakt = tokio::time::interval(SESSION_PRUEFUNG);
    sitzungstakt.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // Der erste Tick beider Intervalle kommt sofort; einmal abholen.
    herzschlag.tick().await;
    sitzungstakt.tick().await;
    let mut letztes_lebenszeichen = Instant::now();

    let schliessen: Option<SchliessGrund> = loop {
        tokio::select! {
            biased;

            grund = &mut abbruch => {
                match grund {
                    Ok(grund) => break Some(grund),
                    // Der Bus hat den Sender fallen lassen; nichts zu melden.
                    Err(_) => break None,
                }
            }

            eingang = rahmen.recv() => {
                match eingang {
                    Ok(rahmen) => {
                        if !buch.live(rahmen.id) {
                            continue;
                        }
                        if schreiber.send(Message::Text(rahmen.json.to_string())).await.is_err() {
                            break None;
                        }
                    }
                    Err(RecvError::Lagged(verloren)) => {
                        // Der Socket kam nicht hinterher. Die Luecke steht in
                        // der Tabelle, also von dort nachziehen statt sie
                        // stillschweigend zu verlieren.
                        warn!(channel_id, verloren, "OBS-Dock: Socket hinkte nach, Luecke wird nachgezogen");
                        if !luecke_nachreichen(&pool, &channel_id, &mut buch, &mut schreiber).await {
                            break None;
                        }
                    }
                    Err(RecvError::Closed) => break None,
                }
            }

            _ = herzschlag.tick() => {
                if letztes_lebenszeichen.elapsed() > LEERLAUF_DECKEL {
                    break Some(SchliessGrund::Leerlauf);
                }
                if schreiber.send(Message::Ping(Vec::new())).await.is_err() {
                    break None;
                }
            }

            _ = sitzungstakt.tick() => {
                if !waechter.gueltig().await {
                    break Some(SchliessGrund::SessionAbgelaufen);
                }
            }

            eingang = leser.next() => {
                match eingang {
                    Some(Ok(nachricht)) => {
                        letztes_lebenszeichen = Instant::now();
                        if matches!(nachricht, Message::Close(_)) {
                            break None;
                        }
                        if ist_ping(&nachricht)
                            && schreiber.send(Message::Text(PONG.to_string())).await.is_err()
                        {
                            break None;
                        }
                    }
                    Some(Err(_)) | None => break None,
                }
            }
        }
    };

    if let Some(grund) = schliessen {
        debug!(
            channel_id,
            grund = grund.text(),
            "OBS-Dock: Socket geschlossen"
        );
        let _ = schreiber
            .send(Message::Close(Some(CloseFrame {
                code: grund.code(),
                reason: grund.text().into(),
            })))
            .await;
    }
    let _ = schreiber.close().await;
}

/// Zieht nach einem `Lagged` die verpassten Zeilen aus der Tabelle nach.
/// `false` heisst: der Socket ist weg.
async fn luecke_nachreichen(
    pool: &PgPool,
    channel_id: &str,
    buch: &mut Auslieferung,
    schreiber: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> bool {
    let zeilen =
        match nachlauf_lesen(pool, channel_id, Some(buch.letzte_id()), NACHLAUF_DECKEL).await {
            Ok(zeilen) => zeilen,
            Err(db_fehler) => {
                warn!(%db_fehler, channel_id, "OBS-Dock: Luecke nicht nachreichbar");
                return true;
            }
        };
    for (id, json) in zeilen {
        if !buch.live(id) {
            continue;
        }
        if schreiber.send(Message::Text(json)).await.is_err() {
            return false;
        }
    }
    true
}

/// Der einzige Rahmen, den der Client senden darf.
fn ist_ping(nachricht: &Message) -> bool {
    let Message::Text(text) = nachricht else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|wert| {
            wert.get("typ")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .is_some_and(|typ| typ == "ping")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    fn partner(login: &str, user_id: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: user_id.to_string(),
            display_name: login.to_string(),
        }
    }

    /// Ein Pool, der nie verbindet. Reicht fuer alle Pfade, die vor dem ersten
    /// Datenbankzugriff abbiegen.
    fn traeger_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://obs-dock-test@127.0.0.1:1/obs")
            .expect("lazy pool")
    }

    fn test_router() -> Router {
        Router::new()
            .route("/obs/ws", get(obs_ws_handler))
            .layer(Extension(ObsDockBus::ohne_datenbank()))
            .with_state(traeger_pool())
    }

    async fn fehlerkoerper(antwort: Response) -> serde_json::Value {
        let bytes = to_bytes(antwort.into_body(), 1 << 16).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Beweisziel 3a: ohne Session gibt die Route 401, nicht 303 und nicht 426.
    #[tokio::test]
    async fn route_ohne_session_gibt_401() {
        let antwort = test_router()
            .oneshot(
                Request::builder()
                    .uri("/obs/ws?streamer=earlysalty")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(antwort.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(fehlerkoerper(antwort).await["error"], "nicht_angemeldet");
    }

    /// Ohne `DashboardAuthState` faellt der Extractor auf `None` zurueck; ein
    /// mitgeschickter Cookie darf daran nichts aendern.
    #[tokio::test]
    async fn route_mit_unbrauchbarem_cookie_bleibt_401() {
        let antwort = test_router()
            .oneshot(
                Request::builder()
                    .uri("/obs/ws?streamer=earlysalty")
                    .header("cookie", "twitch_dash_session=erfunden")
                    .header("connection", "upgrade")
                    .header("upgrade", "websocket")
                    .header("sec-websocket-version", "13")
                    .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(antwort.status(), StatusCode::UNAUTHORIZED);
    }

    /// Beweisziel 3b: ein Partner auf einem fremden Kanal bekommt 403.
    #[tokio::test]
    async fn partner_auf_fremdem_kanal_gibt_403() {
        let antwort =
            kanal_freigabe(&partner("earlysalty", "9062301"), Some("ismile_e")).unwrap_err();
        assert_eq!(antwort.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn partner_auf_eigenem_kanal_wird_freigegeben() {
        let auth = partner("EarlySalty", "9062301");
        assert_eq!(
            kanal_freigabe(&auth, Some("earlysalty")).unwrap(),
            "earlysalty"
        );
        // Ohne Parameter bleibt es beim eigenen Login.
        assert_eq!(kanal_freigabe(&auth, None).unwrap(), "earlysalty");
    }

    #[tokio::test]
    async fn ohne_session_gibt_die_freigabe_401_ohne_redirect() {
        let antwort = kanal_freigabe(&DashboardAuthLevel::None, Some("earlysalty")).unwrap_err();
        assert_eq!(antwort.status(), StatusCode::UNAUTHORIZED);
        assert!(antwort
            .headers()
            .get(axum::http::header::LOCATION)
            .is_none());
    }

    #[tokio::test]
    async fn admin_darf_jeden_kanal_muss_ihn_aber_nennen() {
        let admin = DashboardAuthLevel::admin();
        assert_eq!(
            kanal_freigabe(&admin, Some("ismile_e")).unwrap(),
            "ismile_e"
        );
        let antwort = kanal_freigabe(&admin, None).unwrap_err();
        assert_eq!(antwort.status(), StatusCode::BAD_REQUEST);
    }

    /// Die Kanalkennung kommt ohne Datenbankzugriff aus der Partner-Session.
    #[tokio::test]
    async fn kanal_kennung_kommt_aus_der_session() {
        let auth = partner("EarlySalty", "9062301");
        let kennung = kanal_kennung(&traeger_pool(), &auth, "earlysalty")
            .await
            .unwrap();
        assert_eq!(kennung, "9062301");
    }

    /// Ohne Kennung in der Session und ohne erreichbare Tabelle bleibt es bei
    /// 404 statt bei einem 500 aus der Datenbank.
    #[tokio::test]
    async fn kanal_ohne_kennung_gibt_404() {
        let auth = partner("earlysalty", "");
        let antwort = kanal_kennung(&traeger_pool(), &auth, "earlysalty")
            .await
            .unwrap_err();
        assert_eq!(antwort.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn nur_der_ping_gilt_als_client_rahmen() {
        assert!(ist_ping(&Message::Text(r#"{"typ":"ping"}"#.to_string())));
        assert!(!ist_ping(&Message::Text(r#"{"typ":"chat"}"#.to_string())));
        assert!(!ist_ping(&Message::Text("kein json".to_string())));
        assert!(!ist_ping(&Message::Binary(vec![1, 2, 3])));
        assert!(!ist_ping(&Message::Pong(Vec::new())));
    }

    #[test]
    fn seit_wird_nachsichtig_gelesen() {
        let mit = |wert: &str| ObsWsAbfrage {
            streamer: None,
            seit: Some(wert.to_string()),
        };
        assert_eq!(mit("42").seit(), Some(42));
        assert_eq!(mit(" 42 ").seit(), Some(42));
        assert_eq!(mit("").seit(), None);
        assert_eq!(mit("keine zahl").seit(), None);
        assert_eq!(mit("-3").seit(), None);
        assert_eq!(mit("0").seit(), None);
        assert_eq!(ObsWsAbfrage::default().seit(), None);
    }

    #[test]
    fn session_waechter_ohne_partner_prueft_nicht_nach() {
        let waechter =
            SessionWaechter::bauen(&DashboardAuthLevel::admin(), None, &HeaderMap::new());
        assert!(matches!(waechter, SessionWaechter::Ohne));
    }

    #[test]
    fn session_waechter_ohne_cookie_prueft_nicht_nach() {
        let waechter =
            SessionWaechter::bauen(&partner("earlysalty", "9062301"), None, &HeaderMap::new());
        assert!(matches!(waechter, SessionWaechter::Ohne));
    }

    #[tokio::test]
    async fn session_waechter_ohne_pruefung_bleibt_gueltig() {
        assert!(SessionWaechter::Ohne.gueltig().await);
    }
}

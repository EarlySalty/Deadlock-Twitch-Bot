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
//! # Drahtformat zum Dock
//!
//! Jeder Serverrahmen ist ein Huellobjekt mit der `obs_dock_events.id`:
//!
//! ```json
//! {"id": 123, "ereignis": {"typ": "chat", ...}}
//! ```
//!
//! Ohne diese `id` koennte ein Dock den Punkt 4 oben gar nicht bedienen: es
//! wuesste nach einem Neustart nicht, wo es stand, muesste jedes Mal ohne
//! `?seit=` verbinden und bekaeme die letzten [`VORLAUF_OHNE_SEIT`] Zeilen
//! erneut. Genau das sind die Dubletten und Luecken, die der Nachlauf
//! ausschliessen soll. Das Ereignis selbst steht unveraendert unter
//! `ereignis`; sein Format ist `tb_platform_core::PlatformEvent` und dort
//! eingefroren, deshalb die Huelle darum statt eines Feldes darin (ein `id`
//! auf oberster Ebene wuerde ausserdem mit `chat.id` kollidieren).
//!
//! Musste der Server Zeilen ueberspringen, weil das Dock zu weit zurueckliegt,
//! kommt statt eines Ereignisses ein Lueckenhinweis mit derselben `id`-Stelle:
//!
//! ```json
//! {"id": 900, "luecke": true}
//! ```
//!
//! Das Dock uebernimmt die `id` genauso und weiss, dass sein Verlauf davor
//! unvollstaendig ist. Einen solchen Rahmen gibt es in drei Faellen: der
//! Nachlauf reicht ueber [`NACHLAUF_RUNDEN`] Runden hinaus, die Datenbank ist
//! beim Nachlauf nicht lesbar, oder der Bus selbst hat Zeilen ueberspringen
//! muessen. Stumm uebersprungen wird nichts.
//!
//! `"id": 0` ist dabei der Sonderfall "Wiederaufsetzstelle unbekannt". Er
//! trifft ein Dock, das ohne `?seit=` verbindet und dessen Vorlauf schon an
//! der Datenbank scheitert: dann kennt der Server keine einzige `id` dieses
//! Kanals und kann auch keine nennen. Das Dock behandelt ihn wie jeden
//! Lueckenrahmen, behaelt aber sein altes Lesezeichen (0 hebt nichts an) und
//! faengt beim naechsten Versuch wieder von vorn an.
//!
//! # Was das Dock sich merken muss
//!
//! `obs_dock_events.id` wird **nicht** in Sichtbarkeitsreihenfolge vergeben
//! (Begruendung in [`crate::obs::bus`]). Der Socket liefert deshalb bewusst
//! auch einen Rahmen aus, dessen `id` kleiner ist als eine schon gesendete:
//! ihn wegzuwerfen waere die stille Luecke, die dieses Modul ausschliesst.
//!
//! Daraus folgen zwei Punkte fuer das Dock:
//!
//! - Das Lesezeichen fuer `?seit=` ist **nicht** blind die zuletzt empfangene
//!   `id`. Kommt eine kleinere `id` nach einer groesseren, ist die groessere
//!   als Lesezeichen zu hoch: `WHERE id > seit` beim naechsten Verbindungs-
//!   aufbau wuerde die kleinere nie liefern. Das Dock nimmt in dem Fall die
//!   kleinere minus eins.
//! - Innerhalb einer Verbindung ist die Auslieferung dublettenfrei; ueber einen
//!   Verbindungswechsel hinweg gilt "mindestens einmal". Das Dock kennt zu
//!   jedem Rahmen die `id` und wirft eine Wiederholung selbst weg.
//!
//! Der Client darf ausschliesslich `{"typ":"ping"}` senden und bekommt darauf
//! `{"typ":"pong"}`. Jeder andere Rahmen wird verworfen.

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
use crate::auth::session::{
    DashboardAuthState, ADMIN_COOKIE_NAME, PARTNER_ACCESS_COOKIE_NAME, PARTNER_COOKIE_NAME,
};
use crate::obs::bus::{
    Auslieferung, ObsDockBus, SchliessGrund, NACHLAUF_DECKEL, NACHZUG_RUECKGRIFF, VORLAUF_OHNE_SEIT,
};

/// Abstand der Server-Pings.
const HEARTBEAT: Duration = Duration::from_secs(30);

/// Nach so langer Funkstille gilt der Socket als tot. Vier verpasste Pings.
const LEERLAUF_DECKEL: Duration = Duration::from_secs(120);

/// Abstand der Session-Nachpruefung.
const SESSION_PRUEFUNG: Duration = Duration::from_secs(60);

/// Antwort auf `{"typ":"ping"}`.
const PONG: &str = r#"{"typ":"pong"}"#;

/// Wie viele Nachlauf-Runden zu je [`NACHLAUF_DECKEL`] Zeilen ein Socket faehrt,
/// bevor er aufgibt und dem Dock stattdessen einen Lueckenhinweis schickt.
///
/// Eine einzelne gekappte Abfrage waere die stille Variante desselben Fehlers:
/// das Dock bekaeme die aeltesten 200 Zeilen und wuerde danach live
/// weiterlaufen, alles dazwischen waere weg, ohne dass es jemand merkt.
const NACHLAUF_RUNDEN: u32 = 5;

/// Zusaetzliche Leserunden, die allein dafuer draufgehen, den Rueckgriff des
/// Aufholens wieder einzuholen.
///
/// Ohne diesen Zuschlag frisst der Rueckgriff das ganze Budget:
/// [`NACHLAUF_RUNDEN`] mal [`NACHLAUF_DECKEL`] sind 1000 Zeilen, und
/// [`NACHZUG_RUECKGRIFF`] setzt den Lesezeiger genau 1000 Zeilen unter den
/// Anker. Netto-Vorwaertsreichweite null, sobald der Kanal die IDs darunter
/// selbst fuellt, und genau das tut er, wenn ein `Lagged` entsteht. Der Socket
/// laese dann fuenf Runden lang nur Zeilen, die er schon gesendet hat, und
/// wuerde die wirklich verpassten hinterher per Lueckenrahmen wegwerfen,
/// obwohl sie vollstaendig in `obs_dock_events` liegen.
///
/// Der Zuschlag ist aus den beiden Zahlen gerechnet und nicht geraten, damit
/// er nicht falsch wird, wenn jemand an einer davon dreht.
const RUECKGRIFF_RUNDEN: u32 =
    ((NACHZUG_RUECKGRIFF + NACHLAUF_DECKEL - 1) / NACHLAUF_DECKEL) as u32;

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
    /// Interner Token (`X-Internal-Token`): daran haengt keine ablaufende
    /// Sitzung, es gibt nichts nachzupruefen.
    Ohne,
    /// Partner-Cookie, wird gegen den lokalen Sitzungsspeicher nachgeprueft.
    Partner {
        state: DashboardAuthState,
        sitzung: Option<String>,
        dauersitzung: Option<String>,
        user_agent: String,
    },
    /// Master-Session (`master_dash_session`), wird nachgeprueft.
    ///
    /// Die traegt nicht nur das Admin-Level: `master_session_auth` in
    /// `auth/level.rs` macht aus derselben Sitzung im oeffentlichen Kontext ein
    /// [`DashboardAuthLevel::Partner`] auf den Admin-Login, ganz ohne
    /// Partner-Cookie. Genau diese Sockets liefen frueher als [`Self::Ohne`]
    /// und wurden nie nachgeprueft, obwohl der Modulkopf die Nachpruefung im
    /// Minutentakt zusagt.
    ///
    /// # Was hier NICHT geprueft wird
    ///
    /// Geprueft wird `load_admin_session`, also der lokale Spiegel. Die
    /// Wahrheit ueber eine Admin-Sitzung liegt aber beim zentralen
    /// Discord-Dienst; `auth/level.rs` fragt dort mit
    /// `DiscordAdminLoginConfig::client::validate_session` nach und spiegelt
    /// nur. Eine dort entzogene Sitzung haelt diesen Socket also bis zum
    /// lokalen Ablauf offen.
    ///
    /// Das ist bewusst so geblieben und kein Versehen: `validate_session`
    /// liefert bei Entzug **und** bei einem Netzaussetzer denselben
    /// `DiscordAdminOAuthError`, die beiden Faelle sind am Rueckgabewert nicht
    /// zu trennen. Fuer eine Seitenanfrage ist das harmlos, dort kostet ein
    /// Aussetzer eine 401 und einen neuen Versuch. Fuer einen Socket, der acht
    /// Stunden haengt und im Minutentakt fragt, wuerde derselbe Aussetzer
    /// jedes Admin-Dock hinauswerfen. Die Trennung gehoert in den Fehlertyp in
    /// `auth/discord_admin_login.rs` und damit in eine eigene Aenderung am
    /// Auth-Modul.
    Master {
        state: DashboardAuthState,
        sitzungen: Vec<String>,
    },
}

impl SessionWaechter {
    fn bauen(
        auth: &DashboardAuthLevel,
        state: Option<DashboardAuthState>,
        headers: &HeaderMap,
    ) -> Self {
        if matches!(auth, DashboardAuthLevel::None) {
            return Self::Ohne;
        }
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
        if sitzung.is_some() || dauersitzung.is_some() {
            return Self::Partner {
                state,
                sitzung,
                dauersitzung,
                user_agent: headers
                    .get(axum::http::header::USER_AGENT)
                    .and_then(|wert| wert.to_str().ok())
                    .unwrap_or("")
                    .to_string(),
            };
        }

        let sitzungen: Vec<String> = crate::auth::level::cookie_values(headers, ADMIN_COOKIE_NAME)
            .into_iter()
            .filter(|wert| !wert.is_empty())
            .map(str::to_string)
            .collect();
        if !sitzungen.is_empty() {
            return Self::Master { state, sitzungen };
        }

        // Weder Partner- noch Master-Cookie: dann kam das Auth-Level aus dem
        // internen Token (`tb_http_core::AuthLevel::Admin`, Header
        // `X-Internal-Token`). Der laeuft nicht ab, also ist nichts
        // nachzupruefen.
        Self::Ohne
    }

    /// `true`, solange die Sitzung traegt. Ein Datenbankfehler zaehlt bewusst
    /// als "traegt": ein kurzer Aussetzer soll kein Dock aus dem Stream werfen.
    async fn gueltig(&self) -> bool {
        match self {
            Self::Ohne => true,
            Self::Partner {
                state,
                sitzung,
                dauersitzung,
                user_agent,
            } => {
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
            Self::Master { state, sitzungen } => {
                for id in sitzungen {
                    match state.load_admin_session(id).await {
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
    }
}

/// `GET /obs/ws`.
///
/// `WebSocketUpgrade` steht bewusst als `Option`: ein gewoehnlicher GET ohne
/// Upgrade-Header soll die Auth-Antwort sehen (401/403) und nicht die
/// Upgrade-Abweisung des Extractors. Erst wenn die Auth traegt, wird der
/// fehlende Upgrade-Header zum Fehler.
///
/// # Kein Origin-Check, und warum das hier traegt
///
/// Ein WebSocket-Upgrade unterliegt nicht der Same-Origin-Policy, ein
/// Origin-Check waere also die uebliche Bremse gegen Cross-Site-Hijacking.
/// Hier haelt stattdessen das Cookie selbst: alle Sitzungs-Cookies dieses
/// Dashboards werden mit `SameSite=Lax` gesetzt
/// (`handlers/auth_login.rs:396` fuer `twitch_dash_session`,
/// `handlers/partner_login.rs:311` fuer `twitch_dash_session_partner`), und
/// `auth::session::SameSite` kennt ueberhaupt nur `Lax` und `Strict`, ein
/// `None` gibt es im Typ nicht. Ein Upgrade ist keine Top-Level-Navigation,
/// also schickt der Browser ein Lax-Cookie dabei nicht mit, und ein fremder
/// Ursprung landet hier ohne Sitzung auf 401.
///
/// Wer je ein Sitzungs-Cookie auf `SameSite=None` stellt, muss an dieser
/// Stelle eine Origin-Pruefung nachziehen.
///
/// **Ungeprueft bleibt `master_dash_session`.** Dieses Cookie wird nach
/// `auth/level.rs` auch aus einer zentralen Admin-Instanz gespiegelt, und
/// deren Code liegt nicht in diesem Repo. Ob es dort ebenfalls mit
/// `SameSite=Lax` gesetzt wird, konnte ich nicht nachsehen. Fuer die beiden
/// Cookies aus diesem Repo (`twitch_dash_session` in
/// `handlers/auth_login.rs`, `twitch_dash_session_partner` in
/// `handlers/partner_login.rs`) traegt das Argument nachweislich, fuer den
/// Admin-Weg ist es eine Annahme. Wer die zentrale Instanz einsehen kann,
/// sollte das nachtragen oder hier eine Origin-Pruefung einziehen.
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

    upgrade.on_upgrade(move |socket| async move {
        // Anmelden erst hier, nicht schon bei den Upgrade-Kopfzeilen: die
        // Anmeldung verdraengt ueber `MAX_SOCKETS_JE_PARTNER` den aeltesten
        // Socket desselben Streamers. Haenge das an die Kopfzeilen, koennte
        // eine Reihe abgebrochener Upgrade-Anfragen die echten Docks
        // reihum hinauswerfen, ohne je einen Socket zu bedienen.
        //
        // Die Reihenfolge, auf die es ankommt, bleibt gewahrt: abonniert wird
        // vor dem Nachlauf in `socket_bedienen`, dazwischen sammeln sich die
        // Live-Rahmen im Puffer.
        let anmeldung = bus.anmelden(&channel_id);
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

/// Der Rahmen, der wirklich ueber die Leitung geht: das Ereignis in einer
/// Huelle mit seiner `obs_dock_events.id`.
///
/// `ereignis_json` ist die Spalte `payload` als Text und damit garantiert
/// gueltiges JSON (die Spalte ist `JSONB`), deshalb wird sie hier nur
/// eingesetzt und nicht neu geparst.
fn drahtrahmen(id: i64, ereignis_json: &str) -> String {
    format!(r#"{{"id":{id},"ereignis":{ereignis_json}}}"#)
}

/// Hinweis an das Dock, dass der Server bis `bis` Zeilen uebersprungen hat.
fn luecken_rahmen(bis: i64) -> String {
    format!(r#"{{"id":{bis},"luecke":true}}"#)
}

/// Hoechste `id`, die der Kanal gerade hat. Fuer den Fall, dass der Nachlauf
/// aufgibt und dem Dock stattdessen sagen muss, wo es weitergeht.
async fn hoechste_id(pool: &PgPool, channel_id: &str) -> Result<i64, sqlx::Error> {
    let wert: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(MAX(id), 0) FROM obs_dock_events WHERE channel_id = $1",
    )
    .bind(channel_id)
    .fetch_optional(pool)
    .await?
    .flatten();
    Ok(wert.unwrap_or(0))
}

/// Wofuer der Nachlauf gerade gelesen wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NachlaufArt {
    /// Das Dock kennt keine `id` und bekommt die letzten
    /// [`VORLAUF_OHNE_SEIT`] Zeilen. Eine Runde, hier gibt es nichts aufzuholen.
    Vorlauf,
    /// Verbindungsaufbau mit `?seit=<id>`: gelesen wird ab dem Lesezeichen des
    /// Docks. Ein Rueckgriff waere hier falsch, das Dock hat alles darunter
    /// laut eigener Angabe schon.
    Lesezeichen,
    /// Aufholen nach einem `Lagged`. Hier wird
    /// [`NACHZUG_RUECKGRIFF`] Zeilen **unter** den Anker zurueckgelesen: der
    /// Anker ist die hoechste gesendete `id`, und darunter kann eine Zeile
    /// nachtraeglich sichtbar geworden sein (siehe [`crate::obs::bus`]). Was
    /// dieser Socket davon schon gesendet hat, filtert [`Auslieferung`] weg.
    ///
    /// Der Rueckgriff endet an [`Auslieferung::untergrenze`]. Sonst bekaeme
    /// ausgerechnet ein Dock ohne `?seit=`, das nur die letzten
    /// [`VORLAUF_OHNE_SEIT`] Zeilen bestellt hat, beim ersten `Lagged` bis zu
    /// [`NACHZUG_RUECKGRIFF`] alte Zeilen als frische Rahmen.
    Aufholen,
}

/// Sendet den Nachlauf und laesst dabei nichts liegen.
///
/// Ab dem Startpunkt der jeweiligen [`NachlaufArt`] in Runden zu
/// [`NACHLAUF_DECKEL`] Zeilen, bis der Socket wirklich aufgeschlossen hat.
/// Reichen [`NACHLAUF_RUNDEN`] Runden nicht, bekommt das Dock einen
/// Lueckenhinweis und faengt beim aktuellen Stand an, statt dass die Zeilen
/// dazwischen stumm verschwinden.
///
/// Gelesen wird streng ueber den lokalen Lesezeiger, nicht ueber
/// `buch.anker()`: der Anker ist ein Sendestand und kein Lesestand, und der
/// Rueckgriff beim Aufholen wuerde sich sonst nach der ersten Runde selbst
/// wieder aufheben.
///
/// `false` heisst: der Socket ist weg. Ein Datenbankfehler heisst das
/// ausdruecklich nicht; er endet aber in einem Lueckenhinweis ans Dock, denn
/// der Modulkopf sagt zu, dass uebersprungene Zeilen immer angekuendigt werden.
async fn nachlauf_senden(
    pool: &PgPool,
    channel_id: &str,
    buch: &mut Auslieferung,
    schreiber: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    art: NachlaufArt,
) -> bool {
    if art == NachlaufArt::Vorlauf {
        match nachlauf_lesen(pool, channel_id, None, VORLAUF_OHNE_SEIT).await {
            Ok(zeilen) => {
                // Die Untergrenze VOR dem Senden ziehen, nicht danach: dieses
                // Dock hat nur diesen Ausschnitt bestellt, und ein spaeterer
                // Rueckgriff darf ihm nicht die Stunde davor ins Overlay
                // kippen. Ohne die Grenze stuende sie bei 0.
                if let Some((erste, _)) = zeilen.first() {
                    buch.untergrenze_setzen(erste - 1);
                }
                for (id, json) in zeilen {
                    if schreiber
                        .send(Message::Text(drahtrahmen(id, &json)))
                        .await
                        .is_err()
                    {
                        return false;
                    }
                    buch.nachlauf(id);
                }
            }
            Err(db_fehler) => {
                warn!(%db_fehler, channel_id, "OBS-Dock: Vorlauf nicht lesbar");
                // `anker()` ist hier 0, das Dock bekommt also
                // `{"id":0,"luecke":true}`. Eine bessere Zahl gibt es nicht:
                // die Abfrage, die alle `id` dieses Kanals kennt, ist gerade
                // die, die fehlgeschlagen ist. 0 heisst deshalb ausdruecklich
                // "Verlauf unvollstaendig, Wiederaufsetzstelle unbekannt",
                // siehe Modulkopf.
                return luecke_melden(schreiber, buch, buch.anker()).await;
            }
        }
        return true;
    }

    let mut zeiger = match art {
        // Nur bis zur Untergrenze zurueck, nicht bis 0. Das ist reine
        // Sparsamkeit und kein Schutz: was darunter liegt, wirft `buch.live`
        // ohnehin weg, hier wird es nur gar nicht erst gelesen. Deshalb gibt
        // es dazu auch keinen eigenen Test, ein Ausbau waere von aussen nicht
        // zu sehen.
        NachlaufArt::Aufholen => (buch.anker() - NACHZUG_RUECKGRIFF).max(buch.untergrenze()),
        _ => buch.anker(),
    };
    // Runden fuer die Strecke nach vorn plus die Runden, die der Rueckgriff
    // kostet. Sonst zaehlt das wiederholte Lesen des eigenen Rueckgriffs gegen
    // das Budget, mit dem der Socket aufholen soll.
    let runden = if zeiger < buch.anker() {
        NACHLAUF_RUNDEN + RUECKGRIFF_RUNDEN
    } else {
        NACHLAUF_RUNDEN
    };
    for _ in 0..runden {
        let zeilen = match nachlauf_lesen(pool, channel_id, Some(zeiger), NACHLAUF_DECKEL).await {
            Ok(zeilen) => zeilen,
            Err(db_fehler) => {
                warn!(%db_fehler, channel_id, "OBS-Dock: Nachlauf nicht lesbar");
                return luecke_melden(schreiber, buch, buch.anker()).await;
            }
        };
        let anzahl = zeilen.len() as i64;
        for (id, json) in zeilen {
            zeiger = zeiger.max(id);
            if !buch.live(id) {
                continue;
            }
            if schreiber
                .send(Message::Text(drahtrahmen(id, &json)))
                .await
                .is_err()
            {
                return false;
            }
        }
        if anzahl < NACHLAUF_DECKEL {
            return true;
        }
    }

    // Immer noch hinterher. Dem Dock sagen, dass hier etwas fehlt, und beim
    // aktuellen Stand weitermachen.
    let ziel = match hoechste_id(pool, channel_id).await {
        Ok(ziel) => ziel,
        Err(db_fehler) => {
            warn!(%db_fehler, channel_id, "OBS-Dock: Lueckenhinweis nicht bestimmbar");
            return luecke_melden(schreiber, buch, buch.anker()).await;
        }
    };
    if ziel <= buch.anker() {
        return true;
    }
    warn!(
        channel_id,
        uebersprungen = ziel - buch.anker(),
        "OBS-Dock: Nachlauf zu gross, Rest wird uebersprungen"
    );
    luecke_melden(schreiber, buch, ziel).await
}

/// Schickt dem Dock einen Lueckenhinweis bis `bis` und bucht ihn.
///
/// `false` heisst wie ueberall: der Socket ist weg.
async fn luecke_melden(
    schreiber: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    buch: &mut Auslieferung,
    bis: i64,
) -> bool {
    if schreiber
        .send(Message::Text(luecken_rahmen(bis)))
        .await
        .is_err()
    {
        return false;
    }
    buch.luecke_bis(bis);
    true
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
    let art = match seit {
        Some(_) => NachlaufArt::Lesezeichen,
        None => NachlaufArt::Vorlauf,
    };
    if !nachlauf_senden(&pool, &channel_id, &mut buch, &mut schreiber, art).await {
        return;
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

    // Reihenfolge von `biased`: erst alles, was selten dran ist und trotzdem
    // drankommen muss, zuletzt der Ereignisstrom. Stuende `rahmen.recv()` vorn,
    // kaemen bei einem dauerhaft vollen Kanal weder Ping noch
    // Session-Nachpruefung noch die Rahmen des Clients je an die Reihe, weil
    // `select!` bei jedem Durchlauf wieder oben anfaengt.
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

            eingang = rahmen.recv() => {
                match eingang {
                    Ok(rahmen) => match rahmen.json {
                        // Lueckenhinweis vom Bus: der hat Zeilen ueberspringen
                        // muessen und weiss nicht mehr, zu welchem Kanal sie
                        // gehoerten. Das Dock erfaehrt es trotzdem.
                        None => {
                            if !luecke_melden(&mut schreiber, &mut buch, rahmen.id).await {
                                break None;
                            }
                        }
                        Some(json) => {
                            // `live` fragt nicht "ist die id hoeher", sondern
                            // "habe ich genau die schon gesendet". Ein Rahmen,
                            // der hinter einer groesseren id eintrifft, geht
                            // deshalb trotzdem raus.
                            if !buch.live(rahmen.id) {
                                continue;
                            }
                            if schreiber.send(Message::Text(drahtrahmen(rahmen.id, &json))).await.is_err() {
                                break None;
                            }
                        }
                    },
                    Err(RecvError::Lagged(verloren)) => {
                        // Der Socket kam nicht hinterher. Die Luecke steht in
                        // der Tabelle, also von dort nachziehen statt sie
                        // stillschweigend zu verlieren.
                        warn!(channel_id, verloren, "OBS-Dock: Socket hinkte nach, Luecke wird nachgezogen");
                        if !nachlauf_senden(&pool, &channel_id, &mut buch, &mut schreiber, NachlaufArt::Aufholen).await {
                            break None;
                        }
                    }
                    Err(RecvError::Closed) => break None,
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

    fn kopfzeilen(paare: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, wert) in paare {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                wert.parse().unwrap(),
            );
        }
        headers
    }

    /// Wie [`traeger_pool`], aber mit kurzer Wartezeit: die Waechter-Tests
    /// wollen den Datenbankfehler sehen und nicht 30 Sekunden darauf warten.
    fn auth_state() -> DashboardAuthState {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy("postgres://obs-dock-test@127.0.0.1:1/obs")
            .expect("lazy pool");
        DashboardAuthState::new(pool, "erfundener-key".to_string())
    }

    /// Der Fall aus dem Review: `master_dash_session` erzeugt im oeffentlichen
    /// Kontext ein Partner-Level ohne Partner-Cookie. Dieser Socket muss
    /// nachgeprueft werden und darf nicht als [`SessionWaechter::Ohne`]
    /// durchrutschen.
    #[tokio::test]
    async fn master_session_ohne_partner_cookie_wird_nachgeprueft() {
        let waechter = SessionWaechter::bauen(
            &partner("earlysalty", "9062301"),
            Some(auth_state()),
            &kopfzeilen(&[("cookie", "master_dash_session=abc")]),
        );
        assert!(matches!(waechter, SessionWaechter::Master { .. }));
    }

    /// Auch ein echtes Admin-Level aus derselben Sitzung wird nachgeprueft.
    #[tokio::test]
    async fn admin_mit_master_cookie_wird_nachgeprueft() {
        let waechter = SessionWaechter::bauen(
            &DashboardAuthLevel::admin(),
            Some(auth_state()),
            &kopfzeilen(&[("cookie", "master_dash_session=abc")]),
        );
        assert!(matches!(waechter, SessionWaechter::Master { .. }));
    }

    /// Ohne jedes Cookie bleibt nur der interne Token, und der laeuft nicht ab.
    #[tokio::test]
    async fn interner_token_ohne_cookie_bleibt_ohne_pruefung() {
        let waechter = SessionWaechter::bauen(
            &DashboardAuthLevel::admin(),
            Some(auth_state()),
            &kopfzeilen(&[("x-internal-token", "geheim")]),
        );
        assert!(matches!(waechter, SessionWaechter::Ohne));
    }

    #[tokio::test]
    async fn partner_cookie_schlaegt_master_cookie() {
        let waechter = SessionWaechter::bauen(
            &partner("earlysalty", "9062301"),
            Some(auth_state()),
            &kopfzeilen(&[("cookie", "twitch_dash_session=abc; master_dash_session=def")]),
        );
        assert!(matches!(waechter, SessionWaechter::Partner { .. }));
    }

    /// Beide Zweige von [`SessionWaechter::gueltig`] gegen eine unerreichbare
    /// Datenbank: ein Aussetzer wirft kein Dock aus dem Stream.
    #[tokio::test]
    async fn db_aussetzer_wirft_kein_dock_raus() {
        let partner_waechter = SessionWaechter::Partner {
            state: auth_state(),
            sitzung: Some("abc".to_string()),
            dauersitzung: Some("def".to_string()),
            user_agent: "OBS".to_string(),
        };
        assert!(partner_waechter.gueltig().await);

        let master_waechter = SessionWaechter::Master {
            state: auth_state(),
            sitzungen: vec!["abc".to_string()],
        };
        assert!(master_waechter.gueltig().await);
    }

    #[test]
    fn drahtrahmen_traegt_die_id_neben_dem_ereignis() {
        let text = drahtrahmen(42, r#"{"typ":"chat","id":"abc"}"#);
        let wert: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(wert["id"], 42);
        assert_eq!(wert["ereignis"]["typ"], "chat");
        // Die eigene `id` des Ereignisses bleibt unangetastet; genau deshalb
        // die Huelle statt eines Feldes im Ereignis.
        assert_eq!(wert["ereignis"]["id"], "abc");

        let luecke: serde_json::Value = serde_json::from_str(&luecken_rahmen(900)).unwrap();
        assert_eq!(luecke["id"], 900);
        assert_eq!(luecke["luecke"], true);
    }
}

/// Tests am echten Socket: eigener Server auf einem echten Port, echter
/// WebSocket-Client, echte Tabelle.
///
/// Diese Tests belegen den Weg, den kein Baustein-Test belegen kann:
/// [`socket_bedienen`], [`nachlauf_lesen`], [`nachlauf_senden`] und das
/// Drahtformat mit der `obs_dock_events.id`.
///
/// Ohne `TB_TEST_DATABASE_URL` ueberspringen sie sich selbst.
#[cfg(test)]
mod socket_tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use sqlx::postgres::PgPoolOptions;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    const TOKEN: &str = "obs-dock-testtoken";
    const KANAL: &str = "9062301";

    macro_rules! dsn_oder_skip {
        () => {
            match std::env::var("TB_TEST_DATABASE_URL") {
                Ok(dsn) => dsn,
                Err(_) => {
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    /// Eigenes Schema je Test, damit parallele Tests sich nicht in die Quere
    /// kommen. Das DDL ist dasselbe wie in der Migration des Schreibpfads.
    async fn schema_pool(dsn: &str, schema: &str) -> PgPool {
        let aufbau = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("Test-DB erreichbar");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&aufbau)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&aufbau)
            .await
            .expect("Schema anlegen");
        aufbau.close().await;

        let name = schema.to_string();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |conn, _| {
                let name = name.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {name}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(dsn)
            .await
            .expect("Testpool");

        sqlx::query(
            "CREATE TABLE obs_dock_events (
                 id BIGSERIAL PRIMARY KEY,
                 channel_id TEXT NOT NULL,
                 payload JSONB NOT NULL,
                 dedupe_key TEXT,
                 created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
             )",
        )
        .execute(&pool)
        .await
        .expect("obs_dock_events anlegen");
        sqlx::query(
            "CREATE TABLE twitch_partners (
                 id BIGSERIAL PRIMARY KEY,
                 twitch_login TEXT NOT NULL,
                 twitch_user_id TEXT
             )",
        )
        .execute(&pool)
        .await
        .expect("twitch_partners anlegen");
        sqlx::query("INSERT INTO twitch_partners (twitch_login, twitch_user_id) VALUES ($1, $2)")
            .bind("earlysalty")
            .bind(KANAL)
            .execute(&pool)
            .await
            .expect("Partner anlegen");
        pool
    }

    async fn ereignis_schreiben(pool: &PgPool, text: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO obs_dock_events (channel_id, payload)
             VALUES ($1, $2::jsonb)
             RETURNING id",
        )
        .bind(KANAL)
        .bind(format!(r#"{{"typ":"chat","id":"{text}","text":"{text}"}}"#))
        .fetch_one(pool)
        .await
        .expect("Ereignis schreiben")
    }

    /// Startet den echten Router auf einem echten Port.
    async fn server_starten(pool: PgPool) -> std::net::SocketAddr {
        let router = Router::new()
            .route("/obs/ws", get(obs_ws_handler))
            .layer(Extension(ObsDockBus::ohne_datenbank()))
            .layer(Extension(tb_http_core::ExpectedToken(TOKEN.to_string())))
            .with_state(pool);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Port");
        let adresse = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        adresse
    }

    type Socket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn verbinden(adresse: std::net::SocketAddr, seit: Option<i64>) -> Socket {
        let anhang = match seit {
            Some(id) => format!("&seit={id}"),
            None => String::new(),
        };
        let mut anfrage = format!("ws://{adresse}/obs/ws?streamer=earlysalty{anhang}")
            .into_client_request()
            .expect("Anfrage");
        anfrage
            .headers_mut()
            .insert(tb_http_core::INTERNAL_TOKEN_HEADER, TOKEN.parse().unwrap());
        let (socket, _) = tokio_tungstenite::connect_async(anfrage)
            .await
            .expect("Socket verbunden");
        socket
    }

    /// Liest, bis der Server eine Weile still ist. Genau so merkt auch ein Dock,
    /// dass der Nachlauf durch ist.
    async fn alles_lesen(socket: &mut Socket) -> Vec<serde_json::Value> {
        let mut gelesen = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_millis(750), socket.next()).await {
                Ok(Some(Ok(ClientMessage::Text(text)))) => {
                    gelesen.push(serde_json::from_str(&text).expect("JSON-Rahmen"));
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => break,
            }
        }
        gelesen
    }

    fn texte(rahmen: &[serde_json::Value]) -> Vec<String> {
        rahmen
            .iter()
            .filter_map(|wert| wert["ereignis"]["text"].as_str())
            .map(str::to_string)
            .collect()
    }

    /// Beweis fuer den Review-Fund: die `id` steht im Drahtformat, das Dock kann
    /// sie auslesen, mit `?seit=<id>` neu verbinden und bekommt dann genau die
    /// Zeilen dazwischen: keine fehlt, keine kommt doppelt.
    #[tokio::test(flavor = "multi_thread")]
    async fn reconnect_mit_gesehener_id_am_echten_socket() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_reconnect").await;
        for lauf in 1..=5 {
            ereignis_schreiben(&pool, &format!("e{lauf}")).await;
        }
        let adresse = server_starten(pool.clone()).await;

        // 1. Verbinden ohne `?seit=`: der Vorlauf kommt.
        let mut erster = verbinden(adresse, None).await;
        let erste_runde = alles_lesen(&mut erster).await;
        assert_eq!(texte(&erste_runde), vec!["e1", "e2", "e3", "e4", "e5"]);

        // 2. Die gesehene `id` aus dem Drahtformat auslesen. Genau das war
        //    vorher unmoeglich.
        let gesehen = erste_runde
            .last()
            .and_then(|wert| wert["id"].as_i64())
            .expect("id im Rahmen");

        // 3. Socket weg, waehrenddessen laeuft der Kanal weiter.
        erster.close(None).await.ok();
        drop(erster);
        for lauf in 6..=8 {
            ereignis_schreiben(&pool, &format!("e{lauf}")).await;
        }

        // 4. Neu verbinden mit der gesehenen id.
        let mut zweiter = verbinden(adresse, Some(gesehen)).await;
        let zweite_runde = alles_lesen(&mut zweiter).await;

        // Weder Luecke (e6..e8 sind da) noch Dublette (e5 nicht noch einmal).
        assert_eq!(texte(&zweite_runde), vec!["e6", "e7", "e8"]);
        let ids: Vec<i64> = zweite_runde
            .iter()
            .filter_map(|wert| wert["id"].as_i64())
            .collect();
        assert!(ids.iter().all(|id| *id > gesehen), "ids: {ids:?}");
        zweiter.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_reconnect CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Live-Weg am echten Socket: was nach dem Nachlauf hereinkommt, geht mit
    /// derselben Huelle und derselben `id` hinaus, und die naechste Verbindung
    /// kann daran anschliessen.
    #[tokio::test(flavor = "multi_thread")]
    async fn live_rahmen_traegt_die_id_am_echten_socket() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_live").await;
        let bus = ObsDockBus::ohne_datenbank();
        let router = Router::new()
            .route("/obs/ws", get(obs_ws_handler))
            .layer(Extension(Arc::clone(&bus)))
            .layer(Extension(tb_http_core::ExpectedToken(TOKEN.to_string())))
            .with_state(pool.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Port");
        let adresse = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let mut socket = verbinden(adresse, None).await;
        // Nachlauf leerlaufen lassen, damit der Socket nachweislich live ist.
        assert!(alles_lesen(&mut socket).await.is_empty());

        let id = ereignis_schreiben(&pool, "live").await;
        bus.veroeffentlichen(
            KANAL,
            crate::obs::bus::BusRahmen::neu(id, r#"{"typ":"chat","id":"live","text":"live"}"#),
        );

        let gelesen = alles_lesen(&mut socket).await;
        assert_eq!(texte(&gelesen), vec!["live"]);
        assert_eq!(gelesen[0]["id"].as_i64(), Some(id));
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_live CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Der Fund dieser Runde, am echten Socket: der Bus meldet erst 101 und
    /// danach 100, weil `INSERT` und `pg_notify` beim Schreiber nicht atomar
    /// sind. Beide muessen beim Dock ankommen, keines doppelt.
    ///
    /// Mit dem alten Wasserstand-Filter (`id > letzte_id`) fiel 100 hier stumm
    /// heraus: kein Log, kein Lueckenrahmen, kein Nachholpfad.
    #[tokio::test(flavor = "multi_thread")]
    async fn verkehrte_reihenfolge_erreicht_das_dock_am_echten_socket() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_reihenfolge").await;
        let bus = ObsDockBus::ohne_datenbank();
        let router = Router::new()
            .route("/obs/ws", get(obs_ws_handler))
            .layer(Extension(Arc::clone(&bus)))
            .layer(Extension(tb_http_core::ExpectedToken(TOKEN.to_string())))
            .with_state(pool.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Port");
        let adresse = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let mut socket = verbinden(adresse, None).await;
        assert!(alles_lesen(&mut socket).await.is_empty(), "Kanal ist leer");

        // Zwei nebenlaeufige Schreiber: 100 wird zuerst geschrieben, 101
        // meldet sich zuerst.
        let klein = ereignis_schreiben(&pool, "klein").await;
        let gross = ereignis_schreiben(&pool, "gross").await;
        assert!(gross > klein);
        bus.veroeffentlichen(
            KANAL,
            crate::obs::bus::BusRahmen::neu(gross, r#"{"typ":"chat","id":"gross","text":"gross"}"#),
        );
        bus.veroeffentlichen(
            KANAL,
            crate::obs::bus::BusRahmen::neu(klein, r#"{"typ":"chat","id":"klein","text":"klein"}"#),
        );
        // Und noch einmal dieselben beiden, wie sie ein Nachzug mit Rueckgriff
        // liefern wuerde: die duerfen nicht durchkommen.
        bus.veroeffentlichen(
            KANAL,
            crate::obs::bus::BusRahmen::neu(klein, r#"{"typ":"chat","id":"klein","text":"klein"}"#),
        );

        let gelesen = alles_lesen(&mut socket).await;
        assert_eq!(
            texte(&gelesen),
            vec!["gross", "klein"],
            "beide Rahmen, jeder genau einmal"
        );
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_reihenfolge CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Ein Lueckenhinweis des Busses geht bis ans Dock durch.
    #[tokio::test(flavor = "multi_thread")]
    async fn bus_luecke_erreicht_das_dock() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_busluecke").await;
        let bus = ObsDockBus::ohne_datenbank();
        let router = Router::new()
            .route("/obs/ws", get(obs_ws_handler))
            .layer(Extension(Arc::clone(&bus)))
            .layer(Extension(tb_http_core::ExpectedToken(TOKEN.to_string())))
            .with_state(pool.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Port");
        let adresse = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let mut socket = verbinden(adresse, None).await;
        assert!(alles_lesen(&mut socket).await.is_empty());

        bus.an_alle_veroeffentlichen(crate::obs::bus::BusRahmen::luecke(4242));
        let gelesen = alles_lesen(&mut socket).await;
        assert_eq!(gelesen.len(), 1);
        assert_eq!(gelesen[0]["id"].as_i64(), Some(4242));
        assert_eq!(gelesen[0]["luecke"], true);
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_busluecke CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Derselbe Fehler auf dem Vorlauf-Weg, also ohne `?seit=`.
    ///
    /// Der Zweig war bisher ungetestet: der Nachbartest verbindet mit
    /// `?seit=77` und laeuft damit durch den Lesezeichen-Zweig. Hier gibt es
    /// keine Wiederaufsetzstelle, deshalb `id` 0.
    #[tokio::test(flavor = "multi_thread")]
    async fn db_fehler_im_vorlauf_meldet_eine_luecke_ohne_wiederaufsetzstelle() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_dbfehler_vorlauf").await;
        sqlx::query("DROP TABLE obs_dock_events")
            .execute(&pool)
            .await
            .expect("Tabelle entfernen");

        let adresse = server_starten(pool.clone()).await;
        let mut socket = verbinden(adresse, None).await;
        let gelesen = alles_lesen(&mut socket).await;

        assert_eq!(gelesen.len(), 1, "genau ein Lueckenhinweis: {gelesen:?}");
        assert_eq!(gelesen[0]["luecke"], true);
        assert_eq!(gelesen[0]["id"].as_i64(), Some(0));
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_dbfehler_vorlauf CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Ein Datenbankfehler im Nachlauf darf das Dock nicht in dem Glauben
    /// lassen, sein Verlauf sei vollstaendig.
    ///
    /// Aufbau: `twitch_partners` steht, `obs_dock_events` ist weg. Damit
    /// traegt die Kanalaufloesung, und genau die Nachlauf-Abfrage bricht.
    #[tokio::test(flavor = "multi_thread")]
    async fn db_fehler_im_nachlauf_meldet_eine_luecke() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_dbfehler").await;
        sqlx::query("DROP TABLE obs_dock_events")
            .execute(&pool)
            .await
            .expect("Tabelle entfernen");

        let adresse = server_starten(pool.clone()).await;
        let mut socket = verbinden(adresse, Some(77)).await;
        let gelesen = alles_lesen(&mut socket).await;

        assert_eq!(gelesen.len(), 1, "genau ein Lueckenhinweis: {gelesen:?}");
        assert_eq!(gelesen[0]["luecke"], true);
        assert_eq!(gelesen[0]["id"].as_i64(), Some(77));
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_dbfehler CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Nimmt einen aufgebauten Socket entgegen und reicht ihn an den Test
    /// durch, statt ihn selbst zu bedienen.
    ///
    /// Damit laesst sich [`nachlauf_senden`] gegen einen echten WebSocket
    /// fahren, ohne ein `Lagged` von aussen erzwingen zu muessen. Ein `Lagged`
    /// entsteht erst, wenn der Schreiber im TCP-Puffer haengt, und das ist von
    /// einem Test aus nicht verlaesslich zu treffen.
    async fn socket_abgeben(
        upgrade: WebSocketUpgrade,
        Extension(kanal): Extension<
            Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<WebSocket>>>>,
        >,
    ) -> Response {
        upgrade.on_upgrade(move |socket| async move {
            if let Some(sender) = kanal.lock().await.take() {
                let _ = sender.send(socket);
            }
        })
    }

    /// Aufholen nach `Lagged` ueber ein volles Rueckgriff-Fenster hinweg.
    ///
    /// Der Rechenfehler aus dem Review: das Lesebudget war
    /// NACHLAUF_DECKEL * NACHLAUF_RUNDEN = 1000 Zeilen, der Lesezeiger startet
    /// aber NACHZUG_RUECKGRIFF = 1000 Zeilen unter dem Anker. Fuellt der Kanal
    /// diese 1000 IDs selbst, und genau dann entsteht ein `Lagged`, kommt der
    /// Socket keine einzige Zeile vorwaerts. Danach greift der Aufgabe-Pfad und
    /// wirft die wirklich verpassten Zeilen per Lueckenrahmen weg, obwohl sie
    /// vollstaendig in `obs_dock_events` liegen.
    ///
    /// Aufbau ist die Rechnung selbst: Anker 1400, Untergrenze 1, Tabelle bis
    /// 1700. Der Socket muss die 300 Zeilen 1401..=1700 bekommen und keinen
    /// Lueckenrahmen.
    #[tokio::test(flavor = "multi_thread")]
    async fn aufholen_kommt_ueber_das_rueckgriff_fenster_hinaus() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_aufholen").await;
        let anker: i64 = 1400;
        let gesamt: i64 = 1700;
        assert!(
            anker - NACHZUG_RUECKGRIFF > 0,
            "der Rueckgriff muss echt unter den Anker reichen"
        );
        sqlx::query(
            "INSERT INTO obs_dock_events (channel_id, payload)
             SELECT $1, jsonb_build_object('typ','chat','id',lauf::text,'text','z' || lauf::text)
               FROM generate_series(1, $2) AS lauf",
        )
        .bind(KANAL)
        .bind(gesamt)
        .execute(&pool)
        .await
        .expect("Massen-Einfuegen");

        // Ein Socket, der mit ?seit=1 verbunden ist und inzwischen live bis
        // zum Anker mitgelesen hat. Genau die Lage vor einem `Lagged`.
        let mut buch = Auslieferung::neu(Some(1));
        for id in 2..=anker {
            buch.nachlauf(id);
        }
        assert_eq!(buch.anker(), anker);
        assert_eq!(buch.untergrenze(), 1);

        let (sender, empfaenger) = tokio::sync::oneshot::channel::<WebSocket>();
        let router = Router::new()
            .route("/roh", get(socket_abgeben))
            .layer(Extension(Arc::new(tokio::sync::Mutex::new(Some(sender)))));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Port");
        let adresse = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let (mut dock, _) = tokio_tungstenite::connect_async(format!("ws://{adresse}/roh"))
            .await
            .expect("Socket verbunden");
        let server_socket = empfaenger.await.expect("Socket abgegeben");
        let (mut schreiber, _leser) = server_socket.split();

        assert!(
            nachlauf_senden(
                &pool,
                KANAL,
                &mut buch,
                &mut schreiber,
                NachlaufArt::Aufholen
            )
            .await,
            "der Socket lebt noch"
        );

        let gelesen = alles_lesen(&mut dock).await;
        assert!(
            gelesen.iter().all(|wert| wert["luecke"].is_null()),
            "kein Lueckenrahmen, die Zeilen stehen in der Tabelle"
        );
        let ids: Vec<i64> = gelesen
            .iter()
            .filter_map(|wert| wert["id"].as_i64())
            .collect();
        assert_eq!(
            ids,
            (anker + 1..=gesamt).collect::<Vec<i64>>(),
            "genau die verpassten Zeilen, keine doppelt"
        );

        dock.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_aufholen CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Ein Dock ohne `?seit=` bekommt seinen Vorlauf und danach nichts, was
    /// aelter ist. Auch dann nicht, wenn der Bus mit dem vollen Rueckgriff
    /// nachzieht.
    ///
    /// Das ist der Folgefehler des Rueckgriffs: `Auslieferung::neu(None)` setzt
    /// die Untergrenze auf 0, im Merkfenster stehen nur die
    /// [`VORLAUF_OHNE_SEIT`] Zeilen des Vorlaufs, und ein Nachzug ab
    /// `wasserstand - NACHZUG_RUECKGRIFF` haette dem Overlay den ganzen
    /// Verlauf davor als frische Rahmen eingespielt.
    ///
    /// Gefahren wird der echte Weg: derselbe Bus wie der Socket, echter
    /// `luecke_nachziehen` gegen die echte Tabelle, echter WebSocket.
    #[tokio::test(flavor = "multi_thread")]
    async fn vorlauf_dock_bekommt_beim_nachzug_nichts_altes() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_vorlauf_grenze").await;
        // Deutlich mehr Zeilen als der Vorlauf traegt, damit unterhalb des
        // Vorlauf-Fensters wirklich etwas zum Wiederausspielen liegt.
        let gesamt = VORLAUF_OHNE_SEIT + 40;
        sqlx::query(
            "INSERT INTO obs_dock_events (channel_id, payload)
             SELECT $1, jsonb_build_object('typ','chat','id',lauf::text,'text','alt' || lauf::text)
               FROM generate_series(1, $2) AS lauf",
        )
        .bind(KANAL)
        .bind(gesamt)
        .execute(&pool)
        .await
        .expect("Massen-Einfuegen");

        let bus = ObsDockBus::neu(pool.clone());
        let router = Router::new()
            .route("/obs/ws", get(obs_ws_handler))
            .layer(Extension(Arc::clone(&bus)))
            .layer(Extension(tb_http_core::ExpectedToken(TOKEN.to_string())))
            .with_state(pool.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Port");
        let adresse = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let mut dock = verbinden(adresse, None).await;
        let vorlauf = alles_lesen(&mut dock).await;
        assert_eq!(vorlauf.len() as i64, VORLAUF_OHNE_SEIT);
        let aelteste_im_vorlauf = vorlauf[0]["id"].as_i64().expect("id im Rahmen");
        assert!(
            aelteste_im_vorlauf > 1,
            "unter dem Vorlauf muss Verlauf liegen"
        );

        // Der Bus zieht nach, als waere die Verbindung abgerissen. Sein
        // Rueckgriff reicht hier bis unter Zeile 1 zurueck.
        bus.nachzug_ab_stand_fuer_tests(&pool, gesamt)
            .await
            .expect("Nachzug");

        let danach = alles_lesen(&mut dock).await;
        assert!(
            danach.is_empty(),
            "kein alter Rahmen darf nachkommen, bekommen: {:?}",
            danach
                .iter()
                .filter_map(|wert| wert["id"].as_i64())
                .collect::<Vec<_>>()
        );

        // Und der Socket ist dabei nicht taub geworden: was wirklich neu ist,
        // geht weiter hinaus.
        let neu = ereignis_schreiben(&pool, "neu").await;
        bus.veroeffentlichen(
            KANAL,
            crate::obs::bus::BusRahmen::neu(neu, r#"{"typ":"chat","id":"neu","text":"neu"}"#),
        );
        assert_eq!(texte(&alles_lesen(&mut dock).await), vec!["neu"]);

        dock.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_vorlauf_grenze CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Der Fund, der ueber Leben und Tod des Features entscheidet: in der
    /// Produktion haengt `/obs/ws` unter `CompressionLayer`, `TraceLayer`, dem
    /// Audit-Middleware und den Security-Headern aus [`crate::build_router`].
    /// Ein 101-Upgrade hat keine `Content-Length`, und ob es durch diese Kette
    /// sauber durchgeht, sagt kein Test am nackten Router.
    ///
    /// Dieser hier oeffnet deshalb einen echten Socket gegen den echten
    /// `build_router`.
    ///
    /// # Was er belegt und was nicht
    ///
    /// Belegt: der Handshake kommt durch die Layer-Kette, die Auth traegt, und
    /// der Nachlauf geht ueber die Leitung. Der Nachlauf liest ueber
    /// `State(pool)` und damit ueber den Pool dieses Tests.
    ///
    /// Nicht belegt: der Live-Weg. `build_router` haengt den Prozess-Singleton
    /// [`ObsDockBus::gemeinsam`] an, und dessen Pool friert der erste Aufrufer
    /// im Testbinary ein, nicht dieser Test. Je nach Testreihenfolge horcht der
    /// Bus hier also auf einer fremden Datenbank und schreibt `warn!` in einer
    /// Reconnect-Schleife. Deshalb steht der Name auf `upgrade_und_nachlauf`
    /// und nicht auf `socket`; den Live-Weg belegen die Tests mit eigenem Bus
    /// ([`live_rahmen_traegt_die_id_am_echten_socket`],
    /// [`verkehrte_reihenfolge_erreicht_das_dock_am_echten_socket`]).
    #[tokio::test(flavor = "multi_thread")]
    async fn upgrade_und_nachlauf_gehen_durch_die_volle_layer_kette() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_volle_kette").await;
        for lauf in 1..=3 {
            ereignis_schreiben(&pool, &format!("k{lauf}")).await;
        }

        let router = crate::build_router(pool.clone(), TOKEN.to_string());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Port");
        let adresse = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        // Schon `connect_async` wuerde scheitern, wenn die Layer-Kette das
        // Upgrade verschluckt oder komprimiert.
        let mut socket = verbinden(adresse, None).await;
        let gelesen = alles_lesen(&mut socket).await;
        assert_eq!(texte(&gelesen), vec!["k1", "k2", "k3"]);
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_volle_kette CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Ein Dock, das weiter zurueckliegt als eine einzelne Abfrage hergibt,
    /// bekommt den Rest nachgezogen statt ihn stumm zu verlieren.
    #[tokio::test(flavor = "multi_thread")]
    async fn nachlauf_zieht_ueber_den_abfragedeckel_hinaus_nach() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_deckel").await;
        // Mehr als eine Abfrage traegt, aber weniger als alle Runden zusammen.
        let anzahl = NACHLAUF_DECKEL * 2 + 7;
        sqlx::query(
            "INSERT INTO obs_dock_events (channel_id, payload)
             SELECT $1, jsonb_build_object('typ','chat','id',lauf::text,'text',lauf::text)
               FROM generate_series(1, $2) AS lauf",
        )
        .bind(KANAL)
        .bind(anzahl)
        .execute(&pool)
        .await
        .expect("Massen-Einfuegen");

        let adresse = server_starten(pool.clone()).await;
        let mut socket = verbinden(adresse, Some(1)).await;
        let gelesen = alles_lesen(&mut socket).await;

        // Alles ab id 2, in einem Rutsch, ohne Lueckenhinweis.
        assert_eq!(gelesen.len() as i64, anzahl - 1);
        assert!(gelesen.iter().all(|wert| wert["luecke"].is_null()));
        let ids: Vec<i64> = gelesen
            .iter()
            .filter_map(|wert| wert["id"].as_i64())
            .collect();
        assert_eq!(ids.first(), Some(&2));
        assert_eq!(ids.last(), Some(&anzahl));
        // Streng aufsteigend, also keine Dublette.
        assert!(ids.windows(2).all(|paar| paar[0] < paar[1]));
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_deckel CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    /// Reicht selbst das Nachziehen nicht, sagt der Server es dem Dock, statt
    /// die Zeilen stillschweigend fallen zu lassen.
    #[tokio::test(flavor = "multi_thread")]
    async fn zu_grosser_nachlauf_meldet_die_luecke() {
        let dsn = dsn_oder_skip!();
        let pool = schema_pool(&dsn, "obs_ws_luecke").await;
        let anzahl = NACHLAUF_DECKEL * i64::from(NACHLAUF_RUNDEN) + 25;
        sqlx::query(
            "INSERT INTO obs_dock_events (channel_id, payload)
             SELECT $1, jsonb_build_object('typ','chat','id',lauf::text,'text',lauf::text)
               FROM generate_series(1, $2) AS lauf",
        )
        .bind(KANAL)
        .bind(anzahl)
        .execute(&pool)
        .await
        .expect("Massen-Einfuegen");

        let adresse = server_starten(pool.clone()).await;
        let mut socket = verbinden(adresse, Some(1)).await;
        let gelesen = alles_lesen(&mut socket).await;

        let luecken: Vec<&serde_json::Value> = gelesen
            .iter()
            .filter(|wert| wert["luecke"] == serde_json::Value::Bool(true))
            .collect();
        assert_eq!(luecken.len(), 1, "genau ein Lueckenhinweis");
        assert_eq!(luecken[0]["id"].as_i64(), Some(anzahl));
        // Davor die Zeilen, die noch in die Runden gepasst haben.
        assert_eq!(
            gelesen.len() as i64,
            NACHLAUF_DECKEL * i64::from(NACHLAUF_RUNDEN) + 1
        );
        socket.close(None).await.ok();
        sqlx::query("DROP SCHEMA obs_ws_luecke CASCADE")
            .execute(&pool)
            .await
            .ok();
    }
}

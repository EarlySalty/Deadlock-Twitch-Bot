//! Der Verteiler zwischen Postgres und den offenen Dock-Sockets.
//!
//! Aufbau (Plan Abschnitt 2.3): **ein** [`sqlx::postgres::PgListener`] je
//! Prozess horcht auf dem Kanal `obs_dock`. Jede Benachrichtigung traegt nur
//! `{"channel_id":"...","id":123}`; die eigentliche Nutzlast holt der Listener
//! per `id` aus `obs_dock_events` und schiebt sie unveraendert als Text in
//! einen [`tokio::sync::broadcast`]-Kanal je `channel_id`.
//!
//! Warum ein Listener und nicht einer je Socket: ein Dock laeuft stundenlang,
//! ein Streamer haelt bis zu drei Fenster offen. Ein Listener je Socket waere
//! eine eigene Postgres-Verbindung je Fenster.
//!
//! Die Kanaltabelle wird traege angelegt und wieder abgeraeumt, sobald der
//! letzte Empfaenger eines Kanals weg ist. Anlegen und Abonnieren passieren
//! unter derselben Sperre, sonst koennte ein gerade endender Socket den Kanal
//! wegraeumen, waehrend ein neuer ihn schon geholt, aber noch nicht abonniert
//! hat.
//!
//! Der Schreibpfad (Tabelle, Migration und `pg_notify`) gehoert zu Auftrag B.
//! Hier wird ausschliesslich gelesen.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use sqlx::postgres::PgListener;
use sqlx::PgPool;
use tokio::sync::{broadcast, oneshot};
use tracing::{debug, warn};

/// Postgres-Kanal, auf dem Auftrag B benachrichtigt.
pub const NOTIFY_KANAL: &str = "obs_dock";

/// Vorlauf ohne `?seit`: so viele Zeilen bekommt ein frisch verbundenes Dock.
pub const VORLAUF_OHNE_SEIT: i64 = 50;

/// Deckel fuer den Nachlauf mit `?seit=<id>` (Plan Abschnitt 2.3).
pub const NACHLAUF_DECKEL: i64 = 200;

/// Harte Obergrenze offener Sockets je Kanal. Beim Ueberschreiten wird der
/// aelteste Socket geschlossen, nicht der neue abgewiesen: ein OBS-Neustart
/// laesst regelmaessig Zombie-Sockets zurueck, und der Streamer soll dadurch
/// nicht ausgesperrt werden.
pub const MAX_SOCKETS_JE_PARTNER: usize = 6;

/// Puffertiefe je Kanal. Reicht fuer einen Chatschub, ohne dass ein langsamer
/// Socket sofort `Lagged` sieht.
const BROADCAST_TIEFE: usize = 256;

/// Deckel beim Nachziehen einer Luecke nach einem Listener-Neuaufbau.
const LUECKE_DECKEL: i64 = 500;

/// Kleinster und groesster Wiederholungsabstand des Listeners.
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_DECKEL: Duration = Duration::from_secs(30);

/// Sentinelwert fuer "Wasserstand noch nicht bestimmt".
const WASSERSTAND_UNBEKANNT: i64 = -1;

/// Ein Ereignis auf dem Bus.
///
/// `json` ist die Spalte `payload` als Text, genau so wie Auftrag B sie
/// geschrieben hat. Das Gateway formt daran nichts um; das Drahtformat ist
/// `tb_platform_core::PlatformEvent` und in dessen `tests/drahtformat.rs`
/// eingefroren.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusRahmen {
    /// `obs_dock_events.id`, monoton steigend.
    pub id: i64,
    /// `obs_dock_events.payload` als JSON-Text.
    pub json: Arc<str>,
}

impl BusRahmen {
    /// Baut einen Rahmen.
    pub fn neu(id: i64, json: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            json: json.into(),
        }
    }
}

/// Grund, aus dem der Server einen Socket schliesst.
///
/// Die Codes liegen im privaten Bereich 4000-4999 der WebSocket-Spezifikation.
/// Der Grundtext ist Teil des Vertrags mit dem Dock: es entscheidet daran, ob
/// es neu verbindet (`zu_viele_verbindungen`, `leerlauf`) oder den Nutzer zum
/// Login schickt (`session_abgelaufen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchliessGrund {
    /// Der Streamer hat mehr Sockets offen als [`MAX_SOCKETS_JE_PARTNER`].
    ZuVieleVerbindungen,
    /// Die Dashboard-Session ist abgelaufen oder wurde entzogen.
    SessionAbgelaufen,
    /// Der Socket hat zu lange kein Lebenszeichen mehr geschickt.
    Leerlauf,
}

impl SchliessGrund {
    /// Grundtext im Close-Frame.
    pub const fn text(self) -> &'static str {
        match self {
            Self::ZuVieleVerbindungen => "zu_viele_verbindungen",
            Self::SessionAbgelaufen => "session_abgelaufen",
            Self::Leerlauf => "leerlauf",
        }
    }

    /// Close-Code im Close-Frame.
    pub const fn code(self) -> u16 {
        match self {
            Self::ZuVieleVerbindungen => 4001,
            Self::SessionAbgelaufen => 4002,
            Self::Leerlauf => 4003,
        }
    }
}

/// Ein angemeldeter Socket in der Kanaltabelle.
struct SocketEintrag {
    nummer: u64,
    abbruch: oneshot::Sender<SchliessGrund>,
}

/// Zustand eines Kanals: der Verteiler und die daran haengenden Sockets.
struct KanalZustand {
    sender: broadcast::Sender<BusRahmen>,
    sockets: Vec<SocketEintrag>,
}

/// Was ein Socket beim Anmelden bekommt.
pub struct Anmeldung {
    /// Solange dieser Waechter lebt, gilt der Socket als angemeldet.
    pub waechter: SocketWaechter,
    /// Live-Ereignisse des Kanals.
    pub rahmen: broadcast::Receiver<BusRahmen>,
    /// Wird erfuellt, wenn der Server den Socket schliessen will.
    pub abbruch: oneshot::Receiver<SchliessGrund>,
}

/// Meldet den Socket beim Fallenlassen wieder ab und raeumt leere Kanaele auf.
pub struct SocketWaechter {
    bus: Arc<ObsDockBus>,
    channel_id: String,
    nummer: u64,
}

impl Drop for SocketWaechter {
    fn drop(&mut self) {
        self.bus.abmelden(&self.channel_id, self.nummer);
    }
}

/// Der Prozess-Verteiler.
pub struct ObsDockBus {
    /// Nur der Listener nutzt diesen Pool. `None` in Tests ohne Datenbank.
    pool: Option<PgPool>,
    kanaele: Mutex<HashMap<String, KanalZustand>>,
    naechste_nummer: AtomicU64,
    /// Hoechste `id`, die der Listener schon verteilt hat. Grundlage fuer das
    /// Nachziehen der Luecke nach einem Verbindungsabriss.
    wasserstand: AtomicI64,
    listener_gestartet: AtomicBool,
}

/// Der eine Bus des Prozesses.
static GEMEINSAMER_BUS: OnceLock<Arc<ObsDockBus>> = OnceLock::new();

impl ObsDockBus {
    /// Der Prozess-Singleton. Der erste Aufruf bestimmt den Pool des
    /// Listeners; in der Produktion ist das der Pool aus `build_router`.
    pub fn gemeinsam(pool: PgPool) -> Arc<Self> {
        GEMEINSAMER_BUS.get_or_init(|| Self::neu(pool)).clone()
    }

    /// Ein eigener Bus mit Datenbank (Tests, Sonderfaelle).
    pub fn neu(pool: PgPool) -> Arc<Self> {
        Self::bauen(Some(pool))
    }

    /// Ein Bus ohne Datenbank: der Listener startet nie, Ereignisse kommen
    /// ausschliesslich ueber [`ObsDockBus::veroeffentlichen`]. Fuer Tests der
    /// Verteillogik.
    pub fn ohne_datenbank() -> Arc<Self> {
        Self::bauen(None)
    }

    fn bauen(pool: Option<PgPool>) -> Arc<Self> {
        Arc::new(Self {
            pool,
            kanaele: Mutex::new(HashMap::new()),
            naechste_nummer: AtomicU64::new(1),
            wasserstand: AtomicI64::new(WASSERSTAND_UNBEKANNT),
            listener_gestartet: AtomicBool::new(false),
        })
    }

    /// Meldet einen Socket an und abonniert den Kanal in einem Zug.
    ///
    /// Beides unter derselben Sperre, damit [`ObsDockBus::abmelden`] den Kanal
    /// nicht zwischen Holen und Abonnieren wegraeumen kann.
    ///
    /// Sind danach mehr als [`MAX_SOCKETS_JE_PARTNER`] Sockets offen, wird der
    /// aelteste mit [`SchliessGrund::ZuVieleVerbindungen`] abgebrochen.
    pub fn anmelden(self: &Arc<Self>, channel_id: &str) -> Anmeldung {
        let nummer = self.naechste_nummer.fetch_add(1, Ordering::Relaxed);
        let (abbruch_tx, abbruch_rx) = oneshot::channel();

        let (rahmen, zu_schliessen) = {
            let mut kanaele = self.kanaele.lock().expect("obs-bus-sperre vergiftet");
            let zustand = kanaele
                .entry(channel_id.to_string())
                .or_insert_with(|| KanalZustand {
                    sender: broadcast::channel(BROADCAST_TIEFE).0,
                    sockets: Vec::new(),
                });
            let rahmen = zustand.sender.subscribe();
            zustand.sockets.push(SocketEintrag {
                nummer,
                abbruch: abbruch_tx,
            });

            let mut zu_schliessen = Vec::new();
            while zustand.sockets.len() > MAX_SOCKETS_JE_PARTNER {
                zu_schliessen.push(zustand.sockets.remove(0));
            }
            (rahmen, zu_schliessen)
        };

        for eintrag in zu_schliessen {
            debug!(
                channel_id,
                socket = eintrag.nummer,
                "OBS-Dock: aeltesten Socket wegen Ueberzahl geschlossen"
            );
            let _ = eintrag.abbruch.send(SchliessGrund::ZuVieleVerbindungen);
        }

        Anmeldung {
            waechter: SocketWaechter {
                bus: Arc::clone(self),
                channel_id: channel_id.to_string(),
                nummer,
            },
            rahmen,
            abbruch: abbruch_rx,
        }
    }

    /// Nimmt einen Socket aus der Tabelle und raeumt den Kanal ab, wenn er
    /// danach niemanden mehr traegt.
    ///
    /// Massstab ist die Socketliste, nicht `receiver_count`: ein Empfaenger
    /// entsteht ausschliesslich in [`ObsDockBus::anmelden`] und liegt dort im
    /// selben [`Anmeldung`]-Buendel wie der Waechter. `receiver_count` waere
    /// beim Abmelden noch 1, weil der Waechter innerhalb des Buendels vor dem
    /// Empfaenger faellt; der Kanal bliebe dann fuer immer stehen.
    fn abmelden(&self, channel_id: &str, nummer: u64) {
        let mut kanaele = self.kanaele.lock().expect("obs-bus-sperre vergiftet");
        let Some(zustand) = kanaele.get_mut(channel_id) else {
            return;
        };
        zustand.sockets.retain(|eintrag| eintrag.nummer != nummer);
        if zustand.sockets.is_empty() {
            kanaele.remove(channel_id);
        }
    }

    /// Verteilt einen Rahmen an alle Empfaenger eines Kanals.
    ///
    /// Gibt die Zahl der erreichten Empfaenger zurueck. Kein Empfaenger ist
    /// kein Fehler: dann horcht gerade niemand auf diesen Kanal.
    ///
    /// Der Wasserstand wird hier bewusst **nicht** angefasst; darum kuemmern
    /// sich die beiden Aufrufer im Listener, damit der Test die Verteillogik
    /// ohne Datenbank fahren kann.
    pub fn veroeffentlichen(&self, channel_id: &str, rahmen: BusRahmen) -> usize {
        let kanaele = self.kanaele.lock().expect("obs-bus-sperre vergiftet");
        match kanaele.get(channel_id) {
            Some(zustand) => zustand.sender.send(rahmen).unwrap_or(0),
            None => 0,
        }
    }

    /// Zahl der gerade gefuehrten Kanaele (fuer Tests und Diagnose).
    pub fn kanal_anzahl(&self) -> usize {
        self.kanaele.lock().expect("obs-bus-sperre vergiftet").len()
    }

    /// Zahl der angemeldeten Sockets eines Kanals (fuer Tests und Diagnose).
    pub fn socket_anzahl(&self, channel_id: &str) -> usize {
        self.kanaele
            .lock()
            .expect("obs-bus-sperre vergiftet")
            .get(channel_id)
            .map(|zustand| zustand.sockets.len())
            .unwrap_or(0)
    }

    /// Startet den Listener beim ersten Socket, danach nie wieder.
    ///
    /// Traege statt beim Router-Bau, damit ein Prozess ohne offenes Dock keine
    /// Postgres-Verbindung dauerhaft belegt und ein Testlauf keine
    /// Reconnect-Schleife gegen eine fehlende Datenbank faehrt.
    pub fn listener_sicherstellen(self: &Arc<Self>) {
        let Some(pool) = self.pool.clone() else {
            return;
        };
        if self
            .listener_gestartet
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let bus = Arc::clone(self);
        tokio::spawn(async move { bus.listener_schleife(pool).await });
    }

    /// Horcht auf `obs_dock`, mit Wiederaufbau und Backoff.
    async fn listener_schleife(self: Arc<Self>, pool: PgPool) {
        let mut backoff = BACKOFF_START;
        loop {
            match self.horchen(&pool).await {
                Ok(()) => backoff = BACKOFF_START,
                Err(fehler) => {
                    warn!(%fehler, "OBS-Dock-Listener abgebrochen, neuer Versuch folgt");
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(BACKOFF_DECKEL);
        }
    }

    /// Ein Durchlauf: verbinden, Wasserstand setzen, Luecke schliessen,
    /// horchen bis die Verbindung abreisst.
    async fn horchen(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen(NOTIFY_KANAL).await?;

        if self.wasserstand.load(Ordering::SeqCst) == WASSERSTAND_UNBEKANNT {
            // Erster Start: alles, was vor dem Prozess passiert ist, gilt als
            // erledigt. Den Vorlauf holt sich jeder Socket selbst aus der
            // Tabelle, dafuer ist der Wasserstand nicht zustaendig.
            let hoechste: Option<i64> =
                sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM obs_dock_events")
                    .fetch_optional(pool)
                    .await?
                    .flatten();
            self.wasserstand
                .store(hoechste.unwrap_or(0), Ordering::SeqCst);
        } else {
            // Wiederaufbau: was waehrend des Abrisses geschrieben wurde,
            // nachziehen.
            self.luecke_nachziehen(pool).await?;
        }

        loop {
            match listener.try_recv().await? {
                Some(benachrichtigung) => {
                    self.benachrichtigung_verarbeiten(pool, benachrichtigung.payload())
                        .await?;
                }
                // `None` heisst: sqlx hat die Verbindung im Hintergrund neu
                // aufgebaut. Alles dazwischen ist verloren und wird aus der
                // Tabelle nachgezogen.
                None => self.luecke_nachziehen(pool).await?,
            }
        }
    }

    /// Verarbeitet eine `pg_notify`-Nutzlast `{"channel_id":"...","id":123}`.
    async fn benachrichtigung_verarbeiten(
        &self,
        pool: &PgPool,
        nutzlast: &str,
    ) -> Result<(), sqlx::Error> {
        let Some(hinweis) = NotifyHinweis::lesen(nutzlast) else {
            warn!(nutzlast, "OBS-Dock: unlesbare NOTIFY-Nutzlast verworfen");
            return Ok(());
        };

        // Auch ohne Empfaenger den Wasserstand mitziehen, sonst zieht ein
        // spaeterer Wiederaufbau eine Luecke nach, die niemanden interessiert.
        self.wasserstand.fetch_max(hinweis.id, Ordering::SeqCst);

        if !self.hat_empfaenger(&hinweis.channel_id) {
            return Ok(());
        }

        let json: Option<String> =
            sqlx::query_scalar("SELECT payload::text FROM obs_dock_events WHERE id = $1")
                .bind(hinweis.id)
                .fetch_optional(pool)
                .await?;
        let Some(json) = json else {
            // Retention hat die Zeile schon geloescht. Nicht dramatisch.
            debug!(id = hinweis.id, "OBS-Dock: Zeile zur NOTIFY nicht gefunden");
            return Ok(());
        };
        self.veroeffentlichen(&hinweis.channel_id, BusRahmen::neu(hinweis.id, json));
        Ok(())
    }

    /// Zieht alles nach, was seit dem Wasserstand geschrieben wurde.
    async fn luecke_nachziehen(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        let seit = self.wasserstand.load(Ordering::SeqCst).max(0);
        let zeilen: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT id, channel_id, payload::text
               FROM obs_dock_events
              WHERE id > $1
              ORDER BY id
              LIMIT $2",
        )
        .bind(seit)
        .bind(LUECKE_DECKEL)
        .fetch_all(pool)
        .await?;

        for (id, channel_id, json) in zeilen {
            self.wasserstand.fetch_max(id, Ordering::SeqCst);
            self.veroeffentlichen(&channel_id, BusRahmen::neu(id, json));
        }
        Ok(())
    }

    fn hat_empfaenger(&self, channel_id: &str) -> bool {
        self.kanaele
            .lock()
            .expect("obs-bus-sperre vergiftet")
            .get(channel_id)
            .is_some_and(|zustand| !zustand.sockets.is_empty())
    }
}

/// Die entpackte `pg_notify`-Nutzlast.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NotifyHinweis {
    pub(crate) channel_id: String,
    pub(crate) id: i64,
}

impl NotifyHinweis {
    /// Liest `{"channel_id":"...","id":123}`. Alles andere ergibt `None`.
    pub(crate) fn lesen(nutzlast: &str) -> Option<Self> {
        let wert: serde_json::Value = serde_json::from_str(nutzlast).ok()?;
        let channel_id = wert.get("channel_id")?.as_str()?.trim();
        let id = wert.get("id")?.as_i64()?;
        if channel_id.is_empty() || id <= 0 {
            return None;
        }
        Some(Self {
            channel_id: channel_id.to_string(),
            id,
        })
    }
}

/// Buchfuehrung eines einzelnen Sockets ueber das, was er schon gesendet hat.
///
/// Der Uebergang von Nachlauf auf Live ist die einzige Stelle, an der eine
/// Dublette oder eine Luecke entstehen kann. Deshalb abonniert der Socket den
/// Kanal **vor** dem Lesen des Nachlaufs (Live-Rahmen sammeln sich solange im
/// Puffer) und verwirft danach jeden Live-Rahmen, dessen `id` der Nachlauf
/// schon abgedeckt hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Auslieferung {
    letzte_id: i64,
}

impl Auslieferung {
    /// Startet die Buchfuehrung. `seit` ist der `?seit=<id>`-Wert des Docks:
    /// alles bis einschliesslich dieser `id` hat das Dock schon gesehen.
    pub fn neu(seit: Option<i64>) -> Self {
        Self {
            letzte_id: seit.unwrap_or(0).max(0),
        }
    }

    /// Hoechste bereits ausgelieferte `id`.
    pub fn letzte_id(self) -> i64 {
        self.letzte_id
    }

    /// Bucht eine Zeile aus dem Nachlauf. Die Reihenfolge kommt aus der
    /// `ORDER BY id`-Abfrage, deshalb wird hier nur der Stand mitgezogen.
    pub fn nachlauf(&mut self, id: i64) {
        self.letzte_id = self.letzte_id.max(id);
    }

    /// Entscheidet ueber einen Live-Rahmen: `true` heisst senden.
    pub fn live(&mut self, id: i64) -> bool {
        if id <= self.letzte_id {
            return false;
        }
        self.letzte_id = id;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rahmen(id: i64) -> BusRahmen {
        BusRahmen::neu(id, format!(r#"{{"typ":"chat","id":{id}}}"#))
    }

    #[test]
    fn schliessgruende_sind_der_vertrag_mit_dem_dock() {
        assert_eq!(SchliessGrund::ZuVieleVerbindungen.code(), 4001);
        assert_eq!(
            SchliessGrund::ZuVieleVerbindungen.text(),
            "zu_viele_verbindungen"
        );
        assert_eq!(SchliessGrund::SessionAbgelaufen.code(), 4002);
        assert_eq!(
            SchliessGrund::SessionAbgelaufen.text(),
            "session_abgelaufen"
        );
        assert_eq!(SchliessGrund::Leerlauf.code(), 4003);
        assert_eq!(SchliessGrund::Leerlauf.text(), "leerlauf");
    }

    #[test]
    fn notify_nutzlast_wird_gelesen() {
        let hinweis = NotifyHinweis::lesen(r#"{"channel_id":"12345","id":7}"#).unwrap();
        assert_eq!(hinweis.channel_id, "12345");
        assert_eq!(hinweis.id, 7);
    }

    #[test]
    fn kaputte_notify_nutzlast_wird_verworfen() {
        assert!(NotifyHinweis::lesen("kein json").is_none());
        assert!(NotifyHinweis::lesen(r#"{"id":7}"#).is_none());
        assert!(NotifyHinweis::lesen(r#"{"channel_id":"12345"}"#).is_none());
        assert!(NotifyHinweis::lesen(r#"{"channel_id":"","id":7}"#).is_none());
        assert!(NotifyHinweis::lesen(r#"{"channel_id":"12345","id":0}"#).is_none());
    }

    /// Beweisziel 1: zwei Empfaenger auf demselben Kanal bekommen dasselbe
    /// Ereignis, ein Empfaenger auf einem anderen Kanal bekommt es nicht.
    #[tokio::test]
    async fn fanout_erreicht_beide_empfaenger_desselben_kanals() {
        let bus = ObsDockBus::ohne_datenbank();
        let mut a = bus.anmelden("kanal-a");
        let mut b = bus.anmelden("kanal-a");
        let mut fremd = bus.anmelden("kanal-b");

        assert_eq!(bus.veroeffentlichen("kanal-a", rahmen(1)), 2);

        assert_eq!(a.rahmen.recv().await.unwrap(), rahmen(1));
        assert_eq!(b.rahmen.recv().await.unwrap(), rahmen(1));
        assert!(matches!(
            fremd.rahmen.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        // Und andersherum: der fremde Kanal erreicht die beiden nicht.
        assert_eq!(bus.veroeffentlichen("kanal-b", rahmen(2)), 1);
        assert_eq!(fremd.rahmen.recv().await.unwrap(), rahmen(2));
        assert!(matches!(
            a.rahmen.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn kanal_wird_abgeraeumt_wenn_der_letzte_socket_geht() {
        let bus = ObsDockBus::ohne_datenbank();
        let anmeldung = bus.anmelden("kanal-a");
        assert_eq!(bus.kanal_anzahl(), 1);
        assert_eq!(bus.socket_anzahl("kanal-a"), 1);
        drop(anmeldung);
        assert_eq!(bus.kanal_anzahl(), 0);
        assert_eq!(bus.veroeffentlichen("kanal-a", rahmen(1)), 0);
    }

    #[test]
    fn kanal_bleibt_solange_ein_socket_haengt() {
        let bus = ObsDockBus::ohne_datenbank();
        let bleibt = bus.anmelden("kanal-a");
        let geht = bus.anmelden("kanal-a");
        drop(geht);
        assert_eq!(bus.kanal_anzahl(), 1);
        assert_eq!(bus.socket_anzahl("kanal-a"), 1);
        drop(bleibt);
        assert_eq!(bus.kanal_anzahl(), 0);
    }

    #[tokio::test]
    async fn siebter_socket_schliesst_den_aeltesten() {
        let bus = ObsDockBus::ohne_datenbank();
        let mut offen: Vec<Anmeldung> = (0..MAX_SOCKETS_JE_PARTNER)
            .map(|_| bus.anmelden("kanal-a"))
            .collect();
        assert_eq!(bus.socket_anzahl("kanal-a"), MAX_SOCKETS_JE_PARTNER);

        let _neu = bus.anmelden("kanal-a");
        assert_eq!(bus.socket_anzahl("kanal-a"), MAX_SOCKETS_JE_PARTNER);

        let aeltester = offen.remove(0);
        assert_eq!(
            aeltester.abbruch.await.unwrap(),
            SchliessGrund::ZuVieleVerbindungen
        );
        // Der zweitaelteste bleibt unbehelligt.
        assert!(offen[0].abbruch.try_recv().is_err());
    }

    #[test]
    fn auslieferung_ohne_seit_liefert_alles() {
        let mut buch = Auslieferung::neu(None);
        assert!(buch.live(1));
        assert!(buch.live(2));
        assert_eq!(buch.letzte_id(), 2);
    }

    /// Beweisziel 2: ein Reconnect mit `seit=<letzte id>` erzeugt weder eine
    /// Luecke noch eine Dublette.
    ///
    /// Aufbau wie im Socket: erst abonnieren, dann den Nachlauf aus der
    /// Tabelle senden, dann den Puffer der Live-Rahmen leeren. Der Puffer
    /// enthaelt hier absichtlich Ueberschneidung (6 und 7 stehen sowohl im
    /// Nachlauf als auch live an).
    #[tokio::test]
    async fn reconnect_mit_seit_ohne_luecke_und_ohne_dublette() {
        let bus = ObsDockBus::ohne_datenbank();
        let mut anmeldung = bus.anmelden("kanal-a");

        // Waehrend das Dock den Nachlauf aus der Tabelle holt, laufen 6..=10
        // live in den Puffer.
        for id in 6..=10 {
            bus.veroeffentlichen("kanal-a", rahmen(id));
        }

        // Das Dock war bis 5 gekommen und fragt mit ?seit=5 nach.
        let mut buch = Auslieferung::neu(Some(5));
        let mut gesendet: Vec<i64> = Vec::new();

        // Nachlauf aus der Tabelle: 6 und 7 lagen beim Lesen schon vor.
        for id in [6, 7] {
            buch.nachlauf(id);
            gesendet.push(id);
        }

        // Danach der Live-Puffer.
        while let Ok(rahmen) = anmeldung.rahmen.try_recv() {
            if buch.live(rahmen.id) {
                gesendet.push(rahmen.id);
            }
        }

        assert_eq!(gesendet, vec![6, 7, 8, 9, 10]);
        assert_eq!(buch.letzte_id(), 10);
    }

    #[tokio::test]
    async fn reconnect_ohne_ueberschneidung_verliert_nichts() {
        let bus = ObsDockBus::ohne_datenbank();
        let mut anmeldung = bus.anmelden("kanal-a");
        for id in 3..=4 {
            bus.veroeffentlichen("kanal-a", rahmen(id));
        }

        let mut buch = Auslieferung::neu(Some(2));
        let mut gesendet: Vec<i64> = Vec::new();
        // Der Nachlauf war leer, weil die Zeilen erst nach der Abfrage kamen.
        while let Ok(rahmen) = anmeldung.rahmen.try_recv() {
            if buch.live(rahmen.id) {
                gesendet.push(rahmen.id);
            }
        }
        assert_eq!(gesendet, vec![3, 4]);
    }

    #[test]
    fn auslieferung_verwirft_alte_ids() {
        let mut buch = Auslieferung::neu(Some(10));
        assert!(!buch.live(9));
        assert!(!buch.live(10));
        assert!(buch.live(11));
        assert_eq!(buch.letzte_id(), 11);
    }

    #[test]
    fn negatives_seit_wird_auf_null_geklemmt() {
        let buch = Auslieferung::neu(Some(-5));
        assert_eq!(buch.letzte_id(), 0);
    }
}

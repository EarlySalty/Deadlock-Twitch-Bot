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
//!
//! # `id`-Reihenfolge ist nicht Sichtbarkeitsreihenfolge
//!
//! `obs_dock_events.id` ist ein `BIGSERIAL`. Die Nummer wird beim `INSERT`
//! gezogen, sichtbar wird die Zeile aber erst beim `COMMIT`, und der Schreiber
//! (`PgObsDockSink::write` in `bin/tb-bot/src/obs_dock.rs`) setzt `INSERT` und
//! `pg_notify` als zwei Anweisungen auf zwei Poolverbindungen ab. Zwei
//! nebenlaeufige EventSub-Notifications koennen deshalb als 101 vor 100
//! ankommen und auch in dieser Reihenfolge sichtbar werden.
//!
//! Daraus folgt die Bauregel dieses Moduls: **kein monotoner Wasserstand als
//! Filter.** Der Bus liefert bewusst *mindestens einmal* (siehe
//! [`NACHZUG_RUECKGRIFF`]), und die Entdopplung sitzt allein in
//! [`Auslieferung`], die sich die zuletzt gesendeten `id` merkt statt nur die
//! hoechste. Ein Wasserstand darf nur noch bestimmen, wo ein Nachzug *anfaengt*
//! zu lesen, nie was er wegwirft.

use std::collections::{HashMap, HashSet, VecDeque};
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

/// Zeilen je Runde beim Nachziehen einer Luecke nach einem Listener-Neuaufbau.
///
/// Das ist die Groesse eines einzelnen `SELECT`, **nicht** die Obergrenze der
/// Luecke: [`ObsDockBus::luecke_nachziehen`] faehrt so viele Runden, bis die
/// Tabelle leergelesen ist, hoechstens aber [`LUECKE_RUNDEN_DECKEL`] Stueck.
const LUECKE_DECKEL: i64 = 500;

/// Wie viele Runden das Nachziehen hoechstens faehrt. Wer danach immer noch
/// hinterherhaengt, hat mehr verpasst, als der Nachlaufpuffer ueberhaupt
/// vorhaelt (die Tabelle behaelt 15 Minuten); dann wird der Rest bewusst
/// uebersprungen, aber mit einem `warn!` und nicht stumm.
const LUECKE_RUNDEN_DECKEL: u32 = 20;

/// Kleinster und groesster Wiederholungsabstand des Listeners.
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_DECKEL: Duration = Duration::from_secs(30);

/// Ab dieser Laufzeit gilt ein Listener-Durchlauf als geglueckt, und der
/// Backoff faengt beim naechsten Abriss wieder von vorn an.
///
/// [`ObsDockBus::horchen`] kehrt ausschliesslich mit einem Fehler zurueck, ein
/// "Ok heisst geglueckt" gibt es dort also nicht. Massstab ist deshalb die
/// Standzeit: wer eine Minute lang gehorcht hat, hatte eine gesunde
/// Verbindung, und ein spaeterer Abriss darf nicht mit dem Backoff der letzten
/// Stoerung bestraft werden.
const STABIL_AB: Duration = Duration::from_secs(60);

/// Sentinelwert fuer "Wasserstand noch nicht bestimmt".
const WASSERSTAND_UNBEKANNT: i64 = -1;

/// Wie weit **unter** den Wasserstand ein Nachzug zurueckgreift.
///
/// Der Wasserstand ist die hoechste schon verteilte `id`, und weil `id` nicht
/// in Sichtbarkeitsreihenfolge vergeben wird (siehe Modulkopf), kann darunter
/// noch eine Zeile auftauchen, die nie verteilt wurde. Ein `WHERE id > stand`
/// wuerde sie fuer immer ueberspringen. Der Nachzug liest deshalb ein Stueck
/// unter den Stand zurueck und verteilt bewusst Dubletten; jeder Socket wirft
/// sie ueber [`Auslieferung`] wieder weg.
///
/// Der Wert deckt zwei Groessen ab: die Zeilen, die waehrend eines
/// Verbindungsabrisses geschrieben wurden, und die Spanne, ueber die zwei
/// nebenlaeufige Schreiber ihre `id` verkehrt herum sichtbar machen koennen.
/// Letztere ist eine Handvoll, 1000 ist grosszuegig und kostet einen
/// Index-Scan.
pub const NACHZUG_RUECKGRIFF: i64 = 1000;

/// Ein Ereignis auf dem Bus.
///
/// `json` ist die Spalte `payload` als Text, genau so wie Auftrag B sie
/// geschrieben hat: ein `tb_platform_core::PlatformEvent`, eingefroren in
/// dessen `tests/drahtformat.rs`. Am Ereignis selbst formt niemand etwas um.
///
/// Die `id` bleibt nicht serverintern: der Socket legt sie beim Senden in eine
/// Huelle um das Ereignis (`{"id":123,"ereignis":{...}}`), sonst wuesste ein
/// Dock nach einem Neustart nicht, wo es stand. Siehe [`crate::obs::ws`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusRahmen {
    /// `obs_dock_events.id`. Steigend vergeben, aber **nicht** in dieser
    /// Reihenfolge sichtbar; siehe Modulkopf.
    pub id: i64,
    /// `obs_dock_events.payload` als JSON-Text.
    ///
    /// `None` heisst Lueckenhinweis: der Bus hat bis einschliesslich `id`
    /// Zeilen uebersprungen und kann sie nicht mehr nachliefern. Der Socket
    /// macht daraus den `{"id":..,"luecke":true}`-Rahmen ans Dock.
    pub json: Option<Arc<str>>,
}

impl BusRahmen {
    /// Baut einen Rahmen mit Nutzlast.
    pub fn neu(id: i64, json: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            json: Some(json.into()),
        }
    }

    /// Baut einen Lueckenhinweis bis einschliesslich `bis`.
    pub fn luecke(bis: i64) -> Self {
        Self {
            id: bis,
            json: None,
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
    /// Die Dashboard-Session traegt nicht mehr.
    ///
    /// Was der Socket dabei wirklich sieht, steht in
    /// [`crate::obs::ws`]: den lokalen Sitzungsspiegel. Ein zentral entzogener
    /// Admin-Zugang faellt erst mit dem lokalen Ablauf auf, nicht sofort.
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

    /// Verteilt einen Rahmen an **jeden** gefuehrten Kanal.
    ///
    /// Nur fuer den Lueckenhinweis aus [`ObsDockBus::luecke_nachziehen`]: wenn
    /// der Bus Zeilen ueberspringt, weiss er nicht, zu welchen Kanaelen sie
    /// gehoerten, also muss es jedes offene Dock erfahren. Gibt die Zahl der
    /// erreichten Empfaenger zurueck.
    pub fn an_alle_veroeffentlichen(&self, rahmen: BusRahmen) -> usize {
        let kanaele = self.kanaele.lock().expect("obs-bus-sperre vergiftet");
        kanaele
            .values()
            .map(|zustand| zustand.sender.send(rahmen.clone()).unwrap_or(0))
            .sum()
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
    ///
    /// [`ObsDockBus::horchen`] kann nur mit einem Fehler zurueckkommen, sein
    /// Erfolgsfall ist unbewohnt. Zurueckgesetzt wird der Backoff deshalb an
    /// der Standzeit ([`STABIL_AB`]) und nicht an einem `Ok`, das es nie gibt:
    /// sonst waechst er ueber die Prozesslaufzeit monoton bis
    /// [`BACKOFF_DECKEL`] und ein Abriss nach acht ruhigen Stunden kostet
    /// 30 Sekunden Stille.
    async fn listener_schleife(self: Arc<Self>, pool: PgPool) {
        let mut backoff = BACKOFF_START;
        loop {
            let start = tokio::time::Instant::now();
            let Err(fehler) = self.horchen(&pool).await;
            let stand = start.elapsed();
            warn!(
                %fehler,
                stand_s = stand.as_secs(),
                "OBS-Dock-Listener abgebrochen, neuer Versuch folgt"
            );
            if stand >= STABIL_AB {
                backoff = BACKOFF_START;
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(BACKOFF_DECKEL);
        }
    }

    /// Ein Durchlauf: verbinden, Wasserstand setzen, Luecke schliessen,
    /// horchen bis die Verbindung abreisst.
    ///
    /// Der Rueckgabetyp sagt es ausdruecklich: hier kommt nur ein Fehler
    /// heraus, nie ein regulaeres Ende.
    async fn horchen(&self, pool: &PgPool) -> Result<std::convert::Infallible, sqlx::Error> {
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

        if !self.hat_empfaenger(&hinweis.channel_id) {
            // Ohne Empfaenger ist nichts zu holen und nichts zu verlieren, also
            // darf der Wasserstand sofort mit: sonst zieht ein spaeterer
            // Wiederaufbau eine Luecke nach, die niemanden interessiert.
            //
            // Dass der Stand damit ueber eine kleinere, noch nicht gemeldete
            // `id` springt, ist verkraftet: der Nachzug liest
            // [`NACHZUG_RUECKGRIFF`] Zeilen unter den Stand zurueck.
            self.wasserstand.fetch_max(hinweis.id, Ordering::SeqCst);
            return Ok(());
        }

        let json: Option<String> =
            sqlx::query_scalar("SELECT payload::text FROM obs_dock_events WHERE id = $1")
                .bind(hinweis.id)
                .fetch_optional(pool)
                .await?;
        // Erst ab hier den Wasserstand mitziehen. Schlaegt der SELECT oben
        // fehl, kehrt die Funktion ueber `?` zurueck, der Listener baut neu auf
        // und zieht genau diese Zeile ueber `luecke_nachziehen` nach. Wuerde
        // der Wasserstand schon vor dem SELECT stehen, waere sie uebersprungen.
        self.wasserstand.fetch_max(hinweis.id, Ordering::SeqCst);
        let Some(json) = json else {
            // Retention hat die Zeile schon geloescht. Nicht dramatisch.
            debug!(id = hinweis.id, "OBS-Dock: Zeile zur NOTIFY nicht gefunden");
            return Ok(());
        };
        self.veroeffentlichen(&hinweis.channel_id, BusRahmen::neu(hinweis.id, json));
        Ok(())
    }

    /// Zieht alles nach, was seit dem Wasserstand geschrieben wurde.
    ///
    /// In Haeppchen zu [`LUECKE_DECKEL`] Zeilen, aber so lange, bis die Tabelle
    /// leergelesen ist. Ein einzelner gekappter `SELECT` waere auf einer
    /// Instanz mit vielen Partnern in Sekunden voll, und alles darueber waere
    /// dauerhaft weg, ohne dass es irgendwo auffaellt.
    ///
    /// Nur wenn selbst [`LUECKE_RUNDEN_DECKEL`] Runden nicht reichen, wird der
    /// Rest uebersprungen; dann geht ein Lueckenhinweis an **alle** offenen
    /// Docks, statt dass die Zeilen nur im Serverlog verschwinden.
    ///
    /// Angefangen wird [`NACHZUG_RUECKGRIFF`] Zeilen **unter** dem
    /// Wasserstand, nicht darueber: der Stand kann ueber eine kleinere `id`
    /// gesprungen sein, die noch nie verteilt wurde (siehe Modulkopf). Der
    /// Rueckgriff verteilt dadurch Dubletten; die wirft [`Auslieferung`] im
    /// Socket wieder weg.
    async fn luecke_nachziehen(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        // Der Lesezeiger ist bewusst lokal und nicht der Wasserstand: der Stand
        // darf nur sagen, wo angefangen wird, nicht was durchgelassen wird.
        let mut zeiger =
            (self.wasserstand.load(Ordering::SeqCst).max(0) - NACHZUG_RUECKGRIFF).max(0);
        for runde in 1..=LUECKE_RUNDEN_DECKEL {
            let zeilen: Vec<(i64, String, String)> = sqlx::query_as(
                "SELECT id, channel_id, payload::text
                   FROM obs_dock_events
                  WHERE id > $1
                  ORDER BY id
                  LIMIT $2",
            )
            .bind(zeiger)
            .bind(LUECKE_DECKEL)
            .fetch_all(pool)
            .await?;

            let anzahl = zeilen.len() as i64;
            for (id, channel_id, json) in zeilen {
                zeiger = zeiger.max(id);
                self.veroeffentlichen(&channel_id, BusRahmen::neu(id, json));
                self.wasserstand.fetch_max(id, Ordering::SeqCst);
            }
            if anzahl < LUECKE_DECKEL {
                return Ok(());
            }
            debug!(runde, "OBS-Dock: Luecke noch nicht leer, naechste Runde");
        }

        // Mehr, als der Nachlaufpuffer ueberhaupt vorhaelt. Den Rest bewusst
        // ueberspringen, sonst laeuft der Listener hier fest, aber laut.
        let hoechste: Option<i64> =
            sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM obs_dock_events")
                .fetch_optional(pool)
                .await?
                .flatten();
        let ziel = hoechste.unwrap_or(0);
        warn!(
            uebersprungen = ziel.saturating_sub(zeiger),
            runden = LUECKE_RUNDEN_DECKEL,
            "OBS-Dock: Luecke groesser als der Nachzugsdeckel, Rest wird uebersprungen"
        );
        // Welche Kanaele betroffen sind, weiss hier niemand mehr, also erfaehrt
        // es jedes offene Dock. Ein Dock, dessen Kanal gar nichts verloren hat,
        // sieht dadurch einen ueberfluessigen Hinweis; das ist die guenstigere
        // Seite des Irrtums.
        self.an_alle_veroeffentlichen(BusRahmen::luecke(ziel));
        self.wasserstand.fetch_max(ziel, Ordering::SeqCst);
        Ok(())
    }

    /// Testhaken: der Nachzug ist im Betrieb nur ueber den Listener erreichbar,
    /// und ein Test soll ihn fahren koennen, ohne einen Verbindungsabriss
    /// nachzustellen. Setzt den Wasserstand und zieht danach nach, also genau
    /// die Reihenfolge des echten Wiederaufbaus in [`ObsDockBus::horchen`].
    #[cfg(test)]
    pub(crate) async fn nachzug_ab_stand_fuer_tests(
        &self,
        pool: &PgPool,
        stand: i64,
    ) -> Result<(), sqlx::Error> {
        self.wasserstand.store(stand, Ordering::SeqCst);
        self.luecke_nachziehen(pool).await
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

/// Wie viele schon gesendete `id` ein Socket vorhaelt.
///
/// Das ist das Fenster, in dem eine Dublette noch erkannt wird. Es muss alles
/// abdecken, was der Bus doppelt liefern kann: den Broadcast-Puffer
/// ([`BROADCAST_TIEFE`], 256), den Rueckgriff des Nachzugs
/// ([`NACHZUG_RUECKGRIFF`], 1000) und den Nachlauf eines Sockets. 4096 liegt
/// deutlich darueber und kostet je Socket rund 100 KB.
pub const GESENDET_FENSTER: usize = 4096;

// Das Fenster muss ueber allem liegen, was der Bus doppelt liefern kann; sonst
// faellt eine Dublette heraus, bevor sie erkannt wird. Zur Uebersetzungszeit
// festgenagelt, damit niemand eine der drei Zahlen ohne die andere anhebt.
const _: () = assert!(GESENDET_FENSTER as i64 > NACHZUG_RUECKGRIFF);
const _: () = assert!(GESENDET_FENSTER > BROADCAST_TIEFE);
const _: () = assert!(GESENDET_FENSTER as i64 > NACHLAUF_DECKEL);

/// Buchfuehrung eines einzelnen Sockets ueber das, was er schon gesendet hat.
///
/// # Warum kein Wasserstand
///
/// Naheliegend waere ein `id > letzte_id`. Das war die erste Fassung und es war
/// falsch: `id` wird nicht in Sichtbarkeitsreihenfolge vergeben (siehe
/// Modulkopf). Trifft 101 vor 100 ein, haette ein Wasserstand 101 gesendet und
/// 100 danach stumm verworfen, ohne Log und ohne Lueckenhinweis. Bei laufendem
/// Chat waere das der Normalfall, nicht der Sonderfall.
///
/// Stattdessen merkt sich die Buchfuehrung die zuletzt gesendeten `id` selbst
/// ([`GESENDET_FENSTER`] Stueck, aeltere fallen hinten heraus). Der Filter
/// deckt damit genau das ab, wofuer er da ist: die Ueberschneidung von Nachlauf
/// und Live und die Dubletten aus dem Rueckgriff des Nachzugs. Ein Live-Rahmen
/// wird nur noch verworfen, wenn dieser Socket ihn wirklich schon gesendet hat.
///
/// `vor_verbindung` ist der `?seit=<id>`-Wert: was das Dock selbst als gesehen
/// meldet, wird nicht noch einmal geschickt. Dasselbe Feld traegt nach einem
/// Lueckenhinweis den Sprung, denn ab da ist alles darunter erledigt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Auslieferung {
    vor_verbindung: i64,
    hoechste: i64,
    gesendet: HashSet<i64>,
    reihenfolge: VecDeque<i64>,
}

impl Auslieferung {
    /// Startet die Buchfuehrung. `seit` ist der `?seit=<id>`-Wert des Docks:
    /// alles bis einschliesslich dieser `id` hat das Dock schon gesehen.
    pub fn neu(seit: Option<i64>) -> Self {
        let seit = seit.unwrap_or(0).max(0);
        Self {
            vor_verbindung: seit,
            hoechste: seit,
            gesendet: HashSet::new(),
            reihenfolge: VecDeque::new(),
        }
    }

    /// Hoechste schon ausgelieferte `id`. Ankerpunkt fuer die naechste
    /// Nachlauf-Abfrage, **kein** Filter.
    pub fn anker(&self) -> i64 {
        self.hoechste
    }

    /// Bucht eine Zeile aus dem Nachlauf als gesendet.
    pub fn nachlauf(&mut self, id: i64) {
        self.merken(id);
    }

    /// Entscheidet ueber einen Rahmen: `true` heisst senden.
    pub fn live(&mut self, id: i64) -> bool {
        if id <= self.vor_verbindung || self.gesendet.contains(&id) {
            return false;
        }
        self.merken(id);
        true
    }

    /// Bucht einen Lueckenhinweis bis einschliesslich `bis`: alles darunter
    /// gilt als erledigt, egal ob gesendet oder uebersprungen.
    pub fn luecke_bis(&mut self, bis: i64) {
        self.untergrenze_setzen(bis);
    }

    /// Kleinste `id`, die dieser Socket ueberhaupt noch senden darf.
    ///
    /// Ein Rueckgriff darf darunter gar nicht erst lesen.
    pub fn untergrenze(&self) -> i64 {
        self.vor_verbindung
    }

    /// Zieht die Untergrenze auf `bis` hoch: alles bis einschliesslich `bis`
    /// gilt als erledigt und geht nie mehr hinaus.
    ///
    /// Das braucht vor allem der Vorlauf. Ein Dock ohne `?seit=` bestellt
    /// ausdruecklich nur die letzten [`VORLAUF_OHNE_SEIT`] Zeilen und sonst
    /// nichts. Ohne diese Grenze stuende `vor_verbindung` auf 0, im Merkfenster
    /// staenden nur eben diese Zeilen, und der naechste Rueckgriff (Aufholen
    /// nach `Lagged` oder ein Nachzug des Busses) wuerde jede aeltere Zeile als
    /// neuen Rahmen ins Overlay spuelen: bis zu [`NACHZUG_RUECKGRIFF`] alte
    /// Chatnachrichten auf einen Schlag.
    pub fn untergrenze_setzen(&mut self, bis: i64) {
        self.hoechste = self.hoechste.max(bis);
        if bis <= self.vor_verbindung {
            return;
        }
        self.vor_verbindung = bis;
        // Was jetzt unter der Grenze liegt, muss nicht mehr einzeln gemerkt
        // werden; das haelt das Fenster fuer die wirklich strittigen `id` frei.
        self.reihenfolge.retain(|id| *id > bis);
        self.gesendet.retain(|id| *id > bis);
    }

    fn merken(&mut self, id: i64) {
        self.hoechste = self.hoechste.max(id);
        if id <= self.vor_verbindung {
            return;
        }
        if self.gesendet.insert(id) {
            self.reihenfolge.push_back(id);
            while self.reihenfolge.len() > GESENDET_FENSTER {
                if let Some(alt) = self.reihenfolge.pop_front() {
                    self.gesendet.remove(&alt);
                }
            }
        }
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
        assert_eq!(buch.anker(), 2);
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
        assert_eq!(buch.anker(), 10);
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
        assert_eq!(buch.anker(), 11);
    }

    #[test]
    fn negatives_seit_wird_auf_null_geklemmt() {
        let buch = Auslieferung::neu(Some(-5));
        assert_eq!(buch.anker(), 0);
    }

    /// Der Kern des Review-Funds: `pg_notify` kommt nicht in `id`-Reihenfolge
    /// an. Meldet der Bus erst 101 und dann 100, muessen **beide** hinaus, und
    /// keines von beiden ein zweites Mal.
    ///
    /// Ein Wasserstand-Filter (`id > letzte_id`) haette 100 hier stumm
    /// verworfen.
    #[test]
    fn verkehrte_id_reihenfolge_verliert_keinen_rahmen() {
        let mut buch = Auslieferung::neu(None);
        assert!(buch.live(101), "101 kommt zuerst und geht raus");
        assert!(buch.live(100), "100 kommt danach und muss trotzdem raus");
        assert!(!buch.live(101), "keine Dublette");
        assert!(!buch.live(100), "keine Dublette");
        assert_eq!(buch.anker(), 101);
    }

    /// Dasselbe im Uebergang von Nachlauf auf Live: was der Nachlauf schon
    /// hatte, wird verworfen, alles andere geht raus, auch wenn es unter dem
    /// hoechsten schon gesendeten Wert liegt.
    #[test]
    fn nachlauf_entdoppelt_ohne_kleinere_ids_zu_fressen() {
        let mut buch = Auslieferung::neu(Some(5));
        for id in [6, 7, 9] {
            buch.nachlauf(id);
        }
        assert!(!buch.live(6), "stand schon im Nachlauf");
        assert!(!buch.live(9), "stand schon im Nachlauf");
        assert!(buch.live(8), "wurde erst nach der Abfrage sichtbar");
        assert!(!buch.live(5), "unter dem Lesezeichen des Docks");
        assert!(buch.live(10));
        assert!(!buch.live(8), "keine Dublette");
    }

    /// Ein Dock ohne `?seit=` hat nur seinen Vorlauf bestellt. Wird die
    /// Untergrenze nicht gesetzt, laesst `live()` danach jede aeltere `id`
    /// durch, die nicht mehr im Merkfenster steht, und der naechste Rueckgriff
    /// spuelt bis zu [`NACHZUG_RUECKGRIFF`] alte Zeilen ins Overlay.
    #[test]
    fn vorlauf_setzt_die_untergrenze_unter_seine_aelteste_zeile() {
        let mut buch = Auslieferung::neu(None);
        // Vorlauf: die Zeilen 51..=60, alles davor hat das Dock nie bestellt.
        buch.untergrenze_setzen(50);
        for id in 51..=60 {
            buch.nachlauf(id);
        }
        assert_eq!(buch.untergrenze(), 50);

        // Genau der Rueckgriff aus dem Aufholen bzw. dem Bus-Nachzug.
        for alt in 1..=50 {
            assert!(!buch.live(alt), "alte Zeile {alt} darf nicht ins Overlay");
        }
        // Der eigene Vorlauf kommt auch nicht doppelt.
        assert!(!buch.live(55));
        // Neues geht weiter raus, auch verkehrt herum.
        assert!(buch.live(62));
        assert!(buch.live(61));
    }

    /// Nach einem Lueckenhinweis ist alles darunter erledigt, und das Fenster
    /// der gemerkten `id` wird dabei frei.
    #[test]
    fn luecke_bis_setzt_die_untergrenze() {
        let mut buch = Auslieferung::neu(None);
        assert!(buch.live(3));
        buch.luecke_bis(900);
        assert!(!buch.live(3), "unter der Luecke");
        assert!(!buch.live(900), "die Luecke selbst gilt als erledigt");
        assert!(buch.live(901));
        assert_eq!(buch.anker(), 901);
    }

    /// Kann der Bus eine Luecke nicht mehr nachziehen, erfaehrt es jedes
    /// offene Dock, nicht nur der Serverlog.
    #[tokio::test]
    async fn lueckenhinweis_erreicht_jeden_kanal() {
        let bus = ObsDockBus::ohne_datenbank();
        let mut a = bus.anmelden("kanal-a");
        let mut b = bus.anmelden("kanal-b");

        assert_eq!(bus.an_alle_veroeffentlichen(BusRahmen::luecke(900)), 2);

        let von_a = a.rahmen.recv().await.unwrap();
        let von_b = b.rahmen.recv().await.unwrap();
        assert_eq!(von_a.id, 900);
        assert!(von_a.json.is_none(), "Lueckenhinweis traegt keine Nutzlast");
        assert_eq!(von_b, von_a);
    }

    /// Der Nachzug nach einem Listener-Abriss darf nicht am Abfragedeckel
    /// haengen bleiben: was darueber liegt, waere sonst dauerhaft weg, ohne Log
    /// und ohne Hinweis ans Dock.
    ///
    /// Ohne `TB_TEST_DATABASE_URL` ueberspringt sich der Test selbst.
    #[tokio::test]
    async fn luecke_wird_ueber_den_abfragedeckel_hinaus_nachgezogen() {
        let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let schema = "obs_bus_luecke";
        let aufbau = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("Test-DB erreichbar");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&aufbau)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&aufbau)
            .await
            .unwrap();
        aufbau.close().await;

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |conn, _| {
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&dsn)
            .await
            .expect("Testpool");
        sqlx::query(
            "CREATE TABLE obs_dock_events (
                 id BIGSERIAL PRIMARY KEY,
                 channel_id TEXT NOT NULL,
                 payload JSONB NOT NULL
             )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let anzahl = LUECKE_DECKEL * 2 + 13;
        sqlx::query(
            "INSERT INTO obs_dock_events (channel_id, payload)
             SELECT 'kanal-a', jsonb_build_object('typ','chat','id',lauf::text)
               FROM generate_series(1, $1) AS lauf",
        )
        .bind(anzahl)
        .execute(&pool)
        .await
        .unwrap();

        let bus = ObsDockBus::neu(pool.clone());
        bus.wasserstand.store(0, Ordering::SeqCst);
        bus.luecke_nachziehen(&pool).await.unwrap();

        // Eine einzelne gekappte Abfrage waere bei LUECKE_DECKEL stehen
        // geblieben und haette den Rest verschluckt.
        assert_eq!(bus.wasserstand.load(Ordering::SeqCst), anzahl);

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .ok();
    }

    /// Der Nachzug darf nicht auf dem Wasserstand aufsetzen.
    ///
    /// Aufbau ist genau der Fall aus dem Review: der Stand steht schon auf der
    /// hoeheren `id`, weil deren NOTIFY zuerst kam, waehrend die kleinere `id`
    /// nie verteilt wurde. Ein `WHERE id > stand` haette sie fuer immer
    /// uebersprungen; der Rueckgriff holt sie.
    ///
    /// Ohne `TB_TEST_DATABASE_URL` ueberspringt sich der Test selbst.
    #[tokio::test]
    async fn nachzug_greift_unter_den_wasserstand_zurueck() {
        let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let schema = "obs_bus_rueckgriff";
        let aufbau = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("Test-DB erreichbar");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&aufbau)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&aufbau)
            .await
            .unwrap();
        aufbau.close().await;

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |conn, _| {
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&dsn)
            .await
            .expect("Testpool");
        sqlx::query(
            "CREATE TABLE obs_dock_events (
                 id BIGSERIAL PRIMARY KEY,
                 channel_id TEXT NOT NULL,
                 payload JSONB NOT NULL
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO obs_dock_events (id, channel_id, payload)
             VALUES (100, 'kanal-a', '{\"typ\":\"chat\",\"id\":\"100\"}'::jsonb),
                    (101, 'kanal-a', '{\"typ\":\"chat\",\"id\":\"101\"}'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let bus = ObsDockBus::neu(pool.clone());
        let mut dock = bus.anmelden("kanal-a");
        // 101 kam per NOTIFY zuerst durch und hat den Stand hochgezogen; 100
        // wurde nie verteilt.
        bus.wasserstand.store(101, Ordering::SeqCst);
        bus.luecke_nachziehen(&pool).await.unwrap();

        let mut geholt: Vec<i64> = Vec::new();
        while let Ok(rahmen) = dock.rahmen.try_recv() {
            geholt.push(rahmen.id);
        }
        assert!(
            geholt.contains(&100),
            "die uebersprungene id muss nachkommen, geholt: {geholt:?}"
        );

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .ok();
    }
}

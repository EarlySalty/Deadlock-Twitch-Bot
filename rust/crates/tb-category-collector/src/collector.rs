//! Der Dauerdienst des Kategoriesammlers.
//!
//! Schleifen:
//! - **Discovery** je Konfigurationsintervall (Default: Minute): vollständiger,
//!   deduplizierter Kategorie-Durchlauf; ein Poll ist eine abgeschlossene
//!   Beobachtung mit Status, Start/Abschluss und Kanal-/Viewerzahl.
//!   Unvollständige Polls schreiben keine Snapshot-/Roster-Zeilen; der
//!   Chat-Roster verfällt erst nach `roster_decay` ohne vollständigen Poll.
//! - **Chat-Writer**: Batch-Inserts der anonym gelesenen PRIVMSGs.
//! - **Rollup**: stündliche Aggregate, idempotent, mit Catch-up.
//! - **Datenhaltung**: Rohchat und Snapshots bleiben ohne Alterslöschung
//!   erhalten. Rollups sind zusätzliche Auswertungen, kein Rohdaten-Ersatz.
//! - **Status**: schreibt Gesundheit + Drop-Zähler für das Admin-Panel.
//! - **VOD-Sweep**: sparsame Helix-VOD-Metadaten (Listenfelder), kein
//!   Video-/Audio-Download, kein STT.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Timelike, Utc};
use sqlx::PgPool;
use tb_engagement::irc_message::parse_privmsg;
use tb_monitoring::anon_chat::{
    AnonChatConfig, AnonChatHandle, AnonChatStatsSnapshot, PrivmsgSink, TokioTaskSpawner,
};
use tb_transport_twitch::streams::HelixStream;
use tb_transport_twitch::HelixClient;
use tokio::sync::mpsc;

use crate::config::{lade, SammlerKonfig};
use crate::sprache;
use crate::store::{self, ChatZeile, SnapshotZeile, StatusUpdate};

/// Sanity-Deckel für den Kategorie-Durchlauf; ein Erreichen wird transparent
/// als unvollständig markiert, nie als vollständiges Weltbild ausgegeben.
const KATEGORIE_HARD_CAP: usize = 5000;

/// Chat-Batch-Größe und Flush-Intervall.
const CHAT_BATCH_GROESSE: usize = 500;
/// Redaktionsaufträge pro Flush (CLEARMSG/CLEARCHAT).
const REDAKTION_BATCH_GROESSE: usize = 64;
const CHAT_BATCH_FLUSH: Duration = Duration::from_secs(2);
/// Kapazität der Warteschlange zwischen Chat-Sink und DB-Writer (bounded).
const CHAT_QUEUE_KAPAZITAET: usize = 20_000;

/// Maximal VOD-Sweeps pro Minute (sparsame Zusatzsammlung).
const VOD_BATCH_PRO_TICK: usize = 5;
const VOD_TICK: Duration = Duration::from_secs(60);

/// Periodische game_id-Validierung gegen die Kategoriesuche.
const GAME_ID_PRUEFINTERVALL: chrono::Duration = chrono::Duration::hours(1);

/// Roster aktuell beobachteter Deadlock-Kanäle: login → user_id (stabile
/// Identität auch für Steuerereignisse ohne room-id-Tag).
type Roster = Arc<RwLock<HashMap<String, String>>>;

fn roster_lese(roster: &Roster) -> RwLockReadGuard<'_, HashMap<String, String>> {
    roster
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn roster_schreibe(roster: &Roster) -> RwLockWriteGuard<'_, HashMap<String, String>> {
    roster
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Shared Status zwischen den Schleifen; ein eigener Writer persistiert.
struct StatusFeld {
    inner: Mutex<StatusUpdate>,
    game_id_geprueft: Mutex<Option<DateTime<Utc>>>,
    chat_duplikate: AtomicU64,
    chat_queue_dropped: AtomicU64,
    redaktionen: AtomicU64,
}

impl StatusFeld {
    fn neu() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(StatusUpdate::default()),
            game_id_geprueft: Mutex::new(None),
            chat_duplikate: AtomicU64::new(0),
            chat_queue_dropped: AtomicU64::new(0),
            redaktionen: AtomicU64::new(0),
        })
    }

    fn setze_fehler(&self, meldung: impl Into<String>) {
        let mut status = self.inner.lock().unwrap();
        status.last_error = Some(meldung.into());
        status.last_error_at = Some(Utc::now());
    }

    fn setze_poll(&self, started_at: Option<DateTime<Utc>>, complete_at: Option<DateTime<Utc>>) {
        let mut status = self.inner.lock().unwrap();
        if started_at.is_some() {
            status.last_poll_started_at = started_at;
        }
        if complete_at.is_some() {
            status.last_complete_poll_at = complete_at;
        }
    }

    fn setze_roster(&self, groesse: usize) {
        self.inner.lock().unwrap().roster_size = groesse as i32;
    }

    fn setze_rollup(&self, stunde: DateTime<Utc>) {
        self.inner.lock().unwrap().last_rollup_hour = Some(stunde);
    }

    fn game_id_geprueft_am(&self) -> Option<DateTime<Utc>> {
        *self.game_id_geprueft.lock().unwrap()
    }

    fn setze_game_id_geprueft(&self, am: DateTime<Utc>) {
        *self.game_id_geprueft.lock().unwrap() = Some(am);
    }

    fn snapshot(&self, chat_stats: &AnonChatStatsSnapshot) -> StatusUpdate {
        let mut status = self.inner.lock().unwrap().clone();
        status.chat_privmsgs_dispatched = chat_stats.privmsgs_dispatched;
        status.chat_privmsgs_dropped = chat_stats.privmsgs_dropped;
        status.chat_commands_dropped = chat_stats.commands_dropped;
        status.chat_invalid_logins_rejected = chat_stats.invalid_logins_rejected;
        status.chat_reconnects = chat_stats.reconnects;
        status.chat_queue_dropped = self.chat_queue_dropped.load(Ordering::Relaxed);
        status.chat_duplicates_skipped = self.chat_duplikate.load(Ordering::Relaxed);
        status.chat_redactions = self.redaktionen.load(Ordering::Relaxed);
        status.retention_deleted_total = 0;
        status.last_retention_run_at = None;
        status
    }
}

/// Ein Ereignis aus dem anonymen Chat: eine originäre Nachricht oder ein
/// Moderations-Löschereignis, das Rohtexte nachträglich redigiert.
#[derive(Debug)]
enum ChatEreignis {
    Zeile(ChatZeile),
    Redaktion(RedaktionAuftrag),
}

/// Nachträgliche Redaktion: welche Zeilen, ausgelöst von CLEARMSG/CLEARCHAT.
#[derive(Debug)]
enum RedaktionAuftrag {
    /// Einzelne Nachricht (CLEARMSG, `target-msg-id`).
    Nachricht {
        room_user_id: String,
        message_id: String,
    },
    /// Gesamter Chatter im Raum (CLEARCHAT mit `target-user-id`).
    Chatter {
        room_user_id: String,
        chatter_user_id: String,
    },
    /// Ganzer Raum (CLEARCHAT ohne Ziel).
    Raum { room_user_id: String },
}

/// Chat-Sink des Sammlers: anonyme PRIVMSG → geprüfte ChatZeile in die
/// bounded Queue; CLEARMSG/CLEARCHAT → Redaktionsaufträge. Kein CrewGuard,
/// kein ChatterTracker, keine Bot-Effekte.
struct CollectorChatSink {
    queue: mpsc::Sender<ChatEreignis>,
    roster: Roster,
    status: Arc<StatusFeld>,
}

#[async_trait::async_trait]
impl PrivmsgSink for CollectorChatSink {
    async fn handle_privmsg(&self, line: String) {
        match transport_verb(&line) {
            "PRIVMSG" => self.verarbeite_nachricht(&line),
            "CLEARMSG" | "CLEARCHAT" => self.verarbeite_redaktion(&line),
            _ => {}
        }
    }
}

impl CollectorChatSink {
    fn verarbeite_nachricht(&self, line: &str) {
        let Some(parsed) = parse_privmsg(line) else {
            return;
        };
        let login = parsed.channel.trim().trim_start_matches('#').to_lowercase();
        // Quellkanal muss aktuell als Deadlock beobachtet sein; die stabile
        // user_id kommt bevorzugt aus dem room-id-Tag, sonst aus dem Roster.
        let roster = roster_lese(&self.roster);
        let Some(roster_user_id) = roster.get(&login) else {
            return;
        };
        let broadcaster_id = parsed
            .tags
            .get("room-id")
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| roster_user_id.clone());
        let Some(chatter_id) = parsed
            .tags
            .get("user-id")
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
        else {
            return;
        };
        let chatter_login = parsed.login.trim().to_lowercase();
        let text = parsed.text.trim_end().to_string();
        if text.is_empty() || chatter_login.is_empty() {
            return;
        }
        let sent_at = parsed
            .tags
            .get("tmi-sent-ts")
            .and_then(|ts| ts.trim().parse::<i64>().ok())
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
            .unwrap_or_else(Utc::now);
        let emotes_tag = parsed.tags.get("emotes").map(String::as_str);
        let emote_count = zaehle_emotes(emotes_tag);
        let erkennung = sprache::erkenne(&text, emotes_tag);
        let message_id = parsed
            .tags
            .get("id")
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "noid-{}-{}-{:x}",
                    chatter_id,
                    sent_at.timestamp_millis(),
                    fnv1a(&format!("{chatter_id}{text}"))
                )
            });
        let zeile = ChatZeile {
            room_user_id: broadcaster_id,
            message_id,
            source_message_id: parsed
                .tags
                .get("source-id")
                .map(|id| id.trim())
                .filter(|id| !id.is_empty())
                .map(str::to_string),
            sent_at,
            chatter_user_id: chatter_id,
            chatter_login,
            message_text: text.clone(),
            text_len: text.chars().count() as i32,
            detected_lang: erkennung.lang,
            lang_confidence: erkennung.confidence,
            lang_method: erkennung.method.to_string(),
            emote_count,
        };
        if self.queue.try_send(ChatEreignis::Zeile(zeile)).is_err() {
            self.status
                .chat_queue_dropped
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// CLEARMSG/CLEARCHAT: Rohtexte nachträglich entfernen, ohne selbst je
    /// eine Twitch-Aktion auszulösen (rein passiv aus dem Lese-Strom).
    fn verarbeite_redaktion(&self, line: &str) {
        let Some((tags, verb, channel, _text)) = parse_steuer_zeile(line) else {
            return;
        };
        let login = channel.trim_start_matches('#').to_lowercase();
        let Some(room_user_id) = roster_lese(&self.roster).get(&login).cloned() else {
            return;
        };
        if tags
            .get("room-id")
            .is_some_and(|id| !id.trim().is_empty() && id.trim() != room_user_id)
        {
            return;
        }
        let auftrag = match verb {
            "CLEARMSG" => tags
                .get("target-msg-id")
                .map(|id| id.trim())
                .filter(|id| !id.is_empty())
                .map(|message_id| RedaktionAuftrag::Nachricht {
                    room_user_id,
                    message_id: message_id.to_string(),
                }),
            "CLEARCHAT" => match tags
                .get("target-user-id")
                .map(|id| id.trim())
                .filter(|id| !id.is_empty())
            {
                Some(chatter_user_id) => Some(RedaktionAuftrag::Chatter {
                    room_user_id,
                    chatter_user_id: chatter_user_id.to_string(),
                }),
                None => Some(RedaktionAuftrag::Raum { room_user_id }),
            },
            _ => None,
        };
        if let Some(auftrag) = auftrag {
            if self
                .queue
                .try_send(ChatEreignis::Redaktion(auftrag))
                .is_err()
            {
                self.status
                    .chat_queue_dropped
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Derselbe Kommando-Parser wie im anonymen Transport, kein zweiter Parser.
fn transport_verb(line: &str) -> &str {
    tb_monitoring::anon_chat::irc_command(line).0
}

/// Fachliche Steuerdaten; Framing und Tags kommen aus bestehenden Bausteinen.
fn parse_steuer_zeile(line: &str) -> Option<(HashMap<String, String>, &str, &str, Option<&str>)> {
    use tb_engagement::irc_message::parse_tags;
    let tags = match line.strip_prefix('@') {
        Some(rest) => parse_tags(rest.split_once(' ')?.0),
        None => HashMap::new(),
    };
    let (verb, rest) = tb_monitoring::anon_chat::irc_command(line);
    let (channel, text) = match rest.split_once(' ') {
        Some((channel, text)) => (channel, Some(text.strip_prefix(':').unwrap_or(text))),
        None => (rest, None),
    };
    if channel.is_empty() {
        return None;
    }
    Some((tags, verb, channel, text))
}

fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Zählt Emote-Instanzen aus dem `emotes`-Tag (`id:a-b,c-d/id2:a-b`).
fn zaehle_emotes(emotes: Option<&str>) -> i32 {
    let Some(tag) = emotes else { return 0 };
    tag.split('/')
        .filter(|e| !e.trim().is_empty())
        .map(|emote| {
            emote
                .split_once(':')
                .map(|(_, bereiche)| bereiche.split(',').count() as i32)
                .unwrap_or(0)
        })
        .sum()
}

fn zu_snapshot(stream: &HelixStream) -> SnapshotZeile {
    SnapshotZeile {
        stream_id: stream.id.trim().to_string(),
        user_id: stream.user_id.trim().to_string(),
        user_login: stream.user_login.trim().to_lowercase(),
        viewer_count: stream.viewer_count.clamp(0, i32::MAX as i64) as i32,
        title: Some(stream.title.trim().to_string()).filter(|t| !t.is_empty()),
        language: Some(stream.language.trim().to_lowercase()).filter(|l| !l.is_empty()),
        started_at: DateTime::parse_from_rfc3339(stream.started_at.trim())
            .ok()
            .map(|dt| dt.with_timezone(&Utc)),
        tags: stream.tags.clone(),
        is_mature: stream.is_mature,
    }
}

/// Der laufende Dienst. [`SammlerDienst::run`] kehrt nicht zurück.
pub struct SammlerDienst {
    pool: PgPool,
    helix: HelixClient,
}

impl SammlerDienst {
    pub async fn new(pool: PgPool, helix: HelixClient) -> Result<Self, sqlx::Error> {
        // Hängende Polls vom Vorgängerprozess als fehlgeschlagen schließen.
        store::schliesse_haengende_polls(&pool).await?;
        // Konfiguration beim Start prüfen: schlägt sie fehl, ist das ein
        // Startfehler (systemd-Restart statt stiller Default-Lauf).
        lade(&pool).await?;
        Ok(Self { pool, helix })
    }

    /// Startet alle Schleifen; die Discovery läuft im Vordergrund, damit ihr
    /// Tod den Prozess beendet (systemd-Restart greift).
    pub async fn run(self) {
        let status = StatusFeld::neu();
        let roster: Roster = Arc::new(RwLock::new(HashMap::new()));

        let (queue_tx, queue_rx) = mpsc::channel::<ChatEreignis>(CHAT_QUEUE_KAPAZITAET);
        let chat = AnonChatHandle::start(
            Arc::new(CollectorChatSink {
                queue: queue_tx.clone(),
                roster: Arc::clone(&roster),
                status: Arc::clone(&status),
            }),
            AnonChatConfig::default(),
            Arc::new(TokioTaskSpawner),
        );
        drop(queue_tx);

        tokio::spawn(chat_writer_loop(
            self.pool.clone(),
            queue_rx,
            Arc::clone(&status),
        ));
        tokio::spawn(status_loop(
            self.pool.clone(),
            Arc::clone(&status),
            Some(chat.clone()),
        ));
        tokio::spawn(rollup_loop(self.pool.clone(), Arc::clone(&status)));
        // Keine altersbasierte Löschung: Rohchat und Snapshots bleiben erhalten.
        // Rollups ergänzen die Rohdaten, sie ersetzen sie nicht.
        tokio::spawn(vod_loop(
            self.pool.clone(),
            self.helix.clone(),
            Arc::clone(&status),
        ));

        discovery_loop(
            self.pool.clone(),
            self.helix.clone(),
            status,
            roster,
            Some(chat),
        )
        .await;
    }
}

async fn discovery_loop(
    pool: PgPool,
    helix: HelixClient,
    status: Arc<StatusFeld>,
    roster: Roster,
    chat: Option<AnonChatHandle>,
) {
    loop {
        let konfig = match lade(&pool).await {
            Ok(konfig) => konfig,
            Err(error) => {
                status.setze_fehler(format!("Konfiguration nicht lesbar: {error}"));
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        if !konfig.enabled {
            if let Some(chat) = &chat {
                chat.set_channels(Vec::new()).await;
            }
            roster_schreibe(&roster).clear();
            status.setze_roster(0);
            tokio::time::sleep(konfig.discovery_interval).await;
            continue;
        }

        // game_id exakt validieren (stündlich gecacht): die Kategoriesuche
        // muss den konfigurierten Wert bestätigen, sonst kein Poll.
        let jetzt = Utc::now();
        let pruefung_faellig = status
            .game_id_geprueft_am()
            .map(|am| jetzt.signed_duration_since(am) >= GAME_ID_PRUEFINTERVALL)
            .unwrap_or(true);
        if pruefung_faellig {
            match helix.search_category_id("Deadlock").await {
                Ok(Some(gefundene_id)) if gefundene_id == konfig.deadlock_game_id => {
                    status.setze_game_id_geprueft(jetzt);
                }
                Ok(Some(gefundene_id)) => {
                    let meldung = format!(
                        "Deadlock-game_id unpassend: Suche liefert {gefundene_id}, Konfiguration {} — kein Poll",
                        konfig.deadlock_game_id
                    );
                    tracing::error!(meldung);
                    status.setze_fehler(meldung);
                    status.setze_game_id_geprueft(jetzt);
                    decay_pruefen(&status, &roster, chat.as_ref(), &konfig, jetzt).await;
                    tokio::time::sleep(Duration::from_secs(600)).await;
                    continue;
                }
                Ok(None) => {
                    let meldung = "Kategorie 'Deadlock' nicht gefunden — kein Poll".to_string();
                    tracing::error!(meldung);
                    status.setze_fehler(meldung);
                    tokio::time::sleep(Duration::from_secs(600)).await;
                    continue;
                }
                Err(error) => {
                    status.setze_fehler(format!("Kategoriesuche fehlgeschlagen: {error}"));
                    tokio::time::sleep(Duration::from_secs(600)).await;
                    continue;
                }
            }
        }

        poll_ausfuehren(&pool, &helix, &status, &roster, chat.as_ref(), &konfig).await;
        tokio::time::sleep(konfig.discovery_interval).await;
    }
}

async fn poll_ausfuehren(
    pool: &PgPool,
    helix: &HelixClient,
    status: &Arc<StatusFeld>,
    roster: &Roster,
    chat: Option<&AnonChatHandle>,
    konfig: &SammlerKonfig,
) {
    let started_at = Utc::now();
    status.setze_poll(Some(started_at), None);
    let poll_id = match store::start_poll(pool, started_at).await {
        Ok(id) => id,
        Err(error) => {
            let meldung = format!("Poll-Eintrag nicht anlegbar: {error}");
            tracing::error!(meldung);
            status.setze_fehler(meldung);
            return;
        }
    };

    let fetch = helix
        .get_streams_by_category_full(&konfig.deadlock_game_id, KATEGORIE_HARD_CAP)
        .await;

    if !fetch.complete {
        // Unvollständiger Poll: keine Snapshot-/Roster-Änderung, keine
        // falschen Offline-/Null-Zeilen. Nur der Poll-Eintrag dokumentiert
        // die Lücke.
        let status_text = if fetch.truncated_by_cap {
            "incomplete"
        } else {
            "failed"
        };
        let meldung = fetch
            .error
            .clone()
            .unwrap_or_else(|| "Kategorie-Durchlauf unvollständig (Cap erreicht)".to_string());
        tracing::warn!(poll_id, %meldung, "Discovery unvollständig");
        if let Err(error) = store::beende_poll(
            pool,
            poll_id,
            status_text,
            fetch.streams.len() as i32,
            fetch.streams.iter().map(|s| s.viewer_count).sum::<i64>(),
            fetch.pages_fetched as i32,
            Some(&meldung),
        )
        .await
        {
            status.setze_fehler(format!("Poll-Abschluss fehlgeschlagen: {error}"));
        }
        status.setze_fehler(meldung);
        decay_pruefen(status, roster, chat, konfig, Utc::now()).await;
        return;
    }

    let snapshot_at = Utc::now();
    let zeilen: Vec<SnapshotZeile> = fetch.streams.iter().map(zu_snapshot).collect();
    let viewer_total: i64 = zeilen.iter().map(|z| z.viewer_count as i64).sum();

    if let Err(error) = store::schreibe_snapshots(pool, poll_id, snapshot_at, &zeilen).await {
        let meldung = format!("Snapshots nicht schreibbar: {error}");
        tracing::error!(meldung);
        status.setze_fehler(meldung.clone());
        let _ = store::beende_poll(
            pool,
            poll_id,
            "failed",
            0,
            0,
            fetch.pages_fetched as i32,
            Some(&meldung),
        )
        .await;
        return;
    }
    if let Err(error) = store::upsert_kanaele(pool, &zeilen, snapshot_at).await {
        status.setze_fehler(format!("Kanalstammdaten nicht aktualisierbar: {error}"));
    }
    if let Err(error) = store::beende_poll(
        pool,
        poll_id,
        "complete",
        zeilen.len() as i32,
        viewer_total,
        fetch.pages_fetched as i32,
        None,
    )
    .await
    {
        status.setze_fehler(format!("Poll-Abschluss fehlgeschlagen: {error}"));
    }

    // Roster nur nach vollständigem Erfolg ersetzen (inkrementell; der
    // Transport bildet den JOIN/PART-Diff mit stabiler Zuordnung).
    let mut neue_roster: HashMap<String, String> = HashMap::new();
    for zeile in &zeilen {
        if !zeile.user_login.is_empty() {
            neue_roster.insert(zeile.user_login.clone(), zeile.user_id.clone());
        }
    }
    let chat_roster: Vec<String> = if konfig.chat_enabled {
        neue_roster.keys().cloned().collect()
    } else {
        Vec::new()
    };
    if let Some(chat) = chat {
        chat.set_channels(chat_roster).await;
    }
    *roster_schreibe(roster) = neue_roster.clone();
    status.setze_roster(neue_roster.len());
    status.setze_poll(Some(started_at), Some(snapshot_at));

    enrichment_lauf(pool, helix, status).await;
}

/// Bei längerem Ausfall den alten Roster zeitlich auslaufen lassen: keine
/// Verbindungen in Kanäle, die evtl. nicht mehr Deadlock spielen.
async fn decay_pruefen(
    status: &Arc<StatusFeld>,
    roster: &Roster,
    chat: Option<&AnonChatHandle>,
    konfig: &SammlerKonfig,
    jetzt: DateTime<Utc>,
) {
    let letzte = status.inner.lock().unwrap().last_complete_poll_at;
    let Some(letzte) = letzte else { return };
    let ohne_poll = jetzt.signed_duration_since(letzte);
    if ohne_poll >= chrono::Duration::from_std(konfig.roster_decay).unwrap_or_default()
        && !roster_lese(roster).is_empty()
    {
        tracing::warn!(
            ohne_poll_sekunden = ohne_poll.num_seconds(),
            "kein vollständiger Poll — Chat-Roster verfällt"
        );
        if let Some(chat) = chat {
            chat.set_channels(Vec::new()).await;
        }
        roster_schreibe(roster).clear();
        status.setze_roster(0);
    }
}

/// Profile neuer Kanäle ergänzen: `/users` (Kontalter, Typ, Text) plus
/// `/channels` (Broadcaster-Sprache). Batches von 100, ein Lauf je Poll.
async fn enrichment_lauf(pool: &PgPool, helix: &HelixClient, status: &Arc<StatusFeld>) {
    let fehlend = match store::kanaele_ohne_profil(pool, 100).await {
        Ok(f) => f,
        Err(error) => {
            status.setze_fehler(format!("Profil-Lookup-Liste fehlgeschlagen: {error}"));
            return;
        }
    };
    if fehlend.is_empty() {
        return;
    }
    let users: HashMap<String, tb_transport_twitch::streams::HelixUser> =
        match helix.get_users_by_ids(&fehlend).await {
            Ok(users) => users.into_iter().map(|u| (u.id.clone(), u)).collect(),
            Err(error) => {
                status.setze_fehler(format!("Profil-/users fehlgeschlagen: {error}"));
                return;
            }
        };
    let mut kanalsprache: HashMap<String, String> = HashMap::new();
    match helix.get_channel_information_batch(&fehlend).await {
        Ok(kanaele) => {
            for kanal in kanaele {
                kanalsprache.insert(
                    kanal.broadcaster_id.trim().to_string(),
                    kanal.broadcaster_language.trim().to_lowercase(),
                );
            }
        }
        Err(error) => {
            status.setze_fehler(format!("Profil-/channels fehlgeschlagen: {error}"));
        }
    }
    if let Err(error) = store::ergaenze_profile(pool, &fehlend, &users, &kanalsprache).await {
        status.setze_fehler(format!("Profil-Speicherung fehlgeschlagen: {error}"));
    }
}

async fn chat_writer_loop(
    pool: PgPool,
    mut rx: mpsc::Receiver<ChatEreignis>,
    status: Arc<StatusFeld>,
) {
    let mut batch: Vec<ChatZeile> = Vec::with_capacity(CHAT_BATCH_GROESSE);
    let mut redaktionen: Vec<RedaktionAuftrag> = Vec::new();
    let mut flush = tokio::time::interval(CHAT_BATCH_FLUSH);
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            ereignis = rx.recv() => {
                match ereignis {
                    Some(ChatEreignis::Zeile(zeile)) => {
                        batch.push(zeile);
                        if batch.len() >= CHAT_BATCH_GROESSE {
                            flush_batch(&pool, &mut batch, &status).await;
                        }
                    }
                    Some(ChatEreignis::Redaktion(auftrag)) => {
                        redaktionen.push(auftrag);
                        if redaktionen.len() >= REDAKTION_BATCH_GROESSE {
                            flush_redaktionen(&pool, &mut redaktionen, &status).await;
                        }
                    }
                    None => {
                        flush_batch(&pool, &mut batch, &status).await;
                        flush_redaktionen(&pool, &mut redaktionen, &status).await;
                        return;
                    }
                }
            }
            _ = flush.tick() => {
                flush_batch(&pool, &mut batch, &status).await;
                flush_redaktionen(&pool, &mut redaktionen, &status).await;
            }
        }
    }
}

async fn flush_redaktionen(
    pool: &PgPool,
    auftraege: &mut Vec<RedaktionAuftrag>,
    status: &Arc<StatusFeld>,
) {
    if auftraege.is_empty() {
        return;
    }
    let mut redigiert: u64 = 0;
    for auftrag in auftraege.drain(..) {
        let ergebnis = match &auftrag {
            RedaktionAuftrag::Nachricht {
                room_user_id,
                message_id,
            } => store::redigiere_nachricht(pool, room_user_id, message_id).await,
            RedaktionAuftrag::Chatter {
                room_user_id,
                chatter_user_id,
            } => store::redigiere_chatter(pool, room_user_id, chatter_user_id).await,
            RedaktionAuftrag::Raum { room_user_id } => {
                store::redigiere_raum(pool, room_user_id).await
            }
        };
        match ergebnis {
            Ok(anzahl) => redigiert += anzahl,
            Err(error) => {
                // Redaktion erneut versuchen beim nächsten Flush wäre doppelt;
                // Fehler sichtbar machen, batch weiterlaufen lassen.
                tracing::error!(%error, "Chat-Redaktion fehlgeschlagen");
                status.setze_fehler(format!("Chat-Redaktion fehlgeschlagen: {error}"));
            }
        }
    }
    if redigiert > 0 {
        status.redaktionen.fetch_add(redigiert, Ordering::Relaxed);
        tracing::info!(redigiert, "Moderations-Rohtexte redigiert");
    }
}

async fn flush_batch(pool: &PgPool, batch: &mut Vec<ChatZeile>, status: &Arc<StatusFeld>) {
    if batch.is_empty() {
        return;
    }
    match store::schreibe_chat_batch(pool, batch).await {
        Ok((eingefuegt, duplikate)) => {
            status
                .chat_duplikate
                .fetch_add(duplikate, Ordering::Relaxed);
            let _ = eingefuegt;
        }
        Err(error) => {
            // Batch verwerfen statt endlos wiederholen: diese Zeilen sind
            // verloren — sichtbar im Status/Fehler, nicht verschwiegen.
            tracing::error!(%error, anzahl = batch.len(), "Chat-Batch fehlgeschlagen");
            status.setze_fehler(format!("Chat-Batch fehlgeschlagen: {error}"));
        }
    }
    batch.clear();
}

async fn rollup_loop(pool: PgPool, status: Arc<StatusFeld>) {
    let mut tick = tokio::time::interval(Duration::from_secs(300));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let konfig = match lade(&pool).await {
            Ok(k) => k,
            Err(error) => {
                status.setze_fehler(format!("Rollup: Konfiguration nicht lesbar: {error}"));
                continue;
            }
        };
        if !konfig.rollup_enabled {
            continue;
        }
        if let Err(error) = rollup_catchup(&pool, &status).await {
            let _ = error; // bereits im Status/Fehler vermerkt
        }
    }
}

/// Rollup-Catch-up: von der letzten gesicherten Stunde (minus 2 h Überlappung
/// für späte Nachrichten) bis zur aktuellen Stunde, idempotent je Stunde.
async fn rollup_catchup(pool: &PgPool, status: &Arc<StatusFeld>) -> Result<(), sqlx::Error> {
    let letzte = store::letzte_rollup_stunde(pool).await?;
    let start = match letzte {
        Some(letzte) => letzte - chrono::Duration::hours(2),
        None => match store::frueheste_chat_stunde(pool).await? {
            Some(frueheste) => frueheste,
            None => return Ok(()), // noch keine Chat-Daten
        },
    };
    let jetzt_stunde = stunde_von(Utc::now());
    let mut stunde = start;
    let mut letzte_berechnete: Option<DateTime<Utc>> = None;
    while stunde < jetzt_stunde {
        if let Err(error) = store::rollup_stunde(pool, stunde).await {
            status.setze_fehler(format!("Rollup {stunde} fehlgeschlagen: {error}"));
            return Err(error);
        }
        letzte_berechnete = Some(stunde);
        stunde += chrono::Duration::hours(1);
    }
    if let Some(stunde) = letzte_berechnete {
        status.setze_rollup(stunde);
    }
    Ok(())
}

fn stunde_von(zeit: DateTime<Utc>) -> DateTime<Utc> {
    Utc.from_utc_datetime(
        &zeit
            .date_naive()
            .and_hms_opt(zeit.time().hour(), 0, 0)
            .unwrap_or_default(),
    )
}

async fn status_loop(pool: PgPool, status: Arc<StatusFeld>, chat: Option<AnonChatHandle>) {
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let chat_stats = chat
            .as_ref()
            .map(|handle| handle.stats())
            .unwrap_or_default();
        let update = status.snapshot(&chat_stats);
        if let Err(error) = store::schreibe_status(&pool, &update).await {
            tracing::error!(%error, "Status nicht schreibbar");
        }
    }
}

async fn vod_loop(pool: PgPool, helix: HelixClient, status: Arc<StatusFeld>) {
    let mut tick = tokio::time::interval(VOD_TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut warteschlange: Vec<(String, String)> = Vec::new();
    loop {
        tick.tick().await;
        let konfig = match lade(&pool).await {
            Ok(k) => k,
            Err(_) => continue, // Fehler meldet der Discovery-Loop ohnehin.
        };
        if !konfig.vod_metadata_enabled {
            continue;
        }
        if warteschlange.is_empty() {
            match sqlx::query_as::<_, (String, String)>(
                "SELECT user_id, login FROM category_channels \
                  WHERE last_seen_live_at >= NOW() - INTERVAL '7 days' \
                  ORDER BY last_seen_live_at DESC NULLS LAST",
            )
            .fetch_all(&pool)
            .await
            {
                Ok(kanaele) => warteschlange = kanaele,
                Err(error) => {
                    status.setze_fehler(format!("VOD-Kanalliste fehlgeschlagen: {error}"));
                    continue;
                }
            }
        }
        for _ in 0..VOD_BATCH_PRO_TICK {
            let Some((user_id, _login)) = warteschlange.pop() else {
                break;
            };
            match helix.get_archive_videos(&user_id, 5).await {
                Ok(vods) => {
                    if let Err(error) = store::speichere_vods(&pool, &user_id, &vods).await {
                        status.setze_fehler(format!("VOD-Metadaten nicht speicherbar: {error}"));
                    }
                }
                Err(error) => {
                    // Best effort: einzelne Fehler brechen den Sweep nicht ab.
                    tracing::debug!(%error, user_id, "VOD-Liste fehlgeschlagen");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emote_zaehler_liest_bereiche() {
        assert_eq!(zaehle_emotes(Some("25:0-4,6-10/1902:12-16")), 3);
        assert_eq!(zaehle_emotes(Some("")), 0);
        assert_eq!(zaehle_emotes(None), 0);
    }

    #[test]
    fn verb_erkennung_ignoriert_steuerwoerter_im_text() {
        assert_eq!(
            transport_verb("@id=x :u!u@u PRIVMSG #chan :bitte RECONNECT"),
            "PRIVMSG"
        );
        assert_eq!(transport_verb(":tmi.twitch.tv RECONNECT"), "RECONNECT");
        assert_eq!(
            transport_verb("@target-msg-id=x :tmi.twitch.tv CLEARMSG #chan :text"),
            "CLEARMSG"
        );
        assert_eq!(
            transport_verb("@room-id=1;target-user-id=2 :tmi.twitch.tv CLEARCHAT #chan :u"),
            "CLEARCHAT"
        );
    }

    #[tokio::test]
    async fn sink_verwirft_kanaele_außerhalb_des_rosters() {
        // Direkter Sink-Test ohne Socket: Roster leer → nichts in der Queue.
        let (tx, mut rx) = mpsc::channel(8);
        let status = StatusFeld::neu();
        let sink = CollectorChatSink {
            queue: tx,
            roster: Arc::new(RwLock::new(HashMap::new())),
            status: Arc::clone(&status),
        };
        sink.handle_privmsg(
            "@room-id=99;user-id=42;id=m1;tmi-sent-ts=1784138400123 \
             :viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #coolysdl :hallo welt"
                .to_string(),
        )
        .await;
        assert!(rx.try_recv().is_err(), "ohne Roster keine Zeile");
        assert_eq!(status.chat_queue_dropped.load(Ordering::Relaxed), 0);

        // Mit Roster kommt die Zeile an, korrekt befüllt.
        roster_schreibe(&sink.roster).insert("coolysdl".to_string(), "99".to_string());
        sink.handle_privmsg(
            "@room-id=99;user-id=42;id=m1;tmi-sent-ts=1784138400123 \
             :viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #coolysdl :Das war ein starker Kampf heute"
                .to_string(),
        )
        .await;
        let ChatEreignis::Zeile(zeile) = rx.try_recv().expect("Zeile muss ankommen") else {
            panic!("Zeile erwartet");
        };
        assert_eq!(zeile.room_user_id, "99");
        assert_eq!(zeile.chatter_user_id, "42");
        assert_eq!(zeile.message_id, "m1");
        assert_eq!(
            zeile.text_len,
            "Das war ein starker Kampf heute".chars().count() as i32
        );
        let erwartet = Utc.timestamp_millis_opt(1784138400123).single().unwrap();
        assert_eq!(zeile.sent_at, erwartet);
    }

    #[tokio::test]
    async fn redaktionsereignisse_landen_als_auftraege_in_der_queue() {
        let (tx, mut rx) = mpsc::channel(8);
        let status = StatusFeld::neu();
        let sink = CollectorChatSink {
            queue: tx,
            roster: Arc::new(RwLock::new(HashMap::from([(
                "coolysdl".to_string(),
                "99".to_string(),
            )]))),
            status: Arc::clone(&status),
        };

        // CLEARMSG ohne room-id-Tag: user_id kommt aus dem Roster.
        sink.handle_privmsg(
            "@login=viewer;target-msg-id=m1 :tmi.twitch.tv CLEARMSG #coolysdl :hallo".to_string(),
        )
        .await;
        let ChatEreignis::Redaktion(RedaktionAuftrag::Nachricht {
            room_user_id,
            message_id,
        }) = rx.try_recv().expect("CLEARMSG muss ankommen")
        else {
            panic!("Redaktion erwartet");
        };
        assert_eq!((room_user_id.as_str(), message_id.as_str()), ("99", "m1"));

        // CLEARCHAT mit Ziel → Chatter-Redaktion.
        sink.handle_privmsg(
            "@room-id=99;target-user-id=42 :tmi.twitch.tv CLEARCHAT #coolysdl :viewer".to_string(),
        )
        .await;
        let ChatEreignis::Redaktion(RedaktionAuftrag::Chatter {
            room_user_id,
            chatter_user_id,
        }) = rx.try_recv().expect("CLEARCHAT muss ankommen")
        else {
            panic!("Chatter-Redaktion erwartet");
        };
        assert_eq!(
            (room_user_id.as_str(), chatter_user_id.as_str()),
            ("99", "42")
        );

        // CLEARCHAT ohne Ziel → Raum-Redaktion.
        sink.handle_privmsg("@room-id=99 :tmi.twitch.tv CLEARCHAT #coolysdl".to_string())
            .await;
        let ChatEreignis::Redaktion(RedaktionAuftrag::Raum { room_user_id }) =
            rx.try_recv().expect("Raum-Redaktion muss ankommen")
        else {
            panic!("Raum-Redaktion erwartet");
        };
        assert_eq!(room_user_id, "99");

        // Kanal außerhalb des Rosters → keine Redaktion (niemals in fremde
        // Räume hineinredigieren, die nicht beobachtet werden).
        sink.handle_privmsg(
            "@room-id=555;target-user-id=42 :tmi.twitch.tv CLEARCHAT #fremd :viewer".to_string(),
        )
        .await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn sink_zaehlt_queue_verluste_sichtbar() {
        let (tx, _rx_gehalten) = mpsc::channel(1);
        let status = StatusFeld::neu();
        let sink = CollectorChatSink {
            queue: tx,
            roster: Arc::new(RwLock::new(HashMap::from([(
                "coolysdl".to_string(),
                "99".to_string(),
            )]))),
            status: Arc::clone(&status),
        };
        let zeile = "@room-id=99;user-id=42;id=m1 \
             :viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #coolysdl :Das war ein starker Kampf heute";
        sink.handle_privmsg(zeile.to_string()).await;
        sink.handle_privmsg(zeile.to_string()).await;
        // Kapazität 1, kein Leser: mindestens einer der beiden Versuch fiel
        // in den sichtbaren Drop-Zähler.
        assert!(status.chat_queue_dropped.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn snapshot_konvertierung_normalisiert() {
        let stream: HelixStream = serde_json::from_value(serde_json::json!({
            "id": " 991 ", "user_id": "42", "user_login": "CoolysDL",
            "title": " Push ", "language": " DE ", "viewer_count": 7,
            "started_at": "2026-07-15T12:00:00Z", "tags": ["Deutsch"]
        }))
        .unwrap();
        let zeile = zu_snapshot(&stream);
        assert_eq!(zeile.stream_id, "991");
        assert_eq!(zeile.user_login, "coolysdl");
        assert_eq!(zeile.language.as_deref(), Some("de"));
        assert_eq!(zeile.viewer_count, 7);
        let erwartet = Utc
            .with_ymd_and_hms(2026, 7, 15, 12, 0, 0)
            .single()
            .unwrap();
        assert_eq!(zeile.started_at, Some(erwartet));
    }
}

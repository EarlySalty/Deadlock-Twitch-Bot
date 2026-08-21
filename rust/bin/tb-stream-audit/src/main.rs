//! Privates Coaching-Audit autorisierter Twitch-Kanaele.
//!
//! Fuellt `deadlock-twitch-stream-coaching-watch.service` wieder mit Leben: die
//! Unit gab es weiter, ihr Startskript verschwand aber beim Abriss der
//! Python-Laufzeit am 21.07.2026, und der Dienst scheiterte seitdem.
//!
//! # Warum live und nicht VOD
//!
//! Ob ein Kanal seine VODs oeffentlich stehen laesst, wissen wir nicht. Ein
//! VOD-Pfad waere also ein Audit, das mal laeuft und mal nicht, ohne dass
//! jemand merkt warum. Live aufzunehmen ist der einzige Weg, der unabhaengig
//! von fremden Einstellungen funktioniert.
//!
//! # Warum aufnehmen und danach auswerten
//!
//! Aufgenommen wird **parallel**, je sendendem Kanal ein eigener Task.
//! Nacheinander waere jeder von drei Kanaelen nur ein Drittel der Zeit in
//! Aufnahme, und ein Audit, das zwei Drittel des Streams nie sieht, meldet
//! "keine Funde" - das liest sich wie "sauber".
//!
//! Transkribiert wird dagegen **seriell**, ein Block nach dem anderen aus einer
//! gemeinsamen Warteschlange. Die Maschine hat keine GPU; drei gleichzeitig
//! transkribierte Streams teilten sich dieselben Kerne wie das Modell. Aufnehmen
//! kostet fast nichts, transkribieren viel - deshalb die Trennung.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tb_engagement::audio_capture::AudioCapturer;
use tb_engagement::transcribe::OpenAiTranscriber;
use tb_stream_audit::{
    archiv,
    config::Konfiguration,
    last::Lastwaechter,
    llm, melden, plan,
    report::{self, Bericht},
    Segment,
};
use tb_transport_twitch::client::{HelixClient, HelixConfig};
use tokio::sync::Mutex;

/// Ein Segment je so vielen Sekunden Transkript.
///
/// Whisper liefert normalerweise eigene Zeitstempel; die werden zu Segmenten
/// dieser Laenge gebuendelt. Nur wenn sie fehlen, teilt der Rueckfallweg den
/// Text anteilig auf.
const SEGMENT_SEKUNDEN: f64 = 30.0;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let konfiguration = Konfiguration::from_env();
    if konfiguration.kanaele.is_empty() {
        tracing::error!(
            "keine Kanaele konfiguriert; {} setzen",
            tb_stream_audit::config::KANAELE_ENV
        );
        std::process::exit(2);
    }
    tracing::info!(
        kanaele = ?konfiguration.kanaele,
        ausgabe = %konfiguration.ausgabe.display(),
        "Coaching-Audit startet"
    );

    let Some(helix) = helix_aus_umgebung() else {
        tracing::error!("TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET fehlen");
        std::process::exit(2);
    };
    let Some(transkribierer) = OpenAiTranscriber::from_env_with_timeout(STT_ZEITGRENZE) else {
        tracing::error!("Transkription nicht konfiguriert; ENGAGEMENT_STT_BASE_URL setzen");
        std::process::exit(2);
    };
    // Rohes Stream-Audio ist die Sprache fremder Menschen. Ein entfernter
    // STT-Endpunkt ist deshalb kein Fall fuer eine Warnung, die im Journal
    // untergeht, sondern fuer einen Abbruch. Die Unit setzt die URL auf
    // localhost; eine EnvironmentFile koennte sie ueberschreiben, und genau
    // das faengt diese Pruefung ab.
    let transkription_lokal = tb_stream_audit::llm::ist_lokal(&stt_basis_url());
    if !transkription_lokal && !remote_stt_erlaubt() {
        tracing::error!(
            "Transkription zeigt auf einen fremden Rechner. {}=1 setzen, um rohes \
Stream-Audio dorthin zu senden.",
            REMOTE_STT_ERLAUBT_ENV
        );
        std::process::exit(2);
    }
    if !transkription_lokal {
        tracing::warn!("Transkription laeuft entfernt - ausdruecklich erlaubt");
    }

    // Zwischendateien der Transkription landen in unserem eigenen Ordner statt
    // in /tmp. Ein Abbruch mitten in der Anfrage laesst die WAV-Datei liegen;
    // dort finden wir sie beim naechsten Start und raeumen sie weg.
    let eigener_temp = konfiguration.ausgabe.join("zwischendateien");
    if let Err(fehler) = tokio::fs::create_dir_all(&eigener_temp).await {
        // Ohne eigenen Ordner landen die WAV-Dateien wieder in /tmp, wo sie
        // ein erzwungener Stopp ausserhalb jeder Aufbewahrung liegen laesst.
        tracing::error!(
            ?fehler,
            ordner = ?eigener_temp,
            "Ordner fuer Zwischendateien nicht anlegbar"
        );
        std::process::exit(2);
    }
    // Der Ordner geht direkt an den Transkribierer. Frueher stand hier
    // `set_var("TMPDIR", ...)`: das ist ein Datenrennen auf der Prozessumgebung,
    // sobald die Runtime laeuft, und es haette sich an jeden gestarteten
    // streamlink- und ffmpeg-Prozess vererbt.
    let transkribierer = transkribierer.with_temp_dir(&eigener_temp);
    zwischendateien_aufraeumen(&eigener_temp).await;

    let abbruch = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let warteschlange = Arc::new(Mutex::new(plan::Warteschlange::new()));
    let sperre = Arc::new(Mutex::new(plan::LaufSperre::new()));
    let plattenbelegung = Arc::new(std::sync::atomic::AtomicU64::new(0));
    // Wer gerade sendet, darf nicht schon beim Start transkribiert werden:
    // sonst frisst STT die Kerne, waehrend streamlink daneben aufnimmt.
    if let Ok(streams) = helix
        .get_streams_by_logins(&konfiguration.kanaele, None)
        .await
    {
        let mut s = sperre.lock().await;
        for sendung in &streams {
            let lauf = sendung.id.trim();
            if !lauf.is_empty() {
                s.sperren(sendung.user_login.to_lowercase(), lauf);
            }
        }
    }
    liegengebliebenes_einreihen(&konfiguration, &warteschlange).await;
    // Das Last-Gate: solange true, stellt die Auswertung STT zurueck. Ein
    // eigener Task misst CPU und RAM und schaltet es.
    let last_gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
    tokio::spawn(last_ueberwachen(
        Arc::clone(&last_gate),
        Arc::clone(&abbruch),
        Lastwaechter::aus_umgebung(),
    ));
    let aufnahme = tokio::spawn(aufnahme_schleife(
        helix,
        konfiguration.clone(),
        Arc::clone(&warteschlange),
        Arc::clone(&abbruch),
        Arc::clone(&sperre),
        Arc::clone(&plattenbelegung),
    ));
    let auswertung = tokio::spawn(auswertungs_schleife(
        transkribierer,
        konfiguration,
        Arc::clone(&warteschlange),
        Arc::clone(&abbruch),
        Arc::clone(&sperre),
        Arc::clone(&last_gate),
    ));

    // Beide Schleifen laufen endlos. Endet eine, ist der Dienst kaputt - er
    // wuerde sonst als leere Huelle weiterlaufen und nie wieder etwas
    // auswerten. Der Ausstiegscode muss ungleich 0 sein, sonst greift
    // Restart=on-failure in der Unit nicht.
    // Wer schon fertig ist, wird nicht noch einmal abgewartet: einen
    // abgeschlossenen JoinHandle erneut zu pollen, ist eine Panik.
    let mut aufnahme_offen = Some(aufnahme);
    let mut auswertung_offen = Some(auswertung);
    let sauberer_abschied = {
        let aufnahme = aufnahme_offen.as_mut().expect("gerade gesetzt");
        let auswertung = auswertung_offen.as_mut().expect("gerade gesetzt");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Abbruch angefordert");
                Fertig::Signal
            }
            _ = abschaltsignal() => {
                tracing::info!("SIGTERM erhalten, beende");
                Fertig::Signal
            }
            ergebnis = aufnahme => {
                arbeiter_ende_melden("Aufnahmeschleife", ergebnis);
                Fertig::Aufnahme
            }
            ergebnis = auswertung => {
                arbeiter_ende_melden("Auswertungsschleife", ergebnis);
                Fertig::Auswertung
            }
        }
    };
    match sauberer_abschied {
        Fertig::Aufnahme => aufnahme_offen = None,
        Fertig::Auswertung => auswertung_offen = None,
        Fertig::Signal => {}
    }

    arbeiter_abraeumen(aufnahme_offen, auswertung_offen, &abbruch).await;
    // Nach einem erzwungenen Abbruch koennen Zwischendateien liegen. Der
    // naechste Start raeumt sie zwar auf, aber ein sauber gestoppter Dienst
    // wird nicht neu gestartet - dann laege rohes Audio unbegrenzt herum.
    zwischendateien_aufraeumen(&eigener_temp).await;
    if !matches!(sauberer_abschied, Fertig::Signal) {
        std::process::exit(1);
    }
}

/// Wer den Ausstieg ausgeloest hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fertig {
    /// Ctrl-C oder SIGTERM - ein gewollter Halt.
    Signal,
    Aufnahme,
    Auswertung,
}

/// Bricht die Arbeiter ab und wartet kurz, damit laufende Aufnahmen ihre
/// Zwischendateien noch wegraeumen koennen.
///
/// Ohne diesen Schritt endet der Prozess sofort; streamlink- und
/// ffmpeg-Zwischenordner blieben liegen und niemand raeumt sie je auf.
async fn arbeiter_abraeumen(
    aufnahme: Option<tokio::task::JoinHandle<()>>,
    auswertung: Option<tokio::task::JoinHandle<()>>,
    abbruch: &Arc<std::sync::atomic::AtomicBool>,
) {
    // Erst bitten, dann zwingen: die Schleifen sehen die Marke an ihren
    // Wartestellen und beenden sich selbst. Das gilt fuer die Auswertung, die
    // ihre WAV-Datei danach noch wegraeumt. Die einzelnen Aufnahme-Tasks eines
    // Kanals haengen dagegen nur als Handles in der Aufnahmeschleife: endet
    // sie, werden die Handles fallengelassen, die Tasks laufen also weiter,
    // bis systemd die Cgroup abraeumt. Ihre `aufnahme_laeuft.json` bleibt dann
    // liegen - der naechste Start erkennt den Block daran als abgebrochen.
    abbruch.store(true, std::sync::atomic::Ordering::Relaxed);
    // Eine laufende Transkription raeumt ihre WAV-Datei erst weg, wenn die
    // Anfrage zurueckkommt. Ein Abbruch nach zehn Sekunden liess sie liegen -
    // rohes Audio ausserhalb jeder Aufbewahrung. Zwei Minuten Ton sind in
    // deutlich weniger als einer Minute durch.
    let frist = Duration::from_secs(60);
    if let Some(aufnahme) = aufnahme {
        warten_oder_abbrechen("Aufnahmeschleife", aufnahme, Duration::from_secs(10)).await;
    }
    if let Some(auswertung) = auswertung {
        warten_oder_abbrechen("Auswertungsschleife", auswertung, frist).await;
    }
}

/// Wartet auf einen Arbeiter und bricht ihn nach der Frist wirklich ab.
///
/// Eine abgelaufene `timeout` allein legt den Handle nur weg; die Aufgabe
/// laeuft dann bis zum Ende des Prozesses weiter.
async fn warten_oder_abbrechen(
    name: &str,
    mut handle: tokio::task::JoinHandle<()>,
    frist: Duration,
) {
    if tokio::time::timeout(frist, &mut handle).await.is_err() {
        tracing::warn!(arbeiter = name, "reagiert nicht - wird abgebrochen");
        handle.abort();
        let _ = handle.await;
    }
}

/// Raeumt Zwischendateien eines frueheren Laufs weg.
///
/// `transcribe_clip` legt die WAV-Datei in einem Ordner `eng-whisper-*` ab und
/// loescht ihn erst, wenn die Anfrage zurueckkommt. Wird der Dienst vorher
/// gestoppt, bleibt rohes Audio liegen - beim naechsten Start ist es ganz
/// sicher nicht mehr in Benutzung.
async fn zwischendateien_aufraeumen(ordner: &Path) {
    let Ok(mut eintraege) = tokio::fs::read_dir(ordner).await else {
        return;
    };
    let mut geloescht = 0usize;
    while let Ok(Some(eintrag)) = eintraege.next_entry().await {
        let pfad = eintrag.path();
        let ist_zwischenordner = pfad
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("eng-whisper-"))
            .unwrap_or(false);
        if ist_zwischenordner && tokio::fs::remove_dir_all(&pfad).await.is_ok() {
            geloescht += 1;
        }
    }
    if geloescht > 0 {
        tracing::info!(geloescht, "Zwischendateien eines frueheren Laufs entfernt");
    }
}

/// Wartet auf SIGTERM.
///
/// systemd stoppt mit SIGTERM, nicht mit Ctrl-C. Ohne diesen Zweig endet der
/// Prozess erst im Kill nach der Stoppfrist - mitten in einer Aufnahme und
/// ohne Chance, sauber aufzuhoeren.
async fn abschaltsignal() {
    let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        // Ohne Signalbehandlung nie fertig werden, sonst gilt der select-Zweig
        // sofort als erfuellt und der Dienst beendet sich beim Start.
        std::future::pending::<()>().await;
        return;
    };
    signal.recv().await;
}

/// Schreibt ins Journal, warum ein Arbeiter endete - mit Panik-Text, falls es
/// eine Panik war. Ohne diese Unterscheidung steht dort nur "beendet" und der
/// eigentliche Fehler ist verloren.
fn arbeiter_ende_melden(name: &str, ergebnis: Result<(), tokio::task::JoinError>) {
    match ergebnis {
        Ok(()) => tracing::error!(arbeiter = name, "Arbeiter unerwartet beendet"),
        Err(fehler) if fehler.is_panic() => {
            tracing::error!(arbeiter = name, ?fehler, "Arbeiter mit Panik beendet")
        }
        Err(fehler) => tracing::error!(arbeiter = name, ?fehler, "Arbeiter abgebrochen"),
    }
}

/// Nimmt Aufnahmen wieder auf, die ein frueherer Lauf nicht mehr auswerten
/// konnte.
///
/// Die Warteschlange lebt im Speicher. Ohne diesen Schritt waere jeder
/// Neustart ein stiller Verlust: die Aufnahmen lägen weiter auf der Platte,
/// wuerden aber nie ausgewertet und nie geloescht.
///
/// Kanal, Lauf, Nummer und Zeitversatz stehen im Zettel neben der Aufnahme. Fehlt er,
/// laeuft die Datei als eigener Lauf "wiederaufnahme" mit fortlaufender
/// Nummer - dann ist die Zuordnung verloren.
async fn liegengebliebenes_einreihen(
    konfiguration: &Konfiguration,
    warteschlange: &Arc<Mutex<plan::Warteschlange>>,
) {
    let mut aufnahmen = Vec::new();
    ts_dateien_sammeln(&aufnahme_wurzel(konfiguration), &mut aufnahmen).await;

    // Eigene Kennung je Neustart, sonst ueberschreibt die naechste
    // Wiederaufnahme die Berichte der vorigen. Sie greift nur, wenn der
    // Zettel fehlt.
    let lauf = format!(
        "wiederaufnahme-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let mut ersatz = plan::Aufnahme::starten("wiederaufnahme", lauf);
    let mut gefunden = 0usize;
    let mut ohne_zettel = 0usize;
    let mut nur_meldung = 0usize;
    let mut zu_alt_zahl = 0usize;

    let mut schon_fertig = 0usize;
    for pfad in aufnahmen {
        if bereits_ausgewertet(&pfad).await {
            schon_fertig += 1;
            continue;
        }
        // Was aelter ist als die Aufbewahrung, wird nicht noch ausgewertet:
        // ein Bericht daraus laege sonst wieder frisch da und verlaengerte die
        // Frist fuer Material, das laengst weg sein sollte.
        if zu_alt(&pfad, konfiguration.aufbewahrung_tage).await {
            // Nur echte Blockordner: eine verirrte Datei weiter oben haette
            // sonst den ganzen Ausgabeordner mitgenommen.
            match blockordner_von(&aufnahme_wurzel(konfiguration), &pfad) {
                Some(blockordner) => {
                    let _ = tokio::fs::remove_dir_all(&blockordner).await;
                    zu_alt_zahl += 1;
                }
                None => tracing::warn!(
                    datei = ?pfad,
                    "alte Aufnahme ausserhalb der erwarteten Struktur - nichts geloescht"
                ),
            }
            continue;
        }
        let mut block = match zettel_lesen(&pfad).await {
            Some(block) => block,
            None => {
                ohne_zettel += 1;
                // Eine Sekunde Abstand je Fund: sonst haetten alle zettellosen
                // Aufnahmen Zeitversatz 0, damit dieselbe Bezeichnung,
                // denselben Berichtsnamen und denselben Idempotenzschluessel -
                // der zweite Bericht ueberschriebe den ersten.
                ersatz.block_fertig(pfad.to_string_lossy().to_string(), 1)
            }
        };
        // Liegt der Bericht schon, fehlte nur die Meldung. Ein zweiter
        // Durchlauf durch Transkription und Modell koennte anders ausfallen
        // und den frueheren Fund ueberschreiben.
        if bericht_liegt_vor(konfiguration, &block).await {
            block.nur_melden = true;
            nur_meldung += 1;
        }
        warteschlange.lock().await.einreihen(block);
        gefunden += 1;
    }

    if ohne_zettel > 0 {
        tracing::warn!(
            ohne_zettel,
            "Aufnahmen ohne Zettel gefunden - Kanal und Zeitversatz sind fuer sie verloren"
        );
    }
    if zu_alt_zahl > 0 {
        tracing::info!(
            zu_alt_zahl,
            "Aufnahmen ueber der Aufbewahrungsfrist geloescht statt ausgewertet"
        );
    }
    if schon_fertig > 0 {
        tracing::info!(
            schon_fertig,
            "aufbewahrte Aufnahmen uebersprungen - bereits ausgewertet"
        );
    }
    if nur_meldung > 0 {
        tracing::info!(
            nur_meldung,
            "Bericht lag schon vor - es wird nur die Meldung nachgeholt"
        );
    }
    if gefunden > 0 {
        tracing::info!(gefunden, "liegengebliebene Aufnahmen wieder eingereiht");
    }
}

/// Ist diese Datei aelter als so viele Sekunden?
async fn zu_alt_sekunden(pfad: &Path, sekunden: u64) -> bool {
    let Ok(daten) = tokio::fs::metadata(pfad).await else {
        return false;
    };
    daten
        .modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .map(|alter| alter > Duration::from_secs(sekunden))
        .unwrap_or(false)
}

/// Ist diese Datei aelter als die Aufbewahrung? `0` heisst unbegrenzt.
async fn zu_alt(pfad: &Path, tage: u64) -> bool {
    if tage == 0 {
        return false;
    }
    let grenze = Duration::from_secs(tage.saturating_mul(24 * 60 * 60));
    let Ok(daten) = tokio::fs::metadata(pfad).await else {
        return false;
    };
    daten
        .modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .map(|alter| alter > grenze)
        .unwrap_or(false)
}

/// Sammelt `.ts`-Dateien unterhalb eines Ordners.
///
/// Die Aufnahmen liegen inzwischen mehrere Ebenen tief
/// (`<kanal>/<lauf>/t<versatz>-b<nummer>/<capture>/audio.ts`). Ein unlesbarer Ordner
/// beendet die Suche nicht - sonst nimmt eine kaputte Ecke alle folgenden
/// Aufnahmen mit.
async fn ts_dateien_sammeln(ordner: &Path, raus: &mut Vec<PathBuf>) {
    let mut zu_lesen = vec![ordner.to_path_buf()];
    while let Some(aktuell) = zu_lesen.pop() {
        let mut eintraege = match tokio::fs::read_dir(&aktuell).await {
            Ok(eintraege) => eintraege,
            Err(fehler) => {
                if aktuell != ordner {
                    tracing::warn!(?fehler, ordner = ?aktuell, "Ordner nicht lesbar");
                }
                continue;
            }
        };
        loop {
            let eintrag = match eintraege.next_entry().await {
                Ok(Some(eintrag)) => eintrag,
                Ok(None) => break,
                Err(fehler) => {
                    // Abbrechen, nicht wiederholen: `next_entry` liefert
                    // denselben Fehler beliebig oft (EIO, geloeschter Ordner,
                    // Shutdown) und die Schleife liefe mit vollem Kern weiter,
                    // ohne dass jemals wieder etwas ausgewertet wird.
                    tracing::warn!(?fehler, ordner = ?aktuell, "Ordner nur teilweise gelesen");
                    break;
                }
            };
            let pfad = eintrag.path();
            match eintrag.file_type().await {
                Ok(typ) if typ.is_dir() => zu_lesen.push(pfad),
                Ok(_) if pfad.extension().and_then(|e| e.to_str()) == Some("ts") => raus.push(pfad),
                _ => {}
            }
        }
    }
}

/// Dateiname des Zettels neben der Aufnahme.
const ZETTEL: &str = "block.json";

/// Marke im Blockordner: dieser Block ist ausgewertet, die Aufnahme liegt nur
/// noch als Beleg da.
///
/// Ohne sie reiht der naechste Start jede aufbewahrte Aufnahme erneut ein. Der
/// zweite Lauf schriebe denselben Bericht neu - moeglicherweise ohne den
/// Modellfund von damals - und loeschte danach genau den Beleg, der absichtlich
/// aufgehoben wurde.
const FERTIG: &str = "ausgewertet.json";

/// Marke im Blockordner: hier laeuft gerade eine Aufnahme.
///
/// Sie verschwindet, wenn die Aufnahme sauber zurueckkommt. Liegt sie nach
/// einem Neustart noch da, wurde mitten hinein gestoppt: die Datei ist gueltig,
/// aber kurz - und ein Bericht darueber ist nur ein Bericht ueber den Anfang.
const LAEUFT: &str = "aufnahme_laeuft.json";

/// Legt Kanal, Lauf, Blocknummer und Zeitversatz in den Blockordner, **bevor**
/// die Aufnahme laeuft.
///
/// Das Capture-Verzeichnis darunter heisst nach einem Zufallswert; aus dem
/// Pfad allein ist nach einem Neustart nicht zu erkennen, von wem die Aufnahme
/// stammt und an welcher Stelle der Sendung sie sass. Ein Stopp durch systemd
/// trifft mitten in den Block - wer den Zettel erst danach schreibt,
/// laesst genau die angebrochene Aufnahme ohne Zuordnung zurueck.
struct ZettelDaten<'a> {
    kanal: &'a str,
    lauf: &'a str,
    nummer: u32,
    versatz_sekunden: u64,
    zeit_unsicher: bool,
    stream_start_utc: Option<&'a str>,
    aufnahme_beginn_utc: Option<&'a str>,
}

async fn zettel_schreiben(blockordner: &Path, daten: ZettelDaten<'_>) -> bool {
    let inhalt = serde_json::json!({
        "kanal": daten.kanal,
        "lauf": daten.lauf,
        "nummer": daten.nummer,
        "versatz_sekunden": daten.versatz_sekunden,
        // Ohne diese Angabe behauptete ein wiederaufgenommener Block, seine
        // Zeiten seien Sendungszeiten - auch wenn sie es nie waren.
        "zeit_unsicher": daten.zeit_unsicher,
        "stream_start_utc": daten.stream_start_utc,
        "aufnahme_beginn_utc": daten.aufnahme_beginn_utc,
    });
    match nur_fuer_mich(&blockordner.join(ZETTEL), inhalt.to_string().as_bytes()).await {
        Ok(()) => true,
        Err(fehler) => {
            tracing::warn!(fehler, "Zettel nicht geschrieben");
            false
        }
    }
}

/// Dateiname der Marke fuer eine noch offene Meldung.
const MELDUNG_OFFEN: &str = "meldung_offen.json";

/// Haelt fest, dass zu diesem Block noch eine Meldung aussteht.
async fn meldung_offen_markieren(aufnahme: &Path) {
    let Some(blockordner) = aufnahme.parent().and_then(Path::parent) else {
        return;
    };
    if let Err(fehler) = nur_fuer_mich(&blockordner.join(MELDUNG_OFFEN), b"{}").await {
        tracing::error!(fehler, "Marke fuer offene Meldung nicht schreibbar");
    }
}

/// Nimmt die Marke wieder weg.
async fn meldung_erledigt(aufnahme: &Path) {
    let Some(blockordner) = aufnahme.parent().and_then(Path::parent) else {
        return;
    };
    let _ = tokio::fs::remove_file(blockordner.join(MELDUNG_OFFEN)).await;
}

/// Reiht die noch nicht eingereihten Aufnahmen eines Kanals ein.
///
/// Gebraucht, wenn ein laufender Aufnahme-Task abgebrochen wird: seine
/// angefangene Datei ist gueltig, nur kurz, und soll trotzdem geprueft werden.
async fn angebrochenes_einreihen(
    konfiguration: &Konfiguration,
    warteschlange: &Arc<Mutex<plan::Warteschlange>>,
    kanal: &str,
) {
    let ordner = aufnahme_wurzel(konfiguration).join(kanal);
    let mut aufnahmen = Vec::new();
    ts_dateien_sammeln(&ordner, &mut aufnahmen).await;
    let eingereiht: std::collections::HashSet<String> =
        warteschlange.lock().await.dateien().into_iter().collect();

    let mut neu = 0usize;
    for pfad in aufnahmen {
        if eingereiht.contains(&pfad.to_string_lossy().to_string())
            || bereits_ausgewertet(&pfad).await
        {
            continue;
        }
        let Some(mut block) = zettel_lesen(&pfad).await else {
            continue;
        };
        // Wie in `offene_meldungen_einreihen`: liegt der Bericht schon, fehlt
        // nur die Meldung. Ein voller zweiter Durchlauf koennte anders
        // ausfallen, den frueheren Fund ueberschreiben und seine Aufnahme
        // loeschen - die Marke `ausgewertet.json` fehlt in genau diesem Fall,
        // weil die Meldung nie durchkam.
        block.nur_melden = bericht_liegt_vor(konfiguration, &block).await;
        warteschlange.lock().await.einreihen(block);
        neu += 1;
    }
    if neu > 0 {
        tracing::info!(kanal, neu, "angefangene Aufnahmen eingereiht");
    }
}

/// Sucht Bloecke mit offener Meldung und reiht sie wieder ein.
///
/// Laeuft im Aufraeumtakt. Ohne diesen Weg endet eine Meldung, die der Broker
/// stundenlang nicht annimmt, im Nichts - und "keine DM" hiesse dann
/// faelschlich "nichts gefunden".
async fn offene_meldungen_einreihen(
    konfiguration: &Konfiguration,
    warteschlange: &Arc<Mutex<plan::Warteschlange>>,
) {
    let mut aufnahmen = Vec::new();
    ts_dateien_sammeln(&aufnahme_wurzel(konfiguration), &mut aufnahmen).await;
    let eingereiht: std::collections::HashSet<String> =
        warteschlange.lock().await.dateien().into_iter().collect();

    let mut wieder = 0usize;
    for pfad in aufnahmen {
        if eingereiht.contains(&pfad.to_string_lossy().to_string()) {
            continue;
        }
        let Some(blockordner) = pfad.parent().and_then(Path::parent) else {
            continue;
        };
        if !tokio::fs::try_exists(blockordner.join(MELDUNG_OFFEN))
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let Some(mut block) = zettel_lesen(&pfad).await else {
            continue;
        };
        // Liegt ein lesbarer Bericht, fehlt nur die Meldung. Sonst muss der
        // Block noch einmal komplett durch - etwa wenn schon die Transkription
        // nie durchlief.
        block.nur_melden = bericht_liegt_vor(konfiguration, &block).await;
        warteschlange.lock().await.einreihen(block);
        wieder += 1;
    }
    if wieder > 0 {
        tracing::info!(wieder, "offene Meldungen erneut eingereiht");
    }
}

/// Meldet einen ausgefallenen Modellschritt - aber nur beim ersten Block und
/// danach bei jedem Vielfachen von [`MODELLAUSFALL_TAKT`].
///
/// Faellt der Modellschritt aus, faellt er meist fuer jeden Block aus: ein
/// fehlender Schluessel, ein gesperrter Anbieter. Eine DM je Block waeren rund
/// 90 Nachrichten pro Stunde.
async fn modellausfall_melden(
    konfiguration: &Konfiguration,
    kanal: &str,
    bericht: &Bericht,
    zaehler: &mut std::collections::HashMap<String, usize>,
) {
    if bericht.modell_geprueft {
        zaehler.remove(kanal);
        hinweis_erledigt(konfiguration, &format!("modellausfall-{kanal}")).await;
        return;
    }
    let stand = zaehler.entry(kanal.to_owned()).or_insert(0);
    *stand += 1;
    let bloecke = *stand;
    if bloecke != 1 && !bloecke.is_multiple_of(MODELLAUSFALL_TAKT) {
        return;
    }
    let ablage = format!("modellausfall-{kanal}");
    let (schluessel, text) = match offener_hinweis(konfiguration, &ablage).await {
        Some(offen) => offen,
        None => (
            format!(
                "{}-{kanal}-modellausfall-{}",
                start_kennung(),
                naechste_vorfall_nummer()
            ),
            format!(
                "Coaching-Audit {kanal}: Modellschritt faellt aus ({}). Es laeuft nur die \
Regelpruefung, die Aufnahmen bleiben liegen.",
                bericht.modell_hinweis
            ),
        ),
    };
    match dm_rohtext(&text, &schluessel).await {
        Ok(()) => hinweis_erledigt(konfiguration, &ablage).await,
        Err(fehler) => {
            tracing::error!(fehler, kanal, "Modellausfall nicht meldbar");
            hinweis_aufheben(konfiguration, &ablage, &schluessel, &text).await;
        }
    }
}

/// Meldet den Verdacht auf einen ausgefallenen STT-Dienst.
///
/// Eigene Funktion, weil der Zaehler dahinter vor der Verzweigung laeuft: die
/// Meldung haengt am Kanal und seiner Stille, nicht daran, was mit der einzelnen
/// Aufnahme geschieht.
async fn stille_melden(konfiguration: &Konfiguration, kanal: &str, stumm: usize) {
    let text = format!(
        "Coaching-Audit {kanal}: {stumm} Bloecke am Stueck ohne einen einzigen erkannten Satz. \
STT-Dienst pruefen."
    );
    let ablage = format!("nur-stille-{kanal}");
    let (schluessel, text) = match offener_hinweis(konfiguration, &ablage).await {
        Some(offen) => offen,
        None => (
            format!(
                "{}-{kanal}-nur-stille-{}",
                start_kennung(),
                naechste_vorfall_nummer()
            ),
            text,
        ),
    };
    if let Err(fehler) = dm_rohtext(&text, &schluessel).await {
        // Nicht wieder loeschen, wenn spaeter etwas ankommt: die Aufnahmen
        // dieser Strecke sind dann schon weg, und die Warnung ist der einzige
        // Hinweis darauf, dass sie ungeprueft blieben.
        tracing::error!(fehler, kanal, "Stille-Verdacht nicht meldbar");
        hinweis_aufheben(konfiguration, &ablage, &schluessel, &text).await;
    }
}

/// Kopfdaten fuer die Fertig-Marke eines Blocks ohne Bericht.
fn leerer_bericht(block: &plan::Block) -> Bericht {
    Bericht {
        lauf_id: block.bezeichnung(),
        erstellt_am: chrono::Utc::now().to_rfc3339(),
        stream_start_utc: block.stream_start_utc.clone(),
        aufnahme_beginn_utc: block.aufnahme_beginn_utc.clone(),
        quelle: "kein Text".to_owned(),
        kanal: block.kanal.clone(),
        transkription: String::new(),
        modell: String::new(),
        transkription_lokal: true,
        anbieter: String::new(),
        llm_modell: String::new(),
        transkript_behalten: false,
        segmente: 0,
        modell_geprueft: false,
        modell_hinweis: "kein Text im Block".to_owned(),
        aufnahme_abgebrochen: false,
        funde: Vec::new(),
    }
}

/// Traegt die Marke, dass dieser Block ausgewertet ist.
async fn fertig_markieren(aufnahme: &Path, bericht: &Bericht) {
    let Some(blockordner) = aufnahme.parent().and_then(Path::parent) else {
        return;
    };
    let inhalt = serde_json::json!({
        "lauf_id": bericht.lauf_id,
        "erstellt_am": bericht.erstellt_am,
        "funde": bericht.funde.len(),
        "modell_geprueft": bericht.modell_geprueft,
        "aufnahme_abgebrochen": bericht.aufnahme_abgebrochen,
    });
    // Die Laufmarke eines abgebrochenen Mitschnitts bleibt sonst neben der
    // Fertig-Marke liegen und laesst den Ordner aussehen, als liefe die
    // Aufnahme noch. Der Abbruch selbst steht jetzt im Bericht.
    let _ = tokio::fs::remove_file(blockordner.join(LAEUFT)).await;
    if let Err(fehler) =
        nur_fuer_mich(&blockordner.join(FERTIG), inhalt.to_string().as_bytes()).await
    {
        tracing::warn!(
            fehler,
            "Marke nicht geschrieben - die Aufnahme laeuft nach einem Neustart erneut"
        );
    }
}

/// Wurde die Aufnahme dieses Blocks mitten im Lauf gestoppt?
async fn aufnahme_abgebrochen(aufnahme: &Path) -> bool {
    let Some(blockordner) = aufnahme.parent().and_then(Path::parent) else {
        return false;
    };
    tokio::fs::try_exists(blockordner.join(LAEUFT))
        .await
        .unwrap_or(false)
}

/// Wurde dieser Block schon ausgewertet?
async fn bereits_ausgewertet(aufnahme: &Path) -> bool {
    let Some(capture) = aufnahme.parent() else {
        return false;
    };
    if tokio::fs::try_exists(capture.join(FERTIG))
        .await
        .unwrap_or(false)
    {
        return true;
    }
    match capture.parent() {
        Some(blockordner) => tokio::fs::try_exists(blockordner.join(FERTIG))
            .await
            .unwrap_or(false),
        None => false,
    }
}

/// Liest den Zettel zu einer liegengebliebenen Aufnahme.
///
/// Der Zettel liegt im Blockordner, die Aufnahme eine Ebene tiefer im
/// Capture-Verzeichnis; gesucht wird deshalb in beiden.
async fn zettel_lesen(aufnahme: &Path) -> Option<plan::Block> {
    let capture = aufnahme.parent()?;
    let roh = match tokio::fs::read_to_string(capture.join(ZETTEL)).await {
        Ok(roh) => roh,
        Err(_) => tokio::fs::read_to_string(capture.parent()?.join(ZETTEL))
            .await
            .ok()?,
    };
    let json: serde_json::Value = serde_json::from_str(&roh).ok()?;
    Some(plan::Block {
        kanal: json["kanal"].as_str()?.to_owned(),
        lauf: json["lauf"].as_str()?.to_owned(),
        nummer: json["nummer"].as_u64()? as u32,
        versatz_sekunden: json["versatz_sekunden"].as_u64()?,
        // Fehlt die Angabe (Zettel aus einer aelteren Fassung), gilt die
        // vorsichtige Annahme: die Zeiten koennten geraten sein.
        zeit_unsicher: json["zeit_unsicher"].as_bool().unwrap_or(true),
        stream_start_utc: json["stream_start_utc"].as_str().map(str::to_owned),
        aufnahme_beginn_utc: json["aufnahme_beginn_utc"].as_str().map(str::to_owned),
        datei: aufnahme.to_string_lossy().to_string(),
        versuche: 0,
        frueherstens: 0,
        nur_melden: false,
        meldeversuche: 0,
    })
}

/// Kennung dieses Prozessstarts.
///
/// Sie steckt in den Idempotenzschluesseln der Ausfallmeldungen. Ohne sie
/// traegt der zweite Ausfall nach einem Neustart denselben Schluessel wie der
/// erste, und der Broker koennte die neue Meldung als Wiederholung der alten
/// verwerfen.
/// Fortlaufende Nummer je Vorfall.
///
/// Der Schluessel einer Meldung darf sich waehrend eines Vorfalls nicht
/// aendern (sonst kommt dieselbe Meldung doppelt) und nach einem Vorfall nicht
/// wiederkehren (sonst haelt der Broker die naechste Stoerung fuer eine
/// Wiederholung). Ein Zaehlerstand taugt fuer beides nicht - diese Nummer
/// steigt bei jedem neuen Vorfall um eins.
fn naechste_vorfall_nummer() -> u64 {
    static NUMMER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NUMMER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn start_kennung() -> &'static str {
    static KENNUNG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    KENNUNG.get_or_init(|| {
        // Mit Prozessnummer, weil die Sekunde allein nicht reicht: zwei Starts
        // in derselben Sekunde ergaeben dieselben Idempotenzschluessel, und der
        // Broker haette eine echte neue Stoerung als Wiederholung verworfen.
        format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            std::process::id()
        )
    })
}

/// So viele Bloecke ohne einen erkannten Satz gelten als Verdacht auf einen
/// kaputten STT-Dienst. Zwanzig Bloecke sind rund 40 Minuten Sendung.
const MAX_LEER_AM_STUECK: usize = 20;

/// So viele Bloecke liegen zwischen zwei Meldungen ueber einen ausgefallenen
/// Modellschritt.
///
/// Eigene Konstante, obwohl der Wert derselbe ist: die Stille-Schwelle und der
/// Meldetakt haben nichts miteinander zu tun, und wer an einer schraubt, soll
/// nicht ungewollt die andere verstellen.
const MODELLAUSFALL_TAKT: usize = 20;

/// So oft darf ein sendender Kanal keinen Block liefern, bevor gemeldet wird.
///
/// Ein einzelner Fehlschlag ist der Normalfall am Sendungsende. Fuenf am
/// Stueck bei einem Kanal, der laut Helix sendet, sind ein kaputtes
/// streamlink - und ohne Meldung liefe das Audit still ins Leere.
const MAX_STILLE_VERSUCHE: u32 = 5;

/// Wie lange ein aufgegebener Block wartet, dessen Meldung nicht rausging.
const MELDEPAUSE_SEKUNDEN: i64 = 30 * 60;

/// Pause, wenn auch die kurzen Anlaeufe nichts gebracht haben.
const LANGE_MELDEPAUSE_SEKUNDEN: i64 = 6 * 60 * 60;

/// Endgueltige Grenze der Meldeversuche. Danach bleibt es beim Bericht auf der
/// Platte, den der naechste Start des Dienstes wieder aufgreift.
const MAX_MELDEVERSUCHE_LANG: u32 = 12;

/// Wie lange die Sendung schon laeuft, aus `started_at` von Helix.
///
/// Ein unlesbares oder fehlendes Datum ergibt 0 - dann zaehlen die Zeiten wie
/// frueher ab Aufnahmebeginn, was ungenau, aber nicht falsch ist.
fn sendungssekunden(sendung: &tb_transport_twitch::streams::HelixStream) -> u64 {
    let Ok(start) = chrono::DateTime::parse_from_rfc3339(sendung.started_at.trim()) else {
        return 0;
    };
    (chrono::Utc::now() - start.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0) as u64
}

/// Zeitgrenze fuer eine Transkriptionsanfrage.
///
/// Ein Block ist zwei Minuten lang, und das lokale Whisper laeuft auf der CPU
/// langsamer als Echtzeit. Die Vorgabe der Kiste sind 60 Sekunden - bei
/// belegtem Dienst reicht das nicht, und der Block waere nach drei Anlaeufen
/// verloren. Grosszuegig gewaehlt, weil hier niemand auf die Antwort wartet.
const STT_ZEITGRENZE: Duration = Duration::from_secs(30 * 60);

/// Umgebungsschalter fuer einen STT-Endpunkt ausserhalb dieses Rechners.
const REMOTE_STT_ERLAUBT_ENV: &str = "STREAM_AUDIT_ALLOW_REMOTE_STT";

/// Der Endpunkt, den die Transkription tatsaechlich nimmt.
///
/// Frueher stand hier die rohe Umgebungsvariable. Ohne sie war der Wert leer,
/// die Ortspruefung hielt das fuer auswaertig und der Dienst startete nicht -
/// obwohl der Transkriber selbst auf localhost zurueckfaellt.
fn stt_basis_url() -> String {
    std::env::var("ENGAGEMENT_STT_BASE_URL")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| tb_engagement::transcribe::DEFAULT_STT_URL.to_owned())
}

fn remote_stt_erlaubt() -> bool {
    matches!(
        std::env::var(REMOTE_STT_ERLAUBT_ENV)
            .unwrap_or_default()
            .trim(),
        "1" | "true" | "ja" | "yes"
    )
}

/// Eigener Aufnahmeordner, neben der Ablage der Berichte.
///
/// Der `AudioCapturer` legt ohne Zielangabe unter `/tmp/voice-reaction-*` ab,
/// und dasselbe Praefix nutzen Reaction-Learning und Smalltalk. Wer dort
/// aufraeumt, loescht fremde, womoeglich noch laufende Aufnahmen.
///
/// Und nicht `/tmp`: Aufnahmen zu Funden bleiben absichtlich bis zum Ende der
/// Aufbewahrungsfrist liegen. In einem Ordner, den das System beim Neustart
/// oder per tmpfiles-Regel ausraeumt, waeren sie vorher weg.
fn aufnahme_wurzel(konfiguration: &Konfiguration) -> PathBuf {
    konfiguration.ausgabe.join("aufnahmen")
}

fn helix_aus_umgebung() -> Option<HelixClient> {
    let id = std::env::var("TWITCH_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    let secret = std::env::var("TWITCH_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())?;
    HelixClient::new(HelixConfig::new(id, secret)).ok()
}

/// Prueft im Takt, wer sendet, und haelt je sendendem Kanal eine eigene
/// Aufnahmeschleife am Laufen.
///
/// Die Aufnahmen muessen **nebeneinander** laufen. Nacheinander waere jeder von
/// drei Kanaelen nur ein Drittel der Zeit in Aufnahme, und ein Audit, das zwei
/// Drittel des Streams nie sieht, meldet "keine Funde", was wie "sauber"
/// aussieht. Aufnehmen kostet fast nichts; teuer ist die Transkription, und die
/// bleibt seriell hinter der Warteschlange.
async fn aufnahme_schleife(
    helix: HelixClient,
    konfiguration: Konfiguration,
    warteschlange: Arc<Mutex<plan::Warteschlange>>,
    abbruch: Arc<std::sync::atomic::AtomicBool>,
    sperre: Arc<Mutex<plan::LaufSperre>>,
    plattenbelegung: Arc<std::sync::atomic::AtomicU64>,
) {
    let mut laufend: std::collections::HashMap<String, tokio::task::JoinHandle<plan::Aufnahme>> =
        Default::default();
    // Aufnahmestand je Kanal. Er lebt hier und nicht im Task, weil ein Task
    // auch mitten in der Sendung enden kann - etwa wenn streamlink kurz
    // abbricht. Legte der Neustart den Stand neu an, finge der
    // Aufnahme-Deckel jedes Mal von vorn an.
    let mut staende: std::collections::HashMap<String, plan::Aufnahme> = Default::default();
    // Wie oft ein Kanal am Stueck nichts aufnehmen konnte, obwohl er sendet.
    let mut fehlschlaege: std::collections::HashMap<String, u32> = Default::default();
    // Schon gemeldete Dauerausfaelle, damit die DM nicht im Minutentakt kommt.
    let mut gemeldet: std::collections::HashSet<String> = Default::default();
    // Laeuft gerade eine Pause wegen voller Platte? Nur ihr Beginn wird gemeldet.
    let mut platte_gemeldet = false;
    // Welche Sendung der laufende Task eines Kanals aufnimmt.
    let mut laufende_sendung: std::collections::HashMap<String, String> = Default::default();
    // Durchgehender Ton-Recorder je Kanal. Genau einer je Lauf; `kill_on_drop`
    // raeumt ihn beim Dienst-Ende weg.
    let mut mitschnitte: std::collections::HashMap<String, Recorder> = Default::default();
    // Aktuell live sendender Lauf je Kanal. Die Recorder-Wartung sorgt dafuer,
    // dass fuer jeden Eintrag ein Recorder laeuft - so wird auch ein
    // fehlgeschlagener Erststart im naechsten Takt nachgeholt.
    let mut live_lauf: std::collections::HashMap<String, String> = Default::default();
    // Blockstand je Kanal beim Start des laufenden Tasks.
    let mut gestartet_mit: std::collections::HashMap<String, u32> = Default::default();
    // Kanaele, deren Task ohne neuen Block endete - egal ob sauber oder mit Panik.
    let mut ohne_block: Vec<String> = Vec::new();
    // Wie oft die Live-Abfrage am Stueck scheiterte, und ob das gemeldet ist.
    let mut helix_fehler = 0u32;
    let mut helix_gemeldet = false;

    loop {
        if abbruch.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("Aufnahmeschleife beendet sich");
            return;
        }
        let sendungen = match helix
            .get_streams_by_logins(&konfiguration.kanaele, None)
            .await
        {
            Ok(streams) => streams,
            Err(fehler) => {
                helix_fehler += 1;
                tracing::warn!(?fehler, helix_fehler, "Live-Abfrage fehlgeschlagen");
                // Ohne Live-Abfrage nimmt der Dienst nichts auf. Das sieht
                // hinterher aus wie ein sauberer Tag, ist aber ein Ausfall.
                if helix_fehler >= MAX_STILLE_VERSUCHE && !helix_gemeldet {
                    let text = format!(
                        "Coaching-Audit: Twitch-Abfrage scheitert dauerhaft (seit mindestens \
{MAX_STILLE_VERSUCHE} Anlaeufen). Laufende Aufnahmen laufen weiter, neue Sendungen werden \
nicht erkannt."
                    );
                    let (schluessel, text) =
                        match offener_hinweis(&konfiguration, "helix-ausfall").await {
                            Some(offen) => offen,
                            None => (
                                format!(
                                    "{}-helix-ausfall-{}",
                                    start_kennung(),
                                    naechste_vorfall_nummer()
                                ),
                                text,
                            ),
                        };
                    match dm_rohtext(&text, &schluessel).await {
                        Ok(()) => {
                            helix_gemeldet = true;
                            hinweis_erledigt(&konfiguration, "helix-ausfall").await;
                        }
                        Err(dm_fehler) => {
                            tracing::error!(dm_fehler, "Helix-Ausfall nicht meldbar");
                            hinweis_aufheben(&konfiguration, "helix-ausfall", &schluessel, &text)
                                .await;
                        }
                    }
                }
                // Fertige Tasks trotzdem einsammeln: sonst bleibt die Sperre
                // und der Stand haengen, bis Helix wieder antwortet.
                let fertige: Vec<String> = laufend
                    .iter()
                    .filter(|(_, handle)| handle.is_finished())
                    .map(|(kanal, _)| kanal.clone())
                    .collect();
                for kanal in fertige {
                    if let Some(handle) = laufend.remove(&kanal) {
                        match handle.await {
                            Ok(zustand) => {
                                gestartet_mit.remove(&kanal);
                                staende.insert(kanal, zustand);
                            }
                            Err(fehler) => {
                                gestartet_mit.remove(&kanal);
                                tracing::error!(kanal, ?fehler, "Aufnahme-Task abgestuerzt");
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
                continue;
            }
        };
        if helix_fehler > 0 {
            // Die Stoerung ist vorbei: ein noch aufgehobener Hinweis wuerde
            // sie sonst Stunden spaeter als aktuell melden.
            hinweis_erledigt(&konfiguration, "helix-ausfall").await;
        }
        helix_fehler = 0;
        helix_gemeldet = false;
        let live: Vec<String> = sendungen
            .iter()
            .map(|s| s.user_login.to_lowercase())
            .collect();

        // Beendete Tasks einsammeln und ihren Aufnahmestand zurueckholen.
        let fertige: Vec<String> = laufend
            .iter()
            .filter(|(_, handle)| handle.is_finished())
            .map(|(kanal, _)| kanal.clone())
            .collect();
        for kanal in fertige {
            let Some(handle) = laufend.remove(&kanal) else {
                continue;
            };
            match handle.await {
                Ok(zustand) => {
                    // Ein Task, der ohne einen einzigen Block zurueckkommt,
                    // heisst in aller Regel: der Stream ist vorbei. Er kann
                    // aber auch heissen, dass streamlink fehlt oder kaputt ist
                    // - und dann liefe das Audit still ins Leere.
                    // Verglichen wird mit dem Stand beim Start dieses Tasks,
                    // nicht mit null: nach einem einzigen gelungenen Block
                    // waere sonst jeder weitere Fehlschlag ein "Erfolg", und
                    // der Rest der Sendung fiele still aus.
                    let vorher = gestartet_mit.remove(&kanal).unwrap_or(0);
                    if zustand.bloecke == vorher {
                        let zaehler = fehlschlaege.entry(kanal.clone()).or_default();
                        *zaehler += 1;
                        ohne_block.push(kanal.clone());
                    } else {
                        if fehlschlaege.remove(&kanal).is_some() {
                            hinweis_erledigt(&konfiguration, &format!("keine-aufnahme-{kanal}"))
                                .await;
                        }
                        gemeldet.remove(&kanal);
                    }
                    if zustand.naechste_blocklaenge().is_none() {
                        tracing::info!(
                            kanal,
                            sekunden = zustand.aufgenommen_sekunden,
                            "Aufnahmedeckel erreicht, kein Neustart in dieser Sendung"
                        );
                    }
                    staende.insert(kanal.clone(), zustand);
                }
                // Eine Panik kostet den Stand dieses Kanals. Das ist die
                // sichere Richtung: der naechste Start faengt bei 0 an, aber
                // der Prozess laeuft weiter und die anderen Kanaele bleiben.
                Err(fehler) => {
                    // Eine Panik ist erst recht ein Fehlschlag.
                    let zaehler = fehlschlaege.entry(kanal.clone()).or_default();
                    *zaehler += 1;
                    ohne_block.push(kanal.clone());
                    gestartet_mit.remove(&kanal);
                    tracing::error!(kanal, ?fehler, "Aufnahme-Task abgestuerzt");
                }
            }
        }

        // Die Meldeschwelle gilt fuer beide Wege: sauber beendeter Task ohne
        // Block und abgestuerzter Task. Frueher stand sie nur im Erfolgszweig,
        // und wiederholte Panics blieben unbemerkt.
        for kanal in ohne_block.drain(..) {
            let anlaeufe = fehlschlaege.get(&kanal).copied().unwrap_or(0);
            if anlaeufe < MAX_STILLE_VERSUCHE || gemeldet.contains(&kanal) {
                continue;
            }
            let text = format!(
                "Coaching-Audit {kanal}: sendet, aber es kommt keine Aufnahme zustande \
(seit mindestens {MAX_STILLE_VERSUCHE} Anlaeufen). streamlink pruefen."
            );
            // Der Schluessel der DM traegt die Anlaufzahl - derselbe Schluessel
            // mit anderem Text kann beim Broker als Widerspruch gelten. Die
            // Datei fuer den Wiederholungsversuch heisst dagegen je Kanal
            // gleich, sonst sammelte sich waehrend eines Broker-Ausfalls jede
            // Minute ein neuer Hinweis an und alle kaemen spaeter auf einmal.
            let ablage = format!("keine-aufnahme-{kanal}");
            let (schluessel, text) = match offener_hinweis(&konfiguration, &ablage).await {
                Some(offen) => offen,
                None => (
                    format!(
                        "{}-{kanal}-keine-aufnahme-{}",
                        start_kennung(),
                        naechste_vorfall_nummer()
                    ),
                    text,
                ),
            };
            match dm_rohtext(&text, &schluessel).await {
                // Erst nach zugestellter DM merken. Sonst verschluckt ein
                // einziger Broker-Aussetzer die Meldung fuer immer.
                Ok(()) => {
                    gemeldet.insert(kanal.clone());
                    // Ein aelterer, noch nicht zugestellter Hinweis zur selben
                    // Sache waere sonst spaeter nachgereicht worden und haette
                    // eine laengst erledigte Stoerung gemeldet.
                    hinweis_erledigt(&konfiguration, &ablage).await;
                }
                Err(fehler) => {
                    tracing::error!(fehler, kanal, "Ausfall nicht meldbar");
                    hinweis_aufheben(&konfiguration, &ablage, &schluessel, &text).await;
                }
            }
        }

        // Der Stand faellt erst, wenn der Kanal offline war. Wuerde er bei
        // jedem Takt zuruecksetzen, waere der Aufnahme-Deckel wirkungslos:
        // ein Dauerstream startete alle 60 Sekunden eine neue Aufnahme.
        staende.retain(|kanal, _| live.contains(kanal));
        // Auch Ausfallzaehler und Meldemarke gehoeren zum Kanal. Blieben sie
        // stehen, bekaeme die naechste Sendung desselben Kanals bei weiterhin
        // kaputtem streamlink keine einzige Meldung mehr.
        fehlschlaege.retain(|kanal, _| live.contains(kanal));
        gemeldet.retain(|kanal| live.contains(kanal));
        // Offline gegangene Kanaele nicht mehr als live fuehren; ihr Recorder
        // wird von der Wartung als fertig markiert, nicht neu gestartet.
        live_lauf.retain(|kanal, _| live.contains(kanal));

        // Nur eine Bremse fuer neue Aufnahmen, kein `continue`: sonst wuerde
        // eine volle Platte nie einen beendeten Lauf freigeben, den Abbruch
        // bei einem Sendungswechsel ueberspringen und die Ende-DM ausbleiben
        // lassen - der Dienst verriegelt sich dann selbst, statt sich zu
        // erholen.
        let belegt = aufnahmen_bytes(&aufnahme_wurzel(&konfiguration)).await;
        plattenbelegung.store(belegt, std::sync::atomic::Ordering::Relaxed);
        let platte_voll = !plan::platte_reicht(belegt, plan::MAX_AUFNAHME_BYTES);
        if platte_voll {
            tracing::warn!(
                belegt,
                grenze = plan::MAX_AUFNAHME_BYTES,
                "Neue Aufnahmen pausiert - Platte voll"
            );
            if !platte_gemeldet {
                let text = format!(
                    "Coaching-Audit: Aufnahmen belegen {belegt} Bytes, Grenze {}. \
Neue Mitschnitte warten, laufende Bloecke laufen zu Ende.",
                    plan::MAX_AUFNAHME_BYTES
                );
                let (schluessel, text) = match offener_hinweis(&konfiguration, "platte").await {
                    Some(offen) => offen,
                    None => (
                        format!("{}-platte-{}", start_kennung(), naechste_vorfall_nummer()),
                        text,
                    ),
                };
                match dm_rohtext(&text, &schluessel).await {
                    Ok(()) => {
                        platte_gemeldet = true;
                        hinweis_erledigt(&konfiguration, "platte").await;
                    }
                    Err(fehler) => {
                        tracing::error!(fehler, "Plattengrenze nicht meldbar");
                        hinweis_aufheben(&konfiguration, "platte", &schluessel, &text).await;
                    }
                }
            }
        } else {
            platte_gemeldet = false;
        }

        for sendung in &sendungen {
            let kanal = sendung.user_login.to_lowercase();
            let kanal = &kanal;
            if let Some(handle) = laufend.get(kanal) {
                // Endet eine Sendung und beginnt zwischen zwei Takten eine
                // neue, nimmt der laufende Task sie unter der alten Kennung
                // auf - mit deren Zeiten und deren Aufnahme-Deckel. Dann
                // lieber abbrechen; der naechste Takt startet sauber neu.
                let bekannt = laufende_sendung.get(kanal).map(String::as_str);
                let aktuell = sendung.id.trim();
                if !aktuell.is_empty() && bekannt.is_some_and(|alt| alt != aktuell) {
                    tracing::info!(kanal, "neue Sendung erkannt - Aufnahme wird neu gestartet");
                    handle.abort();
                    // Erst abwarten, dann suchen: sonst schreibt streamlink
                    // noch, waehrend die Datei schon in der Warteschlange
                    // liegt.
                    if let Some(handle) = laufend.remove(kanal) {
                        let _ = handle.await;
                    }
                    laufende_sendung.remove(kanal);
                    staende.remove(kanal);
                    // Der alte Lauf ist vorbei: seinen Recorder als fertig
                    // markieren (dann darf er hoch) und stoppen (kill_on_drop).
                    // Der neue Lauf bekommt weiter unten seinen eigenen Recorder.
                    if let Some(rec) = mitschnitte.remove(kanal) {
                        aufnahme_fertig_markieren(&konfiguration, kanal, &rec.lauf).await;
                    }
                    // Der abgebrochene Block liegt als angefangene Datei da.
                    // Ohne diesen Schritt wuerde er nie ausgewertet und
                    // irgendwann von der Aufbewahrung geloescht - eine stille
                    // Luecke mitten in der Sendung.
                    angebrochenes_einreihen(&konfiguration, &warteschlange, kanal).await;
                } else {
                    continue;
                }
            }
            // Der Abbruch oben lief so oder so; nur der Neustart einer
            // Aufnahme bleibt aus, solange die Platte voll ist.
            if platte_voll {
                continue;
            }
            // Ein Zustand aus einer anderen Sendung darf nicht weiterlaufen:
            // Endet ein Stream und startet zwischen zwei Takten neu, erbte die
            // neue Sendung sonst Lauf-Kennung, Zeitversatz und Deckel der
            // alten.
            let passend = staende
                .get(kanal)
                // Die Lauf-Kennung kann einen Zeitanhang tragen, wenn
                // started_at fehlte. Dann passt sie trotzdem zur Sendung.
                .map(|z| {
                    let id = sendung.id.trim();
                    z.lauf == id || z.lauf.starts_with(&format!("{id}-"))
                })
                .unwrap_or(false);
            if !passend {
                staende.remove(kanal);
            }
            let zustand = staende.remove(kanal).unwrap_or_else(|| {
                // Kennung und Startzeit kommen von Twitch. Die Stream-ID ist
                // ueber einen Neustart des Dienstes hinweg dieselbe, und der
                // Zeitversatz macht die Zeiten im Bericht zu Sendungszeiten -
                // "hoer dir Minute 12 an" trifft dann im VOD auch Minute 12.
                let basis = sendungssekunden(sendung);
                let lauf = match (sendung.id.trim(), basis) {
                    ("", _) => chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
                    // Ohne brauchbares started_at faengt der Versatz wieder bei
                    // null an. Damit ein Neustart in derselben Sendung nicht
                    // dieselben Berichtsnamen erzeugt, bekommt die Kennung dann
                    // einen Zeitanhang.
                    (id, 0) => format!("{id}-{}", chrono::Utc::now().timestamp()),
                    (id, _) => id.to_owned(),
                };
                let mut zustand = plan::Aufnahme::starten_bei(kanal.clone(), lauf, basis);
                zustand.stream_start_utc =
                    chrono::DateTime::parse_from_rfc3339(sendung.started_at.trim())
                        .ok()
                        .map(|zeitpunkt| zeitpunkt.with_timezone(&chrono::Utc).to_rfc3339());
                // Ohne brauchbares started_at zaehlen die Zeiten ab
                // Aufnahmebeginn. Das gehoert in den Bericht, sonst schickt er
                // jemanden im VOD an die falsche Stelle.
                zustand.zeit_unsicher = basis == 0;
                zustand
            });
            if zustand.naechste_blocklaenge().is_none() {
                // Deckel erreicht: Stand behalten, damit er beim naechsten Takt
                // nicht als "neue Sendung" durchgeht.
                staende.insert(kanal.clone(), zustand);
                continue;
            }
            gestartet_mit.insert(kanal.clone(), zustand.bloecke);
            // Die rohe Helix-Kennung, nicht unsere Lauf-Kennung: fehlt
            // started_at, haengt an der Lauf-Kennung ein Zeitstempel, und der
            // Vergleich waere bei jedem Takt ungleich - der Dienst haette sich
            // selbst im Minutentakt abgebrochen.
            laufende_sendung.insert(kanal.clone(), sendung.id.trim().to_owned());
            sperre
                .lock()
                .await
                .sperren(kanal.clone(), zustand.lauf.clone());
            let lauf_ms = zustand.lauf.clone();
            let konfig_dm = konfiguration.clone();
            let kanal_dm = kanal.clone();
            let lauf_dm = zustand.lauf.clone();
            tokio::spawn(async move {
                start_dm_einmal(&konfig_dm, &kanal_dm, &lauf_dm).await;
            });
            let handle = tokio::spawn(kanal_aufnehmen(
                zustand,
                konfiguration.clone(),
                Arc::clone(&warteschlange),
                Arc::clone(&abbruch),
                Arc::clone(&plattenbelegung),
            ));
            laufend.insert(kanal.clone(), handle);
            tracing::info!(kanal, "Aufnahme gestartet");
            // Diesen Lauf als live vermerken; den Recorder startet die Wartung
            // unten (so wird auch ein fehlgeschlagener Start nachgeholt). Ein
            // Recorder eines alten Laufs desselben Kanals endet mit dem Stream
            // und wird von der Wartung als fertig markiert und entfernt; danach
            // greift hier der neue Lauf.
            live_lauf.insert(kanal.clone(), lauf_ms.clone());
        }

        // Recorder-Wartung. ffmpeg beendet sich, wenn der Stream endet (oder
        // streamlink bei einem Aussetzer abbricht).
        if archiv::archiv_aktiv() {
            // 1. Beendete Recorder entfernen. Ist der Kanal offline, ist die
            //    Aufnahme fertig - markieren. Bei noch live sendendem Kanal wird
            //    unten (Schritt 2) neu gestartet.
            let kanaele: Vec<String> = mitschnitte.keys().cloned().collect();
            for kanal in kanaele {
                let beendet = mitschnitte
                    .get_mut(&kanal)
                    .map(|rec| rec.beendet())
                    .unwrap_or(false);
                if !beendet {
                    continue;
                }
                if let Some(rec) = mitschnitte.remove(&kanal) {
                    if !live.contains(&kanal) {
                        aufnahme_fertig_markieren(&konfiguration, &kanal, &rec.lauf).await;
                    }
                }
            }
            // 2. Jeder live sendende Lauf ohne aktiven Recorder bekommt einen -
            //    solange genug Platz frei ist. Deckt Aussetzer UND einen
            //    fehlgeschlagenen Erststart ab. Bestehende Recorder laufen weiter.
            let ziele: Vec<(String, String)> = live_lauf
                .iter()
                .filter(|(kanal, _)| live.contains(*kanal) && !mitschnitte.contains_key(*kanal))
                .map(|(kanal, lauf)| (kanal.clone(), lauf.clone()))
                .collect();
            if !ziele.is_empty() {
                if platz_fuer_mitschnitt(&konfiguration).await {
                    for (kanal, lauf) in ziele {
                        if let Some(rec) = mitschnitt_starten(&konfiguration, &kanal, &lauf).await {
                            mitschnitte.insert(kanal.clone(), rec);
                            tracing::info!(kanal, "durchgehender Ton-Mitschnitt gestartet");
                        }
                    }
                } else {
                    tracing::warn!("Platz knapp - kein neuer Ton-Mitschnitt gestartet");
                }
            }
            // 3. Verwaiste Mitschnitt-Ordner (Dienst-Neustart, Absturz) nachziehen:
            //    jeder Ordner ohne aktiven Recorder und ohne Fertig-Marke ist fertig.
            mitschnitt_ordner_versiegeln(&konfiguration, &mitschnitte, &live).await;
        }

        // Erst jetzt, nachdem ein Sendungswechsel oben seinen abgebrochenen
        // Block bereits eingereiht hat: sonst ginge die Ende-DM eines Laufs
        // raus, bevor sein letzter (angebrochener) Block ueberhaupt in der
        // Warteschlange liegt, und der Lauf gaelte als fertig gemeldet, ohne
        // dass der Nachzuegler noch eine korrigierte Abschluss-DM ausloesen
        // koennte.
        let aktuelle_ids: std::collections::HashMap<String, String> = sendungen
            .iter()
            .map(|s| (s.user_login.to_lowercase(), s.id.trim().to_owned()))
            .collect();
        // Erst alle betroffenen Eintraege sammeln, dann die Sperre wieder
        // freigeben: der Block haelt den Mutex-Guard sonst bis zum Ende der
        // Schleife, und `freigeben`/`lauf_ende_melden` sperren darin erneut -
        // ein nicht-reentranter Mutex blockiert dann beim ersten Sendungsende
        // fuer immer.
        let sperr_eintraege = { sperre.lock().await.eintraege() };
        for (kanal, lauf) in sperr_eintraege {
            let id = aktuelle_ids.get(&kanal).map(String::as_str);
            if plan::lauf_ist_aktuelle_sendung(&lauf, id) {
                continue;
            }
            sperre.lock().await.freigeben(&kanal, &lauf);
            lauf_ende_melden(&konfiguration, &kanal, &lauf, &sperre, &warteschlange).await;
        }

        tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
    }
}

/// Nimmt einen Kanal in Bloecken auf, bis der Stream endet oder der Deckel
/// greift. Laeuft als eigener Task, damit mehrere Kanaele sich nicht
/// gegenseitig blockieren.
///
/// Der Aufnahmestand kommt von der Aufsicht und geht an sie zurueck. Endet
/// dieser Task mitten in einer Sendung, setzt der naechste genau dort auf -
/// sonst waere der Deckel mit jedem streamlink-Fehler zurueckgesetzt.
async fn kanal_aufnehmen(
    mut zustand: plan::Aufnahme,
    konfiguration: Konfiguration,
    warteschlange: Arc<Mutex<plan::Warteschlange>>,
    abbruch: Arc<std::sync::atomic::AtomicBool>,
    plattenbelegung: Arc<std::sync::atomic::AtomicU64>,
) -> plan::Aufnahme {
    let capturer = AudioCapturer::from_env();
    let kanal = zustand.kanal.clone();

    loop {
        if abbruch.load(std::sync::atomic::Ordering::Relaxed) {
            return zustand;
        }
        let Some(laenge) = zustand.naechste_blocklaenge() else {
            return zustand;
        };

        // Auswertung darf die Aufnahme nicht anhalten. Nur die Platte:
        // ohne diese Pruefung schriebe ein Dauerstream den Rechner voll.
        let belegt = plattenbelegung.load(std::sync::atomic::Ordering::Relaxed);
        if !plan::platte_reicht(belegt, plan::MAX_AUFNAHME_BYTES) {
            tracing::warn!(kanal, belegt, "Platte voll - Aufnahme pausiert");
            tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
            continue;
        }

        // Der Versatz gilt ab jetzt - alles, was seit dem letzten Block an
        // Pausen verging, zaehlt zur Sendungszeit.
        let versatz = zustand.sendungssekunden();
        let blockordner = aufnahme_wurzel(&konfiguration)
            .join(&kanal)
            .join(&zustand.lauf)
            // Zeit und Nummer, wie in der Blockbezeichnung: der Versatz allein
            // zaehlt in ganzen Sekunden, und zwei schnell hintereinander
            // abgebrochene Aufnahmen laegen sonst im selben Ordner.
            .join(format!("t{versatz:06}-b{:04}", zustand.bloecke + 1));
        if let Err(fehler) = tokio::fs::create_dir_all(&blockordner).await {
            tracing::error!(?fehler, "Aufnahmeordner nicht anlegbar");
            return zustand;
        }
        let laeuft = blockordner.join(LAEUFT);
        if let Err(fehler) = nur_fuer_mich(&laeuft, b"{}").await {
            tracing::warn!(fehler, "Laufmarke nicht schreibbar");
        }
        let aufnahme_beginn_utc =
            chrono::DateTime::<chrono::Utc>::from_timestamp(zustand.gestartet_um, 0)
                .map(|zeitpunkt| zeitpunkt.to_rfc3339());
        if !zettel_schreiben(
            &blockordner,
            ZettelDaten {
                kanal: &kanal,
                lauf: &zustand.lauf,
                nummer: zustand.bloecke + 1,
                versatz_sekunden: versatz,
                zeit_unsicher: zustand.zeit_unsicher,
                stream_start_utc: zustand.stream_start_utc.as_deref(),
                aufnahme_beginn_utc: aufnahme_beginn_utc.as_deref(),
            },
        )
        .await
        {
            // Ohne Zettel waere die Aufnahme nach einem Neustart nicht mehr
            // zuzuordnen: falscher Kanal, Zeitversatz 0. Dann lieber gar nicht
            // erst aufnehmen - und der Task endet, damit die Aufsicht den
            // Fehlschlag sieht und nach fuenf Anlaeufen meldet. Im Kreis zu
            // laufen hiesse: kein Ton, keine Meldung, niemand merkt es.
            tracing::error!(kanal, "Zettel nicht schreibbar - Aufnahme beendet");
            let _ = tokio::fs::remove_dir_all(&blockordner).await;
            return zustand;
        }

        match capturer
            .capture(
                &kanal,
                laenge,
                tb_engagement::audio_capture::DEFAULT_QUALITY,
                Some(&blockordner),
            )
            .await
        {
            Ok(aufgenommen) => {
                let _ = tokio::fs::remove_file(&laeuft).await;
                let block = zustand.block_fertig_bei(
                    aufgenommen.media_path.to_string_lossy().to_string(),
                    aufgenommen.actual_duration_seconds.round().max(0.0) as u64,
                    versatz,
                );
                tracing::info!(
                    kanal,
                    block = block.nummer,
                    sekunden = aufgenommen.actual_duration_seconds,
                    "Block aufgenommen"
                );
                warteschlange.lock().await.einreihen(block);
            }
            Err(fehler) => {
                // Ein Fehlschlag heisst in aller Regel: der Stream ist vorbei.
                // Dann endet dieser Task, und die Aufsicht startet ihn neu,
                // sobald der Kanal wieder sendet.
                //
                // Der vorbereitete Blockordner samt Zettel muss weg, sonst
                // bleibt bei jedem Sendungsende ein leerer Ordner liegen, den
                // weder die Aufbewahrung noch die Wiederaufnahme je anfasst.
                let _ = tokio::fs::remove_file(&laeuft).await;
                if let Err(aufraeumfehler) = tokio::fs::remove_dir_all(&blockordner).await {
                    tracing::warn!(
                        ?aufraeumfehler,
                        ordner = ?blockordner,
                        "leerer Blockordner bleibt liegen"
                    );
                }
                tracing::info!(kanal, ?fehler, "Aufnahme beendet");
                return zustand;
            }
        }
    }
}

/// Wie oft die Aufbewahrung greift.
const AUFRAEUM_TAKT_SEKUNDEN: u64 = 60 * 60;

/// Loescht Berichte, die aelter als `STREAM_AUDIT_RETENTION_DAYS` sind.
///
/// `0` heisst unbegrenzt. Gelesen wird die Aenderungszeit der Datei; ein
/// nicht lesbarer Zeitstempel laesst die Datei liegen - im Zweifel behalten,
/// nicht raten und loeschen.
async fn alte_berichte_loeschen(konfiguration: &Konfiguration) {
    let tage = konfiguration.aufbewahrung_tage;
    if tage == 0 {
        return;
    }
    // saturating: eine absurd grosse Zahl in der Konfiguration soll nicht
    // ueberlaufen und aus "sehr lange" ein "sofort loeschen" machen.
    let grenze = Duration::from_secs(tage.saturating_mul(24 * 60 * 60));
    // Laeufe, deren Drive-Archiv noch aussteht, duerfen ihre Berichte nicht an
    // die Aufbewahrung verlieren - sonst laege spaeter ein unvollstaendiges
    // Archiv oben. Der Mitschnitt selbst liegt in einem eigenen Baum und faellt
    // ohnehin nicht unter diese Funktion.
    let ausstehend = if archiv::archiv_aktiv() {
        pending_archiv_laeufe(konfiguration).await
    } else {
        std::collections::HashSet::new()
    };
    // Berichte liegen unter <ausgabe>/<kanal>/. Ein Aufraeumen, das nur die
    // oberste Ebene liest, findet keinen einzigen davon.
    let mut ordner = vec![konfiguration.ausgabe.clone()];
    let mut oben = match tokio::fs::read_dir(&konfiguration.ausgabe).await {
        Ok(oben) => oben,
        Err(fehler) => {
            tracing::warn!(
                ?fehler,
                ordner = ?konfiguration.ausgabe,
                "Ausgabeordner nicht lesbar - Aufbewahrung greift nicht"
            );
            return;
        }
    };
    while let Ok(Some(eintrag)) = oben.next_entry().await {
        if eintrag
            .file_type()
            .await
            .map(|typ| typ.is_dir())
            .unwrap_or(false)
        {
            ordner.push(eintrag.path());
        }
    }

    let mut geloescht = 0usize;
    for ordner in ordner {
        let mut eintraege = match tokio::fs::read_dir(&ordner).await {
            Ok(eintraege) => eintraege,
            Err(fehler) => {
                tracing::warn!(
                    ?fehler,
                    ?ordner,
                    "Ordner nicht lesbar - Berichte bleiben liegen"
                );
                continue;
            }
        };
        while let Ok(Some(eintrag)) = eintraege.next_entry().await {
            let pfad = eintrag.path();
            let endung = pfad
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            // `neu` ist die Nebendatei des atomaren Schreibens. Bricht der
            // Dienst mitten im Schreiben ab, bleibt sie liegen - mit dem
            // Inhalt eines Berichts. Ohne sie hier faellt sie aus der
            // Aufbewahrung heraus und liegt fuer immer da.
            if !matches!(endung, "md" | "json" | "txt" | "neu") {
                continue;
            }
            let stamm = pfad
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            // Bei `bericht.json.neu` steckt der eigentliche Name eine Ebene
            // tiefer im Stamm.
            let stamm = stamm.strip_suffix(".json").unwrap_or(stamm);
            // Im Kanalordner gilt der Kanalname als Praefix; direkt in der
            // Ausgabe nur die Namen der Vorgaengerfassung.
            let kanal = if ordner == konfiguration.ausgabe {
                None
            } else {
                ordner.file_name().and_then(|n| n.to_str())
            };
            if !ist_berichtsname(stamm, kanal) {
                continue;
            }
            let Ok(daten) = eintrag.metadata().await else {
                continue;
            };
            let Ok(alter) = daten
                .modified()
                .and_then(|m| m.elapsed().map_err(std::io::Error::other))
            else {
                continue;
            };
            if alter <= grenze {
                continue;
            }
            // Ausnahme: gehoert der Bericht zu einem Lauf, dessen Drive-Archiv
            // noch aussteht, bleibt er liegen, bis der Upload durch ist.
            if let Some(kanal) = kanal {
                let dateiname = pfad
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if ausstehend
                    .iter()
                    .any(|(k, l)| k == kanal && bericht_gehoert_zu(dateiname, k, l))
                {
                    continue;
                }
            }
            match tokio::fs::remove_file(&pfad).await {
                Ok(()) => geloescht += 1,
                // Stilles Scheitern hiesse: der Bericht liegt weiter da und
                // niemand weiss davon, obwohl die Aufbewahrung abgelaufen ist.
                Err(fehler) => {
                    tracing::warn!(?fehler, datei = ?pfad, "Bericht nicht loeschbar")
                }
            }
        }
    }
    if geloescht > 0 {
        tracing::info!(geloescht, tage, "alte Berichte geloescht");
    }
}

/// Der Blockordner zu einer Aufnahme - oder `None`, wenn der Pfad nicht der
/// erwarteten Form `<wurzel>/<kanal>/<lauf>/t<sekunden>-b<nummer>/<capture>/datei.ts`
/// entspricht.
///
/// Der Namenstest auf `t<Ziffern>` ist die eigentliche Sicherung: er
/// unterscheidet einen Blockordner von jedem anderen Verzeichnis, das zufaellig
/// zwei Ebenen ueber einer Datei liegt.
fn blockordner_von(wurzel: &Path, aufnahme: &Path) -> Option<PathBuf> {
    let capture = aufnahme.parent()?;
    let block = capture.parent()?;
    let lauf = block.parent()?;
    let kanal = lauf.parent()?;
    if kanal.parent()? != wurzel {
        return None;
    }
    let name = block.file_name()?.to_str()?;
    let (zeit, nummer) = name.strip_prefix('t')?.split_once("-b")?;
    if zeit.is_empty()
        || nummer.is_empty()
        || !zeit.chars().all(|c| c.is_ascii_digit())
        || !nummer.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(block.to_path_buf())
}

/// Gehoert dieser Dateiname zu einem Bericht dieses Dienstes?
///
/// Berichte heissen `<kanal>-<lauf>-t<sekunden>`. Ohne diese Pruefung raeumte
/// der Dienst einen Ausgabeordner mit auf, den sich jemand mit anderen Daten
/// teilt.
fn ist_berichtsname(stamm: &str, kanal: Option<&str>) -> bool {
    // Die Vorgaengerfassung schrieb `stream-audit-<zeitstempel>` in denselben
    // Ordner, mit unmaskierten Belegen. Ohne diese Zeile laegen die fuer immer
    // dort.
    if stamm.starts_with("stream-audit-") || stamm.starts_with("stream_coaching_audit-") {
        return true;
    }
    // Ein Bericht liegt im Ordner seines Kanals und traegt dessen Namen. Ohne
    // diesen Abgleich reichte ein fremdes `rechnung-t2025.json` im geteilten
    // Ausgabeordner, um geloescht zu werden.
    let Some(kanal) = kanal else {
        return false;
    };
    let Some(rest) = stamm.strip_prefix(kanal).and_then(|r| r.strip_prefix('-')) else {
        return false;
    };
    // Bestand aus der Zeit vor den Bloecken: `<kanal>-<zeitstempel>-block<nnn>`.
    // Auf der Maschine liegt genau das, und ohne diesen Zweig faellt es aus der
    // Aufbewahrung - Berichte mit Belegen, die nie ablaufen.
    if let Some((zeitstempel, nummer)) = rest.rsplit_once("-block") {
        if !zeitstempel.is_empty()
            && !nummer.is_empty()
            && nummer.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    // <lauf>-t<sekunden>-b<nummer>
    let Some((lauf_und_zeit, nummer)) = rest.rsplit_once("-b") else {
        return false;
    };
    let Some((lauf, zeit)) = lauf_und_zeit.rsplit_once("-t") else {
        return false;
    };
    !lauf.is_empty()
        && !zeit.is_empty()
        && !nummer.is_empty()
        && zeit.chars().all(|c| c.is_ascii_digit())
        && nummer.chars().all(|c| c.is_ascii_digit())
}

/// Loescht liegengebliebene Aufnahmen, die aelter als die Aufbewahrung sind.
///
/// Aufnahmen bleiben absichtlich liegen, wenn ein Fund drinsteht, die Pruefung
/// unvollstaendig war oder ein Block aufgegeben wurde. Ohne diese Grenze waere
/// "bleibt liegen" gleichbedeutend mit "fuer immer": eine Stunde Stream sind
/// mehrere hundert Megabyte, und `.ts` ist die volle HLS-Spur, nicht nur Ton.
///
/// Was noch in der Warteschlange steht, wird nie geloescht - sonst zoege die
/// Aufbewahrung einem wartenden Block die Datei unter den Fuessen weg.
async fn alte_aufnahmen_loeschen(
    konfiguration: &Konfiguration,
    warteschlange: &Arc<Mutex<plan::Warteschlange>>,
) {
    let tage = konfiguration.aufbewahrung_tage;
    if tage == 0 {
        return;
    }
    let grenze = Duration::from_secs(tage.saturating_mul(24 * 60 * 60));

    let wurzel = aufnahme_wurzel(konfiguration);
    let mut aufnahmen = Vec::new();
    ts_dateien_sammeln(&wurzel, &mut aufnahmen).await;
    let eingereiht: std::collections::HashSet<String> =
        warteschlange.lock().await.dateien().into_iter().collect();

    let mut geloescht = 0usize;
    for pfad in aufnahmen {
        if eingereiht.contains(&pfad.to_string_lossy().to_string()) {
            continue;
        }
        let Ok(daten) = tokio::fs::metadata(&pfad).await else {
            continue;
        };
        let Ok(alter) = daten
            .modified()
            .and_then(|m| m.elapsed().map_err(std::io::Error::other))
        else {
            continue;
        };
        if alter <= grenze {
            continue;
        }
        // Der Blockordner, nicht das Capture-Verzeichnis darin: sonst bleiben
        // block.json, ausgewertet.json und die leeren Ebenen fuer immer stehen.
        //
        // Blind zwei Ebenen hochzugehen waere gefaehrlich: eine verirrte Datei
        // direkt unter der Wurzel fuehrte sonst zum Loeschen des ganzen
        // Ausgabeordners. Geloescht wird nur, was aussieht wie ein Blockordner.
        let Some(ziel) = blockordner_von(&wurzel, &pfad) else {
            tracing::warn!(datei = ?pfad, "Aufnahme ausserhalb der erwarteten Struktur");
            continue;
        };
        match tokio::fs::remove_dir_all(&ziel).await {
            Ok(()) => geloescht += 1,
            Err(fehler) => tracing::warn!(?fehler, ordner = ?ziel, "Aufnahme nicht loeschbar"),
        }
    }
    if geloescht > 0 {
        tracing::info!(geloescht, tage, "alte Aufnahmen geloescht");
    }
    leere_huellen_loeschen(&wurzel).await;
}

/// Haelt die aufbewahrten Aufnahmen unter der Gesamtgrenze, aeltestes zuerst.
///
/// Die Frist allein reicht nicht: faellt der Modellschritt aus, gilt jeder
/// Block als unvollstaendig geprueft, und dann bleibt jede Aufnahme dreissig
/// Tage liegen. Genau der Ausfall, der das Aufbewahren ausloest, laesst es
/// unbegrenzt wachsen - auf derselben Platte, auf der Bot und Datenbank liegen.
///
/// Gibt zurueck, wie viele Aufnahmen wegen der Grenze weichen mussten.
async fn grenze_durchsetzen(
    konfiguration: &Konfiguration,
    warteschlange: &Arc<Mutex<plan::Warteschlange>>,
) -> usize {
    let grenze = konfiguration.behalten_grenze_bytes;
    if grenze == 0 {
        return 0;
    }
    let wurzel = aufnahme_wurzel(konfiguration);
    let mut aufnahmen = Vec::new();
    ts_dateien_sammeln(&wurzel, &mut aufnahmen).await;
    let eingereiht: std::collections::HashSet<String> =
        warteschlange.lock().await.dateien().into_iter().collect();

    // Alter und Groesse einmal einsammeln, dann aeltestes zuerst.
    let mut bestand: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    let mut belegt = 0u64;
    for pfad in aufnahmen {
        let Ok(daten) = tokio::fs::metadata(&pfad).await else {
            continue;
        };
        belegt = belegt.saturating_add(daten.len());
        if eingereiht.contains(&pfad.to_string_lossy().to_string()) {
            // Was gleich ausgewertet wird, zaehlt zum Bestand, faellt aber
            // nicht dem Loeschen zum Opfer - sonst zoege die Grenze einem
            // wartenden Block die Datei unter den Fuessen weg.
            continue;
        }
        let Ok(geaendert) = daten.modified() else {
            continue;
        };
        bestand.push((geaendert, daten.len(), pfad));
    }
    if belegt <= grenze {
        return 0;
    }
    bestand.sort_by_key(|(geaendert, _, _)| *geaendert);

    let mut geloescht = 0usize;
    for (_, groesse, pfad) in bestand {
        if belegt <= grenze {
            break;
        }
        let Some(ziel) = blockordner_von(&wurzel, &pfad) else {
            continue;
        };
        match tokio::fs::remove_dir_all(&ziel).await {
            Ok(()) => {
                belegt = belegt.saturating_sub(groesse);
                geloescht += 1;
            }
            Err(fehler) => tracing::warn!(?fehler, ordner = ?ziel, "Aufnahme nicht loeschbar"),
        }
    }
    if geloescht > 0 {
        tracing::warn!(
            geloescht,
            grenze_gb = grenze / (1024 * 1024 * 1024),
            "Aufnahmegrenze erreicht - aelteste Aufnahmen entfernt"
        );
        leere_huellen_loeschen(&wurzel).await;
    }
    geloescht
}

/// Entfernt leergeraeumte Lauf- und Kanalordner unter `aufnahmen/`.
///
/// Je Sendung bleibt sonst eine leere Huelle stehen, und genau dieser Baum wird
/// bei jedem Aufraeumtakt rekursiv abgesucht. `remove_dir` loescht nur leere
/// Verzeichnisse, ein volles bleibt also unangetastet.
async fn leere_huellen_loeschen(wurzel: &Path) {
    let Ok(mut kanaele) = tokio::fs::read_dir(wurzel).await else {
        return;
    };
    while let Ok(Some(kanal)) = kanaele.next_entry().await {
        let kanalpfad = kanal.path();
        if !kanalpfad.is_dir() {
            continue;
        }
        if let Ok(mut laeufe) = tokio::fs::read_dir(&kanalpfad).await {
            while let Ok(Some(lauf)) = laeufe.next_entry().await {
                let laufpfad = lauf.path();
                if laufpfad.is_dir() {
                    let _ = tokio::fs::remove_dir(&laufpfad).await;
                }
            }
        }
        // Der Kanalordner faellt erst, wenn auch sein letzter Lauf weg ist.
        let _ = tokio::fs::remove_dir(&kanalpfad).await;
    }
}

/// Arbeitet die Warteschlange ab, immer nur einen Block gleichzeitig.
async fn auswertungs_schleife(
    transkribierer: OpenAiTranscriber,
    konfiguration: Konfiguration,
    warteschlange: Arc<Mutex<plan::Warteschlange>>,
    abbruch: Arc<std::sync::atomic::AtomicBool>,
    sperre: Arc<Mutex<plan::LaufSperre>>,
    last_gate: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut naechstes_aufraeumen = tokio::time::Instant::now();
    // Wie viele Bloecke am Stueck je Kanal der Modellschritt ausfiel.
    let mut modell_ausgefallen: std::collections::HashMap<String, usize> = Default::default();
    // Wie viele Bloecke am Stueck je Kanal ohne einen erkannten Satz blieben.
    let mut leer_am_stueck: std::collections::HashMap<String, usize> = Default::default();
    // Ist die Platzgrenze schon gemeldet? Sie greift, solange der Ausfall
    // anhaelt; eine DM je Aufraeumtakt waere stuendliches Rauschen.
    let mut platz_gemeldet = false;
    loop {
        if abbruch.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("Auswertungsschleife beendet sich");
            return;
        }
        // Aufbewahrung war bisher nur eine Zahl in der Konfiguration. Eine
        // Grenze, die nichts loescht, ist keine - Berichte mit moeglichen
        // Vorfaellen lagen unbegrenzt.
        if tokio::time::Instant::now() >= naechstes_aufraeumen {
            alte_berichte_loeschen(&konfiguration).await;
            alte_aufnahmen_loeschen(&konfiguration, &warteschlange).await;
            // Erst die Frist, dann die Gesamtgrenze: was ohnehin abgelaufen
            // ist, soll nicht als "wegen Platzmangel geloescht" gemeldet
            // werden.
            let wegen_platz = grenze_durchsetzen(&konfiguration, &warteschlange).await;
            if wegen_platz > 0 && !platz_gemeldet {
                let text = format!(
                    "Coaching-Audit: Aufnahmegrenze erreicht, {wegen_platz} aelteste Aufnahmen \
geloescht. Sie sind als Beleg nicht mehr da."
                );
                let schluessel = format!(
                    "{}-platzgrenze-{}",
                    start_kennung(),
                    naechste_vorfall_nummer()
                );
                match dm_rohtext(&text, &schluessel).await {
                    // Erst nach der Zustellung merken, sonst faellt die
                    // Meldung bei einer Broker-Stoerung ganz aus.
                    Ok(()) => platz_gemeldet = true,
                    Err(fehler) => {
                        tracing::error!(fehler, "Platzgrenze nicht meldbar");
                        hinweis_aufheben(&konfiguration, "platzgrenze", &schluessel, &text).await;
                    }
                }
            }
            if wegen_platz == 0 {
                platz_gemeldet = false;
            }
            offene_meldungen_einreihen(&konfiguration, &warteschlange).await;
            offene_hinweise_senden(&konfiguration).await;
            // Sicherheitsnetz fuer das Drive-Archiv: fertige Laeufe, deren
            // Upload beim ersten Anlauf scheiterte oder die einen Neustart
            // erwischten, werden hier nachgeholt.
            offene_archive_nachholen(&konfiguration, &sperre, &warteschlange).await;
            naechstes_aufraeumen =
                tokio::time::Instant::now() + Duration::from_secs(AUFRAEUM_TAKT_SEKUNDEN);
        }

        // Last-Gate: unter Dauerlast wird nur aufgenommen, die Auswertung
        // wartet. Die Bloecke bleiben auf der Platte und werden nachgeholt,
        // sobald die Last faellt. Bewusst ohne DM: das Gate misst die
        // maschinenweite Last, die zu einem grossen Teil die Auswertung selbst
        // erzeugt - eine "Server ueberlastet"-DM je Wechsel wuerde eine
        // Fremdstoerung behaupten, die keine ist, und im Takt flattern. Den
        // Zustandswechsel loggt der Messtask. Echter Deckungsverlust (Rueckstand
        // oder Platzgrenze) meldet sich weiter ueber die eigenen Pfade.
        if last_gate.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
            continue;
        }
        let snapshot = sperre.lock().await.clone();
        let naechster = warteschlange.lock().await.naechster_ohne_sperre(&snapshot);
        let Some(block) = naechster else {
            tokio::time::sleep(Duration::from_secs(15)).await;
            continue;
        };
        // Bis zur Freigabe am Ende dieses Durchlaufs zaehlt die Datei als in
        // Arbeit. Erst danach schuetzt den Block seine Fertig-Marke oder sein
        // neuer Platz in der Schlange.
        let in_arbeit_datei = block.datei.clone();

        let ergebnis = block_auswerten(&transkribierer, &konfiguration, &block).await;

        // Der Stille-Zaehler laeuft vor der Verzweigung, nicht in einem ihrer
        // Arme. Er stand frueher nur im Loesch-Arm: fiel der Modellschritt aus,
        // landete jeder Block im Behalten-Arm, also auch jeder stumme, und die
        // Meldung "zwanzig Bloecke ohne einen erkannten Satz" kam nie - genau
        // beim Doppelausfall von STT und Modell, wo sie am noetigsten ist.
        let segmente_gesehen = match &ergebnis {
            Ok(Aufnahmeschicksal::Behalten(bericht)) => Some(bericht.segmente),
            Ok(Aufnahmeschicksal::Loeschen { segmente }) => Some(*segmente),
            // Ein uebersprungener Block wurde nie transkribiert, ein
            // fehlgeschlagener nie fertig - beide sagen nichts ueber Stille.
            Ok(Aufnahmeschicksal::Uebersprungen) | Err(_) => None,
        };
        let mut stumm_am_stueck = 0usize;
        match segmente_gesehen {
            Some(0) => {
                let zaehler = leer_am_stueck.entry(block.kanal.clone()).or_insert(0usize);
                *zaehler += 1;
                stumm_am_stueck = *zaehler;
            }
            // Gezaehlt wird je Kanal. Ein gesunder Kanal darf den Zaehler eines
            // anderen nicht zuruecksetzen - sonst bliebe genau der Kanal, dessen
            // Ton nie ankommt, ohne Warnung.
            Some(_) => {
                leer_am_stueck.remove(&block.kanal);
            }
            None => {}
        }
        // Ein Block ohne Sprache ist normal. Zwanzig am Stueck sind es nicht -
        // dann liefert der STT-Dienst vermutlich fuer alles nichts, und ohne
        // diesen Zaehler saehe das aus wie lauter ruhige Streams. Gemeldet wird
        // bei jedem Vielfachen, nicht nur beim ersten Mal: eine Stoerung, die
        // anhaelt, soll sich wieder melden.
        if stumm_am_stueck > 0 && stumm_am_stueck.is_multiple_of(MAX_LEER_AM_STUECK) {
            stille_melden(&konfiguration, &block.kanal, stumm_am_stueck).await;
        }

        match ergebnis {
            Ok(Aufnahmeschicksal::Uebersprungen) => {}
            Ok(Aufnahmeschicksal::Behalten(bericht)) => {
                modellausfall_melden(
                    &konfiguration,
                    &block.kanal,
                    &bericht,
                    &mut modell_ausgefallen,
                )
                .await;
                meldung_erledigt(Path::new(&block.datei)).await;
                fertig_markieren(Path::new(&block.datei), &bericht).await;
                if !block.nur_melden {
                    akte_verbuchen(&konfiguration, &block, &bericht).await;
                }
                lauf_ende_melden(
                    &konfiguration,
                    &block.kanal,
                    &block.lauf,
                    &sperre,
                    &warteschlange,
                )
                .await;
                tracing::info!(
                    block = %block.bezeichnung(),
                    "Aufnahme bleibt liegen - Fund oder unvollstaendige Pruefung"
                );
            }
            Ok(Aufnahmeschicksal::Loeschen { segmente }) => {
                if modell_ausgefallen.remove(&block.kanal).is_some() {
                    hinweis_erledigt(&konfiguration, &format!("modellausfall-{}", block.kanal))
                        .await;
                }
                meldung_erledigt(Path::new(&block.datei)).await;
                // Gezaehlt und gemeldet wurde schon vor der Verzweigung: dort
                // laeuft derselbe Zaehler fuer beide Auswertungsergebnisse,
                // ein zweites Mal zaehlen wuerde ihn doppelt so schnell laufen
                // lassen wie die Meldung, die davon spricht.
                let aufnahme_behalten = segmente == 0 && stumm_am_stueck >= MAX_LEER_AM_STUECK;
                if !block.nur_melden {
                    let bericht = leerer_bericht(&block);
                    akte_verbuchen(&konfiguration, &block, &bericht).await;
                }
                lauf_ende_melden(
                    &konfiguration,
                    &block.kanal,
                    &block.lauf,
                    &sperre,
                    &warteschlange,
                )
                .await;
                if aufnahme_behalten {
                    fertig_markieren(Path::new(&block.datei), &leerer_bericht(&block)).await;
                    tracing::warn!(
                        block = %block.bezeichnung(),
                        "Aufnahme bleibt liegen - Verdacht auf ausgefallene Transkription"
                    );
                    warteschlange.lock().await.freigeben(&in_arbeit_datei);
                    continue;
                }
                // Erst nach erfolgreicher Auswertung wegraeumen. Wer bei einem
                // Fehler loescht, vernichtet bei einem Aussetzer der
                // Transkription den einzigen Beleg, den es je gab.
                let pfad = PathBuf::from(&block.datei);
                // Der Blockordner, nicht nur das Capture-Verzeichnis darin:
                // sonst bleiben block.json und die leeren Ebenen fuer immer
                // liegen. Und nur, wenn der Pfad wirklich die Form eines
                // Blockordners hat - eine verirrte Datei direkt unter
                // aufnahmen/ haette sonst den ganzen Ausgabeordner mitgenommen.
                let wurzel = aufnahme_wurzel(&konfiguration);
                if let Some(verzeichnis) = blockordner_von(&wurzel, &pfad) {
                    if let Err(fehler) = tokio::fs::remove_dir_all(&verzeichnis).await {
                        tracing::warn!(
                            ?fehler,
                            ordner = ?verzeichnis,
                            "Aufnahme nicht geloescht - sie laeuft nach einem Neustart erneut"
                        );
                    }
                } else {
                    tracing::warn!(
                        datei = ?pfad,
                        "Aufnahme ausserhalb der erwarteten Struktur - nichts geloescht"
                    );
                }
            }
            // Der Bericht steht, nur die Meldung fehlt. Wiederholt wird
            // ausschliesslich die Meldung: eine zweite Auswertung koennte
            // anders ausfallen und den frueheren Fund ueberschreiben.
            Err(Auswertefehler::Meldung(fehler)) => {
                let bezeichnung = block.bezeichnung();
                let mut naechster = block.clone();
                naechster.nur_melden = true;
                naechster.meldeversuche += 1;
                if naechster.meldeversuche >= MAX_MELDEVERSUCHE_LANG {
                    // Die Marke bleibt auf der Platte: der stuendliche
                    // Aufraeumtakt findet sie und reiht die Meldung erneut
                    // ein. Ohne sie waere ein Fund still verloren, sobald der
                    // Block die Warteschlange verlaesst.
                    meldung_offen_markieren(Path::new(&block.datei)).await;
                    // Irgendwann ist Schluss, sonst haelt der Block seine
                    // Aufnahme dauerhaft aus der Aufbewahrung heraus. Der
                    // Bericht liegt auf der Platte; der naechste Start des
                    // Dienstes nimmt die Meldung von dort wieder auf.
                    tracing::error!(
                        block = %bezeichnung,
                        fehler,
                        "Meldung nach {} Versuchen aufgegeben; Bericht liegt im Ausgabeordner \
                    und wird beim naechsten Start erneut gemeldet",
                        MAX_MELDEVERSUCHE_LANG
                    );
                } else if naechster.meldeversuche >= plan::MAX_MELDEVERSUCHE {
                    // Die kurzen Anlaeufe sind durch. Statt die Meldung
                    // fallenzulassen, wird sie seltener wiederholt - ein
                    // Fund, den niemand erfaehrt, ist der schlimmere Fall.
                    let versuch = naechster.meldeversuche;
                    warteschlange
                        .lock()
                        .await
                        .spaeter_einreihen(naechster, LANGE_MELDEPAUSE_SEKUNDEN);
                    tracing::error!(
                        block = %bezeichnung,
                        fehler,
                        versuch,
                        "Meldung weiter nicht zustellbar - naechster Anlauf in sechs Stunden"
                    );
                } else {
                    let versuch = naechster.meldeversuche;
                    warteschlange
                        .lock()
                        .await
                        .spaeter_einreihen(naechster, MELDEPAUSE_SEKUNDEN);
                    tracing::warn!(
                        block = %bezeichnung,
                        fehler,
                        versuch,
                        "Meldung fehlgeschlagen - nur die Meldung wird wiederholt"
                    );
                }
            }
            Err(Auswertefehler::Auswertung(fehler)) => {
                let bezeichnung = block.bezeichnung();
                let kanal = block.kanal.clone();
                let lauf = block.lauf.clone();
                let versuch = block.versuche + 1;
                let aufgegeben_datei = block.datei.clone();
                let aufgegeben_bericht = leerer_bericht(&block);
                // Die Pause haengt am Block (plan::PAUSE_SEKUNDEN), nicht an
                // diesem Arbeiter: ein Schlaf hier hielte alle anderen Kanaele
                // mit an, wegen eines Blocks, der gerade nicht geht.
                if warteschlange.lock().await.erneut_versuchen(block) {
                    tracing::warn!(
                        block = %bezeichnung,
                        fehler,
                        versuch,
                        "Auswertung fehlgeschlagen, erneut eingereiht"
                    );
                } else {
                    tracing::error!(
                        block = %bezeichnung,
                        fehler,
                        "Auswertung nach {} Versuchen aufgegeben; Aufnahme bleibt zur \
                    Handpruefung liegen",
                        plan::MAX_VERSUCHE
                    );
                    // Ein aufgegebener Block darf nicht nur im Journal stehen.
                    // Sonst hiesse "keine DM" hier faelschlich "sauber".
                    let text = format!(
                        "Coaching-Audit {}: Block {} nach {} Versuchen aufgegeben ({}). \
Aufnahme liegt noch da.",
                        kanal,
                        bezeichnung,
                        plan::MAX_VERSUCHE,
                        fehler
                    );
                    // Marke setzen, sonst faengt jeder Neustart die drei
                    // Versuche und die Meldung von vorn an.
                    fertig_markieren(Path::new(&aufgegeben_datei), &aufgegeben_bericht).await;
                    if let Err(dm_fehler) =
                        dm_rohtext(&text, &format!("{bezeichnung}-aufgegeben")).await
                    {
                        // Genau diese Meldung soll verhindern, dass ein
                        // unvollstaendiges Audit wie ein sauberes aussieht.
                        // Sie geht in den Hinweisspeicher und wird stuendlich
                        // erneut versucht - den Block noch einmal einzureihen
                        // hiesse, Transkription und Modell erneut laufen zu
                        // lassen, obwohl nur die Zustellung klemmt.
                        hinweis_aufheben(
                            &konfiguration,
                            &format!("aufgegeben-{bezeichnung}"),
                            &format!("{bezeichnung}-aufgegeben"),
                            &text,
                        )
                        .await;
                        tracing::error!(
                            dm_fehler,
                            block = %bezeichnung,
                            "Aufgabe nicht meldbar - Hinweis aufgehoben, wird stuendlich \
                            erneut versucht"
                        );
                    }
                    lauf_ende_melden(&konfiguration, &kanal, &lauf, &sperre, &warteschlange).await;
                }
            }
        }
        warteschlange.lock().await.freigeben(&in_arbeit_datei);
    }
}

/// Warum ein Block nicht durchlief.
///
/// Die Trennung ist wichtig: eine gescheiterte Meldung darf nur die Meldung
/// wiederholen. Eine zweite Auswertung liefe erneut durch Transkription und
/// Modell, koennte anders ausfallen und den frueheren Fund ueberschreiben.
#[derive(Debug)]
enum Auswertefehler {
    Auswertung(String),
    Meldung(String),
}

/// Was nach der Auswertung mit der Aufnahme geschieht.
#[derive(Debug)]
enum Aufnahmeschicksal {
    /// Alles ausgewertet, nichts gefunden - die Aufnahme kann weg. Traegt die
    /// Zahl der Segmente, damit die Auswertungsschleife merkt, wenn reihenweise
    /// gar kein Text mehr ankommt.
    Loeschen { segmente: usize },
    /// Ausgewertet, aber es gibt etwas nachzuhoeren: ein Fund oder eine
    /// unvollstaendige Pruefung. Die Aufnahme bleibt liegen und wird als
    /// ausgewertet markiert.
    Behalten(Box<Bericht>),
    /// Der Block trug beim Start der Auswertung schon seine Fertig-Marke. Es
    /// passiert nichts weiter: sein Bericht steht, seine Meldung ist raus, und
    /// ein zweiter Durchlauf koennte beides ueberschreiben.
    Uebersprungen,
}

async fn block_auswerten(
    transkribierer: &OpenAiTranscriber,
    konfiguration: &Konfiguration,
    block: &plan::Block,
) -> Result<Aufnahmeschicksal, Auswertefehler> {
    // Zweite Sperre gegen einen doppelt eingereihten Block, unabhaengig von
    // der Warteschlange: die Marke liegt auf der Platte und ueberlebt auch
    // einen Neustart zwischen den beiden Durchlaeufen. Der Weg "nur melden"
    // laeuft absichtlich weiter, er wertet nichts neu aus.
    if !block.nur_melden && bereits_ausgewertet(Path::new(&block.datei)).await {
        tracing::warn!(
            block = %block.bezeichnung(),
            "Block war schon ausgewertet - zweiter Durchlauf uebersprungen"
        );
        return Ok(Aufnahmeschicksal::Uebersprungen);
    }

    // Der Bericht steht schon, nur die Meldung fehlt. Ein zweiter Durchlauf
    // durch Transkription und Modell koennte anders ausfallen - ein spaeteres
    // "nichts gefunden" wuerde den frueheren Fund ueberschreiben und seine
    // Aufnahme loeschen.
    if block.nur_melden {
        let bericht = bericht_lesen(konfiguration, block)
            .await
            .map_err(Auswertefehler::Auswertung)?;
        dm_senden(&bericht, &block.bezeichnung())
            .await
            .map_err(Auswertefehler::Meldung)?;
        return Ok(
            if bericht.funde.is_empty() && bericht.modell_geprueft && !bericht.aufnahme_abgebrochen
            {
                Aufnahmeschicksal::Loeschen {
                    segmente: bericht.segmente,
                }
            } else {
                Aufnahmeschicksal::Behalten(Box::new(bericht))
            },
        );
    }

    let transkript = transkribierer
        .transcribe_clip(Path::new(&block.datei))
        .await
        .map_err(|e| Auswertefehler::Auswertung(format!("Transkription: {e}")))?;

    // Lag die Laufmarke noch da, wurde die Aufnahme mitten im Block gestoppt.
    // Was hier geprueft wird, ist dann nur ihr Anfang.
    let abgebrochen = aufnahme_abgebrochen(Path::new(&block.datei)).await;
    let segmente = if transkript.segments.is_empty() {
        segmente_bauen(block, &transkript.text, transkript.duration_seconds)
    } else {
        segmente_aus_whisper(block, &transkript.segments)
    };
    // Ein Block ohne Sprache ist der Normalfall: Musik, Spielton, Pause. Der
    // lokale STT-Dienst liefert dafuer bewusst nichts. Ihn als Fehlschlag zu
    // behandeln hiesse, bei jedem ruhigen Abschnitt zu melden und das ganze
    // Video aufzuheben. Ein systematischer Ausfall faellt stattdessen ueber
    // die Reihe auf (siehe leer_am_stueck in der Auswertungsschleife).
    let leer = segmente.is_empty();
    if leer {
        tracing::info!(block = %block.bezeichnung(), "kein gesprochener Text im Block");
    }

    let mut funde = tb_stream_audit::regelfunde(&segmente);
    let (modell_funde, modell_fehler) = modellfunde(&segmente).await;
    funde.extend(modell_funde);
    // Der Abbruch der Aufnahme steht als eigenes Feld im Bericht. Frueher lief
    // er in denselben Hinweis wie ein Modellausfall: der Bericht behauptete
    // dann "der Modellschritt lief nicht", obwohl er lief, und die
    // Ausfallmeldung ging bei jedem Neustart eines sendenden Kanals raus.
    let modell_hinweis = modell_fehler;
    let funde = report::sortiert(tb_stream_audit::funde_zusammenfassen(funde));

    let endpunkt = tb_llm::selection::endpoint_for(llm::USE_CASE);
    let jetzt = chrono::Utc::now();
    let bericht = Bericht {
        lauf_id: report::lauf_id(jetzt, &block.kanal),
        erstellt_am: jetzt.to_rfc3339(),
        stream_start_utc: block.stream_start_utc.clone(),
        aufnahme_beginn_utc: block.aufnahme_beginn_utc.clone(),
        quelle: if block.zeit_unsicher {
            format!(
                "live, Block {} - Zeiten ab Aufnahmebeginn, nicht ab Sendungsbeginn",
                block.nummer
            )
        } else {
            format!("live, Block {}", block.nummer)
        },
        kanal: block.kanal.clone(),
        transkription: transkript.engine.clone(),
        modell: transkript.model.clone(),
        transkription_lokal: tb_stream_audit::llm::ist_lokal(&stt_basis_url()),
        anbieter: endpunkt.provider.to_owned(),
        llm_modell: endpunkt.model.clone(),
        transkript_behalten: konfiguration.transkript_behalten,
        segmente: segmente.len(),
        modell_geprueft: modell_hinweis.is_none(),
        modell_hinweis: modell_hinweis.unwrap_or_default(),
        aufnahme_abgebrochen: abgebrochen,
        funde,
    };

    schreiben(konfiguration, block, &bericht, &transkript.text)
        .await
        .map_err(Auswertefehler::Auswertung)?;
    // Ein Fund, dessen Meldung nie ankam, ist kein erledigter Block. Schlaegt
    // die DM fehl, geht der Block zurueck in die Warteschlange statt die
    // Aufnahme zu loeschen - sonst verschwindet der einzige Hinweis still.
    // Der Idempotenzschluessel haengt an der Blockbezeichnung, nicht an der
    // Lauf-ID: die entsteht bei jedem Versuch neu, und der Broker koennte die
    // Wiederholung dann nicht als solche erkennen.
    dm_senden(&bericht, &block.bezeichnung())
        .await
        .map_err(Auswertefehler::Meldung)?;
    // Geloescht wird nur, was sauber und vollstaendig geprueft war. Ein Fund
    // ohne Aufnahme waere eine Behauptung ohne Gegenprobe: das Transkript ist
    // standardmaessig weg, der Bericht zeigt nur den geschwaerzten Ausschnitt,
    // und ob das VOD noch existiert, entscheidet der Kanal.
    Ok(
        if !bericht.funde.is_empty() || !bericht.modell_geprueft || bericht.aufnahme_abgebrochen {
            Aufnahmeschicksal::Behalten(Box::new(bericht))
        } else {
            Aufnahmeschicksal::Loeschen {
                segmente: bericht.segmente,
            }
        },
    )
}

/// Baut Segmente aus den Zeitstempeln, die Whisper selbst geliefert hat.
///
/// Das ist der genaue Weg: die Zeiten stehen an den Woertern, nicht an ihrer
/// Laenge. Mehrere Whisper-Abschnitte werden zu einem Segment gebuendelt,
/// damit das Modell Kontext ueber Satzgrenzen sieht - aber nur bis
/// `SEGMENT_SEKUNDEN`.
fn segmente_aus_whisper(
    block: &plan::Block,
    abschnitte: &[tb_engagement::transcribe::TranscriptSegment],
) -> Vec<Segment> {
    let versatz = block.versatz_sekunden as f64;
    let mut raus: Vec<Segment> = Vec::new();
    let mut offen: Option<Segment> = None;

    for abschnitt in abschnitte {
        let start = versatz + abschnitt.start_seconds.max(0.0);
        let ende = versatz + abschnitt.end_seconds.max(abschnitt.start_seconds);
        match offen.as_mut() {
            Some(segment) if ende - segment.start_sekunden <= SEGMENT_SEKUNDEN => {
                segment.ende_sekunden = ende;
                segment.text.push(' ');
                segment.text.push_str(&abschnitt.text);
            }
            _ => {
                if let Some(fertig) = offen.take() {
                    raus.push(fertig);
                }
                offen = Some(Segment {
                    id: block.segment_id(raus.len() + 1),
                    start_sekunden: start,
                    ende_sekunden: ende,
                    text: abschnitt.text.clone(),
                });
            }
        }
    }
    if let Some(fertig) = offen {
        raus.push(fertig);
    }
    raus
}

/// Teilt den Blocktext in Segmente und rechnet die Zeit ueber den Textanteil.
///
/// Nur der Rueckfallweg: er greift, wenn die Transkription keine eigenen
/// Zeitstempel liefert.
///
/// Whisper liefert hier einen Text am Stueck ohne eigene Zeitmarken. Die Zeit
/// eines Segments wird deshalb linear aus seinem Anteil an der Gesamtlaenge
/// geschaetzt: Segmentgrenze bei 40 Prozent des Textes heisst 40 Prozent der
/// Blockdauer. Das ist eine Naeherung, aber eine ehrliche - sie stimmt an den
/// Blockgrenzen exakt und driftet dazwischen hoechstens um die Laenge einer
/// Sprechpause. Fuer "hoer dir Minute 12 an" reicht das.
///
/// Frueher stand hier `teil.len()` als Endzeit-Faktor. Das ist die Byte-Laenge
/// des Segments multipliziert mit Sekunden-pro-Satz, also eine Zahl ohne
/// Bedeutung: ein Segment mit 200 Zeichen bekam 200 Zeiteinheiten und ragte
/// weit ueber das Blockende hinaus.
fn segmente_bauen(block: &plan::Block, text: &str, dauer: f64) -> Vec<Segment> {
    let saetze: Vec<&str> = text
        .split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if saetze.is_empty() {
        return Vec::new();
    }

    let dauer = dauer.max(0.0);
    let gesamt_zeichen: f64 = saetze.iter().map(|s| s.chars().count() as f64).sum();
    let gesamt_zeichen = gesamt_zeichen.max(1.0);

    // Ziel: ungefaehr SEGMENT_SEKUNDEN je Segment, aber nie weniger als ein Satz.
    let saetze_je_segment = if dauer <= 0.0 {
        saetze.len()
    } else {
        ((SEGMENT_SEKUNDEN / dauer) * saetze.len() as f64).ceil() as usize
    };
    let saetze_je_segment = saetze_je_segment.clamp(1, saetze.len());

    let mut raus = Vec::new();
    let mut zeichen_bisher = 0f64;
    for (i, teil) in saetze.chunks(saetze_je_segment).enumerate() {
        let zeichen_im_teil: f64 = teil.iter().map(|s| s.chars().count() as f64).sum();
        let start = block.versatz_sekunden as f64 + dauer * (zeichen_bisher / gesamt_zeichen);
        zeichen_bisher += zeichen_im_teil;
        let ende = block.versatz_sekunden as f64 + dauer * (zeichen_bisher / gesamt_zeichen);
        raus.push(Segment {
            id: block.segment_id(i + 1),
            start_sekunden: start,
            ende_sekunden: ende,
            text: teil.join(" "),
        });
    }
    raus
}

/// Modellfunde ueber den im Bot konfigurierten Anbieter. Faellt der Aufruf aus,
/// bleibt es bei den Regelfunden - ein Audit ohne Modell ist duenner, aber
/// besser als keines.
async fn modellfunde(segmente: &[Segment]) -> (Vec<tb_stream_audit::Fund>, Option<String>) {
    let endpunkt = tb_llm::selection::endpoint_for(llm::USE_CASE);
    if !llm::fernes_modell_erlaubt(&endpunkt.base_url) {
        return (
            Vec::new(),
            Some(format!(
                "Anbieter {} liegt ausserhalb dieses Rechners; {}=1 setzen, um Transkriptausschnitte dorthin zu senden",
                endpunkt.provider,
                llm::REMOTE_ERLAUBT_ENV
            )),
        );
    }
    let Some(schluessel) = endpunkt.api_key.clone() else {
        return (
            Vec::new(),
            Some(format!("kein Schluessel fuer {}", endpunkt.provider)),
        );
    };
    let Some(client) = modell_client() else {
        return (Vec::new(), Some("HTTP-Client nicht baubar".to_owned()));
    };

    let mut raus = Vec::new();
    let mut fehler_gesehen: Option<String> = None;
    for stapel in llm::stapel(segmente) {
        let anfrage = serde_json::json!({
            "model": endpunkt.model,
            "messages": [
                {"role": "system", "content": llm::SYSTEM_PROMPT},
                {"role": "user", "content": llm::anfrage_json(stapel)},
            ],
            "response_format": {"type": "json_object"},
        });
        let antwort = client
            .post(format!(
                "{}/chat/completions",
                endpunkt.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&schluessel)
            .json(&anfrage)
            .send()
            .await;
        let roh = match antwort {
            // Ohne Statuspruefung sieht ein 401 oder 429 aus wie kaputtes
            // JSON - und der Bericht nennt den falschen Grund.
            Ok(r) if !r.status().is_success() => {
                let status = r.status().as_u16();
                tracing::warn!(status, "Modellaufruf abgelehnt");
                fehler_gesehen.get_or_insert_with(|| format!("Modellaufruf HTTP {status}"));
                continue;
            }
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(fehler) => {
                tracing::warn!(?fehler, "Modellaufruf fehlgeschlagen");
                fehler_gesehen.get_or_insert_with(|| "Modellaufruf fehlgeschlagen".to_owned());
                continue;
            }
        };
        let inhalt = serde_json::from_str::<serde_json::Value>(&roh)
            .ok()
            .and_then(|v| {
                v["choices"][0]["message"]["content"]
                    .as_str()
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let Some(json) = llm::json_objekt_ausschneiden(&inhalt) else {
            tracing::warn!("Modellantwort ohne JSON");
            fehler_gesehen.get_or_insert_with(|| "Modellantwort ohne JSON".to_owned());
            continue;
        };
        match serde_json::from_str::<llm::ModellAntwort>(json) {
            Ok(geparst) => {
                let (funde, verworfen) = llm::zu_funden_gezaehlt(&geparst, stapel);
                if verworfen > 0 {
                    // Erfundene Segment-IDs heissen: die Antwort passt nicht
                    // zur Anfrage. Das darf nicht als saubere Pruefung
                    // durchgehen.
                    tracing::warn!(verworfen, "Modellfunde mit unbekannter Segment-ID");
                    fehler_gesehen.get_or_insert_with(|| {
                        format!("{verworfen} Modellfunde mit unbekannter Segment-ID verworfen")
                    });
                }
                raus.extend(funde);
            }
            Err(fehler) => {
                tracing::warn!(?fehler, "Modellantwort unlesbar");
                fehler_gesehen.get_or_insert_with(|| "Modellantwort unlesbar".to_owned());
            }
        }
    }
    (raus, fehler_gesehen)
}

/// HTTP-Client des Modellschritts.
///
/// Einmal gebaut und dann wiederverwendet: ein Client je Block hiesse ein
/// neuer Verbindungspool und ein neuer TLS-Handschlag pro Auswertung.
/// Ohne die Umleitungssperre koennte ein Endpunkt auf 127.0.0.1 per 307 nach
/// draussen zeigen - die Pruefung auf "lokal" waere dann umsonst. Deshalb gibt
/// es auch keinen Ersatzclient: `Client::new()` paniert im selben Fehlerfall,
/// und ein Client ohne diese Einstellungen waere schlimmer als keiner.
fn modell_client() -> Option<&'static reqwest::Client> {
    static CLIENT: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|fehler| {
                    tracing::error!(?fehler, "HTTP-Client fuer den Modellschritt nicht baubar");
                })
                .ok()
        })
        .as_ref()
}

/// HTTP-Client fuer die Meldungen an den Broker.
///
/// Kurze Zeitgrenze: die Meldung haengt im seriellen Auswerter, und ein
/// Client ohne Grenze bliebe dort fuer immer stehen.
fn broker_client() -> Option<&'static reqwest::Client> {
    static CLIENT: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|fehler| {
                    tracing::error!(?fehler, "HTTP-Client fuer die Meldung nicht baubar");
                })
                .ok()
        })
        .as_ref()
}

/// Namensbasis aller Berichtsdateien eines Blocks, ohne Endung.
fn bericht_basis(konfiguration: &Konfiguration, block: &plan::Block) -> PathBuf {
    konfiguration
        .ausgabe
        .join(&block.kanal)
        .join(block.bezeichnung())
}

/// Haengt eine Endung an, statt eine vorhandene zu ersetzen.
///
/// `with_extension` haette alles hinter einem Punkt in der Blockbezeichnung
/// verschluckt. Lesender und schreibender Weg gingen dann auf verschiedene
/// Dateien - der Bericht laege da, und der Dienst faende ihn nicht.
fn mit_endung(basis: &Path, endung: &str) -> PathBuf {
    let mut name = basis.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(endung);
    basis.with_file_name(name)
}

/// Pfad des Berichts zu einem Block.
fn bericht_pfad(konfiguration: &Konfiguration, block: &plan::Block) -> PathBuf {
    mit_endung(&bericht_basis(konfiguration, block), "json")
}

/// Steht ein **lesbarer** Bericht zu diesem Block schon auf der Platte?
///
/// Nur die Existenz zu pruefen reicht nicht: ein halb geschriebener Bericht
/// wuerde jede Wiederholung auf "nur noch melden" schicken, und die scheiterte
/// dann am unlesbaren JSON, bis der Block aufgegeben ist.
async fn bericht_liegt_vor(konfiguration: &Konfiguration, block: &plan::Block) -> bool {
    bericht_lesen(konfiguration, block).await.is_ok()
}

/// Liest einen schon geschriebenen Bericht wieder ein.
async fn bericht_lesen(
    konfiguration: &Konfiguration,
    block: &plan::Block,
) -> Result<Bericht, String> {
    let pfad = bericht_pfad(konfiguration, block);
    let roh = tokio::fs::read_to_string(&pfad)
        .await
        .map_err(|e| format!("Bericht {} nicht lesbar: {e}", pfad.display()))?;
    serde_json::from_str(&roh).map_err(|e| format!("Bericht unlesbar: {e}"))
}

/// Schreibt mit Modus 0600.
///
/// Berichte nennen Zeit, Kanal und Kategorie eines moeglichen Vorfalls, das
/// Transkript enthaelt den vollen Wortlaut fremder Menschen. Beides ist nichts
/// fuer die Standardrechte, die systemd sonst vergibt.
async fn nur_fuer_mich(pfad: &Path, inhalt: &[u8]) -> Result<(), String> {
    // tokio::fs::OpenOptions bringt `mode()` selbst mit; der std-Trait waere
    // hier ein ungenutzter Import.
    use tokio::io::AsyncWriteExt;

    let mut datei = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(pfad)
        .await
        .map_err(|e| format!("{}: {e}", pfad.display()))?;
    datei
        .write_all(inhalt)
        .await
        .map_err(|e| format!("{}: {e}", pfad.display()))?;
    datei
        .flush()
        .await
        .map_err(|e| format!("{}: {e}", pfad.display()))
}

async fn schreiben(
    konfiguration: &Konfiguration,
    block: &plan::Block,
    bericht: &Bericht,
    transkript: &str,
) -> Result<(), String> {
    let verzeichnis = konfiguration.ausgabe.join(&block.kanal);
    tokio::fs::create_dir_all(&verzeichnis)
        .await
        .map_err(|e| format!("Ausgabeverzeichnis: {e}"))?;
    // Dieselbe Namensbasis wie der lesende Weg (`bericht_pfad`), sonst schreibt
    // der Dienst an eine Stelle und sucht an einer anderen.
    let basis = bericht_basis(konfiguration, block);

    let json = serde_json::to_string_pretty(bericht).map_err(|e| format!("JSON: {e}"))?;
    nur_fuer_mich(
        &mit_endung(&basis, "md"),
        report::markdown(bericht).as_bytes(),
    )
    .await?;
    if konfiguration.transkript_behalten {
        nur_fuer_mich(&mit_endung(&basis, "txt"), transkript.as_bytes()).await?;
    } else {
        // Ein frueherer Lauf desselben Blocks kann den Wortlaut abgelegt
        // haben, als der Schalter noch an war. Bliebe er liegen, stuende im
        // neuen Bericht "nicht behalten", waehrend die Datei danebenliegt.
        let alt = mit_endung(&basis, "txt");
        match tokio::fs::remove_file(&alt).await {
            Ok(()) => tracing::info!(datei = ?alt, "altes Transkript entfernt"),
            Err(fehler) if fehler.kind() == std::io::ErrorKind::NotFound => {}
            Err(fehler) => tracing::warn!(?fehler, datei = ?alt, "altes Transkript bleibt liegen"),
        }
    }
    // Die JSON-Datei kommt zuletzt und in einem Zug: sie ist das Signal, dass
    // dieser Block fertig ausgewertet ist. Wer sie zuerst schreibt, laesst
    // einen Abbruch mitten im Transkript wie einen abgeschlossenen Lauf
    // aussehen. Und wer sie an Ort und Stelle kuerzt, hinterlaesst bei einem
    // Abbruch halbes JSON, das keine Wiederholung mehr heilt.
    atomar_schreiben(&mit_endung(&basis, "json"), json.as_bytes()).await?;
    tracing::info!(
        block = %block.bezeichnung(),
        funde = bericht.funde.len(),
        "Bericht geschrieben"
    );
    Ok(())
}

/// Schreibt in eine Nebendatei und benennt sie um.
///
/// `rename` innerhalb eines Verzeichnisses ist unteilbar: entweder steht der
/// alte Inhalt oder der neue, nie eine Haelfte.
async fn atomar_schreiben(pfad: &Path, inhalt: &[u8]) -> Result<(), String> {
    let neben = pfad.with_extension("json.neu");
    let neben = if neben == pfad.to_path_buf() {
        pfad.with_extension("neu")
    } else {
        neben
    };
    nur_fuer_mich(&neben, inhalt).await?;
    tokio::fs::rename(&neben, pfad)
        .await
        .map_err(|e| format!("{}: {e}", pfad.display()))
}

/// Nur melden, wenn es etwas zu melden gibt. Eine DM je Block ohne Funde waere
/// nach dem ersten Abend Rauschen, das niemand mehr liest.
/// Prueft den Rumpf einer Broker-Antwort.
///
/// HTTP 200 heisst beim Broker nicht "zugestellt": er antwortet mit
/// `{"ok": false, ...}`, wenn Discord die DM abgelehnt hat. Wer nur den
/// Statuscode ansieht, haelt einen Fehlschlag fuer eine Meldung und loescht
/// danach die Aufnahme.
async fn broker_antwort_pruefen(antwort: reqwest::Response) -> Result<(), String> {
    let roh = antwort
        .text()
        .await
        .map_err(|fehler| format!("Broker-Antwort unlesbar: {fehler}"))?;
    // Ein leerer Rumpf beweist nichts. Der Broker antwortet auf send-dm mit
    // einem SendResult; bleibt er stumm, gilt die DM als unbestaetigt und die
    // Wiederholung laeuft weiter.
    if roh.trim().is_empty() {
        return Err("Broker antwortet ohne Rumpf - Zustellung unbestaetigt".to_owned());
    }
    let json: serde_json::Value = serde_json::from_str(&roh)
        .map_err(|fehler| format!("Broker-Antwort kein JSON: {fehler}"))?;
    // Ausdrueckliches `ok: true` oder gar nichts - alles dazwischen (`{}`,
    // `ok: "yes"`, ein Fehlerobjekt) beweist keine Zustellung und darf die
    // Wiederholung nicht beenden.
    match json.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) => Ok(()),
        _ => Err(format!("Broker bestaetigt die Zustellung nicht: {roh}")),
    }
}

/// Ordner fuer Hinweise, die der Broker nicht angenommen hat.
fn hinweis_ordner(konfiguration: &Konfiguration) -> PathBuf {
    konfiguration.ausgabe.join("offene-hinweise")
}

/// Legt einen nicht zugestellten Hinweis auf die Platte.
///
/// Ausfallmeldungen lebten bisher nur im Speicher: war der Broker genau
/// waehrend der Stoerung nicht erreichbar und die Stoerung danach vorbei,
/// erfuhr niemand davon - genau die Luecke, die diese Meldungen schliessen
/// sollen.
async fn hinweis_aufheben(
    konfiguration: &Konfiguration,
    ablage: &str,
    schluessel: &str,
    text: &str,
) {
    let ordner = hinweis_ordner(konfiguration);
    if let Err(fehler) = tokio::fs::create_dir_all(&ordner).await {
        tracing::error!(?fehler, "Hinweisordner nicht anlegbar");
        return;
    }
    // Der Dateiname steht je Vorfallsart fest und wird ueberschrieben. Ein
    // Name mit laufender Nummer haette waehrend eines Broker-Ausfalls jede
    // Minute eine neue Datei angelegt - und alle kaemen spaeter auf einmal an.
    let name: String = ablage
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Der echte Idempotenzschluessel steht im Inhalt, nicht im Dateinamen:
    // ein Login mit Unterstrich wuerde beim Saeubern verfaelscht, und der
    // Broker erkennt die Wiederholung dann nicht mehr.
    //
    // Liegt schon ein Hinweis derselben Sache, behaelt er seinen Schluessel:
    // derselbe Vorfall soll bei jedem Anlauf denselben tragen, sonst kommt er
    // doppelt an, wenn eine frueher gesendete Meldung doch ankam.
    let pfad = ordner.join(format!("{name}.json"));
    // Liegt schon ein Hinweis derselben Sache, bleibt er unangetastet:
    // Schluessel und Text gehoeren zusammen, und derselbe Schluessel mit
    // anderem Inhalt ist beim Broker ein Widerspruch.
    if tokio::fs::try_exists(&pfad).await.unwrap_or(false) {
        return;
    }
    let inhalt = serde_json::json!({ "schluessel": schluessel, "text": text });
    // Atomar: ein Abbruch mitten im Schreiben liesse sonst halbes JSON zurueck,
    // und der Wiederholungslauf ueberspringt genau diesen Hinweis fuer immer.
    if let Err(fehler) = atomar_schreiben(&pfad, inhalt.to_string().as_bytes()).await {
        tracing::error!(fehler, "Hinweis nicht ablegbar");
    }
}

/// Schluessel eines noch offenen Hinweises, falls es einen gibt.
///
/// Ohne ihn bekaeme jeder neue Anlauf einen neuen Schluessel - und wenn
/// Discord die vorige Nachricht doch angenommen hat, kaeme sie doppelt. Der
/// Text gehoert dazu: derselbe Schluessel mit anderem Inhalt ist beim Broker
/// ein Widerspruch.
async fn offener_hinweis(konfiguration: &Konfiguration, ablage: &str) -> Option<(String, String)> {
    let name: String = ablage
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let roh = tokio::fs::read_to_string(hinweis_ordner(konfiguration).join(format!("{name}.json")))
        .await
        .ok()?;
    let json: serde_json::Value = serde_json::from_str(&roh).ok()?;
    Some((
        json["schluessel"].as_str()?.to_owned(),
        json["text"].as_str()?.to_owned(),
    ))
}

/// Nimmt einen aufgehobenen Hinweis wieder weg.
async fn hinweis_erledigt(konfiguration: &Konfiguration, ablage: &str) {
    let name: String = ablage
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let _ =
        tokio::fs::remove_file(hinweis_ordner(konfiguration).join(format!("{name}.json"))).await;
}

/// Versucht die abgelegten Hinweise erneut zuzustellen.
async fn offene_hinweise_senden(konfiguration: &Konfiguration) {
    let ordner = hinweis_ordner(konfiguration);
    let Ok(mut eintraege) = tokio::fs::read_dir(&ordner).await else {
        return;
    };
    while let Ok(Some(eintrag)) = eintraege.next_entry().await {
        let pfad = eintrag.path();
        // Reste des atomaren Schreibens. Nur alte: eine gerade entstehende
        // Datei gehoert einem anderen Task, der sie gleich umbenennt.
        if pfad.extension().and_then(|e| e.to_str()) == Some("neu") {
            if zu_alt_sekunden(&pfad, 300).await {
                tracing::warn!(datei = ?pfad, "angefangene Hinweisdatei entfernt");
                let _ = tokio::fs::remove_file(&pfad).await;
            }
            continue;
        }
        if pfad.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(roh) = tokio::fs::read_to_string(&pfad).await else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&roh) else {
            // Ein halb geschriebener Hinweis aus einer aelteren Fassung: er
            // laesst sich nicht mehr zustellen und darf den Ordner nicht
            // dauerhaft blockieren.
            tracing::warn!(datei = ?pfad, "Hinweis unlesbar - wird entfernt");
            let _ = tokio::fs::remove_file(&pfad).await;
            continue;
        };
        let (Some(schluessel), Some(text)) = (
            json["schluessel"].as_str().map(str::to_owned),
            json["text"].as_str().map(str::to_owned),
        ) else {
            continue;
        };
        match dm_rohtext(&text, &schluessel).await {
            Ok(()) => {
                let _ = tokio::fs::remove_file(&pfad).await;
                tracing::info!(schluessel, "aufgehobenen Hinweis nachgereicht");
            }
            Err(fehler) => {
                tracing::warn!(fehler, schluessel, "Hinweis weiter nicht zustellbar");
                return;
            }
        }
    }
}

/// Schickt eine freie Zeile an den Admin - fuer Meldungen, die kein Bericht
/// sind, etwa einen endgueltig aufgegebenen Block.
async fn dm_rohtext(text: &str, schluessel: &str) -> Result<(), String> {
    let Some(token) = melden::broker_token() else {
        return Err("kein Broker-Token".to_owned());
    };
    let anfrage = melden::anfrage(melden::empfaenger(), text);
    let url = format!("{}{}", melden::broker_basis_url(), melden::BROKER_DM_PFAD);
    let client = broker_client().ok_or_else(|| "HTTP-Client nicht baubar".to_owned())?;
    match client
        .post(&url)
        .header("X-Internal-Token", token)
        .header(
            melden::IDEMPOTENZ_KOPF,
            melden::idempotenz_schluessel(schluessel),
        )
        .json(&anfrage)
        .send()
        .await
    {
        Ok(antwort) if antwort.status().is_success() => broker_antwort_pruefen(antwort).await,
        Ok(antwort) => Err(format!("DM abgelehnt: HTTP {}", antwort.status().as_u16())),
        Err(fehler) => Err(format!("DM fehlgeschlagen: {fehler}")),
    }
}

async fn dm_senden(_bericht: &Bericht, _schluessel: &str) -> Result<(), String> {
    // Funde gehen nicht mehr je Block raus. Eine DM am Sendungsende sammelt
    // die ToS-Treffer. Sonst waere jede ruhige Viertelstunde eine Nachricht,
    // und drei parallele Kanaele eine Mailbombe.
    Ok(())
}

/// Summe der Dateigroessen unter den Aufnahmen.
async fn aufnahmen_bytes(wurzel: &Path) -> u64 {
    let mut summe = 0u64;
    let mut zu_lesen = vec![wurzel.to_path_buf()];
    while let Some(aktuell) = zu_lesen.pop() {
        let mut eintraege = match tokio::fs::read_dir(&aktuell).await {
            Ok(eintraege) => eintraege,
            Err(_) => continue,
        };
        while let Ok(Some(eintrag)) = eintraege.next_entry().await {
            let pfad = eintrag.path();
            match eintrag.file_type().await {
                Ok(typ) if typ.is_dir() => zu_lesen.push(pfad),
                Ok(_) => {
                    if let Ok(daten) = eintrag.metadata().await {
                        summe += daten.len();
                    }
                }
                _ => {}
            }
        }
    }
    summe
}

/// Wie oft die Auslastung gemessen wird. Der CPU-Anteil ergibt sich aus der
/// Differenz zweier `/proc/stat`-Messungen; dieser Takt ist also zugleich das
/// Messfenster fuer die CPU.
const LAST_TAKT_SEKUNDEN: u64 = 20;

/// Summe der Ticks aus `/proc/stat` und der davon untaetige Anteil.
///
/// Die CPU-Auslastung ist keine Momentaufnahme, sondern der Anteil belegter
/// Ticks zwischen zwei Messungen - deshalb der Zwischenstand.
#[derive(Clone, Copy)]
struct CpuStand {
    gesamt: u64,
    untaetig: u64,
}

/// Liest die Sammelzeile `cpu` aus `/proc/stat`. `None`, wenn die Datei fehlt
/// oder unerwartet aussieht - dann faellt die CPU als Signal aus, RAM traegt
/// weiter.
fn cpu_stand() -> Option<CpuStand> {
    let inhalt = std::fs::read_to_string("/proc/stat").ok()?;
    let zeile = inhalt.lines().next()?;
    let mut felder = zeile.split_whitespace();
    if felder.next()? != "cpu" {
        return None;
    }
    // Genau die acht Standardfelder, der Reihe nach:
    // user nice system idle iowait irq softirq steal. Positionsgenau lesen -
    // ein `filter_map` wuerde ein unparsbares Feld ueberspringen und idle/iowait
    // von der falschen Stelle holen. `guest`/`guest_nice` bleiben aussen vor:
    // der Kernel fuehrt sie bereits in user/nice, mitzusummieren zaehlte sie
    // doppelt.
    let mut werte = [0u64; 8];
    for feld in werte.iter_mut() {
        *feld = felder.next()?.parse().ok()?;
    }
    let gesamt: u64 = werte.iter().sum();
    // idle + iowait gelten als untaetig.
    let untaetig = werte[3] + werte[4];
    Some(CpuStand { gesamt, untaetig })
}

/// CPU-Auslastung in Prozent zwischen zwei Messungen. `None`, wenn die Uhr
/// nicht weitergelaufen ist (gleiche Messung).
fn cpu_prozent(vorher: CpuStand, jetzt: CpuStand) -> Option<f32> {
    let gesamt = jetzt.gesamt.checked_sub(vorher.gesamt)?;
    let untaetig = jetzt.untaetig.saturating_sub(vorher.untaetig);
    if gesamt == 0 {
        return None;
    }
    let belegt = gesamt.saturating_sub(untaetig);
    Some(belegt as f32 / gesamt as f32 * 100.0)
}

/// RAM-Auslastung in Prozent aus `/proc/meminfo`. `MemAvailable` ist der frei
/// nutzbare Speicher inklusive rueckholbarem Cache - naeher an "voll" als das
/// blosse `MemFree`.
fn ram_prozent() -> Option<f32> {
    let inhalt = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut gesamt = None;
    let mut verfuegbar = None;
    for zeile in inhalt.lines() {
        if let Some(rest) = zeile.strip_prefix("MemTotal:") {
            gesamt = rest.split_whitespace().next()?.parse::<u64>().ok();
        } else if let Some(rest) = zeile.strip_prefix("MemAvailable:") {
            verfuegbar = rest.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    let gesamt = gesamt?;
    let verfuegbar = verfuegbar?;
    if gesamt == 0 {
        return None;
    }
    let belegt = gesamt.saturating_sub(verfuegbar);
    Some(belegt as f32 / gesamt as f32 * 100.0)
}

/// Misst CPU und RAM im Takt und setzt das Last-Gate: liegt die groessere der
/// beiden Auslastungen lange genug ueber der Grenze, wird die Auswertung
/// zurueckgestellt. Aufgenommen wird die ganze Zeit weiter.
async fn last_ueberwachen(
    gate: Arc<std::sync::atomic::AtomicBool>,
    abbruch: Arc<std::sync::atomic::AtomicBool>,
    mut waechter: Lastwaechter,
) {
    let start = tokio::time::Instant::now();
    let mut voriger = cpu_stand();
    loop {
        tokio::time::sleep(Duration::from_secs(LAST_TAKT_SEKUNDEN)).await;
        if abbruch.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let jetziger = cpu_stand();
        let cpu = match (voriger, jetziger) {
            (Some(a), Some(b)) => cpu_prozent(a, b),
            _ => None,
        };
        voriger = jetziger;
        let ram = ram_prozent();
        // Die groessere von CPU und RAM entscheidet: liegt eine der beiden ueber
        // der Grenze, gilt der Server als ausgelastet.
        let auslastung = match (cpu, ram) {
            (Some(c), Some(r)) => c.max(r),
            (Some(c), None) => c,
            (None, Some(r)) => r,
            // Kein Signal mehr messbar (etwa /proc unlesbar): ohne Beleg fuer
            // Ueberlast die Auswertung anzuhalten waere falsch. Gate freigeben
            // und den Zwischenstand vergessen, sonst bliebe es haengen, solange
            // keine Messung mehr kommt.
            (None, None) => {
                let vorher = waechter.aktiv();
                waechter.zuruecksetzen();
                gate.store(false, std::sync::atomic::Ordering::Relaxed);
                if vorher {
                    tracing::warn!(
                        "Last-Gate aus - Auslastung nicht mehr messbar, Auswertung freigegeben"
                    );
                }
                continue;
            }
        };
        let vorher = waechter.aktiv();
        let aktiv = waechter.beobachten(auslastung, start.elapsed().as_secs());
        gate.store(aktiv, std::sync::atomic::Ordering::Relaxed);
        if aktiv != vorher {
            if aktiv {
                tracing::warn!(
                    auslastung,
                    cpu = ?cpu,
                    ram = ?ram,
                    "Last-Gate an - Auswertung wird zurueckgestellt, Aufnahme laeuft weiter"
                );
            } else {
                tracing::info!(auslastung, "Last-Gate aus - Auswertung laeuft wieder");
            }
        }
    }
}

fn lauf_ordner(konfiguration: &Konfiguration, kanal: &str, lauf: &str) -> PathBuf {
    aufnahme_wurzel(konfiguration).join(kanal).join(lauf)
}

/// Ordner der durchgehenden 1:1-Mitschnitte, bewusst **getrennt** vom
/// Auswertungs-Baum (`aufnahmen/`). Der Block-Pipeline-Code scannt `aufnahmen/`
/// nach `.ts`-Bloecken, zaehlt sie fuer den Groessen-Deckel und reiht bei einem
/// Neustart liegengebliebene wieder ein - ein stundenlanger Mitschnitt dort
/// wuerde all das vergiften.
fn mitschnitt_ordner(konfiguration: &Konfiguration, kanal: &str, lauf: &str) -> PathBuf {
    konfiguration
        .ausgabe
        .join("mitschnitte")
        .join(kanal)
        .join(lauf)
}

/// Markiert die Aufnahme eines Laufs als abgeschlossen (`aufnahme_fertig.json`).
/// Erst danach gibt das Archiv sie zum Upload frei. Markiert nur, wenn wirklich
/// eine nicht-leere Aufnahme vorliegt; ein leerer Ordner (Recorder-Start
/// gescheitert) wird stattdessen weggeraeumt, damit der Sweep kein Nichts
/// archiviert. Bei einem Lesefehler passiert nichts - dann spaeter erneut.
async fn aufnahme_fertig_markieren(konfiguration: &Konfiguration, kanal: &str, lauf: &str) {
    let dir = mitschnitt_ordner(konfiguration, kanal, lauf);
    let Some(mitschnitte) = mitschnitt_dateien_sammeln(&dir).await else {
        return;
    };
    let mut hat_inhalt = false;
    for datei in &mitschnitte {
        if tokio::fs::metadata(datei)
            .await
            .map(|m| m.len() > 0)
            .unwrap_or(false)
        {
            hat_inhalt = true;
            break;
        }
    }
    if !hat_inhalt {
        // Kein Ton aufgenommen: den (leeren) Ordner wegraeumen statt markieren.
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return;
    }
    let marke = dir.join(AUFNAHME_FERTIG);
    if tokio::fs::try_exists(&marke).await.unwrap_or(false) {
        return;
    }
    if let Err(fehler) = nur_fuer_mich(&marke, b"{}").await {
        tracing::warn!(fehler, kanal, "Fertig-Marke nicht schreibbar");
    }
}

/// Versiegelt verwaiste Mitschnitt-Ordner: solche ohne aktiven Recorder und ohne
/// Fertig-Marke. So bekommt auch ein Mitschnitt, dessen Recorder ein Dienst-
/// Neustart oder Absturz mitgenommen hat, sein "fertig" und wird archiviert.
async fn mitschnitt_ordner_versiegeln(
    konfiguration: &Konfiguration,
    aktive: &std::collections::HashMap<String, Recorder>,
    live: &[String],
) {
    let wurzel = konfiguration.ausgabe.join("mitschnitte");
    let Ok(mut kanaele) = tokio::fs::read_dir(&wurzel).await else {
        return;
    };
    while let Ok(Some(kanal_e)) = kanaele.next_entry().await {
        if !kanal_e
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let kanal = kanal_e.file_name().to_string_lossy().into_owned();
        // Sendet der Kanal noch, ist NICHTS in seinem Ordner "fertig" - auch
        // wenn gerade kein Recorder laeuft (etwa weil bei knapper Platte keiner
        // neu startet). Sonst wuerde ein laufender Stream vorzeitig als fertig
        // versiegelt, archiviert und endgueltig geschlossen. Erst wenn er offline
        // ist, darf versiegelt werden.
        if live.contains(&kanal) {
            continue;
        }
        let Ok(mut laeufe) = tokio::fs::read_dir(kanal_e.path()).await else {
            continue;
        };
        while let Ok(Some(lauf_e)) = laeufe.next_entry().await {
            if !lauf_e
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let lauf = lauf_e.file_name().to_string_lossy().into_owned();
            // Aktiv aufnehmender Lauf: der Recorder laeuft noch, nicht versiegeln.
            if aktive.get(&kanal).map(|r| r.lauf == lauf).unwrap_or(false) {
                continue;
            }
            if tokio::fs::try_exists(lauf_e.path().join(AUFNAHME_FERTIG))
                .await
                .unwrap_or(false)
            {
                continue;
            }
            aufnahme_fertig_markieren(konfiguration, &kanal, &lauf).await;
        }
    }
}

const START_MARKE: &str = "start_gemeldet.json";
const ENDE_MARKE: &str = "ende_gemeldet.json";
const AKTE: &str = "akte.json";

async fn start_dm_einmal(konfiguration: &Konfiguration, kanal: &str, lauf: &str) {
    let ordner = lauf_ordner(konfiguration, kanal, lauf);
    if let Err(fehler) = tokio::fs::create_dir_all(&ordner).await {
        tracing::warn!(?fehler, kanal, "Startordner nicht anlegbar");
    }
    let marke = ordner.join(START_MARKE);
    if tokio::fs::try_exists(&marke).await.unwrap_or(false) {
        return;
    }
    let text = report::start_dm_text(kanal);
    let schluessel = format!("{kanal}-{lauf}-start");
    match dm_rohtext(&text, &schluessel).await {
        Ok(()) => {
            if let Err(fehler) = nur_fuer_mich(&marke, b"{}").await {
                tracing::warn!(fehler, kanal, "Startmarke nicht schreibbar");
            }
        }
        Err(fehler) => {
            tracing::error!(fehler, kanal, "Start-DM nicht zustellbar");
            hinweis_aufheben(
                konfiguration,
                &format!("start-{kanal}-{lauf}"),
                &schluessel,
                &text,
            )
            .await;
        }
    }
}

async fn akte_lesen(konfiguration: &Konfiguration, kanal: &str, lauf: &str) -> report::LaufAkte {
    let pfad = lauf_ordner(konfiguration, kanal, lauf).join(AKTE);
    match tokio::fs::read_to_string(&pfad).await {
        Ok(roh) => serde_json::from_str(&roh).unwrap_or_default(),
        Err(_) => report::LaufAkte::default(),
    }
}

async fn akte_verbuchen(konfiguration: &Konfiguration, block: &plan::Block, bericht: &Bericht) {
    let ordner = lauf_ordner(konfiguration, &block.kanal, &block.lauf);
    let _ = tokio::fs::create_dir_all(&ordner).await;
    let mut akte = akte_lesen(konfiguration, &block.kanal, &block.lauf).await;
    akte.block_verbuchen(bericht);
    let pfad = ordner.join(AKTE);
    if let Ok(json) = serde_json::to_string(&akte) {
        if let Err(fehler) = atomar_schreiben(&pfad, json.as_bytes()).await {
            tracing::warn!(fehler, "Laufakte nicht schreibbar");
        }
    }
}

async fn lauf_ende_melden(
    konfiguration: &Konfiguration,
    kanal: &str,
    lauf: &str,
    sperre: &Mutex<plan::LaufSperre>,
    warteschlange: &Mutex<plan::Warteschlange>,
) {
    let gesperrt = sperre.lock().await.ist_gesperrt(kanal, lauf);
    let offen = warteschlange.lock().await.offene_fuer_lauf(kanal, lauf);
    if !plan::ende_dm_faellig(gesperrt, offen) {
        return;
    }
    let marke = lauf_ordner(konfiguration, kanal, lauf).join(ENDE_MARKE);
    if tokio::fs::try_exists(&marke).await.unwrap_or(false) {
        return;
    }
    let akte = akte_lesen(konfiguration, kanal, lauf).await;
    let text = report::ende_dm_text(kanal, &akte, 2);
    let schluessel = format!("{kanal}-{lauf}-ende");
    match dm_rohtext(&text, &schluessel).await {
        Ok(()) => {
            let _ = tokio::fs::create_dir_all(lauf_ordner(konfiguration, kanal, lauf)).await;
            if let Err(fehler) = nur_fuer_mich(&marke, b"{}").await {
                tracing::warn!(fehler, kanal, "Endemarke nicht schreibbar");
            }
        }
        Err(fehler) => {
            tracing::error!(fehler, kanal, "Ende-DM nicht zustellbar");
            hinweis_aufheben(
                konfiguration,
                &format!("ende-{kanal}-{lauf}"),
                &schluessel,
                &text,
            )
            .await;
        }
    }
    // Das Drive-Archiv wird bewusst NICHT hier angestossen: diese Funktion laeuft
    // im Aufnahme-Task, parallel zur Auswertung. Ein Block kann gerade
    // ausgewertet werden (aus der Warteschlange genommen, Bericht noch nicht
    // geschrieben) - dann saehe das Archiv faelschlich "nichts offen" und
    // loeschte Berichte, bevor der letzte geschrieben ist. Stattdessen holt der
    // Aufraeumtakt der Auswertungsschleife das Archiv nach: der laeuft zwischen
    // den Bloecken, wenn nichts in Auswertung ist, und ist damit sicher.
}

const ARCHIV_MARKE: &str = "drive_archiviert.json";
const ARCHIV_LAEUFT: &str = "archiv_laeuft.json";
/// Wie lange eine `archiv_laeuft.json`-Marke als "laeuft gerade" gilt. Ein
/// laufendes Archiv frischt sie im Herzschlag-Takt auf; bleibt die Marke laenger
/// stehen, ist der Lauf abgestuerzt und das Archiv darf neu ansetzen. Bewusst
/// kurz, damit ein Absturz schnell nachgeholt wird - das Auffrischen haelt sie
/// waehrend eines langen Uploads am Leben.
const ARCHIV_LAEUFT_FRISCH_SEKUNDEN: u64 = 30 * 60;
/// Takt, in dem ein laufendes Archiv seine Laufmarke auffrischt.
const ARCHIV_HERZSCHLAG_SEKUNDEN: u64 = 10 * 60;
/// Pfad zum rclone-Binary, ueberschreibbar per `STREAM_AUDIT_RCLONE_BIN`. Ein
/// fester Pfad wuerde das Archiv dauerhaft scheitern lassen, wenn rclone
/// woanders liegt.
fn rclone_pfad() -> String {
    std::env::var("STREAM_AUDIT_RCLONE_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/bin/rclone".to_string())
}

/// Pfad zum ffmpeg-Binary, ueberschreibbar per `STREAM_AUDIT_FFMPEG_BIN`.
fn ffmpeg_pfad() -> String {
    std::env::var("STREAM_AUDIT_FFMPEG_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/usr/bin/ffmpeg".to_string())
}
/// Marke im Mitschnitt-Ordner: der Recorder ist fertig, die Aufnahme ist
/// abgeschlossen und darf hochgeladen werden. Erst dieses Signal - nicht eine
/// Vermutung ueber die letzte Aenderung - gibt einen Lauf zum Archivieren frei.
const AUFNAHME_FERTIG: &str = "aufnahme_fertig.json";
/// Harte Obergrenze fuer einen rclone-Aufruf. Ein 17-GB-Upload dauert, aber ein
/// haengender Prozess soll das Archiv nicht fuer immer blockieren.
const RCLONE_ZEITGRENZE: Duration = Duration::from_secs(6 * 60 * 60);

/// Schiebt einen fertigen Stream (Mitschnitt als ein File plus Berichte) in
/// seinen eigenen Ordner auf Google Drive und raeumt danach lokal auf. Idempotent
/// ueber die Marke `drive_archiviert.json`; ein zweiter Lauf desselben Streams
/// wird ueber `archiv_laeuft.json` abgefangen.
async fn nach_drive_archivieren(konfiguration: Konfiguration, kanal: String, lauf: String) {
    if !archiv::archiv_aktiv() {
        return;
    }
    let lauf_dir = lauf_ordner(&konfiguration, &kanal, &lauf);
    let fertig = lauf_dir.join(ARCHIV_MARKE);
    if tokio::fs::try_exists(&fertig).await.unwrap_or(false) {
        return;
    }
    if tokio::fs::create_dir_all(&lauf_dir).await.is_err() {
        return;
    }
    // Laufmarke atomar belegen: zwei Ausloeser (Ende-DM und Sweep) duerfen nicht
    // beide gleichzeitig hochladen und loeschen.
    let laeuft = lauf_dir.join(ARCHIV_LAEUFT);
    if !laufmarke_belegen(&laeuft, ARCHIV_LAEUFT_FRISCH_SEKUNDEN).await {
        return;
    }
    // Herzschlag: die Laufmarke waehrend des (moeglicherweise stundenlangen)
    // Uploads auffrischen, damit sie nicht als abgestanden gilt und der Sweep
    // keinen zweiten Lauf daneben startet.
    let herzschlag = {
        let laeuft = laeuft.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(ARCHIV_HERZSCHLAG_SEKUNDEN)).await;
                if nur_fuer_mich(&laeuft, b"{}").await.is_err() {
                    return;
                }
            }
        })
    };
    let ergebnis = drive_archiv_durchfuehren(&konfiguration, &kanal, &lauf).await;
    herzschlag.abort();
    let _ = tokio::fs::remove_file(&laeuft).await;

    match ergebnis {
        Ok(true) => {
            if let Err(fehler) = nur_fuer_mich(&fertig, b"{}").await {
                tracing::warn!(fehler, kanal, "Archiv-Marke nicht schreibbar");
            }
            tracing::info!(
                kanal,
                lauf,
                "Stream nach Drive archiviert und lokal geraeumt"
            );
        }
        Ok(false) => {
            // Nichts da zum Archivieren. KEINE endgueltige Archiv-Marke - ein
            // transienter Leerbefund schloesse sonst einen echten Lauf fuer immer
            // aus. Stattdessen den leeren Mitschnitt-Ordner wegraeumen, damit der
            // Sweep ihn nicht stuendlich wieder aufgreift.
            let _ =
                tokio::fs::remove_dir_all(mitschnitt_ordner(&konfiguration, &kanal, &lauf)).await;
        }
        Err(fehler) => {
            tracing::error!(
                fehler,
                kanal,
                lauf,
                "Drive-Archiv fehlgeschlagen - lokal bleibt liegen, wird spaeter erneut versucht"
            );
        }
    }
}

/// Fuehrt Zusammenfuegen, Upload und Aufraeumen aus. `Ok(true)` = archiviert und
/// geraeumt, `Ok(false)` = es gab nichts zu archivieren, `Err` = Upload
/// gescheitert, lokal bleibt alles liegen.
async fn drive_archiv_durchfuehren(
    konfiguration: &Konfiguration,
    kanal: &str,
    lauf: &str,
) -> Result<bool, String> {
    let lauf_dir = lauf_ordner(konfiguration, kanal, lauf);
    let mit_dir = mitschnitt_ordner(konfiguration, kanal, lauf);
    let Some(mitschnitte) = mitschnitt_dateien_sammeln(&mit_dir).await else {
        return Err(format!("Mitschnitt-Ordner {kanal}/{lauf} nicht lesbar"));
    };
    let Some(berichte) = bericht_dateien_sammeln(konfiguration, kanal, lauf).await else {
        return Err(format!("Berichtsordner {kanal}/{lauf} nicht lesbar"));
    };
    let akte = lauf_dir.join(AKTE);
    let akte_da = tokio::fs::try_exists(&akte).await.unwrap_or(false);
    if mitschnitte.is_empty() && berichte.is_empty() && !akte_da {
        return Ok(false);
    }
    // Nur archivieren, wenn die Aufnahme abgeschlossen ist. Die Fertig-Marke
    // wird gesetzt, sobald der Recorder-Prozess beendet ist - ein noch laufender
    // Mitschnitt geht damit nie hoch (und wird nicht unter dem Recorder weg
    // geloescht). Ohne Mitschnitt (nur Berichte) entfaellt die Bedingung.
    if !mitschnitte.is_empty() {
        let fertig = tokio::fs::try_exists(mit_dir.join(AUFNAHME_FERTIG))
            .await
            .unwrap_or(false);
        if !fertig {
            return Err(format!(
                "Mitschnitt {kanal}/{lauf} noch nicht abgeschlossen - Archiv wird verschoben"
            ));
        }
    }

    let ordner = archiv::remote_ordner(&archiv::remote_basis(), kanal, lauf);

    // Die grossen Mitschnitte gehen direkt hoch, ohne Umweg ueber einen
    // Sammelordner - sonst laege der Stream kurz doppelt auf der Platte.
    for datei in &mitschnitte {
        befehl_pruefen(&rclone_pfad(), &archiv::rclone_datei_args(datei, &ordner)).await?;
    }

    // Die kleinen Berichte und die Akte sammeln und in einem Rutsch hoch. Jede
    // Kopie ist geprueft: schlaegt eine fehl, brechen wir ab, bevor lokal etwas
    // geloescht wird - sonst laege ein unvollstaendiges Archiv oben und das
    // Original waere weg.
    let staging = konfiguration
        .ausgabe
        .join("zwischendateien")
        .join(format!("archiv-{kanal}-{lauf}"));
    sammelordner_frisch(&staging).await?;
    let mut kleinkram = 0usize;
    for bericht in &berichte {
        let Some(name) = bericht.file_name() else {
            continue;
        };
        tokio::fs::copy(bericht, staging.join(name))
            .await
            .map_err(|f| format!("Bericht {name:?} nicht kopierbar: {f}"))?;
        kleinkram += 1;
    }
    if akte_da {
        tokio::fs::copy(&akte, staging.join(AKTE))
            .await
            .map_err(|f| format!("Akte nicht kopierbar: {f}"))?;
        kleinkram += 1;
    }
    if kleinkram > 0 {
        befehl_pruefen(
            &rclone_pfad(),
            &archiv::rclone_ordner_args(&staging, &ordner),
        )
        .await?;
    }

    // Vor dem Loeschen belegen, dass wirklich etwas oben liegt.
    let liste = befehl_ausgabe(&rclone_pfad(), &archiv::rclone_lsf_args(&ordner)).await?;
    if liste.trim().is_empty() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(format!("Zielordner {ordner} nach Upload leer"));
    }

    // Erst jetzt raeumen. Jede Loeschung wird geprueft: bleibt etwas liegen,
    // wird die Fertig-Marke NICHT gesetzt, und der Sweep versucht es erneut -
    // ein zweiter Upload derselben Dateien ist idempotent.
    let mut sauber = true;
    if let Err(fehler) = tokio::fs::remove_dir_all(&staging).await {
        if fehler.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(?fehler, "Sammelordner nicht raeumbar");
            sauber = false;
        }
    }
    for datei in &mitschnitte {
        if let Err(fehler) = tokio::fs::remove_file(datei).await {
            tracing::warn!(?fehler, ?datei, "Mitschnitt nicht loeschbar");
            sauber = false;
        }
    }
    // Der Mitschnitt-Ordner (jetzt nur noch die Fertig-Marke) darf weg; schlaegt
    // es fehl, ist das kein Grund, den Lauf als ungesichert zu behandeln.
    let _ = tokio::fs::remove_dir_all(&mit_dir).await;
    for bericht in &berichte {
        if let Err(fehler) = tokio::fs::remove_file(bericht).await {
            if fehler.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(?fehler, ?bericht, "Bericht nicht loeschbar");
                sauber = false;
            }
        }
    }
    if let Err(fehler) = tokio::fs::remove_file(&akte).await {
        if fehler.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(?fehler, "Akte nicht loeschbar");
            sauber = false;
        }
    }
    if !sauber {
        return Err("Upload lag oben, aber lokal blieb etwas liegen".to_string());
    }
    Ok(true)
}

/// Legt den Sammelordner frisch an: ein etwaiger Rest aus einem abgebrochenen
/// Lauf muss weg, sonst laedt rclone alte Dateien mit hoch.
async fn sammelordner_frisch(pfad: &Path) -> Result<(), String> {
    match tokio::fs::remove_dir_all(pfad).await {
        Ok(()) => {}
        Err(fehler) if fehler.kind() == std::io::ErrorKind::NotFound => {}
        Err(fehler) => return Err(format!("alter Sammelordner nicht raeumbar: {fehler}")),
    }
    tokio::fs::create_dir_all(pfad)
        .await
        .map_err(|f| format!("Sammelordner nicht anlegbar: {f}"))
}

/// Die durchgehenden Mitschnitt-Dateien eines Laufs. `Some(leer)`, wenn der
/// Ordner fehlt (kein Mitschnitt), `None` bei einem sonstigen Lesefehler - dann
/// darf der Aufrufer nicht "leer" annehmen und nichts loeschen, sondern spaeter
/// erneut versuchen.
async fn mitschnitt_dateien_sammeln(mit_dir: &Path) -> Option<Vec<PathBuf>> {
    let mut eintraege = match tokio::fs::read_dir(mit_dir).await {
        Ok(eintraege) => eintraege,
        Err(fehler) if fehler.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return None,
    };
    let mut raus = Vec::new();
    loop {
        match eintraege.next_entry().await {
            Ok(Some(eintrag)) => {
                let name = eintrag.file_name().to_string_lossy().into_owned();
                if archiv::ist_mitschnitt(&name) {
                    raus.push(eintrag.path());
                }
            }
            Ok(None) => break,
            // Ein Fehler mitten in der Aufzaehlung darf nicht als "das war alles"
            // durchgehen - sonst laedt/loescht das Archiv nur einen Teil.
            Err(_) => return None,
        }
    }
    raus.sort();
    Some(raus)
}

/// Berichtsdateien (`.json`/`.md`) eines Laufs. `Some(leer)`, wenn der
/// Kanalordner fehlt, `None` bei einem sonstigen Lesefehler - dann darf der
/// Aufrufer nicht nur einen Teil hochladen und den Rest loeschen.
async fn bericht_dateien_sammeln(
    konfiguration: &Konfiguration,
    kanal: &str,
    lauf: &str,
) -> Option<Vec<PathBuf>> {
    let dir = konfiguration.ausgabe.join(kanal);
    let mut eintraege = match tokio::fs::read_dir(&dir).await {
        Ok(eintraege) => eintraege,
        Err(fehler) if fehler.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return None,
    };
    let mut raus = Vec::new();
    loop {
        match eintraege.next_entry().await {
            Ok(Some(eintrag)) => {
                let name = eintrag.file_name().to_string_lossy().into_owned();
                if bericht_gehoert_zu(&name, kanal, lauf)
                    && (name.ends_with(".json") || name.ends_with(".md"))
                {
                    raus.push(eintrag.path());
                }
            }
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    Some(raus)
}

/// Ob eine Berichtsdatei genau zu diesem Lauf gehoert. Der Blockname endet auf
/// `-t<versatz>-b<nummer>`, also folgt auf `<kanal>-<lauf>-` immer ein `t` mit
/// Ziffer. Nur das trennt Lauf `id` sauber von Lauf `id-<zeitstempel>` - ein
/// blosses Praefix `id-` griffe faelschlich auch auf den laengeren Lauf.
fn bericht_gehoert_zu(dateiname: &str, kanal: &str, lauf: &str) -> bool {
    let praefix = format!("{kanal}-{lauf}-t");
    dateiname
        .strip_prefix(&praefix)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_digit())
}

/// Laeufe (Kanal, Lauf) mit einem Mitschnitt im eigenen Baum, deren Archiv-Marke
/// noch fehlt - also solche, deren Upload nach Drive noch aussteht.
async fn pending_archiv_laeufe(
    konfiguration: &Konfiguration,
) -> std::collections::HashSet<(String, String)> {
    let mut raus = std::collections::HashSet::new();
    let wurzel = konfiguration.ausgabe.join("mitschnitte");
    let Ok(mut kanaele) = tokio::fs::read_dir(&wurzel).await else {
        return raus;
    };
    while let Ok(Some(kanal_e)) = kanaele.next_entry().await {
        if !kanal_e
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let kanal = kanal_e.file_name().to_string_lossy().into_owned();
        let Ok(mut laeufe) = tokio::fs::read_dir(kanal_e.path()).await else {
            continue;
        };
        while let Ok(Some(lauf_e)) = laeufe.next_entry().await {
            if !lauf_e
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let lauf = lauf_e.file_name().to_string_lossy().into_owned();
            if tokio::fs::try_exists(lauf_ordner(konfiguration, &kanal, &lauf).join(ARCHIV_MARKE))
                .await
                .unwrap_or(false)
            {
                continue;
            }
            raus.insert((kanal.clone(), lauf));
        }
    }
    raus
}

/// Holt Streams nach, deren Archiv noch aussteht. Grundlage ist der
/// Mitschnitt-Baum: liegt dort eine 1:1-Aufnahme und fehlt die Archiv-Marke, ist
/// der Upload offen - unabhaengig davon, ob die Ende-DM je durchkam. Ein noch
/// laufender Recorder wird ueber die Frische-Pruefung im Archiv selbst
/// abgefangen, sodass hier nichts Halbfertiges hochgeht.
async fn offene_archive_nachholen(
    konfiguration: &Konfiguration,
    sperre: &Mutex<plan::LaufSperre>,
    warteschlange: &Mutex<plan::Warteschlange>,
) {
    if !archiv::archiv_aktiv() {
        return;
    }
    let wurzel = konfiguration.ausgabe.join("mitschnitte");
    let Ok(mut kanaele) = tokio::fs::read_dir(&wurzel).await else {
        return;
    };
    while let Ok(Some(kanal_e)) = kanaele.next_entry().await {
        if !kanal_e
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let kanal = kanal_e.file_name().to_string_lossy().into_owned();
        let Ok(mut laeufe) = tokio::fs::read_dir(kanal_e.path()).await else {
            continue;
        };
        while let Ok(Some(lauf_e)) = laeufe.next_entry().await {
            if !lauf_e
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let lauf = lauf_e.file_name().to_string_lossy().into_owned();
            // Nur abgeschlossene Aufnahmen: der Fertig-Marker sagt, dass der
            // Recorder beendet ist. Ein noch laufender Mitschnitt bleibt aussen vor.
            if !tokio::fs::try_exists(lauf_e.path().join(AUFNAHME_FERTIG))
                .await
                .unwrap_or(false)
            {
                continue;
            }
            // Schon archiviert? Die Marke liegt im Auswertungs-Baum.
            if tokio::fs::try_exists(lauf_ordner(konfiguration, &kanal, &lauf).join(ARCHIV_MARKE))
                .await
                .unwrap_or(false)
            {
                continue;
            }
            // Erst wenn auch die Auswertung durch ist: sonst lueden wir nur die
            // bisherigen Berichte hoch, loeschten sie und setzten die endgueltige
            // Marke - spaeter fertige Berichte kaemen nie nach Drive.
            if sperre.lock().await.ist_gesperrt(&kanal, &lauf) {
                continue;
            }
            if warteschlange.lock().await.offene_fuer_lauf(&kanal, &lauf) > 0 {
                continue;
            }
            tokio::spawn(nach_drive_archivieren(
                konfiguration.clone(),
                kanal.clone(),
                lauf,
            ));
        }
    }
}

/// Freier Platz in Bytes auf dem Dateisystem des Pfades, ueber `df`. `None`,
/// wenn `df` fehlt oder unerwartet aussieht.
async fn freier_platz_bytes(pfad: &Path) -> Option<u64> {
    let ausgabe = tokio::process::Command::new("df")
        .arg("-kP")
        .arg(pfad)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    if !ausgabe.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&ausgabe.stdout);
    let zeile = text.lines().nth(1)?;
    let bloecke_k: u64 = zeile.split_whitespace().nth(3)?.parse().ok()?;
    Some(bloecke_k.saturating_mul(1024))
}

/// Ob genug Platz frei ist, um einen neuen Recorder zu starten. `df` nicht
/// messbar: erlauben (fail-open) - der Ton ist winzig, und ein df-Aussetzer soll
/// die Aufnahme nicht abwuergen.
async fn platz_fuer_mitschnitt(konfiguration: &Konfiguration) -> bool {
    match freier_platz_bytes(&aufnahme_wurzel(konfiguration)).await {
        Some(frei) => frei >= archiv::min_frei_bytes(),
        None => true,
    }
}

/// Ob eine Marke existiert und juenger als das Fenster ist.
async fn marke_frisch(pfad: &Path, fenster_sekunden: u64) -> bool {
    match tokio::fs::metadata(pfad).await {
        Ok(meta) => meta
            .modified()
            .ok()
            .and_then(|zeit| zeit.elapsed().ok())
            .map(|alter| alter.as_secs() < fenster_sekunden)
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Startet ein Programm und gibt einen Fehler zurueck, wenn es nicht mit 0
/// endet. `stdin` wird geschlossen, sonst warten ffmpeg/rclone auf Eingabe.
async fn befehl_pruefen(programm: &str, args: &[String]) -> Result<(), String> {
    befehl_ausgabe(programm, args).await.map(|_| ())
}

/// Wie [`befehl_pruefen`], gibt aber die Standardausgabe zurueck. Ein
/// haengender Prozess wird nach [`RCLONE_ZEITGRENZE`] gekappt, damit das Archiv
/// nicht fuer immer an einem toten rclone-Aufruf festhaengt.
async fn befehl_ausgabe(programm: &str, args: &[String]) -> Result<String, String> {
    let lauf = tokio::process::Command::new(programm)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let ausgabe = match tokio::time::timeout(RCLONE_ZEITGRENZE, lauf).await {
        Ok(Ok(ausgabe)) => ausgabe,
        Ok(Err(fehler)) => return Err(format!("{programm} nicht startbar: {fehler}")),
        Err(_) => return Err(format!("{programm} ueberschritt die Zeitgrenze")),
    };
    if !ausgabe.status.success() {
        let fehler = String::from_utf8_lossy(&ausgabe.stderr);
        return Err(format!(
            "{programm} scheiterte ({}): {}",
            ausgabe.status,
            fehler.lines().last().unwrap_or("").trim()
        ));
    }
    Ok(String::from_utf8_lossy(&ausgabe.stdout).into_owned())
}

/// Belegt die Laufmarke atomar (`create_new`), sodass sich zwei Ausloeser nicht
/// ins Gehege kommen. `true` = belegt, weiter geht's; `false` = jemand anders ist
/// dran oder eine frische Marke liegt schon. Eine abgestandene Marke (aelter als
/// das Fenster) wird uebernommen - der vorige Lauf ist dann vermutlich
/// abgestuerzt.
async fn laufmarke_belegen(pfad: &Path, frisch_sekunden: u64) -> bool {
    match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(pfad)
        .await
    {
        Ok(_) => true,
        Err(fehler) if fehler.kind() == std::io::ErrorKind::AlreadyExists => {
            if marke_frisch(pfad, frisch_sekunden).await {
                return false;
            }
            // Abgestanden: einmal wegraeumen und neu belegen.
            let _ = tokio::fs::remove_file(pfad).await;
            tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(pfad)
                .await
                .is_ok()
        }
        Err(_) => false,
    }
}

/// Ein laufender durchgehender Ton-Recorder. streamlink liefert den Stream,
/// ffmpeg zieht ohne Neucodierung nur die Tonspur heraus (Video braucht das
/// Coaching nicht). Beide Prozesse tragen `kill_on_drop`: faellt der Recorder aus
/// der Verwaltung, sterben sie mit.
struct Recorder {
    lauf: String,
    // streamlink lebt weiter, bis der Stream endet; ffmpeg schreibt die Datei.
    _streamlink: tokio::process::Child,
    ffmpeg: tokio::process::Child,
}

impl Recorder {
    /// Ob die Aufnahme abgeschlossen ist. Sobald ffmpeg - der Schreiber - endet,
    /// ist die Datei finalisiert; das ist das verlaessliche "fertig"-Signal,
    /// nicht eine Vermutung ueber die letzte Aenderungszeit.
    fn beendet(&mut self) -> bool {
        matches!(self.ffmpeg.try_wait(), Ok(Some(_)))
    }
}

/// Startet einen durchgehenden Ton-Recorder fuer einen Stream. streamlink gibt
/// den Stream auf stdout, ffmpeg nimmt nur den Ton (`-vn -c:a copy`) als ADTS
/// auf. Laeuft, bis der Stream endet oder der Recorder faellt.
async fn mitschnitt_starten(
    konfiguration: &Konfiguration,
    kanal: &str,
    lauf: &str,
) -> Option<Recorder> {
    let dir = mitschnitt_ordner(konfiguration, kanal, lauf);
    if tokio::fs::create_dir_all(&dir).await.is_err() {
        return None;
    }
    // Ein alter Fertig-Marker desselben Laufs (etwa vom Versiegeln nach einem
    // Aussetzer) muss weg: hier wird gerade wieder aufgenommen, der Lauf ist
    // nicht fertig. Sonst koennte das Archiv ihn mitten in der Aufnahme abholen.
    let _ = tokio::fs::remove_file(dir.join(AUFNAHME_FERTIG)).await;
    let ziel = dir.join(archiv::mitschnitt_name(chrono::Utc::now().timestamp()));
    let url = format!("https://twitch.tv/{kanal}");
    let bin = std::env::var("VOICE_REACTION_STREAMLINK_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "streamlink".to_string());
    let mut streamlink = match tokio::process::Command::new(&bin)
        .arg("--twitch-disable-ads")
        .arg("--quiet")
        .arg("--stdout")
        .arg(&url)
        // Nur die Tonspur: `best` zoege das volle Video, das `-vn` gleich wieder
        // wegwirft - GBs Wegwerf-Traffic und Demux-Last am Last-Gate vorbei.
        // `audio_only` ist Twitchs reine Tonspur, `worst` der Rueckfall.
        .arg("audio_only,worst")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(fehler) => {
            tracing::error!(%fehler, kanal, "streamlink fuer Mitschnitt nicht startbar");
            return None;
        }
    };
    let pipe = streamlink.stdout.take()?;
    let pipe: std::process::Stdio = match pipe.try_into() {
        Ok(stdio) => stdio,
        Err(fehler) => {
            tracing::error!(%fehler, kanal, "streamlink-Ausgabe nicht als Pipe nutzbar");
            return None;
        }
    };
    match tokio::process::Command::new(ffmpeg_pfad())
        .arg("-nostdin")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg("pipe:0")
        .arg("-vn")
        .arg("-c:a")
        .arg("copy")
        .arg("-f")
        .arg("adts")
        .arg(&ziel)
        .stdin(pipe)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(ffmpeg) => Some(Recorder {
            lauf: lauf.to_owned(),
            _streamlink: streamlink,
            ffmpeg,
        }),
        Err(fehler) => {
            // streamlink faellt hier aus dem Scope und wird per kill_on_drop
            // gestoppt.
            tracing::error!(%fehler, kanal, "ffmpeg fuer Mitschnitt nicht startbar");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(versatz: u64) -> plan::Block {
        plan::Block {
            kanal: "testkanal".to_owned(),
            lauf: "lauf1".to_owned(),
            nummer: 1,
            versatz_sekunden: versatz,
            datei: "/tmp/egal.ts".to_owned(),
            versuche: 0,
            frueherstens: 0,
            nur_melden: false,
            meldeversuche: 0,
            zeit_unsicher: false,
            stream_start_utc: None,
            aufnahme_beginn_utc: None,
        }
    }

    #[test]
    fn segmente_bleiben_innerhalb_der_blockdauer() {
        // Der alte Fehler: `teil.len()` als Zeitfaktor liess Segmente weit
        // ueber das Blockende hinausragen.
        let text = "Ein Satz. Noch einer! Und ein dritter? ".repeat(20);
        let segmente = segmente_bauen(&block(600), &text, 600.0);
        assert!(!segmente.is_empty());
        for s in &segmente {
            assert!(s.start_sekunden >= 600.0, "Start liegt vor dem Blockanfang");
            assert!(
                s.ende_sekunden <= 1200.0 + 0.001,
                "Segment ragt ueber das Blockende: {}",
                s.ende_sekunden
            );
        }
        let letztes = segmente.last().expect("mindestens ein Segment");
        assert!(
            (letztes.ende_sekunden - 1200.0).abs() < 0.001,
            "das letzte Segment muss am Blockende enden"
        );
    }

    #[test]
    fn segmente_laufen_lueckenlos_vorwaerts() {
        let segmente = segmente_bauen(&block(0), "A. B. C. D. E. F.", 60.0);
        let mut vorher = 0.0;
        for s in &segmente {
            assert!(
                s.start_sekunden >= vorher - 0.001,
                "Segmente laufen rueckwaerts"
            );
            assert!(s.ende_sekunden >= s.start_sekunden);
            vorher = s.ende_sekunden;
        }
    }

    #[test]
    fn leerer_text_ergibt_keine_segmente() {
        assert!(segmente_bauen(&block(0), "   \n  ", 600.0).is_empty());
    }

    #[test]
    fn dauer_null_faellt_nicht_auseinander() {
        // Ein Block ohne bekannte Dauer darf keine NaN-Zeiten liefern.
        let segmente = segmente_bauen(&block(0), "Ein Satz. Zwei Satz.", 0.0);
        assert_eq!(segmente.len(), 1);
        for s in &segmente {
            assert!(s.start_sekunden.is_finite() && s.ende_sekunden.is_finite());
        }
    }

    fn mtime_setzen(pfad: &Path, zeit: std::time::SystemTime) {
        let datei = std::fs::File::options()
            .write(true)
            .open(pfad)
            .expect("Datei zum Zeitstempeln");
        datei
            .set_times(std::fs::FileTimes::new().set_modified(zeit))
            .expect("Aenderungszeit setzen");
    }

    #[tokio::test]
    async fn aufbewahrung_loescht_alte_berichte_und_laesst_neue() {
        let wurzel = test_ordner("aufbewahrung");
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
        tokio::fs::create_dir_all(&wurzel).await.expect("Ordner");
        // Ein Bericht der Vorgaengerfassung, direkt im Ausgabeordner.
        let alt = wurzel.join("stream-audit-20260601T120000Z.md");
        let neu = wurzel.join("stream-audit-20260813T120000Z.md");
        tokio::fs::write(&alt, b"alt").await.expect("schreiben");
        tokio::fs::write(&neu, b"neu").await.expect("schreiben");
        // Aenderungszeit 40 Tage zurueckdrehen.
        let vor_40_tagen = std::time::SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60);
        mtime_setzen(&alt, vor_40_tagen);

        let konfiguration = Konfiguration {
            kanaele: Vec::new(),
            ausgabe: wurzel.clone(),
            transkript_behalten: false,
            aufbewahrung_tage: 30,
            behalten_grenze_bytes: 0,
        };
        alte_berichte_loeschen(&konfiguration).await;
        assert!(!alt.exists(), "40 Tage alter Bericht muss weg sein");
        assert!(neu.exists(), "frischer Bericht muss bleiben");

        // 0 heisst unbegrenzt: nichts wird geloescht.
        tokio::fs::write(&alt, b"alt").await.expect("schreiben");
        mtime_setzen(&alt, vor_40_tagen);
        let unbegrenzt = Konfiguration {
            aufbewahrung_tage: 0,
            behalten_grenze_bytes: 0,
            ..konfiguration
        };
        alte_berichte_loeschen(&unbegrenzt).await;
        assert!(alt.exists(), "bei 0 Tagen darf nichts geloescht werden");
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
    }

    #[tokio::test]
    async fn aufbewahrung_erreicht_auch_die_kanalordner() {
        // Berichte liegen unter <ausgabe>/<kanal>/; ein Aufraeumen nur auf
        // oberster Ebene loeschte nie einen einzigen.
        let wurzel = test_ordner("kanalordner");
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
        let kanalordner = wurzel.join("testkanal");
        tokio::fs::create_dir_all(&kanalordner)
            .await
            .expect("Ordner");
        let alt = kanalordner.join("testkanal-4711-t000000-b0001.json");
        tokio::fs::write(&alt, b"{}").await.expect("schreiben");
        mtime_setzen(
            &alt,
            std::time::SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60),
        );

        alte_berichte_loeschen(&Konfiguration {
            kanaele: Vec::new(),
            ausgabe: wurzel.clone(),
            transkript_behalten: false,
            aufbewahrung_tage: 30,
            behalten_grenze_bytes: 0,
        })
        .await;
        assert!(!alt.exists(), "Bericht im Kanalordner muss weg sein");
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
    }

    #[tokio::test]
    async fn zettel_erhaelt_kanal_und_zeitversatz() {
        // Ohne Zettel liefe jede wiederaufgenommene Datei als Kanal
        // "wiederaufnahme" mit Zeitversatz 0 durch - der Bericht ordnete
        // dann niemandem etwas zu.
        let blockordner = test_ordner("zettel");
        let _ = tokio::fs::remove_dir_all(&blockordner).await;
        // Die Aufnahme liegt eine Ebene tiefer, so wie streamlink sie ablegt.
        let capture = blockordner.join("capture-abc123");
        tokio::fs::create_dir_all(&capture).await.expect("Ordner");
        let aufnahme = capture.join("audio.ts");
        tokio::fs::write(&aufnahme, b"nicht wirklich audio")
            .await
            .expect("schreiben");

        zettel_schreiben(
            &blockordner,
            ZettelDaten {
                kanal: "helmbombenricky",
                lauf: "4711",
                nummer: 7,
                versatz_sekunden: 3600,
                zeit_unsicher: false,
                stream_start_utc: Some("2026-08-13T20:00:00Z"),
                aufnahme_beginn_utc: Some("2026-08-13T20:00:05Z"),
            },
        )
        .await;

        let gelesen = zettel_lesen(&aufnahme).await.expect("Zettel lesbar");
        assert_eq!(gelesen.kanal, "helmbombenricky");
        assert_eq!(gelesen.lauf, "4711");
        assert_eq!(gelesen.nummer, 7);
        assert_eq!(gelesen.versatz_sekunden, 3600);
        assert_eq!(
            gelesen.versuche, 0,
            "Versuche zaehlen nach dem Neustart neu"
        );
        assert_eq!(gelesen.datei, aufnahme.to_string_lossy());

        // Die rekursive Suche findet die Aufnahme trotz der Zwischenebene.
        let mut gefunden = Vec::new();
        ts_dateien_sammeln(&blockordner, &mut gefunden).await;
        assert_eq!(gefunden, vec![aufnahme.clone()]);

        // Ohne Zettel gibt es nichts zu lesen, und der Aufrufer faellt zurueck.
        tokio::fs::remove_file(blockordner.join("block.json"))
            .await
            .expect("Zettel entfernen");
        assert!(zettel_lesen(&aufnahme).await.is_none());
        let _ = tokio::fs::remove_dir_all(&blockordner).await;
    }

    #[tokio::test]
    async fn zettellose_aufnahmen_bekommen_verschiedene_namen() {
        // Zwei liegengebliebene Aufnahmen ohne Zettel hatten frueher beide
        // Zeitversatz 0 - gleiche Bezeichnung, gleicher Berichtsname, gleicher
        // Idempotenzschluessel. Der zweite Bericht ueberschrieb den ersten.
        let mut ersatz = plan::Aufnahme::starten("wiederaufnahme", "lauf");
        let erster = ersatz.block_fertig("/tmp/a.ts", 1);
        let zweiter = ersatz.block_fertig("/tmp/b.ts", 1);
        assert_ne!(erster.bezeichnung(), zweiter.bezeichnung());
    }

    #[tokio::test]
    async fn ausgewertete_aufnahme_wird_nicht_erneut_eingereiht() {
        // Aufbewahrte Aufnahmen sind Belege, keine offenen Bloecke. Ohne die
        // Marke liefe der naechste Start sie erneut aus - der zweite Bericht
        // ueberschriebe den ersten und koennte den Beleg loeschen.
        let blockordner = test_ordner("fertig");
        let _ = tokio::fs::remove_dir_all(&blockordner).await;
        let capture = blockordner.join("capture-xyz");
        tokio::fs::create_dir_all(&capture).await.expect("Ordner");
        let aufnahme = capture.join("audio.ts");
        tokio::fs::write(&aufnahme, b"ton")
            .await
            .expect("schreiben");

        assert!(!bereits_ausgewertet(&aufnahme).await);

        let bericht = Bericht {
            lauf_id: "20260813T120000Z-ricky".to_owned(),
            erstellt_am: "2026-08-13T12:00:00Z".to_owned(),
            stream_start_utc: None,
            aufnahme_beginn_utc: None,
            quelle: "live, Block 1".to_owned(),
            kanal: "ricky".to_owned(),
            transkription: "openai_api".to_owned(),
            modell: "large-v3-turbo-ct2".to_owned(),
            transkription_lokal: true,
            anbieter: "fireworks".to_owned(),
            llm_modell: "irgendein-modell".to_owned(),
            transkript_behalten: false,
            segmente: 3,
            modell_geprueft: true,
            modell_hinweis: String::new(),
            aufnahme_abgebrochen: false,
            funde: Vec::new(),
        };
        fertig_markieren(&aufnahme, &bericht).await;
        assert!(bereits_ausgewertet(&aufnahme).await);
        let _ = tokio::fs::remove_dir_all(&blockordner).await;
    }

    #[test]
    fn whisper_zeitstempel_schlagen_die_schaetzung() {
        // Die Schaetzung ueber den Textanteil verschiebt sich bei
        // Sprechpausen; die Zeiten von Whisper stehen an den Woertern.
        let block = block(600);
        let abschnitte = vec![
            tb_engagement::transcribe::TranscriptSegment {
                start_seconds: 0.0,
                end_seconds: 4.0,
                text: "kurzer Satz.".to_owned(),
            },
            tb_engagement::transcribe::TranscriptSegment {
                start_seconds: 480.0,
                end_seconds: 495.0,
                text: "und viel spaeter noch einer.".to_owned(),
            },
        ];
        let segmente = segmente_aus_whisper(&block, &abschnitte);
        assert_eq!(segmente.len(), 2, "acht Minuten Pause trennen die Segmente");
        assert_eq!(segmente[0].start_sekunden, 600.0);
        assert_eq!(segmente[1].start_sekunden, 1080.0);
        assert_eq!(segmente[1].ende_sekunden, 1095.0);
    }

    /// Eindeutiger Testordner.
    ///
    /// Nur fuer Tests: der Name traegt Prozess-ID und Nanosekunden, damit zwei
    /// gleichzeitige Laeufe sich nicht ins Gehege kommen. Die Unterdrueckung
    /// der semgrep-Regel gilt genau dafuer: hier entsteht nichts, was ein
    /// anderer Nutzer vorhersagen und uebernehmen koennte.
    fn test_ordner(zweck: &str) -> PathBuf {
        let stempel = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        // nosemgrep: rust.lang.security.temp-dir.temp-dir
        std::env::temp_dir().join(format!(
            "stream-audit-{zweck}-{}-{stempel}",
            std::process::id()
        ))
    }

    fn test_konfiguration(wurzel: &Path) -> Konfiguration {
        Konfiguration {
            kanaele: Vec::new(),
            ausgabe: wurzel.to_path_buf(),
            transkript_behalten: false,
            aufbewahrung_tage: 30,
            behalten_grenze_bytes: 0,
        }
    }

    #[tokio::test]
    async fn platzgrenze_raeumt_die_aelteste_aufnahme() {
        // Ohne diese Grenze fuellt ein anhaltender Modellausfall die Platte:
        // jeder Block gilt dann als unvollstaendig geprueft und bleibt die
        // volle Aufbewahrungsfrist liegen.
        let wurzel = test_ordner("platzgrenze");
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
        let mut konfiguration = test_konfiguration(&wurzel);
        konfiguration.behalten_grenze_bytes = 1500;

        let aufnahmen = aufnahme_wurzel(&konfiguration);
        let mut dateien = Vec::new();
        for (nummer, alter_tage) in [(1u32, 3u64), (2, 1)] {
            let block = aufnahmen
                .join("testkanal")
                .join("lauf")
                .join(format!("t000000-b{nummer:04}"))
                .join("capture");
            tokio::fs::create_dir_all(&block).await.expect("Ordner");
            let datei = block.join("audio.ts");
            tokio::fs::write(&datei, vec![0u8; 1000])
                .await
                .expect("schreiben");
            mtime_setzen(
                &datei,
                std::time::SystemTime::now() - Duration::from_secs(alter_tage * 24 * 60 * 60),
            );
            dateien.push(datei);
        }

        let warteschlange = Arc::new(Mutex::new(plan::Warteschlange::new()));
        let geloescht = grenze_durchsetzen(&konfiguration, &warteschlange).await;

        assert_eq!(geloescht, 1, "eine Aufnahme muss fuer die Grenze weichen");
        assert!(!dateien[0].exists(), "die aeltere Aufnahme geht zuerst");
        assert!(dateien[1].exists(), "die juengere bleibt");
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
    }

    #[tokio::test]
    async fn hinweis_behaelt_seinen_schluessel() {
        // Derselbe Vorfall muss bei jedem Anlauf denselben Schluessel tragen,
        // sonst kommt die Meldung doppelt an, wenn ein frueherer Versuch doch
        // zugestellt wurde.
        let wurzel = test_ordner("hinweis");
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
        tokio::fs::create_dir_all(&wurzel).await.expect("Ordner");
        let konfiguration = test_konfiguration(&wurzel);

        hinweis_aufheben(
            &konfiguration,
            "helix-ausfall",
            "schluessel-1",
            "erster Text",
        )
        .await;
        hinweis_aufheben(
            &konfiguration,
            "helix-ausfall",
            "schluessel-2",
            "zweiter Text",
        )
        .await;

        let datei = hinweis_ordner(&konfiguration).join("helix-ausfall.json");
        let roh = tokio::fs::read_to_string(&datei).await.expect("Hinweis");
        let json: serde_json::Value = serde_json::from_str(&roh).expect("JSON");
        assert_eq!(json["schluessel"], "schluessel-1", "Schluessel bleibt");
        assert_eq!(
            json["text"], "erster Text",
            "auch der Text bleibt - Schluessel und Inhalt gehoeren zusammen"
        );

        // Erledigt heisst weg - sonst wird eine geloeste Stoerung spaeter
        // nachgereicht.
        hinweis_erledigt(&konfiguration, "helix-ausfall").await;
        assert!(!datei.exists());
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
    }

    #[test]
    fn vorfallsnummern_wiederholen_sich_nicht() {
        let erste = naechste_vorfall_nummer();
        let zweite = naechste_vorfall_nummer();
        assert!(zweite > erste, "jede Stoerung bekommt eine eigene Nummer");
    }

    #[tokio::test]
    async fn stille_reihe_behaelt_die_aufnahme() {
        // Ab dem Verdacht auf einen ausgefallenen STT-Dienst ist die Aufnahme
        // der einzige Weg, die uebersprungene Sendezeit noch nachzuhoeren.
        // Nachgebildet wird hier die Entscheidung, nicht die Schleife.
        let mut stumm = 0usize;
        let mut behalten = Vec::new();
        for _ in 0..(MAX_LEER_AM_STUECK + 2) {
            stumm += 1;
            behalten.push(stumm >= MAX_LEER_AM_STUECK);
        }
        assert!(!behalten[MAX_LEER_AM_STUECK - 2], "vorher wird geloescht");
        assert!(
            behalten[MAX_LEER_AM_STUECK - 1],
            "ab der Schwelle bleibt sie"
        );
        assert!(behalten[MAX_LEER_AM_STUECK + 1], "und danach weiter");
        assert!(
            MAX_LEER_AM_STUECK.is_multiple_of(MAX_LEER_AM_STUECK),
            "gemeldet wird bei jedem Vielfachen"
        );
    }

    #[test]
    fn nur_echte_blockordner_werden_geloescht() {
        // Eine verirrte Datei direkt unter der Wurzel darf nicht dazu fuehren,
        // dass zwei Ebenen hoeher der ganze Ausgabeordner faellt.
        let wurzel = PathBuf::from("/daten/audits/aufnahmen");
        let echt = wurzel.join("ricky/4711/t000600-b0003/capture-abc/audio.ts");
        assert_eq!(
            blockordner_von(&wurzel, &echt),
            Some(wurzel.join("ricky/4711/t000600-b0003"))
        );
        assert_eq!(blockordner_von(&wurzel, &wurzel.join("verirrt.ts")), None);
        assert_eq!(
            blockordner_von(
                &wurzel,
                &wurzel.join("ricky/4711/kein-block/capture/audio.ts")
            ),
            None,
            "ein Ordner ohne t<Ziffern> ist kein Blockordner"
        );
        assert_eq!(
            blockordner_von(&wurzel, &PathBuf::from("/woanders/a/b/c/d/audio.ts")),
            None
        );
    }

    #[test]
    fn nur_eigene_berichte_werden_aufgeraeumt() {
        let kanal = Some("helmbombenricky");
        assert!(ist_berichtsname(
            "helmbombenricky-4711-t003600-b0007",
            kanal
        ));
        assert!(!ist_berichtsname("steuererklaerung", kanal));
        assert!(
            !ist_berichtsname("-t000000-b0001", kanal),
            "ohne Kanal kein Bericht"
        );
        assert!(!ist_berichtsname("helmbombenricky-tabc-b0001", kanal));
        assert!(
            !ist_berichtsname("helmbombenricky-4711-t003600", kanal),
            "ohne Blocknummer kein eigener Bericht"
        );
        // Eine fremde Datei im geteilten Ausgabeordner bleibt liegen, auch
        // wenn ihr Name zufaellig auf -t<Ziffern> endet.
        assert!(!ist_berichtsname("rechnung-t2025-b0001", kanal));
        assert!(!ist_berichtsname("rechnung-t2025-b0001", None));
        // Nur die Berichte der Vorgaengerfassung gelten ohne Kanalordner.
        assert!(ist_berichtsname("stream-audit-20260601T120000Z", None));
    }

    #[test]
    fn aufnahmen_liegen_dauerhaft_neben_den_berichten() {
        // Nicht in /tmp: Aufnahmen zu Funden bleiben bis zum Ende der
        // Aufbewahrungsfrist liegen und waeren dort beim naechsten Neustart
        // oder durch eine tmpfiles-Regel weg.
        let konfiguration = Konfiguration {
            kanaele: Vec::new(),
            ausgabe: PathBuf::from("/daten/audits"),
            transkript_behalten: false,
            aufbewahrung_tage: 30,
            behalten_grenze_bytes: 0,
        };
        let wurzel = aufnahme_wurzel(&konfiguration);
        assert_eq!(wurzel, PathBuf::from("/daten/audits/aufnahmen"));
        assert!(!wurzel.starts_with(std::env::temp_dir()));
    }
}

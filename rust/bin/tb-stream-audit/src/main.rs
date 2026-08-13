//! Privates Coaching-Audit autorisierter Twitch-Kanaele.
//!
//! Ersetzt `deadlock-twitch-stream-coaching-watch.service`, das seit dem Abriss
//! der Python-Laufzeit am 21.07.2026 auf ein geloeschtes Startskript zeigte.
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
    config::Konfiguration,
    llm, melden, plan,
    report::{self, Bericht},
    Segment,
};
use tb_transport_twitch::client::{HelixClient, HelixConfig};
use tokio::sync::Mutex;

/// Ein Segment je so vielen Sekunden Transkript. Whisper liefert einen Block am
/// Stueck; fuer Zeitbezug im Bericht wird gleichmaessig aufgeteilt.
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

    let warteschlange = Arc::new(Mutex::new(plan::Warteschlange::new()));
    liegengebliebenes_einreihen(&warteschlange).await;
    let aufnahme = tokio::spawn(aufnahme_schleife(
        helix,
        konfiguration.clone(),
        Arc::clone(&warteschlange),
    ));
    let auswertung = tokio::spawn(auswertungs_schleife(
        transkribierer,
        konfiguration,
        Arc::clone(&warteschlange),
    ));

    // Beide Schleifen laufen endlos. Endet eine, ist der Dienst kaputt - er
    // wuerde sonst als leere Huelle weiterlaufen und nie wieder etwas
    // auswerten. Der Ausstiegscode muss ungleich 0 sein, sonst greift
    // Restart=on-failure in der Unit nicht.
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Abbruch angefordert");
        }
        _ = abschaltsignal() => {
            tracing::info!("SIGTERM erhalten, beende");
        }
        ergebnis = aufnahme => {
            arbeiter_ende_melden("Aufnahmeschleife", ergebnis);
            std::process::exit(1);
        }
        ergebnis = auswertung => {
            arbeiter_ende_melden("Auswertungsschleife", ergebnis);
            std::process::exit(1);
        }
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
/// Erkannt werden die Capture-Verzeichnisse an ihrem Praefix; Kanal und
/// Blocknummer sind aus dem Dateinamen nicht mehr rekonstruierbar, deshalb
/// laufen sie als eigener Lauf "wiederaufnahme" mit fortlaufender Nummer.
async fn liegengebliebenes_einreihen(warteschlange: &Arc<Mutex<plan::Warteschlange>>) {
    let mut aufnahmen = Vec::new();
    ts_dateien_sammeln(&aufnahme_wurzel(), &mut aufnahmen).await;

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

    for pfad in aufnahmen {
        let block = match zettel_lesen(&pfad).await {
            Some(block) => block,
            None => {
                ohne_zettel += 1;
                ersatz.block_fertig(pfad.to_string_lossy().to_string(), 0)
            }
        };
        warteschlange.lock().await.einreihen(block);
        gefunden += 1;
    }

    if ohne_zettel > 0 {
        tracing::warn!(
            ohne_zettel,
            "Aufnahmen ohne Zettel gefunden - Kanal und Zeitversatz sind fuer sie verloren"
        );
    }
    if gefunden > 0 {
        tracing::info!(gefunden, "liegengebliebene Aufnahmen wieder eingereiht");
    }
}

/// Sammelt `.ts`-Dateien unterhalb eines Ordners.
///
/// Die Aufnahmen liegen inzwischen mehrere Ebenen tief
/// (`<kanal>/<lauf>/t<versatz>/<capture>/audio.ts`). Ein unlesbarer Ordner
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
        while let Ok(Some(eintrag)) = eintraege.next_entry().await {
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

/// Legt Kanal, Lauf, Blocknummer und Zeitversatz in den Blockordner, **bevor**
/// die Aufnahme laeuft.
///
/// Das Capture-Verzeichnis darunter heisst nach einem Zufallswert; aus dem
/// Pfad allein ist nach einem Neustart nicht zu erkennen, von wem die Aufnahme
/// stammt und an welcher Stelle der Sendung sie sass. Ein Stopp durch systemd
/// trifft mitten in die zehn Minuten - wer den Zettel erst danach schreibt,
/// laesst genau die angebrochene Aufnahme ohne Zuordnung zurueck.
async fn zettel_schreiben(
    blockordner: &Path,
    kanal: &str,
    lauf: &str,
    nummer: u32,
    versatz_sekunden: u64,
) {
    let inhalt = serde_json::json!({
        "kanal": kanal,
        "lauf": lauf,
        "nummer": nummer,
        "versatz_sekunden": versatz_sekunden,
    });
    if let Err(fehler) =
        nur_fuer_mich(&blockordner.join(ZETTEL), inhalt.to_string().as_bytes()).await
    {
        tracing::warn!(fehler, "Zettel nicht geschrieben");
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
        datei: aufnahme.to_string_lossy().to_string(),
        versuche: 0,
        frueherstens: 0,
    })
}

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
/// Ein Block ist zehn Minuten lang, und das lokale Whisper laeuft auf der CPU
/// langsamer als Echtzeit. Die Vorgabe der Kiste sind 60 Sekunden - damit
/// liefe jeder volle Block in die Zeitueberschreitung, dreimal, und waere
/// danach verloren.
const STT_ZEITGRENZE: Duration = Duration::from_secs(30 * 60);

/// Umgebungsschalter fuer einen STT-Endpunkt ausserhalb dieses Rechners.
const REMOTE_STT_ERLAUBT_ENV: &str = "STREAM_AUDIT_ALLOW_REMOTE_STT";

fn stt_basis_url() -> String {
    std::env::var("ENGAGEMENT_STT_BASE_URL").unwrap_or_default()
}

fn remote_stt_erlaubt() -> bool {
    matches!(
        std::env::var(REMOTE_STT_ERLAUBT_ENV)
            .unwrap_or_default()
            .trim(),
        "1" | "true" | "ja" | "yes"
    )
}

/// Eigener Aufnahmeordner.
///
/// Der `AudioCapturer` legt ohne Zielangabe unter `/tmp/voice-reaction-*` ab,
/// und dasselbe Praefix nutzen Reaction-Learning und Smalltalk. Wer dort
/// aufraeumt, loescht fremde, womoeglich noch laufende Aufnahmen. Dieser
/// Dienst bekommt deshalb sein eigenes Verzeichnis.
fn aufnahme_wurzel() -> PathBuf {
    std::env::temp_dir().join("stream-audit-captures")
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
) {
    let mut laufend: std::collections::HashMap<String, tokio::task::JoinHandle<plan::Aufnahme>> =
        Default::default();
    // Aufnahmestand je Kanal. Er lebt hier und nicht im Task, weil ein Task
    // auch mitten in der Sendung enden kann - etwa wenn streamlink kurz
    // abbricht. Legte der Neustart den Stand neu an, finge der
    // Sechs-Stunden-Deckel jedes Mal von vorn an.
    let mut staende: std::collections::HashMap<String, plan::Aufnahme> = Default::default();

    loop {
        let sendungen = match helix
            .get_streams_by_logins(&konfiguration.kanaele, None)
            .await
        {
            Ok(streams) => streams,
            Err(fehler) => {
                tracing::warn!(?fehler, "Live-Abfrage fehlgeschlagen");
                tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
                continue;
            }
        };
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
                Err(fehler) => tracing::error!(kanal, ?fehler, "Aufnahme-Task abgestuerzt"),
            }
        }

        // Der Stand faellt erst, wenn der Kanal offline war. Wuerde er bei
        // jedem Takt zuruecksetzen, waere die Sechs-Stunden-Grenze wirkungslos:
        // ein Dauerstream startete alle 60 Sekunden eine neue Aufnahme.
        staende.retain(|kanal, _| live.contains(kanal));

        for sendung in &sendungen {
            let kanal = sendung.user_login.to_lowercase();
            let kanal = &kanal;
            if laufend.contains_key(kanal) {
                continue;
            }
            // Ein Zustand aus einer anderen Sendung darf nicht weiterlaufen:
            // Endet ein Stream und startet zwischen zwei Takten neu, erbte die
            // neue Sendung sonst Lauf-Kennung, Zeitversatz und Deckel der
            // alten.
            let passend = staende
                .get(kanal)
                .map(|z| z.lauf == sendung.id.trim())
                .unwrap_or(false);
            if !passend {
                staende.remove(kanal);
            }
            let zustand = staende.remove(kanal).unwrap_or_else(|| {
                // Kennung und Startzeit kommen von Twitch. Die Stream-ID ist
                // ueber einen Neustart des Dienstes hinweg dieselbe, und der
                // Zeitversatz macht die Zeiten im Bericht zu Sendungszeiten -
                // "hoer dir Minute 12 an" trifft dann im VOD auch Minute 12.
                let lauf = if sendung.id.trim().is_empty() {
                    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
                } else {
                    sendung.id.trim().to_owned()
                };
                plan::Aufnahme::starten_bei(kanal.clone(), lauf, sendungssekunden(sendung))
            });
            if zustand.naechste_blocklaenge().is_none() {
                // Deckel erreicht: Stand behalten, damit er beim naechsten Takt
                // nicht als "neue Sendung" durchgeht.
                staende.insert(kanal.clone(), zustand);
                continue;
            }
            let handle = tokio::spawn(kanal_aufnehmen(zustand, Arc::clone(&warteschlange)));
            laufend.insert(kanal.clone(), handle);
            tracing::info!(kanal, "Aufnahme gestartet");
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
    warteschlange: Arc<Mutex<plan::Warteschlange>>,
) -> plan::Aufnahme {
    let capturer = AudioCapturer::from_env();
    let kanal = zustand.kanal.clone();

    loop {
        let Some(laenge) = zustand.naechste_blocklaenge() else {
            return zustand;
        };

        // Eigener Ordner je Block, mit dem Zettel darin, bevor aufgenommen wird.
        let blockordner = aufnahme_wurzel()
            .join(&kanal)
            .join(&zustand.lauf)
            .join(format!("t{:06}", zustand.sendungssekunden()));
        if let Err(fehler) = tokio::fs::create_dir_all(&blockordner).await {
            tracing::error!(?fehler, "Aufnahmeordner nicht anlegbar");
            return zustand;
        }
        zettel_schreiben(
            &blockordner,
            &kanal,
            &zustand.lauf,
            zustand.bloecke + 1,
            zustand.sendungssekunden(),
        )
        .await;

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
                let block = zustand.block_fertig(
                    aufgenommen.media_path.to_string_lossy().to_string(),
                    aufgenommen.actual_duration_seconds.round().max(0.0) as u64,
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
    // Berichte liegen unter <ausgabe>/<kanal>/. Ein Aufraeumen, das nur die
    // oberste Ebene liest, findet keinen einzigen davon.
    let mut ordner = vec![konfiguration.ausgabe.clone()];
    let Ok(mut oben) = tokio::fs::read_dir(&konfiguration.ausgabe).await else {
        return;
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
        let Ok(mut eintraege) = tokio::fs::read_dir(&ordner).await else {
            continue;
        };
        while let Ok(Some(eintrag)) = eintraege.next_entry().await {
            let pfad = eintrag.path();
            let endung = pfad
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if !matches!(endung, "md" | "json" | "txt") {
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

/// Arbeitet die Warteschlange ab, immer nur einen Block gleichzeitig.
async fn auswertungs_schleife(
    transkribierer: OpenAiTranscriber,
    konfiguration: Konfiguration,
    warteschlange: Arc<Mutex<plan::Warteschlange>>,
) {
    let mut naechstes_aufraeumen = tokio::time::Instant::now();
    loop {
        // Aufbewahrung war bisher nur eine Zahl in der Konfiguration. Eine
        // Grenze, die nichts loescht, ist keine - Berichte mit moeglichen
        // Vorfaellen lagen unbegrenzt.
        if tokio::time::Instant::now() >= naechstes_aufraeumen {
            alte_berichte_loeschen(&konfiguration).await;
            naechstes_aufraeumen =
                tokio::time::Instant::now() + Duration::from_secs(AUFRAEUM_TAKT_SEKUNDEN);
        }

        let naechster = warteschlange.lock().await.naechster();
        let Some(block) = naechster else {
            tokio::time::sleep(Duration::from_secs(15)).await;
            continue;
        };

        match block_auswerten(&transkribierer, &konfiguration, &block).await {
            Ok(Aufnahmeschicksal::Behalten) => {
                tracing::warn!(
                    block = %block.bezeichnung(),
                    "Aufnahme bleibt liegen - der Bericht taugt nicht als Nachweis"
                );
            }
            Ok(Aufnahmeschicksal::Loeschen) => {
                // Erst nach erfolgreicher Auswertung wegraeumen. Wer bei einem
                // Fehler loescht, vernichtet bei einem Aussetzer der
                // Transkription den einzigen Beleg, den es je gab.
                let pfad = PathBuf::from(&block.datei);
                if let Some(verzeichnis) = pfad.parent() {
                    // Bleibt die Aufnahme liegen, wird sie nach dem naechsten
                    // Neustart erneut ausgewertet und gemeldet. Das gehoert
                    // ins Journal, nicht ins Nichts.
                    if let Err(fehler) = tokio::fs::remove_dir_all(verzeichnis).await {
                        tracing::warn!(
                            ?fehler,
                            ordner = ?verzeichnis,
                            "Aufnahme nicht geloescht - sie laeuft nach einem Neustart erneut"
                        );
                    }
                }
            }
            Err(fehler) => {
                let bezeichnung = block.bezeichnung();
                let versuch = block.versuche + 1;
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
                }
            }
        }
    }
}

/// Was nach der Auswertung mit der Aufnahme geschieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Aufnahmeschicksal {
    /// Alles ausgewertet und gemeldet - die Aufnahme kann weg.
    Loeschen,
    /// Ausgewertet, aber das Ergebnis ist nicht vertrauenswuerdig. Die
    /// Aufnahme bleibt liegen, damit noch jemand hineinhoeren kann.
    Behalten,
}

async fn block_auswerten(
    transkribierer: &OpenAiTranscriber,
    konfiguration: &Konfiguration,
    block: &plan::Block,
) -> Result<Aufnahmeschicksal, String> {
    let transkript = transkribierer
        .transcribe_clip(Path::new(&block.datei))
        .await
        .map_err(|e| format!("Transkription: {e}"))?;

    let segmente = segmente_bauen(block, &transkript.text, transkript.duration_seconds);
    // Ein leerer Block ist keine ruhige Stunde, sondern ein unklarer Befund:
    // stille Passage oder ausgefallene Transkription, von aussen nicht zu
    // unterscheiden. Er wird gemeldet, und die Aufnahme bleibt liegen.
    let leer = segmente.is_empty();
    if leer {
        tracing::warn!(block = %block.bezeichnung(), "kein Text im Block");
    }

    let mut funde = tb_stream_audit::regelfunde(&segmente);
    let (modell_funde, modell_fehler) = modellfunde(&segmente).await;
    funde.extend(modell_funde);
    let modell_hinweis = if leer {
        Some("Transkription lieferte keinen Text; nichts geprueft".to_owned())
    } else {
        modell_fehler
    };
    let funde = report::sortiert(tb_stream_audit::funde_zusammenfassen(funde));

    let endpunkt = tb_llm::selection::endpoint_for(llm::USE_CASE);
    let jetzt = chrono::Utc::now();
    let bericht = Bericht {
        lauf_id: report::lauf_id(jetzt, &block.kanal),
        erstellt_am: jetzt.to_rfc3339(),
        quelle: format!("live, Block {}", block.nummer),
        kanal: block.kanal.clone(),
        transkription: transkript.engine.clone(),
        modell: transkript.model.clone(),
        transkription_lokal: tb_stream_audit::llm::ist_lokal(&stt_basis_url()),
        anbieter: endpunkt.provider.to_owned(),
        transkript_behalten: konfiguration.transkript_behalten,
        segmente: segmente.len(),
        modell_geprueft: modell_hinweis.is_none(),
        modell_hinweis: modell_hinweis.unwrap_or_default(),
        funde,
    };

    schreiben(konfiguration, block, &bericht, &transkript.text).await?;
    // Ein Fund, dessen Meldung nie ankam, ist kein erledigter Block. Schlaegt
    // die DM fehl, geht der Block zurueck in die Warteschlange statt die
    // Aufnahme zu loeschen - sonst verschwindet der einzige Hinweis still.
    // Der Idempotenzschluessel haengt an der Blockbezeichnung, nicht an der
    // Lauf-ID: die entsteht bei jedem Versuch neu, und der Broker koennte die
    // Wiederholung dann nicht als solche erkennen.
    dm_senden(&bericht, &block.bezeichnung()).await?;
    Ok(if leer {
        Aufnahmeschicksal::Behalten
    } else {
        Aufnahmeschicksal::Loeschen
    })
}

/// Teilt den Blocktext in Segmente und rechnet die Zeit ueber den Textanteil.
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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap_or_default();

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
    let basis = verzeichnis.join(block.bezeichnung());

    let json = serde_json::to_string_pretty(bericht).map_err(|e| format!("JSON: {e}"))?;
    nur_fuer_mich(
        &basis.with_extension("md"),
        report::markdown(bericht).as_bytes(),
    )
    .await?;
    nur_fuer_mich(&basis.with_extension("json"), json.as_bytes()).await?;
    if konfiguration.transkript_behalten {
        nur_fuer_mich(&basis.with_extension("txt"), transkript.as_bytes()).await?;
    }
    tracing::info!(
        block = %block.bezeichnung(),
        funde = bericht.funde.len(),
        "Bericht geschrieben"
    );
    Ok(())
}

/// Nur melden, wenn es etwas zu melden gibt. Eine DM je Block ohne Funde waere
/// nach dem ersten Abend Rauschen, das niemand mehr liest.
async fn dm_senden(bericht: &Bericht, schluessel: &str) -> Result<(), String> {
    // Ohne Funde gibt es nichts zu melden - je Block eine DM "alles ruhig"
    // waere bei drei Kanaelen im Zehnminutentakt nur noch Rauschen. Eine
    // Ausnahme: lief der Modellschritt nicht, ist Stille irrefuehrend, denn
    // dann hat nur die Regelpruefung hingesehen.
    if bericht.funde.is_empty() && bericht.modell_geprueft {
        return Ok(());
    }
    let Some(token) = melden::broker_token() else {
        return Err("kein Broker-Token, Fund waere unbemerkt geblieben".to_owned());
    };
    let anfrage = melden::anfrage(melden::empfaenger(), &report::dm_text(bericht, 5));
    let url = format!("{}{}", melden::broker_basis_url(), melden::BROKER_DM_PFAD);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    match client
        .post(&url)
        .header("X-Internal-Token", token)
        // Kommt die Antwort nicht an, obwohl die DM raus ist, laeuft der
        // Block erneut. Der Schluessel haengt am Bericht, nicht am Versuch -
        // der Broker erkennt die Wiederholung daran.
        .header(
            melden::IDEMPOTENZ_KOPF,
            melden::idempotenz_schluessel(schluessel),
        )
        .json(&anfrage)
        .send()
        .await
    {
        Ok(antwort) if antwort.status().is_success() => {
            tracing::info!("DM zugestellt");
            Ok(())
        }
        Ok(antwort) => Err(format!("DM abgelehnt: HTTP {}", antwort.status().as_u16())),
        Err(fehler) => Err(format!("DM fehlgeschlagen: {fehler}")),
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
        let wurzel =
            std::env::temp_dir().join(format!("stream-audit-aufbewahrung-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
        tokio::fs::create_dir_all(&wurzel).await.expect("Ordner");
        let alt = wurzel.join("alt.md");
        let neu = wurzel.join("neu.md");
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
        };
        alte_berichte_loeschen(&konfiguration).await;
        assert!(!alt.exists(), "40 Tage alter Bericht muss weg sein");
        assert!(neu.exists(), "frischer Bericht muss bleiben");

        // 0 heisst unbegrenzt: nichts wird geloescht.
        tokio::fs::write(&alt, b"alt").await.expect("schreiben");
        mtime_setzen(&alt, vor_40_tagen);
        let unbegrenzt = Konfiguration {
            aufbewahrung_tage: 0,
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
        let wurzel =
            std::env::temp_dir().join(format!("stream-audit-kanalordner-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&wurzel).await;
        let kanalordner = wurzel.join("testkanal");
        tokio::fs::create_dir_all(&kanalordner)
            .await
            .expect("Ordner");
        let alt = kanalordner.join("alt.json");
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
        let blockordner =
            std::env::temp_dir().join(format!("stream-audit-zettel-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&blockordner).await;
        // Die Aufnahme liegt eine Ebene tiefer, so wie streamlink sie ablegt.
        let capture = blockordner.join("capture-abc123");
        tokio::fs::create_dir_all(&capture).await.expect("Ordner");
        let aufnahme = capture.join("audio.ts");
        tokio::fs::write(&aufnahme, b"nicht wirklich audio")
            .await
            .expect("schreiben");

        zettel_schreiben(&blockordner, "helmbombenricky", "4711", 7, 3600).await;

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

    #[test]
    fn aufnahmewurzel_liegt_unter_eigenem_ordner() {
        // Eigener Ordner, damit das Aufraeumen nie fremde Dateien in /tmp trifft.
        let wurzel = aufnahme_wurzel();
        assert!(wurzel.ends_with("stream-audit-captures"), "{wurzel:?}");
    }
}

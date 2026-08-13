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
    let Some(transkribierer) = OpenAiTranscriber::from_env() else {
        tracing::error!("Transkription nicht konfiguriert; ENGAGEMENT_STT_BASE_URL setzen");
        std::process::exit(2);
    };
    if !transkribierer.is_local() {
        tracing::warn!(
            "Transkription zeigt nicht auf localhost - Stream-Audio verlaesst den Rechner"
        );
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

    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("Abbruch angefordert"),
        _ = aufnahme => tracing::error!("Aufnahmeschleife beendet"),
        _ = auswertung => tracing::error!("Auswertungsschleife beendet"),
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
    let wurzel = std::env::temp_dir();
    let Ok(mut eintraege) = tokio::fs::read_dir(&wurzel).await else {
        return;
    };
    let mut zustand = plan::Aufnahme::starten("wiederaufnahme", "wiederaufnahme");
    let mut gefunden = 0usize;

    while let Ok(Some(eintrag)) = eintraege.next_entry().await {
        let name = eintrag.file_name().to_string_lossy().to_string();
        if !name.starts_with("voice-reaction-") {
            continue;
        }
        let Ok(mut dateien) = tokio::fs::read_dir(eintrag.path()).await else {
            continue;
        };
        while let Ok(Some(datei)) = dateien.next_entry().await {
            let pfad = datei.path();
            if pfad.extension().and_then(|e| e.to_str()) != Some("ts") {
                continue;
            }
            let block = zustand.block_fertig(pfad.to_string_lossy().to_string(), 0);
            warteschlange.lock().await.einreihen(block);
            gefunden += 1;
        }
    }

    if gefunden > 0 {
        tracing::info!(gefunden, "liegengebliebene Aufnahmen wieder eingereiht");
    }
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
    let mut laufend: std::collections::HashMap<String, tokio::task::JoinHandle<()>> =
        Default::default();

    loop {
        let live = match helix
            .get_streams_by_logins(&konfiguration.kanaele, None)
            .await
        {
            Ok(streams) => streams
                .into_iter()
                .map(|s| s.user_login.to_lowercase())
                .collect::<Vec<_>>(),
            Err(fehler) => {
                tracing::warn!(?fehler, "Live-Abfrage fehlgeschlagen");
                tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
                continue;
            }
        };

        // Beendete Aufnahmen aufraeumen: wer neu sendet, faengt wieder bei
        // Block 1 an und bekommt eine neue Lauf-Kennung.
        laufend.retain(|_, handle| !handle.is_finished());

        for kanal in &live {
            if laufend.contains_key(kanal) {
                continue;
            }
            let handle = tokio::spawn(kanal_aufnehmen(kanal.clone(), Arc::clone(&warteschlange)));
            laufend.insert(kanal.clone(), handle);
            tracing::info!(kanal, "Aufnahme gestartet");
        }

        tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
    }
}

/// Nimmt einen Kanal in Bloecken auf, bis der Stream endet oder der Deckel
/// greift. Laeuft als eigener Task, damit mehrere Kanaele sich nicht
/// gegenseitig blockieren.
async fn kanal_aufnehmen(kanal: String, warteschlange: Arc<Mutex<plan::Warteschlange>>) {
    let capturer = AudioCapturer::from_env();
    // Eine Kennung je Sendung. Ohne sie ueberschreibt der naechste Stream
    // desselben Kanals die Berichte des vorigen.
    let lauf = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut zustand = plan::Aufnahme::starten(kanal.clone(), lauf);

    loop {
        let Some(laenge) = zustand.naechste_blocklaenge() else {
            tracing::info!(kanal, "Aufnahmedeckel erreicht");
            return;
        };

        match capturer
            .capture(
                &kanal,
                laenge,
                tb_engagement::audio_capture::DEFAULT_QUALITY,
                None,
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
                return;
            }
        }
    }
}

/// Arbeitet die Warteschlange ab, immer nur einen Block gleichzeitig.
async fn auswertungs_schleife(
    transkribierer: OpenAiTranscriber,
    konfiguration: Konfiguration,
    warteschlange: Arc<Mutex<plan::Warteschlange>>,
) {
    loop {
        let naechster = warteschlange.lock().await.naechster();
        let Some(block) = naechster else {
            tokio::time::sleep(Duration::from_secs(15)).await;
            continue;
        };

        match block_auswerten(&transkribierer, &konfiguration, &block).await {
            Ok(()) => {
                // Erst nach erfolgreicher Auswertung wegraeumen. Wer bei einem
                // Fehler loescht, vernichtet bei einem Aussetzer der
                // Transkription den einzigen Beleg, den es je gab.
                let pfad = PathBuf::from(&block.datei);
                if let Some(verzeichnis) = pfad.parent() {
                    let _ = tokio::fs::remove_dir_all(verzeichnis).await;
                }
            }
            Err(fehler) => {
                tracing::warn!(
                    block = %block.bezeichnung(),
                    fehler,
                    "Auswertung fehlgeschlagen, Aufnahme bleibt liegen"
                );
            }
        }
    }
}

async fn block_auswerten(
    transkribierer: &OpenAiTranscriber,
    konfiguration: &Konfiguration,
    block: &plan::Block,
) -> Result<(), String> {
    let transkript = transkribierer
        .transcribe_clip(Path::new(&block.datei))
        .await
        .map_err(|e| format!("Transkription: {e}"))?;

    let segmente = segmente_bauen(block, &transkript.text, transkript.duration_seconds);
    if segmente.is_empty() {
        tracing::info!(block = %block.bezeichnung(), "kein Text im Block");
        return Ok(());
    }

    let mut funde = tb_stream_audit::regelfunde(&segmente);
    let (modell_funde, modell_hinweis) = modellfunde(&segmente).await;
    funde.extend(modell_funde);
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
        anbieter: endpunkt.provider.to_owned(),
        transkript_behalten: konfiguration.transkript_behalten,
        segmente: segmente.len(),
        modell_geprueft: modell_hinweis.is_none(),
        modell_hinweis: modell_hinweis.unwrap_or_default(),
        funde,
    };

    schreiben(konfiguration, block, &bericht, &transkript.text).await?;
    dm_senden(&bericht).await;
    Ok(())
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
            .post(format!("{}/chat/completions", endpunkt.base_url))
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
            Ok(geparst) => raus.extend(llm::zu_funden(&geparst, stapel)),
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
async fn dm_senden(bericht: &Bericht) {
    if bericht.funde.is_empty() {
        return;
    }
    let Some(token) = melden::broker_token() else {
        tracing::warn!("kein Broker-Token, DM entfaellt");
        return;
    };
    let anfrage = melden::anfrage(&melden::empfaenger(), &report::dm_text(bericht, 5));
    let url = format!("{}{}", melden::broker_basis_url(), melden::BROKER_DM_PFAD);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    match client
        .post(&url)
        .header("X-Internal-Token", token)
        .json(&anfrage)
        .send()
        .await
    {
        Ok(antwort) if antwort.status().is_success() => tracing::info!("DM zugestellt"),
        Ok(antwort) => tracing::warn!(status = antwort.status().as_u16(), "DM abgelehnt"),
        Err(fehler) => tracing::warn!(?fehler, "DM fehlgeschlagen"),
    }
}

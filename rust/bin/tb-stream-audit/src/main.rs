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
//! Aufnahme und Transkription laufen nicht gleichzeitig. Die Maschine hat keine
//! GPU; drei parallel transkribierte Streams wuerden sich dieselben Kerne
//! teilen wie das Modell, und der Rueckstand waechst schneller, als er abgebaut
//! wird. Aufnehmen ist billig, also nimmt der Dienst in Bloecken auf und
//! arbeitet die Warteschlange danach der Reihe nach ab.

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

fn helix_aus_umgebung() -> Option<HelixClient> {
    let id = std::env::var("TWITCH_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    let secret = std::env::var("TWITCH_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())?;
    HelixClient::new(HelixConfig::new(id, secret)).ok()
}

/// Prueft im Takt, wer sendet, und nimmt von jedem sendenden Kanal einen Block
/// auf. Aufnahmen laufen nebeneinander, Auswertung nicht.
async fn aufnahme_schleife(
    helix: HelixClient,
    konfiguration: Konfiguration,
    warteschlange: Arc<Mutex<plan::Warteschlange>>,
) {
    let capturer = AudioCapturer::from_env();
    let mut laufend: std::collections::HashMap<String, plan::Aufnahme> = Default::default();

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

        // Wer nicht mehr sendet, beginnt beim naechsten Mal wieder bei Block 1.
        laufend.retain(|kanal, _| live.contains(kanal));

        for kanal in &live {
            let zustand = laufend
                .entry(kanal.clone())
                .or_insert_with(|| plan::Aufnahme::starten(kanal.clone()));
            let Some(laenge) = zustand.naechste_blocklaenge() else {
                tracing::info!(kanal, "Aufnahmedeckel erreicht");
                continue;
            };

            match capturer
                .capture(
                    kanal,
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
                Err(fehler) => tracing::warn!(kanal, ?fehler, "Aufnahme fehlgeschlagen"),
            }
        }

        tokio::time::sleep(Duration::from_secs(plan::LIVE_PRUEFUNG_SEKUNDEN)).await;
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

        if let Err(fehler) = block_auswerten(&transkribierer, &konfiguration, &block).await {
            tracing::warn!(block = %block.bezeichnung(), fehler, "Auswertung fehlgeschlagen");
        }

        // Aufnahme wegräumen, sobald sie ausgewertet ist. Wer den Wortlaut
        // braucht, liest das Transkript; das Rohvideo waere nur Plattenlast.
        let pfad = PathBuf::from(&block.datei);
        if let Some(verzeichnis) = pfad.parent() {
            let _ = tokio::fs::remove_dir_all(verzeichnis).await;
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
    funde.extend(modellfunde(&segmente).await);
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
        funde,
    };

    schreiben(konfiguration, block, &bericht, &transkript.text).await?;
    dm_senden(&bericht).await;
    Ok(())
}

/// Teilt den Blocktext gleichmaessig in Segmente. Whisper liefert hier einen
/// Text am Stueck; der Zeitbezug im Bericht ist damit auf das Segmentraster
/// genau, was fuer "hoer dir Minute 12 an" reicht.
fn segmente_bauen(block: &plan::Block, text: &str, dauer: f64) -> Vec<Segment> {
    let saetze: Vec<&str> = text
        .split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if saetze.is_empty() {
        return Vec::new();
    }
    let je_segment = (SEGMENT_SEKUNDEN / dauer.max(1.0) * saetze.len() as f64).ceil() as usize;
    let je_segment = je_segment.max(1);

    saetze
        .chunks(je_segment)
        .enumerate()
        .map(|(i, teil)| {
            let anteil = dauer / saetze.len().max(1) as f64;
            let start = block.versatz_sekunden as f64 + i as f64 * je_segment as f64 * anteil;
            Segment {
                id: block.segment_id(i + 1),
                start_sekunden: start,
                ende_sekunden: start + teil.len() as f64 * anteil,
                text: teil.join(" "),
            }
        })
        .collect()
}

/// Modellfunde ueber den im Bot konfigurierten Anbieter. Faellt der Aufruf aus,
/// bleibt es bei den Regelfunden - ein Audit ohne Modell ist duenner, aber
/// besser als keines.
async fn modellfunde(segmente: &[Segment]) -> Vec<tb_stream_audit::Fund> {
    let endpunkt = tb_llm::selection::endpoint_for(llm::USE_CASE);
    let Some(schluessel) = endpunkt.api_key.clone() else {
        tracing::info!("kein Schluessel fuer {}, nur Regelfunde", endpunkt.provider);
        return Vec::new();
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap_or_default();

    let mut raus = Vec::new();
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
            continue;
        };
        match serde_json::from_str::<llm::ModellAntwort>(json) {
            Ok(geparst) => raus.extend(llm::zu_funden(&geparst, stapel)),
            Err(fehler) => tracing::warn!(?fehler, "Modellantwort unlesbar"),
        }
    }
    raus
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

    tokio::fs::write(basis.with_extension("md"), report::markdown(bericht))
        .await
        .map_err(|e| format!("Bericht: {e}"))?;
    let json = serde_json::to_string_pretty(bericht).map_err(|e| format!("JSON: {e}"))?;
    tokio::fs::write(basis.with_extension("json"), json)
        .await
        .map_err(|e| format!("JSON schreiben: {e}"))?;
    if konfiguration.transkript_behalten {
        tokio::fs::write(basis.with_extension("txt"), transkript)
            .await
            .map_err(|e| format!("Transkript: {e}"))?;
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

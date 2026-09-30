# Auftrag: Aus !clip-Momenten lernen, wie gute Clips entstehen (Paket B)

## Ziel in Nutzerworten

Wenn jemand im Chat `!clip` schreibt, wissen wir den Zeitpunkt. Dann holen wir aus der Stream-Wiederholung (VOD) das Stück von 1,5 Minuten vor bis 1,5 Minuten nach diesem Moment, werten Bild und Ton aus und lernen daraus, wie es zu der Situation kam und was sie besonders macht. Am Ende steht eine Vorlage, mit der der Bot selbst gute Clips findet und sauber schneidet (Einstieg, Aufbau, Höhepunkt, Ende), statt stur die letzten 30 Sekunden zu nehmen.

## Bestand (nicht neu bauen, weiterverwenden)

- `!clip` im Twitch-Bot erzeugt Clips von rund 30 s (`source_kind` in `twitch_clips_social_media`); Fundstelle des Befehls per `graphify query "!clip command create clip"` im Repo suchen.
- `twitch_clips_social_media.vod_id` / `vod_offset_s` (Migration aus Strang A 2026-09-06) und `twitch_clip_merkmale`, `twitch_vod_highlights`.
- Lernkorpus: Bin `rust/crates/tb-social-media/src/bin/korpus_ernte.rs`, Report `.tasks/2026-09-06-highlight-erkennung-vod/korpus-report.md`: 241 Clips, Merkmale nur im Fenster "letzte 30 s vor Clip-Ende", stärkstes Signal Lautstärke-Spitze (72 %), dann Soul-Sprung, Death-Screen, Objective. Sprache, Lachen und Schlüsselwörter lieferten 0 %, das spricht für einen Fehler in der Sprach-Auswertung, zuerst prüfen.
- Detektor-Prototyp `ops/highlight-detector/` ist **Python** und damit nur Legacy-Referenz. Die Logik, die weiterlebt, wird in Rust nachgebaut (OCR per tesseract-CLI, Lautstärke per ffmpeg `ebur128`/`astats`, Bild per ffmpeg-Filter); nichts an dem Python-Code erweitern.
- Lokaler STT-Dienst `127.0.0.1:8791` (faster-whisper, bewusste Python-Ausnahme) für Sprache, Aufrufer siehe `rust/crates/tb-social-media/src/transcription.rs`.
- VOD-Zugriff: `tb-vod-archive` (Download-Logik) und Helix `videos`; für ein ±90-s-Fenster nur den Ausschnitt aus der HLS-Playlist laden, nicht das ganze VOD. Nichts dauerhaft auf der Platte halten, Ausschnitte nach der Auswertung löschen (Platte knapp).

## Arbeitsschritte

1. **Zeitpunkt jedes !clip-Aufrufs** sauber festhalten: VOD-ID und Offset beim Anlegen des Clips schreiben (Daten beim Schreiben richtig, kein Nachtrag-Mechanismus); für Altdaten einmaliger Backfill über Helix `clips` (`vod_offset`, `video_id`).
2. **Kontext-Ernte in Rust**: neues Bin oder Erweiterung von `korpus_ernte`, das je Clip mit VOD-Bezug das Fenster Clip-Moment −90 s bis +90 s aus dem VOD lädt und je Sekunde eine Zeitleiste der Signale schreibt: Lautstärke (LUFS, Spitzen), Sprache mit Zeitstempeln (STT), Lachen/Ausrufe, Kill-Feed/Souls/Objective/Death-Screen per OCR bzw. Bildmerkmal, Szenenwechsel, Chat-Aktivität je Sekunde aus `twitch_chat_messages` desselben Kanals. Speicherung in Postgres (neue Tabelle per neuer Migration unter `rust/migrations/`, Schema-Snapshot und `.sqlx` mitziehen).
3. **Muster lernen**: aus den Zeitleisten ableiten, wie lange der Aufbau vor dem Höhepunkt typischerweise ist, welche Signalfolgen einen Höhepunkt ankündigen und wo ein Clip sinnvoll endet (Reaktion danach). Ergebnis als Report `LERN-REPORT.md` in diesem Ordner mit echten Beispielen (Clip-Link, Zeitleiste, gelernte Schnittpunkte) und als gespeicherte Vorlage (Gewichte, typische Vorlauf- und Nachlaufzeit) in der DB, nicht einkompiliert.
4. **Vorlage anwenden**: eine Funktion, die für einen Clip-Moment die empfohlenen Schnittpunkte (Start, Ende) aus Zeitleiste plus Vorlage liefert; Paket A rendert später mit diesen Grenzen. Nur Funktion plus Test, noch keine Umschaltung der Upload-Pipeline.
5. **Messung an echtem Material**: mindestens 20 echte !clip- bzw. Streamer-Clips von earlysalty und Partnern; Ergebnis mit Zahlen und Beispielen im Report. Weicht die Datengrundlage ab (zu wenige Clips mit VOD, abgelaufene VODs), das offen dokumentieren statt die Grundlage passend zu machen.

## Nicht anfassen

- Render-Pipeline, Layout, Untertitel, Uploader (Paket A, Branch `feat/clip-social-format-20260929`).
- Kein Python, keine Code-Kommentare, keine externen KI-Anbieter für Nutzer- oder Streamdaten (alles lokal). Kein LLM-Aufruf ohne zentralen Provider `tb_llm::endpoint_for`, kein neues Modell.

## Fertig-Kriterium

- Kontext-Zeitleisten für mindestens 20 echte Clips in der DB, `LERN-REPORT.md` mit Befund und Beispielen, Vorlage gespeichert, Schnittpunkt-Funktion mit Test.
- `cargo fmt`, `cargo clippy`, `cargo test` für die berührten Crates grün bzw. nicht schlechter als Baseline.

## Deploy-Weg

Nach Review und Merge-Gate durch die Hauptsession: Migration von Hand als postgres, Release per `deploy-twitch-release <sha>`.

# PLAN: Highlight-Erkennung aus VODs (OCR, Sprache, Bild)

- status: aktiv
- datum: 2026-09-06
- contract: ./CONTRACT.md (Massstab, unveraenderlich; Amendment REQ-09 gilt)
- evidence: ./EVIDENCE.md

Reihenfolge fest: erst Lern-Korpus (REQ-09), dann Detektor mit gelernten Gewichten, dann Messung gegen die 25 echten Clips. Kein Blackbox-Modell, alle Gewichte und Schwellen liegen erklaerbar in einer Konfigurationsdatei.

## Kernentscheidungen (mit Begruendung)

1. Helix bleibt in Rust, Python ruft nie selbst Helix. Der geteilte `HelixClient` (client.rs:88, Token via AppTokenManager, Secrets aus Infisical) hat den einzigen sauberen App-Token-Weg. Der billigste Pfad fuer REQ-05 und REQ-09: den bestehenden Clip-Fetch in `rust/crates/tb-social-media/src/clip/` erweitern (parse_clip liest zusaetzlich `vod_offset`/`video_id`, Migration + Insert um `vod_id`/`vod_offset_s`), und eine Rust-Erntefunktion fuellt die Korpusliste (Partner-Clips plus Deadlock-Clips nach Views). Python liest diese Zeilen read-only aus Postgres und macht nur OCR/STT/Bild. Kein zweiter OAuth-Weg, kein Secret im Prototyp.

2. Migrationsverzeichnis ist real `rust/migrations/`, nicht das im Contract-Scope genannte `rust/crates/tb-db/migrations/` (existiert nicht). Entscheidung: Migration nach `rust/migrations/` legen (dort liegen alle echten Migrationen) und die Scope-Abweichung ueber eine Freigabe-Datei `~/.claude/.contract-approvals/2026-09-06-highlight-erkennung-vod` (legt nur der Nutzer an) oder ein Contract-Amendment abdecken. Der Detektor-Prototyp selbst braucht diese Freigabe nicht, nur der Migrations-Schritt M1.

3. STT liefert heute Segment-Zeitstempel, keine Wort-Zeitstempel (`stt_server.py:109` ruft `transcribe` ohne `word_timestamps=True`, `ops/stt-server` ist ausserhalb des erlaubten Bereichs). Entscheidung: das Sprach-Signal (Lautstaerke-Huellkurve aus ffmpeg, Schluesselwoerter, Wortrate) arbeitet auf Segmentebene, das reicht fuer die Signalfenster in REQ-03. Echte Wort-Zeitstempel fuer Strang-B-Untertitel sind ein spaeteres Amendment am STT-Server, nicht Teil dieses Auftrags.

4. Schnitt per Reencode, nicht Stream-Copy. Vorlauf 20 s und Nachlauf 10 s (REQ-06) muessen exakt sitzen; Stream-Copy schneidet nur an Keyframes (bei 2-s-GOP bis 2 s daneben). Vorbild ist `build_ffmpeg_cmd` in tb-highlight (twitch_vod.rs:149). Kosten sind unkritisch, weil nur Kandidaten ueber der Schwelle geschnitten werden.

5. opencv-python fehlt systemweit (`import cv2` schlaegt fehl), tesseract 5.3.4 ist da. Der Prototyp bekommt ein eigenes venv nach dem Muster `ops/stt-server` (venv, pip: opencv-python-headless, numpy, requests, psycopg2-binary), keine System-Pakete ausser tesseract/ffmpeg/yt-dlp.

## Meilensteine

### M0: Geruest und OCR-Kalibrierung (Sichtpruefung Nutzer)

- Aenderungen: `ops/highlight-detector/` anlegen (venv-Bootstrap `setup.sh` nach stt-server-Muster, `requirements.txt`, `regionen.toml` als leere Kalibrierungsdatei, `cli.py` mit Subkommando-Geruest `lernen|analyse|messen`), `config/regionen.toml` mit Bildregionen (Kill-Feed oben rechts, Mitte-Einblendung, Soul/Objective-Leiste), Frame-Vorverarbeitung und tesseract-Parameter.
- Vorverarbeitung je Region (Vorschlag): Crop auf die Region, Graustufen, Skalierung x2 (tesseract mag ~30 px Zeichenhoehe), Otsu-Threshold, bei hellem Text auf dunklem HUD invertieren, leichter Median gegen Kompressionsrauschen.
- tesseract-Parameter (Vorschlag): `--psm 7` (Kill-Feed-Zeile) bzw. `--psm 6` (mehrzeilige Einblendung), `--oem 1`, Whitelist Buchstaben+Ziffern+Doppelpunkt fuer Namen/Zahlen, Sprache `deu+eng`.
- Zwischenzustand: `highlight-detector kalibrieren <vod.mp4>` zieht 1 Frame je 10 Minuten (ffmpeg), zeichnet die Regionen aus `regionen.toml` ein und legt die Beispielbilder in `.tasks/2026-09-06-highlight-erkennung-vod/kalibrierung/` ab.
- Validierung: `ls .tasks/2026-09-06-highlight-erkennung-vod/kalibrierung/*.png` zeigt mindestens 20 markierte Frames aus 2 verschiedenen VODs.
- SICHTPRUEFUNG NUTZER: der Nutzer prueft die eingezeichneten Regionen an den Beispielbildern und bestaetigt Kill-Feed/HUD-Lage, bevor OCR im grossen Lauf startet.
- Stop-Regel: keine OCR-Massenlaeufe, bevor der Nutzer die Regionen freigibt.
- Zeit: 0,5 Tag.

### M1: Rust, Clip-Fetch um vod_id/vod_offset_s und Korpus-Ernte (REQ-05, REQ-09)

- Aenderungen: Migration in `rust/migrations/` (`ALTER TABLE twitch_clips_social_media ADD COLUMN vod_id TEXT, ADD COLUMN vod_offset_s INTEGER`); `parse_clip` (clip/helix.rs) liest `vod_offset`/`video_id`; `ClipRecord` (clip/model.rs) und Insert (clip/repository.rs:108) erweitern; Rust-Erntefunktion in clip/service.rs, die die Korpusliste (Partner-Clips aus DB plus Deadlock-Clips per Helix nach Views, Ziel 500 bis 2000) bereitstellt; `rust/.sqlx/` und `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` nachziehen.
- Zwischenzustand: neu gefetchte Clips tragen `vod_id`/`vod_offset_s`; die 25 echten Clips werden per Backfill-Lauf (bestehender Fetch, kein Dauer-Nachtrag) einmalig verortet.
- Validierung: `cargo test -p tb-db fresh_migrations_match_committed_schema_snapshot` gruen; `cargo sqlx prepare --check`; SELECT auf `twitch_clips_social_media` zeigt gefuellte `vod_offset_s` fuer earlysalty-Clips.
- Stop-Regel: Migration als `postgres` anwenden, danach SELECT/INSERT/UPDATE/DELETE an twitchbot/twitchdash, Version in `_sqlx_migrations` (sha384) eintragen, nie CREATE auf public an die Dienst-Rollen (Docs/workspace/gates-und-merge.md). Ohne Contract-Freigabe fuer `rust/migrations/` kein Merge.
- Zeit: 1 Tag.

### M2: Signal-Extraktoren mit Cache (REQ-02, REQ-03, REQ-04, REQ-08)

- Aenderungen in `ops/highlight-detector/`: je Signal ein Modul plus Cache je VOD unter `~/.cache/highlight-detector/<vod_id>/` (ocr.jsonl, audio.jsonl, transcript.json, scene.jsonl), abschaltbar per Konfig.
- OCR: ffmpeg-Frame-Sampling 2 fps (konfigurierbar), tesseract je Region, Parser erkennt eigene Kills, eigenen Tod, Multikill, Objective; Regeln in `regionen.toml`.
- Sprache: Lautstaerke-Huellkurve per ffmpeg `astats`/`ebur128` je 1 s; STT je VOD in 10-Minuten-Bloecken an 127.0.0.1:8791 (`POST /v1/audio/transcriptions`, `response_format=verbose_json`), Segmente mit Start/Ende gespeichert; Signale: Pegel-Spike gegen gleitendes 60-s-Mittel, Schluesselwortliste (konfigurierbar), Wortrate je Segment.
- Bild: Szenenwechsel per ffmpeg `select='gt(scene,0.4)'`, Death-Screen per Saettigungs-/Grauwert-Einbruch je Sekunde, Template-Matching (OpenCV `matchTemplate`) fuer feste Einblendungen; Templates werden in M3 aus dem Korpus geschnitten.
- Zwischenzustand: `highlight-detector analyse <vod.mp4> --nur-signale` schreibt die vier Cache-Dateien und laesst alle 16 VODs durchlaufen.
- Validierung: `pytest ops/highlight-detector/tests/ -k parser` gruen (OCR-Parser gegen echte Frame-Fixtures, Score-Funktion); Cache-Wiederholungslauf ruft kein tesseract erneut auf (Zeitmessung).
- Stop-Regel: `nice 15`, hoechstens 8 Prozesse; wenn ein 4-h-VOD ueber 2 h braucht, Frame-Rate/Regionen kuerzen statt Kerne erhoehen.
- Zeit: 2 Tage.

### M3: Lern-Korpus, Report und Gewichts-Ableitung (REQ-09) (Sichtpruefung Nutzer)

- Aenderungen: Migration `twitch_clip_merkmale` in `rust/migrations/` (Spalten: clip_id, streamer_twitch_id, views, dauer_s, ersteller, signal, offset_vom_ende_s, wert DOUBLE, created_at); `highlight-detector lernen` laedt je Korpus-Clip nur das Zeitfenster (yt-dlp `--download-sections`), extrahiert die Signale aus M2, schreibt je Signal-Treffer eine Zeile relativ zum Clip-Ende und loescht die Clip-Datei danach.
- Auswertung: Verteilung je Signal ueber die letzten 30 s vor Clip-Ende, Top-Views gegen Rest; Gewichte per einfacher logistischer Regression oder Trefferhaeufigkeit je Signal (erklaerbar), Schwelle aus der Trennung gute/schlechte Clips. Ergebnis in `config/gewichte.toml`.
- Report `.tasks/2026-09-06-highlight-erkennung-vod/korpus-report.md`: "Warum sind gute Clips gut", Verteilungen, Top-Muster, je Muster echte Beispiel-Clip-URLs.
- Validierung: `SELECT count(*) FROM twitch_clip_merkmale` liegt im Zielbereich (500 bis 2000 Clips verarbeitet); `config/gewichte.toml` existiert und ist nicht leer; Report nennt echte clip_url.
- SICHTPRUEFUNG NUTZER: der Nutzer liest den Korpus-Report und bestaetigt die abgeleiteten Gewichte/Schwellen, bevor der Detektor sie nutzt.
- Stop-Regel: Clip-Dateien nach der Extraktion loeschen (Platte knapp); kein Modell ausser der offenen Regression.
- Zeit: 2 Tage.

### M4: Detektor mit gelernten Gewichten (REQ-01, REQ-04)

- Aenderungen: Sliding-Window ueber die Signal-Zeitreihen, gewichtete Summe aus `config/gewichte.toml`, Nicht-Maximum-Unterdrueckung, Schwelle aus M3; `highlight-detector analyse <vod.mp4>` gibt Kandidaten als JSON (start_s, end_s, score, signale mit Einzelwerten) plus Konsolentabelle.
- Zwischenzustand: alle 16 VODs laufen vollstaendig durch, Kandidatenliste je VOD als JSON.
- Validierung: `highlight-detector analyse <vod.mp4> --json` liefert valides JSON mit Einzelwerten je Signal; die 16 VODs enden ohne Fehler.
- Stop-Regel: Gewichte nur aus M3, nicht von Hand geraten.
- Zeit: 1 Tag.

### M5: Messung gegen die 25 echten Clips (REQ-05)

- Aenderungen: `highlight-detector messen` verortet die echten Clips per `vod_id`/`vod_offset_s` (aus M1) im VOD, gibt je VOD Trefferquote (Kandidat ueberlappt einen echten Clip, hoechstens 15 s versetzt) und Kandidaten ohne echten Clip aus; earlysalty ueber twitch_streamers zur Twitch-User-ID aufgeloest, nicht per Login gefiltert.
- Zwischenzustand: Trefferquote je VOD und gesamt liegt vor; Schwelle in `config/gewichte.toml` gegen die Messung nachjustiert.
- Validierung: `highlight-detector messen --alle` druckt Trefferquote und Falsch-Positiv-Liste; Zielmarke fuer die Freigabe legt der Nutzer nach Sicht der ersten Zahlen fest.
- Stop-Regel: keine Filter, die schlechte Treffer verstecken; Falsch-Positive offen ausweisen.
- Zeit: 0,5 Tag.

### M6: Schnitt und Persistenz (REQ-06, REQ-07)

- Aenderungen: Migration `twitch_vod_highlights` in `rust/migrations/` (streamer_twitch_id, vod_id, start_s, end_s, score, signale JSONB, status, created_at); `highlight-detector analyse --schneiden` schneidet jeden Kandidaten ueber der Schwelle per ffmpeg-Reencode (Vorlauf 20 s, Nachlauf 10 s, konfigurierbar) in ein Ausgabeverzeichnis je VOD, Dateiname mit Zeitfenster und Score; Kandidaten werden in `twitch_vod_highlights` geschrieben.
- Zwischenzustand: geschnittene mp4 je VOD liegen zur Sichtpruefung bereit, Zeilen in `twitch_vod_highlights`.
- Validierung: `cargo test -p tb-db fresh_migrations_match_committed_schema_snapshot` gruen nach der Migration; geschnittene Dateien tragen Zeitfenster und Score im Namen; `SELECT count(*) FROM twitch_vod_highlights` groesser 0.
- SICHTPRUEFUNG NUTZER: der Nutzer urteilt an den geschnittenen mp4, ob die Kandidaten echte Highlights sind.
- Stop-Regel: Migration wie M1 als postgres mit Least-Privilege; Ausgabeverzeichnis auf der grossen Partition, nicht /tmp fuellen.
- Zeit: 1 Tag.

## Rot-Test (Klasse hoch, vor dem Bau)

- Vor M2/M4: `pytest ops/highlight-detector/tests/test_score.py` und `test_ocr_parser.py`. Der OCR-Parser-Test fuettert echte Kill-Feed-Frame-Fixtures (aus M0-Kalibrierung geschnitten) und erwartet die geparsten Kill-Ereignisse; der Score-Test erwartet aus einer festen Signal-Zeitreihe einen bekannten Kandidaten. Beide sind rot, solange die Module fehlen, und werden mit Testname plus Fehlermeldung als Baseline festgehalten. Der Implementierer aendert diese Tests nicht.
- Vor M1/M6: `cargo test -p tb-db fresh_migrations_match_committed_schema_snapshot` ist nach dem Hinzufuegen der Migration rot (Schema-Drift), bis der Snapshot regeneriert ist; das ist der erwartete rote Ausgangslauf der Schemaaenderung.

## Last und Laufzeit (REQ-08)

- OCR ist der Kostentreiber. Bei 2 fps und 4 h sind das 28 800 Frames; je Frame nur die kalibrierten Regionen (nicht das Vollbild). Realistische tesseract-Zeit je kleiner Region ist in M0 zu messen (Annahme 20 bis 60 ms je Region, 3 Regionen), macht grob 0,1 bis 0,2 s je Frame, seriell 45 bis 90 Minuten, mit 8 Prozessen deutlich darunter. Der exakte Messwert kommt aus M0 in den Plan.
- STT: RTF 0,200 (README-Messung), 4 h Audio also rund 48 Minuten, einmal je VOD, Ergebnis gecacht.
- Bild (ffmpeg scene/astats): ein Durchlauf je VOD, wenige Minuten.
- Cache je Signal (M2) sorgt dafuer, dass ein Wiederholungslauf mit geaenderten Gewichten keine neue OCR/STT braucht (REQ-08).
- Grenzen: `nice 15`, hoechstens 8 Prozesse, Ausgabe und Cache auf der grossen Partition.

## Umsetzungsstatus (Implementierer)

### Rote Baseline (vor M2/M4, Klasse hoch)
- Befehl: `.venv/bin/python -m pytest tests/ -q` in `ops/highlight-detector/`.
- `tests/test_ocr_parser.py`: `ImportError: cannot import name 'ocr' from 'highlight_detector'` (8 Parser-Tests, rot).
- `tests/test_score.py`: `ImportError: cannot import name 'detector' from 'highlight_detector'` (4 Score-Tests, rot).
- 2 errors during collection, festgehalten am 2026-09-06 vor Implementierung der Module.

### Milestone-Status (2026-09-06)
- M0 Kalibrierung: ERLEDIGT. 37 markierte Frames aus 2 VODs unter `kalibrierung/` (Kill-Feed oben links mit Killer/Opfer-Trennlinie, Banner zentral, Souls unten links). OCR an echten Crops kalibriert (Kill-Feed skal 4x invertiert, Souls x0=0.058..0.132). Sichtpruefung Nutzer offen.
- M1 Rust Clip-VOD-Felder + Korpus-Ernte: ERLEDIGT (Subagent). `vod_id`/`vod_offset_s` in parse_clip/model/INSERT, `fetch_top_game_clips`/`register_corpus_clip` als Bibliotheks-API. tb-social-media 224->228 gruen, fresh_migrations gruen, sqlx/snapshot nachgezogen. Migrationen NICHT auf Prod (offen: Nutzer).
- M2 Signal-Extraktoren + Cache: ERLEDIGT. OCR (parallel bis 8 Prozesse), Lautstaerke, STT, Szenenwechsel, Death-Screen; Rohdaten-Cache getrennt vom Event-Bau (Gewichts-Rerun ohne neue OCR).
- M3 Lern-Korpus + Gewichte + Report: Teilbeleg (limit 20) laeuft; voller 800er-Lauf als Hintergrund offen. Gewichte werden aus Praevalenz abgeleitet, Schwelle aus 25-Perzentil der Clip-Spitzenscores.
- M4 Detektor: ERLEDIGT. Sliding-Window + NMS, Score mit Einzelbeitraegen, JSON+Tabelle. Kandidaten sitzen an echten Clip-Momenten (t=13787 eigener Kill, t=13799 eigener Tod).
- M5 Messung: Real-Beleg ueber lokale VOD-.info.json-Verortung (created_at gegen VOD-Zeitfenster), 18/25 earlysalty-Clips in 4 lokalen VODs verortbar. Prod-Weg ueber vod_offset_s (Helix-Backfill) offen.
- M6 Schnitt + Persistenz: Code fertig (ffmpeg-Reencode, twitch_vod_highlights). DB-Schreiben faellt bis zur Prod-Migration lokal zurueck.

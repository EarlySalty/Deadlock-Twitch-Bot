# STT-Server: Dauerlast beenden, Betrieb sauber machen, Rust-Port prüfen

Stand 2026-09-05. Klasse medium. Noch nicht beauftragt, nur Plan.

## Befund

- `ops/stt-server/stt_server.py` (faster-whisper, large-v3-turbo, int8, 8 Threads) läuft als User-Unit `deadlock-stt-server.service` seit 2026-08-13 durchgehend mit rund 30 bis 45 % CPU (9 Tage 16 Stunden CPU-Zeit in 22 Tagen), 1,3 GB RSS.
- Die Unit zeigt auf einen fremden Checkout (`/home/naniadm/Documents/Deadlock-Twitch-Bot`), nicht auf `/opt/deadlock/twitch/current`. Kein CPU-Deckel, kein Nice, keine Abhängigkeit vom Bot.
- Aufrufer: `tb-engagement/src/background.rs:186` (Stream-Transkripte, alle 75 s je Kanal mit `enabled = TRUE` in `twitch_engagement_settings`, 45-s-Capture) und `background.rs:344` (Reaktions-Lernmodus, Aufnahmen von `zro_dl` alle rund 5 Minuten). Dazu `tb-social-media/src/enrich_pipeline.rs:229` (Clip-Anreicherung) und `tb-stream-audit`.
- Aktiv ist nur der Kanal `test` (enabled) plus der Lernmodus auf `zro_dl`. Es gibt keinen Live-Check vor dem Capture: `run_transcribe_capture` (`background.rs:146`) ruft streamlink für jeden enabled Kanal, egal ob der Kanal live ist.
- Warum Python: faster-whisper (CTranslate2) war auf diesem Host (EPYC, keine GPU, AVX-512) der gemessen schnellste CPU-Pfad, siehe Tabelle in `ops/stt-server/README.md`. Ein Rust-Weg wurde nie gemessen.

## Ziel

STT verbraucht nur dann CPU, wenn ein Partner live ist und eine Funktion das Transkript wirklich nutzt. Der Dienst gehört zum Bot-Deploy, nicht in eine User-Session.

## Schritte

1. Sofortmaßnahme (erledigt 2026-09-05): `CPUWeight=50` und `CPUQuota=400%` auf der User-Unit, damit Bot, Dashboard und Postgres (CPUWeight 1000) immer Vorrang haben.
2. Live-Gate im Bot: `schedule_stream_transcripts` und der Lernmodus fragen vor dem Capture `twitch_live_state`; offline Kanäle werden übersprungen. Regressionstest: enabled Kanal offline erzeugt keinen Capture-Aufruf.
3. Verbraucher prüfen: Wer liest `twitch_stream_transcript_segments` und das Lern-Archiv? Wird das Ergebnis von keinem Live-Pfad (Smalltalk, Reaktions-Mapper) genutzt, dann Feature-Schalter default aus und Lernmodus nur auf ausdrückliche Freigabe.
4. Unit umziehen: System-Unit mit `User=twitchbot`, `WorkingDirectory=/opt/deadlock/twitch/current`, `ExecStart` aus dem Release-Checkout, `After=deadlock-twitch-bot-rust.service` nicht nötig (Bot ruft per HTTP mit Retry). Modell-Cache (`~/.cache/huggingface`, 36 GB, sechs Whisper-Varianten) auf das eine genutzte Modell reduzieren.
5. Rust-Port bewerten: `whisper-rs` (whisper.cpp) mit ggml `large-v3-turbo` q5_1 gegen den Python-Dienst messen (gleiche 20-s-Fenster, gleiche Sprache, RTF und Wortfehler). Nur portieren, wenn RTF gleich oder besser. Dann als Crate `tb-stt` mit derselben HTTP-Form (`POST /v1/audio/transcriptions`, `verbose_json`) in `tb-bot` einbetten, Python-Dienst löschen.

## Aufwand

- Schritte 1 bis 3: ein Nachmittag Agentenarbeit inklusive Tests.
- Schritt 4: eine Stunde plus Deploy.
- Schritt 5: Messung ein Tag, Port zwei bis drei Tage, nur wenn die Messung dafür spricht.

## Nicht-Ziele

- Kein Modellwechsel ohne Messung.
- Keine Cloud-STT (Audio verlässt die Maschine nicht).

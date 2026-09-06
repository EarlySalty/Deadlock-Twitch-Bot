# Contract: Highlight-Erkennung aus VODs (OCR, Sprache, Bild)

status: aktiv
datum: 2026-09-06
klasse: hoch
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

Bestand: `.tasks/2026-09-06-clip-aufbereitung-vod/RESEARCH-*.md`. Entscheidung des
Users vom 2026-09-06: tb-highlight (Kill-Momente aus Demos) hat nur Murks geliefert
und wird nicht weiterverwendet; Erkennung wird neu gebaut mit OCR, Sprache und Bild.

## Ziel

Aus einem lokalen VOD des Nutzers entsteht automatisch eine bewertete Liste von
Highlight-Kandidaten (Zeitfenster, Score, Signal-Begründung) plus geschnittene mp4,
gemessen an den echten Clips, die Zuschauer aus demselben VOD gemacht haben.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: `highlight-detector analyse <vod.mp4>` liefert für ein VOD eine Kandidatenliste als JSON (start_s, end_s, score, signale mit Einzelwerten) und eine lesbare Tabelle auf der Konsole; die 16 VODs unter `/home/nathanael/vod-archive/downloads/` laufen als Testkorpus vollständig durch.
- REQ-02: Signal OCR: Kill-Feed und HUD-Bereich des Deadlock-Bildes werden per Frame-Sampling (Standard 2 Bilder je Sekunde, konfigurierbar) mit tesseract gelesen; erkannt werden eigene Kills, eigener Tod, Multikill-Einblendungen und Objective-Meldungen. Die Bildregionen werden aus echten Frames des Nutzers kalibriert und liegen als Konfigurationsdatei im Repo, nicht hart im Code.
- REQ-03: Signal Sprache: Tonspur wird über den lokalen STT-Server (127.0.0.1:8791, faster-whisper) transkribiert; Signalwerte sind Lautstärke-Spitzen gegen das Stream-Mittel, Lachen und Ausrufe, Schlüsselwörter (konfigurierbare Liste). Transkript mit Wort-Zeitstempeln wird je VOD gespeichert und von Strang B (Untertitel) wiederverwendet.
- REQ-04: Signal Bild: Szenenwechsel-Dichte, Death-Screen (Grau-Overlay) und Template-Treffer für feste Einblendungen; jedes Signal ist einzeln abschaltbar, der Gesamtscore ist eine gewichtete Summe mit Gewichten in der Konfigurationsdatei.
- REQ-05: Messung: die echten Twitch-Clips des Nutzers werden per Helix (`vod_id`, `vod_offset`) im VOD verortet; die Tabelle `twitch_clips_social_media` bekommt dafür die Spalten `vod_id` und `vod_offset_s`. `highlight-detector messen` gibt je VOD Trefferquote (Kandidat überlappt einen echten Clip um höchstens 15 s versetzt) und Kandidaten ohne echten Clip aus.
- REQ-06: Schnitt: jeder Kandidat über der Score-Schwelle wird mit ffmpeg als mp4 geschnitten (Vorlauf 20 s, Nachlauf 10 s, konfigurierbar) und in ein Ausgabeverzeichnis je VOD gelegt, Dateiname trägt Zeitfenster und Score, damit der Nutzer per Sichtprüfung urteilen kann.
- REQ-07: Persistenz: erkannte Kandidaten landen in einer neuen Tabelle `twitch_vod_highlights` (streamer_twitch_id, vod_id, start_s, end_s, score, signale JSONB, status, created_at) in `twitch_analytics`, damit Dashboard und Aufbereitung später darauf aufsetzen.
- REQ-08: Last: ein 4-Stunden-VOD läuft in unter 2 Stunden auf höchstens 8 Kernen mit `nice 15`, damit Bot und Dashboard nicht leiden; Zwischenergebnisse je Signal werden gecacht, ein Wiederholungslauf mit geänderten Gewichten braucht keine neue OCR.

## Invarianten (darf sich nicht ändern)

- INV-01: Kein Code aus `rust/crates/tb-highlight` wird erweitert oder aufgerufen; die Crate bleibt bis zum Nachweis der neuen Erkennung unangetastet (Löschen ist ein eigener Auftrag).
- INV-02: Keine ENV-Dateien, keine Environment-Variablen für Konfiguration; Konfiguration als Datei im Repo, Secrets (DB-DSN, Helix) aus Infisical über den bestehenden Weg.
- INV-03: Kein neues `*_ENABLED`-Flag.
- INV-04: Sprachmodell-Aufrufe, falls nötig, nur über tb-llm mit Deepseek V4 Flash; keine anderen Modelle.
- INV-05: Bestehende Tests werden nicht gelöscht oder abgeschwächt; Migrationen folgen dem Weg aus `Docs/workspace/gates-und-merge.md` (als `postgres` anwenden, Rechte an `twitchbot` und `twitchdash`, `_sqlx_migrations` eintragen).
- INV-06: Keine Identitätsauflösung über Login-Namen; Streamer-Zuordnung über die Twitch-User-ID.
- INV-07: Die VOD-Dateien unter `/home/nathanael/vod-archive/downloads/` werden nicht verändert oder gelöscht; die Pipeline liest nur.

## Nicht-Ziele

- Upload, Layout, Untertitel-Rendering (Strang B, `.tasks/2026-09-06-clip-aufbereitung-social/`).
- Live-Erkennung während des Streams.
- Portierung des Prototyps nach Rust (erst nach Nachweis, eigener Auftrag).
- Löschen von tb-highlight.
- Erkennung aus Deadlock-Demos oder Match-Metadaten.

## Erlaubter Änderungsbereich

- ops/highlight-detector/
- rust/crates/tb-db/migrations/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- rust/.sqlx/
- rust/crates/tb-social-media/src/clip/
- .tasks/2026-09-06-highlight-erkennung-vod/
- docs/funktionsweise/highlight-erkennung.md

## Verbotene Änderungen

- rust/crates/tb-highlight/
- rust/bin/tb-bot/src/main.rs
- Lint- und CI-Konfiguration
- /etc/systemd, Caddyfile
- Alle Dateien unter /home/nathanael/vod-archive/

## Offene Produktfragen

- keine. Entscheidungen des Orchestrators (technisch, reversibel): Prototyp in Python unter `ops/` nach dem Muster von `ops/stt-server`, weil OCR, OpenCV und Whisper-Anbindung dort in Stunden statt Tagen stehen; Start-Gewichte OCR 0,5, Sprache 0,3, Bild 0,2; Score-Schwelle wird aus der Messung gegen echte Clips gesetzt, nicht geraten.

## Amendments

- 2026-09-06: REQ-09 (neu) alt -> neu: kein Lern-Korpus -> `highlight-detector lernen` baut einen Lern-Korpus aus existierenden Twitch-Clips in Masse (alle Partner-Clips aus `twitch_clips_social_media` plus Deadlock-Clips über Helix `GET /clips` nach Views mit dem bestehenden Helix-Client des Repos, Ziel 500 bis 2000 Clips), lädt jeden Clip per yt-dlp, wendet OCR, Sprache und Bild wie in REQ-02 bis REQ-04 an, speichert je Clip eine Merkmalstabelle (welches Signal wann relativ zum Clip-Ende, plus Views, Dauer, Ersteller) in `twitch_clip_merkmale`, erzeugt einen Report "Warum sind gute Clips gut" (Verteilungen, Top-Muster, je Muster echte Beispiel-Clips) und leitet die Score-Gewichte und Schwellen aus REQ-04 aus diesem Korpus ab statt sie zu raten; Clip-Dateien werden nach der Merkmalsextraktion gelöscht. Grund: Lernen aus echten Clips vor dem Bauen des Detektors, entschieden von User.
- 2026-09-06: Erlaubter Änderungsbereich alt -> neu: `rust/crates/tb-db/migrations/` -> `rust/migrations/`. Grund: das Verzeichnis `rust/crates/tb-db/migrations/` existiert nicht, Migrationen liegen im Repo unter `rust/migrations/`, entschieden von Orchestrator.

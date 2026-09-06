# Contract: Clips für Social Media aufbereiten

status: aktiv
datum: 2026-09-06
klasse: hoch
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

Bestand: `.tasks/2026-09-06-clip-aufbereitung-vod/RESEARCH-social-media.md`. Kern:
9:16-Crop läuft, Facecam-Compositing `compose_vertical` ist gebaut und getestet, hat
aber keinen Aufrufer; Untertitel fehlen ganz; Betrieb ist kalt (126 Clips, 0 Downloads,
0 Uploads).

## Ziel

Ein Clip des Nutzers wird zu einem Hochformat-Video, das sein gewähltes Facecam-Layout
trägt, eingebrannte Untertitel hat und im Dashboard als echtes Video vorschaubar ist,
bevor es freigegeben wird.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Das im Dashboard gespeicherte Layout (`social_media_streamer_layout`) wirkt im gerenderten Video: der Upload-Pfad ruft `compose_vertical` mit diesem Layout auf; Center-Crop bleibt nur der Fallback, wenn kein Layout gespeichert ist.
- REQ-02: Neue Layout-Variante "Blur-Rand": das 16:9-Bild sitzt mittig im 9:16-Rahmen, oben und unten füllt eine verschwommene, vergrößerte Kopie des Bildes; im Layout-Editor als dritte Option wählbar.
- REQ-03: Untertitel: das Transkript kommt über den lokalen STT-Server (127.0.0.1:8791) mit Wort-Zeitstempeln und wird als Text unten im Bild eingebrannt (höchstens zwei Zeilen, Gold auf dunklem Balken, Vokabel-Korrektur aus `correction.rs` greift); je Streamer im Dashboard an- und abschaltbar, Standard an. Liegt für den Clip schon ein Transkript aus Strang A vor, wird es wiederverwendet.
- REQ-04: Vorschau: ein Knopf "Vorschau rendern" je Clip erzeugt das fertige Hochformat-Video und zeigt es im Dashboard als abspielbares Video statt nur des Thumbnails; die Freigabe zeigt dasselbe Video.
- REQ-05: Batch für den Nutzer: `tb-social-media` bietet einen Kommandoweg, alle Clips eines Streamers (Twitch-User-ID) nach REQ-01 bis REQ-03 zu rendern und in ein Verzeichnis zu legen, damit der Nutzer seine eigenen Clips gesammelt sichten kann.
- REQ-06: Kalter Betrieb: die Ursache, warum für earlysalty 0 Downloads und 0 Enrichment-Zeilen vorliegen, ist ermittelt und behoben; danach laufen Download und Enrichment für neue Clips des Nutzers automatisch, sichtbar im Dashboard.
- REQ-07: Doku `docs/funktionsweise/social-media-uploads.md` und `docs/architecture/social-media.md` versprechen nur, was der Code tut (Whisper-, Claude-, MiniMax-, Ollama-Nennungen raus, Untertitel erst nach REQ-03 rein).

## Invarianten (darf sich nicht ändern)

- INV-01: Alle Sprachmodell-Aufrufe laufen über tb-llm mit Deepseek V4 Flash; Titel und Hashtags bleiben auf dem bestehenden Weg (`llm_dispatch.rs`, `title_gate.rs`).
- INV-02: Schnittstellen der Uploader (`uploaders/*`) und die Freigabe-Strecke (`approval.rs`) bleiben kompatibel; keine neue Token-Ablage, keine zweite OAuth-Strecke.
- INV-03: Keine ENV-Dateien, keine Environment-Variablen für Konfiguration; kein neues `*_ENABLED`-Flag; Streamer-Schalter leben in den bestehenden `social_media_*`-Tabellen.
- INV-04: Dashboard bleibt in der gemeinsamen Shell und im Gold-Look; keine Standalone-Route.
- INV-05: Bestehende Tests werden nicht gelöscht oder abgeschwächt; Migrationen nach `Docs/workspace/gates-und-merge.md`.
- INV-06: Identitäten über die Twitch-User-ID, nie über Login-Namen.
- INV-07: Gerenderte Dateien werden nach Upload oder Ablauf der bestehenden Retention gelöscht; nichts bleibt dauerhaft auf der Platte.

## Nicht-Ziele

- Highlight-Erkennung aus VODs (Strang A).
- Neue Plattform-Konten (TikTok, Instagram) anlegen oder verifizieren.
- Clip-Automation nach Zeitplan (Roadmap, eigener Auftrag).
- Neue KI-Modelle.

## Erlaubter Änderungsbereich

- rust/crates/tb-social-media/
- rust/crates/tb-dashboard-api/src/handlers/social_media.rs
- rust/crates/tb-dashboard-api/src/routes.rs
- rust/bin/tb-bot/src/wiring.rs
- rust/bin/tb-bot/src/main.rs
- bot/dashboard_v2/src/pages/SocialMedia.tsx
- bot/dashboard_v2/src/pages/SocialMediaAdmin.tsx
- bot/dashboard_v2/src/api/socialMedia.ts
- bot/dashboard_v2/src/components/social-media/
- rust/crates/tb-db/migrations/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- rust/.sqlx/
- docs/funktionsweise/social-media-uploads.md
- docs/architecture/social-media.md
- .tasks/2026-09-06-clip-aufbereitung-social/

## Verbotene Änderungen

- rust/crates/tb-highlight/
- rust/crates/tb-vod-archive/
- rust/crates/tb-llm/
- Lint- und CI-Konfiguration
- /etc/systemd, Caddyfile

## Offene Produktfragen

- keine. Entscheidungen des Orchestrators (technisch, reversibel): Untertitel-Stil Gold auf dunklem Balken nach dem Dashboard-Look; Vorschau-Render läuft als Hintergrundjob mit Statusanzeige, nicht synchron im Request.

## Amendments

- 2026-09-06: Erlaubter Änderungsbereich alt -> neu: `rust/crates/tb-db/migrations/` -> `rust/migrations/`. Grund: das Verzeichnis `rust/crates/tb-db/migrations/` existiert nicht, Migrationen liegen im Repo unter `rust/migrations/`, entschieden von Orchestrator.

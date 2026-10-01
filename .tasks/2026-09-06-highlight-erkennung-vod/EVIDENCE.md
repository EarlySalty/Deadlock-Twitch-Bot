# EVIDENCE: Highlight-Erkennung aus VODs (OCR, Sprache, Bild)

- status: aktiv
- datum: 2026-09-06
- contract: ./CONTRACT.md (inkl. Amendment REQ-09 Lern-Korpus)

Jede Zeile eine echte Fundstelle. Beobachtung, keine Wertung.

## Helix-Client und Clip-Fetch (REQ-05, REQ-09)

- rust/crates/tb-transport-twitch/src/client.rs:88: `HelixConfig::new(client_id, client_secret)` mit `token_url` id.twitch.tv; der geteilte App-Helix-Client des Repos lebt hier, Secrets kommen von aussen in die Config.
- rust/crates/tb-transport-twitch/src/client.rs:123: `AppTokenManager::new(...)` holt und cached das App-Access-Token (Client-Credentials); zentrale Token-Quelle.
- rust/crates/tb-transport-twitch/src/client.rs:191: jeder Request setzt Header `Client-Id` plus Bearer-App-Token; einziger sauberer Helix-Zugang, kein zweiter OAuth-Weg noetig.
- rust/crates/tb-transport-twitch/src/clips.rs:15: `HelixClip`-Struct (id, broadcaster_name, title, duration, game_id); `get_clips_by_broadcaster` sortiert nach Views, aber `vod_offset`/`video_id` werden nicht deserialisiert.
- rust/crates/tb-social-media/src/clip/helix.rs:69: `HelixClipSource` kapselt `Arc<HelixClient>`, `fetch_clips_page` ruft `GET /clips` im 14-Tage-Fenster; der Clip-Fetch nutzt bereits genau diesen Client.
- rust/crates/tb-social-media/src/clip/helix.rs:153: `parse_clip` liest id/url/title/thumbnail_url/created_at/duration/view_count/game_name/game_id und verwirft `vod_offset`/`video_id`, obwohl Helix sie im selben Objekt liefert; genau die zwei Felder fehlen fuer REQ-05.
- rust/crates/tb-social-media/src/clip/repository.rs:108: `INSERT INTO twitch_clips_social_media (clip_id, ..., category_key, status)` ohne `vod_id`/`vod_offset_s`; Insert und Migration muessen um die zwei Spalten wachsen.
- rust/crates/tb-social-media/src/clip/repository.rs:231: Test-DDL derselben Tabelle bestaetigt den aktuellen Spaltensatz (kein vod-Feld).
- rust/bin/tb-bot/src/partner_lookup.rs (is_target_partner): Muster der Identitaetsaufloesung, `WHERE twitch_user_id = $1 OR LOWER(twitch_login) = LOWER($2) AND status='active'`; Vorbild, um `earlysalty` ueber die ID statt Login zu treffen.

## Datenmodell und Migrationen (REQ-05, REQ-07, REQ-09)

- rust/migrations/20260601000000_baseline_schema.sql:631: Basisschema `twitch_clips_social_media` (streamer_login, twitch_user_id, created_at, duration_seconds, view_count, game_name, source_kind, local_file_path); Ziel der Spaltenerweiterung.
- rust/migrations/20260601000000_baseline_schema.sql:1161: `CREATE TABLE public.twitch_partners (... twitch_user_id, twitch_login, status ...)`; aktive Partner fuer den Korpus.
- rust/migrations/20260601000000_baseline_schema.sql:1514: `CREATE TABLE public.twitch_streamers` (PK `twitch_login`, Unique-Index auf `twitch_user_id`); hier wird der Login earlysalty zur Twitch-User-ID aufgeloest.
- rust/migrations/ (Verzeichnis, neueste Datei 20260903090000_twitch_moderation_settings.sql): die realen Migrationen liegen unter `rust/migrations/`, NICHT unter dem im Contract-Scope genannten `rust/crates/tb-db/migrations/` (existiert nicht); Scope-Abweichung, siehe PLAN M1.
- rust/crates/tb-db/tests/fresh_migrations_schema.rs:186: Test `fresh_migrations_match_committed_schema_snapshot` baut das Schema frisch und vergleicht gegen den Snapshot; jede neue Migration muss ihn aktualisieren.
- rust/crates/tb-db/tests/fresh_migrations_schema.rs:58: `write_schema_snapshot` schreibt `tests/fresh_schema_snapshot.txt` neu (Regenerierungsweg bei Schemaaenderung).
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt: committeter Schema-Snapshot (94 KB), liegt im Contract-Scope und aendert sich mit den neuen Tabellen.

## STT / Sprache (REQ-03)

- ops/stt-server/README.md (Abschnitt Betrieb/Verdrahtung): `POST /v1/audio/transcriptions` auf 127.0.0.1:8791, OpenAI-kompatibel, keine Auth, loopback-only; Rust-Default zeigt schon hierher.
- ops/stt-server/stt_server.py:97: `response_format: str = Form(default="verbose_json")`; Antwortformat.
- ops/stt-server/stt_server.py:109: `segments, info = _model.transcribe(...)` OHNE `word_timestamps=True`; der Server liefert Segment-Zeitstempel, keine Wort-Zeitstempel.
- ops/stt-server/stt_server.py:136: `body["segments"] = [...]` je Segment (Start/Ende/Text); das ist die verfuegbare Zeitbasis fuer das Sprach-Signal.
- ops/stt-server/README.md (Abschnitt Modellwahl): large-v3-turbo, 8 Threads, RTF 0,200 bei 20-s-Fenstern gemessen; Grundlage der STT-Laufzeitschaetzung.

## Bild, Schnitt, ffmpeg-Vorbilder (REQ-02, REQ-04, REQ-06)

- rust/crates/tb-highlight/src/config.rs:29: `FFMPEG_PATH = "/usr/bin/ffmpeg"`; fester Binary-Pfad als Vorbild.
- rust/crates/tb-highlight/src/twitch_vod.rs:149: `build_ffmpeg_cmd(raw, compressed)` Reencode-Muster fuer den Clip-Schnitt (Grenzen exakt, nicht keyframe-gebunden).
- rust/crates/tb-highlight/src/twitch_vod.rs:175: `build_yt_dlp_cmd` mit `--download-sections *HH:MM:SS-HH:MM:SS`; laedt nur ein Zeitfenster, ideal fuer den Korpus-Download je Clip (REQ-09).

## VODs, Host, Werkzeuge (REQ-01, REQ-08)

- /home/nathanael/vod-archive/downloads/: 16 VODs 1080p60 mit `.info.json` je Datei (z. B. v2832675815.mp4 16,3 GB); Testkorpus fuer REQ-01, nur lesen.
- /home/nathanael/vod-archive/downloads/v2832675815.info.json: `timestamp` 1785410713 (Wall-Clock-Start), `duration` 19510 s, 1920x1080 @ 60; VOD-Zeitbasis fuer die Clip-Verortung.
- Host (which/version): `/usr/bin/tesseract` = tesseract 5.3.4 (deu/eng), `/usr/bin/ffmpeg` 6.1.1, `~/.local/bin/yt-dlp` 2026.07.04 vorhanden; `python3 -c 'import cv2'` schlaegt fehl, opencv-python muss per pip ins venv.
- ops/stt-server (venv ~/stt-tools, Unit deadlock-stt-server.service): Muster fuer den Python-Prototyp, eigenes venv, keine System-Pakete ausser tesseract/ffmpeg, Config ueber os.environ mit Defaults.
- /home/nathanael/vod-archive/config.env (Modus 0600): lokaler Ablageort fuer den DB-DSN im Nachbar-Repo; Vorbild fuer den read-only-DB-Zugang des Detektors ohne Secret im Repo.

---
status: aktiv
datum: 2026-09-06
thema: Bestandsaufnahme Auto-Clipper im Twitch-Bot (Grundlage VOD-Durchsicht + Clip-Entstehung)
methode: graphify query zuerst, dann Quelle gelesen, grep nur zum Verifizieren
---

# Auto-Clipper Bestandsaufnahme

## TLDR (Beobachtung)

Es gibt zwei getrennte, real gebaute Wege, plus einen Zuschauer-Befehl:

1. **tb-highlight** ("Highlight-Clipper"): erkennt Highlight-Momente aus **Deadlock-Match-Demos** (boon-Parser) und ersatzweise aus **deadlock-api-Match-Metadaten**, rechnet aus Match-Start und Twitch-VOD-Start einen VOD-Offset, schneidet mit yt-dlp und ffmpeg ein Clip-Fenster aus dem Twitch-Archiv-VOD und postet die mp4 an einen lokalen Highlight-Discord-Relay. Erkennt **nur Kill-basierte Ereignisse** (Multikill, Teamfight, Close-Fight, Solo-Clutch). **Kein OCR, keine Bilderkennung, keine Chat-Spikes.** Im Bot verdrahtet, aber standardmaessig **AUS** (Opt-in `TB_HIGHLIGHT_CLIPPER_ENABLED`).
2. **tb-social-media Clip-Fetch**: holt periodisch die **bereits existierenden** Twitch-Clips der Partner ueber Helix `GET /clips` und legt sie in `twitch_clips_social_media` fuer die Social-Media-Pipeline ab. Erstellt selbst keine Clips, sammelt nur. Opt-in `TB_CLIP_FETCHER_ENABLED`, fuer earlysalty **live gelaufen** (886 Fetch-Laeufe, 25 Clips in der DB).
3. **`!clip`**: Zuschauer-Befehl, ruft Helix `POST /clips`, erzeugt einen nativen Twitch-Clip (Twitch bestimmt die Laenge, rund 30 s), gibt die `clips.twitch.tv`-URL zurueck.

Fuer die zwei Fragen: (a) Das VOD-Durchgehen mit Clip-Erkennung ist in tb-highlight vom Prinzip her da (Match-Zeit -> VOD-Offset -> ffmpeg-Schnitt), aber nur kill-getrieben und ohne Persistenz. (b) Wann Clips entstehen, laesst sich aus `twitch_clips_social_media.created_at` und `clip_fetch_history` lesen, nicht aus einer Highlight-Tabelle (die gibt es nicht).

---

## 1) tb-highlight (der eigentliche Auto-Clipper)

### Erkannte Ereignisse und Quelle (Beobachtung)

- Modul-Doku: Pipeline laedt Deadlock-Match-Demos, erkennt Multikills/Teamfights/Clutches ueber das externe `boon`-Binary, schneidet Clips aus dem Twitch-VOD per ffmpeg. `rust/crates/tb-highlight/src/lib.rs:1`
- Ereignistypen: `EventType::Multikill | Teamfight | CloseFight`. `rust/crates/tb-highlight/src/event_detector.rs:18`
- API-Pfad `detect_events` liest Kills/Tode aus `match_info["players"]` der deadlock-api. `rust/crates/tb-highlight/src/event_detector.rs:57`
- Demo-Pfad `detect_all_events` liest Kills, Ability-Casts und Health direkt aus dem Replay ueber boon. `rust/crates/tb-highlight/src/demo_analyzer.rs:96`
- `KillMoment` bewertet Health-Prozent (Clutch bei `health_pct < 0.35`) plus Combo/High-Impact-Abilities. `rust/crates/tb-highlight/src/demo_analyzer.rs:39`, Scoring `:52`, High-Impact-Liste `:21`
- `moments_to_events`: Multikill ab 2 Kills in 15 s und Solo-Clutch mit `excitement_score >= max(min_score, 1)`. `rust/crates/tb-highlight/src/demo_analyzer.rs:179`
- boon ist ein Subprocess auf ein vorkompiliertes Source-2-Demo-Parser-Binary (`tools/boon`), Tickrate 64, Timeout 60 s; Fehler degradieren auf leer. `rust/crates/tb-highlight/src/boon.rs:33`, TICK_RATE `:19`, Timeout `:16`
- Quelle der Match-Liste/Metadaten ist die oeffentliche deadlock-api.com: `get_match_history` (`/players/{id}/match-history`) und `get_match_metadata` (`/matches/{id}/metadata`). `rust/crates/tb-highlight/src/deadlock_client.rs:47`, `:58`

Nur Kills. **Keine** Fail-/Funny-Erkennung, **kein** OCR (siehe Abschnitt 3).

### Schwellen und Limits (Beobachtung)

- Poll-Intervall des Worker-Loops: 600 s. `rust/crates/tb-highlight/src/config.rs:11`
- Clip-Vorlauf 6 s, Nachlauf 4 s, max. Cliplaenge 40 s, Padding 10 s. `rust/crates/tb-highlight/src/config.rs:13`,`:15`,`:17`,`:19`
- Multikill ab 2 Kills; Teamfight ab 4 Toden, verkettet innerhalb 15 s. `rust/crates/tb-highlight/src/config.rs:21`,`:25`,`:27`
- Close-Fight-Fenster 40 s, dynamischer Pre-Roll gedeckelt auf 35 s. `rust/crates/tb-highlight/src/event_detector.rs:13`,`:14`
- Match muss juenger als 24 h und noch nicht verarbeitet sein. `rust/crates/tb-highlight/src/worker.rs:36` (Filter `now - 86400`, `:42`)
- Match-History-Abruf mit Limit 10, Archiv-VOD-Abruf mit 20. `rust/crates/tb-highlight/src/worker.rs:80`, `rust/crates/tb-highlight/src/twitch_vod.rs:59`
- Discord-Dateigrenze 24 MB (danach ffmpeg-Reencode/Groessencheck). `rust/crates/tb-highlight/src/config.rs:31`, Pipeline-Doku `twitch_vod.rs:1`

### Ablauf, Match-Zeit zu VOD-Offset (Beobachtung)

- `run_once` laeuft ueber alle aktiven Partner. `rust/crates/tb-highlight/src/worker.rs:148`
- Partner-Aufloesung aus drei Quellen: Postgres `twitch_streamers_partner_state`, `core.steam_links`, manuelle `data/highlight_clipper/steamids.json`. `rust/crates/tb-highlight/src/partners.rs:1`, Default-Pfad `:17`
- `process_streamer`: Match-History holen, `filter_recent_matches`, Twitch-Channel-ID aufloesen. `rust/crates/tb-highlight/src/worker.rs:171`
- `process_match`: Metadaten holen, Demo-First-Erkennung, sonst API-Fallback, dann `find_vod_for_match`. `rust/crates/tb-highlight/src/worker.rs:216`
- **VOD-Offset = Match-Start minus VOD-Start**: `let vod_offset_s = m.start_time - vod.vod_started_at;`. `rust/crates/tb-highlight/src/worker.rs:307`
- Der VOD-Start kommt aus dem Twitch-Archiv-VOD (`created_at`), nicht aus einer Session-Tabelle. VOD-Auswahl deckt das Match-Zeitfenster ab. `rust/crates/tb-highlight/src/twitch_vod.rs:65`, Zeit-Parser `:289`,`:298`
- `compute_clip_window(vod_offset, event)`: `clip_start = vod_offset + game_time - PRE_ROLL`, Ende gedeckelt auf `MAX_CLIP_SECONDS`. `rust/crates/tb-highlight/src/worker.rs:82`
- Schnitt ueber yt-dlp Download-Section, dann ffmpeg. `rust/crates/tb-highlight/src/worker.rs:319`, `download_clip`-Doku `twitch_vod.rs:1`
- Versand der fertigen mp4 an lokalen Relay `http://127.0.0.1:8899/highlight-clips`, der in den Highlight-Discord-Channel postet; best-effort, Fehler verschluckt. `rust/crates/tb-highlight/src/worker.rs:337`, `highlight_sender.rs:15`, Doku `highlight_sender.rs:1`
- Zustand (verarbeitete Matches) liegt als JSON-Datei `data/highlight_clipper/state.json`, nicht in der DB. `rust/crates/tb-highlight/src/config.rs:7`, `state.rs` (`is_match_processed`/`mark_match_processed`)
- Temp-Clip-Verzeichnisse werden nach dem Post geloescht. `rust/crates/tb-highlight/src/worker.rs:107`

### Ist es verdrahtet und live? (Beobachtung)

- Cargo-Dep vorhanden. `rust/bin/tb-bot/Cargo.toml:39`
- Worker wird gespawnt, aber hinter Opt-in `TB_HIGHLIGHT_CLIPPER_ENABLED`; Kommentar sagt "standardmaessig AUS". `rust/bin/tb-bot/src/main.rs:1560`,`:1562`,`:1576`; boon-Pfad `cwd/tools/boon` `:1566`; Loop mit `POLL_INTERVAL_SECONDS` `:1580`; Aus-Log `:1589`
- Twitch-Zugriff ueber `HelixVodSource`, das `TwitchVodApi` implementiert. `rust/bin/tb-bot/src/wiring.rs:416`, Trait `rust/crates/tb-highlight/src/twitch_vod.rs:21`
- Keine DB-Tabelle mit erkannten Momenten oder VOD-Offsets. Nichts in der DB belegt Live-Laeufe dieses Pfades (State ist eine Datei). **(Vermutung: aktuell aus.)**

---

## 2) tb-social-media Clip-Fetch (sammelt existierende Twitch-Clips)

### Ablauf (Beobachtung)

- Hintergrund-Task `ClipFetchTask`: Intervall 6 h, Initial-Delay 60 s, Loop. `rust/crates/tb-social-media/src/clip/task.rs:7`,`:10`,`:70`
- Pro Streamer max. 20 Clips, Helix `GET /clips`, Rate-Limit-Pause zwischen Partnern. `rust/crates/tb-social-media/src/clip/service.rs:26`,`:69`,`:149`; Helix-Fetch `clip/helix.rs:69`,`:102`
- Schreibt Fetch-Protokoll in `clip_fetch_history` und neue Clips in `twitch_clips_social_media`. `rust/crates/tb-social-media/src/clip/repository.rs:166`, INSERT-Query `rust/.sqlx/query-766601...json:3`
- Verdrahtet, aber Opt-in `TB_CLIP_FETCHER_ENABLED`. `rust/bin/tb-bot/src/main.rs:1817`,`:1819`,`:1825`

### Zielschema (Beobachtung)

- `twitch_clips_social_media` Spalten: `clip_id, clip_url, clip_title, streamer_login, twitch_user_id, created_at (text, Twitch-Zeitstempel), duration_seconds, view_count, game_name, status, source_kind (twitch|manual_upload), local_file_path, retention_until, discarded_at`. `rust/migrations/20260601000000_baseline_schema.sql:631`
- **Keine `vod_offset`-Spalte, keine `game_time`-Spalte.** Clips referenzieren nur die Twitch-clip_id/URL.
- Kontingent-Spalte `kontingent_verbraucht_at` per Migration nachgezogen (nur fuer manual_upload gesetzt, gefetchte bleiben NULL). `rust/migrations/20260823140000_clip_kontingent_verbrauch.sql:30`,`:37`

### DB-Bestand (SELECT, twitch_analytics)

- `twitch_clips_social_media` gesamt: **126** Zeilen.
- **earlysalty: 25 Clips**, alle `source_kind='twitch'`, `created_at` von **2026-08-02 bis 2026-09-05**; Dauer 23,1 bis 60 s (Schnitt 32,8 s); `view_count`-Summe 46; Status 24 `awaiting_approval`, 1 `approved`.
- `clip_fetch_history` fuer earlysalty: **886 Fetch-Laeufe** (Spalten `streamer_login, fetched_at, clips_found, clips_new, fetch_duration_ms, error, twitch_user_id`). Belegt: der Fetcher lief live.
- Top-Streamer nach Clip-Zahl: albiiionlu 35, earlysalty 25, whysolowkey 13.
- Vorhandene Clip-/Highlight-Tabellen in `twitch_analytics`: `clip_fetch_history, clip_last_hashtags, clip_templates_global, clip_templates_streamer, social_media_clip_approval, social_media_clip_enrichment, twitch_clip_form_submissions, twitch_clips_social_analytics, twitch_clips_social_media, twitch_clips_upload_queue`. **Keine Highlight-/Auto-Clip-Momenttabelle.**

---

## 3) `!clip` (Zuschauer-Clip) und Fehlanzeige OCR/Chat

### `!clip` (Beobachtung)

- Katalog-Eintrag `!clip`. `rust/crates/tb-chat/src/catalog.rs:101`
- Handler `cmd_clip`: Kanal-Cooldown 10 s, Dashboard-Toggle `streamer_plans.clip_command_enabled`, Partner-Pflicht, Titel-Fallbacks. `rust/crates/tb-chat/src/commands.rs:1329`, Cooldown-Konstante `:50`, Toggle-SQL `:1353`, Fallbacks `:66`
- Clip-Erstellung ueber Helix `POST /clips`, Rueckgabe `https://clips.twitch.tv/{id}`. `rust/bin/tb-bot/src/chat_wiring.rs:113`,`:128`,`:164`
- **Laenge**: keine explizite Laengenangabe im Code; Twitch bestimmt sie beim nativen Clip (rund letzte 30 s). `POST /clips` ohne Dauerparameter.
- `rust/crates/tb-transport-twitch/src/clips.rs` ist nur der **Lese**-Pfad `GET /clips` (Fetch), nicht die Erstellung. `:1`,`:53`

### OCR / Browser-Automation / Chat-Spikes (Fehlanzeige, Beobachtung)

- Grep ueber `rust/` nach `ocr|tesseract|screenshot|headless|playwright|chat.spike.*clip|emote.spike.*clip` liefert **keine** Clip-relevanten Treffer; die `headless`-Treffer sind der Discord-Noop-Backend und ein Dashboard-Forwarding, ohne Clip-Bezug. `rust/crates/tb-transport-discord/src/noop.rs:11`, `rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:11`
- `maybe_send_viewer_spike_promo` ist ein Werbe-Trigger in der Promo-Engine, kein Clip. `rust/crates/tb-chat/src/promos.rs:1530`
- Die Memory-Notiz "Erkennung ueber OCR und Browser-Automation" ist in **diesem** Repo **nicht** umgesetzt. **(Vermutung: nur Konzept/andere Baustelle.)**

---

## Wiederverwendbar

Fuer (a) VODs durchgehen mit Clip-Erkennung:
- Komplette Match-zu-VOD-Bruecke in tb-highlight: `find_vod_for_match`/`select_vod_for_match` (VOD, das ein Zeitfenster abdeckt), `vod_offset_s = match_start - vod_started_at`, `compute_clip_window`, `download_clip` (yt-dlp Section + ffmpeg-Reencode + Groessencheck). `worker.rs:82`,`:216`,`:307`; `twitch_vod.rs:53`,`:65`
- `TwitchVodApi`-Abstraktion plus `HelixVodSource` als sauberer Twitch-Zugang ohne Helix-Kopplung in der Crate. `twitch_vod.rs:21`, `wiring.rs:416`
- Deterministische, I/O-freie Event-Erkennung (`event_detector.rs`) und Demo-Scoring mit Clutch/Combo/High-Impact (`demo_analyzer.rs`) als fertige, testbare Bausteine.
- boon-Wrapper als vorhandener Demo-Parser-Zugang (Kills, Abilities, Health, Ticks). `boon.rs`

Fuer (b) verstehen, wann Clips entstehen:
- `twitch_clips_social_media.created_at` (Twitch-Erstellzeit), `duration_seconds`, `view_count`, `game_name`, `source_kind` liegen je Clip vor. `baseline_schema.sql:631`
- `clip_fetch_history` liefert Zeitreihe der Fetch-Laeufe (`fetched_at, clips_found, clips_new`), fuer earlysalty 886 Laeufe. Direkt auswertbar.
- Der Clip-Fetch-Task (6 h, 20/Streamer) als bestehende Erntemaschine fuer existierende Clips. `clip/task.rs`, `clip/service.rs`

## Fehlt

- **Keine Zuordnung Ereignis-Zeit zu `twitch_stream_sessions`/Stream-Start.** Der VOD-Offset kommt allein aus dem Twitch-Archiv-VOD `created_at`. Fehlt ein Archiv-VOD (VODs deaktiviert oder abgelaufen), gibt es keinen Offset und keinen Clip. `worker.rs:307`, `twitch_vod.rs:65`
- **Keine Persistenz erkannter Highlights/Offsets.** tb-highlight schreibt nur eine JSON-Statusdatei und postet mp4 nach Discord; nichts ist spaeter fuer eine VOD-Durchsicht durchsuchbar. `state.rs`, `worker.rs:337`
- **Keine `vod_offset`/`game_time`-Spalte in `twitch_clips_social_media`.** Erkannte Momente und Clip-Datensaetze sind nicht verknuepfbar. `baseline_schema.sql:631`
- **Nur Kill-basierte Erkennung.** Keine Fails, keine lustigen Momente, kein OCR, keine Bilderkennung, keine Chat-/Emote-Spikes. Grep-Fehlanzeige im gesamten `rust/`.
- **Beide automatischen Pfade sind Opt-in.** Highlight-Clipper standardmaessig aus (`TB_HIGHLIGHT_CLIPPER_ENABLED`), Clip-Fetch aus (`TB_CLIP_FETCHER_ENABLED`, fuer earlysalty aber nachweislich gelaufen). `main.rs:1562`,`:1817`
- **`!clip` erzeugt nur native Twitch-Clips** (Twitch-Laenge, kein eigenes Zeitfenster, keine KI-Titel, kein Upload). `chat_wiring.rs:128`

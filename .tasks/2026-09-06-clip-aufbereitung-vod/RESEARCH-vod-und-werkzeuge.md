---
status: aktiv
datum: 2026-09-06
thema: VODs des Nutzers earlysalty als Datei beschaffen und nach Clip-Momenten durchsuchen; Bestand an Video-Analyse-Werkzeugen auf dem Host
methode: read-only, nichts geaendert, nichts committed
---

# Kernbefund (TLDR)

- Es liegen bereits **16 vollstaendige earlysalty-VODs als 1080p60-mp4 lokal auf der Platte** (114 GB unter `/home/nathanael/vod-archive/downloads/`), jeweils mit `.info.json`-Seitendatei. Kein Download noetig, um sofort zu durchsuchen.
- Der Download-Pfad ist ein **eigenstaendiges Repo** `/home/nathanael/vod-archive` (Rust, per Timer, laedt via yt-dlp in voller Aufloesung). Daneben existiert die Crate `tb-vod-archive` im Twitch-Bot als zweite, parallele Implementierung derselben Idee.
- Werkzeuge da: ffmpeg 6.1.1, ffprobe, yt-dlp 2026.07.04, lokaler STT-Server (faster-whisper large-v3-turbo) auf 127.0.0.1:8791. **Kein tesseract (OCR), kein streamlink, keine GPU.**
- Deadlock-Match-Zeitleiste (Kills/Deaths/Objectives mit `game_time_s`) ist beschaffbar; earlysalty Steam-ID ist bekannt (76561199026913891).

Beobachtung vs. Vermutung ist unten je Punkt getrennt.

---

# 1. VOD-Download-Pfad im Code

## 1a. Zweite, tatsaechlich laufende Implementierung: eigenstaendiges Repo `/home/nathanael/vod-archive`

Beobachtung. Dieses Repo ist NICHT die Crate im Twitch-Bot, sondern ein separates Rust-Projekt mit eigenem Git, eigener SQLite-State-DB und eigenem systemd-Timer.

- Zweck laut README: `Holt neue Twitch-VODs von earlysalty, legt sie lokal ab und laedt sie auf YouTube hoch.` (`/home/nathanael/vod-archive/README.md:3`)
- Aufbau: `src/twitch.rs yt-dlp und ffmpeg`, `src/db.rs SQLite-Zustand`, `state/vods.db`, `downloads/ lokale Kopien` (`/home/nathanael/vod-archive/README.md`, Abschnitt Aufbau).
- Dienst und Timer: `Unit=vod-archive.service`, `OnCalendar=*-*-* 05:00:00` und `13:00:00`, `User=nathanael`, `WorkingDirectory=/home/nathanael/vod-archive`, `EnvironmentFile=/home/nathanael/vod-archive/config.env`, `Nice=15 IOSchedulingClass=idle` (`/etc/systemd/system/vod-archive.service`, `/etc/systemd/system/vod-archive.timer`).
- Verhalten laut README: VOD-Liste kommt ueber yt-dlp ohne Twitch-API-Zugangsdaten; laufender Stream wird uebersprungen; ohne YouTube-Login nur lokal archiviert; Uploads byte-genau fortgesetzt; VODs ueber 11,5 h verlustfrei geschnitten; `KEEP_LOCAL_DAYS`-Default so, dass lokale Dateien liegen bleiben, bis der Wert ueber 0 gesetzt wird (`/home/nathanael/vod-archive/README.md`, Abschnitt Verhalten).

Belegte Dateien auf Platte (Befehl `du -sh downloads; ls downloads`):
```
114G    downloads
16 mp4, 38 info.json, 2 .part, 2 .ytdl
gesamt mp4-bytes = 122.038.065.369
groesste: v2832851252.mp4 20,7 GB ; v2855186418.mp4 18,7 GB
```
Aufloesung geprueft (`ffprobe v2862489221.mp4`): `codec_name=h264 width=1920 height=1080 r_frame_rate=60/1`. Also **1080p60, voller Ton, kein Reencode**.

`.info.json` traegt die Metadaten (Befehl auf eine Datei):
`{'id':'v2832675815','title':'Goon Deadlock Session mit Mo','duration':19510,'timestamp':1785410713,'upload_date':'20260730','width':1920,'height':1080,'resolution':'1920x1080'}`

Hinweis Beobachtung: `config.env` laeuft ueber `EnvironmentFile` und widerspricht der Workspace-Regel keine ENV-Dateien fuer Config. Nicht angefasst, nur vermerkt.

## 1b. Crate `tb-vod-archive` im Twitch-Bot (zweite Quelle desselben Musters)

Beobachtung. Reiner yt-dlp/ffmpeg-Weg, kein Twitch-API-Kontingent.

- Modulkopf: `Alles laeuft ueber yt-dlp und ffmpeg statt ueber die Twitch-API` (`rust/crates/tb-vod-archive/src/twitch.rs:3`).
- VOD-Liste je Kanal: `liste_vods()` ruft yt-dlp auf `https://www.twitch.tv/{kanal}/videos?filter=archives` (`rust/crates/tb-vod-archive/src/twitch.rs:184`, URL bei `:193`), parst die Playlist in `parse_vod_liste()` (`:163`).
- Voll-Download-Argumente: `download_args()` (`rust/crates/tb-vod-archive/src/twitch.rs:216`): `--retries 10 --fragment-retries 20 --concurrent-fragments 4 --merge-output-format mp4 --write-info-json`, optionale `--limit-rate`, Ausgabe `{twitch_id}.%(ext)s`. **Kein `-f`/Format-Filter, also yt-dlp-Default = beste verfuegbare Aufloesung.**
- Download selbst: `lade_vod()` (`:248`), Timeout `cfg.download_timeout`. Zielverzeichnis je Streamer.
- Verlustfreier 12-h-Schnitt fuer YouTube: `schneide_bei_bedarf()` (`:370`), `schnitt_args()` (`:337`), Grenze `MAX_PART_SECONDS = 11h30m` (`rust/crates/tb-vod-archive/src/config.rs:15`).
- Laenge messen: `miss_laenge()` via ffprobe (`:316`).

Config-Defaults (`rust/crates/tb-vod-archive/src/config.rs:52-69`):
```
download_dir = data/vod-archive
max_downloads_per_run = 6
max_uploads_per_run = 2
min_free_gb = 80
keep_local_days = 0   // 0 heisst: nie loeschen, lokales Archiv ist der Verlustschutz
download_timeout = 21600 s (6 h)
interval = 12 h
```
Loeschregeln: `platz_reicht()` prueft `freier_platz_gb() >= min_free_gb` vor jedem Lauf (`rust/crates/tb-vod-archive/src/worker.rs:804`, Aufruf `:283` und `:350`); `raeume_auf()` loescht nur Dateien, deren `keep_local_days`-Frist abgelaufen ist (`worker.rs:784`), physisch via `loesche_dateien()` -> `std::fs::remove_file` (`worker.rs:871-878`). Bei `keep_local_days=0` wird nie geloescht.

Plattenplatz-Grenze: `freier_platz_gb()` liest `df` und vergleicht gegen `min_free_gb` (`worker.rs:849`, `parse_df` `:860`). Bei zu wenig Platz setzt der Lauf aus (`worker.rs:284-286`).

DB-Zustand dieser Crate: Tabelle `twitch_vod_archive_vods` und `twitch_vod_archive_parts` in `twitch_analytics` (`rust/crates/tb-vod-archive/src/store.rs:92,211,459,522`). Welche Kanaele archiviert werden, steht je Streamer in `social_media_vod_archive` (`rust/crates/tb-vod-archive/src/config.rs:1-8`; Settings-Quelle `rust/crates/tb-social-media/src/vod_archive.rs`, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs`).

## 1c. Partieller Download eines Zeitfensters (fuer Analyse ideal): tb-highlight

Beobachtung. `tb-highlight` laedt gezielt nur einen Zeitabschnitt eines VODs, statt das ganze VOD zu ziehen.

- `download_clip()` (`rust/crates/tb-highlight/src/twitch_vod.rs:98`): yt-dlp Download-Section -> ffmpeg-Reencode.
- `build_yt_dlp_cmd()` (`:175`): `--download-sections *HH:MM:SS-HH:MM:SS`, `--merge-output-format mp4`, `-f bestvideo[height<=720]+bestaudio/bestvideo+bestaudio/best` (`:197` im Body).
- `build_ffmpeg_cmd()` (`:203`): `scale=-2:720 libx264 crf28 preset fast aac 96k`.
- VOD-Auswahl per Helix: Trait `TwitchVodApi` mit `get_user_info`/`get_archive_videos` (`:23-25`), `get_channel_id()` (`:37`), `find_vod_for_match()` (`:53`), `select_vod_for_match()` (`:65`). Das ist die einzige Helix-videos-Auflistung im Code (der VOD-Archiv-Weg nutzt bewusst kein Helix).

## Twitch-VOD-Listing im Code

Beobachtung. Zwei Wege:
- yt-dlp-Playlist ohne Twitch-API: `liste_vods()` (`tb-vod-archive/src/twitch.rs:184`) und das eigenstaendige Repo.
- Helix `get_archive_videos` (`tb-highlight/src/twitch_vod.rs:25`).

Live-Gegenprobe (Befehl `yt-dlp --flat-playlist --print id .../earlysalty/videos?filter=archives`) liefert aktuell 20+ Archiv-VODs, u. a. `v2866130464 v2866074657 v2864193655 ... v2858159248`. Also **jedes derzeit auf Twitch liegende VOD ist ohne Zugangsdaten sofort ziehbar.**

---

# 2. Host-Werkzeuge und Hardware

Beobachtung (Befehl `which`/`--version`, `nproc`, `free`, `df`, `curl health`):
```
ffmpeg   /usr/bin/ffmpeg        6.1.1-3ubuntu5
ffprobe  /usr/bin/ffprobe
yt-dlp   /home/nathanael/.local/bin/yt-dlp   2026.07.04
tesseract  MISSING
whisper    MISSING (CLI)
streamlink MISSING
nvidia-smi MISSING, lspci VGA leer  -> keine GPU
CPU-Kerne 16 ; RAM 48 GiB (aktuell ~19 GiB frei)
Platte /  1,7T, 392G frei (76% belegt) ; /tmp gleiche Partition
/opt/deadlock liegt auf derselben Partition
```

STT-Server (Beobachtung):
- Laeuft: `ss` zeigt `127.0.0.1:8791` LISTEN (python, pid 2488363).
- Health: `{"status":"ok","model":"deepdml/faster-whisper-large-v3-turbo-ct2","threads":8}`.
- API: OpenAI-kompatibel, `POST /v1/audio/transcriptions`, keine Auth, loopback-only. Rust-Aufrufer zeigt per Default hierher (`ops/stt-server/README.md`, Abschnitte Betrieb und Verdrahtung).
- Gemessene Leistung ohne GPU (README, Modellwahl): large-v3-turbo bei 20-s-Fenstern Median 3,97 s, RTF 0,200; tiny fuer Deadlock-Vokabular unbrauchbar.
- Code: `stt_server.py` (6,3 KB), venv `~/stt-tools`, User-Unit `deadlock-stt-server.service`.

Bewertung: Audio/Transkript-Analyse (Reaktionen, lustige Momente ueber Sprache) ist voll ausgestattet. **Bild-/OCR-basierte Erkennung (Kill-Feed, HUD-Text) fehlt komplett: kein tesseract, keine GPU.** ffmpeg kann Frames extrahieren, aber ein OCR-Werkzeug muesste erst installiert werden.

---

# 3. Browser-Automation im Workspace

Beobachtung. Im Twitch-Bot-Repo gibt es keine echte Video-/Clip-Browser-Automation. Der Graphify-Treffer zu Browser/Automation zeigt nur:
- `website/src/components/ui/BrowserMockup.tsx` (reine UI-Attrappe).
- `rust/crates/tb-transport-discord/src/noop.rs:11 HeadlessNoop` (Transport-Stub, kein Browser).
- OAuth-Scope-Doku und Spam-Doku, kein Automations-Code.

Vermutung. Die in AGENTS.md erwaehnte vorhandene Browser-Automation fuer die Clip-Erkennung liegt nicht im Twitch-Bot-Repo, sondern in einem anderen Repo (Kandidaten: Social-Media-Uploader oder ein Auto-Clipper). Nicht in diesem Read-only-Lauf verifiziert; als offener Punkt markiert.

---

# 4. Twitch-Seite: VOD-Verfuegbarkeit, Sessions, Clips (DB twitch_analytics)

Zugriff read-only ueber DSN aus `python3 /home/nathanael/Documents/Infisical/export_gpt_secret.py --secret DEADLOCK_CENTRAL_DSN` (Wert nicht ausgegeben), `\c twitch_analytics`. Der offiziell genannte Pfad `~/Documents/claude-config/bin/export_gpt_secret.py` existiert nicht; die reale Datei liegt unter `Documents/Infisical/`.

Beobachtung. earlysalty = `twitch_user_id 1186925760` (aus `twitch_stream_sessions`).

Sessions (SELECT):
```
letzte 60 Tage: 62 Sessions, 123,6 Stunden, 2026-07-08 bis 2026-09-05
gesamt: 146 Sessions
```

VOD-Archiv-Zeilen fuer earlysalty (`twitch_vod_archive_vods`):
```
uploaded         13  (local_path gesetzt)
archived         22  (kein local_path)
upload_failed     2  (local_path gesetzt)
download_failed   1
```
Die in der DB genannten Pfade sind relativ (`data/vod-archive/earlysalty/vXXXX.mp4`) und existieren am erwarteten Ort NICHT; die real vorhandenen Dateien liegen im eigenstaendigen Repo unter `/home/nathanael/vod-archive/downloads/vXXXX.mp4` (Abschnitt 1a). Also: diese Postgres-Tabelle und der lokale Dateibestand stammen aus zwei verschiedenen Implementierungen.

Clips (`twitch_clips_social_media`):
```
25 Clips, 2026-08-02 bis 2026-09-05, davon mit local_file_path: 0
```
Wichtig: Die Tabelle hat **keine `vod_offset`-Spalte** (Spaltenliste geprueft). Sie speichert `clip_id, clip_url, created_at, duration_seconds, view_count, game_name, status, ...`, aber keinen VOD-Zeitversatz. Eine Zuordnung Clip -> Position im VOD ist aus dieser DB direkt nicht moeglich; `vod_offset` muesste ueber Helix (`GET clips` liefert `vod_offset`) nachgezogen werden.

Transkripte: `twitch_engagement_stream_transcripts` hat fuer earlysalty **0 Zeilen** (Spalte `channel_login`). Es gibt also noch keine gespeicherten Transkripte des Nutzers, obwohl der STT-Server laeuft.

VOD-Aufbewahrung auf Twitch:
- Beobachtung: Live-yt-dlp-Liste zeigt derzeit 20+ Archiv-VODs; der lokale Bestand reicht bis 2026-07-30 zurueck (aus Archiv-Kopien, nicht mehr zwingend live).
- Vermutung: Twitch-Plattformregel ist Affiliate/regulaer 14 Tage, Partner/Prime/Turbo 60 Tage. Der Partner-Status von earlysalty wurde hier nicht abgefragt. Fuer die Aufgabe unkritisch, weil 16 Voll-VODs bereits lokal liegen und jedes aktuell gelistete VOD per yt-dlp sofort ziehbar ist.

---

# 5. Deadlock-Match-Daten: Steam-ID und Ereignis-Zeitleiste

Beobachtung. earlysalty ist in `core.steam_links` verknuepft (DB `deadlock`, Suche ueber `steam_display_name ILIKE 'EarlySalty'`):
```
steam_id (steamid64) = 76561199026913891
deadlock_rank_name = Emissary
deadlock_rank_match_id = 103014677   (konkrete Match-ID als Beispiel)
is_steam_friend = t
```
`core.steam_links` ist nach `discord_id` verschluesselt, nicht nach Twitch-Identitaet; die Bruecke Twitch->Steam laeuft ueber Discord bzw. den Anzeigenamen.

Match-Liste je Spieler: Beobachtung. Der Steam-Bot holt Match-History ueber den Game Coordinator: `CMsgClientToGcGetMatchHistory` Request/Response in `rust/crates/steam-core/src/task/handlers/gc.rs:415-426`, Paging in `should_stop_match_history_paging()` (`gc.rs:559`).

Ereignis-Zeitleiste eines Matches: Beobachtung. Der Steam-Bot kann die Match-Metadaten schon dekomprimieren und dekodieren:
- `CMsgClientToGcGetMatchMetaData` Request/Response, liefert `replay_salt`/`metadata_salt` (`rust/crates/steam-core/src/task/handlers/gc.rs:883-898`, auch `lobby.rs:689`).
- `steam-flows/src/native_rank.rs:134-146` dekodiert `CMsgMatchMetaData` -> `CMsgMatchMetaDataContents` -> `match_info.players` (aktuell nur fuer den Rang genutzt).

Der Proto-Inhalt traegt die volle Zeitleiste (`.../deadlock-proto/protos/deadlock/citadel_gcmessages_common.proto`):
```
message CMsgMatchMetaDataContents (L475)
  message Deaths { optional uint32 game_time_s = 1; ... }  (L482)
  weitere game_time_s-Ereignisse bei L492, L504 (Objektiv-/Ward-artig)
  message Players { repeated Deaths death_details = 3; kills=8; assists=10; net_worth=11; hero_id=12; abandon_match_time_s=23 }  (L606)
  je Spieler: bullet_kills/melee_kills/ability_kills/headshot_kills (L579-582), damage-Felder
```
Also: **Deaths mit `game_time_s`, per-Spieler death_details, plus Kill-/Objective-/Damage-Aggregate** sind vorhanden und dekodierbar.

Alternativer Weg (Beobachtung, im Workspace bereits genutzt): `https://api.deadlock-api.com/v1` (`Deadlock-Bots/rust/crates/dl-tierlist/src/lib.rs:30`, `Deadlock-Bots/rust/crates/dl-brain/src/api_ingest.rs:11-12`, `Deadlock-Brain/.../api.rs:8`). Die deadlock-api bietet `/v1/matches/{id}/metadata` als bereits dekodiertes JSON derselben Proto-Struktur, also eine Zeitleiste ohne eigenen GC-Aufruf. Vermutung: kostet keinen Bot-GC-Aufruf und umgeht das Rate-Limit; nicht in diesem Lauf getestet.

---

# Wiederverwendbar (ohne Neubau nutzbar)

1. **16 fertige 1080p60-VODs (114 GB) unter `/home/nathanael/vod-archive/downloads/`** mit `.info.json` je Datei. Sofort mit ffmpeg/ffprobe durchsuchbar.
2. **yt-dlp-Voll-Download ohne Twitch-API**: `download_args()` (`tb-vod-archive/src/twitch.rs:216`) oder das eigenstaendige Repo; beliebiges aktuell gelistetes VOD ziehbar.
3. **yt-dlp Zeitfenster-Download** `download_clip()`/`build_yt_dlp_cmd()` (`tb-highlight/src/twitch_vod.rs:98,175`): laedt nur den interessanten Abschnitt, ideal fuer gezielte Analyse ohne 20-GB-Voll-Download.
4. **ffprobe/ffmpeg** fuer Laenge, Keyframes, Frame-Extraktion, Schnitt (`miss_laenge`/`schnitt_args`).
5. **Lokaler STT-Server** (large-v3-turbo, OpenAI-kompatibel, 127.0.0.1:8791) fuer Sprach-/Reaktionsmomente; Rust-Client zeigt per Default schon hierher.
6. **Match-Zeitleiste**: GC-Weg (`gc.rs` GetMatchHistory + GetMatchMetaData, Decode in `native_rank.rs`) und deadlock-api `/v1/matches/{id}/metadata`; earlysalty Steam-ID 76561199026913891 bekannt.

# Fehlt (Luecke fuer Clip-Erkennung)

1. **Kein OCR** (tesseract fehlt) und **keine GPU** -> Bild-basierte Kill-Feed-/HUD-Erkennung muesste erst ein Werkzeug bekommen; CPU-only OCR ueber 100+ Stunden VOD ist teuer.
2. **Kein Bindeglied VOD-Zeit <-> Match-Zeit**: VOD `.info.json` traegt Wall-Clock-`timestamp`; die Match-Zeitleiste traegt `game_time_s` ab Matchstart. Der Versatz (wann im VOD begann welches Match) ist nirgends gespeichert und muss berechnet werden (z. B. Match-Startzeit aus deadlock-api gegen VOD-Startzeitstempel).
3. **`vod_offset` der Clips fehlt in der DB** (`twitch_clips_social_media` ohne Spalte); die 25 vorhandenen Clips lassen sich nicht direkt im VOD verorten, ohne Helix `vod_offset` nachzuladen.
4. **Keine gespeicherten Transkripte** des Nutzers (`twitch_engagement_stream_transcripts` = 0 Zeilen), obwohl der STT-Server laeuft.
5. **Browser-Automation fuer Clip-Erkennung nicht im Twitch-Bot-Repo gefunden**; Fundort in einem Nachbar-Repo offen.
6. **Zwei parallele VOD-Archiv-Implementierungen** (eigenstaendiges Repo mit SQLite und Dateien vs. `tb-vod-archive`-Crate mit Postgres): fuer eine Analyse-Pipeline auf eine Quelle festlegen.

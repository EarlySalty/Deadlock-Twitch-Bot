# highlight_clipper/ — Architektur & Funktionsreferenz

> Pfad: `bot/highlight_clipper/` · Stand: 2026-06-08 · 11 Dateien, ~1.555 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [api.md](api.md) (VOD-Suche/Clip), [social-media.md](social-media.md) (Weiterverarbeitung), [monitoring.md](monitoring.md). Hintergrund: Memory „Gute Clip-Momente = Skill/Outplay“, „Clip-Analyse-Methode“, „Highlight-Clipper Redesign“.

## 1. Zweck & Abgrenzung

`highlight_clipper/` erkennt **Deadlock-Highlight-Momente** eines Partner-Streamers (Multikills, Teamfights, knappe Outplays — nicht bloß Kills bei wenig HP), schneidet den passenden Ausschnitt aus dem **Twitch-VOD** und schickt ihn dem Streamer per DM. Die Erkennung kombiniert die **Match-Daten** (Deadlock-API) mit der **Demo** (für Combo-Labels) und filtert Solo-Kills heraus.

Abgrenzung: Die Video-Quelle ist das **VOD** (nicht Live-Capture). Das Hochladen auf Social-Media macht [social-media.md](social-media.md); hier endet es bei der DM an den Streamer.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `TwitchStreamCog` (`HighlightClipperMixin`); der Worker läuft als Hintergrund-Task. |
| **Nutzt** | Deadlock-API (`deadlock_client`), Match-Demo (`demo_downloader`/`demo_analyzer`), Twitch-Helix/VOD (`twitch_vod`, über `api/`), Steam-Bot-DB (Steam-IDs), `ffmpeg` (Download/Schnitt), Discord (DM). |
| **DB / Daten** | `data/highlight_clipper/` (Steam-ID-Map, Clips), Partner-Streamer-Liste, per-Streamer-State. |
| **Externe Dienste** | Deadlock-API, Twitch (VOD), Discord. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `worker.py` | 443 | `HighlightClipperWorker` — Loop pro Streamer/Match. |
| `demo_analyzer.py` | 429 | Demo-basierte Event-Erkennung (`KillMoment`). |
| `event_detector.py` | 219 | Match-basierte Event-Erkennung (`HighlightEvent`). |
| `twitch_vod.py` | 178 | VOD zum Match finden + Clip herunterladen/schneiden. |
| `state.py` | 76 | Per-Streamer-State (zuletzt verarbeitete Matches). |
| `demo_downloader.py` | 75 | Match-Demo herunterladen. |
| `dm_sender.py` | 47 | Clip per Discord-DM an den Streamer. |
| `deadlock_client.py` | 40 | Deadlock-API-Client (Matches). |
| `mixin.py` | 26 | `HighlightClipperMixin` — Worker an den Cog hängen. |
| `config.py` | 20 | Konfiguration (Schwellen, Pfade). |

## 4. Datenfluss / Lebenszyklus

1. `HighlightClipperWorker._loop` → `_run_once` iteriert über Partner-Streamer (`_get_partner_streamers`, Steam-IDs aus der Steam-Bot-DB via `_load_steam_account_ids` + manuelle Map `_load_manual_steamids`).
2. Pro Streamer (`_process_streamer`): neue Deadlock-Matches holen, gegen `state` filtern (`_filter_recent_matches`).
3. Pro Match (`_process_match`): `event_detector.detect_events(account_id, match_info)` findet Multikills/Teamfights/Close-Fights; die Demo wird geladen (`demo_downloader`) und `demo_analyzer.detect_all_events(demo_path, hero_id, login)` liefert `KillMoment`s mit Excitement-Score; `_score_events_with_demo` reichert die Events mit Combo-Labels an und **filtert Solo-Kills**.
4. `twitch_vod.find_vod_for_match(channel_id, match_start, duration)` findet das passende VOD; `download_clip(vod_id, start, end, out)` schneidet den Ausschnitt (ffmpeg).
5. `dm_sender` schickt den Clip dem Streamer per Discord-DM; `state` wird fortgeschrieben.

## 5. Funktionsreferenz pro Datei

### worker.py — `HighlightClipperWorker`
- `start()` / `stop()` / `_loop()` / `_run_once()` — Lebenszyklus.
- `_process_streamer(*, state, twitch_login, steam_id, twitch_api, now)` / `_process_match(*, state, twitch_login, steam_id, match, channel_id, twitch_api, clip_dir)` — Verarbeitung.
- `_get_partner_streamers()` / `_query_partner_streamers()` — Partner-Liste.
- `_load_steam_account_ids(discord_ids)` — primäre Steam-Account-IDs aus der Steam-Bot-SQLite. `_load_manual_steamids()` — manuelle Map aus `data/`.
- `_filter_recent_matches(matches, state, *, login, now)`, `_get_hero_id(steam_id, match_info)`, `_score_events_with_demo(events, moments)` (Combo-Labels, Solo-Kill-Filter).

### event_detector.py
- `detect_events(account_id, match_info) -> list[HighlightEvent]` — Hauptzerlegung.
- `_find_multikill_ranges(player_kills)`, `_find_teamfights(all_deaths, player_slot)` (verkettete Tode ≤ Schwelle), `_find_close_fights(player_kills, player_own_deaths)`, `_deduplicate_events(events)`, `_multikill_name(kill_count)`, `_find_player_slot`/`_collect_deaths`.

### demo_analyzer.py
- `detect_all_events(demo_path, hero_id, twitch_login) -> list[KillMoment]` — vollständige demo-basierte Erkennung ohne API-Abhängigkeit.
- `KillMoment.excitement_score()` — Bewertung eines Moments. `moments_to_events(moments, min_score)` — in Events überführen.

### twitch_vod.py
- `get_channel_id(login, twitch_api)`, `find_vod_for_match(channel_id, match_start_unix, match_duration_s, twitch_api) -> dict | None`, `download_clip(vod_id, clip_start_s, clip_end_s, output_path) -> bool` (ffmpeg via `_run_process`), Zeit-Helfer `_parse_twitch_datetime`/`_format_hhmmss`/`_parse_duration_seconds`.

### Übrige
`deadlock_client.py` (Match-Abruf), `demo_downloader.py` (Demo laden), `state.py` (verarbeitete Matches je Streamer), `dm_sender.py` (Discord-DM), `mixin.py` (`HighlightClipperMixin`), `config.py` (Schwellen/Pfade).

## 6. Datenbank & externe Schnittstellen

- **Daten:** `data/highlight_clipper/` (Steam-ID-Map, Clips), Steam-Bot-SQLite (Account-IDs), Partner-Liste.
- **Extern:** Deadlock-API (Matches/Demos), Twitch-VOD (Helix), Discord (DM).

## 7. Stolperfallen / Besonderheiten

- **VOD ist die Videoquelle, nicht Live:** Der Clip wird aus dem fertigen Twitch-VOD geschnitten — gibt es kein passendes VOD (`find_vod_for_match` → None), entsteht kein Clip.
- **Solo-Kills werden gefiltert:** `_score_events_with_demo` wirft reine Solo-Kills raus; gewollt sind Skill/Outplay/Teamfights (siehe Memory). Wer „warum kein Clip?“ fragt, prüft den Excitement-Score/Combo-Filter.
- **Steam-ID-Auflösung ist zweistufig:** primär aus der Steam-Bot-DB, ergänzt durch eine manuelle Map in `data/`. Ohne Steam-ID kein Match-Lookup.
- **Match→VOD-Zeitabgleich ist heikel:** `find_vod_for_match` rechnet Match-Start/-Dauer auf die VOD-Zeitachse; Drift führt zu falsch geschnittenen Clips (Framing-Fix war Thema, siehe Memory).

# Stat-Befehle & Stream-Overlay (SP2)

Streamer-Statistiken im Twitch-Chat + ein OBS-Overlay. **Alles GC-nativ über den eigenen Steam-Bot — kein `deadlock-api` als Datenquelle, kein API-Key.** Die `deadlock-api` wurde nur als Referenz evaluiert; ihre öffentliche match-history ist exakt der GC-Call `GetMatchHistory`, den unser steam-core selbst macht.

## Chat-Befehle (`tb-chat`)

In `tb-chat/src/{stats.rs, commands.rs, catalog.rs}`. Jeder Befehl löst die Broadcaster-`discord_id` auf (`resolve_discord_id`, `twitch_streamer_identities`) und ruft einen Steam-Bot-HTTP-Endpoint:

| Befehl | Quelle (Steam-Bot) | Inhalt |
|--------|--------------------|--------|
| `!rank` | `/rank` | aktueller Rang (gecacht in `steam_links`) |
| `!wins` | `/rank?include_stats=1` | Karriere-Siege (GC `KEStatWins`; **nur Siege** verlässlich) |
| `!winrate` `!lastmatch` `!streak` `!mostplayed` | `/player-matches` | aus der Match-Liste abgeleitet (`not_scored` ausgeschlossen) |
| `!mmr` `!climb` | `/player-mmr-trend` | Rang + Trend (badge-delta über Fenster) |
| `!live` | `/player-live` | im Match? (+ Hero, Minute) |

Reply-Funktionen sind pur + verhaltens-getestet (exakte `assert_eq!`, inkl. `not_scored`-Ausschluss). Steam-Bot-Basis-URL via env `STEAM_BOT_RANK_URL` (Default `http://127.0.0.1:8783`), Pfade via `*_url_from_rank`-Helfer abgeleitet.

## Steam-Bot-Endpoints (GC-nativ)

- `GET /rank?discord_id=&include_stats=` — Rang (ProfileCard) + optional Karriere-Siege (`GC_GET_ACCOUNT_STATS`). `losses`/`matches` bleiben `null` (im Hero-Stat-Namensraum nicht verlässlich rekonstruierbar; `KEStatGamesPlayed` trifft dort eine andere Größe).
- `GET /player-matches?discord_id=&limit=` — `GC_GET_MATCH_HISTORY` (msg 9112/9113, paginiert ≤150/5 Seiten), pro Match `match_result`/`hero_id`/`hero_name`/KDA/`not_scored`. **Concurrency-1-Lane** (`profile_card`), da die Response keine account_id trägt.
- `GET /player-mmr-trend?discord_id=&days=` — aus `steam_rank_history` (Snapshot beim Rank-Sync, dedupliziert). Trend akkumuliert ab Einbau.
- `GET /player-live?discord_id=` — aus `live_player_state` (`in_match_now_strict` + Freshness), liefert `live`/`in_deadlock`/`hero`/`minutes`/`stage`.

## Overlay

- **Render:** `GET /twitch/overlay?streamer=<login>` — self-contained transparentes HTML (`tb-dashboard-api/src/handlers/overlay.rs`, öffentlicher CSRF-freier Router), pollt alle 20 s. Config rein clientseitig über URL-Parameter: `theme` ∈ `dark|light|accent` (Default `dark`, `accent` = Marken-Gradient Cyan→Purple), `layout` ∈ `box|bar` (Default `box`), `pos` ∈ `bl|br|tl|tr` (Default `bl`), `opacity` 0–100 (Default 85, wirkt nur auf Karten-Hintergrund), `recent_n` 1–15 (Default 10) sowie Modul-Flags `header|rank|winrate|today|streak|kd|lastmatch|mostplayed|recent|live|branding` (`0`/`1`, alle Default `1` außer `lastmatch`+`mostplayed` Default `0`). Glassmorphism-Optik via `data-theme` + CSS-Custom-Properties; **leere Module werden gar nicht gerendert** (kein N/A). **Der Handler verzweigt:** mit `streamer`-Param → Render-HTML (für OBS, ohne Login erreichbar); ohne `streamer`-Param → SPA-Index (`serve_dashboard_v2_index` aus `spa.rs`).
- **Daten:** `GET /twitch/api/v2/public/overlay?streamer=<login>` — bündelt die 3 Steam-Bot-Endpoints, **30 s In-Memory-Cache pro Login**. Auflösung `twitch_streamers.twitch_login → twitch_user_id → twitch_streamer_identities.discord_user_id → steam`. Liefert neben Rang/MMR-Trend/Winrate/Serie/Live auch GC-nativ abgeleitete Felder: `today_*` (heutige W/L, Tagesgrenze `Europe/Berlin` via `chrono_tz`), `kd` (Σkills/Σdeaths übers Fenster), `recent[]` (bis 15, newest-first, je `result`+`hero`), `last_*`, `most_played_*` — alle aus `/player-matches` berechnet (pure Helfer `summarize_today`/`compute_kd`/`build_recent`/`summarize_matches`, `not_scored` ausgeschlossen, unit-getestet).
- **Builder:** eigene Seite `dashboard_v2/src/pages/OverlayBuilder.tsx` unter `/twitch/overlay` (Route in `App.tsx` via `isOverlayBuilderRoute`). Rendert `components/verwaltung/OverlayBuilderSection.tsx`: Stil-/Layout-Select, 11 Modul-Toggles, Slider für Verlaufslänge + Deckkraft, Position, generierte URL + Copy, OBS-Anleitung, Live-Vorschau (iframe, Höhe layout-adaptiv). Erreichbar zusätzlich über den Sidebar-Eintrag **„Stream-Overlay"** (Gruppe TOOLS in `InternalHomeLanding.tsx`). `VerwaltungPage` verlinkt nur.

## Assets (Deadlock-Spielgrafiken, © Valve)

Nur öffentliche Asset-URLs der Deadlock-CDN (kein fremder Code):
- Rang-Badge: `https://assets-bucket.deadlock-api.com/assets-api-res/images/ranks/rank{tier}/badge_lg_subrank{sub}.png` — `tier = badge_level/10`, `sub = badge_level%10`.
- Hero-Bild: Namens-Map aus `https://assets.deadlock-api.com/v2/heroes` (`images.icon_image_small`).

## Test-Hinweise

- Steam-Link-Test-DB: `/home/naniadm/Documents/Deadlock-Bots/data/deadlock.sqlite3`, `steam_links.user_id` = Discord-ID (in bun:sqlite als TEXT lesen — Snowflake-Präzisionsverlust!), `account_id = steam_id64 − 76561197960265728`.
- Live-Verify immer gegen einen echten verknüpften Account (der Leerfall `discord_id=1` kurzschließt und beweist nichts).

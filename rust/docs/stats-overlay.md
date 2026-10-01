# Stat-Befehle & Stream-Overlay (SP2)

Streamer-Statistiken im Twitch-Chat + ein OBS-Overlay. **Primär GC-nativ über den eigenen Steam-Bot.** Seit 2026-09-17 besitzt der Chat-Befehl `!rank` zusätzlich einen öffentlichen Deadlock-API-Fallback ohne API-Key. Die Overlay-Datenquelle bleibt unverändert.

## Chat-Befehle (`tb-chat`)

In `tb-chat/src/{stats.rs, commands.rs, catalog.rs}`. Ohne Ziel löst jeder Spielstatistikbefehl die Broadcaster-`discord_id` auf (`resolve_discord_id`, `twitch_streamer_identities`) und ruft einen Steam-Bot-HTTP-Endpoint:

| Befehl | Quelle (Steam-Bot) | Inhalt |
|--------|--------------------|--------|
| `!rank` | `/rank` | aktueller Rang (gecacht in `steam_links`) |
| `!wins` | `/rank?include_stats=1` | Karriere-Siege (GC `KEStatWins`; **nur Siege** verlässlich) |
| `!winrate` `!lastmatch` `!streak` `!mostplayed` | `/player-matches` | aus der Match-Liste abgeleitet (`not_scored` ausgeschlossen) |
| `!mmr` `!climb` | `/player-mmr-trend` | Rang + Trend (badge-delta über Fenster) |
| `!live` | `/player-live` | im Match? (+ Hero, Minute) |

Reply-Funktionen sind pur + verhaltens-getestet (exakte `assert_eq!`, inkl. `not_scored`-Ausschluss). Steam-Bot-Basis-URL via env `STEAM_BOT_RANK_URL` (Default `http://127.0.0.1:8783`), Pfade via `*_url_from_rank`-Helfer abgeleitet.

## Optionale Benutzerziele und Rank-Fallback (2026-09-17)

`command_target.rs` löst genau einen optionalen Twitch-Login (`@name` oder `name`) aus EventSub-Mentions, Absender/Kanal oder Helix auf. Alle acht Stat-Befehle und ihre Aliase verwenden dann die Twitch-ID des Ziels. Das Originalevent bleibt unverändert: Kanal-Einstellungen, Versandkanal und Berechtigungen bleiben am Aufrufkanal/Absender. `!watchtime` nutzt denselben Resolver, standardmäßig aber den Absender, und zählt ausschließlich im Aufrufkanal. Sein atomarer 10-Sekunden-Cooldown bleibt pro `(Kanal-ID, Absender-ID)`, nicht pro Ziel.

`rank_lookup.rs` verwendet zuerst `/rank?discord_id=...`. Bei fehlendem Rang/Freundschaft oder GC-Ausfall wird die bestätigte bevorzugte Verknüpfung aus `core.steam_links` gelesen (Primary zuerst, deterministischer Fallback). Nur dieser Steam-Account wird öffentlich abgefragt; unbestätigte Links, DB-Fehler und geschützte Accounts lösen keine alternative Namenssuche aus.

Nur bei explizitem Twitch-Ziel ohne bestehende Steam-Verknüpfung folgt `/v1/players/steam-search?search_query=...&min_matches_played_last_30d=0&matches_played_weight=0&limit=100`. Genau ein exakter Namensfund darf einen **als unbestätigt markierten** Rang anzeigen. Mehrdeutige, ähnliche oder abgeschnittene Suchergebnisse werden nicht automatisch zugeordnet. `!rank steam:<Account-ID/SteamID64>` ist eine rein lesende Auswahl; es gibt keine Link-Schreiboperation.

Öffentlicher Rang: `/v1/players/{account_id}/rank`, Namen dynamisch aus `/v1/assets/ranks`. Kein MMR-Schätzwert, kein Match-Teamdurchschnitt. Die Antwort kennzeichnet das letzte erfasste Ranked-Match und dessen Datum, sofern geliefert. HTTP-Timeout 3 Sekunden pro öffentlichem Request; Single-flight und 120-Sekunden-Cache pro Ressource (Rangnamen 1 Stunde), Fehler 15 Sekunden, maximal 256 Cache-Einträge. Prozessweites Budget pro CommandEngine: 16 öffentliche Requests/Minute, zusätzlich `Retry-After` bei HTTP 429 (1–300 Sekunden). Kein API-Key erforderlich.

Vertrag: https://api.deadlock-api.com/openapi.json (geprüft 2026-09-17). Tests liegen in `command_target_tests.rs` und `rank_lookup/tests.rs`; alle HTTP-Daten werden gemockt, Datenbanktests laufen auf privaten PostgreSQL-Prozessen ohne Produktionszugriff.

## Steam-Bot-Endpoints (GC-nativ)

- `GET /rank?discord_id=&include_stats=` — Rang (ProfileCard) + optional Karriere-Siege (`GC_GET_ACCOUNT_STATS`). `losses`/`matches` bleiben `null` (im Hero-Stat-Namensraum nicht verlässlich rekonstruierbar; `KEStatGamesPlayed` trifft dort eine andere Größe).
- `GET /player-matches?discord_id=&limit=` — `GC_GET_MATCH_HISTORY` (msg 9112/9113, paginiert ≤150/5 Seiten), pro Match `match_result`/`hero_id`/`hero_name`/KDA/`not_scored`. **Concurrency-1-Lane** (`profile_card`), da die Response keine account_id trägt.
- `GET /player-mmr-trend?discord_id=&days=` — aus `steam_rank_history` (Snapshot beim Rank-Sync, dedupliziert). Trend akkumuliert ab Einbau.
- `GET /player-live?discord_id=` — aus `live_player_state` (`in_match_now_strict` + Freshness), liefert `live`/`in_deadlock`/`hero`/`minutes`/`stage`.

## Overlay

Das Layout `canvas` ist eine freie OBS-Leinwand: `canvas_w` und `canvas_h` bestimmen die Browser-Quelle, `scene` enthält pro Quelle `x`, `y`, `width` und `height`. Die Overlay-Seite erzeugt diese URL automatisch.

Im Builder „Freie OBS-Leinwand" lassen sich Leinwandgröße und sichtbare Quellen per Drag-and-drop, Eckgriff oder X-/Y-/Breite-/Höhe-Feldern anpassen. „Layout speichern" merkt die komplette Konfiguration lokal im Browser; OBS erhält danach die fertige URL.

- **Render:** `GET /twitch/overlay?streamer=<login>` — self-contained transparentes HTML (`tb-dashboard-api/src/handlers/overlay.rs`, öffentlicher CSRF-freier Router), pollt alle 20 s. Config rein clientseitig über URL-Parameter: `theme` ∈ `dark|light|accent` (Default `dark`, `accent` = Marken-Gradient Cyan→Purple), `layout` ∈ `box|bar|canvas` (Default `box`), `pos` ∈ `bl|br|tl|tr` (Default `bl`), `opacity` 0–100 (Default 85, wirkt nur auf Karten-Hintergrund), `recent_n` 1–15 (Default 10) sowie Modul-Flags `header|rank|winrate|today|streak|kd|lastmatch|mostplayed|recent|live|branding` (`0`/`1`, alle Default `1` außer `lastmatch`+`mostplayed` Default `0`). Beim Layout `canvas` kommen `canvas_w`, `canvas_h` und die JSON-Scene `scene` mit `x`, `y`, `width` und `height` je Quelle hinzu. Glassmorphism-Optik via `data-theme` + CSS-Custom-Properties; **leere Module werden gar nicht gerendert** (kein N/A). **Der Handler verzweigt:** mit `streamer`-Param → Render-HTML (für OBS, ohne Login erreichbar); ohne `streamer`-Param → SPA-Index (`serve_dashboard_v2_index` aus `spa.rs`).
- **Daten:** `GET /twitch/api/v2/public/overlay?streamer=<login>` — bündelt die 3 Steam-Bot-Endpoints, **30 s In-Memory-Cache pro Login**. Auflösung `twitch_streamers.twitch_login → twitch_user_id → twitch_streamer_identities.discord_user_id → steam`. Liefert neben Rang/MMR-Trend/Winrate/Serie/Live auch GC-nativ abgeleitete Felder: `today_*` (heutige W/L, Tagesgrenze `Europe/Berlin` via `chrono_tz`), `kd` (Σkills/Σdeaths übers Fenster), `recent[]` (bis 15, newest-first, je `result`+`hero`), `last_*`, `most_played_*` — alle aus `/player-matches` berechnet (pure Helfer `summarize_today`/`compute_kd`/`build_recent`/`summarize_matches`, `not_scored` ausgeschlossen, unit-getestet).
- **Builder:** eigene Seite `dashboard_v2/src/pages/OverlayBuilder.tsx` unter `/twitch/overlay` (Route in `App.tsx` via `isOverlayBuilderRoute`). Rendert `components/verwaltung/OverlayBuilderSection.tsx`: Stil-/Layout-Select, 11 Modul-Toggles, Slider für Verlaufslänge + Deckkraft, Position, generierte URL + Copy, OBS-Anleitung, Live-Vorschau (iframe, Höhe layout-adaptiv). Im Layout **„Freie OBS-Leinwand"** lassen sich Leinwandgröße und sichtbare Quellen per Drag-and-drop, Eckgriff oder X-/Y-/Breite-/Höhe-Feldern anpassen. **„Layout speichern"** merkt die komplette Konfiguration lokal im Browser; OBS erhält danach die fertige URL. Erreichbar zusätzlich über den Sidebar-Eintrag **„Stream-Overlay"** (Gruppe TOOLS in `InternalHomeLanding.tsx`). `VerwaltungPage` verlinkt nur.

## Assets (Deadlock-Spielgrafiken, © Valve)

Nur öffentliche Asset-URLs der Deadlock-CDN (kein fremder Code):
- Rang-Badge: `https://assets-bucket.deadlock-api.com/assets-api-res/images/ranks/rank{tier}/badge_lg_subrank{sub}.png` — `tier = badge_level/10`, `sub = badge_level%10`.
- Hero-Bild: Namens-Map aus `https://assets.deadlock-api.com/v2/heroes` (`images.icon_image_small`).

## Test-Hinweise

- Steam-Link-Test-DB: `/home/naniadm/Documents/Deadlock-Bots/data/deadlock.sqlite3`, `steam_links.user_id` = Discord-ID (in bun:sqlite als TEXT lesen — Snowflake-Präzisionsverlust!), `account_id = steam_id64 − 76561197960265728`.
- Live-Verify immer gegen einen echten verknüpften Account (der Leerfall `discord_id=1` kurzschließt und beweist nichts).

## Direkte Zuschauerverknüpfung (2026-09-18)

`!connect` verlinkt `/twitch/connect`, ohne benutzerspezifische Bearer-Tokens im öffentlichen Chat. Der vorhandene Twitch-OAuth-Callback akzeptiert für das exakte, gespeicherte Ziel `/twitch/connect` eine getrennte `twitch_player`-Session; die Partner-/Admin-Gates bleiben unverändert. Anschließend startet ein sessiongebundener CSRF-POST Steam OpenID 2.0. Pinning von Provider und return_to, vollständige signierte Identitätsfelder, serverseitige check_authentication-Verifikation, frische Nonces, globale Nonce-Replaysperre und atomare Linkrevision schützen den Abschluss.

`twitch_player_steam_links` ist unabhängig von `core.steam_links`. Direkte Links gewinnen bei Rank vor Discord und Namenssuche; Discord-Daten anderer Konten dürfen nicht als Daten des direkt verbundenen Kontos erscheinen. `!unconnect` setzt einen selbstbezogenen Opt-out und löscht die direkte Steam-ID. Der Opt-out gilt für alle identitätsbasierten Chat-Stats, nicht für explizite öffentliche `!rank steam:<ID>`-Abfragen und nicht für Watchtime. Keine automatischen Discord-Link-Schreibzugriffe oder Bot-Freundschaftsanfragen.

## Direkte Zuschauer-Verknüpfung (2026-09-18)

- `!connect` postet ausschließlich die öffentliche URL `/twitch/connect`, keine personenbezogenen Einmaltokens im Chat.
- Vorhandener Twitch-OAuth-Codeflow mit Browser-Kontext und dem registrierten Callback. `next=/twitch/connect` erstellt ausschließlich eine kurzlebige `twitch_player`-Session, keine Partner-/Admin-Session und keine Partnerschaft.
- Steam OpenID 2.0: POST-Start mit sessiongebundenem CSRF, fester Provider und HTTPS-Callback aus Serverkonfiguration, signierte Identität/Nonce/Return-URL, serverseitiges `check_authentication`, Nonce-Replay-Sperre und einmaliger browsergebundener Flow.
- `twitch_player_steam_links`: stabile Twitch-ID, optionale SteamID64, lookup_enabled, Revision und Zeitstempel. CAS verhindert Wiederbelebung nach `!unconnect` und konkurrierendes Überschreiben.
- Rank liest direkte Links vor Discord-/Namensauflösung. Opt-out und Kontowechsel werden nach HTTP erneut geprüft, auch auf dem Legacy-Pfad. Weitere Stat-Commands bleiben auf denselben verknüpften Discord-/Steam-Account beschränkt; direkte Steam-ID allein entsperrt derzeit nur Rank. Watchtime bleibt unverändert unabhängig von Steam.
- `!unconnect`/`!disconnect` mutiert nur die authentische Chatter-ID, keine Ziele oder Mod-Ausnahmen; löscht die gespeicherte Steam-ID und erhält nur den Twitch-bezogenen Opt-out. Separate Discord-Verknüpfungen bleiben erhalten.
- Datenbankmigrationen `20260918100000_twitch_player_steam_links.sql` und `20260920170000_twitch_player_multi_steam.sql` müssen vor dem Dienstneustart über den vorhandenen Migrationsdienst laufen. Die Runtime darf weiterhin kein DDL.

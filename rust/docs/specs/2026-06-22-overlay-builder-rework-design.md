# Overlay-Builder Rework — Design-Spec (SP2)

**Datum:** 2026-06-22
**Status:** abgenommen (Richtung), wartet auf Spec-Review → writing-plans

## Ziel

Den bestehenden Overlay-Baukasten (4 Toggles + Position) zu einem **hochwertigen, optisch
schicken** Builder ausbauen — inspiriert vom deadlock-api-Streamkit, aber mit **eigenem Code**
und **GC-nativ** (Datenquelle = unser Steam-Bot, kein deadlock-api).

Zusätzlich: der Overlay-Builder bekommt einen **eigenen Eintrag in der Dashboard-Seitenleiste**
(neben „Verwaltung"), nicht nur den Link aus der Verwaltung.

## Harte Rahmenbedingungen (Locks)

- **Lizenz:** `deadlock-api/streamkit` hat KEINE Lizenz → wir kopieren **kein** Markup, kein CSS,
  kein URL-Schema. Nur die UX-Idee + die öffentlichen Valve-Spiel-Assets (Rang-Badges, Hero-Icons)
  werden übernommen. Eigener Code.
- **GC-nativ, kein deadlock-api:** nur Daten, die unser Steam-Bot zuverlässig liefert.
- **Stateless:** Config ausschließlich über URL-Parameter, **keine DB, kein State**.
- **User-sichtbare Texte = Claude**, nicht Codex.
- **„Schick":** explizite User-Anforderung — die Referenz ist funktional aber lieblos
  (zeigt z. B. `RANK -` / `PLACE N/A`). Wir machen es hochwertig: Glassmorphism, saubere
  Typografie, Themes, Recent-Matches-Strip, leere Module verstecken sich statt Platzhalter-Müll.

## Nicht-Ziele (YAGNI)

- Kein Command-Builder (eigene `!befehl`-Templates) — bewusst aufgeschoben.
- Kein deadlock-api: kein Leaderboard-Platz, kein Predicted Rank, keine Career-Hours/Total-Matches,
  keine „Max … Stacks"-Stats (unzuverlässiger GC-Account-Stats-Namensraum, siehe `!wins`-Fund).
- Keine freie „Variable+Label"-Tabelle wie im Streamkit → kuratierte Module (sehen per Default gut
  aus, ehrlich an Datenlage gekoppelt).
- Max. 2 Layouts (Box, Bar). Max. 3 Themes (Dunkel, Hell, Akzent).

## Datenschicht — `tb-dashboard-api/src/handlers/overlay.rs::build_overlay_json`

Quellen bleiben (alle schon geholt, ein `tokio::join!`): `/player-mmr-trend`, `/player-matches`,
`/player-live`. Career-Siege optional via `/rank?include_stats=1`.

`/player-matches` liefert pro Match: `match_result` (1=Sieg/0=Niederlage), `hero_id`, `hero_name`,
`start_time` (unix, UTC), `not_scored`, `team_abandoned`, `player_kills/deaths/assists`.
**Alle neuen Stats sind daraus berechenbar — kein steam-core/steam-bot-Eingriff nötig.**

`SteamMatch`-Deserialisierung erweitern um: `start_time`, `player_kills`, `player_deaths`,
`player_assists`, `hero_name`.

Neue/erweiterte Ableitungen (nur gewertete Matches, `not_scored != true`):
- **today**: `today_wins`, `today_losses`, `today_winrate`, `today_matches` — Matches mit
  `start_time ≥ Tagesbeginn Europe/Berlin (00:00 lokale Zeit)`. Tagesgrenze fix Europe/Berlin
  (Community-Zeitzone); kein User-TZ-Input.
- **kd**: `kd = Σkills / max(Σdeaths, 1)` übers Fenster, auf 2 Nachkommastellen.
- **recent**: Array der letzten `recent_n` Matches, **newest-first**, je `{ result: "win"|"loss",
  hero: <hero_name> }`. `recent_n` aus URL (Default 10, Cap 15). `not_scored` ausgeschlossen.
- **most_played**: `most_played_hero` + `most_played_count` (Fenster).
- **last_match**: `last_result`, `last_hero`, `last_kills`, `last_deaths`, `last_assists`.
- Bestehend: `rank_name`, `badge_level`, `delta` (MMR-Trend); `winrate`/`wins`/`losses` (Fenster);
  `streak_kind`/`streak_len`; `live`/`hero`/`minutes`; `career_wins` (optional).

Antwort-JSON (`OverlayResponse`), alle neuen Felder `Option`/Default, `null`=Modul versteckt sich:
```
ok, streamer,
rank_name, badge_level, delta,
winrate, wins, losses,
today_wins, today_losses, today_winrate, today_matches,
kd,
streak_kind, streak_len,
last_result, last_hero, last_kills, last_deaths, last_assists,
most_played_hero, most_played_count,
recent: [ { result, hero } ],
career_wins,
live, hero, minutes
```
30s-In-Memory-Cache pro Login bleibt.

## Render-Schicht — `overlay.rs::OVERLAY_HTML` (neu)

Self-contained HTML, transparenter Body (OBS), pollt alle 20 s `/twitch/api/v2/public/overlay`.

**URL-Parameter** (alle optional, Defaults):
- `streamer` (Pflicht für Render)
- `theme` ∈ `dark|light|accent` (Default `dark`)
- `layout` ∈ `box|bar` (Default `box`)
- `pos` ∈ `bl|br|tl|tr` (Default `bl`)
- `opacity` 0–100 (Default `85`) — wirkt **nur** auf Karten-Hintergrund, nie auf Text/Bilder
- Modul-Flags (`1`/`0`, Default `1` außer wo genannt): `header`, `rank`, `winrate`, `today`,
  `streak`, `kd`, `lastmatch` (Default `0`), `mostplayed` (Default `0`), `recent`, `live`, `branding`
- `recent_n` 1–15 (Default 10)

**Visual-Spezifikation („schick"):**
- **Karte (Box):** Glassmorphism — `backdrop-filter: blur(12px) saturate(140%)`,
  `border-radius: 14px`, Schatten `0 18px 40px rgba(0,0,0,.45)`, 1px Border in Theme-Border-Farbe,
  dünner Akzent-Glow/Linie oben oder links. Innen-Padding großzügig, klare vertikale Rhythmik.
- **Theme über CSS-Custom-Properties** (ein `data-theme`-Attribut schaltet um). `--bg-alpha` =
  `opacity/100`:
  - `dark`: `--bg: rgba(13,15,20,var(--bg-alpha))`, `--fg:#f4f7fb`, `--muted:#9aa6b6`,
    `--accent:#22d3ee`, `--win:#34d399`, `--loss:#fb7185`, `--border:rgba(255,255,255,.10)`.
  - `light`: `--bg: rgba(248,250,253,var(--bg-alpha))`, `--fg:#0f172a`, `--muted:#475569`,
    `--accent:#0891b2`, `--win:#059669`, `--loss:#e11d48`, `--border:rgba(15,23,42,.12)`.
  - `accent` (Marke): `--bg` dunkel mit Marken-Tint, Akzent-Elemente nutzen den Marken-Gradient
    `linear-gradient(135deg,#06B6D4,#A855F7)` (Header-Unterstrich, Glow); `--fg` hell.
- **Typografie:** Inter (system-ui-Fallback). Stat-Werte halbfett, größer,
  `font-variant-numeric: tabular-nums` (saubere Zahlen-Ausrichtung). Labels klein,
  `text-transform: uppercase`, `letter-spacing:.06em`, in `--muted`.
- **Header:** Spielername (`--fg`, bold) links, **Live-Badge** rechts (pulsierender Punkt +
  „LIVE" in `--win`), darunter dünne Akzent-Linie (Gradient bei `accent`).
- **Stat-Raster (Box):** horizontale Zellen, je dünner vertikaler Trenner (`--border`); pro Zelle
  Label oben (muted) + Wert unten. Bei Rang das **Badge-Bild** (40px, Deadlock-CDN). Deutsche
  Auto-Labels: `RANG`, `WINRATE`, `HEUTE`, `SERIE`, `K/D`, `LAST`, `MAIN`.
- **Recent-Matches-Strip:** Reihe runder **Hero-Icons** (26px) mit 2px-Ring in `--win`/`--loss`
  je Ergebnis; fehlt das Icon → farbiger Punkt-Fallback (Ring-Farbe). Optionales Mini-Label
  „Letzte". Neueste links.
- **Bar-Layout:** schlanke Pille (`border-radius: 999px`/12px), Module als `·`-getrennte
  Inline-Segmente, Recent-Strip kompakt, Live-Punkt. Eine Zeile, schmal.
- **Animation:** Eintritt `fade + translateY(4px)` 180ms ease-out; Live-Punkt `pulse`-Keyframe.
- **Leere Module verstecken sich** (kein `N/A`/`-`): hat ein Modul keinen Wert (z. B. heute noch
  kein Match, kein Live), wird die Zelle/das Segment **gar nicht gerendert**.
- Werte-Formatierung: Winrate `56,7 %` (Komma, 1 Nachkomma), K/D `1,80` (Komma, 2 Nachkomma),
  W/L `4–2` (Gedankenstrich). Deutsche Zahlformatierung.

## Builder-Schicht — `bot/dashboard_v2/src/components/verwaltung/OverlayBuilderSection.tsx`

Regler (alle steuern URL-Params, Live-Vorschau aktualisiert sofort):
- **Theme**-Select: Dunkel / Hell / Akzent
- **Layout**-Select: Box / Bar
- **Module-Toggles** (deutsch): Header (Name + Live) · Rang · Winrate (Fenster) · Heute (W/L) ·
  Serie · K/D · Last Match · Most Played Hero · Recent-Matches (+ Anzahl-Slider 1–15) · Live-Match ·
  Branding
- **Hintergrund-Deckkraft**-Slider (0–100, Default 85)
- **Position** (4 Ecken)
- **Live-Vorschau** (iframe auf Checker-Background) mit **echten Daten** des verknüpften Accounts;
  Vorschau-Höhe passt zu Layout (Box ~220px, Bar ~100px)
- **Overlay-URL** + Copy
- **OBS-Anleitung** + empfohlene Browser-Quellen-Größe je Layout (Box 360×200, Bar 520×80)

Hochwertige UI: konsistent mit dashboard_v2-Designsystem (`panel-card`, `--color-primary`),
gruppierte Regler, klare Hierarchie. Deutsche Texte = Claude.

## Sidebar-Navigation

Neuer Eintrag im Nav-Array in `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx` (Gruppe **TOOLS**,
direkt neben „Verwaltung"):
```
{ href: PREVIEW_OVERLAY_ROUTE, label: 'Stream-Overlay', icon: MonitorPlay }
```
(`MonitorPlay` aus lucide-react; `PREVIEW_OVERLAY_ROUTE` existiert schon.) Voll-Navigation auf
`/twitch/overlay` → Builder-SPA. Prüfen, ob weitere geteilte Sidebar-Instanzen existieren; falls ja,
dort gespiegelt ergänzen.

## Architektur / betroffene Dateien

| Datei | Änderung |
|-------|----------|
| `tb-dashboard-api/src/handlers/overlay.rs` | Daten-Ableitungen + Render-HTML neu |
| `bot/dashboard_v2/src/components/verwaltung/OverlayBuilderSection.tsx` | reicher Builder |
| `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx` | Sidebar-Nav-Eintrag |
| `rust/knowledge/bot/faq-stats-overlay.md`, `rust/docs/stats-overlay.md` | Doku-Update |

**Caddy:** keine neue Änderung nötig — die `#265`-CSP für `/twitch/overlay` erlaubt bereits
Inline-Script/Style + beide Deadlock-CDN-Hosts (`img-src`) + `connect-src` für die Hero-Map.
Theme/Layout sind reine URL-Params.

## Fehlerbehandlung

- Unbekannter/unverknüpfter Streamer → `ok:false` → Overlay rendert nichts (Bestand).
- Modul ohne Daten → Modul/Zelle versteckt sich.
- Asset-Ladefehler → `onerror` entfernt `img` bzw. Punkt-Fallback.
- Ungültige Params → Default-Wert.

## Tests

- **Daten-Ableitungen** (wiremock + Schema, exakte `assert_eq!`): today-W/L mit `start_time`-
  Tagesgrenze (gestern ausgeschlossen, heute drin, `not_scored` raus), K/D, recent-Array
  (newest-first, Länge = `recent_n`-Cap), most_played, last_match. Bestehende Overlay-Tests anpassen.
- **Render-Branches** (String-Assert am HTML): `theme=light|accent` → korrektes `data-theme`/Var;
  `layout=bar` → Bar-Container; Modul-Flags an/aus → Präsenz/Abwesenheit; `opacity` im Style.
- Frontend: `tsc -b` grün; manuelle Vorschau-Verifikation.

## Deploy

`npm run build` (dashboard_v2 → `bot/analytics/dashboard_v2/dist`) → `cargo build --release --bin
tb-dashboard` → Service-Restart → Verifikation 8769 direkt + öffentlich durch Caddy (Render mit
allen Layouts/Themes, Builder-SPA, Daten-Endpoint, echter Streamer) → CHANGELOG #267 → Discord
(`twitch`) + In-App → Worktree/Branch aufräumen → Memory-Update.

## Implementierung

Technische Umsetzung an Codex (gpt-5.5/xhigh) delegierbar; user-sichtbare deutsche Texte schreibt
Claude. Reihenfolge: Datenschicht (TDD) → Render → Builder → Sidebar → Doku → Deploy.

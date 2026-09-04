# Plan: Schwarz und Kacheln der Landingpage ins Dashboard

status: aktiv
datum: 2026-09-04
contract: CONTRACT.md
research: RESEARCH.md, EVIDENCE.md

## Entscheidungen (Orchestrator)

- E1 Vorlage ist `website/src/theme-v2.css` (aktiv über `data-theme="v2"`). Alle Werte werden 1:1 übernommen, nichts „ähnlich" erfunden; jede übernommene Zeile bekommt in EVIDENCE.md eine Fundstelle.
- E2 Body-Hintergrund im Dashboard: nur `--gradient-bg` der Vorlage (180deg, #0f0f0e → #0b0b0b 55% → #101010). Die drei Gold-Radials und die Holzmaserung (index.css ~86-99) entfallen. Raster wie Vorlage: weiß rgba(255,255,255,0.045), 1px, 36px, radiale Ellipsen-Maske, Deckkraft 0.72. Noise nur, wenn die Vorlage Noise hat, sonst weg.
- E3 `.internal-home-vibe` bleibt als Klasse (Test hängt daran), die Pseudo-Elemente `::before/::after` (animierte Gold-Aura) werden ersatzlos gestrichen. `BackgroundBlobs` aus `DashboardShell.tsx` entfernt (Funktion und Aufruf).
- E4 Kacheln: `.panel-card` und `.glass` werden flach wie die Vorlage: Fläche `linear-gradient(0deg, rgba(239,212,157,0.05), rgba(239,212,157,0.05)), rgba(18,18,18,0.86)` (Vorlagenwerte theme-v2.css:65-67), Kante rgba(239,212,157,0.18), Schatten `0 14px 40px rgba(0,0,0,0.5)`, Radius der Vorlage. Gusseisen-Streifen, Lichtabfall, Bevel-Insets und die Niet-Punkte (`.panel-card::after`) entfallen. `card-glow` bleibt, Hover-Kante in der Kantenfarbe der Vorlage.
- E5 Border-Tokens (`--color-border*`) auf die Vorlagen-Alphas (0.16/0.34/0.28). Gold-Akzent-Tokens, Verlaufstext, Buttons, Badges unverändert.
- E6 Keine Hex-Literale neu einführen (brandPalette.test.ts), Werte als rgba schreiben. tailwind.config.* und ddc-design-tokens.css nicht anfassen (tot).
- E7 Regressionstest `tests/dashboardLook.test.ts` (Quelltext-Regex, in package.json scripts.test eintragen): (a) index.css enthält kein `.panel-card::after` und keine `repeating-linear-gradient` in `.glass/.panel-card`, (b) `.internal-home-vibe::before/::after` nicht vorhanden, (c) DashboardShell.tsx enthält kein `BackgroundBlobs`, (d) das Raster in index.css nutzt rgba(255,255,255,0.045) und `--gradient-bg` trägt die drei Vorlagenwerte. Rot-Gegenprobe je Punkt per Sabotage, Ist/Soll notieren.
- E8 Keine Code-Kommentare; bestehende Kommentare in angefassten CSS-Blöcken löschen.

## Milestones

### M1 Baseline
- `cd bot/dashboard_v2 && npm ci --no-audit --no-fund && npm test > /tmp/tb-look-baseline.log 2>&1; echo exit=$?`; `npx tsc -b && npx vite build`. Zahlen notieren.

### M2 Hintergrund (E2, E3)
- index.css Body/Raster/Vibe, DashboardShell.tsx ohne Blobs. Test (E7 b, c, d) rot vor, grün nach.
- Validierung: npm test in Datei, tsc, vite build.

### M3 Kacheln (E4, E5)
- index.css `.glass`, `.panel-card`, `.card-glow`, Border-Tokens. Test (E7 a) rot vor, grün nach.
- Validierung wie M2.

### M4 Sichtprüfung
- `npx vite --mode preview --host localhost --port 4176 --strictPort` im Hintergrund (Port 4174 ist belegt), dann je Route `google-chrome --headless=new --disable-gpu --no-sandbox --hide-scrollbars --window-size=2560,1400 --virtual-time-budget=8000 --screenshot=<name>.png http://localhost:4176<pfad>` für `/dashboard`, `/`, `/social-media-admin`, `/uplink`, `/verwaltung`, `/overlay`, `/pricing`; danach mit ffmpeg auf 1280px Breite verkleinern (`-vf scale=1280:-1`) und nur die kleinen Dateien unter `.tasks/2026-09-04-dashboard-schwarz-kacheln/screens/` ablegen. Server danach über die PID aus `ss -ltnp` beenden.
- Erwartung: Hintergrund neutral schwarz mit weißem Raster, Karten flach mit feiner heller Kante, Gold nur als Akzent. Vergleich mit `vorlage-streamer.png`.

### M5 Selbstprüfung
- `python3 /home/nathanael/Documents/claude-config/bin/diff-policy.py /home/nathanael/.worktrees/tb-dashboard-schwarz origin/main`
- Commit je Milestone, Status hier nachführen.

## Status

- M1: fertig, Baseline 179 pass, 0 fail, 0 skipped, tsc und vite gruen
- M2: fertig, Test E7 b/c/d gruen, tsc und vite gruen
- M3: fertig, Test E7 a gruen, 183 pass, 0 fail, tsc und vite gruen
- M4: offen
- M5: offen

## Befunde ausserhalb des Scopes

- keine

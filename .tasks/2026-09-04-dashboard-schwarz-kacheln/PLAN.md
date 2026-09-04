# Plan: Schwarz und Kacheln der Landingpage ins Dashboard

status: erledigt
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
- M4: fertig, sieben Screenshots unter screens/, Hintergrund neutral schwarz mit weissem Raster, Kacheln flach mit feiner Goldkante, Gold nur Akzent; Vergleich mit vorlage-streamer.png passt
- M5: fertig, diff-policy OK (16 Dateien, 247 Quellzeilen, 0 User-Freigaben), Endstand 183 pass, 0 fail, 0 skipped

## Befunde ausserhalb des Scopes

- bot/dashboard_v2/src/components/socialmedia/LayoutEditor.tsx:243 und :245 tragen harte warme Diagonal-Streifen (`repeating-linear-gradient(45deg, rgba(197,160,89,0.18) ...)`), die die Quell- und Ziel-Rahmen im Social-Layout-Editor braun-gestreift wirken lassen. Liegt ausserhalb des erlaubten Bereichs (weder Shell noch Kartenklassen), daher nicht geaendert.

## Runde 2 (REQ-07, REQ-08)

status: fertig

- REQ-07: DashboardShell.tsx:20 Wurzelcontainer von `relative mx-auto max-w-[2200px]` auf `relative` (volle Breite, kein mx-auto, kein max-w-); Demo-Zweig ohne Sidebar nutzt denselben Container. Test dashboardShell.test.ts:57 auf das neue Soll umgestellt (doesNotMatch mx-auto und max-w-).
  - TESTNACHWEIS REQ-07 Sabotage: max-w-[2200px] wieder in die Shell -> Test "die Shell traegt Hintergrund, Gesamtbreite, Sidebar-Spalte und den Main-Slot" not ok 3 (Ist: match auf max-w-, Soll: doesNotMatch), Sabotage zurueckgenommen -> ok 3.
- REQ-08: App.tsx Badge-Zeile `<div className="flex justify-end"><AuthBadge /></div>` und die AuthBadge-Definition entfernt; ungenutzte lucide-Imports (Sparkles, Shield, ShieldAlert, ShieldCheck, Wifi) und die dadurch ungenutzten Destructuring-Bindungen loadingAuth und authError entfernt. i18n-Schluessel t('Partner') und t('Demo-Daten') bleiben (weiter genutzt in SocialMediaAdmin.tsx und Header.tsx), Dictionary unangetastet. Demo-Hinweisbanner bleibt.
  - TESTNACHWEIS REQ-08: neuer Test "App.tsx traegt keine AuthBadge-Zeile mehr ueber dem Analyse-Kopf" (doesNotMatch AuthBadge). Vor dem Fix not ok 9 (Ist: AuthBadge in App.tsx vorhanden, Soll: keins), nach dem Fix ok, npm test 184 pass 0 fail 0 skipped.
- Validierung: npm test 184 pass/0 fail/0 skipped, tsc exit=0, vite build exit=0.
- Sichtpruefung: screens/dashboard.png, screens/analyse.png (`/`), screens/uplink.png ersetzt (Port 4176). Sidebar bündig am linken Rand nach dem Außenabstand; Sidebar-Oberkante und erste Inhaltskarte auf gleicher Höhe (dashboard, uplink); auf `/` kein AuthBadge mehr über dem Kopf, Demo-Banner bleibt.

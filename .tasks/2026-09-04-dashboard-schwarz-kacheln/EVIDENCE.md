# Evidence: Schwarz und Kacheln der Streamer-Landingpage ins Dashboard übernehmen

status: erledigt
datum: 2026-09-04
contract: CONTRACT.md

Repo-Aufklärung vor dem ersten Edit. Jede Zeile ist eine Fundstelle `pfad:zeile`,
keine Vermutung.

## Vorlage: welches Stylesheet lädt `/streamer` wirklich

- website/index.html:5 : `<html lang="de" data-theme="v2">`; die Root-index.html ist die `/streamer`-Seite und trägt das v2-Attribut.
- website/index.html:2 : Kommentar bestätigt, `data-theme="v2"` aktiviert das schwarze Patch-Theme (src/theme-v2.css) NUR auf der Landing-index.html.
- website/src/main.tsx:3 : `import './index.css'` (Basis-Theme, warm).
- website/src/main.tsx:5 : `import './theme-v2.css'` (schwarzer v2-Override, gescoped auf `[data-theme="v2"]`).
- website/src/theme-v2.css:3-6 : Datei ist bewusst auf `[data-theme="v2"]` gescoped, damit die schwarzen Tokens nicht in faq/onboarding/affiliate/vertriebler leaken; nur die Landing-index.html trägt das Attribut.

Fazit: `/streamer` = index.css (Basis) plus theme-v2.css (aktive schwarze Werte, weil `data-theme="v2"` gesetzt). Die maßgeblichen Vorlagenwerte stehen im v2-Scope von theme-v2.css.

## Vorlage: exakte Werte (theme-v2.css, aktiv auf /streamer)

- website/src/theme-v2.css:12-15 : `--color-bg:#0b0b0b`, `--color-background:#101010`, `--color-card:rgba(20,20,20,0.82)`, `--color-card-hover:rgba(32,32,32,0.88)`.
- website/src/theme-v2.css:18-20 : `--color-border:rgba(239,212,157,0.16)`, `--color-border-strong:rgba(239,212,157,0.34)`, `--color-border-hover:rgba(239,212,157,0.28)`.
- website/src/theme-v2.css:21-23 : `--color-text-primary:#f2eee6`, `--color-text-secondary:#9d968a`, `--color-secondary:#9d968a`.
- website/src/theme-v2.css:26-27 : `--gradient-bg:linear-gradient(180deg,#0f0f0e 0%,#0b0b0b 55%,#101010 100%)`, `--gradient-hero:linear-gradient(160deg,#0b0b0b 0%,#101010 55%,#161616 100%)`.
- website/src/theme-v2.css:28-29 : `--shadow-card:0 26px 70px rgba(0,0,0,0.62), inset 0 1px 0 rgba(255,255,255,0.04)`, `--shadow-card-soft:0 16px 45px rgba(0,0,0,0.5), inset 0 1px 0 rgba(255,255,255,0.035)`.
- website/src/theme-v2.css:40-42 : `[data-glow-orb]{display:none}`, die drei animierten GlowOrb-Lichtkugeln der Landing sind im v2-Look ausgeschaltet.
- website/src/theme-v2.css:45-47 : Body-Hintergrund im v2 ist `background:var(--gradient-bg)`, kein Gold-Glow-Radial, flach schwarz.
- website/src/theme-v2.css:50-55 : Raster (`body::before`) im v2, `background-image` zwei `linear-gradient(rgba(255,255,255,0.045) 1px, transparent 1px)`, `opacity:0.72`. Größe und Maske erbt es aus index.css.
- website/src/index.css:120-125 : Basis-`body::before`, `background-size:36px 36px`, `mask-image:radial-gradient(ellipse at top, black 40%, transparent 75%)`, `opacity:0.5` (im v2 auf 0.72 überschrieben).
- website/src/theme-v2.css:62-70 : Kachel im v2 (`.panel-card,.glass,.rd-twitch-embed`), `background:linear-gradient(0deg, rgba(201,168,106,0.05), rgba(201,168,106,0.05)), rgba(18,18,18,0.86)`; `border-color:rgba(239,212,157,0.18)`; `box-shadow:0 14px 40px rgba(0,0,0,0.5)`. Flach, ein dünner matter Goldfilm, feine Kante, ein einziger weicher Schlagschatten.
- website/src/index.css:73 : Gold-Akzentverlauf bleibt, `--gradient-brand:linear-gradient(120deg,#f6ddb0 0%,#efd49d 28%,#c8a86b 66%,#a98746 100%)` (im v2 nicht überschrieben).

## Dashboard-Ist: Tokens (bot/dashboard_v2/src/index.css @theme)

- bot/dashboard_v2/src/index.css:23-26 : `--color-bg:#0b0b0b`, `--color-background:#101010`, `--color-card:rgba(20,20,20,0.82)`, `--color-card-hover:rgba(32,32,32,0.88)`, identisch zur Vorlage.
- bot/dashboard_v2/src/index.css:49-50 : `--color-text-primary:#f2eee6`, `--color-text-secondary:#9d968a`, identisch zur Vorlage.
- bot/dashboard_v2/src/index.css:54-56 : `--color-border:rgba(239,212,157,0.22)`, `--color-border-strong:rgba(239,212,157,0.40)`, `--color-border-hover:rgba(239,212,157,0.34)`, dieselbe Grundfarbe, aber höhere Alpha als Vorlage (0.16/0.34/0.28).
- bot/dashboard_v2/src/index.css:65 : `--gradient-bg:linear-gradient(180deg,#0f0f0e 0%,#0b0b0b 55%,#101010 100%)`, identisch zur Vorlage.
- bot/dashboard_v2/src/index.css:66 : `--gradient-hero:linear-gradient(160deg,#0b0b0b 0%,#101010 55%,#161616 100%)`, identisch zur Vorlage.
- bot/dashboard_v2/src/index.css:27-48 : Gold/Messing-Akzente und Statusfarben (`--color-primary:#C5A059`, `--color-accent:#E0BE86`, `--color-success:#43b581`, `--color-warning:#E8A33D`, `--color-danger:#FF5A3C`) bleiben (REQ-03/05).

## Dashboard-Ist: warme Flächenquellen (müssen für REQ-01 weg)

- bot/dashboard_v2/src/index.css:86-89 : Body-Hintergrund, drei goldene `radial-gradient` (rgba(197,160,89,0.16) / rgba(224,190,134,0.08) / rgba(197,160,89,0.09)).
- bot/dashboard_v2/src/index.css:91-99 : Body zusätzlich `repeating-linear-gradient` Holzmaserung (rgba(241,210,153,0.012) / rgba(0,0,0,0.09)) über `var(--gradient-bg)`.
- bot/dashboard_v2/src/index.css:105-117 : Raster `body::before`, goldene Linien `rgba(197,160,89,0.05)`, `background-size:36px 36px`, `mask-image:radial-gradient(ellipse at top, black 40%, transparent 75%)`, `opacity:0.35`.
- bot/dashboard_v2/src/index.css:121-129 : `body::after`, statische SVG-fractalNoise-Körnung, `opacity:0.028` (Vorlage hat keine Körnung).
- bot/dashboard_v2/src/index.css:609-630 : `.internal-home-vibe::before`, vier goldene `radial-gradient` (rgba(197,160,89,0.18) u.a.) plus 36s-Animation `internal-home-gradient-flow`. Zweite, stärkere warme Aura-Ebene über dem Body.
- bot/dashboard_v2/src/index.css:631-680 : `.internal-home-vibe::after`, weitere goldene `radial-gradient`-Auren plus weißes 44px-Gridnetz plus Vignette (rgba(11,11,11,...)), `opacity:0.96`.
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx:6-12 : `BackgroundBlobs()`, drei farbige Weichzeichner `bg-primary/22`, `bg-accent/24`, `bg-success/20` mit `blur-3xl`.
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx:30 : `<BackgroundBlobs />` wird in der Shell gerendert.
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx:29 : Wrapper trägt Klasse `internal-home-vibe` (Klassenname muss bleiben, s. Test).

## Dashboard-Ist: Kartenmaterial (müssen für REQ-02 flach werden)

- bot/dashboard_v2/src/index.css:194-209 : `.glass`, `background-image:repeating-linear-gradient(177deg,...)` Gusseisen-Streifen, `backdrop-filter:blur(14px)`, dreiteiliger `box-shadow` mit zwei Bevel-Insets (`inset 0 1px 0 rgba(241,210,153,0.12)`, `inset 0 -1px 0 rgba(0,0,0,0.5)`).
- bot/dashboard_v2/src/index.css:223-242 : `.panel-card`, `background-image` = `linear-gradient(158deg, rgba(241,210,153,0.05)...)` Lichtabfall plus `repeating-linear-gradient(177deg,...)` Schleifspuren; `box-shadow` mit zwei Bevel-Insets; kein eigener `border-radius` (Radius kommt aus Tailwind-`rounded-*` am Element).
- bot/dashboard_v2/src/index.css:245-256 : `.panel-card::after`, vier `radial-gradient`-Nieten in den Ecken (`#F1D299`/`#9A7C42`-Punkte), `opacity:0.6`.
- bot/dashboard_v2/src/index.css:259-303 : `.card-glow` inkl. `::before`-Goldkante (color-mix aus `--color-primary`/`--color-accent`) und Hover-Lift; bleibt erhalten (REQ-03).

## Token-Quelle und Tailwind-Mapping

- bot/dashboard_v2/src/index.css:18 : `@theme{...}`, die Farb-Tokens werden per Tailwind-v4-`@theme` in index.css gepflegt; daraus mappen `bg-card`, `border-border`, `text-text-secondary` automatisch.
- bot/dashboard_v2/tailwind.config.js und .ts : leer bzw. ohne Farbdefinition (cat liefert keinen Inhalt), kein Farb-Mapping dort, nur `@theme` in index.css ist maßgeblich.
- bot/dashboard_v2/src/main.tsx:3 : einzig geladenes globales Stylesheet ist `./index.css`.
- bot/dashboard_v2/src/ddc-design-tokens.css:14-18 : definiert eigene WARME Tokens (`--color-bg:#140D0A`, `--color-card:#1F1815`), wird aber nirgends importiert (nur index.css und uplinkHelp.css sind eingebunden), also tote Datei ohne Wirkung auf das Dashboard.
- bot/dashboard_v2/src/pages/Uplink.tsx:16 : `import '../uplinkHelp.css'`, uplinkHelp.css ist nur im Uplink-Hilfetext aktiv, eigene dunklere Gold-Palette, kein Flächen-Theme.

## Weitere harte braun/gold-getönte Flächen in der Shell

- bot/dashboard_v2/src/components/layout/DashboardSidebar.tsx:53 : aktiver Eintrag `bg-primary/10 text-primary border-primary/25` (Gold-Akzent, bleibt REQ-03).
- bot/dashboard_v2/src/components/layout/DashboardSidebar.tsx:229,255,266 : `bg-background/60 border-border` Flächen (laufen über Tokens, folgen automatisch).
- bot/dashboard_v2/src/components/layout/Header.tsx:95 : `bg-gradient-to-br from-primary/30 to-accent/25 border-primary/25` (Gold-Kachel, Akzent, bleibt).
- bot/dashboard_v2/src/components/layout/Header.tsx:117,145 : `bg-background/70 border-border` (über Tokens).

## Relevante Tests (laufen vorher, laufen nachher)

- bot/dashboard_v2/tests/brandPalette.test.ts:21-49 : `ALLOWED_HEX`-Whitelist; enthält bereits `#0b0b0b`,`#101010`,`#0f0f0e`,`#161616`,`#f2eee6`,`#9d968a` (Zeilen 30-32). Prüft nur `#rrggbb`-Literale, nicht `rgba(...)`.
- bot/dashboard_v2/tests/brandPalette.test.ts:68-79 : Test rot, sobald irgendein neues `#hex` außerhalb der Whitelist im Code steht.
- bot/dashboard_v2/tests/brandPalette.test.ts:81-92 : verbietet Tailwind-Standardpaletten (slate/gray/...).
- bot/dashboard_v2/tests/brandPalette.test.ts:94-160 : kein weißer Text auf heller Markenfläche (Gold/Plasma); prüft Zeilen mit `gradient-accent`/`bg-primary` etc. gegen `text-white`.
- bot/dashboard_v2/tests/scoreColors.test.ts:1-4 : testet `getScoreColor`/`getRetentionColor` aus einer Scoring-Util, nicht von CSS-Flächen betroffen.
- bot/dashboard_v2/tests/dashboardShell.test.ts : prüft, dass die Shell die Klasse `internal-home-vibe`, `max-w-[2200px]`, `lg:grid-cols-[220px_minmax(0,1fr)]`, `<DashboardSidebar activeRoute={activeRoute} />` und den Main-Slot trägt; verlangt zusätzlich, dass keine Seite `internal-home-vibe` selbst setzt.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- bot/dashboard_v2/src/components/layout/DashboardShell.tsx:29 : Klassenname `internal-home-vibe` ist Vertrag mit dashboardShell.test.ts, darf nicht entfernt werden, nur die Pseudo-Element-CSS wird neutralisiert.
- bot/dashboard_v2/src/index.css:18 : `@theme`-Token-Namen sind Vertrag mit allen Komponenten (bg-card, border-border, text-text-secondary), nur Werte ändern.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- bot/dashboard_v2/src/index.css : Body-Hintergrund, Raster, `.internal-home-vibe::before/::after`, `.glass`, `.panel-card`, `.panel-card::after`, Border-Alpha-Tokens.
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx : `BackgroundBlobs` entfernen, Klasse `internal-home-vibe` behalten.

## Offene Architekturfrage

- keine

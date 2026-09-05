# Evidence: Dashboard-Karten heben sich vom Hintergrund ab

status: aktiv
datum: 2026-09-05
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- bot/dashboard_v2/src/index.css:25 — `--color-card: rgba(20, 20, 20, 0.82)` liegt fast auf `--color-bg: #0b0b0b` (Zeile 23); Ursache des Verschmelzens.
- bot/dashboard_v2/src/index.css:225 — `.panel-card` legt zusätzlich einen dunklen Verlauf (rgba(0,0,0,0.18) bis 0.3) über die Karte und drückt sie weiter ins Schwarz.
- bot/dashboard_v2/src/index.css:756 — `.internal-home-log-card` nutzt bereits einen eigenen Verlauf (rgba(28,28,28) nach rgba(17,17,17)); Muster für Verlauf auf Karten.
- bot/dashboard_v2/tests/brandPalette.test.ts:20 — erlaubte Hex-Werte, darunter warme Töne #1a1310, #221a15, #2a221c, #362c23; alles andere blockt der Test.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- bot/dashboard_v2/src/index.css:18 — Tailwind-`@theme`-Block; `bg-card` (129 Nutzungen in pages/components) und `border-border` (140) hängen an `--color-card` und `--color-border`.
- bot/dashboard_v2/src/index.css:194 — `.glass` und `.panel-card` sind die zentralen Kachel-Klassen, tragen Bevel (inset-Schatten) und Schlagschatten.
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx:6 — `BackgroundBlobs` (drei geblurrte Gold/Messing/Grün-Kreise) und Wrapper `.internal-home-vibe` (index.css:580, ::before 609, ::after 631) erzeugen den Gold-Schein im Hintergrund.
- bot/dashboard_v2/src/index.css:86 — `body`-Hintergrund mit drei radialen Gold-Gradienten plus `--gradient-bg`; index.css:104 `body::before` ist das Raster (36px, Gold 0.05, radial maskiert, Opacity 0.35).

## Relevante Tests (laufen vorher, laufen nachher)

- bot/dashboard_v2/tests/brandPalette.test.ts:20 — Hex-Allowlist für alle Quelldateien.
- bot/dashboard_v2/tests/dashboardShell.test.ts:33 — Shell wickelt alle sieben Routen, prüft max-w-Klassen.
- bot/dashboard_v2/package.json `test`-Skript — Gesamtlauf per `node --test`.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- bot/dashboard_v2/src/components/layout/DashboardShell.tsx:17 — Props `activeRoute`, `demoMode`, `showSidebar`, `children` bleiben.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- bot/dashboard_v2/src/index.css — Tokens, body-Hintergrund, Raster, `.glass`, `.panel-card`, `.internal-home-vibe`-Pseudoelemente.
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx — `BackgroundBlobs` entfernen.

## Offene Architekturfrage

- keine

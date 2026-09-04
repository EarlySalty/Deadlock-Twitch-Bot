# Contract: Schwarz und Kacheln der Streamer-Landingpage ins Dashboard übernehmen

status: aktiv
datum: 2026-09-04
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Das Streamer-Dashboard (alle Routen der gemeinsamen `DashboardShell`) sieht farblich aus wie die öffentliche Landingpage `/streamer`: derselbe neutrale Schwarz-Hintergrund mit demselben feinen Raster statt des warmen, braun-goldenen Schimmers, und Karten in derselben flachen, dunklen Kachel-Optik mit feiner Kante wie die Kacheln auf `/streamer` (Vorlage: `vorlage-streamer.png` in diesem Ordner, Referenz die zwei Stream-Kacheln und die Kopfzeile).

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Der Seitenhintergrund des Dashboards (Shell-Rahmen, Klasse `internal-home-vibe` und Body) verwendet exakt die Hintergrund-Farbwerte und das Raster der Landingpage `/streamer` (Werte aus dem Website-Stylesheet, das `/streamer` tatsächlich lädt, per Fundstelle belegt). Die farbigen Weichzeichner-Blobs (`BackgroundBlobs`: primary, accent, success) und jede braun-goldene Grundtönung entfallen.
- REQ-02: Karten (`panel-card`, `glass` und die davon abgeleiteten Kartenklassen) übernehmen Kachel-Hintergrund, Kantenfarbe, Radius und Schatten der `/streamer`-Kacheln: flache dunkle Fläche, feine helle Kante, ohne die bisherigen Niet-Punkte (`panel-card::after`), Gusseisen-Streifen und Bevel-Verläufe.
- REQ-03: Goldene Akzente (Primärfarbe, Badges, aktive Sidebar-Einträge, Buttons, Verlaufstexte) bleiben erhalten; nur Flächen und Kanten ändern sich. Der Hover-Glow (`card-glow`) bleibt, in der Kantenfarbe der Vorlage.
- REQ-04: Die Änderung wirkt auf allen Shell-Routen gleich (Home, Analyse, Social Media, Uplink, Verwaltung, Stream-Overlay, Preise) und auf der Sidebar selbst; kein Bereich behält den alten warmen Look.
- REQ-05: Lesbarkeit bleibt: Textfarben (`text-secondary`, Weiß) und Statusfarben (success, warning, danger) behalten mindestens den bisherigen Kontrast auf den neuen Flächen.
- REQ-06: Sichtprüfung als Screenshot-Serie derselben sieben Preview-Routen wie in `.tasks/2026-09-04-dashboard-shell/screens/`, abgelegt unter `screens/` in diesem Ordner, plus ein Vergleichsbild neben `vorlage-streamer.png`.

## Invarianten (darf sich nicht ändern)

- INV-01: Kein Fachinhalt, keine Komponentenstruktur, keine Routen; nur Stylesheet-Tokens, Kartenklassen und die Hintergrund-Komponente der Shell.
- INV-02: Die Landingpage `website/` und ihr Stylesheet werden nur gelesen, nicht verändert.
- INV-03: Bestehende Tests unter `bot/dashboard_v2/` bleiben grün und werden nicht abgeschwächt.
- INV-04: Keine neuen Abhängigkeiten, keine Änderung an Tailwind- oder Vite-Konfiguration außer Farb-Tokens, falls diese in `tailwind.config` gepflegt werden (dann nur Werte, keine Struktur).
- INV-05: Keine Code-Kommentare; bestehende Kommentare in angefassten Blöcken entfallen.

## Nicht-Ziele

- Typografie, Abstände, Layout oder Sidebar-Struktur ändern.
- Die Landingpage selbst anfassen.
- Admin-Dashboard (`bot/admin_dashboard/`) umstylen.

## Erlaubter Änderungsbereich

- bot/dashboard_v2/src/index.css
- bot/dashboard_v2/src/App.css
- bot/dashboard_v2/src/ddc-design-tokens.css
- bot/dashboard_v2/src/uplinkHelp.css
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx
- bot/dashboard_v2/src/components/layout/DashboardSidebar.tsx
- bot/dashboard_v2/tailwind.config.js
- bot/dashboard_v2/tailwind.config.ts
- bot/dashboard_v2/tests/
- bot/dashboard_v2/package.json
- .tasks/2026-09-04-dashboard-schwarz-kacheln/

## Verbotene Änderungen

- website/
- rust/
- bot/admin_dashboard/
- bot/dashboard_v2/src/pages/
- bot/dashboard_v2/src/api/
- bot/dashboard_v2/vite.config.ts

## Offene Produktfragen

- keine

## Amendments


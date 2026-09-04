# Contract: Einheitliche Dashboard-Shell (Sidebar und Rahmen) für alle Twitch-Dashboards

status: erledigt
datum: 2026-09-04
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Jede Seite des Streamer-Dashboards (Home, Analyse, Social Media, Uplink, Verwaltung, Stream-Overlay, Preise) zeigt dieselbe linke Sidebar wie heute die Home-Seite `/twitch/dashboard` und denselben äußeren Rahmen (Breite, Abstände, Hintergrund), sodass der Wechsel zwischen den Seiten keinen sichtbaren Layout-Sprung erzeugt.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Es gibt genau eine Shell-Komponente (Sidebar plus Content-Rahmen) unter `bot/dashboard_v2/src/components/layout/`, und alle sieben Routen (`/twitch/dashboard`, `/analyse` und die Analytics-Aliasse, `/social-media-admin`, `/twitch/uplink`, `/twitch/verwaltung`, `/twitch/overlay`, `/twitch/pricing`) rendern ihren Inhalt in dieser Shell.
- REQ-02: Die Sidebar ist auf jeder Route inhaltsgleich mit der heutigen Home-Sidebar: Profilkopf (Avatar, Login, Plan-Badge), Gruppe „Main" (Home, Analyse, Social Media Dashboard, Uplink), Gruppe „Tools" (Verwaltung, Stream-Overlay, Plan, Changelog), Gruppe „Admin" (Admin-Modus-Schalter plus Hinweistext), Gruppe „Hilfe" (FAQ & Hilfe, Tour neu starten). Der zur aktuellen Route passende Eintrag ist als aktiv markiert.
- REQ-03: Rahmenmaße sind auf allen Routen identisch: dieselbe maximale Gesamtbreite, dieselbe Sidebar-Spaltenbreite, dieselben Außenabstände und derselbe Hintergrund (`internal-home-vibe`). Keine Seite setzt im Hauptbereich eine eigene, abweichende Gesamt-Maximalbreite oder eigene Außenabstände.
- REQ-04: Auf schmalen Viewports (unter der lg-Grenze) verhält sich die Sidebar auf allen Routen gleich (dasselbe Verhalten, das die Home-Sidebar heute hat).
- REQ-05: Die Analyse-Seite behält ihre horizontale Tab-Navigation und ihren Seitenkopf innerhalb des Hauptbereichs; Tab-Wechsel und Deep-Links (`analyticsTabHref`) funktionieren unverändert.
- REQ-06: Die Seitenkopf-Zeile jeder Route (Eyebrow, Titel, Untertitel, Aktionsknöpfe) nutzt dieselbe Typografie- und Karten-Klassen wie der Home-Kopf („Willkommen zurück"-Karte).
- REQ-07: Sidebar-Links, Admin-Modus-Schalter, FAQ-Link und „Tour neu starten" funktionieren von jeder Route aus so wie heute von Home aus (gleiche Ziele, gleiche Zustandsspeicherung).
- REQ-08: Der Prüfer-Login `/twitch/auth/google` und die Shell-Gates in `spa.rs` bleiben unberührt; die Shell ist rein clientseitig.

## Invarianten (darf sich nicht ändern)

- INV-01: Fachinhalt der Seiten (Karten, Formulare, Tabellen, Daten-Hooks, API-Aufrufe) bleibt inhaltlich unverändert; verschoben wird nur die Hülle.
- INV-02: Route-Konstanten in `bot/dashboard_v2/src/preview/routes.ts` und `tabAliases.ts` bleiben kompatibel; keine URL ändert sich.
- INV-03: Bestehende Tests (`Uplink.layout.test.tsx` und alle anderen unter `bot/dashboard_v2/`) werden nicht gelöscht oder abgeschwächt; sie dürfen nur an die neue Shell-Struktur angepasst werden, wenn der Test die Hülle und nicht den Fachinhalt prüft.
- INV-04: Demo-Modus, Sprachwahl (`LanguageProvider`), Fehlergrenze und Auth-Status-Fluss in `App.tsx` bleiben funktional gleich.
- INV-05: Kein Rust-Code, keine Migration, keine Caddy-Änderung.
- INV-06: Keine Code-Kommentare; bestehende Kommentare in angefassten Dateien dürfen entfallen.

## Nicht-Ziele

- Neue Sidebar-Einträge oder neue Seiten.
- Umbau des Fachinhalts einzelner Seiten (z. B. Übersetzung der englischen Social-Media-Texte, neue Karten).
- Änderungen an der öffentlichen Streamer-Landingpage (`website/`).
- Änderungen am Admin-Dashboard (`bot/admin_dashboard/`).

## Erlaubter Änderungsbereich

- bot/dashboard_v2/src/App.tsx
- bot/dashboard_v2/src/App.css
- bot/dashboard_v2/src/index.css
- bot/dashboard_v2/src/components/layout/
- bot/dashboard_v2/src/pages/InternalHomeLanding.tsx
- bot/dashboard_v2/src/pages/Uplink.tsx
- bot/dashboard_v2/src/pages/Uplink.layout.test.tsx
- bot/dashboard_v2/src/pages/SocialMediaAdmin.tsx
- bot/dashboard_v2/src/pages/Verwaltung.tsx
- bot/dashboard_v2/src/pages/OverlayBuilder.tsx
- bot/dashboard_v2/src/pages/Pricing.tsx
- bot/dashboard_v2/src/hooks/
- .tasks/2026-09-04-dashboard-shell/

## Verbotene Änderungen

- rust/
- bot/admin_dashboard/
- website/
- bot/dashboard_v2/src/preview/routes.ts
- bot/dashboard_v2/src/api/
- Lint-, Vite- und Tailwind-Konfiguration

## Offene Produktfragen

- keine

## Amendments

- 2026-09-04, Scope: bot/dashboard_v2/package.json (nur scripts.test um tests/dashboardShell.test.ts erweitert), Grund: expliziter Test-Runner, entschieden von Orchestrator (technisch, reversibel)


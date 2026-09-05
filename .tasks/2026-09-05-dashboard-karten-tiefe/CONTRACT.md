# Contract: Dashboard-Karten heben sich vom Hintergrund ab

status: aktiv
datum: 2026-09-05
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Die Karten des Partner-Dashboards (dashboard_v2) lesen sich als warme, erhabene Flächen auf dem neutralen schwarzen Rasterhintergrund, statt mit ihm zu verschmelzen.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Die Kartenfläche (`--color-card`, damit `bg-card`, `.panel-card`, `.glass`) ist deckend und warm braun-gold, als Verlauf von oben links hell nach unten rechts dunkel, und nutzt nur Hex-Werte aus der erlaubten Palette in `tests/brandPalette.test.ts` (etwa #2a221c, #221a15, #1a1310, #362c23).
- REQ-02: Jede Karte hat eine sichtbare Kante zum Grund: kräftigere Gold-Rahmenlinie (`--color-border` mindestens 0.32 Alpha), 1px helle Innenkante oben, tiefer Schlagschatten nach unten und ein dunkler 1px-Außenring, damit die Kante auch über den Rasterlinien liest.
- REQ-03: Der Seitenhintergrund bleibt neutrales Schwarz mit Raster. Gold-Schein-Flächen im Hintergrund fallen weg: die radialen Gold-Gradienten am `body`, die `BackgroundBlobs` in `DashboardShell.tsx` und die `.internal-home-vibe::before/::after`-Gradienten. Das Raster darf dafür etwas kräftiger und über die ganze Fläche sichtbar werden, damit der Grund nicht ins Dunkle abdriftet.
- REQ-04: Innenkacheln in Karten (heute `bg-background/40`, `bg-white/5`) bleiben dunkler als die Kartenfläche, sodass die Hierarchie Grund < Karte > Innenkachel erhalten bleibt.
- REQ-05: Alle sieben Routen (home, analyse, social, uplink, verwaltung, overlay, pricing) und die Sidebar zeigen denselben Karten-Look; kein Dashboard sieht anders aus als das andere.

## Invarianten (darf sich nicht ändern)

- INV-01: Farb-Tokens werden nur in `bot/dashboard_v2/src/index.css` (`@theme`) geändert; keine neuen Hex-Werte außerhalb der Palette aus `tests/brandPalette.test.ts`, kein Cyan/Blau als Fläche.
- INV-02: Keine Änderung an Layout, Abständen, Sidebar-Breite (220px), Komponentenstruktur oder Routen; `tests/dashboardShell.test.ts` bleibt grün.
- INV-03: Bestehende Tests werden nicht gelöscht oder abgeschwächt; `npm test` und `npm run build` im Paket `bot/dashboard_v2` grün.
- INV-04: Kein Glow an Avatar oder Icons, kein Gold-Schein im Hintergrund.

## Nicht-Ziele

- Kein Redesign von Typografie, Charts, Sidebar-Navigation oder Texten.
- Kein Umbau des admin_dashboard, der Website oder des shared-theme.
- Keine Rust- oder API-Änderung.

## Erlaubter Änderungsbereich

- bot/dashboard_v2/src/index.css
- bot/dashboard_v2/src/App.css
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx
- bot/dashboard_v2/tests/dashboardShell.test.ts
- bot/dashboard_v2/tests/brandPalette.test.ts
- .tasks/2026-09-05-dashboard-karten-tiefe/

## Verbotene Änderungen

- bot/shared-theme/
- bot/admin_dashboard/
- website/
- rust/
- Alle Seiten unter bot/dashboard_v2/src/pages/ (Look kommt aus den Tokens, nicht aus Seiten-Edits)

## Offene Produktfragen

- keine

## Amendments

- 2026-09-05 | Erlaubter Änderungsbereich | alt: ohne tests/dashboardLook.test.ts -> neu: plus bot/dashboard_v2/tests/dashboardLook.test.ts | Grund: der Test nagelt Border-Alpha 0.22, Overlay 158deg und Raster 0.045 fest und widerspricht REQ-01 bis REQ-03; zwei Assertions werden auf den neuen Look ausgerichtet, nichts gelöscht | entschieden von Orchestrator (nur technisch, reversibel)
- 2026-09-05 | Erlaubter Änderungsbereich | alt: src/pages gesperrt -> neu: plus bot/dashboard_v2/src/pages/SessionDetail.tsx, nur die Tab-Leiste Zeilen 238-252 | Grund: die inaktiven Tabs tragen rounded-xl plus bg-card und matchen den Karten-Selektor; eine weitere Selektor-Ausnahme in index.css wäre die dritte Krücke, die Tabs auf den Stil der anderen Tab-Leisten (SubTabs) umzustellen ist die Ursache | entschieden von Orchestrator (nur technisch, reversibel)

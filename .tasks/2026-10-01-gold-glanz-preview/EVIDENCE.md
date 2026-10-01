# Prüfnachweise

Basis: origin/main 14bc1f47.

- npm run build:preview: TypeScript und Vite erfolgreich. Vorhandene Hinweise zu __dirname und Bundle-Größe; lokale Schriftdateien liegen dem Preview bei.
- 48 gezielte bestehende Tests: 44 erfolgreich, 4 Fehler. Dieselben 4 Tests scheitern bereits auf dem unveränderten Basiscommit im separaten, anschließend entfernten Baseline-Worktree. Logs: tests.tap und baseline-tests.tap.
- Vorhandene Fehler: fremde Hexwerte im Overlay, Tailwind-Standardfarben, weiße Texte auf Markenflächen, Gesamtbreite im Analytics-Rahmen. Diese liegen außerhalb des Vorschau-Auftrags.
- Dark Ink #1a130c gegen alle Gold-Verlaufsstufen: minimal 5,16:1.
- Vite Preview läuft als systemd-User-Service tb-gold-glanz-preview.service ausschließlich auf Tailnet-IP 100.117.29.112:4187. Host v50671-kde liefert HTTP 200.
- Browser-Skill: keine verbundene Instanz verfügbar. Einmaliger Headless-Chromium-Versuch erfolgreich; keine Flags verändert. Playwright Core 1.63.0, vorhandener Chromium 1243.
- Automatische Screenshots und Browser-Prüfungen: capture.mjs; Ergebnis browser-checks.txt.

- Alle acht Desktop-Kombinationen und beide Mobilansichten ohne Browserfehler und Seitenüberlauf. Status-, Chart- und Markentokens sowie Seitenhintergrund identisch; acht Desktop-Kombinationen ohne API-Aufrufe.
- Zeitraum, Tage-Enter, Sprachwechsel, Hilfe öffnen und Variantenerhalt bei Home-Navigation erfolgreich; interaction-checks.txt.
- Statische Vorschau rekonstruieren: npm run build:preview im Dashboard; /usr/local/bin/node node_modules/vite/bin/vite.js preview --mode preview --host 100.117.29.112 --port 4187 --strictPort.
- Screenshot-Werkzeug: npm install --prefix /tmp/tb-gold-tools playwright-core@1.63.0; node .tasks/2026-10-01-gold-glanz-preview/capture.mjs im Worktree.

## Revision 2

- Nutzerentscheidung aus NUTZERENTSCHEIDUNG-V2.md umgesetzt: Lichtkamm und helle Bänder vollständig entfernt, Champagner als Variante entfernt.
- Zwei ruhige Verläufe mit je drei Tönen an 0 %, 50 % und 100 %; keine lokalen Helligkeitsspitzen. Innenkanten abgeschwächt.
- npm run build:preview erfolgreich; build-preview-v2.txt.
- Sieben vorhandene Gestaltungstests erfolgreich; look-tests-v2.txt.
- Neuer Screenshot-Satz in screenshots-v2/; Browsernachweis browser-checks-v2.txt.
- Minimaler Kontrast zwischen Text #1a130c und den sechs Verlaufstönen: 5,56:1.
- Implementierungscommit 8d2eb5a4. Keine Änderung an Markentokens, Statusfarben, Diagrammen oder neutralem Hintergrund.

# Social Studio: Integration der freigegebenen Gold-Oberfläche

## Auftrag

Die im Chat freigegebene Seitenstruktur in das bestehende Social-Media-Dashboard übernehmen. Bestehendes Industrial-Gold-Design, Deadlock-D-Logo und Manrope/Sora verwenden. Die echten API-Aufrufe bleiben erhalten; die Vorschau mit Beispieldaten wird nicht als Produkt eingebaut.

## Umsetzung

`/social-media-admin?streamer=earlysalty` enthält Pipeline, Auto-Pilot & Zeitplan, Templates & Layouts sowie Konten & Einstellungen. Die bestehende globale Navigation bleibt mit ihren Berechtigungen erhalten und klappt auf kleinen Bildschirmen ein. Das Theme ist auf diese Route begrenzt.

Die Pipeline lädt den kanalgebundenen Bestand über die vorhandene Pagination. Statusfilter, Suche und Plattformfilter arbeiten auf diesem Bestand. Geplante Posts zählen Plattformtermine statt Clips. Ein fehlgeschlagener oder unvollständiger Abruf zeigt einen Fehler statt eines leeren Bestands. Die sichtbare Liste hat 24 Einträge pro Seite; die Kartenansicht verwendet dieselben Daten. Der Vorschau-Renderer startet durch eine ausdrückliche Aktion und wird nicht beim Laden jeder Karte abgefragt.

Freigabe und Ablehnen verwenden die bestehenden Entscheidungen. Terminale Clip-Zustände werden nicht durch einen alten Freigabedatensatz überschrieben. Layout und Transkript öffnen die vorhandenen Editoren in einem fokussierten Dialog. Ein Speicherfehler hält den Layout-Dialog offen. Die Standardvorlagen bleiben bearbeitbare Layout-Daten und lösen erst beim Speichern eine Änderung aus.

Der Auto-Pilot hält einen lokalen Entwurf bis zum Speichern. Da der bestehende Serververtrag mehrere Schreibendpunkte hat, ist das Speichern keine atomare Transaktion: bestätigte Teiländerungen werden übernommen, der gewünschte Entwurf bleibt bei Fehlern stehen. Ein erneuter Abruf gleicht den Serverstand ab. Ist auch dieser Abruf fehlgeschlagen, muss der Stand vor einem weiteren Schreibversuch erfolgreich geladen werden. Kanal- und Tabwechsel warnen vor ungespeicherten Änderungen; Enter übernimmt zunächst das bearbeitete Feld. Es wurden keine zusätzlichen Zeitpuffer- oder Filter-APIs erfunden.

## Nachweise vor dem Merge

Basis: `1327f414ffb8ccb1988276c003cb30f2dbfbfba8`. Der zu Beginn laufende Release `8a1d6bc4bc1aca3a8388f3e3a1bb4388bb5fd56e` ist ein Vorfahr; dessen Rust-Quellen unterscheiden sich nicht von der Basis.

- TypeScript und Vite-Produktionsbuild erfolgreich. ESLint auf den geänderten Komponenten und dem API-Modul ohne Fehler oder Warnungen.
- 49 gezielte Tests erfolgreich, darunter sechs neue Tests für Status, Kennzahlen, vollständige Pagination, Filter und Abbruch. Erste Rotprobe: fehlendes neues Queue-Modul, ein fehlgeschlagener Testdatei-Lauf.
- Zehn Browser-Prüfgruppen am gebauten Produktbundle erfolgreich, vom Node-Runner als elf Tests gezählt. Geprüft wurden Freigabe-Payloads, vollständige Bestandsabfrage, Dialogfokus und Layoutfehler, Teilfehler beim Speichern, Enter, ungültige Eingaben, Vorlagen und Kanalwechsel. Vier Bereiche bei 320, 390, 768 und 1440 Pixeln ohne Dokumentüberlauf. Schreibaufrufe laufen gegen einen isolierten Testserver, nicht gegen Produktionskonten.
- Gesamtsuite: neun Kalenderprüfungen erfolgreich; anschließend 384 von 389 Tests erfolgreich. Die fünf Fehler sind bereits in einem unveränderten Basis-Worktree vorhanden (378 von 383 erfolgreich): drei globale Palettenprüfungen, Sidebar-Skeleton und Uplink-OBS-Hilfetext. Die Prüfungen wurden nicht abgeschwächt.

## Reproduktion

Im Verzeichnis `bot/dashboard_v2`:

```sh
npm run build
node --import tsx --test tests/socialStudioRedesign.test.ts tests/socialMediaContract.test.ts tests/socialMediaLayout.test.ts tests/zeitplanFormular.test.ts tests/dashboardShell.test.ts
node --test tests/socialStudio.browser.test.mjs
```

Für Chromium kann `STUDIO_BROWSER` den lokalen Browserpfad überschreiben. `STUDIO_PLAYWRIGHT_MODULE` erlaubt einen getrennt installierten Playwright-Core-Runner. Die Browser-Fixtures erzeugen synthetische Daten; Produktionsfreigaben werden damit nicht ausgelöst.

## Release

Das Frontend liegt nach dem Build unter `bot/analytics/dashboard_v2/dist`. Der gehärtete Installer verlangt auch für eine Frontend-Änderung neu gebaute Rust-Binaries mit eingebetteter Commit-Herkunft. Der Release wird deshalb aus dem gemergten Commit über den bestehenden Installer gebaut, nicht in einen alten Release hinein kopiert. Merge- und Live-Nachweise werden mit den tatsächlich ausgeführten Schritten protokolliert.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Task-Akte

# Prüfungen der Übergabe

Diese Prüfungen betreffen die Referenz aus `../reference`, nicht das echte Dashboard.

## Ausführen

Zuerst die Referenz mit `npm ci --ignore-scripts` und `npm run build` in `../reference` bauen. Dann in diesem Ordner:

```bash
npm ci --ignore-scripts --no-audit --no-fund
npm run check
npm test
```

Der Browserlauf benötigt einen installierten Chromium. `BROWSER_PATH` kann dessen ausführbare Datei angeben. Andernfalls werden übliche Linux-Pfade und vorhandene Playwright-Browser unter `~/.cache/ms-playwright` geprüft. Das Paket lädt keine Browser automatisch. `--no-sandbox` wird für den lokalen Headless-Test verwendet; geladen wird der offline vorliegende Referenz-HTML-Text, nicht eine Produktionsseite.

`verify-sources.mjs` gleicht 28 importierte Quelldateien sowie zwei gebaute Artefakte mit den Prüfsummen aus dem Chat-Paket ab. Es prüft zusätzlich die Übergabedateien auf eingebundene Fonts, Umgebungsdateien, private Schlüssel und ausgewählte Credential-Muster. Das ist eine begrenzte Veröffentlichungsprüfung, kein vollständiger Security-Audit.

`browser-check.mjs` überträgt den ursprünglichen Vorschau-Browsertest auf den Node-/Playwright-Stack. Er blockiert Netzwerkzugriffe, verwendet Demodaten und erstellt gesettelte Viewport-Aufnahmen unter `screenshots/`.

## Aktuelle Ergebnisdateien

- `source-results.json`: Quellenabgleich und begrenzte Veröffentlichungsprüfung.
- `browser-results.json`: Browser-Prüfgruppen, tatsächliche Breiten, JavaScript-Fehler und Netzwerkzähler.
- `screenshots/preview-desktop.png`: Pipeline auf Desktop.
- `screenshots/preview-autopilot.png` und `preview-autopilot-bottom.png`: Zeitplan als Viewport-Serie.
- `screenshots/preview-templates.png`: Layoutauswahl.
- `screenshots/preview-editor.png`: schematischer Editor, keine Medienverarbeitung.
- `screenshots/preview-accounts.png`: Beispielkonten.
- `screenshots/preview-mobile.png` und `preview-mobile-cards.png`: mobile Kopfansicht und Clip-Bereich.

Die Bilder sind auf dem Projekthost neu gerenderte Referenzaufnahmen. Sie enthalten Beispieldaten und zeigen eine Ersatzschrift. Sie belegen weder Produktfonts noch einen Live-Deploy.

Die ursprünglichen Dateien `../reference/tests/browser-results.json` und `syntax-results.json` bleiben aus Gründen der Herkunft erhalten. Der JSX-Syntaxlauf wurde während dieser Übergabe mit dem vorhandenen TypeScript des Dashboard-Checkouts erneut ausgeführt; sieben JSX-Dateien wurden geparst und transpiliert. Das mountet keine React-App und prüft keine echte API-Anbindung.

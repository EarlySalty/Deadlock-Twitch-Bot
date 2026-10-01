# Social-Media-Redesign: Übergabe an den Integrationsagenten

## Hier anfangen

Der Nutzer möchte die Arbeit aus dem Chat ohne Datei-Uploads weitergeben. Dieser Branch enthält den Auftrag und die Industrial-Gold-Referenz als lesbare Quelldateien. Er ist eine Übergabe, keine fertige Produktintegration.

**Auftrag:** [AUFTRAG.md](AUFTRAG.md)

**Branch:** `docs/social-studio-handoff-20260921`

**Ziel:** `/social-media-admin?streamer=earlysalty` im Repo `EarlySalty/Deadlock-Twitch-Bot`.

Die ursprüngliche neue Seitenstruktur gefiel dem Nutzer. Seine Korrektur betraf Farben, Stil und Logo: bestehendes Community-Design statt einer generischen Indigo-/Zinc-Marke. Die beigefügte Gold-Revision ist die daraus entstandene Referenz. Für die Produktintegration bleiben die aktuellen Marken-Tokens des Repos maßgeblich.

## Material

| Datei | Inhalt |
| --- | --- |
| [AUFTRAG.md](AUFTRAG.md) | Vollständiges Integrationsbriefing mit Abnahmekriterien, API-Fallen und Abschluss bis zum Live-Nachweis |
| [reference/preview.html](reference/preview.html) | Selbstständige interaktive Vorschau mit Demodaten; lokal im Browser öffnen |
| [reference/README.md](reference/README.md) | Ursprüngliche Paketbeschreibung |
| [reference/BRAND.md](reference/BRAND.md) | Markenquellen, Logo-Herkunft und Schrift-Hinweise |
| [reference/INTEGRATION.md](reference/INTEGRATION.md) | Komponentenverträge und Zuordnung zu bestehenden APIs |
| [reference/src/react](reference/src/react) | Shell, Clip-Karte, Zeitplan, Community-Marke und Dialog |
| [reference/src/styles.css](reference/src/styles.css) | Gestaltung und responsive Komponentenstile |
| [reference/src/brand-theme.css](reference/src/brand-theme.css) | Semantische Referenztokens |
| [reference-manifest.json](reference-manifest.json) | Prüfsummen der unveränderten importierten Quelldateien |
| [EVIDENCE.md](EVIDENCE.md) | Aktuelle Übergabeprüfungen und Grenzen des Nachweises |
| [checks](checks) | Browserprüfung und Referenzbilder aus dem auf dem Projekthost gebauten Entwurf |

Die Bilder unter `checks/screenshots/` werden auf dem Projekthost aus der übertragenen Vorschau neu gerendert. Sie sind keine Live-Screenshots und nicht die ursprünglichen PNG-Dateien aus dem Chat. Die Quelldateien sind per Prüfsumme gegen das Chat-Paket abgeglichen. `reference/tests/browser-results.json` und `syntax-results.json` stammen aus der vorherigen Vorschauprüfung; aktuelle Ergebnisse stehen getrennt unter `checks/`.

## Kopierbarer Start für den Agenten

```text
Übernimm im Repo EarlySalty/Deadlock-Twitch-Bot den Branch
 docs/social-studio-handoff-20260921.
Lies .tasks/2026-09-21-social-studio-handoff/README.md und AUFTRAG.md.
Das Designpaket liegt daneben unter reference/, es ist kein Upload nötig.
Integriere das aufgeräumte Layout mit dem bestehenden Community-Design in
das echte Social-Media-Dashboard. Prüfe die aktuellen Branches und die APIs,
erhalte die vorhandenen Funktionen und erledige Tests, Push, Gate, Merge,
Deploy, Live-Nachweis und eigenes Cleanup nach dem Auftrag.
Die enthaltene HTML-Vorschau ist kein Ersatz für die Produktintegration.
```

Bei einem bereits anderweitig benutzten Checkout zuerst dessen Status und Worktrees prüfen. Ein bestehender fremder Branch wird nicht durch ein blindes Checkout überschrieben. Dieser Übergabe-Branch kann als Ausgangspunkt für die Integration verwendet werden; die gemeinsame Basis muss vor Änderungen erneut mit `origin/main` abgeglichen werden.

## Referenz lokal prüfen

Aus dem Repo-Root:

```bash
cd .tasks/2026-09-21-social-studio-handoff/reference
npm ci --ignore-scripts --no-audit --no-fund
npm run build
npm test
npm run check
```

Diese Befehle bauen und testen den Entwurf. Sie deployen keine Produktdateien. Die eigenständige `preview.html` benötigt zum Betrachten keine npm-Installation. Die Produktion soll die vorhandenen Manrope-/Sora-Schriften tatsächlich laden; die Offline-Vorschau arbeitet mit einer Ersatzschrift.

## Grenze dieser Übergabe

Es wurden keine Produktkomponenten angebunden, keine Konten verändert, keine Clips veröffentlicht und keine Produktionsdienste neu gestartet. Der Draft-PR bleibt für den übernehmenden Agenten offen. Merge und Live-Deploy gehören zum nachfolgenden Integrationsauftrag, nicht zur Veröffentlichung dieses Referenzmaterials.

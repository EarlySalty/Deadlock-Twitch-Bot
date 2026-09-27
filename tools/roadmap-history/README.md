# Feature-Stammbaum der Twitch-Entwicklung

Die private Admin-Seite `/twitch/admin/roadmap-history/` zeigt die belegte Entwicklung des Twitch-Bots. Ausgangspunkt ist der erste Twitch-Commit im früheren Repository `EarlySalty/Deadlock-Bots` vom 21. September 2025 (`3654f6c73be53fc569da673fb307e7e0c79f2b87`). Ein Commit-Datum belegt Code, keinen Deploy. Die Elternbeziehungen der Funktionsfamilien sind redaktionelle Einordnungen, keine behaupteten technischen Abhängigkeiten.

## Bedienung

„Übersicht“ zeigt je Kategorie den jüngsten Entwicklungspfad samt Vorfahren. „Hauptfunktionen“ zeigt die belegten Funktionsfamilien, „Alle Änderungen“ zusätzlich die gebündelten Commits. Ein Klick öffnet die Beschreibung, den Elternast und einen in 30er-Schritten ladbaren Verlauf mit Originalmeldung, Datum und Link zum richtigen Git-Repository. Der Knopf „Diesen Zweig ansehen“ blendet fremde Äste aus. Der Plus-/Minus-Knopf an einer Familie klappt ihre Kinder auf oder zu.

Suche findet Funktionstitel, Beschreibungen und **alle** Commit-Meldungen eines Bündels, auch wenn die gesuchte Meldung nicht den Bündeltitel liefert. Datum von/bis berücksichtigt den gesamten Zeitraum eines Bündels. Kategorie, Art und Pflege können zusätzlich gefiltert werden; notwendige Vorfahren bleiben als Kontext sichtbar. Die X-Position entspricht in jeder Ansicht dem echten Datum. Pan funktioniert mit Ziehen oder Pfeiltasten, Zoom mit Mausrad oder den Knöpfen. „Ursprung“ führt zum ersten Knoten; „Alles einpassen“ zeigt den ganzen ausgewählten Ausschnitt. Auf Mobilgeräten beginnt die Ansicht lesbar am Ursprung und kann verschoben werden. Direktlinks mit `feature` oder `focus` werden übernommen.

Der Detaildialog ist nativ modal und gibt beim Schließen per Knopf oder Escape Fokus und Seitenscrollen frei. Die Seite hat keine externe Laufzeitabhängigkeit: Generator, CSS, JavaScript und JSON bilden eine einzelne HTML-Datei. Repository-Text wird ausschließlich über `textContent` eingefügt; Git- und PR-Links werden vor dem Setzen der URL geprüft. Eingebettetes JSON escaped HTML-/Script-Grenzen.

## Datenvertrag und Quellen

`feature_graph.py` erzeugt Schema 3 mit genau einem `genesis`-Root. Jeder Knoten hat `id`, `title`, `description`, ISO-`date`, `category`, `type` und `parentId`; Funktionen und Änderungsbündel tragen `role`, `featureId` und `commitIds`. Unbekannte Eltern, Zyklen, doppelte IDs und Kinder vor ihren Eltern werden abgewiesen. Jeder Git-Commit ist im Graph repräsentiert. Kleine Änderungen werden pro Funktion, Art und Pflege-Status in festen 14-Tage-Kalenderfenstern gebündelt; `spanEnd` bewahrt das Ende des Zeitraums. Das Feld `commits` enthält Originalmeldungen und Hashes. Globale Zahlen deduplizieren nach SHA.

Die Taxonomie und Zuordnungsregeln bleiben in `features.json` und `history_data.py`. `product` ist dort die redaktionelle Taxonomiewurzel; die ausgegebene Graphwurzel ist `genesis`. `other` hält offene oder mehrdeutige Zuordnungen sichtbar. Manuelle Korrekturen brauchen einen vollständigen SHA und eine Begründung; ein `feat`-Commit erzeugt keine neue Funktion. Die abgeschlossene Twitch-Vorgeschichte liegt als deterministisch komprimierter, per SHA-256 geprüfter Metadaten-Snapshot `legacy-history.json.gz` im Repository. Er enthält weder Quelltexte noch Zugangsdaten. Der stündliche Dienst benötigt deshalb keinen zweiten Git-Checkout. Die SHA-Prüfung erkennt beschädigte Daten; sie ist keine Signatur gegen böswillige Änderungen am Repository.

`tools/generate_roadmap.py` pinnt den aktuellen Git-Ref einmal, kombiniert ihn mit dem Legacy-Snapshot und schreibt erst nach vollständiger Validierung ein neues HTML atomar. `revision` bezeichnet den Datenstand; `rendererRevision` ist der Hash der eingebetteten HTML-/CSS-/JS-Dateien. Bei Fehlern bleibt die vorherige Seite erhalten. Ein reiner `feature_graph.py --input`-Lauf ist ein **Graph-Validator** und noch keine vollständige, renderbare Seitenquelle; die veröffentlichte Seite kommt immer aus `generate_roadmap.py`.

## Lokal prüfen

Aus dem Repository-Root:

```sh
git fetch origin main
python3 -m unittest discover -s tools/roadmap-history -p 'test_*.py' -v
node --check tools/roadmap-history/app.js
node --check tools/roadmap-history/browser.test.mjs
python3 tools/generate_roadmap.py --ref origin/main
npm ci --prefix bot/dashboard_v2 --no-audit --no-fund --ignore-scripts
bot/dashboard_v2/node_modules/.bin/playwright-core install chromium
node tools/roadmap-history/browser.test.mjs
shellcheck ops/systemd/install-roadmap-history.sh
systemd-analyze verify ops/systemd/deadlock-roadmap-history.service ops/systemd/deadlock-roadmap-history.timer
```

Der Browser-Test nutzt ausschließlich einen lokalen `127.0.0.1`-Server und den echten vollständigen Datenbestand. Er prüft Ursprung, Elternketten und Kanten, Kalender-X-Positionen, Filter, Suche, Direktlink, Zweigfokus, Dialog, Mobilansicht und HTML-Injektion. Seine Screenshots `feature-tree-desktop.png` und `feature-tree-mobile.png` sowie `browser-report.json` liegen unter `dist/roadmap-history/`; sie werden nicht eingecheckt und müssen zusätzlich visuell geprüft werden. CI installiert `playwright-core` aus dem geprüften Lockfile von `bot/dashboard_v2` und hebt die Nachweise sieben Tage als zugriffsgeschütztes Artefakt auf. Das private HTML wird nicht als Artefakt hochgeladen.

## Veröffentlichung und Rückweg

Caddy und Admin-Authentifizierung bleiben unverändert. Der Installer `ops/systemd/install-roadmap-history.sh` darf nur aus einem sauberen, gepushten `main`-Checkout laufen. Er legt alle Generator-/Renderer-Assets in einem versionierten Ordner unter `/opt/deadlock-roadmap/releases/<SHA>/` ab, erzeugt und prüft die komplette Kandidaten-Seite, stoppt Dienst und Timer für den Wechsel und tauscht `/opt/deadlock-roadmap/current` sowie `/srv/deadlock-roadmap/index.html` atomar aus. Ein Fehler stellt vorherigen Link, Unit, Timer und HTML aus einem nicht öffentlichen Backup wieder her. Die bestehende stündliche `deadlock-roadmap-history.service` generiert danach aus `origin/main` neu. UI- oder Generator-Änderungen benötigen eine erneute Installer-Ausführung; neue Git-Commits nimmt der Timer selbst auf. Dieser statische Deploy benötigt weder Datenbankmigration noch Bot-Binary-Neustart.

Nach der Installation müssen Dienst und Timer erfolgreich sein und die Seite angemeldet den neuen Stammbaum mit richtigem Quellstand zeigen. Unangemeldet muss der Admin-Zugriffsschutz weiter greifen. Der lokale Browser-Proof ersetzt diese Live-Prüfung nicht.

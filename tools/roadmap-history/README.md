# Feature-Roadmap und Entwicklungshistorie

Ersetzt die unbeschriftete Commit-Punktewolke der privaten Admin-Seite
`/twitch/admin/roadmap-history/` durch drei zusammenhängende Ansichten:

- **Roadmap:** Feature-Zeilen mit beschrifteten Monatskarten und einer Zeitachse.
- **Feature-Stammbaum:** Produktbereich → Feature → datierte Entwicklungsstationen.
- **Änderungsliste:** Einzelne Änderungen, neueste zuerst, mit Feature- und Commit-Links.

Ein Klick öffnet die Entwicklungslinie im Detail. Die Ansicht unterstützt Suche,
Bereich, Zeitspanne, eigene Datumsgrenzen, Änderungsart, technische Pflege und
teilbare Direktlinks. Der Dialog kann zwischen der gefilterten Auswahl und der
vollständigen Feature-Historie wechseln. Lange Listen werden seitenweise geladen.

## Datenvertrag

Quelle sind ausschließlich die vom angegebenen Git-Ref erreichbaren Commits im
Twitch-Bot-Repository. Der Generator pinnt den Ref vor der Auswertung. Merge-Commits
werden nicht zusätzlich gezählt; ungemergte Branches werden nicht als geliefert
angezeigt. Die unveränderten Commit-Titel und Dateipfade bleiben als Belege sichtbar.

Ein Commit belegt **keinen** erfolgreichen Deploy. Der früheste zugeordnete Commit
ist ein **erster Nachweis**, kein verifiziertes Einführungsdatum. Die Verbindungen
im Stammbaum sind fachliche Gruppierungen, keine technischen Abhängigkeiten und
kein Git-Branch-Graph. `feat` bedeutet Erweiterung, nicht ein zusätzliches Feature.
Ein Commit kann mehrere Feature-Linien berühren; die globale Summe zählt ihn einmal.

Die Feature-Taxonomie steht in `features.json`. Spezifische Commit-Titel haben
Vorrang vor allgemeinen Router-/Dashboard-Dateien. Anschließend werden spezifische
Dateipfade geprüft. Unklare Änderungen bleiben sichtbar unter „Noch nicht
zugeordnet“. Automatische Zuordnungen werden ausdrücklich gekennzeichnet.
Neue Feature-Familien können durch einen weiteren Taxonomie-Eintrag ergänzt werden.

Redaktionelle Korrekturen sind optional unter `overrides` möglich, mit vollständigem
Commit-Hash und `features`, optional `title`, `reason`, `note`. Der originale
Commit-Titel bleibt auch bei einer Korrektur erhalten. Keine Daten oder technischen
Abhängigkeiten werden aus generierten Beschreibungstexten erfunden.

Der Datenstand und die Erzeugungszeit stehen auf der Seite. Zeitfenster beziehen
sich auf das letzte Commit-Datum. Datumsanzeige: Europe/Berlin. Ein flacher Klon und
ein über sieben Tage alter Erzeugungsstand werden sichtbar angemahnt. Dokumentation,
Tests, Abhängigkeiten und Pflege sind standardmäßig ausgeblendet, nicht gelöscht.

## Erzeugen

Aus dem Repository-Root:

```sh
git fetch origin main
python3 tools/generate_roadmap.py --ref origin/main
```

Ergebnis: `dist/roadmap-history/index.html`. Alternativ `--repo`, `--ref` und
`--output` angeben. Python benötigt nur die Standardbibliothek einschließlich
`zoneinfo`. Keine GitHub-API, kein LLM, keine CDN-Abhängigkeit und kein Frontend-Build.
Die Ausgabe ist eine einzige selbständige HTML-Datei mit eingebetteten Styles,
JavaScript und inertem JSON. Repository-Texte werden als Text, nicht HTML gerendert;
das eingebettete JSON schützt insbesondere vor dem Schließen eines Script-Tags.

Die Ausgabe enthält private Repository-Historie. Sie darf ausschließlich hinter
der vorhandenen Admin-Authentifizierung veröffentlicht werden. Generierte HTMLs,
Test-Screenshots und der Browser-Testcache sind nicht Teil des Git-Commits.

## Tests

```sh
python3 -m unittest discover -s tools/roadmap-history -p 'test_*.py' -v
node --test tools/roadmap-history/model.test.mjs
python3 tools/generate_roadmap.py
```

Optionaler Browser-Test mit einem ausschließlich lokalen HTTP-Server:

```sh
npm install --prefix .roadmap-test-runtime --no-save --ignore-scripts --no-audit --no-fund playwright@1.58.2
# Falls Chromium noch nicht im lokalen Playwright-Cache vorhanden ist:
node .roadmap-test-runtime/node_modules/playwright/cli.js install chromium
node tools/roadmap-history/browser.test.mjs
```

Auf dem Entwicklungsserver wurde Playwright 1.58.2 aus dem bestehenden npm-Cache
mit `--offline` installiert. Der Test deckt Desktop 1440 × 1120 und Mobile
390 × 844 ab, inklusive Dialog, Escape, Belege, Filter-Reset, leerer Suche,
ungültigem Zeitraum, Pagination und Direktlink-Wiederherstellung. Screenshots
werden unter `dist/roadmap-history/` abgelegt. Testserver und Browser schließen
auch bei fehlgeschlagenen Assertions.

## Einmalige Veröffentlichung auf v50671

**Diese Installation benötigt eine ausdrücklich autorisierte Root-Ausführung.**
Das Schreiben und Testen im Arbeitsbaum allein veröffentlicht die Seite nicht.
Der bisherige statische Webroot `/srv/deadlock-roadmap` ist root-eigen; der
reguläre Entwicklungszugang besitzt dort keine Schreibrechte.

Aus einem geprüften Checkout:

```sh
sudo bash ops/systemd/install-roadmap-history.sh
```

Der Installer sichert die vorhandene Seite einschließlich ihrer undatierten
kuratierten Alt-Einträge außerhalb des Webroots unter
`/var/backups/deadlock-roadmap/`. Diese Alt-Einträge werden nicht ohne Datumsbeleg
in die neue Zeitachse einsortiert. Der Installer kopiert den Generator und seine
Assets nach `/opt/deadlock-roadmap/`, richtet einen unprivilegierten Dienst ein
und veröffentlicht eine vollständig erzeugte HTML-Datei atomar.

Anschließend aktualisiert `deadlock-roadmap-history.timer` stündlich aus
`origin/main`. Änderungen an Taxonomie oder Generator benötigen die erneute
Installation aus einem geprüften Checkout; normale neue Commits werden automatisch
aufgenommen. Der Update-Dienst arbeitet ohne Root und darf nur den Webroot und das
Git-Metadatenverzeichnis beschreiben. Git-Fetch- oder Generatorfehler lassen die
vorherige HTML-Datei unverändert. Die Caddy-Route und deren `forward_auth` bleiben
unverändert. Bot, Dashboard-Runtime und Datenbank müssen nicht neu gestartet werden.

Nach einer Installation prüfen:

```sh
systemctl status deadlock-roadmap-history.service deadlock-roadmap-history.timer
journalctl -u deadlock-roadmap-history.service --no-pager -n 30
```

Zusätzlich angemeldet im Admin-Panel die Roadmap öffnen und unangemeldet prüfen,
dass weiterhin die Discord-Anmeldung verlangt wird. Die vorliegende Browser-Abnahme
prüft das statische Artefakt lokal, nicht eine authentifizierte Produktionssession.

Rollback: Timer stoppen, einen zuvor geprüften Backup-Snapshot als temporäre Datei
im Webroot bereitstellen und per atomarem Rename auf `index.html` zurücksetzen.
Dazu werden dieselben autorisierten Betriebsrechte benötigt. Die Backups dürfen
nicht direkt über den Webserver angeboten werden.

## Abnahmestand

Am Quellstand `4a377acfa832c77eaa2f3e3f8769627669e9ce94`:

- 3.622 eindeutige Nicht-Merge-Commits, 24.02.2026 bis 17.09.2026, vollständiger Klon.
- 14 Python- und 16 Node-Tests erfolgreich.
- 9 Browser-Prüfgruppen erfolgreich, keine JavaScript-Laufzeitfehler und keine
  horizontale Seitenüberbreite an den beiden geprüften Bildschirmgrößen.
- Produktionsinstaller und Timer vorbereitet, aber bei dieser Abnahme **nicht
  installiert oder ausgeführt**. Der unveränderte Produktions-Webroot ist noch
  geschützt; die bestehende Live-Seite wurde nicht ausgetauscht.

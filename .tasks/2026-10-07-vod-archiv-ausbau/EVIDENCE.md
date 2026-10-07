# Prüfnachweise

## Compiler und Datenbank

`cargo check -p tb-vod-archive -p tb-dashboard-api -p tb-bot` mit aktueller Toolchain: erfolgreich. Der erste Hintergrundauftrag erreichte sein Zeitlimit, sein Cargo-Kindprozess lief noch weiter und beendete den Lauf erfolgreich nach 40 Minuten. Der erneut gestartete, auf zwei Jobs begrenzte Lauf wartete zunächst auf diesen eigenen Build und endete ebenfalls erfolgreich. Protokolle: `/tmp/vod-archive-check.log`, `/tmp/vod-archive-check-resumed.log`. Eine dabei gefundene ungenutzte Konstruktorfunktion des abgelösten Exportpfads wurde anschließend entfernt. Clippy und abschließender Compilerlauf bleiben offen.

Die Schema-Probe lief gegen die private Datenbank `token_db_vod_archive_20261007` im vorhandenen Wegwerfcontainer. Ein frischer vollständiger Migrationslauf erzeugte den Schema-Snapshot mit den vier neuen Spalten. Ergebnis: 1 passed, 0 failed, 0 ignored. Protokoll: `/tmp/vod-archive-schema-test.log`. Bestätigung ohne Snapshot-Aktualisierung und SQLx-Cache-Prüfung bleiben offen.

## Dashboard

Build erfolgreich: `/tmp/vod-archive-dashboard-build-confirmed.log`. Nach der Sichtprüfung wurden Datumsformat, mobile Titelbreite und Teilezahl einer fertigen Drive-Kopie verbessert; der Abschlussbuild bleibt offen.

Baseline bei 9315b3cf: Kalender 9 passed, Hauptsuite 422 passed, 5 failed, 0 skipped. Feature nach Korrektur der URL-Notation: ebenfalls Kalender 9 passed, Hauptsuite 422 passed, 5 failed, 0 skipped. Die fünf identischen Fehler betreffen drei bestehende Palettenprüfungen, den Analytics-Rahmen und die OBS-Empfehlung. Protokolle: `/tmp/vod-archive-baseline-tests.log`, `/tmp/vod-archive-dashboard-tests-confirmed.log`. Kein zusätzlicher Fehler verbleibt aus dem Archiv-API-Vertrag.

Die kollaborative Browserautomatisierung ist in dieser Umgebung nicht verfügbar. Die lokale Playwright-Prüfung verwendete den gebauten Dashboard-Code und ausdrücklich synthetische API-Daten, nicht echte Konten. Desktop 1440 px und Mobilansicht 390 px: Dokumentbreite entspricht der Viewportbreite, drei Ziellinks sichtbar, keine Browser-Laufzeitfehler. Screenshots: `~/.claude/sichtpruefung/vod-archiv-ausbau/{desktop,mobile,mobile-lower}.png`. Protokoll: `/tmp/vod-archive-visual-check.log`. Eine Bestätigungsrunde nach den gebündelten Korrekturen bleibt offen. Das mechanische Impeccable-Scanning meldete keine Befunde.

## Betrieb

Relative Medienpfade bleiben wie bisher relativ zum Arbeitsverzeichnis des Botdienstes. Der Default `data/vod-archive` erreicht deshalb weiterhin den bestehenden systemd-Bind-Mount. Die zuvor erwogene Auflösung relativ zur Betriebsdatei wurde entfernt, damit kein neues Archiv unter dem Konfigurationsordner entsteht.

Direkter Zugriff auf die root-verwaltete Betriebsdatei und das Dienst-Medienverzeichnis ist mit den aktuellen Dateirechten nicht möglich. Ihre Inhalte und vorhandene Live-Dateien sind damit nicht bestätigt. Die lesbare Dienstdatei enthält die bisherigen Werte drei Uploads je Lauf und einen Tag lokale Aufbewahrung. Der neue typisierte Default hält drei Uploads je zwölf Stunden; die Aufbewahrung wartet stattdessen auf bestätigte Verarbeitung.

Keine Produktionsmigration, kein Deployment, kein Live-Retry, keine Entfernung von Altdiensten oder Python-Dateien erfolgt. Diese Schritte bleiben offen.

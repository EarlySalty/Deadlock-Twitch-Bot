# Prüfnachweise

## Compiler und Datenbank

`cargo check -p tb-vod-archive -p tb-dashboard-api -p tb-bot` mit aktueller Toolchain: erfolgreich. Der erste Hintergrundauftrag erreichte sein Zeitlimit, sein Cargo-Kindprozess lief noch weiter und beendete den Lauf erfolgreich nach 40 Minuten. Der erneut gestartete, auf zwei Jobs begrenzte Lauf wartete zunächst auf diesen eigenen Build und endete ebenfalls erfolgreich. Protokolle: `/tmp/vod-archive-check.log`, `/tmp/vod-archive-check-resumed.log`. Eine dabei gefundene ungenutzte Konstruktorfunktion des abgelösten Exportpfads wurde anschließend entfernt. `SQLX_OFFLINE=true cargo clippy -j2 -p tb-vod-archive -p tb-dashboard-api -p tb-bot --all-targets` bestand mit Exit 0 in 3 Minuten 20 Sekunden; Warnungen bleiben ausdrücklich vorhanden. Protokoll: `/tmp/vod-archive-clippy.log`. Nach dem abschließenden Rebase auf 85d8ec63 werden die betroffenen Tests erneut kompiliert und ausgeführt.

Die Schema-Probe lief gegen die private Datenbank `token_db_vod_archive_20261007` im vorhandenen Wegwerfcontainer. Ein frischer vollständiger Migrationslauf erzeugte den Schema-Snapshot mit den vier neuen Spalten. Die Bestätigung nach dem Rebase lief ohne Snapshot-Aktualisierung erfolgreich: 1 passed, 0 failed, 0 ignored. Protokolle: `/tmp/vod-archive-schema-test.log`, `/tmp/vod-archive-schema-confirmed.log`. Der SQLx-Cache wird gegen die vollständig migrierte private Datenbank geprüft.

Archiv und Konfiguration mit `TB_TEST_DATABASE_URL` und `--include-ignored --nocapture --test-threads=1`: 135 passed, 0 failed, 0 ignored. Davon sind 49 Archivtests; die PostgreSQL-Fälle und die echte Dateilöschung nach bestätigter Verarbeitung liefen tatsächlich. Protokoll: `/tmp/vod-archive-rust-tests-serial.log`. Der frühere parallele Lauf kollidierte mit Datenbank-Advisory-Locks verschiedener Fixtures; getrennte ID-Bereiche beheben diese Fixture-Kollision. Der unmittelbar folgende Sammellauf brach vor den Archivtests in zwei Konfigurations-Editorfällen ab. Der serielle vollständige Lauf bestätigt beide Suiten ohne Abschwächung.

Die Archiv-API-Fälle laufen mit echten abgeschotteten PostgreSQL-Instanzen: 3 passed, 0 failed, 0 ignored, 1334 filtered out. Protokoll: `/tmp/vod-archive-api-tests-serial.log`. Der erste parallele Lauf scheiterte bei zwei Instanzen an der 15-Sekunden-Initialisierungsfrist, nicht an API-Assertions. Die serielle Wiederholung bestand ohne Änderung am PostgreSQL-Helfer.

Die vollständige Cache-Prüfung mit `SQLX_OFFLINE=false cargo sqlx prepare --workspace --check -- -j2 --all-targets` bricht im externen `dbrain-builds` ab: 18 Abfragen benötigen nicht vorhandene `brain.*`-Tabellen in der Twitch-Wegwerfdatenbank. Protokoll: `/tmp/vod-archive-sqlx-cache-check.log`. Der Cache wurde nicht als bestätigt gemeldet und nicht überschrieben. Die neuen Archivabfragen sind Laufzeitabfragen ohne neue SQLx-Makro-Metadaten; der Offline-Compiler und die echten API-Datenbanken prüfen den Archivpfad.

Die workspaceweite Formatprüfung meldete 836 Stellen. Bei den berührten Rust-Quellen verbleiben drei Formatstellen in `tb-dashboard-api/src/lib.rs`; genau diese drei liegen bereits im geprüften `origin/main`-Dateistand. Die neuen Archivquellen sind formatiert. Unbeteiligte Dateien wurden nicht gesammelt umformatiert.

## Dashboard

Abschlussbuild erfolgreich: `/tmp/vod-archive-dashboard-build-final.log`. Nach der ersten Sichtprüfung wurden Datumsformat, mobile Titelbreite und Teilezahl einer fertigen Drive-Kopie verbessert. Der integrierte Wiederholungsbuild war ebenfalls erfolgreich.

Baseline bei 9315b3cf: Kalender 9 passed, Hauptsuite 422 passed, 5 failed, 0 skipped. Feature nach Korrektur der URL-Notation: ebenfalls Kalender 9 passed, Hauptsuite 422 passed, 5 failed, 0 skipped. Die fünf identischen Fehler betreffen drei bestehende Palettenprüfungen, den Analytics-Rahmen und die OBS-Empfehlung. Protokolle: `/tmp/vod-archive-baseline-tests.log`, `/tmp/vod-archive-dashboard-tests-confirmed.log`. Kein zusätzlicher Fehler verbleibt aus dem Archiv-API-Vertrag.

Die kollaborative Browserautomatisierung ist in dieser Umgebung nicht verfügbar. Die lokale Playwright-Prüfung verwendete den gebauten Dashboard-Code und ausdrücklich synthetische API-Daten, nicht echte Konten. Desktop 1440 px und Mobilansicht 390 px: Dokumentbreite entspricht der Viewportbreite, drei Ziellinks sichtbar, keine Browser-Laufzeitfehler. Screenshots: `~/.claude/sichtpruefung/vod-archiv-ausbau/{desktop,mobile,mobile-lower}.png`. Die abschließende Bestätigungsrunde bestand: `/tmp/vod-archive-visual-confirmed.log`. Das mechanische Impeccable-Scanning meldete keine Befunde. Der vorhandene schwebende Hilfeknopf überlappt in der unteren Mobilansicht teilweise die letzte Ausblenden-Aktion; das bleibt als bekannte Sichtgrenze dokumentiert.

## Betrieb

Relative Medienpfade bleiben wie bisher relativ zum Arbeitsverzeichnis des Botdienstes. Der Default `data/vod-archive` erreicht deshalb weiterhin den bestehenden systemd-Bind-Mount. Die zuvor erwogene Auflösung relativ zur Betriebsdatei wurde entfernt, damit kein neues Archiv unter dem Konfigurationsordner entsteht.

Die begrenzte Bestandsprüfung als Dienstkonto konnte die normalen Archivoptionen und den persistenten Medienordner inzwischen lesen. In der TOML-Datei ist kein eigener VOD-Abschnitt gesetzt; im persistenten Archivordner liegen zwölf Dateien. Zugangsdaten wurden dabei nicht gelesen oder ausgegeben. Die lesbare Dienstdatei enthält die bisherigen Werte drei Uploads je Lauf und einen Tag lokale Aufbewahrung. Der neue typisierte Default hält drei Uploads je zwölf Stunden; die Aufbewahrung wartet stattdessen auf bestätigte Verarbeitung.

Keine Produktionsmigration, kein Deployment, kein Live-Retry, keine Entfernung von Altdiensten oder Python-Dateien erfolgt. Diese Schritte bleiben offen.

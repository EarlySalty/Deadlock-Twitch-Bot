# VOD-Archiv: Funktionsnachweis

## Rust und echte Datenbankwege

- `cargo-slot test --manifest-path /home/nathanael/.worktrees/tb-vod-archiv-status-20261007/rust/Cargo.toml -p tb-vod-archive --jobs 3 -- --include-ignored`: 50 passed, 0 failed, 0 ignored, 0 filtered. Der vorhandene Store-Testweg liest die eigene `rust/token-db-tests.conf`; TB_TEST_DATABASE_URL ist dafür nicht maßgeblich. Wegwerf-DB auf eigenem Unix-Socket, keine TCP-Verbindung zur Produktion.
- `cargo-slot test --manifest-path /home/nathanael/.worktrees/tb-vod-archiv-status-20261007/rust/Cargo.toml -p tb-dashboard-api --jobs 3 vod_archive_management -- --include-ignored`: 5 passed, 0 failed, 0 ignored, 1336 filtered im Bibliothekslauf. Die fünf Archivprüfungen verwenden den tatsächlichen Listen-/Aktionshandler und isoliertes PostgreSQL aus dem vorhandenen TestPostgres-Helfer. Keine DB- oder Handlerattrappe. Andere Integrationstest-Binaries sind durch den genannten Filter ausgeschlossen, nicht als geprüft gezählt.
- Der erste API-Filter `social_media_vod_archive` traf keinen Modulnamen: 0 passed. Dieser Null-Lauf wird nicht als Beweis gewertet. Der korrigierte Lauf oben prüft das echte Modul.
- NIT aus Gate-Runde 2: Die neue Versuchsprüfung folgt dem vorhandenen optionalen DB-Testmuster. Fehlende Konfiguration kann dort einen stillen Rücksprung erzeugen. In diesem Auftrag lief sie tatsächlich: Die eigene DB enthält genau einen Datensatz im Schema `t_vod_attempt_transition` mit gespeichertem Versuch und beendetem Status `downloaded`. Das Uploadschema enthält genau einen bestätigten Abschluss mit uploaded_at. Beleg: `db-execution-proof.log`, jeweils `1|t`. Die Testkonfiguration wird nicht mitgeliefert.
- API-Vollsuite beim ersten Lauf: 1319 passed, 22 failed, 0 ignored. Ausgangs-SHA 2ead4d556327596fcc7d9feeaceb848e910cf8f8 unter demselben Befehl und derselben TB_TEST_DATABASE_URL: 1318 passed, 22 failed, 0 ignored. Die 22 fehlschlagenden Testnamen sind identisch; kein neuer Fehlername. Der Vollsuite-Lauf wurde vor den letzten Store-Testergänzungen begonnen, der gezielte API-Lauf und die 50 Archivprüfungen prüfen den aktuellen Produktstand. Vollsuite nicht als grün gemeldet.

TB_TEST_DATABASE_URL bei den protokollierten Befehlen: `postgresql:///token_db_vod_status?host=/tmp/tb-vod-status-20261007-pg&user=nathanael`. VOD_ARCHIVE_PROOF_PATH beim API-Lauf: eigene Taskakte `pruefung/api-fixture.json`. Keine Produktionssitzung oder externe Veröffentlichung. API-TestPostgres startet unabhängig von dieser Umgebungsvariable seine eigenen privaten PostgreSQL-Prozesse.

TESTNACHWEIS[TW-1]: 55 passed, 0 ignored | Baseline: 22 rot

## Frontend

`npm --prefix /home/nathanael/.worktrees/tb-vod-archiv-status-20261007/bot/dashboard_v2 test`: Archiv 5 passed/0 failed, Kalender 9 passed/0 failed, Hauptsuite 425 passed/5 failed. Ausgangsstand: Kalender 9 passed/0 failed, Hauptsuite 425 passed/5 failed. Dieselben fünf Fehler: Palette, Tailwind-Standardfarben, heller Markentext, vorhandener Analytics-Rahmen, bestehende OBS-Hilfe. Archivprüfung ist in den normalen Testbefehl eingebunden.

Produktionsbuild mit `npm ... run build` erfolgreich, einschließlich TypeScript. Tatsächlicher Ausgabeordner: `bot/analytics/dashboard_v2/dist`. Gebündelte Moli-Prüfung und genau eine Bestätigung samt SHA/Asset-Hashes: `SICHTPRUEFUNG.md` und `moli-bestaetigung.json`.

TESTNACHWEIS[TW-1]: 439 passed, 0 ignored | Baseline: 5 rot

## Statusquellen und Wirkung

1. YouTube: `done` mit nichtleerer Video-ID je Teil plus finaler VOD-Status. Die gemeinsame Abschlussfunktion verweigert leere oder unbestätigte Teile. Wiederholte Bestätigung erhält die Zeit, ein neuer Abschluss übernimmt keine veraltete Zeit.
2. Drive: vorhandene Kopie mit geprüfter Größe je Teil, gespeicherter Drive-Link, `drive_uploaded` und uploaded_at. Keine Ableitung aus YouTube-Teilen.
3. Versuch: tatsächlicher Übergang zu downloading/uploading. Eine reine Worker-Sichtung erzeugt keinen Versuch. Laufgrenze setzt aktiven Upload auf einen wartenden/fehlgeschlagenen Zustand zurück.
4. Unklarer Altbestand: fehlende oder widersprüchliche Bestätigung bleibt unklar. Kein automatischer Neu-Upload; vorhandene Links werden unbestätigt bezeichnet.
5. Plattformgrenze: vorhandener YouTubeUploader::video_status bleibt der zentrale lesende Hintergrundweg. Rejection/Missing und processed-vor-lokaler-Bereinigung wurden geprüft. Keine Seitenabfrage, kein zusätzliches Kontingent, kein neuer Client oder Secret. Gespeicherter Abschluss garantiert weder heutige Verfügbarkeit noch öffentliche Sichtbarkeit.

Zwillingssuche: bestehende VOD- und Teilabschlüsse, Resume-Abschluss, Chunk-Abschluss, Drive-Abschluss und Archivbereinigung geprüft. Erfolgsschreibweg zentral im Store, Drive verbleibt beim bestehenden getrennten Zielweg. Fehler vor und während YouTube-Teiltransfer werden am Teil gespeichert; vorhandene VOD-Fehlerbehandlung bleibt erhalten. Produktionsdaten wurden ausschließlich aggregiert gelesen, keine echten VODs verändert.

WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 3/3 geprüft

# Fixrunde 6: Watchdog nach der nativen Umschaltung

## Ziel und Stand

Frischer nativer Implementierer für denselben Auftrag. Eigener Worktree: `/home/nathanael/.worktrees/tb-kategoriesammler-nativ`, detached HEAD `4ddf37032050c2b8cbe74bb921d7ed1c055514b4`. Dieser Kern ist nach dem lokalen ALLOW bereits per geschütztem Push auf main. Die Hauptsession führt den produktiven Abschluss aus. Keine zusätzlichen T3-Threads, keine eigene Review-Session.

Alle sieben Release-Binaries und die drei Frontends wurden aus diesem sauberen SHA gebaut. Die beiden neuen Migrationen sind bereits produktiv angewandt und damit eingefroren. Niemals diese Migrationen ändern. Die neue native Sammlung wurde durch den Deploy-Wrapper nach echter Bereitschaft und neuer Messung übernommen. Der alte Sammler wurde danach gestoppt und seine alten Dateien entfernt. Dashboard und Audit wurden durch die Hauptsession anschließend ebenfalls neu gestartet. Der Deploy endete dennoch mit Exit 1, weil der Watchdog nicht startet. Noch kein vollständiger Live-Abschluss.

## Zwei belegte Fehler

1. Vollständiger finaler Collector-Lauf: Bibliothek 8 passed, 0 failed, 0 ignored; Watchdog 6 passed, 1 failed, 0 ignored. Fachlog `/tmp/tb-category-native-4ddf-final-tests.log`. `incidents_survive_retries_and_recovery_opens_a_new_incident` fällt in `tb-twitch-watchdog.rs:1147`: links 1, rechts 2. Ursache am tatsächlichen Vertrag prüfen. Bestehende Prüfung nachziehen, ohne sie still abzuschwächen. Falls die Produktionslogik falsch ist, diese eng korrigieren. Falls die neuen Beobachtungsgrenzen die synthetische Zeitreihe ungültig machen, die Zeitreihe korrekt konstruieren und den fachlichen Nachweis erhalten.
2. Produktiver Watchdog startet seit der Umschaltung nicht. Diagnose `/tmp/tb-category-native-watchdog-diagnostic.log`: `Error: FileError { kind: FileUnreadable, field: None, line: None, column: None }`. Installierte Unit: Type=oneshot, User=root, Group=root, ExecStart `/opt/deadlock/twitch/current/rust/target/release/tb-twitch-watchdog --config /var/lib/deadlock-twitch/config/bot.toml`, CAP_SETUID/CAP_SETGID. Prüfe CLI, Config-Vertrag, Pfade und Rollenwechsel. Das neue Watchdog-Konfigurationsziel des Deploy-Wrappers ist `/etc/deadlock-twitch/bot-watchdog.conf`. Keine Secret-Inhalte lesen oder ausgeben. Dateimetadaten und Unit-Metadaten sind zulässig. Ursache und korrigierten Startvertrag belegen.

## Eigentum und Grenzen

Du besitzt exklusiv die nötigen engen Änderungen im Watchdog, seinen bestehenden Tests und erforderlichen Watchdog-/Deploy-Quelldateien. Keine neue Architektur, keine globale Formatierung und keine Kommentare in Code. Bereite AUFTRAG, REGISTER und REVIEW wahrheitsgemäß nach. Diese neue Briefingdatei mitcommitten. Keine produktiven Änderungen, kein Push, kein eigener Deploy und kein Cleanup. Die Hauptsession ist alleiniger Integrationsverantwortlicher. Kein paralleler Schreiber am selben Pfad.

Produktiver Code ausschließlich Rust. Keine ENV-Dateien oder neue ENV-Konfiguration. Secrets über den bestehenden Bot-/Infisical-Weg. Anonymes justinfan-IRC, Archivschutz und Speichergrenzen bleiben unverändert. Keine Archivdaten löschen. Den beauftragten Speichernachtrag noch nicht implementieren.

## Prüfung und Gate

Vor Code-Suche den Skill `code-suche` und Graphify verwenden. Moli ist der einzige Agentenbrowser. Brave weder direkt noch indirekt starten; Browserarbeit ist für dieses Paket nicht nötig. Persönlichen Browser und fremde Dienste unangetastet lassen. Nur eigene Cargo-Aufträge stoppen. Release-Builds nur im eigenen Worktree, Cargo über `/home/nathanael/.local/bin/cargo-slot +1.97.1`, `--jobs 3`, SQLX_OFFLINE=true. Compiler und Build sind der Mindestbeweis; die konkret fehlgeschlagene bestehende Suite muss tatsächlich nachgezogen werden.

Der eigene synthetische Testcontainer `tb-category-wb1-db2-168485db` läuft auf Port 33100. Bestehende Test-Konfiguration: `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33100/postgres`, `TB_TEST_REQUIRE_DB=1`. Das ist der vorhandene Test-Kompatibilitätsweg, kein neuer produktiver ENV-Vertrag. Vollständiger Befehl: `SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33100/postgres TB_TEST_REQUIRE_DB=1 /home/nathanael/.local/bin/cargo-slot +1.97.1 test --manifest-path /home/nathanael/.worktrees/tb-kategoriesammler-nativ/rust/Cargo.toml --jobs 3 -p tb-category-collector --all-targets -- --include-ignored`.

Prüfungen mit tatsächlichen Zahlen melden. Aufträge laufen mit ausreichend langer wrapperverwalteter Deadline, nicht nach 20 Minuten vor Slot-Erhalt abbrechen. Changed-file-Fmt mit Edition 2021; striktes Collector-/Monitoring-Clippy. Breites Bot-Clippy hat eine zahlenmäßig belegte unveränderte Baseline mit acht Stellen und ist nicht pauschal grün.

Einziger Reviewer ist `gate_hook.py --review`, dasselbe Modell `gpt-6.1-sol` wie in Runde 1. Bei BLOCK je Runde einen frischen nativen Fixer einsetzen, nicht auf ein anderes Modell ausweichen. Keine Zwischenberichte pro Runde. Abschluss erst mit sauberem Commit, tatsächlichem Prüfresultat und ALLOW, oder mit einem echten Blocker. Commit-Trailer gemäß geltenden Nutzerregeln.

## Übergabe

Bericht an diese Hauptsession: Ursache beider Fehler, enger Fix, SHA, tatsächliche Testzahlen, Gate-Urteil und Nachweisorte. Task-Ende erst nach Prozessende der eigenen Prüfungen. Keine behauptete Startwirkung aus einem Kompilierpass. Die Hauptsession prüft den reparierten Watchdog später in Produktion und schließt den Deploy vollständig ab.

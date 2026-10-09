# Gate-Mängel: Native Kategorieerfassung

Einziger Reviewer: lokaler Merge-Gate, Kritiker `gpt-6.1-sol`. Die Fixer bleiben bei diesem Kritiker. Kein Produktivschritt vor gültigem ALLOW.

## Speichernachtrag: Kern-Gate

Kandidat `561542c0af3afe54155f3b530c56c03fc98b4bcb`, damalige Basis `origin/main = 14eedf4aa602ba835b3619d9f4d8bcfd16351a53`, Kritiker `gpt-6.1-sol`: `ALLOW: No blocking defect found in the supplied changes.` Nachweis `/tmp/tb-storage-core-gate-168485db.log`. Kern veröffentlicht, keine produktive Speicheranwendung.

1. NIT: Clusterweite Rollenerzeugung der synthetischen Fixture verwendet einen nebenläufigen Existenzcheck. Fundstellen `rust/test-support/category_postgres.rs:45`, `category_archive.rs:443` und Erzeugung von `twitchcategoryarchive` in der Migration. Das ist kein BLOCK; parallele Läufe auf einem frischen Testcluster können kollidieren.
2. NIT: Die konfigurierte Test-DSN erzeugt je Test eine Datenbank ohne anschließenden Cleanup. Fundstelle `rust/test-support/category_postgres.rs:29`. Eigene synthetische Ressourcen werden beim vollständigen Auftragsabschluss geprüft und bereinigt.

Paket L wurde in `b9b423edb06db2717a562e731ed57b9a9672fcb1` mit dem veröffentlichten Kern integriert. Gate auf `566b09d82` mit `gpt-6.1-sol`: `ALLOW: No confirmed merge-blocking defect in the supplied diff.` Nachweis `/tmp/tb-storage-runtime-gate-168485db.log`. Keine Gate-Fixrunde wird aus den fünf Workercommits abgeleitet.

1. NIT: Archivmeldungen werden mit der Startzeit des Watchdogs bestätigt. Eine erfolgreiche Zustellung über die Berliner Mitternacht kann dem Vortag zugeordnet werden. Fundstelle `rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs:459`. Nicht als BLOCK gemeldet; die Randbedingung bleibt dokumentiert.
2. NIT geprüft: `rust/bin/tb-bot/src/main.rs:587` verwirft den Schlüsselinitialisierungsfehler mit `.ok()`. Der gemeinsame Archivtask prüft bei aktivierter Auslagerung den fehlenden Schlüssel in `rust/crates/tb-analytics/src/category/storage.rs:970`, protokolliert den Fehlschlag ab Zeile 994 und legt eine entprellte Watchdogmeldung an. Fehlende Verschlüsselung führt damit zu sichtbarem Fehler und keinem Upload.

## Erster BLOCK auf fd0dbebd

1. Gemeinsamer Pausenstatus konnte die Budget-Hysterese falsch einrasten lassen.
2. Eine Pauseüberlappung unterdrückte ganze historische Messlücken.
3. Wartende Prozesse konnten die Identität des aktiven Lease-Inhabers überschreiben.
4. Hinweise: Rollenmatrix, bestätigte leere Messläufe, Zählerlebensdauer, Moli-Bildnachweise.

Fixrunde 1: `a1a49e95`, ALLOW für diesen damaligen Stand. Danach aktuelle Main-Integration `aeb05ce2`.

## BLOCK auf aeb05ce2

1. Eigenständige Wiederanwendung der Kategorien-Rollenmatrix entzog Watchdog-Schreibrechte.
2. Doku über wartende Prozesse und leere Messläufe veraltet.
3. Hinweis: echte Writer-Operationen als `twitchbot` prüfen.

Fixrunde 2: `ac8c61b5`. 11 Archivtests bestanden, 0 ignoriert; Matrix zweimal ausgeführt und echte Writer-Pfade geprüft. Folge-Gate weiterhin BLOCK.

## Offene Funde auf ac8c61b5

Gate: `/tmp/tb-category-gate-fix2-gate.log`.

1. BLOCK: `rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs:24`. Historische Messlücken werden nicht auf das Beobachtungsfenster begrenzt. Ein alter Vorgänger kann neue Vorfälle für ausdrücklich ausgenommene Zeiträume erzeugen.
2. BLOCK: `rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs:330`. Bereits beendete Ausfälle werden als weiterhin fehlende aktuelle Daten angekündigt. Die ausgewählten Vorfälle tragen keine Erholungsinformation bis zur Meldung.
3. NIT: `rust/migrations/20261008160000_category_native_bot.sql:60`. Enges `UPDATE(hour_at)` auf `category_chat_dirty` fehlt gegenüber der Rollenmatrix. Migration-only-Pfad einschließlich Konfliktupdate und Zeilensperre prüfen.
4. NIT: `ops/systemd/deploy-twitch-release`, `required_artifacts`. Basis-Build und Installer wählen unterschiedliche Dashboard-Artefaktpfade. Rückkehr zu einer älteren Release-Version gegen beide Skripte belegen.

Fixrunde 3: `181645db`. Die vier Funde korrigiert, 11 Wrappertests bestanden. Basis-Dashboard tatsächlich gebaut, 22 kopierte Frontend-Artefakte mit identischen SHA256-Werten belegt. Kein vollständiger Rust-Release-Nachweis für die Basisrevision. Folge-Gate BLOCK.

## Offene Funde auf 181645db

Gate: `/tmp/tb-category-fix3-gate.log`.

1. BLOCK: `ops/systemd/install-twitch-release.sh:87`, `:128`, `:165` und `ops/systemd/deploy-twitch-release`, `required_artifacts`. Ältere Zielrevisionen ohne native Sammlung verlieren beim Neuverpacken ihren externen Collector. Die Skripte akzeptieren diese Stände trotzdem. Bedingte Legacy-Verpackung wiederherstellen oder nicht unterstützte Revisionen vor Aktivierung sicher ablehnen.
2. NIT: `rust/bin/tb-category-collector/src/lib.rs:230`. Writer-Drain ohne Frist. Echte Abschaltung anonymer IRC-Produzenten und bestehende DB-Timeouts auf terminierenden Shutdown prüfen. Ein fehlender Nachweis ist kein bereits bestätigter Datenverlust.

Fixrunde 4: `89ef72da`, Legacy-Verpackung und begrenzte Abschaltung korrigiert, 16 Wrappertests bestanden, Gate ALLOW. Fixrunde 5: `4ddf3703`, deterministische Abschaltprüfung an die Timerauflösung angepasst, Gate ALLOW. Die breite Bot-Clippy-Prüfung hat laut Hauptsession eine unveränderte Baseline von acht Lintstellen; sie ist nicht grün.

## Fixrunde 6 nach der nativen Umschaltung

1. Bestehende Vorfallsprüfung: `record` setzt den zweiten Vorfallsbeginn nach Erholung bei 90 s auf 90 s plus 1 µs. Die Auswahl bei 180 s liegt damit vor seiner unveränderten 90-s-Meldegrenze. Der Test belegt jetzt zusätzlich den tatsächlichen Beginn und die Nichtauswahl am Rand; bei 181 s müssen weiterhin beide Vorfälle einschließlich Erholungszustand ausgewählt sein. Retry-, historische Messlücken-, Speicherpausen- und Tagesgrenzenprüfungen bleiben erhalten. Unveränderte Baseline im eigenen vollständigen Lauf: Bibliothek 8 bestanden; Watchdog 6 bestanden und 1 fehlgeschlagen; 0 ignoriert, Exit 101. Nachweis `/tmp/tb-category-fix6-baseline-tests.log`.
2. Watchdog-Start: `--config` bezeichnet weiterhin die gemeinsame Bot-TOML, nicht die separate Watchdog-Konfiguration in `/etc/deadlock-twitch/bot-watchdog.conf`. Der bisherige Rollenwechsel entfernte mit `setgroups(&[])` die für diesen Pfad nötige Gruppe `twitchmedia`. Die Dateirechte sind `root:twitchmedia` 0750 am Elternverzeichnis, 2770 am Konfigurationsverzeichnis und `twitchdash:twitchmedia` 0660 an der Datei. Der Bot-Dienst besitzt `twitchmedia` bereits als Zusatzgruppe. Der Watchdog wechselt weiterhin vor dem Laden zur Bot-UID und Bot-Primärgruppe, behält aber gezielt `twitchmedia` als Zusatzgruppe. Keine Rechteerweiterung der Datei oder der Unit. Metadatenbeleg `/tmp/tb-category-fix6-start-metadata.log`; Inhalte der produktiven Konfiguration wurden nicht gelesen.

Der Startvertrag wurde mit den unveränderten CLI- und Rollenwechsel-Sequenzen aus `4ddf3703` und dem Fix an eigenen synthetischen Dateien mit denselben Eigentümern und Rechten tatsächlich ausgeführt. Beide Prozesse starteten mit root-UID und begrenzten Fähigkeiten `CAP_SETUID`/`CAP_SETGID` sowie `NoNewPrivileges`. Alt: Exit 2 und derselbe `FileUnreadable`. Fix: Exit 0, echte UID 995, primäre GID 984, genau die Zusatzgruppe 985 und erfolgreiches begrenztes TOML-Lesen. Die Probe enthält weder Benachrichtigungszugang noch DB-, Broker- oder Dienstaufruf. Nachweise `/tmp/tb-category-fix6-role-io.log`, `/tmp/tb-category-fix6-role-probe-source.log` und `/tmp/tb-category-fix6-role-probe.OWvloW/`.

Unveränderte Wrapper-Suite: 16 bestanden, Exit 0, Nachweis `/tmp/tb-category-fix6-wrapper-tests.log`. Die vollständige Collector-/Watchdog-Suite ist tatsächlich beendet: Bibliothek 8 bestanden, Watchdog 8 bestanden, zusammen 16 bestanden, 0 fehlgeschlagen, 0 ignoriert, 0 ausgefiltert, Exit 0. Nachweis `/tmp/tb-category-fix6-tests.log`; Befehl wie in `FIXRUNDE-6.md`, einschließlich `SQLX_OFFLINE=true`, vorhandener Test-DSN, `TB_TEST_REQUIRE_DB=1`, `--all-targets` und `--include-ignored`. Der Compiler baute das Testprofil in 3 Minuten; die beiden Testprozesse liefen 6,58 s und 1,96 s.

Der erste Clippy-Lauf ohne `--no-deps` endet an einem `needless_borrows_for_generic_args` in `tb-chat/src/scam_pitch.rs:1502`, Exit 101, Nachweis `/tmp/tb-category-fix6-clippy.log`. Kein Fix außerhalb dieses Pakets. Die strikt ausgewählten Collector-/Monitoring-Ziele sind mit `--all-targets --no-deps -- -D warnings` tatsächlich geprüft und beendet, Exit 0, Nachweis `/tmp/tb-category-fix6-scoped-clippy.log`. Changed-file-Fmt mit Edition 2021 und Diff-Prüfung sind erfolgreich.

Das Urteil des lokalen Gates zum sauberen eigenen Fixrunde-6-Commit wird unter `/tmp/tb-category-fix6-gate.log` nachgewiesen; maßgeblich ist das Urteil für den in der Übergabe genannten SHA. Kritiker bleibt `gpt-6.1-sol`. Diese Auftragsakte behauptet keinen produktiven Start. Keine angewandte Migration verändert; kein produktiver Eingriff, Push oder Deploy durch den Fixer. Der Start in der produktiven Unit und der vollständige Live-Abschluss bleiben bei der Hauptsession.

## Fixrunde 7: Exakter Messbeweis auf Mikrosekundenauflösung

1. Der Textcast von `last_discovery_snapshot_at` rundet die Nanosekunden; SQLx speichert die abgeschnittenen Mikrosekunden. `native_cutover` schneidet deshalb vor dem Cast ausschließlich die Nachkommastellen nach der sechsten Ziffer ab und behält die exakte Gleichheit zum Messlauf bei. PID, Lease, beide frischen Heartbeats, `native_lease_active`, aktive Laufzeit-Lease und der Messzeitpunkt nach dem Cutover bleiben unverändert. Keine Rust-, Speicher- oder Migrationsänderung.
2. Tatsächlicher Start im zugewiesenen Fixer-Worktree war `4177752abf6a65b865de2817e170a9907001c38e`, identisch mit lokalem `origin/main`; `c7440400` ist dessen Vorfahr. Der neuere Stand wurde nicht zurückgesetzt. Unveränderte Wrapper-Baseline: 16 bestanden, 0 ignoriert, Exit 0, `/tmp/tb-category-fix7-wrapper-baseline.log`. Die neuen PG-Regressionen gegen das alte Prädikat scheitern in 6 Unterfällen, darunter die fälschliche Annahme der benachbarten Mikrosekunde; `/tmp/tb-category-fix7-precision-baseline.log`.
3. Vollständiger Lauf nach dem Fix: 18 Tests bestanden, 0 fehlgeschlagen, 0 ignoriert, Exit 0, `/tmp/tb-category-fix7-wrapper-tests.log`. Befehl: `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33100/postgres TB_TEST_REQUIRE_DB=1 python3 -I /home/nathanael/.worktrees/tb-kategoriesammler-legacy-merge/ops/systemd/test_deploy_twitch_pruefen.py -v`. Das unverändert extrahierte Wrapper-Prädikat lief gegen echten PostgreSQL mit synthetischen, lesenden CTE-Fixtures: 11 passende Zeitwerte einschließlich Rundungsgrenze, Sekundenüberlauf und Zeitzonenversatz bestätigt, 17 negative Messungs- und Zustandsfälle abgewiesen. `/tmp/tb-category-fix7-pg-proof.log` enthält die Einzelergebnisse und das Prädikat; `/tmp/tb-category-fix7-pg-version.log` die Serverversion. `bash -n` erfolgreich, Exit 0, `/tmp/tb-category-fix7-bash-syntax.log`.

Das lokale Gate prüft den eigenen sauberen Commit gegen `origin/main` mit `gpt-6.1-sol`; der Nachweis für den Übergabe-SHA liegt unter `/tmp/tb-category-fix7-gate.log`. Das verbindliche Urteil wird im Übergabebericht genannt. Die Testprozesse sind beendet. Der Originalworktree und sein Release-Build blieben unangetastet; kein Push, Deploy, produktiver Eingriff, Cleanup oder Speichernachtrag durch diesen Fixer.

# Gate-Mängel: Native Kategorieerfassung

Einziger Reviewer: lokaler Merge-Gate, Kritiker `gpt-6.1-sol`. Die Fixer bleiben bei diesem Kritiker. Kein Produktivschritt vor gültigem ALLOW.

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

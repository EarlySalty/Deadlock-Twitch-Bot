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

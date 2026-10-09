# Releasezustand und Abschlussnachweise

Aktualisierung2026-10-09: Die Hauptsession hat den begrenzten Systemjournalbeleg mit positiver Kontrolle erbracht. Der ursprüngliche Deploy-/Journal-/Funktionsbeleg gilt für `e0e9fde2`; der getrennt gemeldete aktuelle Prozessstand ist `fdee652a` und enthält unseren Fix laut Ancestor-Prüfung Exit0. [Hauptsession-Beleg](pruefung/main-session-final-proof.json), [Abschlussbericht](ABSCHLUSS.md). Keine erneute erhöhte Leseprüfung, kein neuer Build oder Deploy. Berichtspublikation und eigenes Cleanup werden im Abschlussbericht getrennt nachgeführt.

## Herkunft und tatsächlicher Deploy

- Dokumentationspaket `c7440400ce566393b812738e53bc5d7e56153637`, Produktivpaket einschließlich aller Tests `4177752abf6a65b865de2817e170a9907001c38e`: jeweils vollständiger gpt-6.1-sol-Gate ALLOW. Endgültige Produktivprüfung: [package2-final-gate.json](pruefung/package2-final-gate.json), regulärer Push Exit0: [package2-push.txt](pruefung/package2-push.txt).
- Aktueller Main-Release `e0e9fde20ec27f87acc8833e3d93dcdbe4d2934d`: frisch gefetcht, sauberer eigener Clone, sieben erforderliche ELF-Revisionen ohne dirty. Erster Releasebau50m37s, aktueller Main-Nachbau18m02s, beide Exit0. Drei Frontendbuilds Exit0; ihre Quellen und Migration beim Main-Vorziehen unverändert. [Provenienz](pruefung/release-provenance-current.json), [Build](pruefung/release-build-e0e9fde2.txt), [Frontends](pruefung/release-frontends.txt).
- Autorisierter serialisierter Deploy-Wrapper Exit0, vorhandene Migrationsunit vor Neustarts, keine manuelle Produktionskorrektur. [Deploy](pruefung/deploy-e0e9fde2.txt), [Migrationsunit](pruefung/release-migration-unit.txt). Unit-Metadaten allein liefern keinen eigenen Migrationszeitpunkt; neue Tabellen und vom regulären Worker geschriebene Ergebnisse sind live lesbar. Migration unverändert eingefroren, SHA256 `0a1adc4108c95f64125ab564bcbbc4fabb1a14e4ff15e4d16a3683c688491739`.
- Bot-PID1087471→2453730, Dashboard1112070→2454071, Coaching-Watch1112835→2455109. Drei Prozesse auf diesem Release, exe nicht gelöscht, NRestarts0. [Vorher](pruefung/release-before/processes-immediate.txt), [Nachher](pruefung/release-after-processes.txt). Alle sieben Livebinary-Hashes und alle drei Assetbäume bytegleich mit dem eigenen Build; neuer Binaryanker `twitch_vod_youtube_continuations` vorhanden. [Liveprovenienz](pruefung/release-live-provenance.json).

## Vollständiger tatsächlicher YouTube-Abgleich

83 VODs,77 historische Uploadbestätigungen.82 aktuelle Prüfungen:70 bestätigt,7 nicht abrufbar,5 nicht eindeutig zugeordnet; keine Prüffehler und keine laufende Verarbeitung. Alle70 vollständigen Bestätigungen besitzen nichtleere, verarbeitete Beobachtungen mit Video-ID und aktueller Konto-/Quellen-/Teilsnapshotbindung.61 private und10 öffentliche Beobachtungen;7 mit unbekannter Sichtbarkeit. Beobachtungen sind nicht dasselbe wie VOD-Anzahlen. Nicht abrufbar bedeutet ausdrücklich nicht gelöscht. [Daten](pruefung/release-live-first.txt), [Belegqualität](pruefung/release-validation/database.txt).

Der tatsächlich verbundene Kanal wurde vollständig gelesen: aktuelle Kontobindung, Generation1, kein offener Seitencursor, Scan vollständig und ohne Fehler,57 quellmarkierte Inventareinträge. Keine öffentliche Suche oder Namensheuristik, keine neuen Uploads durch Abgleich oder Prüfknopf.

| Altfall | Dauer | Ergebnis nach vollständigem Scan |
| --- | --- | --- |
| 10 | 24750s | keine eindeutige Zuordnung, keine Beobachtung, keine Teile |
| 11 | 19510s | keine eindeutige Zuordnung, keine Beobachtung, keine Teile |
| 2941 | 13830s | keine eindeutige Zuordnung, keine Beobachtung, bestehender unbestätigter Teil |
| 2995 | 14663s | keine eindeutige Zuordnung, keine Beobachtung, bestehender unbestätigter Teil |
| 2996 | 2830s | keine eindeutige Zuordnung, keine Beobachtung, bestehender unbestätigter Teil |

Alle fünf bleiben archived, ohne erfundene Uploadzeit, akzeptierten Teil oder Vollständigkeit. Das Ergebnis ist eine abgeschlossene Suche ohne belastbare Quellenzuordnung, kein Beweis einer Löschung oder eines fehlenden Videos.

## Sicherung der neuen Releasebelege

Der Featurebackup-Push `bg7x3zdku` von `fabf3d82b45e38a00c2de211a2e0fa6d6a30f952` scheiterte mit Exit1 an fünf generic-api-key-Funden im neu erzeugten Assetmanifest. Alle fünf Werte wurden durch erneutes Hashen der installierten Dateien als SHA256 von Bildern beziehungsweise einem Video belegt, nicht als Zugangsschlüssel. Fünf Zeilenumbrüche im JSON trennen Pfadnamen und Prüfsummen; sämtliche Schlüssel, Werte und81 Assetdigests bleiben nach JSON-Vergleich identisch. Die ursprünglichen Bytes bleiben im Commit erhalten. Der vollständige Snapshot wurde mit semantisch identischer unveränderter Hookpolicy erneut geprüft:0 Funde, Exit0. Keine Gate-/Hookänderung, keine Ausnahme, kein alternativer Pushweg. [Bereinigter Nachweis](pruefung/release-backup-scan.json). Diagnoseverzeichnis und Archiv gehören dieser Session und bleiben bis zur vollständigen Abnahme erhalten.

## Prüfgrenzen und historischer Journalblocker

31 verschiedene fokussierte Rustfälle bestanden,0 ignored; keine aktuelle Vollsuite behauptet. Archivbaseline historisch50/0/0 auf `0ecae1370f1a80d1a101249b5c932663d69be8af`, kein neuer Baselinelauf. Nichtblockierende Gate-NITs bleiben dokumentiert: teilweise Beobachtungen ersetzen gespeicherte UI-Links; drei TypeScript-Testfixtures ohne can_request. Keine weitere UI-Politur oder Browserrunde. Gebaute UI-Anker vorhanden, kein eingeloggter Produktions-E2E-Beweis.

Der Original-SQL-Reporter akzeptiert leere Beobachtungen im Quellguard. Gültige/leere SQL-Kontrollen und bestehender geprüfter API-Fixturefall sind verglichen; leere Beobachtungen sind kein Bestätigungsbeweis. Die zusätzliche aktuelle Leseprobe zeigt70 nichtleere verarbeitete Bestätigungen und0 leere Bestätigungen. Original-SQL und Originalbelege unverändert.

**Historischer Blocker, inzwischen durch Hauptsession-Beleg behoben:** Das Systemjournal ist in dieser Sitzung nicht lesbar. Explizites `journalctl --system` meldet „No journal files were opened due to insufficient permissions.“ Der zunächst leere Standardjournalaufruf zählt deshalb nicht als fehlerfreies Systemjournal. Die positive Kontrolle liefert0 Systemunit-Einträge; [Kontrolle](pruefung/release-journal-controlled.json) und [Systemkontrolle](pruefung/release-journal-system-control.json). `release-journal-errors.json` ist nur die unbestätigte Erstprobe, nicht der gültige Abschlussbeweis; `release-migration-journal.json` ist ebenfalls leer und kein Migrationsjournalbeweis.

Dieser benötigte Leseweg wurde einmalig von der Hauptsession ausgeführt und ist beendet; [Nachweis](pruefung/main-session-final-proof.json). Die früheren erfolglosen Worker-Proben bleiben als historische Originalbelege erhalten. Reguläre Abschlussdokumentation und eigenes Cleanup folgen ohne neue erhöhte Zugriffe. Der ursprüngliche Stagingclone wurde vom regulären Deploy-Wrapper nach `/opt/deadlock/twitch/builds/e0e9fde20ec27f87acc8833e3d93dcdbe4d2934d` verschoben und eingefroren; Release-/Buildbäume bleiben geschützt. Bei der Abschlussinventur existiert der Hilfsbranch weder im eigenen Arbeitsrepo noch remote noch unter den losen/gepackten Branchrefs dieses eingefrorenen Clones. Es wird kein geschützter Gitref verändert. Eigene synthetische PostgreSQL ist bereits gestoppt, pg_ctl status Exit3; erhaltene Originalbelege werden vor der Entfernung des Arbeitsworktrees veröffentlicht.

# Integrationsvorbereitung 2: A02, B05 und B09

Drei getrennte Sol-Prüfrollen bereiten ihre bestehenden Fixes auf die aktuelle Integrationsbasis vor. Kein neuer Anwendungscodeauftrag. Der Main-Push bleibt bei Astra. Keine Veröffentlichung, kein Deploy, Restart oder Aufräumen durch diese Rollen.

## Pakete und vorhandene Abnahme

| Paket | Eigener Worktree | Erhaltener Head | Erlaubter ursprünglicher Diff |
|---|---|---|---|
| A02 | /home/nathanael/.worktrees/tb-vollreview-affiliate-eigentuemer | 3365e6b26b7473cb35f46ac72f895249d7a0d0c6 | handlers/affiliate.rs, handlers/affiliate_portal.rs |
| B05 | /home/nathanael/.worktrees/tb-vollreview-plattform-refresh | 9c11bf6ce4174d2e69a56431c38a00007f76da01 | handlers/platform_token.rs, handlers/plattform_oauth.rs |
| B09 | /home/nathanael/.worktrees/tb-vollreview-idor-fixture | 02f98b8bc1f2e9de170558284926064ad3dbb9d8 | auth/idor_e2e_tests.rs |

Pfade relativ zu rust/crates/tb-dashboard-api/src. Vorhandene Basis der drei Fixes: a8b5b5e986a1de0b8e2f981651f83bda9cf400dd. B02 ist inzwischen auf main als e98b7f016dbab373a5a8dd9490d158b136c97fec; keine andere Integration voraussetzen. Main frisch holen und fixierten tatsächlichen Stand dokumentieren. Keine Koordination über Sessionnachrichten.

Echte Fix-Kritiken für die genannten Heads sind ALLOW und durch Astra auf Sol geprüft. Originalrückgaben: A02/B05 in ABSCHLUESSE-08.json, B09 in ABSCHLUESSE-07.json. Das ersetzt keinen aktuellen Gate oder Quellbindungsnachweis. Alte Leerdiff-Kritiken sind ungültig und bereits ersetzt.

## Gemeinsame Arbeit

1. Bestehende Belege und tatsächliche Protokolle gezielt abnehmen. Teststatus nicht aus Namen oder Dateialter ableiten. Quellstand, Kommando, Setup, Flags, Ausgang und Loghash verbinden. Keine ganze Transcripts oder Gateimplementierungen ausgeben.
2. Originalbasis und Patch sichern. Eigenen Fix regulär und konfliktfrei auf frisches origin/main abgleichen. Gesamtdiff, range-diff und betroffene Quellhashes prüfen. Kein Patchverlust, keine neue Quelländerung. Bei Konflikt oder fachlich nötiger Änderung konkreten Blocker zurückgeben; keinen fremden Diff bereinigen.
3. Prüfungen für den tatsächlichen finalen Stand gemäß AUFTRAG.md belegen. Vorhandene identische geprüfte Quellbäume dürfen mit beweisbarer Bindung wiederverwendet werden. Zwischen a8b5b5e9 und e98b7f01 ändern sich OBS-Dateien, das ist kein reiner Dokumentationsabgleich. Zusammensetzung prüfen und nötige finale Crate-Prüfungen ausführen. Rote Baseline nicht als neue Regression oder grüne Suite darstellen.
4. Passenden lokalen Sol-Gate mit --model gpt-6.1-sol --effort high --timeout 1080. Basis und Head vorher/nachher erfassen. Keine Änderungen am Hook, Gatezustand oder Modellwahl. Ein gegen falsche Basis gerichteter Diff mit rückgängig gemachten fremden Fixes ist keine Integrationsfreigabe.

Neue fachliche Kritik ist bei exakt erhaltenem ursprünglichen Fixpatch nicht nochmals zu würfeln. Muss der Fix verändert werden, stoppen; Astra startet dafür einen frischen Fixer mit anschließender neuer Kritik.

## Zusätzliche Nachweise je Paket

A02: Prüfabschluss adbeb5d23f80960af, 159 Sol-Datensätze, Hash 69988232acf8ff523ce90d5f033159f1c71d43f424d812eaadc0a20f091ccadf. Kritik a0606b2a694d8f114, 65 Sol plus ein synthetischer Datensatz, Hash 9b3657ba2915e5b8767e0b20e1187a0ff057259f00c2cbfa3b1dd241b80ba1da. Drei neue Eigentumsregressionen bestanden, Suite 1336 bestanden/35 identische Baselinefehler, Clippy Exit 0 mit gleichen Warnungen. Baseline 1333 bestanden/35 rot. Paket-fmt 265 identische Abweichungen. Astra verifizierte Suitehashes b85da450e3ea2dcd2977ef4659dfc375df6d0e0f55159cf185e15f5d446aec24 und 28fcbe04b39c1ca9c9e7e400478634b280dc32d01153b7ffbfc7e6d04db83e66. Logs /tmp/tb-a02-3365e6b2-* und /tmp/tb-a02-a8b5b5e9-baseline-*.

B05: Prüfabschluss a8fb5b85c05dddcf9, 150 Sol-Datensätze, Hash 13bbde8f5568994094f4e1119ee12f6dcc8874011f069249171d48a83c0272d3. Kritik aa849a1f7267e4edb, 40 Sol, Hash 835f502ee09086ad74becb502eb1034e4a60a3b802cfb880777a1898a0e42bd7. 24 platform_token-, neun plattform_oauth- und sieben platform_store-Tests bestanden. Fixsuite 1336 bestanden/35 rot, Fix-Clippy Exit 0. Eigene volle Baseline und Baseline-Clippy fehlten nach 69 Minuten Slotwarten; alte Aufgabe bcforeo18 ist beendet, keine Kompilierung abgebrochen. Fehlende Belege zuerst mit der jetzt verfügbaren passenden A02-Baseline vergleichen: gleicher a8b5b5e9-Quellbaum, --locked, --include-ignored, --test-threads=1, dieselbe Toolchain und Test-DB laut originalen Rückgaben. Astra fand 33 identische normal benannte Testfehler; zwei zusätzliche Doctestfehler getrennt prüfen. Identische Namen allein beweisen keine identische Ursache. Keine pauschale Übernahme. B05-Logs /tmp/tb-b05-verify-9c11bf6c/; fix-suite.log SHA256 4137cb5a13a28cc3d0cf4f22d3de9e811e282ba6b1858c42d823efc43b40a435.

B09: minimale vier Einfügungen und zwei Löschungen an Testfixtures. Fixer a127d8b2f91d41dbe, 139 Sol-Datensätze, Hash ca6c49f6bff40ac859b28aeb3cfb642f8fa71f62fc39ecf23e660ec9a9dc8ca5; Kritik abbe7617ff0ff86f5, 38 Sol, Hash cbe007a9686c6322b7207b635acae15c6f2764829e5e34218f4f9c8f606ebd1b. Astra prüfte /tmp/tb-vollreview-b09-head-fokus.log: zwei bestanden, Hash 8f99273610b8c4258551aef6d599f0d16dd2aff0463ee11979083fd12dbd3292. Alte Baseline hat 31 Fehler, Fix 29; verschwunden sind genau beide IDOR-Tests, keine neuen benannten Fehler. Diese Suite lief ohne --include-ignored und ist nicht mit den 35 Fehlern anderer Pakete gleichzusetzen. Doctestphase hatte fehlendes Buildartefakt, separat nachgeholt: null ausgeführt, zwei ignoriert, Exit 0. Kein positiver Doctestabdeckungsnachweis. Clippy laut originaler Ausführung Exit 0. Neue Prüfbindung präzise dokumentieren.

## Betrieb und Ausgabe

Je Rolle exklusiv eigene Worktrees und `${PAKET}-INTEGRATIONSVORBEREITUNG.md` im Taskordner. Keine anderen Taskdateien verändern. Eigene Logs unter neuem /tmp-Verzeichnis. Keine Anwendungscodeänderung, Migration, Prod-DB, echte Konten, Secrets/ENV, Browserarbeit oder ai-coach. Keine weiteren Agenten, T3-Threads, ListAgents oder SendMessage. Git einzeln, literale absolute Pfade. Toolchain 1.97.1, SQLX_OFFLINE=1, cargo-slot, --jobs 1. Keine fremden Prozesse/Slots verändern, eigene laufende Prüfung vor neuen Starts prüfen. Kein Abbruch normaler Kompilierung wegen pauschalem Zeitlimit.

Rückgabe: festes Basis-/Headpaar, Patchgleichheit, Quellenbindung der einzelnen Prüfungen, Gate, verbleibende echte Lücken. Teilprüfungen nicht als vollständigen Abschluss deklarieren. Astra übernimmt den späteren tatsächlichen Main-Push.

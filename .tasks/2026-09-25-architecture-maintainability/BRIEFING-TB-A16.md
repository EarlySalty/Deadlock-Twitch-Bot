# TB-A16 — Legacy-Fallback route-by-route abbauen

Priorität: **P3** · Änderungsrisiko: **hoch**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A11](BRIEFING-TB-A11.md), [TB-A12](BRIEFING-TB-A12.md), [TB-A13](BRIEFING-TB-A13.md), [TB-A15](BRIEFING-TB-A15.md)
Quelle: [SOURCE.md](SOURCE.md), **R13**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Die Migrationskomplexität in rückrollbaren Schritten reduzieren, ohne Feature- oder Eventverlust.

## Betroffene Bereiche

Bestätigte native Rust-/Legacy-Routen; Proxy-Konfiguration im Code; Cutover-/Rollback-Dokumentation

## TODO

- [ ] Mit einer read-only Route beginnen und je Slice vollständige native Parität aus TB-A15 nachweisen.
- [ ] Nur die konkret ersetzte Fallback-Route entfernen; Restliste und Rückfallplan mit derselben PR aktualisieren.
- [ ] Mutierende Routen erst nach Auth-, Idempotenz-, Daten- und Fehlerverträgen bearbeiten; beobachtbare Event-Flows gesondert absichern.
- [ ] Finale Proxy-/Python-Abschaltung als eigene Entscheidung vorbereiten: keine unersetzten Routen, nachgewiesene Consumer-Parität und freigegebener Runtime-Smoke-/Rollbackplan.

## Tests und Abnahme

Contract-Suite für jeden Slice; kein Anwachsen der Proxy-Fläche; synthetischer Event-Replay ohne Verlust/Duplikate; finale Abschaltung bleibt bis zur gesondert autorisierten Live-Verifikation blockiert.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

Eine Route oder eng zusammengehörige Route-Gruppe pro PR. Finaler Fallback-Removal separat.

Branch-Vorschlag: `codex/twitch-tb-a16-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Dieser TODO-Auftrag autorisiert keinen Deploy, Neustart oder Produktions-Cutover. Keine Python-Dateien löschen, nur weil im statischen Review kein Aufrufer sichtbar war.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

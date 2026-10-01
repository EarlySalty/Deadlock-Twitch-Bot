# TB-A13 — Featurelokale Repository-Ports für SQLx einführen

Priorität: **P2** · Änderungsrisiko: **mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A08](BRIEFING-TB-A08.md), [TB-A10](BRIEFING-TB-A10.md), [TB-A12](BRIEFING-TB-A12.md)
Quelle: [SOURCE.md](SOURCE.md), **R11**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Fachliche Application-Logik unabhängig von SQLx testbar machen; Query-Eigentümerschaft bleibt beim Feature.

## Betroffene Bereiche

Bestätigte Hotspots in tb-chat, tb-monitoring, tb-analytics, tb-engagement, tb-social-media und APIs

## TODO

- [ ] Pro Slice genau einen fachlichen Use Case und seine Query-/Transaktionsgrenze auswählen; Partner-Persistenz aus TB-A10 als vorhandenen Baustein nutzen.
- [ ] Kleinen Repository-Port, SQLx-Adapter innerhalb des Features und InMemory/Fake-Testadapter trennen.
- [ ] Fehler, Transaktionen, Idempotenz und Berechtigungsscope im Vertrag ausdrücklich erhalten.
- [ ] Kanonische Migrationen und SQLx-Metadaten nur bei tatsächlichen Schema-/Query-Änderungen aktualisieren; weitere Slices erst nach stabilem Pilot planen.

## Tests und Abnahme

Unit-Tests der Application ohne DB; Integrationsverträge für den PostgreSQL-Adapter; Rollback, Fehlerfälle und Tenant-Filter; vollständiger SQLx-Schema-/Offline-Check.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

Eine Pilot-PR; danach jeweils ein Feature/Use Case pro PR. Keine workspaceweite gleichzeitige Adapter-Abstraktion.

Branch-Vorschlag: `codex/twitch-tb-a13-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Kein zentrales God-Repository in tb-db, keine fachliche Änderung und kein generisches ORM-Projekt.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

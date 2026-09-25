# TB-A10 — tb-bot main.rs auf Bootstrap und Komposition zurückführen

Priorität: **P1** · Änderungsrisiko: **mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md), [TB-A03](BRIEFING-TB-A03.md)
Quelle: [SOURCE.md](SOURCE.md), **R08**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Startlogik, externe Tools, Listener und fachliche SQL-Updates voneinander trennen.

## Betroffene Bereiche

rust/bin/tb-bot/src/main.rs; neue bootstrap-/composition-/adapter-Module; Partner-Persistenz

## TODO

- [ ] Boot-Reihenfolge, Shutdown, Listener-Retry und Tool-Pfadauflösung mit Charakterisierungstests erfassen.
- [ ] Konfigurations-/DB-Aufbau, Tool-Resolution und Listener-Lifecycle in klar benannte Bootstrap-Module verschieben.
- [ ] Direkte Partner-Updates in den fachlich zuständigen Repository-Adapter hinter einem kleinen Port verlagern; nicht alle Queries nach tb-db verschieben.
- [ ] main.rs auf nachvollziehbaren Ablauf load/build/run/shutdown reduzieren; Background-Tasks erhalten explizite Besitz- und Stop-Verträge.

## Tests und Abnahme

Startfehler, Port-belegt/Retry, fehlendes externes Tool und Shutdown sind mit Fakes prüfbar; Partner-Update behält Transaktions- und Fehlersemantik.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Tool-/Listener-Bootstrap. PR B: Partner-Repository. PR C: App-Lifecycle, ohne übriges Chat-Wiring gleichzeitig umzubauen.

Branch-Vorschlag: `codex/twitch-tb-a10-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine neuen Runtime-Defaults und kein Neustart eines echten Dienstes. Weitere Feature-Repositories gehören zu TB-A13.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

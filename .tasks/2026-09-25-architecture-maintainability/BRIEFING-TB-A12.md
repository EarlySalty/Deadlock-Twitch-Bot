# TB-A12 — tb-dashboard-api vertikal modularisieren

Priorität: **P2** · Änderungsrisiko: **mittel bis hoch**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md), [TB-A06](BRIEFING-TB-A06.md), [TB-A07](BRIEFING-TB-A07.md)
Quelle: [SOURCE.md](SOURCE.md), **R10**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

HTTP, Auth, Application-Logik und Integrationen trennen, zunächst innerhalb des bestehenden Crates.

## Betroffene Bereiche

rust/crates/tb-dashboard-api; Dashboard-State; composition, transport, auth und fachliche HTTP-Module

## TODO

- [ ] Routen, gemeinsame Auth-Middleware und fachliche Zuständigkeiten kartieren; einen kleinen AI-Slice als Pilot wählen.
- [ ] Handler nur die Application-Fassade ihres Bereichs sehen lassen; State und Ports explizit injizieren. Auth nicht in jedes Fachmodul kopieren.
- [ ] Weitere Slices für Analytics, Raid, OBS und Social nacheinander umstellen. Querzugriffe dokumentieren beziehungsweise vermeiden; Tests schützen HTTP-Verträge.
- [ ] Erst nach stabilen internen Grenzen eine Crate-Extraktion erwägen. Die im Review genannten LlmPort-/Reasoner-Beispiele gegen laufende Brain-Vertragsarbeit abgleichen.

## Tests und Abnahme

Positive und negative HTTP-Contract-Tests inklusive Auth, Fehlerformat und Scope-Isolation; Handler-Test mit Fake-Fassade; Architektur-Gate verhindert neue fachfremde Querimporte.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Pilot AI. Danach je eine PR für Analytics, Raid, OBS und Social. Optionale spätere Extraktion nur mit belegtem Nutzen.

Branch-Vorschlag: `codex/twitch-tb-a12-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Kein Big-Bang-Multi-Crate-Split. Abstimmung aus Projektkontext: keine neuen direkten Modell-/RAG-Nebenwege parallel zum typisierten Brain-Client aufbauen.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

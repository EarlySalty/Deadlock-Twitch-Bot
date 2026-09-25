# TB-A17 — Eine eindeutige aktuelle Architektur-Dokumentation herstellen

Priorität: **P3** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md)
Quelle: [SOURCE.md](SOURCE.md), **R14**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Contributors sollen sofort erkennen, welcher Runtime-Pfad aktuell, transitional oder legacy ist.

## Betroffene Bereiche

README.md; INDEX.md; docs/ARCHITECTURE.md; rust/docs/00-overview.md, 01-architecture.md und 04-cutover-plan.md

## TODO

- [ ] Dokumente anhand der verifizierten Runtime-Map aus TB-A01 als CURRENT, TRANSITIONAL oder LEGACY kennzeichnen; Geltungsbereich, Nachfolger und validierten Commit angeben.
- [ ] Eine aktuelle Einstiegskarte verlinken; Workspace, Composition Roots, Frontends und Cutover-Tabelle zusammenführen, ohne historische Abläufe zu löschen.
- [ ] Jeder spätere Refactoring-PR aktualisiert seine betroffenen Verträge/Doku. Abschließende Konsolidierung nach TB-A11, TB-A12 und TB-A16 erneut prüfen.
- [ ] Maintainer-Checkliste aus dem Review übernehmen: Dependency-Richtung, State-Klasse, Task-Lifecycle, Migrationen, Verträge, Secrets und tatsächlich laufende Tests.

## Tests und Abnahme

Links und Pfade stimmen; Runtime-Karte entspricht dem verifizierten Workspace; kein Legacy-Einstiegspunkt wird ohne Status als aktueller Produktionspfad dargestellt.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Statusmarkierungen und Current-Einstieg nach TB-A01. PR B: Abschlussabgleich nach den Refactorings/Legacy-Schritten.

Branch-Vorschlag: `codex/twitch-tb-a17-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine erfundene Live-Runtime-Karte. Ungeprüfte Deploy-Zustände explizit offen lassen, nicht aus vorhandenen Quellcodedateien ableiten.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

# TB-A01 — Ist-Baseline und Architektur-Dependency-Gate

Priorität: **P0** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: Keine; aktuelle Befundprüfung im Auftrag selbst erforderlich.
Quelle: [SOURCE.md](SOURCE.md), **R01**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Den aktuellen Zustand nachvollziehbar erfassen und zusätzliche unerlaubte Feature-Kopplung verhindern, bevor Code verschoben wird.

## Betroffene Bereiche

rust/Cargo.toml; rust/docs; Cargo-Manifeste; bestehende CI-/Architektur-Scripts

## TODO

- [ ] Aktuellen main-SHA, relevante offene/zusammengeführte PRs und vorhandene Checks erfassen. Jeden Review-Befund als bestätigt, bereits erledigt oder überholt mit Beleg klassifizieren; insbesondere Überschneidungen mit Issue #572 prüfen.
- [ ] Crate-Graph aus cargo metadata, Runtime-/Cutover-Tabelle und Zustandsinventar erzeugen. Foundation, Feature, Application-Fassade und Binary nachvollziehbar zuordnen.
- [ ] Erlaubte Abhängigkeitsrichtungen und bestehende Ausnahmen mit Begründung und zuständigem Folgeauftrag versionieren. Bestehende laterale Kanten nicht pauschal als Cargo-Zyklen bezeichnen.
- [ ] Gate in den vorhandenen PR-Workflow integrieren: neue unerlaubte Kante ablehnen, bestehende Ausnahmen sichtbar halten und bei Refactorings entfernen.

## Tests und Abnahme

Fixtures für erlaubte Kante, verbotene neue Kante und entfernte Ausnahme; reproduzierbarer Graph auf demselben Commit; Negativfall muss den Check tatsächlich rot machen.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Baseline und verifizierte Ausnahmen. PR B: prüfbares Dependency-Gate und CI-Anbindung.

Branch-Vorschlag: `codex/twitch-tb-a01-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Noch keine Feature-Extraktion und keine vollständige Doku-Umschreibung. Das Gate ersetzt vorhandene Review- und SQLx-Gates nicht.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

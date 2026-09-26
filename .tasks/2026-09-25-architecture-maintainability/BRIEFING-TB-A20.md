# TB-A20 — Inbox-Performance erst messen, dann begrenzt optimieren

Priorität: **OPTIONAL** · Änderungsrisiko: **mittel bei Änderung; Untersuchung niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A09](BRIEFING-TB-A09.md)
Quelle: [SOURCE.md](SOURCE.md), **R17**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Klären, ob sequenzielles spawn-und-await tatsächlich ein relevanter Engpass ist, ohne Ordering oder Panic-Isolation zu verlieren.

## Betroffene Bereiche

tb-monitoring/src/inbox_runtime.rs; synthetischer Workload/Benchmark; Ordering-Verträge

## TODO

- [ ] Aktuelle Lease-/Retry-/Panic-/Ordering-Semantik bestätigen und synthetische Messbasis für Durchsatz/Latenz erstellen.
- [ ] Belegen, welche Work-Types beziehungsweise Schlüssel unabhängig sind. Keine pauschale Parallelisierung aus der Beobachtung im Review ableiten.
- [ ] Nur bei gemessenem Nutzen begrenzte Parallelität für unabhängige Arbeit oder vereinfachte sequenzielle Ausführung evaluieren; Backpressure, Panic-Isolation und Shutdown erhalten.
- [ ] Ohne belegten Nutzen mit dokumentierter Entscheidung unverändert lassen; das ist ein gültiges Untersuchungsergebnis, kein fehlgeschlagenes Refactoring.

## Tests und Abnahme

Vorher/Nachher mit identischem Workload; Ordering je erforderlichem Schlüssel, Lease-Ablauf, Retry/Dead-Letter, Panic und Shutdown bleiben korrekt; kein Duplikat oder Verlust.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Untersuchung/Benchmark und Entscheidung. Nur nach positivem Beleg PR B: eng begrenzte Optimierung.

Branch-Vorschlag: `codex/twitch-tb-a20-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Optionaler Folgeauftrag, kein Blocker für die Architektur-Roadmap und keine behauptete aktuelle Performance-Störung.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

# TB-A04 — Coverage-Baseline messen und anschließend ratcheten

Priorität: **P0** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A03](BRIEFING-TB-A03.md)
Quelle: [SOURCE.md](SOURCE.md), **R03**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Testabdeckung messbar machen, ohne aus dem statischen Review eine erfundene Prozentzahl oder willkürliche Schwelle abzuleiten.

## Betroffene Bereiche

Rust-Coverage-Konfiguration; CI-Artefakte und PR-Zusammenfassung

## TODO

- [ ] cargo llvm-cov beziehungsweise den bereits vorhandenen Coverage-Pfad gegen den tatsächlichen Workspace und das isolierte Test-Harness prüfen.
- [ ] Zunächst report-only: Bericht mit Commit, Tool-Version, ausgeführten Suites, Messumfang und Ausschlüssen als CI-Artefakt veröffentlichen.
- [ ] Erst auf Basis stabiler Messungen eine nachvollziehbare Nicht-Verschlechterungs- oder Changed-Line-Regel festlegen. Baseline-Updates müssen reviewbar sein.

## Tests und Abnahme

Messung ist auf gleicher Basis vergleichbar; Bericht nennt nicht erfasste Bereiche; eine kontrollierte Abdeckungsverschlechterung wird nach Aktivierung des Ratchets erkannt.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Messung und Artefakt, ohne Prozent-Gate. PR B: begründeter Ratchet nach dokumentierter Baseline.

Branch-Vorschlag: `codex/twitch-tb-a04-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Kein pauschales 80-Prozent-Gate am ersten Tag. Coverage ersetzt keine Contract-, Security- oder Browser-Tests.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

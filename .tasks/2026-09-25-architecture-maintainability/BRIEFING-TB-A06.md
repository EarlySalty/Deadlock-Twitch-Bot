# TB-A06 — Globalen AI_STATE durch injizierten AiRuntime ersetzen

Priorität: **P1** · Änderungsrisiko: **mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md), [TB-A03](BRIEFING-TB-A03.md)
Quelle: [SOURCE.md](SOURCE.md), **R05**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

AI-Zustand und Lifecycle explizit machen, zunächst ohne Produkt- oder Persistenzänderung.

## Betroffene Bereiche

rust/crates/tb-dashboard-api/src/ai_state.rs; AI-Handler; Dashboard-AppState und Composition Root

## TODO

- [ ] Aktuelle Nutzer des Singletons sowie Analyse-Guards, Sessions, Modell-Limits und Bereinigung inventarisieren; Charakterisierungstests vorziehen.
- [ ] AiRuntime über Dashboard-State/Konstruktoren injizieren und produktive Zugriffe auf den globalen Singleton entfernen.
- [ ] Separate Runtime-Instanzen pro Test ermöglichen; Locks nicht über externe awaits halten; Zeit-/Lifecycle-Abhängigkeiten testbar machen.
- [ ] Bestehende Stunden-, Modell- und Session-Limits exakt erhalten. Die Zahlen im Review sind Ausgangshinweise und vor dem Umbau zu bestätigen.

## Tests und Abnahme

Zwei AppState-Instanzen beeinflussen sich nicht; parallele Tests sind isoliert; Analyse-Guard, Follow-up, Ablauf und Fehlerfälle behalten ihr Verhalten.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

Eine verhaltensneutrale DI-PR. Persistenz bewusst erst mit TB-A07.

Branch-Vorschlag: `codex/twitch-tb-a06-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Noch keine DB-Migration, keine Änderung an Prompts, Modellauswahl, Auth oder Antwortformat.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

# TODO-Plan: Architektur und Maintainability

**19 Umsetzungsaufträge + 1 optionale Untersuchung. Alles noch offen.**

Die Grundlage und ihre Grenzen stehen in [SOURCE.md](SOURCE.md); verbindliche Scope-/Sicherheitsregeln in [CONTRACT.md](CONTRACT.md). Jedes Briefing enthält einen separat ausführbaren Auftrag und bei größeren Themen mehrere kleine PR-Slices. Dies ist kein Implementierungs- oder Produktionsabschlussbericht.

## Start und Reihenfolge

**Als Erstes TB-A01 und TB-A02:** Baseline/Grenzen schaffen; die kleine fail-closed-Absicherung kann unabhängig und ohne Warten auf große Umbauten bearbeitet werden. Danach TB-A03/TB-A05, anschließend Coverage in TB-A04.

| Welle | Inhalt | Arbeitspakete |
|---|---|---|
| 1 – Absichern | Ist-Baseline, Architektur-Gate, Security, vollständige Tests, Coverage | TB-A01 bis TB-A05 |
| 2 – State und Zuständigkeiten | AI-DI/Persistenz, Shared Policies, Wakeups/Caches, schlanker Bootstrap | TB-A06 bis TB-A10 |
| 3 – Kleine Refactorings | Capability-Wiring, vertikale API-Grenzen, Repository-Ports, Frontend-Testmatrix | TB-A11 bis TB-A14 |
| 4 – Übergang abschließen | Legacy-Verträge und -Abbau, aktuelle Doku, Pins/Provenienz, Runtime-Preflight | TB-A15 bis TB-A19 |
| Optional | Inbox messen und nur bei belegtem Nutzen optimieren | TB-A20 |

Die Wellen sind keine künstliche Vollsperre: TB-A14 kann nach TB-A03 parallel zu Backend-Arbeiten starten. TB-A15 (nur Inventar/Harness), TB-A17 (Statusmarkierung) und TB-A18 können nach ihren einzelnen Voraussetzungen früh beginnen. Der tatsächlich riskante Legacy-Abbau TB-A16 wartet auf seine ausgewiesenen Abhängigkeiten. Eine Priorität bezeichnet Dringlichkeit, nicht die Anzahl der vorgeschriebenen Vorgänger.

## Einzelaufträge

| ID | Priorität | Auftrag | Voraussetzungen für Implementierung |
|---|---|---|---|
| [TB-A01](BRIEFING-TB-A01.md) | P0 | Ist-Baseline und Architektur-Dependency-Gate | — |
| [TB-A02](BRIEFING-TB-A02.md) | P0 | Pentest-Rate-Limit-Bypass fail-closed absichern | — |
| [TB-A03](BRIEFING-TB-A03.md) | P0 | Vollständige Rust-Workspace-Tests in die PR-CI aufnehmen | TB-A01 |
| [TB-A04](BRIEFING-TB-A04.md) | P0 | Coverage-Baseline messen und anschließend ratcheten | TB-A03 |
| [TB-A05](BRIEFING-TB-A05.md) | P0 | Leichte Security-Prüfungen vor dem Merge ergänzen | TB-A01 |
| [TB-A06](BRIEFING-TB-A06.md) | P1 | Globalen AI_STATE durch injizierten AiRuntime ersetzen | TB-A01, TB-A03 |
| [TB-A07](BRIEFING-TB-A07.md) | P1 | AI-Limits und wiederherstellbare Sessions persistieren | TB-A02, TB-A06 |
| [TB-A08](BRIEFING-TB-A08.md) | P1 | Shared Policies aus Feature-Crates entkoppeln | TB-A01, TB-A03 |
| [TB-A09](BRIEFING-TB-A09.md) | P1 | Wakeup-Registry und globale Caches explizit machen | TB-A01, TB-A03 |
| [TB-A10](BRIEFING-TB-A10.md) | P1 | tb-bot main.rs auf Bootstrap und Komposition zurückführen | TB-A01, TB-A03 |
| [TB-A11](BRIEFING-TB-A11.md) | P2 | Chat-Wiring und EventSub-Hooks nach Capability aufteilen | TB-A08, TB-A09, TB-A10 |
| [TB-A12](BRIEFING-TB-A12.md) | P2 | tb-dashboard-api vertikal modularisieren | TB-A01, TB-A06, TB-A07 |
| [TB-A13](BRIEFING-TB-A13.md) | P2 | Featurelokale Repository-Ports für SQLx einführen | TB-A08, TB-A10, TB-A12 |
| [TB-A14](BRIEFING-TB-A14.md) | P2 | Browser-, Fuzz- und Admin-Testmatrix ausbauen | TB-A03 |
| [TB-A15](BRIEFING-TB-A15.md) | P3 | Legacy-8779-Fläche inventarisieren und Contract-Harness bauen | TB-A01, TB-A03 |
| [TB-A16](BRIEFING-TB-A16.md) | P3 | Legacy-Fallback route-by-route abbauen | TB-A11, TB-A12, TB-A13, TB-A15 |
| [TB-A17](BRIEFING-TB-A17.md) | P3 | Eine eindeutige aktuelle Architektur-Dokumentation herstellen | TB-A01 |
| [TB-A18](BRIEFING-TB-A18.md) | P3 | Toolchain-, Dependency- und Brain-Provenienz vereinheitlichen | TB-A01 |
| [TB-A19](BRIEFING-TB-A19.md) | P3 | Runtime-Doctor und Release-Artefakt-Smoke vorbereiten | TB-A10, TB-A18 |
| [TB-A20](BRIEFING-TB-A20.md) | OPTIONAL | Inbox-Performance erst messen, dann begrenzt optimieren | TB-A09 |

## Parallelisierung ohne doppelte Arbeit

CI-/Security-Änderungen TB-A03/04/05/18 teilen Workflow-/Toolchain-Dateien und brauchen einen abgestimmten Besitzer pro Slice. TB-A06→TB-A07 ist eine zwingende Reihenfolge; das State-Refactoring nicht direkt mit einer Schemaumstellung vermischen. TB-A08 und TB-A09 sind fachlich trennbar, benötigen bei gemeinsamen Manifests dennoch abgestimmte Commits. TB-A11/TB-A12 bearbeiten unterschiedliche Integrationsbereiche, dürfen aber gemeinsame State-/Port-Verträge nicht unabhängig neu erfinden.

Die allgemeinen Implementierungsabhängigkeiten stehen in der Tabelle. Zusätzliche **Abschlussabhängigkeit**: TB-A17 beginnt früh, wird aber erst nach dem finalen Abgleich mit TB-A11, TB-A12 und TB-A16 vollständig abgenommen. TB-A16 bleibt für einen echten Produktions-Cutover zusätzlich von gesondert freigegebener Runtime-Verifikation abhängig.

## Abnahme und Übergabe

Ein fertiger Slice liefert Code/Tests, betroffene Vertrags-/Architektur-Doku, echte Check-Ergebnisse, Rücknahmehinweis und eine Aktualisierung des [Registers](REGISTER.md). Coverage-Werte, Performancegewinne oder Runtime-Parität ohne Messung dürfen nicht als erledigt erscheinen. Erst dann den zugehörigen TODO-Punkt abhaken.

**Nächster konkreter Auftrag:** [TB-A01](BRIEFING-TB-A01.md). Unabhängig parallel möglich: [TB-A02](BRIEFING-TB-A02.md).

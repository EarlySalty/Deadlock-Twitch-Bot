# Arbeitsvertrag für den Backlog

## Umfang dieses Dokumentationsauftrags

Nur Aufgabenplanung: 19 Umsetzungsaufträge und 1 optionale Untersuchung mit Priorität, Abhängigkeiten, Dateieinstiegen, TODOs, Tests, Abnahme und kleinen PR-Schnitten. Keine der vorgeschlagenen Codeänderungen ist dadurch implementiert oder abgenommen.

Zulässige Änderungen dieses Branches: neue Markdown-Dateien in diesem Task-Ordner und ein Navigationslink in `INDEX.md`. Vorhandene fremde Änderungen im Haupt-Checkout bleiben unangetastet. Veröffentlichung als eigener Branch und Draft-PR, ohne Auto-Merge.

## Regeln für spätere Umsetzung

1. Pro Slice zuerst aktuellen main-SHA, Review-Befund und parallele PRs prüfen. Bereits gelöste Punkte mit konkretem Nachweis abschließen, nicht doppelt bauen. Unbestätigte Befunde nicht als aktuelle Defekte ausgeben.
2. Ein klar abgegrenzter Branch/PR pro Slice. Gemeinsame Manifeste, Workflow-Dateien und Composition Roots vor paralleler Bearbeitung abstimmen; keine fremden Branches eigenmächtig mergen.
3. Auth, Commandsemantik, Limits, Ausgabeformate und Featureparität erhalten. Die fail-closed-Korrektur TB-A02 ist eine bewusst zu testende Sicherheitsänderung; keine sonstigen stillen Semantikwechsel.
4. Erst Charakterisierung/Verträge, dann Refactoring. Keine neuen versteckten Singletons, fachlichen Rückkanten oder direkten Modell-/RAG-Nebenwege. SQL-Eigentum bleibt featurelokal.
5. DB-Änderungen ausschließlich als reviewbare kanonische Migrationen mit SQLx-Vertrag, isoliertem Test und Rollback-/Kompatibilitätsplan. Keine produktiven Daten für Fixtures und keine echten Bot-Aktionen im Test.
6. Vorhandene CI-/Security-/Review-Gates ergänzen statt umgehen. Erfolg nur behaupten, wenn der entsprechende Test tatsächlich lief. Fremde Baseline-Fehler sichtbar benennen, nicht ignorieren oder willkürlich in diesem Slice reparieren.
7. **Kein Deploy, Merge nach main, Bot-Neustart, Produktions-Konfigurationswechsel, echte Nachricht oder Live-Cutover allein aufgrund dieses Backlogs.** Dafür ist ein gesonderter Auftrag erforderlich. CI-/Testumgebungen sind von echten Runtime-Nachweisen zu trennen.

## Statusmodell

`TODO` → `IN_PROGRESS` → `IN_REVIEW` → `DONE_REMOTE` → gegebenenfalls `DONE_RUNTIME`.
`BLOCKED` nennt konkrete Abhängigkeit/fehlenden Zugriff. `ALREADY_DONE` und `NOT_APPLICABLE` verlangen SHA/PR/Begründung. `DONE_REMOTE` heißt nicht deployed. Optional TB-A20 darf mit belegter „keine Optimierung nötig“-Entscheidung enden.

Owner, Implementierungs-PR, Tests und noch ausstehende Runtime-Abnahme werden in [REGISTER.md](REGISTER.md) geführt. Die TODO-Checkboxen in den Briefings werden erst mit Nachweis abgehakt.

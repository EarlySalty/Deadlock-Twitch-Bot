status: aktiv | 2026-10-01

# Auftrag: Coaching-Kontext-Pitch

## Ziel
Den ursprünglichen Auftrag des Branches `feat/coaching-context-pitch` abschließen oder belegbar feststellen, welche Änderungen bereits in `main` enthalten sind.

## Bestand und Scope
- Der Kern-Pitch aus `10adb3e5` ist Vorfahr von `origin/main` und damit bereits integriert.
- Der WIP-Commit `4d5c899a` enthält noch auftragsbezogene Korrekturen: Coaching-Anleitung auf `!dldc` umstellen, `!discord` für den Creator reserviert lassen und Regressionstest sowie Doku nachziehen.
- Die CBC-Änderung aus `ea1c85f3` ist semantisch in `origin/main` enthalten. Keine erneute Übernahme.
- Kein passender offener Coaching-PR gefunden. Die temporäre Regression-Patchdatei gehört laut Dateieigentümer `nathanael` und benennt denselben Testpfad, lässt sich auf dem Source-Head aber nicht anwenden. Sie bleibt unangetastet.

## Grenzen
Nur auftragsbezogene Änderungen im isolierten Branch. Keine Secrets/ENV lesen und keine fremden Worktrees verändern. Schwere Checks/Builds seriell unter `/tmp/deadlock-cargo-release.lock` und nur bei freiem Lock und ausreichendem RAM. Vor Deploy prüfen, ob der stets gestartete Migrator ausstehende TokenDB-Migrationen anwenden könnte; TokenDB-Owner bleibt exklusiv für seine Runtimeänderungen.

## Abschlusskriterien
Lokales Review-Gate aufgerufen, unabhängige Intent-Abnahme organisiert, Statusdatei aktualisiert. Gate-Deny wird nicht umgangen.

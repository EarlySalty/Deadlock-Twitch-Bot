# TB-A03 — Vollständige Rust-Workspace-Tests in die PR-CI aufnehmen

Priorität: **P0** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md)
Quelle: [SOURCE.md](SOURCE.md), **R03**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Neue Workspace-Tests sollen automatisch tatsächlich laufen und nicht außerhalb selektiver Test-Slices liegen bleiben.

## Betroffene Bereiche

.github/workflows/rust-sqlx-check.yml; weitere Rust-Workflows; rust/.sqlx; Test-DB-Harness

## TODO

- [ ] Bestehende Suites und Required-Check-Namen inventarisieren; selektive wichtige Verträge nicht entfernen. Historische CI-Issues #738, #570 und #633 prüfen, aber ihren Fehlerzustand nicht ungeprüft übernehmen.
- [ ] cargo fmt --all -- --check sowie SQLX_OFFLINE=true cargo test --workspace --all-targets --locked und den vollständigen Build reproduzierbar einhängen. Den vorhandenen Clippy-Pfad mit TB-A05 abstimmen statt doppelt aufsetzen.
- [ ] Offline-SQLx-Kompilierung von Laufzeit-DB-Tests unterscheiden: saubere isolierte PostgreSQL-/Timescale-Testinstanz, kanonische Migrationen und gegebenenfalls abgestimmtes Brain-Schema bereitstellen.
- [ ] Testanzahl, ignorierte Tests und Ausnahmen ausweisen. Fehlende erforderliche Test-DB oder weggefilterte Tests dürfen nicht als erfolgreich ausgeführt erscheinen.

## Tests und Abnahme

Ein neu ergänzter Test in einem bisher nicht selektierten Crate läuft im PR; absichtlich fehlschlagende Fixture wird erkannt; frischer Schema-Replay und Offline-Build funktionieren unabhängig von Produktionsdaten.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: vollständiges Test-Harness und Baseline. PR B: vollständige Workflow-Anbindung. Bestehende Fehler separat beheben, nicht per continue-on-error verschweigen.

Branch-Vorschlag: `codex/twitch-tb-a03-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine Branch-Protection eigenmächtig ändern und keine Produktionsdatenbank verwenden. Coverage ist TB-A04.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

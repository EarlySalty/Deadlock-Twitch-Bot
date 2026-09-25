# TB-A02 — Pentest-Rate-Limit-Bypass fail-closed absichern

Priorität: **P0** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: Keine; aktuelle Befundprüfung im Auftrag selbst erforderlich.
Quelle: [SOURCE.md](SOURCE.md), **R02**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Eine Pentest-Option darf Schutzmechanismen in einer Produktionskonfiguration nicht still deaktivieren.

## Betroffene Bereiche

rust/crates/tb-dashboard-api/src/ai_state.rs; zuständige typisierte Settings; Startup-Validierung

## TODO

- [ ] Auf aktuellem main zuerst prüfen, ob DDC_PENTEST_DISABLE_RATE_LIMITS noch existiert und welche Werte/Build-Modi tatsächlich unterstützt werden.
- [ ] Produktionskonfiguration mit aktiviertem Bypass beim Start ablehnen. Ein expliziter Pentest-Modus beziehungsweise getrenntes Build-Feature darf den Ausnahmefall nur bewusst zulassen.
- [ ] Konfigurationsmatrix und Verhalten für fehlende, leere, gültige und ungültige Werte festschreiben; normale Auth- und Limit-Semantik unverändert lassen.

## Tests und Abnahme

Produktionsmodus plus Bypass endet mit nachvollziehbarem Startfehler; normale Produktion behält Limits; nur der ausdrücklich erlaubte Testmodus kann sie deaktivieren; keine Secret-Werte in Fehlermeldungen.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

Eine kleine Security-PR mit Regressionstest und Konfigurationsdokumentation; unabhängig vom späteren State-Refactoring lieferbar.

Branch-Vorschlag: `codex/twitch-tb-a02-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine Produktions-Environment-Variablen ändern. Keine pauschale Lockerung anderer Schutzmechanismen.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

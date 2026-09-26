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

- [ ] Auf aktuellem main prüfen, wo `DDC_PENTEST_DISABLE_RATE_LIMITS` gelesen wird und welche Testpfade den Bypass benötigen. Den ENV-Schalter als Konfigurationsweg entfernen; keine ENV-Datei und keine Environment-Variable für Konfiguration verwenden.
- [ ] Produktivbetrieb muss den Bypass beim Start ablehnen. Falls Pentests ihn weiterhin benötigen, nur über einen ausdrücklich getrennten Test-Build zulassen; sonst den Bypass vollständig entfernen. Nicht geheime Laufzeitkonfiguration gehört in die normale Config-Datei, Secrets kommen aus Infisical.
- [ ] Zulässige Build-/Config-Kombinationen und das Verhalten bei fehlenden oder ungültigen Werten festschreiben; normale Auth- und Limit-Semantik unverändert lassen.

## Tests und Abnahme

Produktionsmodus plus Bypass endet mit nachvollziehbarem Startfehler; normale Produktion behält Limits; nur ein ausdrücklich getrennter Test-Build kann sie bei weiter bestehendem Bedarf deaktivieren. Tests belegen, dass ein gesetztes `DDC_PENTEST_DISABLE_RATE_LIMITS` keine Konfiguration mehr steuert; keine Secret-Werte in Fehlermeldungen.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

Eine kleine Security-PR mit Regressionstest und Konfigurationsdokumentation; unabhängig vom späteren State-Refactoring lieferbar.

Branch-Vorschlag: `codex/twitch-tb-a02-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine ENV-Dateien oder Environment-Variablen als Konfigurationsweg beibehalten oder neu einführen. Keine pauschale Lockerung anderer Schutzmechanismen.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

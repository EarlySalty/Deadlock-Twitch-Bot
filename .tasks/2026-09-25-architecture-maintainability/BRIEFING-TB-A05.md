# TB-A05 — Leichte Security-Prüfungen vor dem Merge ergänzen

Priorität: **P0** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md)
Quelle: [SOURCE.md](SOURCE.md), **R04**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Schnelle Kontrollen in PRs ergänzen, während umfassende wöchentliche Scans erhalten bleiben.

## Betroffene Bereiche

.github/workflows/rust-security.yml; security-deep-scan.yml; Secret- und Review-Workflows

## TODO

- [ ] Aktuelle Trigger, Scopes und Überschneidungen prüfen; nur fehlende PR-Prüfungen ergänzen.
- [ ] Secret-Prüfung der relevanten Änderungshistorie, cargo audit/deny, passenden Clippy-Pfad und leichte Semgrep-Regeln einplanen. Umfang und Budget gegenüber den vollständigen Scans dokumentieren.
- [ ] SHA-Pins und minimale Berechtigungen erhalten; untrusted PR-Code nicht mit Produktions-Secrets ausführen. Required-Checks dürfen durch Path-Filter nicht unbemerkt fehlen.
- [ ] CodeQL-Rust weiter als ergänzende Kontrolle behandeln; den im Review genannten begrenzten Analyseumfang erneut prüfen, nicht als aktuelle Messung ausgeben.

## Tests und Abnahme

Ungefährliche synthetische Secret-/Regel-Testfälle blockieren wie vorgesehen; saubere PR besteht; fehlende Scan-Ergebnisse sind sichtbar; vorhandene wöchentliche Scans bleiben konfiguriert.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Secrets/Dependencies. PR B: Lint-/SAST-Anbindung und Budgetdokumentation, abgestimmt mit TB-A03.

Branch-Vorschlag: `codex/twitch-tb-a05-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Kein Scanner-Austausch nur für ein grünes Ergebnis und keine großflächigen neuen Ignore-Listen. P0 ist die Planungszuordnung dieses Backlogs; die Quelle priorisiert diese Maßnahme nicht separat.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

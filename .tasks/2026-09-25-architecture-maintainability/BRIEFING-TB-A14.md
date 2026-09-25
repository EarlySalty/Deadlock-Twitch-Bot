# TB-A14 — Browser-, Fuzz- und Admin-Testmatrix ausbauen

Priorität: **P2** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A03](BRIEFING-TB-A03.md)
Quelle: [SOURCE.md](SOURCE.md), **R12**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Vorhandene separate Suites tatsächlich ausführen und die schwächere Admin-Absicherung gezielt ergänzen.

## Betroffene Bereiche

bot/dashboard_v2/package.json und Tests; bot/admin_dashboard; Frontend-PR-Workflows

## TODO

- [ ] Aktuelle Testscript-Namen und vorhandene Browser-/Fuzz-/Calendar-/Viewer-Suites aus den Manifests bestätigen; keine im Review genannten Namen blind voraussetzen.
- [ ] Admin-Routing, Auth-Gating, Mutationen, Fehler- und Leerzustände priorisiert testen.
- [ ] Browser-Suites pfadbasiert einhängen, aber Protokoll-/Auth-Änderungen nicht durch zu enge Filter auslassen. Seeds und Artefakte für reproduzierbare Fuzz-/Browser-Fehler aufbewahren.
- [ ] Bestehenden Base/Head-Comparator erhalten, Baseline-Testschuld sichtbar machen und Rückbaukriterien dokumentieren.

## Tests und Abnahme

Repräsentative Routing-/Auth-/API-Änderungen lösen die richtigen Suites aus; absichtliche Regression wird rot; unveränderte Pfade führen nicht zu fehlenden Required-Checks.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Admin-Verträge. PR B: Browser-Matrix. PR C: Fuzz-/Protocol-Anbindung und Baseline-Schuld.

Branch-Vorschlag: `codex/twitch-tb-a14-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine Dashboard-Neugestaltung und keine Reparatur unrelated Features in derselben PR.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

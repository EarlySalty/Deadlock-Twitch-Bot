# TB-A15 — Legacy-8779-Fläche inventarisieren und Contract-Harness bauen

Priorität: **P3** · Änderungsrisiko: **mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md), [TB-A03](BRIEFING-TB-A03.md)
Quelle: [SOURCE.md](SOURCE.md), **R13**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Die tatsächliche Fallback-Fläche messbar machen, bevor einzelne Routen entfernt werden.

## Betroffene Bereiche

Python/Rust-Internal-API; Legacy-Proxy/Fallback; rust/docs/04-cutover-plan.md; Contract-Fixtures

## TODO

- [ ] Pro Route Methode, Auth, Request/Response, Status/Header, JSON-Naming, Nebenwirkungen, Owner, native Entsprechung und Fallback erfassen; bereits entfernte Routen ausdrücklich markieren.
- [ ] Hermetische Python/Rust-Vertragsfixtures oder gleichwertige gespeicherte Verträge mit synthetischen Daten aufbauen.
- [ ] Erfolgs-/Fehlerfälle, Berechtigungen, Idempotenz und zeit-/reihenfolgesensitive Events vergleichen; Chat/EventSub-Verträge nicht auf JSON-Formate reduzieren.
- [ ] Vertragsprüfung in CI und eine nachvollziehbare Liste verbleibender Proxy-Routen ergänzen.

## Tests und Abnahme

Bekannte künstliche Naming-, Auth-, Status- oder Nebenwirkungsabweichung wird erkannt; keine Produktions-Endpoints und keine echten Nachrichten werden verwendet.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Route-/Event-Inventar. PR B: Harness und erste kritische Verträge. Weitere Fixtures inkrementell ergänzen.

Branch-Vorschlag: `codex/twitch-tb-a15-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Noch keinen Proxy abschalten. P3 entspricht dem Legacy-Arbeitsstrom des Reviews; Inventar/Harness können nach den P0-Gates früh parallel beginnen.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

# TB-A08 — Shared Policies aus Feature-Crates entkoppeln

Priorität: **P1** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md), [TB-A03](BRIEFING-TB-A03.md)
Quelle: [SOURCE.md](SOURCE.md), **R06**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Monitoring soll nicht für ein reines Presence-Prädikat das gesamte Chat-Feature importieren müssen.

## Betroffene Bereiche

tb-monitoring; tb-chat; tb-domain oder neues reines Policy-Leaf; spätere tb-internal-api-Policy

## TODO

- [ ] is_passive_lurker_channel und tatsächliche weitere Nutzungen der Monitoring→Chat-Abhängigkeit bestätigen.
- [ ] Reine Typen/Funktionen in ein klar abgegrenztes Leaf-Modul verschieben; tb-domain nur nutzen, wenn die Policy wirklich domänenrein bleibt.
- [ ] Aufrufer und Manifeste umstellen; die Architektur-Ausnahme erst entfernen, wenn die gesamte betroffene Kante entfallen ist.
- [ ] Spam-Distinktivitätsregeln der Internal API als eigenen zweiten Slice prüfen, nicht ungeprüft in dieselbe Abstraktion pressen.

## Tests und Abnahme

Tabellarische Charakterisierung für bisheriges Verhalten; Leaf hat keine DB-, HTTP- oder Tokio-Abhängigkeit; Cargo-Graph zeigt entfernte Kante.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Presence-Policy. PR B nur bei bestätigtem Bedarf: eigenständig abgegrenzte Spam-Policy.

Branch-Vorschlag: `codex/twitch-tb-a08-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine fachlichen Moderations-/Presence-Regeln ändern und kein allgemeines Sammel-Crate für beliebige Helfer erzeugen.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

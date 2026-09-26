# TB-A09 — Wakeup-Registry und globale Caches explizit machen

Priorität: **P1** · Änderungsrisiko: **mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md), [TB-A03](BRIEFING-TB-A03.md)
Quelle: [SOURCE.md](SOURCE.md), **R07**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Versteckte globale Kopplung entfernen, ohne sinnvolle kurzlebige Caches pauschal in die Datenbank zu verschieben.

## Betroffene Bereiche

tb-monitoring/src/inbox_runtime.rs; Requeue-/Store-/API-Verbindungen; bestätigte Auth-, Overlay-, Analytics- und Chat-Caches

## TODO

- [ ] Zustände als ephemeral cache, distributed correctness state oder test-only klassifizieren; AI-State bleibt bei TB-A06/TB-A07.
- [ ] Globale Weak<Notify>-Registry durch injizierte Wakeup-Handles ersetzen; Store, Handler und Runtime im Composition Root explizit verbinden.
- [ ] Bei verbleibenden Caches Besitzer, Größenlimit, Ablauf, Invalidierung und Restart-Semantik dokumentieren und testbar kapseln.
- [ ] Korrektheitsrelevanten Zustand nicht als Cache tarnen; notwendige Persistenz als separaten kleinen Slice planen.

## Tests und Abnahme

Requeue weckt die richtige Runtime; mehrere Instanzen/Tests sind getrennt; Shutdown hinterlässt keine Registrierung; Polling-/Retry-Fallback verliert keine Arbeit; Cache-Invalidierung und Grenzen sind getestet.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Inbox-Wakeup-Handle. Weitere PRs jeweils für genau einen bestätigten Cache-Bereich.

Branch-Vorschlag: `codex/twitch-tb-a09-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine gleichzeitige Änderung der Inbox-Parallelität; diese Entscheidung bleibt TB-A20 vorbehalten.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

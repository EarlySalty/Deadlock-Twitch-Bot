# TB-A07 — AI-Limits und wiederherstellbare Sessions persistieren

Priorität: **P1** · Änderungsrisiko: **mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A02](BRIEFING-TB-A02.md), [TB-A06](BRIEFING-TB-A06.md)
Quelle: [SOURCE.md](SOURCE.md), **R05**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Korrektheitsrelevanter AI-Zustand darf weder pro Restart noch pro Prozess ein neues Budget erhalten.

## Betroffene Bereiche

AI-Session-/Rate-Limit-Port; PostgreSQL-Adapter; kanonische Migrationen; AI-Handler

## TODO

- [ ] AiSessionStore oder gleichwertigen kleinen Port definieren; InMemory-Testadapter von produktivem persistentem Adapter trennen.
- [ ] Budgetreservierung atomar und mit eindeutigem Scope/Zeitraum umsetzen. Verhalten bei Providerfehlern, Abbruch und Wiederholung ausdrücklich festlegen; bestehende Budgetsemantik nicht nebenbei ändern.
- [ ] Wiederherstellungswürdige Sessions mit Ablauf und Bereinigung speichern. Laufende Tasks nicht blind serialisieren; Wiederanlauf-/Abbruchzustand gesondert festlegen.
- [ ] Additive Migration, SQLx-Vertragsupdate und Daten-/Rollbackplan vorbereiten; auf DB-Fehler nicht unbemerkt auf unlimitierte lokale Verarbeitung ausweichen.

## Tests und Abnahme

Zwei Instanzen konkurrieren um dasselbe Budget ohne Überschreitung; Restart setzt Budget nicht zurück; Tenant-Isolation, TTL-Grenzen, Cleanup, DB-Ausfall und Wiederholung sind geprüft.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Port, Schema und Testadapter. PR B: atomarer Limit-Adapter. PR C: Session-Persistenz und Wiederanlauf.

Branch-Vorschlag: `codex/twitch-tb-a07-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine Migration gegen Produktion ausführen. Keine automatische Freischaltung zusätzlicher AI-Nutzung.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

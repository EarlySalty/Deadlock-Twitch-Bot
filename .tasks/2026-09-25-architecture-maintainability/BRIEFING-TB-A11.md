# TB-A11 — Chat-Wiring und EventSub-Hooks nach Capability aufteilen

Priorität: **P2** · Änderungsrisiko: **mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A08](BRIEFING-TB-A08.md), [TB-A09](BRIEFING-TB-A09.md), [TB-A10](BRIEFING-TB-A10.md)
Quelle: [SOURCE.md](SOURCE.md), **R09**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Große Wiring-Dateien fachlich verständlicher machen, statt sie nur nach Zeilenzahl zu zerschneiden.

## Betroffene Bereiche

rust/bin/tb-bot/src/chat_wiring.rs; eventsub_hooks.rs; neue composition/chat- und EventSub-Module

## TODO

- [ ] Capabilities und Lifecycle inventarisieren: Token, Subscriptions, Moderation, Commands, Chatter-Tracking, Promotions, globale Bans und Adapter.
- [ ] Capability-Builder/RuntimeHandles mit eindeutigem Start/Stop-Vertrag einführen. Bestehende Reihenfolge, Wiederanlauf und Fehlerweitergabe festhalten.
- [ ] Zuerst Chat-Wiring in kleinen Slices verschieben; EventSub-Hooks anschließend separat nach Ereignis-/Capability-Verantwortung schneiden.
- [ ] Jeder Slice erhält Paritätsnachweis für betroffene Commands, Limits, Auth, Ausgabe und Event-Verarbeitung.

## Tests und Abnahme

Registrierung genau einmal; Start/Stop ohne verlorene Tasks; Replay von repräsentativen Chat-/EventSub-Events mit Fakes; keine doppelten externen Aktionen.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

Mehrere kleine PRs: Token/Subscriptions; Moderation/Commands; Tracking/Promotions/Bans; anschließend EventSub-Hooks. Keine Sammel-PR für alle Bereiche.

Branch-Vorschlag: `codex/twitch-tb-a11-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine Command-Umbenennung, neue Features, echten Chat-Nachrichten oder Änderungen der Produktionsschalter.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

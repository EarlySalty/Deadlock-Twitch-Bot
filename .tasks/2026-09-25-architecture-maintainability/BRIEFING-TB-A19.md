# TB-A19 — Runtime-Doctor und Release-Artefakt-Smoke vorbereiten

Priorität: **P3** · Änderungsrisiko: **niedrig bis mittel**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A10](BRIEFING-TB-A10.md), [TB-A18](BRIEFING-TB-A18.md)
Quelle: [SOURCE.md](SOURCE.md), **R16**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Fehlende Runtime-Abhängigkeiten und unbrauchbare Release-Artefakte vor einem Start sichtbar machen.

## Betroffene Bereiche

Bestehende Preflight-/Release-Scripts; tb-bot Bootstrap/Doctor; yt-dlp-/ffmpeg-Auflösung

## TODO

- [ ] Vorhandene Preflight-Arbeit zuerst abgleichen. Planungsbasis enthält bereits Merge-PRs #976 und #977 mit Deploy-Preflight-Titeln; deren konkreten Scope prüfen, nicht als fehlend nachbauen.
- [ ] Read-only Doctor beziehungsweise vorhandenen Preflight ergänzen: Config, erforderliche Secret-Präsenz ohne Werte, DB-Erreichbarkeit/Migrationsstand, ausführbare Tools mit Version und benötigte Pfade prüfen.
- [ ] Externe Tools versionieren/provenienzfähig dokumentieren; das tatsächlich verpackte Release-Artefakt in isolierter Testumgebung prüfen, nicht nur den Source-Checkout.
- [ ] Fehlermeldungen und maschinenlesbare Exit-Codes definieren. Erforderliche Prüfungen dürfen weder Services starten noch Daten migrieren oder echte externe Aktionen ausführen.

## Tests und Abnahme

Fixtures für fehlendes/defektes Tool, falsche Konfiguration, fehlende Migration, unpassendes Artefakt und saubere Installation; sensible Werte bleiben verborgen; Wiederholung ist nebenwirkungsarm.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: vorhandenen Preflight erweitern/zusammenführen. PR B: Release-Smoke mit Fixtures und Betriebsdokumentation.

Branch-Vorschlag: `codex/twitch-tb-a19-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Kein echter Deploy-Smoke gegen Produktion im Rahmen der TODO-Erstellung. P3 ist die Backlog-Zuordnung der Deployment-Empfehlung, keine separate Prioritätstabelle der Quelle.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.

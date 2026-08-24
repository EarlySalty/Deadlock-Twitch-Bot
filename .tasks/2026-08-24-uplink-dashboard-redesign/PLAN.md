# Plan: Uplink-Dashboard neu ordnen

status: erledigt
datum: 2026-08-24
klasse: mittel
research: .tasks/2026-08-24-uplink-dashboard-redesign/RESEARCH.md

## Ziel

Fertig, wenn `/twitch/uplink` den Streamstatus einmal kompakt im Kopf zeigt, OBS als Vier-Schritt-Flow führt, Plattformziele mit Markenlogos klar gewichtet, Offen-Zustände speichert und Docks sowie Hilfe in den Sekundärbereich verschiebt; alle bestehenden Uplink-Verträge bleiben grün.

## Nicht-Ziele

- Kein echter OBS-Verbindungsdetektor ohne Backend-Signal.
- Keine API-, Relay-, Preis-, Auth- oder Navigationsänderung.
- Keine Plattformlimits oder festen Qualitätsüberschreibungen.

## Milestones

### M1 — Strukturvertrag rot
Änderungen: `bot/dashboard_v2/src/pages/Uplink.layout.test.tsx`
Erwarteter Zwischenzustand: Der neue Test fordert semantischen OBS-Stepper, Plattform-Metadaten, lokale Markenlogos und gespeicherte Sekundärbereiche und schlägt auf dem Ausgangsstand gezielt fehl.
Validierung: `node --import tsx --test src/pages/Uplink.layout.test.tsx`
Stop-Regel: Stoppen, wenn der Test aus Infrastrukturgründen statt wegen fehlender UI-Struktur scheitert.

### M2 — Informationshierarchie und A11y
Änderungen: `bot/dashboard_v2/src/pages/Uplink.tsx`, `bot/dashboard_v2/src/pages/UplinkZiel.tsx`
Erwarteter Zwischenzustand: Header-Streamstatus, OBS-Stepper, Plattformkarten und Sekundär-Disclosures erfüllen den Strukturvertrag; Formfelder und dynamische Meldungen sind programmatisch beschriftet.
Validierung: `node --import tsx --test src/pages/Uplink.layout.test.tsx`
Stop-Regel: Stoppen, wenn bestehende Speicher-/Pause-/Qualitätslogik geändert werden müsste.

### M3 — Dashboard-Regression und visuelle Prüfung
Änderungen: nur Fixes innerhalb des erlaubten Frontend-Scopes
Erwarteter Zwischenzustand: Tests, Lint und Production-Build sind grün; Desktop und 320-Pixel-Reflow sind visuell geprüft.
Validierung: `npm test && npm run lint && npm run build`
Stop-Regel: Bei Fehlern nicht deployen; Ursache im eigenen Diff beheben oder als belegte Baseline abgrenzen.

### M4 — Review, Merge und Live-Beweis
Änderungen: Review-Befunde, Taskstatus
Erwarteter Zwischenzustand: frisches Read-only-Review bestätigt Contract und Diff; Feature ist auf `main`, ausgeliefert und live geprüft.
Validierung: `python3 /home/naniadm/Documents/claude-config/bin/diff-policy.py /home/nathanael/repos/Deadlock-Twitch-Bot origin/main`
Stop-Regel: Bei bestätigtem Blocking-Finding nicht mergen.

## Verlauf

- 2026-08-24: Research verifiziert; Strukturvertrag auf dem Ausgangsstand gezielt rot.
- 2026-08-24: Uplink-Hierarchie, Markenlogos, Reconnect-Karte und lokale Disclosure-Persistenz integriert.
- 2026-08-24: Redundante Statusleiste auf User-Wunsch entfernt; Streamstatus bleibt einmal im Kopf.
- 2026-08-24: Browser-QA auf 1440 px und 320 px, Reload-Persistenz sowie Logos belegt; 144/144 Tests, Lint ohne Fehler und Production-Build grün.
- 2026-08-24: Gezielte Fix-Nachprüfung mit 0 Blockern und 0 wichtigen Befunden abgeschlossen.

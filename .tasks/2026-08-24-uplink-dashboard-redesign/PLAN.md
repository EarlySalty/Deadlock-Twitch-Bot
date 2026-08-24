# Plan: Uplink-Dashboard neu ordnen

status: aktiv
datum: 2026-08-24
klasse: mittel
research: .tasks/2026-08-24-uplink-dashboard-redesign/RESEARCH.md

## Ziel

Fertig, wenn `/twitch/uplink` belegbare Zustände oben zusammenfasst, OBS als kompakten Vier-Schritt-Flow zeigt, Plattformziele klar gewichtet und Docks sowie Hilfe standardmäßig eingeklappt sind; alle bestehenden Uplink-Verträge bleiben grün.

## Nicht-Ziele

- Kein echter OBS-Verbindungsdetektor ohne Backend-Signal.
- Keine API-, Relay-, Preis-, Auth- oder Navigationsänderung.
- Keine Plattformlimits oder festen Qualitätsüberschreibungen.

## Milestones

### M1 — Strukturvertrag rot
Änderungen: `bot/dashboard_v2/src/pages/Uplink.layout.test.tsx`
Erwarteter Zwischenzustand: Der neue Test fordert Statusleiste, semantischen OBS-Stepper, Plattform-Metadaten und zwei geschlossene Sekundärbereiche und schlägt auf dem Ausgangsstand gezielt fehl.
Validierung: `node --import tsx --test src/pages/Uplink.layout.test.tsx`
Stop-Regel: Stoppen, wenn der Test aus Infrastrukturgründen statt wegen fehlender UI-Struktur scheitert.

### M2 — Informationshierarchie und A11y
Änderungen: `bot/dashboard_v2/src/pages/Uplink.tsx`, `bot/dashboard_v2/src/pages/UplinkZiel.tsx`
Erwarteter Zwischenzustand: Statusleiste, OBS-Stepper, Plattformkarten und Sekundär-Disclosures erfüllen den Strukturvertrag; Formfelder und dynamische Meldungen sind programmatisch beschriftet.
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

- 2026-08-24: Research verifiziert; Implementierung noch nicht begonnen.


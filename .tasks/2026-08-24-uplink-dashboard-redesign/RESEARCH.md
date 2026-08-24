# Research: Uplink-Dashboard neu ordnen

status: aktiv
datum: 2026-08-24
klasse: mittel

## Auftrag

Die Uplink-Seite zeigt belegbare Zustände sofort, führt kompakt durch OBS und priorisiert die Plattformziele, ohne Relay- oder API-Verhalten zu verändern.

## Beobachtungen (belegt, Datei:Zeile)

- `bot/dashboard_v2/src/pages/Uplink.tsx:366` hält die vier Queries für Zugang, Hilfe, Ziele und Caps; der Umbau kann alle vorhandenen Daten wiederverwenden.
- `bot/dashboard_v2/src/pages/Uplink.tsx:507` erzwingt derzeit den langen Zweispalter OBS/Docks gegen Ziele/Hilfe; Docks sind dauerhaft offen.
- `bot/dashboard_v2/src/pages/Uplink.tsx:537` rendert vier OBS-Schritte als vollständig offene Blöcke statt als Liste oder Disclosure.
- `bot/dashboard_v2/src/pages/UplinkZiel.tsx:294` besitzt bereits native `<details>`-Karten mit Status, Qualität und den vorhandenen Aktionen.
- `bot/dashboard_v2/src/pages/UplinkZiel.tsx:208` speichert Qualitätsänderung, Zielstatus und Zugangsdaten über denselben bestehenden API-Vertrag.
- `bot/dashboard_v2/src/api/uplink.ts:13` definiert `live_status` als Twitch-Livebeobachtung; ein echter OBS-/Relay-Verbindungsstatus existiert im Frontendvertrag nicht.
- `bot/dashboard_v2/src/index.css:6` trennt Gold/Messing-Chrome ausdrücklich von Statusfarben; Plattformfarben dürfen diese Regel nicht brechen.
- `bot/dashboard_v2/tests/uplinkHelp.test.ts:153` prüft bereits, dass Hilfe-Kapitel eingeklappt starten, jedoch zu breit über alle `<details>` der Quelldatei.
- `bot/dashboard_v2/package.json:6` nutzt `node:test` ohne DOM-Testbibliothek; für diesen strukturellen Umbau passt ein fokussierter Quelltext-Vertragstest zum bestehenden Muster.

## Hypothesen (unbelegt — nie als Fakt weiterreichen)

- Keine. Der Datenvertrag und die vorhandenen UI-Komponenten reichen für die beauftragte Informationshierarchie aus.

## Wahrscheinlich zu ändernde Dateien

- `bot/dashboard_v2/src/pages/Uplink.tsx` — Statusleiste, OBS-Stepper, Sekundär-Disclosures und Seitenhierarchie.
- `bot/dashboard_v2/src/pages/UplinkZiel.tsx` — Plattformkopf, visuelles Gewicht und zugängliche Formularbeschriftung.
- `bot/dashboard_v2/src/pages/Uplink.layout.test.tsx` — struktureller Red-Green-Vertrag ohne neue Testabhängigkeit.

## Risiken / Seiteneffekte

- „OBS verbunden“ wäre eine falsche Behauptung; die Leiste muss stattdessen `Uplink bereit` und `Streamstatus live/offline/unbekannt` anzeigen.
- Ein kontrolliertes `<details open>` kann sich bei State-Updates unerwartet wieder öffnen; der Offen-Zustand muss über `onToggle` erhalten bleiben.
- Die SRT-Sicherheitswarnung darf durch das Einklappen nicht verschwinden, solange die Serveradresse sichtbar ist.

## Offene Fragen

- Keine.

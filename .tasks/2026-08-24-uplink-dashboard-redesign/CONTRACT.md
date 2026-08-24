status: aktiv
datum: 2026-08-24

# Contract: Uplink-Dashboard neu ordnen

## Ziel

Die bestehende Seite `/twitch/uplink` soll beim ersten Blick zeigen, ob OBS und die Plattformziele bereit sind, die OBS-Einrichtung in kurzen Schritten führen und aktive sowie noch nicht eingerichtete Ziele klar voneinander unterscheiden.

## Anforderungen

- REQ-01: Oberhalb des Inhalts steht eine kompakte Statusleiste für OBS/Uplink und alle vier Plattformziele; Zustände sind als Text und nicht nur als Farbe erkennbar.
- REQ-02: Die OBS-Einrichtung erscheint als kompakter, zugänglicher Stepper. Serveradresse und Pflichtschritte bleiben sofort erreichbar, längere Ausgabehinweise werden erst auf Wunsch geöffnet.
- REQ-03: Twitch, YouTube, Kick und TikTok erscheinen als klar getrennte Plattformkarten mit Logo/Initiale, gut sichtbarem Status und vorhandenen Qualitäts- und Speicherfunktionen.
- REQ-04: Aktive Ziele sind visuell stärker gewichtet; nicht eingerichtete Ziele bleiben kompakt und bieten einen eindeutigen Einstieg zum Einrichten.
- REQ-05: „Chat & OBS-Fenster“ und „Uplink-Hilfe“ stehen unterhalb des Hauptflows und sind standardmäßig eingeklappt.
- REQ-06: Lange Erklärtexte sind reduziert oder hinter semantischen Disclosure-Elementen verborgen; sicherheitsrelevante Hinweise bleiben ohne Hover zugänglich.
- REQ-07: Desktop und Mobilansicht sind bedienbar; Tastaturfokus, Disclosure-Zustände und Statusmeldungen sind für assistive Technik erkennbar.
- REQ-08: Bestehende API-Verträge, Speichern, Aktivieren/Deaktivieren, manuelle Qualitätswerte und laufende In-place-Neustarts bleiben funktional unverändert.

## Invarianten

- INV-01: Keine neue Plattformgrenze, kein festes Preset und keine stille Überschreibung wird eingeführt.
- INV-02: Gold und Messing bleiben Flächen-/Akzentfarben; Blau und Cyan bleiben Status- oder Diagrammfarben.
- INV-03: Ingest-Key und Serveradresse bleiben standardmäßig verdeckt; ein laufender Stream verhindert das Aufdecken weiterhin.
- INV-04: Bestehende Navigation, Auth-Gates und Relay-API bleiben unverändert.
- INV-05: Es wird keine Funktion beworben, die Backend oder Relay nicht unterstützen.

## Nicht-Ziele

- Keine Änderungen am Relay, an Datenbank, Auth, API oder Streaming-Lebenszyklus.
- Keine neuen OBS-Docks oder Plattformadapter.
- Keine Änderung an Preisen, Berechtigungen oder Warteliste.

## Erlaubter Änderungsbereich

- `bot/dashboard_v2/src/pages/Uplink.tsx`
- `bot/dashboard_v2/src/pages/UplinkZiel.tsx`
- `bot/dashboard_v2/src/**/__tests__/*uplink*`
- `bot/dashboard_v2/src/**/*.test.tsx`
- `bot/dashboard_v2/src/uplink*.css`
- `.tasks/2026-08-24-uplink-dashboard-redesign/*`

## Verbotener Änderungsbereich

- `rust/**`
- Datenbankmigrationen und Deployment-Konfiguration
- andere Dashboard-Seiten

## Offene Produktfragen

- Keine; reversible Detailentscheidungen folgen dem bestehenden Design-System und den gelieferten Strukturvorgaben.

## Amendments

- Der Änderungsbereich umfasst zusätzlich `bot/dashboard_v2/package.json`, ausschließlich um den neuen Strukturvertrag in den bestehenden `npm test`-Lauf aufzunehmen — entschieden vom Orchestrator am 2026-08-24; ein unverdrahteter Test wäre kein dauerhafter Regressionsschutz.
- Der Änderungsbereich umfasst zusätzlich `bot/dashboard_v2/src/assets/platforms/*.svg`; Twitch, YouTube, Kick und TikTok erhalten auf ausdrücklichen Wunsch des Users echte lokale Logos statt Buchstabenkacheln — entschieden vom User am 2026-08-24.

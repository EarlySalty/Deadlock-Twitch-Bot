# Evidence: Uplink-Dashboard neu ordnen

status: erledigt
datum: 2026-08-24
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- `bot/dashboard_v2/src/pages/UplinkZiel.tsx:294` — native `<details>`-Karte mit geschlossenem Zusammenfassungszustand.
- `bot/dashboard_v2/src/pages/Uplink.tsx:735` — Hilfe-Kapitel nutzen bereits browsernative, tastaturbedienbare Disclosures.
- `bot/dashboard_v2/src/components/onboarding/OnboardingWizard.tsx:140` — nummerierte Schritt- und Statusdarstellung als visuelles Muster.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- `bot/dashboard_v2/src/pages/Uplink.tsx:420` — `capsFuer` und die vorhandenen Queries liefern alle für Status und Karten nötigen Daten.
- `bot/dashboard_v2/src/pages/UplinkZiel.tsx:84` — `ZielKarte` kapselt Plattformformular, Vorbelegung, Speichern und Pausieren.
- `bot/dashboard_v2/src/motion/Rise.tsx:25` — respektiert den zentralen Reduced-Motion-Pfad.
- `bot/dashboard_v2/src/index.css:190` — `panel-card` und `card-glow` liefern die bestehende Gusseisen-/Gold-Sprache.

## Relevante Tests (laufen vorher, laufen nachher)

- `bot/dashboard_v2/tests/uplinkHelp.test.ts:153` — Hilfedisclosures starten zugeklappt.
- `bot/dashboard_v2/tests/uplinkEmpfehlung.test.ts:56` — OBS-Bitrate folgt gespeicherten Zielen und bleibt unverändert.
- `bot/dashboard_v2/tests/brandPalette.test.ts:68` — Chrome bleibt Gold/Messing; fremde Plattformfarben werden abgefangen.
- `bot/dashboard_v2/package.json:6` — vollständiger Dashboard-Testlauf über `node:test`.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- `bot/dashboard_v2/src/api/uplink.ts:26` — `/twitch/api/v2/uplink/me` mit Cookie-Session.
- `bot/dashboard_v2/src/api/uplink.ts:179` — bestehender PUT-Vertrag für Ziel, Zugangsdaten, Status und Qualität.
- `bot/dashboard_v2/src/api/uplink.ts:224` — Zielantwort enthält keine Stream-Schlüssel und unterscheidet `requested`/`effective`.
- `bot/dashboard_v2/src/pages/Uplink.tsx:555` — SRT-Adresse wird nur im sicheren Offline-Zustand aufgedeckt.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- `bot/dashboard_v2/src/pages/Uplink.tsx` — Informationshierarchie und Semantik.
- `bot/dashboard_v2/src/pages/UplinkZiel.tsx` — Plattformkarten und Form-A11y.
- `bot/dashboard_v2/src/pages/Uplink.layout.test.tsx` — neuer Strukturvertrag.

## Offene Architekturfrage

- keine

## Abschlussnachweise

- Browser: 320 px Viewport bei 320 px Dokumentbreite; keine horizontale Überbreite.
- Browser: Plattformkarte und OBS-Docks bleiben nach Aufklappen und Reload offen; gespeichert werden nur `ddl:uplink:disclosure:* = 0|1`.
- Browser: vier lokale SVG-Logos laden mit Markenfarben; doppelte Statusleiste ist nicht vorhanden.
- Tests: `npm test` mit 144 bestanden, 0 fehlgeschlagen.
- Qualität: `npm run lint` mit 0 Fehlern; 16 unveränderte Bestandswarnungen außerhalb des Uplink-Diffs.
- Build: `npm run build` erfolgreich, Produktionsbundle erzeugt.

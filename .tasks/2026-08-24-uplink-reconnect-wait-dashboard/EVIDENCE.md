status: erledigt
datum: 2026-08-24
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:501-518` — persönlicher PUT-Proxy ergänzt die stabile Partner-ID als Query und reicht den bereinigten JSON-Rumpf weiter.
- `bot/dashboard_v2/src/pages/Uplink.tsx:356-447` — Query + lokale Eingabe + Mutation + Erfolgs-/Fehleranzeige für Uplink-Einstellungen.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- `bot/dashboard_v2/src/api/uplink.ts:5-10` — `jsonRequest` mit Cookie-Credentials und JSON-Header.
- `bot/dashboard_v2/src/pages/Uplink.tsx:356-447` — `FELD_KLASSE`, `KNOPF_KLASSE`, `LABEL_KLASSE` und die bestehende Uplink-Kartenform.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:357-366` — `partner_id`, `relay_client` und `me_handler` als Auth-/Relay-Pfad.

## Relevante Tests (laufen vorher, laufen nachher)

- `bot/dashboard_v2/package.json:8` — `npm test` führt `uplinkTab.test.ts` und die übrigen Dashboard-Vertragstests aus.
- `bot/dashboard_v2/tests/uplinkTab.test.ts:353-405` — pure Uplink-Modelltests für Zustands- und Anzeigeverträge.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:960-988` — Wiremock-Test des persönlichen `/me`-Relay-Proxy.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- `../../repos/rs-relay/src/api/mod.rs:121` — Relay-Route `PUT /v1/me/reconnect-wait`.
- `../../repos/rs-relay/src/api/user.rs:396-441` — Request-/Response-Felder und Servergrenze.
- `rust/crates/tb-dashboard-api/src/lib.rs:467-484` — bestehender externer Dashboard-Routenraum `/twitch/api/v2/uplink/*`.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs` — Proxy-Handler und Wiremock-Test.
- `rust/crates/tb-dashboard-api/src/lib.rs` — Route.
- `bot/dashboard_v2/src/api/uplink.ts` — Typen und API-Funktion.
- `bot/dashboard_v2/src/pages/Uplink.tsx` — Karte im Streamer-Bereich.
- `bot/dashboard_v2/src/pages/uplinkModel.ts` und `bot/dashboard_v2/tests/uplinkTab.test.ts` — testbare Eingabelogik und Tests.

## Offene Architekturfrage

- keine

## Abschlussnachweis

- Frontend-Vertragstests: 94/94 gruen.
- Frontend-Build: TypeScript und Vite erfolgreich.
- Frontend-Lint: 0 Fehler, 15 bestehende Warnungen ausserhalb der geaenderten Uplink-Dateien.
- Rust: Wiremock-Proxy-Test 1/1 gruen; Clippy fuer `tb-dashboard-api` mit `-D warnings` gruen.
- `git diff --check`: sauber.
- Browser-/T3-Vorschau: in der Umgebung nicht verfuegbar; UI daher statisch gegen Build, Vertragstests und Quellpfad geprueft.

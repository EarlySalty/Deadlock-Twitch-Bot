# Evidence: Uplink-Warteliste im Admin-Modus

status: erledigt
datum: 2026-08-25
contract: CONTRACT.md

## Analoge Implementierungen

- `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx:315` nutzt `auth-status` für ausschließlich admin-sichtbare Oberfläche.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:89` führt Relay-Aufrufe über den bestehenden serverseitigen Proxy aus.
- `/home/nathanael/repos/rs-relay/src/api/admin.rs:24` enthält die vorhandene Freischaltaktion für einen Streamer.

## Bestehende Abstraktionen

- `bot/dashboard_v2/src/api/auth.ts:46` liefert `adminMode` und das CSRF-Token der aktuellen Session.
- `bot/dashboard_v2/src/api/core.ts:83` kapselt Cookie-Requests und einheitliche Fehlerantworten.
- `rust/crates/tb-dashboard-api/src/auth/level.rs:52` ist die zentrale Autorisierungsquelle.

## Relevante Tests

- `bot/dashboard_v2/src/pages/Uplink.layout.test.tsx:9` prüft Uplink-Strukturverträge ohne Textkopplung.
- `rust/crates/tb-dashboard-api/src/handlers/admin_mode.rs:171` belegt die Trennung zwischen normalem Partner und aktivem Admin-Modus.
- `/home/nathanael/repos/rs-relay/src/api/mod.rs:2315` prüft die bestehende Relay-Wartelistenantwort.

## Öffentliche Schnittstellen und Verträge

- `rust/crates/tb-dashboard-api/src/lib.rs:488` ist der bestehende Uplink-Routenzweig.
- `/home/nathanael/repos/rs-relay/src/api/mod.rs:132` nimmt Freischaltungen über `POST /v1/admin/users` an.
- `/home/nathanael/repos/rs-relay/src/api/mod.rs:138` liefert `GET /v1/admin/waitlist`.

## Änderungsfläche

- `bot/dashboard_v2/src/pages/Uplink.tsx` für die Admin-Wartelistenbox.
- `bot/dashboard_v2/src/api/uplink.ts` für Frontend-Verträge.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs` und `rust/crates/tb-dashboard-api/src/lib.rs` für sichere Proxy-Routen.

## Offene Architekturfrage

- keine

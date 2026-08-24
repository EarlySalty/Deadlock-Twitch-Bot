# Research: Uplink-Warteliste im Admin-Modus

status: erledigt
datum: 2026-08-25
klasse: mittel

## Auftrag

Admins sollen wartende Uplink-Streamer im bestehenden Dashboard sehen und sicher freischalten können.

## Beobachtungen (belegt, Datei:Zeile)

- `docs/architecture/dashboard/admin-mode.md:34` beschreibt den aktiven Admin-Modus als zentral aufgelöstes `DashboardAuthLevel::Admin`.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:89` kapselt Relay-Aufrufe bereits mit serverseitigem `X-Relay-Auth`.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:230` enthält die unverändert bleibende Nutzeraktion zum Eintragen in die Warteliste.
- `rust/crates/tb-dashboard-api/src/lib.rs:492` verdrahtet die vorhandene Uplink-Wartelistenroute im authentifizierten und CSRF-geschützten Router.
- `/home/nathanael/repos/rs-relay/src/api/admin.rs:46` schaltet vorhandene Nutzer frei und entfernt sie anschließend aus der Warteliste.
- `/home/nathanael/repos/rs-relay/src/api/admin.rs:846` liefert die bestehende Relay-Warteliste.
- `bot/dashboard_v2/src/api/auth.ts:46` stellt den effektiven Admin-Zustand und das Session-CSRF-Token bereit.
- `bot/dashboard_v2/src/pages/Uplink.tsx:683` enthält den zweispaltigen Hauptbereich, dessen rechte Spalte die Plattformkarten trägt.

## Hypothesen (unbelegt)

- keine

## Wahrscheinlich zu ändernde Dateien

- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs` für zwei admin-geschützte Relay-Proxys und die Reduktion der Antwort auf nicht geheime Felder.
- `rust/crates/tb-dashboard-api/src/lib.rs` für GET- und POST-Routen im vorhandenen Uplink-Namensraum.
- `bot/dashboard_v2/src/api/uplink.ts` für typisierte, CSRF-geschützte Aufrufe.
- `bot/dashboard_v2/src/pages/Uplink.tsx` für die nur im Admin-Modus sichtbare rechte Box.
- `bot/dashboard_v2/src/pages/Uplink.layout.test.tsx` für Sichtbarkeits-, Struktur- und Sicherheitsverträge.
- `bot/dashboard_v2/src/preview/fixtures.ts` für eine realistische visuelle Vorschau.

## Risiken / Seiteneffekte

- Eine reine Frontend-Sichtbarkeit wäre kein Schutz; jeder neue Handler muss den effektiven Admin-Modus selbst prüfen.
- Die Relay-Freischaltung liefert einen Ingest-Schlüssel; der Dashboard-Proxy muss ihn vor der Browserantwort verwerfen.
- Nach erfolgreicher Freischaltung muss die Wartelistenabfrage invalidiert werden, sonst bleibt der Eintrag sichtbar.

## Offene Fragen

- keine

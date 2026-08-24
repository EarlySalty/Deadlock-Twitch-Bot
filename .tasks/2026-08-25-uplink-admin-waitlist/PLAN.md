# Plan: Uplink-Warteliste im Admin-Modus

status: erledigt
datum: 2026-08-25
klasse: mittel
research: .tasks/2026-08-25-uplink-admin-waitlist/RESEARCH.md

## Ziel

Fertig ist die Änderung, wenn die Admin-Box wartende Streamer lädt, sicher freischaltet und für normale Nutzer weder sichtbar noch aufrufbar ist.

## Nicht-Ziele

- Ablehnen, Löschen, Limits oder Relay-Schema ändern.

## Milestones

### M1: Serververtrag und Admin-Gate
Änderungen: `rust/crates/tb-dashboard-api/src/handlers/uplink.rs`, `rust/crates/tb-dashboard-api/src/lib.rs`
Erwarteter Zwischenzustand: Nur `DashboardAuthLevel::Admin` kann Warteliste lesen oder freischalten; die Browserantwort enthält keinen Ingest-Schlüssel.
Validierung: `cargo test -p tb-dashboard-api uplink --all-features -- --include-ignored`
Stop-Regel: Bei fehlender Test-Datenbank nur die nicht datenbankgebundenen Gate-Tests ausführen und den fehlenden DB-Pfad belegen.

### M2: Admin-Box im rechten Uplink-Bereich
Änderungen: `bot/dashboard_v2/src/api/uplink.ts`, `bot/dashboard_v2/src/pages/Uplink.tsx`, `bot/dashboard_v2/src/preview/fixtures.ts`
Erwarteter Zwischenzustand: Admin sieht Liste, Leerzustand und Freischaltaktion; Nutzer sehen keine Box.
Validierung: `npm test`
Stop-Regel: Bei rotem Struktur- oder API-Test nicht zum Build wechseln.

### M3: Abschluss und Live-Betrieb
Änderungen: keine weiteren Produktdateien
Erwarteter Zwischenzustand: Lint und Build sind grün, Frontend und Dashboard-Dienst laufen mit dem neuen Stand.
Validierung: `npm run lint && npm run build`
Stop-Regel: Kein Merge oder Deploy bei Fehlern oder Review-Befunden.

## Verlauf

- 2026-08-25: Contract, Research und Plan angelegt.
- 2026-08-25: Bestehende Relay-Adminpfade als sichere Quelle bestätigt; keine Relay-Änderung nötig.
- 2026-08-25: Admin-Gate, reduzierte Proxyantwort, Wartelistenkarte und private Warnbox implementiert.
- 2026-08-25: 147 Frontendtests, 35 gezielte Rust-Tests, Lint, Build und Paket-Check grün.
- 2026-08-25: Review-Blocker durch Actor-/Ziel-Auditlog geschlossen; Fix-Nachprüfung ohne Befund.

# Evidence: Partneraufnahme im Twitch-Admin-Dashboard

status: geprüft
datum: 2026-08-25
contract: CONTRACT.md

## Fachliche Quelle

- `rust/migrations/20260806060000_partner_signup_denylist.sql:1-34` definiert
  den eigenständigen Partner-Signup-Block, den stabilen ID-Anker und den
  case-insensitiven Login-Index.
- `rust/crates/tb-analytics/src/partner_signup_block.rs:1-28` dokumentiert die
  Richtungsregel und die Trennung von Raid-Blacklist, Opt-out und technischer
  Pause.
- `rust/crates/tb-analytics/src/partner_signup_block.rs:66-109` löst IDs nur
  aus vertrauenswürdigen Bestandsquellen auf und verweigert einen unbekannten
  Login ohne stabile ID.
- `rust/crates/tb-analytics/src/partner_signup_block.rs:111-265` führt Add und
  alle Folgewirkungen in einer Transaktion aus.
- `rust/crates/tb-analytics/src/partner_signup_block.rs:275-378` schützt beim
  Remove fremde Raid-Gründe und unabhängig gesetzte Admin-Pausen.
- `rust/crates/tb-analytics/src/partner_signup_block.rs:404-415` liefert die
  Einträge bereits jüngste-zuerst.

## Vorhandener Serververtrag

- `rust/crates/tb-internal-api/src/handlers/partner_signup_block.rs:1-18`
  beschreibt den existierenden Add-, Remove-, Check- und List-Vertrag.
- `rust/crates/tb-internal-api/src/handlers/partner_signup_block.rs:105-250`
  zeigt Normalisierung, stabile-ID-Auflösung und die fachlichen Outcome-Felder.
- `rust/crates/tb-dashboard-api/src/lib.rs:790-855` bündelt die Admin-
  Streamer-Routen und legt Admin-, CSRF- und Session-Gates als Router-Layer an.
- `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:499-620`
  belegt das bestehende Muster `require_admin` plus gemeinsame Analytics-CRUD-
  Funktion für mutierende Admin-Aktionen.

## Vorhandener Frontend-Anschluss

- `bot/admin_dashboard/src/App.tsx:48-71` enthält die echten React-Router-
  Routen unter `/twitch/admin` und die Community-Gruppe.
- `bot/admin_dashboard/src/components/layout/Sidebar.tsx:67-77` ist die
  Community-Navigation, in die der neue Eintrag gehört.
- `bot/admin_dashboard/src/api/client.ts:51-59` definiert den Admin-API-
  Prefix; `:891-903` sendet JSON-Mutationen mit CSRF.
- `bot/admin_dashboard/src/hooks/useAdmin.ts:56-64` enthält die bestehende
  Invalidation-Kette für Streamer-Mutationen.
- `bot/admin_dashboard/src/pages/streamers/StreamerList.tsx:8-14,618-664`
  belegt die wiederverwendbaren Bestätigungs-, Toast- und Mutation-Muster.

## Verifikation

- Backend gezielt: `cargo test -p tb-analytics partner_signup_block` und die
  neuen `tb-dashboard-api`-Handler-/Auth-Tests.
- Frontend: `npm test` und `npm run build` in `bot/admin_dashboard`.
- Live: ohne Session 401/403, mit Admin-Session GET sichtbar, Add/Remove mit
  CSRF und anschließendem DB-/Bot-Check; keine Secrets ausgeben.

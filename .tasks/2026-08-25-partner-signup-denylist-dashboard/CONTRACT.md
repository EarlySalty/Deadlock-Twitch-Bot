# Contract: Partneraufnahme im Twitch-Admin-Dashboard

status: geplant
datum: 2026-08-25
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract beschreibt die gewünschte Admin-Funktion. Die bestehende
Signup-Block-Logik bleibt die fachliche Quelle; das Dashboard bekommt nur eine
sichere Bedienoberfläche dafür.

## Ziel

Ein Twitch-Admin kann Kanäle in eine eigene Liste „Von der Partneraufnahme
ausgeschlossen“ aufnehmen, die Einträge nachvollziehen und einen Ausschluss
wieder aufheben. Das ist ausdrücklich die Partnerschafts-Signup-Liste und nicht
die Audio-Archiv-Ausschlussliste.

## Anforderungen

- REQ-01: Unter `Community` gibt es die Seite `Partneraufnahme` mit Route
  `/community/partner-signup-blocks`.
- REQ-02: Die Seite listet Login, stabile Twitch-ID, internen Grund, optionalen
  Absagetext, Bearbeiter und Eintragszeitpunkt.
- REQ-03: Ein Admin kann einen Login mit internem Grund und optionalem
  Absagetext hinzufügen. Der Server normalisiert den Login und löst die
  kanonische Twitch-ID auf; der Browser darf keine ID raten oder selbst setzen.
- REQ-04: Vor dem Hinzufügen zeigt die Oberfläche die Nebenwirkungen klar an:
  Raid-Ziel wird gesperrt, gespeicherte Raid-OAuth-Credentials werden entfernt
  und ein aktiver Partner wird stillgelegt. Die Aktion braucht eine explizite
  Bestätigung mit eingegebenem Login.
- REQ-05: Ein Admin kann einen Eintrag mit einer zweiten Bestätigung entfernen.
  Die Oberfläche sagt dabei ausdrücklich, dass gelöschte Credentials nicht
  automatisch wiederhergestellt werden.
- REQ-06: Lade-, Leer-, Erfolgs- und Fehlerzustände sind sichtbar und die Liste
  wird nach Add/Remove automatisch aktualisiert.
- REQ-07: Die Admin-API liefert nur über den bestehenden Session-/CSRF-Pfad;
  `DashboardAuthLevel::Admin` ist serverseitig zwingend. Der interne
  `X-Internal-Token` wird niemals an den Browser gegeben.

## Invarianten

- INV-01: `twitch_partner_signup_denylist` bleibt der einzige fachliche
  Ausschlusszustand. `twitch_raid_blacklist`,
  `manual_partner_opt_out` und `technical_pause_reason` werden nicht als
  Ersatz für diese Liste verwendet.
- INV-02: Die stabile `twitch_user_id` ist der Anker; der Login ist Anzeige und
  Lookup-Fallback. Ein unbekannter Login wird nicht login-only gespeichert.
- INV-03: Die transaktionalen Folgewirkungen und Herkunftsregeln aus
  `tb_analytics::partner_signup_block::{add,remove}` bleiben unverändert.
- INV-04: Der interne Grund wird nie an den Streamer ausgeliefert. Ein
  `public_message` bleibt davon getrennt.
- INV-05: Der bestehende Audio-Archiv-Ausschluss für `niuque` bleibt separat und
  wird durch dieses Feature weder gelesen noch verändert.
- INV-06: Bestehende Admin-, CSRF-, Audit- und API-Tests werden nicht gelöscht
  oder abgeschwächt.

## Nicht-Ziele

- Keine neue Datenbankmigration; die Tabelle und das CRUD existieren bereits.
- Keine Änderung am öffentlichen Partner-Antragsformular oder an der
  Absagekommunikation außerhalb der vorhandenen `public_message`-Semantik.
- Keine Wiederherstellung von Twitch-OAuth nach dem Entfernen eines Blocks.
- Keine Vermischung mit globalen Bans, Streamer-Archivierung oder der
  Audio-Audit-Ausschlussliste.

## Erlaubter Änderungsbereich

- `rust/crates/tb-dashboard-api/src/handlers/admin_partner_signup_block.rs`
- `rust/crates/tb-dashboard-api/src/handlers/mod.rs`
- `rust/crates/tb-dashboard-api/src/lib.rs`
- `bot/admin_dashboard/src/api/client.ts`
- `bot/admin_dashboard/src/api/types.ts`
- `bot/admin_dashboard/src/hooks/useAdmin.ts`
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx`
- `bot/admin_dashboard/src/App.tsx`
- `bot/admin_dashboard/src/components/layout/Sidebar.tsx`
- passende Rust-/Frontend-Tests
- `.tasks/2026-08-25-partner-signup-denylist-dashboard/`

## Verbotene Änderungen

- `rust/migrations/20260806060000_partner_signup_denylist.sql`
- `rust/crates/tb-analytics/src/partner_signup_block.rs` ohne nachgewiesenen
  fachlichen Fehler
- Secrets, Tokens oder Twitch-Credentials im Frontend
- Änderung der Audio-Archiv-Service-Unit als Teil dieses Features

## Offene Produktfragen

- keine; die Benennung „Partneraufnahme“ und die getrennte Liste sind für die
  erste Version festgelegt.

## Amendments

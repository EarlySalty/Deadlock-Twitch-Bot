# Plan: Partneraufnahme im Twitch-Admin-Dashboard

status: umgesetzt
datum: 2026-08-25
klasse: mittel
research: .tasks/2026-08-25-partner-signup-denylist-dashboard/RESEARCH.md
contract: .tasks/2026-08-25-partner-signup-denylist-dashboard/CONTRACT.md

## Ziel

Fertig ist die Änderung, wenn ein Admin die bestehende
`twitch_partner_signup_denylist` im Twitch-Admin-Dashboard sicher verwalten
kann: Kanal hinzufügen, Folgen nachvollziehen, Ausschluss aufheben. Die
Audio-Archiv-Ausschlussliste bleibt davon getrennt.

## Milestones

### M1: Geschützter Dashboard-API-Vertrag

Änderungen:

- `rust/crates/tb-dashboard-api/src/handlers/admin_partner_signup_block.rs`
  mit GET-Liste, POST-Add und POST-Remove.
- `rust/crates/tb-dashboard-api/src/handlers/mod.rs` und
  `rust/crates/tb-dashboard-api/src/lib.rs` für Modul und Admin-Router.
- Gemeinsame `tb_analytics::partner_signup_block`-Funktionen wiederverwenden;
  keine eigene SQL-Kopie und keine Browser-Verbindung zur Internal API.
- Login normalisieren, lokale ID-Quellen prüfen und bei unbekannten Kanälen
  serverseitig über den bestehenden Helix-`get_users`-Client auflösen.
- Actor aus `DashboardAuthLevel::Admin` ableiten; `added_by` aus dem Browser
  ignorieren. CSRF und Admin-Gate unverändert über den bestehenden Router-Layer
  erzwingen.

Erwarteter Zwischenzustand: Eine Admin-Session kann die Liste lesen und Add/
Remove ausführen; Nicht-Admins erhalten keine Daten und keine Mutation. Ein
unauflösbarer Login hinterlässt keinen Eintrag.

Validierung: Neue Handler-Tests für Auth, ID-Auflösung, Add-/Remove-Responses
und Outcome-Fehler; danach `cargo test -p tb-dashboard-api` sowie
`cargo test -p tb-analytics partner_signup_block`.

Stop-Regel: Bei fehlender Test-Datenbank keine Tests überspringen oder auf
Mock-Erfolg umdeuten; stattdessen die nicht datenbankgebundenen Gate-Tests
ausführen und den fehlenden DB-Pfad dokumentieren.

### M2: Frontend-API, Typen und Query-Invalidierung

Änderungen:

- `bot/admin_dashboard/src/api/types.ts`: Entry-, List-, Add- und Outcome-
  Typen in camelCase-Frontendform.
- `bot/admin_dashboard/src/api/client.ts`: Fetch-/Add-/Remove-Funktionen über
  den Admin-Prefix, inklusive der vorhandenen CSRF-Hilfe und robuster
  snake_case/camelCase-Parser.
- `bot/admin_dashboard/src/hooks/useAdmin.ts`: Query plus Mutations und
  Invalidierung von Signup-Block-, Streamer- und Detaildaten.

Erwarteter Zwischenzustand: Die React-Schicht kennt keinen internen Token und
keine interne API-URL; alle Mutationen laufen wie die vorhandenen Admin-
Aktionen über Cookie und CSRF.

Validierung: TypeScript-Build und isolierte Client-/Parser-Tests für Add,
Remove, Leerantwort und API-Fehler.

Stop-Regel: Bei einem Parser- oder Typfehler keine UI-Politur beginnen.

### M3: Eigene Community-Seite „Partneraufnahme"

Änderungen:

- Neue Seite `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx`.
- Route in `bot/admin_dashboard/src/App.tsx` und Navigation in
  `bot/admin_dashboard/src/components/layout/Sidebar.tsx`.
- Bestehende `DataTable`, `ConfirmTypedDialog`, `Toast`, Empty-/Loading-State
  und Design-Tokens verwenden.

Erwarteter Zwischenzustand:

- Oben: Formular für Login, internen Grund und optionalen Absagetext.
- Vor Add: deutlicher Hinweis auf Raid-Blacklist, Credential-Löschung und
  mögliche aktive Partner-Pause; Bestätigung durch erneute Login-Eingabe.
- Unten: Tabelle „Von Partneraufnahme ausgeschlossen“ mit Login, ID, Grund,
  Bearbeiter und Zeitpunkt.
- Remove: Bestätigung mit Hinweis „Credentials werden nicht automatisch
  wiederhergestellt“; nach Erfolg verschwindet der Eintrag.
- Lade-, leerer, Fehler- und Erfolgzustand sind jeweils eigenständig lesbar.

Validierung: React-Router-Smoke-Test, UI-Komponententest für Confirm/Remove,
`npm test` und `npm run build` in `bot/admin_dashboard`.

Stop-Regel: Kein Live-Deploy bei fehlender Bestätigung der Nebenwirkungen oder
wenn die Seite den Block mit Audio-Archiv-/Streamer-Block verwechselt.

### M4: Ende-zu-Ende-Prüfung und Live-Rollout

Änderungen: keine neue Migration; nach erfolgreicher Implementierung Build,
Merge und Deployment des Dashboard-Dienstes.

Erwarteter Zwischenzustand: Ein Testkanal lässt sich über die UI hinzufügen,
die DB zeigt den Signup-Block plus die erwarteten Folgewirkungen, und Remove
räumt nur die vom Block stammenden Folgewirkungen auf. Der bestehende Eintrag
für `niuque` bleibt vorhanden.

Validierung:

- `cargo fmt --check` und gezielte Rust-Tests.
- `npm test` und `npm run build`.
- Live-Check ohne Session und mit Admin-Session für GET/Add/Remove; dabei keine
  Secrets ausgeben.
- Nach Merge: Dashboard neu bauen, `systemctl --user restart
  deadlock-twitch-dashboard-rust.service`, Status/Journal und eine echte
  Browser-Anfrage prüfen.

Stop-Regel: Kein direkter SQL-Eingriff als Ersatz für den API-Test und kein
Restart des Twitch-Bots wegen einer reinen Dashboard-Änderung.

## Reihenfolge

M1 → M2 → M3 → M4. Erst wenn M1 den stabilen API-Vertrag und die
Nebenwirkungsantworten belegt, wird die Oberfläche gebaut.

## Verlauf

- 2026-08-25: Bestehende Tabelle, CRUD-Logik, Internal-API-Vertrag und
  Admin-SPA-Anschluss untersucht; Plan als eigener Review-Branch angelegt.
- 2026-08-25: M1 bis M3 umgesetzt. Handler
  `admin_partner_signup_block.rs` mit GET/POST/POST-remove am Admin-Router,
  Frontend-Typen, Client-Funktionen, Hooks und die Seite
  `/community/partner-signup-blocks` samt Sidebar-Eintrag "Partneraufnahme".
  Rust-Tests 6/6 gruen gegen eine echte Postgres-Test-DB
  (`TB_TEST_DATABASE_URL`), Frontend `npm run build` und `npm test` gruen.

## Abweichungen vom Plan

- CSRF ist geprueft, ohne eigenes Zutun: die drei Routen haengen in
  `build_admin_config_router` (`lib.rs`), der `require_admin_before_csrf` und
  `csrf_protect` als Layer traegt, und `auth_status.rs` liefert dazu ein
  `csrfToken`. Die frueher hier notierte Annahme "das Rust-Dashboard prueft
  nirgends CSRF" war falsch und ist am 2026-08-25 zurueckgenommen worden;
  der `require_admin`-Aufruf in jedem Handler bleibt als zweite, vom Router
  unabhaengige Sperre.
- Antwortfelder bleiben snake_case wie bei `GlobalBanEntry`, statt auf
  camelCase umzustellen. Damit entfaellt auch der geplante Parser und die
  dazugehoerigen Frontend-Parser-Tests: die Client-Funktionen reichen die
  Antwort typisiert durch, es gibt nichts zu parsen.
- `cargo fmt -p tb-dashboard-api` formatiert 380 fremde Dateien um; deshalb nur
  die neue Datei mit `rustfmt` formatiert, der Rest bleibt unberuehrt.

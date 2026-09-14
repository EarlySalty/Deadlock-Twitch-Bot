# Auftrag: partner-signup-tag-blocks

status: aktiv (2026-09-14)

## Ziel

Im Twitch-Admin unter Partneraufnahme eine Liste von Twitch-Stream-Tags pflegen.
Wer so einen Tag am Stream hat, wird vom Bot automatisch von der Partneraufnahme
ausgeschlossen, mit denselben Folgen wie der bestehende manuelle Kanal-Ausschluss.

## Arbeitsschritte

1. Tabelle `twitch_partner_signup_tag_blocks` anlegen (Migration + GRANT an
   `twitchbot` und `twitchdash` analog `20260906140000_twitch_zuschauer_register.sql`).
2. Fachliche CRUD-Logik in `tb-analytics` (neues Modul neben
   `partner_signup_block.rs`). Durchsetzung schreibt über das bestehende
   `partner_signup_block::add`, kein zweiter Ausschlusszustand.
3. Admin-API in `tb-dashboard-api` analog `admin_partner_signup_block.rs`,
   Routen neben den bestehenden Partneraufnahme-Routen.
4. UI-Abschnitt auf der bestehenden Seite Partneraufnahme, keine neue Route.
5. Durchsetzung sofort: Backfill beim Tag-Anlegen, Live-Poll inklusive
   Kategorie-Sample, periodischer Session-Sweep. Promote nur als letzte Sicherung.
6. sqlx-Offline-Cache und Schema-Snapshot mitziehen.

## Produktregeln (nicht nachfragen, so bauen)

- Tags sind die grauen Twitch-Stream-Tags am Stream, so wie man sie auf Twitch
  sieht. `Deutsch` und `English` nur als Beispiel in Hinweis und Platzhalter,
  niemals als voreingetragene Sperre. Die Liste startet leer, keine Seed-Zeilen
  in der Migration. Nicht die lila Spielkategorie (`Deadlock`). Keine
  Content-Labels, keine internen Research-Tags.
- In der DB liegen sie in `twitch_stream_sessions.tags` als Komma-Liste
  (Prod-Stichprobe: `Deutsch`, `English,Deutsch`, `SoloQ,English,German,...`).
  Parser wie `tag_analysis.rs` `parse_tags`: JSON-Array oder Komma-Liste.
  Vergleich nach dem Parsen, exakter Tag, case-insensitive, getrimmt. Kein
  Substring (`fun` trifft nicht `funny`). Ein Treffer reicht (ODER).
- Speichern: `tag` in Kleinbuchstaben als Primärschlüssel, `display_tag` so wie
  der Admin ihn eingegeben hat (`Deutsch` bleibt `Deutsch` in der Anzeige).
  `German` und `Deutsch` sind zwei verschiedene Tags, beide eintragbar.
- Treffer schreibt einen Kanal-Eintrag in `twitch_partner_signup_denylist` über
  `tb_analytics::partner_signup_block::add` mit `reason = tag_block:<tag>`,
  `added_by = tag_block`, `public_message` aus der Tag-Regel oder Default.
- Steht der Kanal schon auf der Denylist, nicht überschreiben.
- Ausschluss bleibt stehen, auch wenn der Tag später vom Stream verschwindet.
  Tag aus der Tag-Liste löschen hebt bestehende Kanal-Einträge nicht auf.
- Harte Regel: aktive Partner bleiben Partner. Die Tag-Regel schreibt nur
  Nicht-Partner auf `/partner-signup-blocks` (`twitch_partner_signup_denylist`).
  Hat der Kanal in `twitch_partners` `status = 'active'`, ist das ein No-op:
  kein Listen-Eintrag, keine Pause, keine Credential-Löschung, kein
  Raid-Blacklist-Write. `enforce`, Backfill, Sweep und Promote-Tagprüfung
  überspringen aktive Partner. Der manuelle Kanal-Ausschluss auf derselben Seite
  bleibt unverändert und darf Partner weiter stilllegen.
- Zeitpunkt: der Ausschluss passiert sobald der Bot den Tag kennt, nicht erst
  beim Partner-Antrag. Signup ist zu spät (der Streamer sieht dann schon die
  Absage). Sobald der Eintrag in der Denylist steht, lässt Scout ihn in Ruhe
  (`detector.rs` filtert die Denylist schon).
- Historisch: alle Sessions in `twitch_stream_sessions` mit diesem Tag. Anker
  `twitch_user_id` wenn gesetzt, sonst Login über `resolve_user_id`. Login ohne
  auflösbare ID überspringen, nicht login-only speichern.
- Beim Anlegen eines Tags in der Admin-UI sofort Backfill über alle Sessions,
  danach ist der Kanal schon ausgeschlossen.
- Live: jeder Poll-Tick, alle Live-Snapshots inklusive Kategorie-Sample
  (Deadlock-Directory, nicht nur bestehende Partner). Treffer sofort `enforce`.
- Zusätzlich periodischer Sweep (alle paar Minuten im Bot) über Sessions mit
  nicht-leeren Tags gegen die aktuelle Tag-Liste, damit niemand wartet bis er
  wieder live ist oder sich anmeldet.
- Promote bleibt nur die letzte Sicherung (fail-closed): historische Tags
  prüfen, dann den bestehenden Signup-Block-Guard. Das ist kein Auslöser.

## Fundstellen (aus dem Vorcheck)

- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:55`: bestehende
  Seite Partneraufnahme. Hier den Tag-Abschnitt einbauen, keine neue Route.
- `bot/admin_dashboard/src/api/client.ts:56` und `:848`: Admin-Base
  `/twitch/api/admin`, Kanal-CRUD `/partner-signup-blocks`.
- `bot/admin_dashboard/src/hooks/useAdmin.ts:182`: React-Query-Hooks, Muster für
  die Tag-Hooks.
- `bot/admin_dashboard/src/api/types.ts:48`: `PartnerSignupBlockEntry`.
- `rust/crates/tb-dashboard-api/src/handlers/admin_partner_signup_block.rs:1`:
  Dashboard-Hülle, Auth/CSRF, `added_by` aus Session.
- `rust/crates/tb-dashboard-api/src/lib.rs:1057`: Routen
  `GET/POST /twitch/api/admin/partner-signup-blocks`.
- `rust/crates/tb-analytics/src/partner_signup_block.rs:1`: fachliches `add` /
  `remove` / `check` inkl. Raid-Blacklist, Credential-Löschung, Partner-Pause.
- `rust/crates/tb-domain/src/signup_block.rs:26`: `SignupBlock`, Absagetext.
- `rust/crates/tb-raid/src/signup_denylist.rs:35`: fail-closed Lookup im Promote.
- `rust/crates/tb-raid/src/partner_setup.rs:657`: Guard in
  `promote_streamer_to_partner` vor jedem Schreibzugriff.
- `rust/crates/tb-transport-twitch/src/streams.rs:33`: `HelixStream.tags`.
- `rust/crates/tb-monitoring/src/stream.rs:22`: `StreamSnapshot.tags`,
  `tags_json` / `tags_joined`.
- `rust/crates/tb-monitoring/src/poller/engine.rs:959`: Poll schreibt Tags in
  Stats/Sessions.
- `rust/crates/tb-monitoring/src/poller/hooks.rs:149`: `PollHooks`, Default
  no-op; hier `on_live_snapshots` ergänzen.
- `rust/bin/tb-bot/src/main.rs:427`: `impl PollHooks for SubscriptionPollHooks`.
- `rust/bin/tb-bot/src/wiring.rs:134`: Helix → Snapshot inkl. Tags.
- `rust/crates/tb-analytics/src/tag_analysis.rs:1`: wertet Session-Tags aus,
  sperrt nicht. Nicht umbiegen.
- `rust/migrations/20260806060000_partner_signup_denylist.sql:14`: bestehende
  Kanal-Denylist. Nicht ändern.
- `rust/migrations/20260906140000_twitch_zuschauer_register.sql:18`: GRANT-Muster.

## Was nicht angefasst wird

- Die bestehende Kanal-Denylist-Semantik (`add`/`remove`/UI-Formular für Logins).
- Audio-Archiv-Ausschlussliste, globale Bans, Raid-Blacklist außer über das
  bestehende `signup_block:`-Präfix in `add`.
- `tag_analysis.rs` und Streamer-Performance.
- Öffentliches Partner-Antragsformular außerhalb des vorhandenen Absagetexts.
- Neue Standalone-Admin-Route, neue `*_ENABLED`-Flags, SQLite.
- Fremde Repos.

## Schema

```sql
CREATE TABLE IF NOT EXISTS public.twitch_partner_signup_tag_blocks (
    tag             text PRIMARY KEY,
    display_tag     text NOT NULL,
    reason          text NOT NULL,
    public_message  text,
    added_by        text NOT NULL,
    added_at        timestamptz NOT NULL DEFAULT now()
);
```

`tag` immer `lower(trim(display_tag))`. Leerer Tag ist 400.

GRANT analog Zuschauer-Register: twitchbot und twitchdash SELECT/INSERT/UPDATE/DELETE.
Keine INSERT-Seeds. Tabelle bleibt nach der Migration leer.

Migration-Name: `20260914120000_partner_signup_tag_blocks.sql`.

## API

Unter dem bestehenden Admin-Router (Session + CSRF + `require_admin`):

- `GET  /twitch/api/admin/partner-signup-tag-blocks` → `{ items: [...] }`
- `POST /twitch/api/admin/partner-signup-tag-blocks` Body
  `{ tag, reason?, public_message? }` → `{ ok, tag, display_tag, inserted }`
- `POST /twitch/api/admin/partner-signup-tag-blocks/remove` Body `{ tag }`
  → `{ ok, tag, removed }`

Default-Grund wenn leer: `tag_block`. Bearbeiter aus der Admin-Session, nie aus
dem Browser.

Frontend-Client analog `fetchPartnerSignupBlocks` mit Suffix
`/partner-signup-tag-blocks`.

## UI-Texte (wortgleich)

Auf `PartnerSignupBlocks.tsx`, neuer `Section` zwischen
„Kanal ausschließen“ und der Kanalliste:

- Titel: `Tags automatisch ausschließen`
- Hinweis: `Die grauen Tags am Twitch-Stream, zum Beispiel Deutsch oder English. Wer so einen Tag live oder in einer gespeicherten Session hat, wird automatisch von der Partneraufnahme ausgeschlossen. Bestehende Partner bleiben unangetastet.`
- Felder: `Tag` (Platzhalter `z. B. Deutsch`), `Interner Grund`, `Absagetext (optional)`
- Knopf: `Tag sperren`
- Warnbox: `Das passiert bei einem Treffer` mit:
  - `Der Kanal kommt auf die Sperrliste der Partneraufnahme.`
  - `Scout spricht ihn nicht mehr an.`
  - `Der Kanal wird als Raid-Ziel gesperrt.`
  - `Bestehende Partner sind davon nicht betroffen.`
- Liste-Titel: `Gesperrte Tags`
- Leer: `Kein Tag ist gesperrt.`
- Aufheben-Knopf: `Aufheben`
- Bestätigung analog ConfirmTypedDialog, expected = Tag.

Look: dieselben `admin-input` / `admin-button` / `Section` / `DataTable` Klassen.
Keine neue Seite, kein Sidebar-Eintrag.

## Durchsetzung

Neues Modul `tb_analytics::partner_signup_tag_block`:

- `list` / `add` / `remove` für die Tag-Tabelle.
- `matching_tag(tags, blocked) -> Option<String>` nach `parse_tags`.
- `enforce(pool, twitch_user_id, twitch_login, tags) -> Result<Option<AddOutcome>, sqlx::Error>`
  lädt die Tag-Liste, matcht. Aktive Partner (`twitch_partners.status = 'active'`
  zu User-ID oder Login) sofort `Ok(None)` ohne Write. Sonst `partner_signup_block::add`
  nur wenn `check` keinen Eintrag liefert.
- `backfill(pool, tag) -> Result<u64, sqlx::Error>`: Distinct-Kanäle aus
  `twitch_stream_sessions` deren geparste Tags den Tag enthalten. Pro Kanal
  `enforce`. Rückgabe: Zahl neu geschriebener Denylist-Einträge. `add` der
  Admin-API ruft das nach dem Insert auf.

Reihenfolge der Durchsetzung, Signup zuletzt:

1. Admin legt Tag an → `backfill` schreibt sofort alle historischen Treffer
   in die Denylist.
2. Live-Poll: `PollHooks::on_live_snapshots(&[StreamSnapshot])` mit Default
   no-op. Engine ruft das einmal pro Tick mit der Vereinigung aus getrackten
   Live-Streams und Kategorie-Sample auf. `SubscriptionPollHooks` ruft
   `enforce` auf. Fehler loggen, Tick nicht abbrechen.
3. Periodischer Sweep im Bot (Kadenz 5 Minuten, analog andere Sweeper in
   tb-bot): `backfill` je aktivem Tag oder ein `enforce_all_sessions`. Nicht
   den Poll-Tick blockieren.
4. Promote (`partner_setup.rs` vor dem bestehenden Lookup) nur als Netz für
   Nicht-Partner: historische Session-Tags laden, `enforce` (aktive Partner
   überspringt enforce selbst), dann den bestehenden Guard. Fail-closed. Ein
   aktiver Partner darf hier nicht neu auf die Tag-Denylist.

Cache der Tag-Liste im Prozess mit kurzer TTL (60 s) ist erlaubt, nach Admin-Write
nicht zwingend invalidieren.

## Fertig-Kriterium

- Admin kann auf `/twitch/admin/community/partner-signup-blocks` Tags anlegen
  und löschen.
- Ein Live-Stream mit gesperrtem Tag (auch nur im Deadlock-Directory, kein
  Partner) steht danach in `twitch_partner_signup_denylist` mit `reason`
  `tag_block:<tag>`, ohne dass der Streamer sich anmelden muss.
- Beim Anlegen eines Tags werden Kanäle mit diesem Tag in
  `twitch_stream_sessions` sofort mit ausgeschlossen (historischer Backfill).
- Partner-Promote bleibt Absicherung für Nicht-Partner: Treffer wird abgelehnt
  (`signup_blocked`), ohne Partner-Zeile. Das darf nicht der erste Ort sein, an
  dem der Block entsteht. Aktive Partner werden durch Tags nicht stillgelegt.
- Bestehende Kanal-Ausschlüsse und ihre UI bleiben unverändert.
- `cargo test -p tb-analytics --lib partner_signup_tag_block` und die
  angefassten Dashboard-/Raid-Tests laufen. Gebrochene bestehende Tests nachziehen.
- sqlx-Offline (`rust/.sqlx`) und `rust/crates/tb-db/tests/fresh_schema_snapshot.txt`
  sind im Diff, wenn Queries oder Schema neu sind.

## Deploy-Weg

Nicht deployen. Branch pushen, Fertigmeldung, dann stoppen. Merge und Live macht
der Orchestrator.

## Rahmen

- Worktree: `/home/nathanael/.worktrees/tb-partner-signup-tag-blocks`
- Branch: `feat/partner-signup-tag-blocks`
- Basis: `origin/main` (`18fa335581fb25067b2615c0e88782a4248c02b9`)
- Repo: nur Deadlock-Twitch-Bot
- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst.
- Nur den eigenen Branch pushen, nie main.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Gedankenstriche
  der Form Em-Dash.
- Auftrag größer als beschrieben: Bump-up, dann stoppen.

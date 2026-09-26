status: aktiv
Datum: 2026-09-24

# Auftrag: Werbemanager-Cluster CI grün machen

## Ziel in Nutzerworten

Die offenen PRs des Werbemanager-Clusters (Twitch-Bot #958, Steam-Bot #69)
sollen eine grüne CI bekommen, damit Merge und Deploy folgen können.

## Befund (Vorcheck, 2026-09-24)

- Twitch-Bot #958, Job „Uplink Auth, Broker und Dashboard-Anschluss“:
  6 von 14 `raid_oauth_impl::callback_tests` bekommen HTTP 500 statt 200.
  Lokal reproduziert (Worktree `tb-admanager-steam-api-20260924`, d84535a0).
  Ursache: `store_new_auth` liest `twitch_partners.raid_admin_enabled`
  (Prod-Migration 20260913153000_admin_raid_wunsch.sql), das Test-Fixture
  `make_pool` in `bin/tb-bot/src/raid_oauth_impl.rs` legt die Spalte nicht an.
  Fail-closed → generische 500-Antwort.
- Twitch-Bot #958, Job schema-gate: Der Online-SQLx-Check kompiliert auch die
  Makros der gepinnten dbrain-Crates (rev d8c34270); deren Tabellen liegen im
  Schema `brain`, das `rust/migrations` nicht anlegt (36 Fehler, nur
  brain-Relationen: hero_catalog, item_catalog, hero_item_synergies,
  hero_item_stats, hero_ability_orders, entity_aliases).
- Steam-Bot #69 und Deadlock-Bots #451: Jobs wurden gar nicht gestartet —
  GitHub-Actions-Abrechnung/Ausgabenlimit (Run 35946064711, Annotation).
  Kein Codeproblem; nur der Account-Inhaber kann das freigeben.

## Arbeitsschritte

1. `raid_admin_enabled BOOLEAN NOT NULL DEFAULT TRUE` im `twitch_partners`-DDL
   des callback_tests-Fixtures ergänzen (Prod-Typ BOOLEAN, 20260913153000).
2. Brain-Schema-Fixture `rust/schema-gate/brain-schema.sql` aus der zentralen
   PG erzeugen (pg_dump --schema-only, Views entfernt) und im Workflow
   `rust-sqlx-check.yml` nach den Twitch-Migrationen anwenden.
3. Lokale Abnahme: callback_tests 14/14, tb-raid-, tb-transport-,
   tb-internal-api-Suiten des Jobs, danach Online-SQLx-Workspace-Check gegen
   Wegwerf-DB mit Migrationen + Fixture.
4. Push auf den PR-Branch, CI-Grün abwarten.

## Nicht angefasst wird

- raid_oauth_impl.rs Produktionscode (Fehler liegt am Test-Fixture),
- Cargo.toml/Cargo.lock und der dbrain-Pin,
- Abrechnungseinstellungen auf GitHub (Nutzer-Sache),
- die übrigen offenen PRs.

## Fertig-Kriterium

Beide Jobs von #958 grün auf GitHub. #69 folgt, sobald das Billing-Problem
gelöst ist (erneuter Run-Anstoß, kein Codeänderungsbedarf bekannt).

## Deploy-Weg

Nach unabhängigem Review und Merge laut Akte rolle-merge-schleuse: Deploy des
Twitch-Releases über den bestehenden frozen-Checkout-Weg
(deploy-werbemanager-Muster), Live-Beweis nach rolle-deploy-verifizierer.
Der PR-first-Testbetrieb (orchestrierung/PR-FIRST-TESTBETRIEB.md) bleibt
maßgeblich: kein Merge/Deploy ohne unabhängiges Review.

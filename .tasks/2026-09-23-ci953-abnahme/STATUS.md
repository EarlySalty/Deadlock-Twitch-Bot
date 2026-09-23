# PR #953 – deterministische CI-Abnahme, 23. September 2026

PR: https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/953
Branch: `ci/deterministic-pr-gate-20260922`
Worktree: `/home/nathanael/.worktrees/tb-deterministic-pr-gate-20260922`

Nur PR-Testbetrieb. Kein Merge, kein Main-Push, kein Deploy, kein Dienst-Neustart und keine Änderung an Rulesets oder Copilot-Pflicht. Fremde Worktrees bleiben unverändert.

## Verifizierter Ausgangspunkt

Sauberer Checkout `c2043a0bc03d434b9311311c14db2761ceecec0f`; origin zeigt auf dieses Repository. `origin/main` wurde in den bestehenden Branch integriert. Einziger manueller Konflikt: `rust-sqlx-check.yml`; sowohl die main-exklusive Cache-Schreibregel als auch der neue Werbemanager-DB-Regressionstest bleiben erhalten.

Integrationscommit: `55d4ec4ebb6688f5397e12fe4e0fd4a2c870b6e0`, gepusht. 18 Gate-Unit-Tests und `git diff --cached --check` bestanden. GitHub-Baseline nach Integration weiterhin rot: https://github.com/EarlySalty/Deadlock-Twitch-Bot/actions/runs/35893931814. Das ist keine positive Gesamtabnahme.

## OAuth und Denylist

Alle sechs Callback-Fehler wurden lokal reproduziert: 8 von 14 Tests bestanden, sechs erwarteten HTTP 200 und bekamen 500. Test-only Tracing zeigte PostgreSQL 42703: `raid_admin_enabled` fehlt im handgebauten Callback-Fixture. Der produktive AuthWriter berücksichtigt diese Spalte beim Persistieren der Berechtigung.

Das Fixture wendet jetzt die unveränderte echte Migration `rust/migrations/20260913153000_admin_raid_wunsch.sql` an. Keine HTTP-Erwartung wurde geändert. Erneuter Lauf `cargo test --locked -j2 -p tb-bot raid_oauth_impl::callback_tests -- --nocapture`: **14 bestanden, 0 fehlgeschlagen, 0 ignoriert**. Enthalten sind auch fehlgeschlagener Denylist-Lookup ohne Credential-Speicherung, Identitäts-/Scope-Ablehnung und der veraltete Uplink-Callback nach Disconnect.

`lookup_or_fail_closed` gibt den ursprünglichen `sqlx::Error` statt `()` zurück. Beide Aufrufer lehnen einen Lookup-Fehler weiterhin ab; Logging und Benutzerantworten bleiben erhalten. Keine Clippy-Ausnahme hinzugefügt.

## Schema-Herkunft und Cross-Repo-Vertrag

Gepinnter Brain-Commit: `d8c34270868e129098e12243f53f5b52ee507b8b`. Die Basistabellen gehören zu `Deadlock-Bots/rust/crates/dl-central-db/migrations`, nicht zu den beiden späteren Brain-Migrationen. Originalstand: `2b62eee4bfca1ea185c2aae758fb1e8d99acaf77`.

Reihenfolge: `0012_brain_knowledge_timeline.sql`, `0013_brain_insight_records.sql`, `2026070410_brain_ingestion_tables.sql`, anschließend Brain `scripts/migrations/2026-09-12-reasoner.sql` und `2026-09-16-population.sql`.

Das Ursprungsrepository ist privat. Der anonyme unveränderliche Download liefert 404; der öffentliche Brain-Stand exportiert die Basis nicht. Kein PAT für PR-Code, keine Veröffentlichung privater DDL durch diesen Auftrag, kein Ersatzschema und keine Offline-Ausnahme. Übergabe mit Pfaden, SHA-256 und benötigtem freigegebenem Export-Commit:
https://github.com/EarlySalty/Deadlock-Brain/pull/9#issuecomment-5799482619

Lokal wurden alle fünf Originalmigrationen mit Prüfsummenprüfung auf eine eigens angelegte Wegwerf-Timescale/PostgreSQL-16-Datenbank angewendet. Das private Original wurde dabei nur gelesen, nicht geändert oder in dieses Repository kopiert. Dieser lokale Nachweis ersetzt noch keinen secrets-freien GitHub-Lauf.

## Noch laufende Abnahme

Weitere Clippy-Befunde, Rustfmt-Bestand, Dashboard-Markenprüfung, SQLx-Onlineprüfung, vollständige Workspace-/DB-/Integrationstests sowie PR-Deep-Scans und kontrollierte Scanner-Gegenproben werden separat protokolliert. Keine dieser offenen Prüfungen ist durch die obigen Einzelnachweise als bestanden erklärt.

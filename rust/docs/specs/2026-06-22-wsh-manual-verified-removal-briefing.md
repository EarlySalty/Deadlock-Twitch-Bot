# WS-H — manual_verified-Removal — GPT-Worker-Briefing (ab 21:00 feuern)

**Modell:** gpt-5.5, effort xhigh. **Worktree:** am Start frisch off aktuellem main anlegen (`git worktree add ../Deadlock-Twitch-Bot-wsh -b fix/wsh-manual-verified-removal`). NICHT committen/pushen — Claude reviewt + merged.

## Kontext
Lies `rust/docs/specs/2026-06-22-partner-state-bugfix-spec.md` §2 (Direktive 1) + §3 Phase 4 (WS-H). Welle 1+2 sind LIVE: `is_partner_active = status='active' AND manual_partner_opt_out=0 AND technical_pause_reason='' AND admin_archived_at IS NULL` (View `public.twitch_partners_all_state`). `manual_verified_*` ist davon ENTKOPPELT und verursacht **keine** Fehlfunktion — reine Code-/Schema-Hygiene (User-Direktive 1: „manual_verified_* komplett aus Rust entfernen").

## Scope (159 Prod-Treffer / 7 Crates + DB)
Zeilennummern können driften → per Symbol lokalisieren, aktuellen Code prüfen.

1. **DB-Migration** (neue Datei `rust/migrations/<ts nach 20260622140000>_drop_manual_verified.sql`):
   - Erst ermitteln, wie `is_verified` in der View aktuell definiert ist (Vorlage: `migrations/20260622130000_partner_state_keystone.sql`) und welche `manual_verified_*`-Spalten existieren (`migrations/20260601000000_baseline_schema.sql` + keystone).
   - `CREATE OR REPLACE VIEW public.twitch_partners_all_state`: `is_verified`-CASE entfernen bzw. eindeutig auf `is_partner_active` mappen.
   - Die `manual_verified_*`-Spalten DROPpen — guarded/idempotent (`DROP COLUMN IF EXISTS`).
   - Angewandte Migrationen NICHT editieren (sqlx-Checksum bricht Live-Boot).
2. **Code (7 Crates):** jeden `manual_verified`-Zugriff entfernen. Treffer: streamer_lifecycle.rs (51), tb-analytics/streamers_crud.rs (34), stats_native.rs (21), tb-raid/partner_setup.rs (14), tb-bot/raid_oauth_impl.rs (9), handlers/streamers.rs (8), admin_audit_log.rs (7), tb-chat/commands.rs (6), admin_streamers.rs (5), admin_legacy_streamers.rs (2), tb-analytics/admin_streamers.rs (2). `is_verified`-Felder, die aus `manual_verified` abgeleitet waren: entfernen oder durch `is_partner_active` ersetzen.
3. **tracked.rs (tb-monitoring/poller):** `is_verified` ist dort bereits `is_partner_active`-Alias (`is_verified = row.is_partner_active != 0`). Den irreführenden Alias entwirren (Feld in `is_partner_active` umbenennen ODER klar dokumentieren) — KEINE Verhaltensänderung.
4. **Admin-Dashboard** (admin_streamers/admin_audit_log): `manual_verified`-Spalten/Felder aus Responses + Audit-Log entfernen.

## Regeln
- **TDD:** betroffene Tests/Fixtures anpassen (Fixtures, die `manual_verified_*` anlegen, mit-entfernen). Test-DB: `tb-test-postgres` (DSN `postgres://postgres:tbtest@127.0.0.1:5434/postgres`, `TB_TEST_REQUIRE_DB=1`, seriell `--test-threads=1`). `--no-fail-fast`: der Test `run_migrations_builds_full_schema_on_fresh_db` ist PRE-EXISTING rot (Migration 20260619010000/Trigger) — ignorieren, nicht „fixen".
- Migrations-Embed: nach neuer `.sql` `touch crates/tb-db/src/migrate.rs` vor `cargo build`.
- **User-sichtbare Texte TABU:** falls ein Frontend-/Audit-String nötig wäre, `"Platzhalter"` + Datei:Zeile melden — Claude formuliert.
- Report: geänderte Dateien + Zeilen je Bereich, Migration, Platzhalter-Stellen, Red→Green-Beleg.

## Verifikation (Claude, nach Worker)
Adversarial-Review · Integration · `touch migrate.rs` + `cargo build --release --bin tb-bot --bin tb-dashboard` · `strings`-Embed-Check · Restart · Live: View `is_verified`/`is_partner_active` konsistent, `\d twitch_partners` zeigt keine `manual_verified_*` mehr, 0 Journal-Errors · CHANGELOG (admin-intern → ggf. nur CHANGELOG, kein Discord) · Push.

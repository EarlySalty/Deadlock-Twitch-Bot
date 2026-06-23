# manual_verified-Spalten-Drop — FERTIG + VERIFIZIERT, aber ZURÜCKGESTELLT bis Python-Cutover

**Status (2026-06-23):** Migration gebaut, gegen Prod-Schema-Klon bewiesen invariant — **NICHT deployt**, weil die
live Python-Twitch-Services die Spalten noch lesen UND schreiben. Deploy ist durch CLAUDE.md verboten:
„Python-Code erst nach Cutover + Verifikation der Rust-Ablösung ausmustern — nie vorher."

## Warum blockiert (Live-Zustand 2026-06-23, empirisch geprüft)

Vier Twitch-Services laufen parallel (Rust **und** Python):
- `deadlock-twitch-bot-rust.service` (Rust) **und** `deadlock-twitch-bot.service` → `python -m bot.bot_service` (Uptime >3 Tage)
- `deadlock-twitch-dashboard-rust.service` (Rust) **und** `deadlock-twitch-dashboard.service` → `python -m bot.dashboard_service`

Python-Consumer der zu droppenden Spalten (würden nach Drop `42703 column does not exist` werfen):
- **Lesen aus View `twitch_streamers_partner_state`:** `bot/core/partner_utils.py:82-84` (`get_all_partners`),
  `:115-117` (`get_live_partners`) — Hot-Path, importiert von 7 Live-Modulen (engagement/pipeline, analytics/mixin,
  chat/moderation, chat/connection, chat/global_ban_sweep, chat/promos, partner_utils);
  `bot/community/leaderboard.py:526-528`, `bot/dashboard/mixin.py:382-384`, `bot/dashboard/live/live.py:586-589`.
- **Schreiben in Basistabelle `twitch_partners`:** `bot/storage/partner_registry.py:974-976/1005-1007/1047/1079/1189-1191`,
  `bot/storage/pg.py:2376-2378` (UPDATE/INSERT `manual_verified_*`).

Die Trace-Annahme „Python läuft nicht" (punkt2-manual-verified-trace.md) war **falsch**.

## Gate vor Ausführung (alle Punkte müssen erfüllt sein)

1. `deadlock-twitch-bot.service` + `deadlock-twitch-dashboard.service` (Python) gestoppt/disabled, Rust-Parität verifiziert.
2. `grep -rniE 'manual_verified' bot/` → keine Live-Lese-/Schreibpfade mehr (oder Python-Repo entfernt).
3. `pg_stat_activity` auf `twitch_analytics` zeigt keine Python-Pools mehr.

## Verifikation, die bereits erbracht ist (gegen Prod :5433 / Klon auf :5434)

- **Invarianten identisch erhalten:** PRE = POST = `aktive=53, is_partner_active=1→49, is_verified=1→52`
  (gegen 1:1-Klon des Prod-Schemas inkl. Daten).
- **Keine versteckte DB-Abhängigkeit:** keine Funktion/kein Trigger/Index/Constraint referenziert `manual_verified`.
  Der Sync-Trigger `trg_twitch_partners_sync_identity` feuert nur bei `UPDATE OF twitch_login, twitch_user_id, status`
  → das Seeding-`UPDATE … SET verified` triggert ihn nicht.
- **Rust-Seite sauber:** 0 `query!/query_as!`-Compile-Time-Makros, kein `.sqlx`-Cache, kein `SELECT *` auf die Views,
  alle `is_verified`-Reads namens-basiert (Badge bleibt erhalten, jetzt aus `verified`).
- **Verlustfrei:** 0 aktive Partner hängen allein an einem zukünftigen `manual_verified_until` (statischer Snapshot ok).

## Deploy-Runbook (erst NACH erfülltem Gate)

1. Diese SQL als `rust/migrations/<frischer-timestamp>_drop_manual_verified_columns.sql` ablegen (Timestamp > letzte Migration).
2. **Stale-Migrator-Falle:** `touch rust/crates/tb-db/src/migrate.rs` (kein build.rs-rerun → sonst wird .sql still nicht eingebettet).
3. `cargo build --release` für **beide** Binaries (tb-bot + tb-dashboard) — beide laufen `MIGRATOR.run()` beim Boot.
4. Sequenziell in ruhigem Fenster, HEAD-Recheck (Parallel-Sessions): einen Dienst neu starten (wendet Migration an),
   dann den zweiten (sieht sie in `_sqlx_migrations`, überspringt).
5. **Verifikation (Artefakt + Live):** (a) Migrations-String im Binary greppen; (b) Journal beider Dienste 0 Errors +
   „applied"; (c) `twitch_partners_all_state WHERE status='active'`: `is_partner_active=1→49`, `is_verified=1→52` unverändert,
   `manual_verified_*` weg, `verified boolean` vorhanden.
6. Danach: dormante Python-Referenzen entsorgen (erst nachdem Python nachweislich aus ist).

## Verifizierte Migration (1:1 bereit zum Einsatz)

```sql
-- Schema-Hygiene: Legacy-Spalten manual_verified_permanent/_until/_at aus twitch_partners entfernen.
-- Neue Quellspalte `verified` (boolean) ersetzt sie als is_verified-Quelle (geseedet aus bisheriger Ableitung).
-- Invarianten (Prod): aktive=53, is_partner_active=1→49, is_verified=1→52 (vorher==nachher, verifiziert).

ALTER TABLE twitch_partners
    ADD COLUMN IF NOT EXISTS verified boolean NOT NULL DEFAULT false;

UPDATE twitch_partners p
SET verified = (
    COALESCE(p.manual_verified_permanent, 0) = 1
    OR (p.manual_verified_until IS NOT NULL AND p.manual_verified_until::timestamptz >= now())
    OR p.manual_verified_at IS NOT NULL
);

DROP VIEW IF EXISTS twitch_streamers_partner_state;
DROP VIEW IF EXISTS twitch_partners_all_state;

CREATE VIEW twitch_partners_all_state AS
SELECT p.id,
    p.twitch_login,
    p.twitch_user_id,
    p.require_discord_link,
    p.next_link_check_at,
    i.discord_user_id,
    i.discord_display_name,
    COALESCE(i.is_on_discord, 0) AS is_on_discord,
    p.manual_partner_opt_out,
    p.partnered_at AS created_at,
    COALESCE(p.admin_archived_at,
        CASE
            WHEN p.status = 'archived'::text THEN p.departnered_at
            ELSE NULL::text
        END) AS archived_at,
    p.raid_bot_enabled,
    p.silent_ban,
    p.silent_raid,
    0 AS is_monitored_only,
    CASE
        WHEN p.verified THEN 1
        ELSE 0
    END AS is_verified,
    1 AS is_partner,
    CASE
        WHEN p.status = 'active'::text AND COALESCE(p.manual_partner_opt_out, 0) = 0 AND COALESCE(p.technical_pause_reason, ''::text) = ''::text AND p.admin_archived_at IS NULL THEN 1
        ELSE 0
    END AS is_partner_active,
    p.live_ping_role_id,
    COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
    p.status,
    p.departnered_at,
    p.technical_pause_reason,
    CASE
        WHEN p.status <> 'active'::text THEN 'inactive'::text
        WHEN p.admin_archived_at IS NOT NULL THEN 'inactive'::text
        WHEN COALESCE(p.technical_pause_reason, ''::text) = 'blocked'::text THEN 'blocked'::text
        WHEN COALESCE(p.manual_partner_opt_out, 0) = 1 THEN 'admin_non_partner'::text
        WHEN COALESCE(p.technical_pause_reason, ''::text) <> ''::text THEN p.technical_pause_reason
        WHEN p.inactivity_flagged_at IS NOT NULL THEN 'inactive'::text
        ELSE 'active'::text
    END AS operational_state
   FROM twitch_partners p
     LEFT JOIN twitch_streamer_identities i ON i.twitch_user_id = p.twitch_user_id;

CREATE VIEW twitch_streamers_partner_state AS
SELECT twitch_login,
    twitch_user_id,
    require_discord_link,
    next_link_check_at,
    discord_user_id,
    discord_display_name,
    is_on_discord,
    manual_partner_opt_out,
    created_at,
    archived_at,
    raid_bot_enabled,
    silent_ban,
    silent_raid,
    is_monitored_only,
    is_verified,
    is_partner,
    is_partner_active,
    live_ping_role_id,
    live_ping_enabled,
    technical_pause_reason,
    operational_state
   FROM twitch_partners_all_state
  WHERE status = 'active'::text;

ALTER TABLE twitch_partners DROP COLUMN IF EXISTS manual_verified_permanent;
ALTER TABLE twitch_partners DROP COLUMN IF EXISTS manual_verified_until;
ALTER TABLE twitch_partners DROP COLUMN IF EXISTS manual_verified_at;
```

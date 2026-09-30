status: aktiv
Datum: 2026-08-30

# Plan: Lurker-Discord-Pitch + LFG-Pitch-Kreis

Ziel und Maßstab: siehe CONTRACT.md im selben Ordner.

## M1 — Migration

- Änderungen: `rust/migrations/20260831<hh>0000_lurker_pitch.sql`:
  `ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS lurker_pitch_enabled integer DEFAULT 0 NOT NULL;`
  plus `CREATE TABLE IF NOT EXISTS twitch_lurker_pitch_log (streamer_login text NOT NULL,
  chatter_identity_key text NOT NULL, chatter_login text NOT NULL, mentioned_at timestamptz
  NOT NULL DEFAULT now(), PRIMARY KEY (streamer_login, chatter_identity_key));`
- Erwarteter Zwischenzustand: Migration läuft gegen die Wegwerf-Test-DB durch.
- Validierung: `cd rust && eval "$(./scripts/test_db.sh env)"` + Migrationslauf des Test-DB-Skripts.
- Stop-Regel: Migration rot → nicht weiterbauen, Ursache (Namenskollision, Typ) klären.

## M2 — LFG-Pitch-Kreis

- Änderungen: `lfg_pitch.rs`: Feld `own_channel_broadcaster_id: String` in
  `new`/`new_with_clock`, Silent-Stufe `ForeignChannel` in `decide()` direkt nach dem
  enabled-Check, Vergleich gegen `event.broadcaster_user_id`. `chat_wiring.rs:834/2828`
  und Test-Helfer (pipeline.rs:2485, :2722, lfg_pitch.rs:1021) mit `bot_user_id` versorgen.
  Tests: Fremdkanal → Silent `foreign_channel`; eigener Kanal → Verhalten unverändert.
- Erwarteter Zwischenzustand: `cargo test -p tb-chat` grün inkl. neuer Tests.
- Validierung: Test-DB-Umgebung + `cargo test -p tb-chat lfg`.
- Stop-Regel: Bestehende LFG-Tests können ohne eigenes Kanal-Argument nicht mehr bauen →
  Konstruktor-Übergang prüfen (Parameter, nicht Default-Magic).

## M3 — Lurker-Discord-Pitch

- Änderungen: `promos.rs`: `lurker_pitch_enabled` in den bestehenden Settings-SELECT
  aufnehmen; Eigenkanal-Gate (identities-Query: twitch_user_id des Logins ==
  `self.api.bot_user_id()`); nach der Kandidaten-Selektion: schon gepitchte Identity-Keys
  aus `twitch_lurker_pitch_log` herausfiltern; Restmenge > 0 → Pitch-Text mit Invite
  senden und bei Erfolg je Kandidat loggen; Restmenge == 0 → bestehender
  Channel-Points-Text wie bisher. Konstanten `LURKER_PITCH_REPLY`, Schwellen neben :283-289.
  Tests: erstgepitcht wird geloggt und bekommt Pitch-Text; zweiter Lauf bekommt Fall-back;
  Fremdkanal nie Pitch; Flag 0 nie Pitch.
- Erwarteter Zwischenzustand: `cargo test -p tb-chat` grün.
- Validierung: Test-DB + `cargo test -p tb-chat promos` (Filter über Testnamen ergänzen).
- Stop-Regel: sqlx-Cache blockt → `cargo sqlx prepare` gegen Test-DSN, nie
  Query-Makros zu Runtime-Queries abwerten.

## M4 — Dashboard-Flag

- Änderungen: `lurker_tax_settings.rs`: GET liefert `lurker_pitch_enabled`, POST nimmt es
  (optional, Default unverändert) an; Tests erweitern (Roundtrip, Default).
- Erwarteter Zwischenzustand: `cargo test -p tb-dashboard-api lurker_tax` grün.
- Validierung: Test-DB + Filter-Lauf.
- Stop-Regel: Auth-Verhalten darf sich nicht ändern (Partner nur eigener Kanal).

## M5 — Verifikation, Merge, Deploy, Live-Beweis

- `claude-config/bin/verify-change.sh Deadlock-Twitch-Bot` (Repo-Pflichtflags),
  `diff-policy.py` gegen origin/main, frischer Review-Agent (read-only) gegen Contract
  + Diff, Merge nach main, `systemctl --user restart deadlock-twitch-bot-rust.service`,
  Live-Beweis: Log zeigt `foreign_channel`-Silents für Fremdkanäle, Lurker-Tax in
  Fremdkanälen unverändert, Pitch-Flag im Dashboard-Endpoint ablesbar (0).
- Stop-Regel: Review-BLOCKING vor Merge abarbeiten, dann Tests + Review erneut.

ORCHESTRIERUNG[OR-1]: Klasse mittel | Phase contract | Artefakt: .tasks/2026-08-30-lurker-pitch/

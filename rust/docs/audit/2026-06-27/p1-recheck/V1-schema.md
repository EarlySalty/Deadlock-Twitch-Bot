# V1 Schema-Recheck: P1 Analytics-Typdrift

Rolle: Verifizierer + Git-Archaeologe. Keine Secrets, keine Runtime-DB, kein
Checkout/Add/Reset/Commit/Push. Es wurden nur Git-Read-only-Kommandos,
statische Source-/Doku-Inspection und diese Audit-Dokumentation verwendet.

## Kurzurteil

Gesamtverdict: `FIX-CLEAR`.

Die bereits in GitHub gebauten Schema-Reparaturen existieren, sind aber
feature-/spaltenspezifisch fuer benachbarte Spalten. Es gibt keine saubere
bestehende Migration fuer:

- `twitch_stream_sessions.id`
- `twitch_stream_sessions.avg_viewers`
- `twitch_session_viewers.session_id`
- `twitch_session_viewers.ts_utc`
- `twitch_chat_messages.session_id`
- `twitch_chat_messages.message_ts`

Die wahrscheinlichste Erklaerung ist nicht "bewusst ausgelassen", sondern
"durch feature-getriebene Reparaturen uebersehen": Der Initial-Commit
`1d977df` materialisierte die Baseline aus `bot/storage/pg.py ensure_schema`,
waehrend das finale Analytics-Schema in `bot/migrations/twitch_analytics_schema.sql`
fuer diese Spalten bereits `BIGINT`/`TIMESTAMPTZ`/`DOUBLE PRECISION` vorgibt.
Spaetere Commits reparierten nur die konkret gebrochenen Nachbarpfade.

## Git-Archaeologie

Ausgefuehrte Kernkommandos:

- `git log --oneline -- rust/migrations`
- `git log -S"session_id" -- rust/migrations`
- `git log --grep=bigint --grep=timestamptz --grep=schema --grep=contract -i`
- `git show <commit> -- rust/migrations/...`
- `git blame -L ... -- rust/migrations/...`

Relevante Funde:

- `1d977df` (`F1: vollstaendiges Ziel-Schema als clean sqlx-Migrations + hermetic test`) fuegte `20260601000000_baseline_schema.sql` hinzu. Blame zeigt, dass die driftenden Zieltypen aus diesem Commit stammen:
  - `twitch_chat_messages.session_id integer`, `message_ts text`: `rust/migrations/20260601000000_baseline_schema.sql:550-557`
  - `twitch_session_viewers.session_id integer`, `ts_utc text`: `rust/migrations/20260601000000_baseline_schema.sql:1413-1417`
  - `twitch_stream_sessions.id integer`, `avg_viewers real`: `rust/migrations/20260601000000_baseline_schema.sql:1461-1471`
- `0b5c475` fuegte `20260621060000_performance_timestamptz_contract.sql` hinzu. Die Migration repariert nur `twitch_stream_sessions.started_at/ended_at` und `twitch_stats_tracked/category.ts_utc`, explizit wegen Performance-/Heatmap-/Tag-Lesepfaden. `twitch_session_viewers.ts_utc` ist nicht in der Ziel-Liste (`rust/migrations/20260621060000_performance_timestamptz_contract.sql:23-29`).
- `f1d2b5b` fuegte `20260622120000_irc_lurker_timestamptz_contract.sql` hinzu. Sie repariert `twitch_session_chatters.first_message_at/last_seen_at`, setzt aber `twitch_live_state.active_session_id` und `twitch_session_chatters.session_id` sogar auf `INTEGER` (`rust/migrations/20260622120000_irc_lurker_timestamptz_contract.sql:42-69`).
- `7b4f513` fuegte `20260622130001_session_boolean_flags.sql` hinzu. Diese Migration repariert nur `twitch_stream_sessions.is_mature` und `had_deadlock_in_session` auf `BOOLEAN` (`rust/migrations/20260622130001_session_boolean_flags.sql:1-58`).
- `4788154` fuegte `20260622140000_b2_session_id_bigint.sql` hinzu. Die Commit-Message lautet `fix(db): #11 B2 - session_id/active_session_id zurueck auf BIGINT`; die Migration korrigiert aber nur `twitch_live_state.active_session_id` und `twitch_session_chatters.session_id` (`rust/migrations/20260622140000_b2_session_id_bigint.sql:1-34`). Der Kommentar sagt sogar, diese Spalten zeigten fachlich auf `twitch_stream_sessions.id`, repariert `twitch_stream_sessions.id` selbst aber nicht.
- `6e41e52` fuegte `20260622150000_chatter_rollup_timestamptz_contract.sql` hinzu. Auch das ist eine Nachbartabelle: `twitch_chatter_rollup.first_seen_at/last_seen_at`, nicht die sechs Zielspalten.

Negativbelege:

- `git log -S"avg_viewers" -- rust/migrations` findet nach `1d977df` keinen weiteren Migrations-Commit.
- `git log -S"message_ts" -- rust/migrations` findet nach `1d977df` keinen weiteren Migrations-Commit.
- `git log -S"twitch_session_viewers" -- rust/migrations` findet nach `1d977df` keinen weiteren Migrations-Commit.
- `rg` ueber `rust/migrations/*.sql` findet `twitch_session_viewers`, `twitch_chat_messages`, `avg_viewers` und die Zielspalten nur in der Baseline; spaetere Treffer betreffen Nachbarspalten oder andere Tabellen.

## Intent / Soll-Vertrag

Der Intent ist ausreichend belegt:

- Grillme-Querschnittsdirektive: SQL/Schema soll sauber und idiomatisch nativ in Rust gebaut werden, mit versionierten sqlx-Migrationen und gleichem Ziel-Schema statt Python-`ensure_schema`-Nachbau (`rust/docs/audit/2026-06-15-grillme-entscheidungen.md:261-262`; gleicher Inhalt in `_work/grillme-decisions-2026-06-15.md:261-262`).
- Block 11 verlangt vollstaendige Rust-Migrations fuer das Fundament (`rust/docs/audit/2026-06-15-grillme-entscheidungen.md:266-270`).
- Block 12 verlangt den Schema-Endzustand in sauberen Rust-Migrations; One-Shot-Werkzeuge duerfen wegfallen, der Endzustand aber nicht (`rust/docs/audit/2026-06-15-grillme-entscheidungen.md:290-299`).
- ADR 0002 macht `rust/migrations/` zur einzigen DDL-SSOT (`rust/docs/adr/0002-db-sqlx-refinery-shared-schema.md:20-26`) und nennt Vertrags-Tests gegen das echte Schema als Absicherung (`rust/docs/adr/0002-db-sqlx-refinery-shared-schema.md:38-43`).
- Das aktuelle Intent-Ledger wiederholt: Finales Schema liegt in SQL-Migrationen, DB-Zugriff/Schemagodclass wurde durch SQLx-Migrationen ersetzt (`rust/docs/audit/2026-06-27/00-baseline.md:52`, `rust/docs/audit/2026-06-27/00-baseline.md:59`).

Zu den konkreten Spalten steht in Grillme/ADR kein Einzelentscheid "diese Spalte darf legacy bleiben". Der konkrete Soll-Typ kommt aus dem finalen Python-Analytics-Schema, dem Prod-Contract und aktiven Rust-Binds.

## Spalten-Verdicts

| Spalte | Aktueller Fresh-Rust-Typ | Soll-Typ | Verdict | Beleg |
|---|---|---|---|---|
| `twitch_stream_sessions.id` | `integer`, Sequenz `AS integer` | `BIGINT`/`BIGSERIAL` | `FIX-CLEAR` | Fresh: `rust/migrations/20260601000000_baseline_schema.sql:1461-1462`, Sequenz `:1494-1502`; Python final: `bot/migrations/twitch_analytics_schema.sql:171-173`; Prod-Contract: `rust/crates/tb-db/tests/prod_contract.rs:116-118`; Rust liest `id` als `i64`: `rust/crates/tb-monitoring/src/sessions/store.rs:160-179`. |
| `twitch_stream_sessions.avg_viewers` | `real` | `DOUBLE PRECISION` | `FIX-CLEAR` | Fresh: `rust/migrations/20260601000000_baseline_schema.sql:1471`; Python final: `bot/migrations/twitch_analytics_schema.sql:181`; Prod-Contract: `rust/crates/tb-db/tests/prod_contract.rs:130-132`; Rust bindet `f64`: `rust/crates/tb-monitoring/src/sessions/store.rs:172-190`. |
| `twitch_session_viewers.session_id` | `integer` | `BIGINT` | `FIX-CLEAR` | Fresh: `rust/migrations/20260601000000_baseline_schema.sql:1413-1417`; Python final: `bot/migrations/twitch_analytics_schema.sql:209-214`; Prod-Contract: `rust/crates/tb-db/tests/prod_contract.rs:135-139`; Rust bindet `session_id` aus `i64`: `rust/crates/tb-monitoring/src/sessions/store.rs:256-267`. |
| `twitch_session_viewers.ts_utc` | `text` | `TIMESTAMPTZ` | `FIX-CLEAR` | Fresh: `rust/migrations/20260601000000_baseline_schema.sql:1413-1417`; Python final: `bot/migrations/twitch_analytics_schema.sql:209-214`; Prod-Contract: `rust/crates/tb-db/tests/prod_contract.rs:140-143`; Rust vergleicht `MAX(sv.ts_utc) < NOW()`: `rust/crates/tb-monitoring/src/sessions/store.rs:502-510`. |
| `twitch_chat_messages.session_id` | `integer` | `BIGINT` | `FIX-CLEAR` | Fresh: `rust/migrations/20260601000000_baseline_schema.sql:550-557`; Python final: `bot/migrations/twitch_analytics_schema.sql:250-258`; Rust schreibt `session_id` aus der Session-ID und joint gegen `twitch_stream_sessions.id`: `rust/crates/tb-chat/src/chatter_tracking.rs:270-282`, `rust/crates/tb-analytics/src/post_stream.rs:961-963`. |
| `twitch_chat_messages.message_ts` | `text` | `TIMESTAMPTZ` | `FIX-CLEAR` | Fresh: `rust/migrations/20260601000000_baseline_schema.sql:550-557`; Python final: `bot/migrations/twitch_analytics_schema.sql:250-258`; Rust schreibt `DateTime<Utc>` und kommentiert `message_ts = timestamptz`: `rust/crates/tb-chat/src/chatter_tracking.rs:270-282`; Rust decodiert `MessageRow.message_ts: DateTime<Utc>`: `rust/crates/tb-analytics/src/chat_analytics.rs:72-80`; Timestamp-Arithmetik: `rust/crates/tb-analytics/src/post_stream.rs:956-966`. |

Kein `ALREADY-CLEAN`: Fuer keine der sechs Spalten existiert eine spaetere
Korrektur-Migration oder eine dokumentierte bewusste Abweichung.

Kein `ASK-USER`: Der generelle Schema-Intent und die konkreten Solltypen sind
ausreichend belegt. Grillme/ADR nennen die sechs Spalten nicht einzeln, aber sie
geben keine Freigabe fuer eine abweichende Legacy-Baseline.

## B4-024 mitziehen?

Ja, fachlich mitziehen.

Grund:

- Python final definiert `twitch_raid_retention.target_session_id BIGINT REFERENCES twitch_stream_sessions(id)` (`bot/migrations/twitch_analytics_schema.sql:816-823`).
- Rust-Baseline definiert `target_session_id integer` (`rust/migrations/20260601000000_baseline_schema.sql:1351-1357`).
- `raid_retention.rs` loest `target_session_id` als `i64`, castet beim Insert aber explizit `$6::int4` (`rust/crates/tb-monitoring/src/raid_retention.rs:90-124`).
- Der Commit `b8bcda4` hat diesen `target_session_id::int4-Cast` in der Commit-Message sogar als Bestandteil der neuen Raid-Retention-Implementierung dokumentiert. Das war keine saubere Bigint-Loesung, sondern ein int4-Vertrag im neuen Code.

Fix-Spec dazu:

- Neue Migration sollte auch `twitch_raid_retention.target_session_id` von `integer` auf `BIGINT` konvertieren, idempotent nur bei `int4`.
- Code-Fix in `rust/crates/tb-monitoring/src/raid_retention.rs`: `$6::int4` entfernen oder auf `$6::bigint` aendern.
- Tests/Fixtures, die `target_session_id INTEGER` erwarten, muessen auf `BIGINT`/`i64` angepasst werden.

## Praezise Fix-Spec

Neue Migration nach dem Muster `20260622140000_b2_session_id_bigint.sql` und
`20260621060000_performance_timestamptz_contract.sql`, z.B.
`rust/migrations/20260627xxxxxx_analytics_session_contract.sql`.

Zieltypen:

- `twitch_stream_sessions.id`: `BIGINT`
- `twitch_stream_sessions_id_seq`: falls Sequenz noch `AS integer`, auf Bigint-Faehigkeit bringen. Praktisch: Sequenz nicht int4-limitiert lassen; mindestens `ALTER SEQUENCE ... AS bigint` pruefen/setzen.
- `twitch_stream_sessions.avg_viewers`: `DOUBLE PRECISION`
- `twitch_session_viewers.session_id`: `BIGINT`
- `twitch_session_viewers.ts_utc`: `TIMESTAMPTZ`
- `twitch_chat_messages.session_id`: `BIGINT`
- `twitch_chat_messages.message_ts`: `TIMESTAMPTZ`
- Mit B4-024: `twitch_raid_retention.target_session_id`: `BIGINT`

Idempotenz:

- Jede Spalte nur konvertieren, wenn Tabelle/Spalte existiert und `atttypid` noch nicht dem Zieltyp entspricht.
- Fuer `integer -> bigint`: `ALTER COLUMN ... DROP DEFAULT`, dann `TYPE bigint USING col::bigint`.
- Fuer `real -> double precision`: `ALTER COLUMN ... TYPE double precision USING col::double precision`.
- Fuer `text -> timestamptz`: defensiver `USING CASE WHEN col IS NULL OR BTRIM(col::text) = '' THEN NULL ELSE col::text::timestamptz END`.
- Defaults danach wieder setzen, wo sie fachlich existieren:
  - `avg_viewers DEFAULT 0`
  - `message_ts`/`ts_utc` kein neuer Default, falls vorher keiner vorhanden war.
- PK/FK/Index-Rebuild-Risiko beachten: `twitch_stream_sessions.id`, `twitch_session_viewers`-PK, `twitch_chat_messages`-Indexes und `twitch_raid_retention`-FK koennen betroffen sein.

Preflight vor Live-Deploy, falls die Migration nicht als No-op laeuft:

- Spaltentypen per `information_schema.columns` pruefen.
- Fuer Text-Zeitspalten parsebare Werte pruefen, bevor `text::timestamptz` deployt wird.
- Maxwerte fuer int4-Spalten pruefen, falls historische Werte nahe/ueber int4-Grenze erwartet werden.
- Zeilenanzahl/Table-Size der betroffenen Tabellen pruefen, besonders `twitch_chat_messages`.

## Live-DB-Risiko

Ohne Live-DB-Zugriff bleibt "Live unbetroffen" eine Inferenz. Der Prod-Contract
belegt direkt vier der sechs Zielspalten (`twitch_stream_sessions.id`,
`avg_viewers`, `twitch_session_viewers.session_id`, `ts_utc`) als Prod-Vertrag
(`rust/crates/tb-db/tests/prod_contract.rs:113-143`). `twitch_chat_messages`
ist dort nicht abgedeckt, aber aktive Rust-Pfade setzen `TIMESTAMPTZ`/`BIGINT`
voraus.

Deploy-Risiko einer direkten ALTER-Migration:

- Wenn Live bereits korrekt ist, ist eine gut guardete Migration ein No-op.
- Wenn Live noch driftet, nimmt `ALTER TABLE ... ALTER COLUMN TYPE` einen starken
  Table-Lock und kann die Tabelle/Indexes rewriten. Das betrifft besonders
  `twitch_chat_messages`, `twitch_session_viewers` und PK/FK-nahe Spalten.
- `CONCURRENTLY` hilft nicht fuer `ALTER COLUMN TYPE` selbst. Concurrent Index
  Builds helfen nur in einer groesseren Expand/Backfill/Swap-Strategie.
- Backfill ist bei direktem `USING` technisch nicht separat noetig, aber bei
  grossen Live-Tabellen oder unklarer Text-Parse-Qualitaet ist ein
  Wartungsfenster oder eine Expand-Contract-Migration sicherer.

## Gesamtentscheidung

`FIX-CLEAR`: Die GitHub-Aenderungen enthalten Teilreparaturen fuer benachbarte
Schema-Drifts, aber keine saubere Loesung fuer die sechs Analytics-Spalten.
Die Solltypen sind aus Finalschema, Prod-Contract und Rust-Code klar. B4-024
sollte im selben Schema-/Code-Cluster mitgezogen werden, weil es denselben
`twitch_stream_sessions.id`-Bigint-Vertrag verletzt.

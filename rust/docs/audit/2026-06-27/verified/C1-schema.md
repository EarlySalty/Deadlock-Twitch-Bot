# C1 Schema-Verifikation

Rolle: adversarialer Verifizierer, refute-by-default. Es wurde keine Runtime-DB
geoeffnet, keine Secrets gelesen und kein Git benutzt. Erlaubte Verifikation:
statische Migration-/Code-Inspection plus `cargo test -p tb-db --no-run`.

## Kurzurteil

Ein frischer Lauf der Rust-SQLx-Migrationen erzeugt die geprueften Spalten
weiterhin mit `int4`/`real`/`text`. Keine spaetere Rust-Migration korrigiert
`twitch_stream_sessions.id`, `twitch_stream_sessions.avg_viewers`,
`twitch_session_viewers.session_id`, `twitch_session_viewers.ts_utc`,
`twitch_chat_messages.session_id` oder `twitch_chat_messages.message_ts`.

Der Befund ist ein echter Deploy-Gate-Bug fuer Fresh-DB/Rebuild aus
`rust/migrations/*.sql`.
Die Live-DB ist nach Source-/Betriebslogik sehr wahrscheinlich nicht betroffen:
laufende Analytics wuerden mit `message_ts text`/`ts_utc text` gegen
`DateTime<Utc>`-Binds sofort brechen; der Prod-Vertrags-Test dokumentiert zudem
bigint/timestamptz/double precision als Live-Vertrag.
Live-Verifikation, eine Zeile:
`SELECT table_name,column_name,data_type FROM information_schema.columns WHERE (table_name,column_name) IN (('twitch_stream_sessions','id'),('twitch_stream_sessions','avg_viewers'),('twitch_session_viewers','session_id'),('twitch_session_viewers','ts_utc'),('twitch_chat_messages','session_id'),('twitch_chat_messages','message_ts'),('twitch_raid_retention','target_session_id')) ORDER BY table_name,column_name;`

## Migrations-Chronologie und Grep-Ergebnis

Chronologisch gelistet und per `rg` auf
`twitch_stream_sessions|twitch_session_viewers|twitch_chat_messages|avg_viewers|message_ts|ts_utc|session_id`
geprueft:

| Migration | Zielcluster-Treffer |
|---|---|
| `20260601000000_baseline_schema.sql` | Erzeugt alle drei Zieltabellen; setzt die Drift-Typen: `twitch_chat_messages.session_id integer`, `message_ts text`; `twitch_session_viewers.session_id integer`, `ts_utc text`; `twitch_stream_sessions.id integer`, `avg_viewers real`; legt nur Default/PK/Indexes an. |
| `20260601000100_observability_hypertable.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260612120000_add_stats_leaderboard_indexes.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260616000000_add_engagement_output_mode.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260617000000_seed_social_media_auto_approve.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260617010000_tb_dashboard_api_billing_profiles.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260617020000_add_engagement_shadow_forwarded.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260617030000_baseline_missing_tables.sql` | Referenziert `twitch_stream_sessions(id)` fuer neue AI-Tabellen mit `session_id BIGINT`, aendert die Zieltabellen aber nicht. |
| `20260617040000_self_explainer_and_irc_read.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260618000000_schema_cleanup_streamers.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260618010000_conversation_scam_guard.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260618020000_conversation_scam_learnings.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260619010000_runtime_type_contract.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260621055701_presence_tick_timestamptz_contract.sql` | Repariert `twitch_viewer_presence_ticks`, nicht die Zieltabellen. |
| `20260621060000_performance_timestamptz_contract.sql` | Repariert nur `twitch_stream_sessions.started_at/ended_at` und `twitch_stats_* .ts_utc`; nicht `twitch_session_viewers.ts_utc`. |
| `20260621070000_golive_tips.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260621080000_streamer_onboarding.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260622120000_irc_lurker_timestamptz_contract.sql` | Behandelt `twitch_live_state.active_session_id` und `twitch_session_chatters.session_id`, nicht die Zieltabellen. |
| `20260622130000_partner_state_keystone.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |
| `20260622130001_session_boolean_flags.sql` | Repariert nur `twitch_stream_sessions.is_mature` und `had_deadlock_in_session`. |
| `20260622140000_b2_session_id_bigint.sql` | Repariert nur `twitch_live_state.active_session_id` und `twitch_session_chatters.session_id`. |
| `20260622150000_chatter_rollup_timestamptz_contract.sql` | Repariert Rollup-Zeitspalten, nicht die Zieltabellen. |
| `20260623150000_drop_manual_verified_columns.sql` | Kein Zieltabellen-/Zielspalten-Treffer. |

Wichtige Zeilen:

- `rust/migrations/20260601000000_baseline_schema.sql:550-557`:
  `twitch_chat_messages.id integer`, `session_id integer`, `message_ts text`.
- `rust/migrations/20260601000000_baseline_schema.sql:1413-1417`:
  `twitch_session_viewers.session_id integer`, `ts_utc text`.
- `rust/migrations/20260601000000_baseline_schema.sql:1461-1471`:
  `twitch_stream_sessions.id integer`, `avg_viewers real`.
- `rust/migrations/20260601000000_baseline_schema.sql:1494-1502`:
  `twitch_stream_sessions_id_seq AS integer`, owned by `id`.
- `rust/migrations/20260621060000_performance_timestamptz_contract.sql:24-29`:
  Korrektur-Liste enthaelt `twitch_stream_sessions.started_at/ended_at` und
  `twitch_stats_* .ts_utc`, aber nicht `twitch_session_viewers.ts_utc`.
- `rust/migrations/20260622130001_session_boolean_flags.sql:23-56`:
  Korrigiert nur die beiden Boolean-Flags auf `twitch_stream_sessions`.
- `rust/migrations/20260622140000_b2_session_id_bigint.sql:17-32`:
  Korrigiert nur `twitch_live_state.active_session_id` und
  `twitch_session_chatters.session_id`.

## Spalten-Verdicts

| Spalte | Finaler Fresh-Typ aus Rust-Migrationen | Erwarteter Typ / Vertrag | Verdict |
|---|---:|---:|---|
| `twitch_stream_sessions.id` | `integer`/int4, Sequenz `AS integer` | Python-Finalschema `BIGSERIAL`; Prod-Vertrag `bigint` in `prod_contract.rs:116-118`; Rust liest IDs als `i64` in `sessions/store.rs:160-179`. | `CONFIRMED-fresh-break` |
| `twitch_stream_sessions.avg_viewers` | `real` | Python-Finalschema `DOUBLE PRECISION`; Prod-Vertrag `double precision` in `prod_contract.rs:130-133`; Rust bindet/aggregiert als `f64`. | `CONFIRMED-fresh-break` |
| `twitch_session_viewers.session_id` | `integer` | Python-Finalschema `BIGINT`; Prod-Vertrag `bigint` in `prod_contract.rs:135-139`; Rust bindet `i64` und nutzt `session_id = ANY(bigint[])`-Familien. | `CONFIRMED-fresh-break` |
| `twitch_session_viewers.ts_utc` | `text` | Python-Finalschema `TIMESTAMPTZ`; Prod-Vertrag `timestamp with time zone` in `prod_contract.rs:140-143`; Rust insertet `DateTime<Utc>` und vergleicht `MAX(sv.ts_utc) < NOW()`. | `CONFIRMED-fresh-break` |
| `twitch_chat_messages.session_id` | `integer` | Python-Finalschema `BIGINT`; Rust schreibt `session_id` aus `i64`-Session-IDs und Analytics nutzt bigint-Arrays/Joins. | `CONFIRMED-fresh-break` |
| `twitch_chat_messages.message_ts` | `text` | Python-Finalschema `TIMESTAMPTZ`; Rust schreibt `DateTime<Utc>`, dekodiert `MessageRow.message_ts: DateTime<Utc>` und nutzt `message_ts >= $1`, `date_trunc`, `EXTRACT`, Timestamp-Differenzen. | `CONFIRMED-fresh-break` |

Nebenbeobachtung: `twitch_chat_messages.id` bleibt in der Rust-Baseline ebenfalls
`integer` (`20260601000000_baseline_schema.sql:550-551`), waehrend das
Python-Finalschema `BIGSERIAL` nutzt. Diese Spalte war nicht Teil der Kernfrage,
ist aber von B9-004 mit umfasst.

Keine der Zielspalten ist `FALSE-ALARM` oder `ALREADY-FIXED`.
`ALREADY-FIXED` gilt nur fuer benachbarte Drift-Klassen:
`started_at/ended_at` durch `20260621060000_performance_timestamptz_contract.sql`,
Session-Boolean-Flags durch `20260622130001_session_boolean_flags.sql` und
`twitch_live_state.active_session_id`/`twitch_session_chatters.session_id` durch
`20260622140000_b2_session_id_bigint.sql`.

## Prod-Contract-Test

`rust/crates/tb-db/tests/prod_contract.rs` ist ein Runtime-Vertrag gegen die
echte DB, nicht gegen Fresh-Migrationen:

- DSN: `TWITCH_ANALYTICS_DSN`, nicht `DATABASE_URL` (`prod_contract.rs:11-13`).
- Mechanik: Wenn `TWITCH_ANALYTICS_DSN` fehlt, schreiben beide Tests
  `SKIP: TWITCH_ANALYTICS_DSN nicht gesetzt` und `return`en
  (`prod_contract.rs:35-42`, `94-101`).
- DB-Zugriff: Nur bei gesetztem `TWITCH_ANALYTICS_DSN`; dann `tb_db::connect`
  und `information_schema.columns` (`prod_contract.rs:15-22`, `50-52`,
  `109-111`).
- Erwartete Zieltypen: `twitch_stream_sessions.id = bigint`,
  `avg_viewers = double precision`, `twitch_session_viewers.session_id = bigint`,
  `ts_utc = timestamp with time zone` (`prod_contract.rs:116-143`).
- Nicht abgedeckt: `twitch_chat_messages.session_id` und
  `twitch_chat_messages.message_ts` kommen in `prod_contract.rs` nicht vor.

Build-Verifikation:

```text
env -u DATABASE_URL -u TWITCH_ANALYTICS_DSN SQLX_OFFLINE=true cargo test -p tb-db --no-run
Finished `test` profile ... target(s) in 1.10s
Executables: tb_db, hermetic, prod_contract, retry_tx
```

Ergebnis: kompiliert. Kein Test wurde ausgefuehrt, keine DB wurde verbunden.

## Live-DB-Logikcheck

Der Live-Bot (#291) laeuft. Wenn die Live-DB dieselben Fresh-Typen haette,
waeren zentrale Analytics-Pfade hochgradig fehleranfaellig:

- `twitch_session_viewers.ts_utc text`: `MAX(sv.ts_utc) < NOW() - INTERVAL '1 hour'`
  in `tb-monitoring/src/sessions/store.rs:504-510` wuerde `text < timestamptz`
  verlangen.
- `twitch_chat_messages.message_ts text`: aktive Query-Familien nutzen
  `message_ts >= $1`, `message_ts AT TIME ZONE`, `date_trunc('minute', message_ts)`,
  `MAX(message_ts)-MIN(message_ts)` und `m.message_ts - s.started_at`.
- `session_id int4`: Rust bindet Session-IDs als `i64` und mehrere Handler nutzen
  `ANY($1::bigint[])`.
- `avg_viewers real`: weniger Crash-Risiko, aber der dokumentierte Prod-Vertrag
  und die Rust-Aggregate erwarten `double precision`/`f64`.

Schlusskette: Die Source zeigt harte Operator-/Funktionskonflikte fuer eine
Live-DB mit Fresh-Rust-Typen. Da diese Kern-Analytics im laufenden Betrieb nicht
als tot vorausgesetzt werden und `prod_contract.rs:113-115` sogar dokumentiert,
dass Prod fuer Session-Tabellen bereits `timestamptz`/`boolean`/`bigint` fuehrt,
ist die wahrscheinlichste Erklaerung: Live wurde aus Python-/Prod-Migrationen
oder manueller Reparatur korrekt typisiert; der Bug betrifft Fresh-Aufbau aus
Rust-Migrationen. Das ist eine Inferenz, keine direkte Live-DB-Abfrage.

## B9-Befunde

| Befund | Verdict | Beleg |
|---|---|---|
| B9-001 `twitch_stream_sessions.id` | `CONFIRMED-fresh-break` | Fresh `integer` + `AS integer` in Rust-Baseline; keine spaetere ALTER-Type-Migration; Prod-Vertrag erwartet `bigint`. Nicht live-bestaetigt. |
| B9-002 `twitch_stream_sessions.avg_viewers` | `CONFIRMED-fresh-break` | Fresh `real`; keine spaetere ALTER-Type-Migration; Prod-Vertrag/Python-Finalschema erwarten `double precision`. Nicht live-bestaetigt. |
| B9-003 `twitch_session_viewers.session_id`, `ts_utc` | `CONFIRMED-fresh-break` | Fresh `integer`/`text`; keine spaetere ALTER-Type-Migration; Prod-Vertrag erwartet `bigint`/`timestamp with time zone`; `MAX(sv.ts_utc) < NOW()` belegt Timestamp-Annahme. Nicht live-bestaetigt. |
| B9-004 `twitch_chat_messages.session_id`, `message_ts` | `CONFIRMED-fresh-break` | Fresh `integer`/`text`; keine spaetere ALTER-Type-Migration; Rust schreibt/liest Zeitwerte als `DateTime<Utc>` und nutzt Timestamp-Operatoren. Nicht live-bestaetigt. |

## B4-Folgebefunde

| Befund | Schema-Folge real? | Live-Relevanz |
|---|---|---|
| B4-016 Watch-Time / `last_seen` | Real fuer Fresh: `MAX(cm.message_ts)` und `session_id = ANY(bigint[])` treffen Fresh-`text`/int4. | Wahrscheinlich Fresh-only; Live wuerde sonst breite Audience-Analytics brechen. |
| B4-017 Audience Demographics | Real fuer Fresh: `EXTRACT(... cm.message_ts AT TIME ZONE ...)` und `cm.message_ts >= $1` brauchen Timestamp. | Wahrscheinlich Fresh-only. |
| B4-018 Chat Social Graph | Real fuer Fresh: `m.message_ts >= $2` mit `DateTime<Utc>` gegen Fresh-`text`; Session-ID-Vertrag driftet. | Wahrscheinlich Fresh-only. |
| B4-019 Raw Chat Status | Real fuer Fresh: `MAX(m.message_ts)` wird als Timestamp/DateTime verwendet und `m.message_ts >= $2` gefiltert. | Wahrscheinlich Fresh-only. |
| B4-020 Chat Analytics/Content/Hype | Real fuer Fresh: `MessageRow.message_ts: DateTime<Utc>`, `date_trunc`, `MIN/MAX`, Timestamp-Differenzen. | Wahrscheinlich Fresh-only. |
| B4-021 Viewer Detail | Real fuer Fresh: `EXTRACT(HOUR/DOW FROM message_ts)` und `message_ts >= $3`. | Wahrscheinlich Fresh-only. |
| B4-022 Session-Viewer-Kurven | Real fuer Fresh: `twitch_session_viewers.session_id` int4 statt bigint und `ts_utc` text statt timestamptz. | Wahrscheinlich Fresh-only. |
| B4-024 Raid-Retention `target_session_id` Cast | Real fuer Fresh: `twitch_raid_retention.target_session_id integer` in der Rust-Baseline und Code castet `$6::int4` (`raid_retention.rs:118-125`) trotz `i64`-Session-ID. | Live-Risiko, aber nicht als aktuell live-broken bestaetigt; bricht sicher bei IDs ausserhalb int4 oder wenn der Live-Vertrag fuer `target_session_id` bigint erzwingt. |
| B4-026 Post-Stream Chat-/Rawdaten | Real fuer Fresh: `ORDER BY message_ts` kann scheinbar sortieren, aber `message_ts - started_at` und Timestamp-Arithmetik brechen auf `text`. | Wahrscheinlich Fresh-only. |
| B4-027 `avg_viewers` Praezision | Real fuer Fresh: `real` statt `double precision`; eher Paritaets-/Praezisionsbruch als Operator-Crash. | Wahrscheinlich Fresh-only. |

Nicht Teil dieses Schema-Typ-Clusters: B4-023 (`known_from_raider`) ist ein
separater Logikbug und wird hier nicht widerlegt oder bestaetigt.


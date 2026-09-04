# Evidence: Streams-Tab, Retention, Geistersessions, Titel

## Frontend (Streams-Tab)
- Seite: `bot/dashboard_v2/src/pages/Sessions.tsx:15` `Sessions`, Daten über `fetchOverview` (`Sessions.tsx:18`)
- KPI-Kacheln lesen `data.summary.streamCount`, `totalAirtime`, `avgViewers`, `peakViewers` (`Sessions.tsx:44-67`)
- Session-Karte: `SessionCard` `Sessions.tsx:130`; Retention-Farbe `Sessions.tsx:132`; Anzeige `retention10m` mit Label "10m Retention" `Sessions.tsx:180-183`; Titel-Fallback `'Untitled Stream'` `Sessions.tsx:157`
- Detail-Boxen 5/10/20 Min und Dropoff `Sessions.tsx:230-234`
- Typ `StreamSession` `bot/dashboard_v2/src/types/analytics.ts:3-22`

## Backend (Sessions-Liste und Summary)
- `overview_sessions` `rust/crates/tb-analytics/src/overview.rs:509`, SQL `base_sessions` mit `duration_seconds` aus `ended_at - started_at` `overview.rs:519-539`; Filter nur `ended_at IS NOT NULL` `overview.rs:536`
- Mapping mit `clamp_pct` `overview.rs:607-630`, Struct `OverviewSession` `overview.rs:462-495`, `SessionRaw` `overview.rs:497-516`
- `overview_metrics` SQL `overview.rs:40-100`: `session_count = COUNT(*)` `overview.rs:97`, `total_airtime_hours` `overview.rs:66`, `avg_retention_10m` bereits auf `avg_viewers >= 3` beschränkt `overview.rs:79-83`
- Handler: `rust/crates/tb-dashboard-api/src/handlers/overview.rs:532` ruft `overview_sessions(&pool, &since, login_ref, 50)`; `stream_count: metrics.session_count` `overview.rs:630`; Test `streamCount == totalSessions` `overview.rs:875-877`
- DB-Tests in `tb-analytics` laufen nur mit `TB_TEST_DATABASE_URL` (`overview.rs:704`, Skip-Meldung `overview.rs:822`); Fixture-Insert-Muster `overview.rs:1015`

## Retention-Berechnung (bleibt unangetastet)
- `retention_at` `rust/crates/tb-monitoring/src/sessions/metrics.rs:14-46`: Zuschauer bei Minute N geteilt durch `max(start_viewers, Peak vor N, Wert bei N)`, geklemmt auf 1.0. Wächst der Stream in den ersten 10 Minuten (Normalfall bei kleinen Kanälen), ist der Wert immer 1.0.
- Aufruf beim Finalize `rust/crates/tb-monitoring/src/sessions/tracker.rs:359-361`

## Session-Anlage und Titel
- EventSub `stream.online` eröffnet die Session mit `StreamSnapshot { id, started_at, ..Default::default() }`, also ohne Titel, Spiel und Zuschauer: `rust/crates/tb-monitoring/src/handlers.rs:281-285`
- `ensure_session` für schon offene Session ruft nur `adopt_incomplete` `tracker.rs:214-225`
- `adopt_incomplete` setzt Titel per `COALESCE(stream_title, $4)` und greift nur `WHERE samples = 0 AND start_viewers = 0` `rust/crates/tb-monitoring/src/sessions/store.rs:437-438`. Beim ersten Poll wird `start_viewers` gesetzt; hatte der Snapshot da noch keinen Titel (Helix-Lag), bleibt der Titel für immer leer. Zudem ist `stream_title` in der DB `''` statt `NULL` (alle 419 titellosen Sessions der letzten 30 Tage sind `''`), `COALESCE` würde also auch später nichts mehr füllen.
- `start_session` schreibt `stream_title` als `&new.title` (getrimmter String, ggf. leer) `store.rs:273-296`
- `title_opt` `rust/crates/tb-monitoring/src/stream.rs:71`

## Datenlage (twitch_analytics, 30 Tage, Stand 2026-09-04)
- `retention_10m`: 841 von 1084 Sessions bei 1.0 (Bucket 10), nur 243 darunter
- Titellos: 419 von 1523 Sessions, davon 386 mit `start_viewers > 0` (adopt-Fenster verpasst)
- `peak_viewers = 0`: 78 Sessions, davon 35 unter 300 s (Geister: Flap online/offline, Beispiel earlysalty 10940 um 13:23:38, 1 min, 0 Samples, direkt danach 10941 um 13:24:45), 43 über 300 s mit teils Hunderten Samples (echte Streams ohne Zuschauer, bleiben sichtbar)
- `avg_viewers / peak_viewers` bei Sessions ab 20 min: breite Verteilung, Schwerpunkt 40 bis 70 %, nur 92 von 1204 bei 100 %; für earlysalty 35 bis 70 % bei Sessions, die als 10m-Retention alle 100 % zeigen

## Roter Lauf

Test-DB: eigener Wegwerf-Container `tb-streams-bindung-pg2` (timescale/timescaledb:2.17.2-pg16, `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33093/postgres`, `SQLX_OFFLINE=true`, `TB_TEST_REQUIRE_DB=1`), nicht die Produktions-DB.

### T1 (tb-monitoring, echter Laufzeit-Rot vor REQ4)
Befehl: `cargo test -p tb-monitoring --test write_core offene_session_bekommt_titel_nach_verpasstem_adopt_fenster -- --exact --nocapture`
```
thread 'offene_session_bekommt_titel_nach_verpasstem_adopt_fenster' panicked at crates/tb-monitoring/tests/write_core.rs:383:5:
assertion `left == right` failed: Titel nachgetragen trotz verpasstem adopt-Fenster
  left: Some("")
 right: Some("Ranked Grind Titel")
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 14 filtered out
```
Ursache: `adopt_incomplete` greift nur `WHERE samples = 0 AND start_viewers = 0`; bei samples=3/start_viewers=15 tut es nichts, `stream_title` bleibt `''`.

### T2 (tb-analytics, Rot-Gegenprobe per Sabotage)
T2 ist ein Lib-Unit-Test unter `overview::tests::`, der volle Modulpfad ist im Filter Pflicht (`--exact` mit nur dem Funktionsnamen matcht 0 Tests). Der Rot-Nachweis wurde als Sabotage-Gegenprobe geführt: `GEISTER_FILTER` in beiden Queries auf `""` neutralisiert und die `hold_pct`-Closure auf `|_,_| 0.0` gesetzt.
Befehl: `cargo test -p tb-analytics --lib overview::tests::geistersession_faellt_raus_und_holdpct_gesetzt -- --exact --nocapture`
```
thread 'overview::tests::geistersession_faellt_raus_und_holdpct_gesetzt' panicked at crates/tb-analytics/src/overview.rs:1102:9:
assertion `left == right` failed: Geistersession (peak 0, 60 s) faellt raus
  left: 2
 right: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 457 filtered out
```
Rot-Gegenprobe: Ist `list.len()` = 2 (Geistersession bleibt in der Liste) statt Soll 1. Mit korrektem Code (Filter + `hold_pct`): 1 passed (`list.len()` = 1, `hold_pct` = 60.0, `session_count` = Some(1)).

### Baseline (vorbestehender Fehler, unabhaengig vom Diff)
`tb-analytics` Integrationstest `queue_lease_idempotenz_und_state_sind_atomar` (`tests/ad_manager_store.rs:56`) faellt in der isolierten Test-DB, weil die Migration `20260901100000_twitch_ad_manager.sql:5` ein `ALTER TABLE twitch_raw_chat_ingest_health` auf eine Tabelle macht, die eine fruehere Migration anlegt; der Test wendet nur diese eine Datei auf ein frisches Schema an (42P01).
- Baseline origin/main (per `git stash`, ohne Diff): `test result: FAILED. 0 passed; 1 failed`.
- Mit Diff: `test result: FAILED. 1 passed; 1 failed` (derselbe Test, gleicher 42P01).
Mein Diff fasst weder Migrationen noch `ad_manager` an; der Fehler ist vorbestehend.

## Gruener Lauf

Befehl: `cargo test --no-fail-fast -p tb-analytics -p tb-monitoring -p tb-dashboard-api` (Env: `SQLX_OFFLINE=true`, `TB_TEST_DATABASE_URL=...:33093`, `TB_TEST_REQUIRE_DB=1`).
- tb-analytics lib: 458 passed, 0 failed (enthaelt T2).
- tb-analytics tests/ad_manager_store: 1 passed, 1 failed (Baseline, siehe oben).
- tb-analytics tests/ad_manager_decision: 9 passed.
- tb-dashboard-api lib: 1096 passed, 0 failed, 1 ignored; plan_stufen_gates 12; public_streamer_comparison 6.
- tb-monitoring lib 89; write_core 15 (enthaelt T1); announce 16, chatters_poller 18, eventsub_dispatch 22, hermetic 9, observability_retention 1, poller 23, raid_retention 9, session_id_first 13, subscriptions 18, twitch_rename 10; alle 0 failed.
Einziger roter Test ist die genannte Baseline.

TESTNACHWEIS[TW-1]: 1825 passed, 2 ignored, 1 failed (Baseline ad_manager) | Rot-Gegenprobe: T1 Ist Some("") statt Some("Ranked Grind Titel"); T2 Ist len 2 statt 1

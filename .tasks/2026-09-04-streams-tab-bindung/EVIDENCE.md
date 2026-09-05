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

## Roter Lauf Runde 2

Neuer Regressionstest fuer die Merge-Gate-Funde 1 bis 3 (Anker und Existenz-Check sehen Geistersessions nicht mehr): `overview::tests::juengste_geistersession_ist_kein_anker_und_zaehlt_nicht`. Fixture: echte Session (id 1, peak 10, 30 min, 2026-05-01) plus juengere Geistersession (id 2, peak 0, 60 s, 2026-06-01) fuer denselben Streamer. `letzte_beendete_session` (stufe.rs) soll die echte Session liefern; `overview_session_count` mit `since` = started_at der Geistersession minus 1 s soll 0 liefern.

Befehl: `cargo test -p tb-analytics --lib overview::tests::juengste_geistersession_ist_kein_anker_und_zaehlt_nicht -- --exact --nocapture` (Env: `SQLX_OFFLINE=true`, `TB_TEST_REQUIRE_DB=1`, `TB_TEST_DATABASE_URL=...:33093`).
```
thread 'overview::tests::juengste_geistersession_ist_kein_anker_und_zaehlt_nicht' panicked at crates/tb-analytics/src/overview.rs:1150:9:
assertion `left == right` failed: juengste beendete Session ist Geistersession, Anker bleibt die echte
  left: 2
 right: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 460 filtered out
```
Ursache: `letzte_beendete_session` nahm `MAX(started_at)` ohne Geister-Kriterium (Fund 2), also die juengere Geistersession (id 2) statt der echten (id 1). Der `overview_session_count`-Teil war ebenfalls ungefiltert (Fund 3); der Anker-Assert schlaegt zuerst zu.

### Fix Runde 2
- `GEISTER_FILTER` in `overview.rs` auf `pub` gehoben.
- `overview_session_count` (`overview.rs`) und `letzte_beendete_session` (`stufe.rs`) setzen `GEISTER_FILTER` in ihre Query ein (Alias `s`, `ended_at IS NOT NULL` gilt schon). `latest_ended_session` in `handlers/last_session.rs` delegiert an `letzte_beendete_session` und erbt den Filter; die Testfixture dort bekam die Spalte `peak_viewers`, damit das Fragment aufloest (Sessions dort > 300 s, also keine Geister).
- Frontend Fund 4: `getHoldColor(holdPct)` (Schwellen 60/40, Konstanten `SCORE_GOOD`/`SCORE_OK`/`SCORE_BAD` wie `getRetentionColor`) in `formatters.ts`; `SessionTable.tsx` nutzt sie fuer den Balken statt `getRetentionColor`. `Sessions.tsx` behaelt seine Tailwind-Klassen (60/40).

## Gruener Lauf Runde 2

Befehl: `cargo test -p tb-dashboard-api -p tb-analytics -p tb-monitoring --no-fail-fast` (Env wie oben).
- tb-analytics lib: 461 passed, 0 failed (enthaelt T2 und den neuen Anker-Test).
- tb-dashboard-api lib: 1114 passed, 0 failed, 1 ignored; plan_stufen_gates 12; public_streamer_comparison 6 (1 ignored).
- tb-monitoring lib 89; write_core 15 (T1); weitere Integrationssuiten 0 failed.
- Einziger roter Test: Baseline `queue_lease_idempotenz_und_state_sind_atomar` (42P01, vorbestehend, siehe oben).
- Frontend: `npx tsc --noEmit` exit 0, `npm run build` exit 0.
- Clippy der drei Crates exit 0; die 2 Warnungen "this assertion has a constant value" liegen in `self_explainer.rs` (unangetastet, vorbestehend), keine neuen Warnungen aus dem Diff.

## Runde 3 (Merge-Gate-Funde)

### Geister-Definition gilt fuer Anker, Existenz-Check, Liste, Summary
`GEISTER_FILTER` (`overview.rs`) ist die eine Definition, wann eine beendete Session ein Geist ist: kein Peak-Zuschauer, kein Start-Zuschauer und unter 300 s (`COALESCE(peak_viewers,0)=0 AND COALESCE(start_viewers,0)=0 AND Dauer < 300 s`). Dasselbe Fragment haengt an allen vier Lesepfaden: am Anker der letzten Session (`stufe::letzte_beendete_session`, davon abgeleitet `last_session::latest_ended_session` und die Session-Detail-Klemme), am Existenz-Check (`overview_session_count`), an der Sessions-Liste (`overview_sessions`) und an der Summary (`overview_metrics`). Damit sehen Anker, Existenz-Check, Liste und Summary exakt dieselbe Menge an Sessions; ein Geist ist nirgends Anker und zaehlt nirgends mit. Deshalb behaupten die Doku-Kommentare an `latest_ended_session`, `session_detail` und `letzte_beendete_session` keine Gleichheit mehr zu `MAX(started_at)` der beendeten Sessions (der juengste beendete Eintrag kann ein Geist sein).

### Fund 2 (start_viewers)
`GEISTER_FILTER` verlangt jetzt zusaetzlich `COALESCE(s.start_viewers, 0) = 0`. Ein Geist ist nur noch, wer nie einen Zuschauer hatte (Peak 0 und Start 0) und unter 300 s lief. Die Geister-Fixtures in T2 (`geistersession_faellt_raus_und_holdpct_gesetzt`) und im Runde-2-Test (`juengste_geistersession_ist_kein_anker_und_zaehlt_nicht`) setzen keine `start_viewers`, die Spalte ist dort NULL (COALESCE 0), der schaerfere Filter greift unveraendert.

### Fund 3 (Query-Fehler sichtbar)
`stufe::letzte_beendete_session` loggt bei einem Query-Fehler `tracing::warn!(%error, login, "letzte beendete Session nicht ladbar")` und gibt `None` zurueck, statt den Fehler still mit `.ok().flatten()` zu schlucken. `tracing` ist bereits Crate-Dependency.

### Funde 4 und 5 (Frontend)
`StreamSession.holdPct` ist optional (`holdPct?: number`); `fetchSessionDetail` liefert es nicht, die `?? 0`-Stellen bleiben. `formatters.ts` hat neu `getHoldTone(holdPct): 'good' | 'ok' | 'bad'` mit den Schwellen 60/40 als einziger Quelle; `getHoldColor` leitet die Farbe daraus ab. `Sessions.tsx` mappt `getHoldTone` auf die vollstaendigen Tailwind-Literale `text-success` / `text-warning` / `text-error`.

### Fund 6 (Demo-Fixture)
Die vier Demo-Sessions landen jetzt bei holdPct 66.1 / 65.7 / 48.2 / 47.0 (zwei ueber 60, zwei zwischen 40 und 60), berechnet aus angepassten `peakViewers` (620 / 540 / 780 / 740) bei unveraenderten `avgViewers`. Kein Test prueft die konkreten Demo-Zahlen (`demo`-Tests pruefen nur `sessions` als Array und `summary.avgViewers` als Zahl).

## Titel-Backfill historischer Sessions (2026-09-05, Live-DB)
- Quelle 1: `twitch_stats_tracked.stream_title` (Poll-Samples innerhalb der Session, häufigster Titel), Quelle 2: `twitch_channel_updates.title` (letztes Update desselben Users zwischen 24 h vor Start und Ende).
- 1133 beendete Sessions ohne Titel, 956 gefüllt (677 aus Samples, 279 aus Updates), 177 bleiben leer (keine Quelle). earlysalty, 30 Tage: 0 leer.
- SQL: `titel-backfill-2026-09-05.sql`, Rücknahme-Liste: `titel-backfill-2026-09-05.tsv`. Vor dem Lauf hatten die betroffenen Zeilen `''` oder `NULL` (der Altwert wurde nicht je Zeile protokolliert; nach dem Lauf sind noch 95 beendete Sessions `NULL` und 82 `''`). Alle Leser behandeln `''` und `NULL` gleich (`overview_sessions` per `COALESCE(bs.stream_title, '')`, Frontend `session.title || 'Untitled Stream'`), eine Rücknahme per TSV-Ids auf `''` stellt deshalb das sichtbare Verhalten exakt wieder her.
- Bei gleich häufigen Titeln innerhalb einer Session war die Wahl nicht deterministisch (`ORDER BY l.id, n DESC` ohne weiteren Tiebreak); verbindlich ist die TSV, nicht ein Wiederholungslauf des Skripts.
- Der Samples-Join lief über `streamer = streamer_login` (exakt, ohne `LOWER`), weil `twitch_stats_tracked.twitch_user_id` bei 997698 von 4397728 Zeilen leer ist und ein ID-Join Deckung gekostet hätte; nach einer Kanal-Umbenennung kann eine Session dadurch leer bleiben, ein falscher Titel entsteht daraus nicht, weil das Zeitfenster auf die Session begrenzt ist.

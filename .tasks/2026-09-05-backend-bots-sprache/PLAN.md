# Plan: Analyse-Backend Bot-Ausschluss, Sessionsprache, Chat-Lücken-Warnung

status: aktiv
datum: 2026-09-05
klasse: mittel
research: EVIDENCE.md

## Ziel

Siehe CONTRACT.md.

## Milestones

### M0: Baseline
Validierung: `cargo test -p tb-dashboard-api -p tb-analytics -p tb-chat -p tb-monitoring -p tb-engagement` mit Toolchain 1.97 (`~/.rustup/toolchains`, siehe Memory tb-bot-build-toolchain), Ausgabe in Datei umleiten; rote Tests vor dem ersten Edit hier notieren.

### M1: Regressionstest Chat-Lücke (rot)
Änderungen: Test in `rust/crates/tb-analytics` gegen `build_raw_chat_status` mit Fixture: zwei Sessions unter 10 Minuten ohne Chat, eine echte Session mit Chat.
Erwarteter Zwischenzustand: Test rot, Fehlermeldung im Verlauf.
Stop-Regel: Test von Anfang an grün.

### M2: Chat-Lücke fixen und Notizen umformulieren
Änderungen: `raw_chat_status.rs`.
Validierung: M1-Test grün, Paket-Tests grün.

### M3: Gemeinsame Bot-Liste plus Anonym-Regel
Änderungen: `tb-analytics/src/bekannte_bots.rs`, Konsumenten in tb-dashboard-api, tb-chat, tb-monitoring; SQL-Filter in lurker_analysis, audience, viewers, audience_demographics, chat_analytics, follower_funnel um das Anonym-Muster `^justinfan[0-9]+$` erweitern oder die Liste zentral binden.
Validierung: Paket-Tests grün; Unit-Test: `own3d`, `kofistreambot`, `justinfan12345`, `justinfan99999` ausgeschlossen, `nani` nicht.

### M4: Sessionsprache
Änderungen: Session-Schreiber setzt `language`; `audience_demographics.rs` filtert leere Sprache; `backfill.sql` im Task-Ordner (REQ-04: `language <> ''` und `recorded_at <= ended_at` je Session).
Validierung: Unit-Test für die Sprachwahl; `backfill.sql` per `EXPLAIN` gegen die DB geprüft (nur lesen), Ausführung durch den Orchestrator.

## Verlauf

### M0 Baseline (2026-09-05)
Befehl: `cargo test -p tb-dashboard-api -p tb-analytics -p tb-chat -p tb-monitoring -p tb-engagement --no-fail-fast` (Toolchain 1.97.1, SQLX_OFFLINE=1, TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql).
Grün: tb-analytics lib 461, tb-chat 707 (+4 ignored) und alle tb-chat-DB-Binaries, tb-dashboard-api 1114 (+1 ignored) und alle Binaries, tb-monitoring 89 und alle Binaries, tb-engagement 253 und crew_review_store 42 etc.
Rote Baseline (2 Tests, vorbestehend, ausserhalb Scope, scheitern nur weil die Wegwerf-Test-DB nicht alle Fremd-Migrationen traegt):
- `tb-analytics tests/ad_manager_store.rs::queue_lease_idempotenz_und_state_sind_atomar` -> `relation "twitch_raw_chat_ingest_health" does not exist`.
- `tb-engagement tests/ledger_side_effects.rs::engagement_client_verbucht_usage_ins_zentrale_ledger` -> `relation "public.minimax_usage" does not exist`.

### M1 Regressionstest rot (2026-09-05)
Neuer Test `raw_chat_status::tests::geistersessions_unter_10min_loesen_keine_luecke_aus` (2 Geister-Sessions je 5 Minuten mit Presence ohne Chat + 1 echte Session 2h mit Chat).
Roter Lauf wörtlich:
```
test raw_chat_status::tests::geistersessions_unter_10min_loesen_keine_luecke_aus ... FAILED
thread '...' panicked at crates/tb-analytics/src/raw_chat_status.rs:487:9:
assertion `left == right` failed
  left: Bool(true)
 right: false
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 465 filtered out
```
`suspectedIngestionIssue` steht vor dem Fix auf true, weil `gap_sessions` die beiden Geister-Sessions ohne Mindestdauer zählt.


### M2 Fix + Notiztexte (2026-09-05)
Gap-Query zaehlt Sessions nur mit `COALESCE(duration_seconds, EXTRACT(EPOCH FROM (ended_at - started_at))) >= 600`. Notiztexte in Nutzersprache ohne Roh-Chat/KPI/Presence/Rollup/Ingestion/Insert; Fehlerdetail nur ins Log (tracing::warn). Bestandstest `mehrere_luecken...` an die Regel angepasst (Gap-Sessions bekommen echte Dauer).
Ergebnis: `cargo test -p tb-analytics --lib raw_chat_status::` = 6 passed, 0 failed. Rot-Gegenprobe: der M1-Lauf lieferte ohne den Fix true statt false.

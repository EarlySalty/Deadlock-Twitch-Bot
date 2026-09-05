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
Gap-Query zaehlt Sessions nur mit `COALESCE(duration_seconds, EXTRACT(EPOCH FROM (ended_at - started_at))) >= 600`. Notiztexte in Nutzersprache ohne Roh-Chat/KPI/Presence/Rollup/Ingestion/Insert; Fehlerdetail nur ins Log (tracing::debug, in der Review-Runde von warn gesenkt, damit ein alter Fehlereintrag nicht jeden Request meldet). Bestandstest `mehrere_luecken...` an die Regel angepasst (Gap-Sessions bekommen echte Dauer).
Ergebnis: `cargo test -p tb-analytics --lib raw_chat_status::` = 6 passed, 0 failed. Rot-Gegenprobe: der M1-Lauf lieferte ohne den Fix true statt false.

### M3 Gemeinsame Bot-Liste + Anonym-Regel (2026-09-05)
Neues Modul `tb-analytics/src/bekannte_bots.rs`: `KNOWN_CHAT_BOTS` (12 Eintraege, Vereinigung der 20 Listen plus own3d und kofistreambot, alphabetisch), `ist_anonymer_login`, `ist_ausgeschlossener_login`, `ANONYM_LOGIN_REGEX_SQL`. Alle 20 Kopien entfernt: 17 Dateien beziehen `KNOWN_CHAT_BOTS` per Import (overview.rs und mention_scoring.rs als API-stabile Re-Exports fuer die out-of-scope-Konsumenten category_comparison, performance, conversation_scam, pipeline, promos, chatters_poller, raid_retention); die 3 Schreibpfade (chatter_tracking, irc_lurker, irc_message) rufen `ist_ausgeschlossener_login` (inkl. Anonym) statt exaktem `.contains`. tb-monitoring und tb-engagement bekamen tb-analytics als Dependency (kein Zyklus, mit den Cargo-Manifesten geprueft).
SQL-Anonym-Filter: 53 `LOWER(chatter_login) <op> ALL(...)`-Praedikate und 8 `chatter_login NOT IN ({})`-Format-Klauseln plus der internal_home-Helfer um `AND LOWER(col) !~ '^justinfan[0-9]+$'` erweitert. Makro-Queries: `.sqlx`-Cache mit `cargo sqlx prepare` gegen die Prod-DB (twitch_analytics, Describe-only, read-only) neu erzeugt; 27 Cache-Eintraege tragen die justinfan-Aenderung, keine unveraenderte Query driftete (keine M-Datei). REQ-03 (audience_demographics Q1) liegt in diesem Commit mit.
Bestandstests angepasst: `mention_scoring::whitelisted_bots_enthält_alle_aus_python` erwartet jetzt die 12er-Liste; 3 Test-Fixtures (chat_content_analysis, chat_social_graph, chat_hype_timeline) bekamen ended_at/duration_seconds, weil sie build_raw_chat_status mit der neuen Gap-Query aufrufen.
Scope-Grenze: category_comparison.rs, performance.rs, viewer_timeline.rs, stream_kennzahlen.rs und die Dashboard-chat_*-Handler binden die Liste ebenfalls, liegen aber nicht in der 20er-Liste bzw. ausserhalb des erlaubten Bereichs und bleiben unangetastet (die Re-Exports halten sie kompilierbar; die Anon-Regex erreicht sie nicht).

### M4 Sessionsprache + Backfill (2026-09-05)
Ursache der leeren language-Werte belegt: EventSub `stream.online` (handlers.rs:279) eroeffnet die Session mit `StreamSnapshot { ..Default::default() }`, also leerer Sprache; der spaetere Poll rief nur `adopt_incomplete`/`backfill_missing_meta` (Titel/Spiel), nie language. Fix: `SessionStore::backfill_missing_meta` traegt jetzt auch die Sprache nach (nur wenn leer), der Tracker-Adopt-Zweig uebergibt `stream.language`. Poller-erst-Pfad setzt die Sprache schon im INSERT (start_session). audience_demographics Q1 filtert `COALESCE(language,'') <> ''`, "Unbekannt" nur bei leerer Ergebnismenge (REQ-03). `backfill.sql` im Task-Ordner: Zaehl-SELECT (884 von 1127 leeren Sessions bekommen einen Wert) plus einmaliges UPDATE (juengster twitch_channel_updates-Wert je twitch_user_id mit language <> '' und recorded_at <= ended_at, nur leere Sessions); per EXPLAIN gegen Prod geprueft, nicht ausgefuehrt.

### Review (2026-09-05)
Runde 1 (gate_hook --review): BLOCK auf zwei Dashboard-chat-Tests, die angeblich an der neuen Gap-Query brechen. Empirisch falsch (localhost_200 je 3/3 gruen, Mega-Lauf tb-dashboard-api 1114/0), weil die Fixtures keine Chat-Nachrichten einfuegen und coverage_start None bleibt, die Gap-Query also nie laeuft. Trotzdem die drei Dashboard-Fixtures prod-treu gehaertet (ended_at/duration_seconds), damit die vom Kritiker genannte Latenz-Falle weg ist; plus Anon-Filter in stream_kennzahlen (im erlaubten Bereich, bindet viewer_exclusion_logins), ist_anonymer_login case-insensitiv, Log auf debug.
Runde 2: ALLOW, keine merge-blockierende Luecke. Offene NITs bewusst so gelassen: laufende Sessions ohne ended_at fallen aus der Lueckenzaehlung (REQ-05 zaehlt nur bestimmbare Dauer >= 10 min, Fehlalarm bei frischem Stream waere schlimmer); ANONYM_LOGIN_REGEX_SQL bleibt exportiert, die 60 SQL-Stellen schreiben das Muster woertlich (ein zentrales format! ueber alle Makros waere unverhaeltnismaessig invasiv); backfill.sql hat keine untere Zeitgrenze (so in REQ-04 vorgegeben, Sprache aendert sich selten). `SQLX_OFFLINE=1 cargo check --workspace --all-targets` = exit 0 (alle query!-Makros aller Test-Targets finden ihren Cache, die 5 entfernten Orphans waren tot).

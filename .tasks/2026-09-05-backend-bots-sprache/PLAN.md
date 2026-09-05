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


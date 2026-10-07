# Integrationsnachweise vor dem Deploy

status: aktiv, 2026-10-08

## Laufender Stand

Lesender Aufruf `/usr/local/bin/deploy-twitch-release --pruefen`, Exit 0. Current und alle vier aktiven Prozesse stammen inzwischen aus `b0bd68248c3accc1771e938e6166c3a122ac154e`, nicht mehr aus dem SHA des Erstbefunds.

| Unit | MainPID | NRestarts |
| --- | --- | --- |
| deadlock-twitch-bot-rust | 3022811 | 0 |
| deadlock-twitch-dashboard-rust | 3022680 | 0 |
| deadlock-twitch-stream-coaching-watch | 3023797 | 0 |
| tb-category-collector | 3023840 | 0 |

Alle exe ohne `(deleted)`. Das ist Herkunft vor dem Deploy, kein Nachweis des Fixes.

## Gezielt bestätigte Daten

Lesende, begrenzte psql-Abfragen über bestehenden lokalen Peer-Zugang zur `twitch_analytics`, jeweils Exit 0. Keine Optionen, Zugangsdaten oder Sessionwerte ausgegeben.

- Clips 124767 und 124768 weiter approved, ohne preview_status, Vorschaupfad, Fehler oder TikTok-Optionen. 124768 uploaded_youtube=true, uploaded_tiktok=false.
- Auftrag 5: YouTube completed, scheduled_at `2026-10-07T16:00:00Z`, completed_at `2026-10-07T16:02:01.465486Z`, attempts=0.
- Auftrag 6: TikTok failed, scheduled_at `2026-10-07T16:00:00Z`, attempts=1, keine TikTok-Optionen.
- Aufträge 7 und 8: YouTube und TikTok pending, scheduled_at `2026-10-08T16:00:00Z`, attempts=0.
- earlysalty: 26 pending und vier failed TikTok-Aufträge, jeweils alle ohne Optionen.

## Isolation und Frontend

Eigener PostgreSQL-16-Testcluster `/tmp/tb-tiktok-test-pg-5d7b1734`, loopback-only Port 19329, kurzlebig, ohne Passwort. Test-DSN `postgresql://nathanael@127.0.0.1:19329/postgres`. Worker P und Q verwenden eigene Testschemata. Keine Produktionstests und keine Schemaänderung am Produktivcluster. Teil-Orchestrator stoppt den Cluster nach den Checks.

`npm ci` in bot/admin_dashboard und website jeweils Exit 0. `npm run build --prefix bot/admin_dashboard` und `npm run build --prefix website` jeweils Exit 0. Unveränderte Warnungen: Vite configLoader/__dirname im Admin-Frontend und React-Listenschlüssel im Website-Prerender. Keine Quellen geändert. Dashboard-V2-Prüfungen gehören zu P/Q und werden separat integriert.

## Native Prüfergebnisse

P und Q melden ihren Schreibbereich am 8. Oktober 2026 eingefroren. Keine laufenden Compilerprozesse, keine Worker-Commits oder Deploys.

- P: `cargo-slot check -p tb-social-media -p tb-dashboard-api --jobs 3 --offline`, Exit 0. Drei echte Vorschau-/Datei-/DB-Tests mit `cargo-slot test -p tb-social-media preview::tests --jobs 3 --offline -- --include-ignored --test-threads=1`, Exit 0 auf Port 19329. 39 bestehende Frontend-Vertragstests, Exit 0. TypeScript, eigener ESLint, UI-Detektor und Frontend-Build jeweils Exit 0. Neues gebautes Asset: `bot/analytics/dashboard_v2/dist/assets/index-DnHVvsIu.js`.
- Q: `cargo-slot test --manifest-path rust/Cargo.toml -p tb-social-media --jobs 3 tiktok_ -- --include-ignored`, 27 bestanden, Exit 0. Dashboard-Queue-JSON-Test, ein bestanden, Exit 0. Beide Läufe mit SQLX_OFFLINE=true, TB_TEST_REQUIRE_DB=1 und eigener Wegwerf-DB. Bestehende UI-Queue-/Vertragstests: 28 bestanden, Exit 0; direkte QueueStage-/Slots-Funktionsprobe: ein bestanden. Finale Typprüfung und Whitespaceprüfung Exit 0.
- Clippy mit `-D warnings`: Exit 101, ein Fehler in unverändertem `tb-raid/src/signup_denylist.rs:71` für Result<_,()>. Separater Lauf nur für unverändertes tb-raid: derselbe eine Fehler, Exit 101. Normaler Clippy-Lauf Exit 0, 27 Diagnosen, keine in Ps Rust-Dateien. Keine fremden Fehler geändert.
- Rekursives rustfmt hatte einen Formatblock in social_media_vod_archive_tests.rs verändert. Q nahm exakt diesen eigenen Block zurück. Dateidiff anschließend leer, Exit 0. Gezielte abschließende Formatchecks müssen skip_children=true verwenden. Keine Archivänderung im Fix.

TESTNACHWEIS[TW-1]: P 42 passed, Q 57 passed, jeweils 0 ignored | Baseline: 1 rot. Mehrfach geprüfte UI-Tests sind in diesen Paketwerten enthalten und werden nicht als eindeutige Gesamtsumme addiert.

## Aktualisierung von main

Frischer Fetch: origin/main `0ecae1370f1a80d1a101249b5c932663d69be8af`. Seit Ausgangs-SHA nur fremde Aktenbelege des Archivauftrags hinzugekommen, keine Produktivdateien. Übernahme nach Abschluss der Bauworker. Keine Änderung am geteilten Checkout.

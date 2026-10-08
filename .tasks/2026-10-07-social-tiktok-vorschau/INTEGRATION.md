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

## F2: bestehende Prüfungen nachgezogen

Commit `7ea50d9c856a7472583da65d886ab4bfdf73358b`, drei Rust-Dateien. Abschließende Befehle im eigenen Worktree, echte Wegwerf-DB:

```bash
cd /home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007/rust && SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgresql://nathanael@127.0.0.1:19329/postgres TB_TEST_REQUIRE_DB=1 /home/nathanael/.local/bin/cargo-slot test --jobs 3 --target-dir /home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007/rust/target -p tb-social-media -- --include-ignored > /tmp/tb-tiktok-f2-social-validated.log 2>&1
cd /home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007/rust && SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgresql://nathanael@127.0.0.1:19329/postgres TB_TEST_REQUIRE_DB=1 /home/nathanael/.local/bin/cargo-slot test --jobs 3 --target-dir /home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007/rust/target -p tb-dashboard-api handlers::social_media::tests -- --include-ignored > /tmp/tb-tiktok-f2-dashboard.log 2>&1
```

Erster Lauf: 345 bestanden, null fehlgeschlagen, null ignoriert, null gefiltert, Exit 0. Zweiter Lauf: 49 bestanden, null fehlgeschlagen, null ignoriert, 1317 gefiltert, Exit 0. Rohlogs `/tmp/tb-tiktok-f2-social-validated.log` und `/tmp/tb-tiktok-f2-dashboard.log`. Formatprüfung mit skip_children=true und Diffcheck jeweils Exit 0. testing ist in diesen Paketen keine angebotene Cargo-Eigenschaft; der Versuch mit diesem Flag hatte keine Testausführung und wird nicht als Testlauf gezählt.

Gemessene Dashboard-Baseline a99b2bcf: gezielt null bestanden, ein fehlgeschlagen, 1340 gefiltert; ganze Handler-Suite 48 bestanden, ein fehlgeschlagen, 1292 gefiltert, jeweils null ignoriert und Exit 101. Gemessener ursprünglicher main 0ecae137: queue_dedup_und_invalid_platform eins bestanden, null fehlgeschlagen, null ignoriert, 339 gefiltert, Exit 0. Das Terminverhalten war durch diesen Auftrag geändert worden, seine bestehende Prüfung wurde entsprechend erhalten und angepasst. Der Dashboard-Test übergab unvollständige Auswahloptionen, keine Vorschau-Fixture; Optionen jetzt vollständig, Historienprüfung unverändert.

Die vorangegangenen Social-Läufe mit 342 bestanden/zwei fehlgeschlagen und 343 bestanden/einem fehlgeschlagen zeigten echte Sperrkollisionen der Test-Schemas mit identischen Clip-IDs. Getrennte IDs korrigierten dies ohne Serialisierung oder Skip. Diese Zwischenstände sind nicht der Abschlusslauf.

Zentraler Gate für 7ea50d9c: BLOCK wegen UI-Rückkehr zu fehlender Vorschau nach bereits angefordertem Renderauftrag. F3 aktiv, endgültiger gemeinsamer SHA und Browser-Beleg stehen noch aus. Noch kein Release-Build oder Deploy.

TESTNACHWEIS[TW-1]: F2 Social 345 passed, Dashboard 49 passed, jeweils 0 ignored | Baseline: Dashboard a99b2bcf 1 rot, ursprünglicher main Termin-Test 0 rot

Inhaltsanker vor Deploy: `preview_source_changed` fehlt nach tatsächlicher Byteprüfung sowohl in tb-bot als auch tb-dashboard des laufenden Releases b0bd6824. Nach Deploy wird derselbe Anker zusätzlich zur ELF-Herkunft am neuen Artefakt geprüft. Das Alter einer Datei dient nicht als Herkunftsnachweis.

## F3 und F4: endgültige Produktivquellen

F3 f1275b27: Social-Suite seriell mit echter Wegwerf-DB, 345 Unit-Tests und ein Doc-Test bestanden, null fehlgeschlagen oder ignoriert. Der erste parallele Lauf hatte tatsächlich einen PoolTimedOut bei 344 bestandenen Tests. Dashboard: 49 bestanden, null fehlgeschlagen oder ignoriert; 1292 Unit- und 25 Integrationstests gefiltert. Rohlogs `/tmp/tb-tiktok-f3-social-tests-serial.log`, `/tmp/tb-tiktok-f3-dashboard-tests-final.log`. TypeScript und gezielter Dialog-/Dictionary-ESLint Exit 0. Vier gezielte Frontend-Dateien: 48 bestanden, null fehlgeschlagen oder übersprungen.

F4 b9e284c5: ursprüngliche npm-Baseline 0ecae137 und finaler Lauf jeweils 439 bestanden, fünf fehlgeschlagen, null übersprungen. Es sind über mehrere Testläufe aggregierte Zahlen, keine eindeutige Gesamtzahl. Die konkreten Farbfundstellen sind identisch. SocialMedia-Purity: zwei finale Befunde gegenüber einem ursprünglichen Befund; der zusätzliche Date.now-Aufruf ist als neuer NIT belegt. Nachweise: browser/baseline-results.json, `/tmp/tb-tiktok-f4-npm-original.log`, `/tmp/tb-tiktok-f4-npm-final.log` sowie beide ESLint-JSONs.

Moli F4: 14 synthetische Beobachtungen plus eine statische Abbruchverdrahtungsprüfung. Consent vor Video-/Kontowechsel tatsächlich gesetzt, danach gelöscht. SHA256 der echten Komponente `887c1cbc93cf05a468196900b2473d490ffaa846249ab4ccad70cff0fcc2f6f2`. Kein Medienplayback, keine echte Zustimmung und kein TikTok-Aufruf. Nachweise browser/results-compact.json und PNGs. Gesicherter Gate für b9: ALLOW. Unabhängige Abschlussabnahme I2: Intent bereit ja, notwendiger Fix nein.

Beim Wiederanlauf erneut `/usr/local/bin/deploy-twitch-release --pruefen`, Exit 0. Release b0bd6824 und dieselben vier PIDs wie oben bestätigt. Kein Deploy aus dem Serverneustart abgeleitet. Aktueller origin/main 6937e4a6 enthält ausschließlich drei neue Vollreview-Akten; in den eigenen TikTok-Worktree integriert. Geteilter Checkout unangetastet.

TESTNACHWEIS[TW-1]: Social 346 passed, Dashboard 49 passed, jeweils 0 ignored | Baseline: ursprüngliches npm 5 rot

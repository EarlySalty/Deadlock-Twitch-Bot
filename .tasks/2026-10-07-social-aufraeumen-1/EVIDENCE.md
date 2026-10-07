# Nachweise

## Stand

Implementierung `6dedb9a4`, Downloadkorrektur `2e38bc03`, Integration `2347772c` gegen `origin/main` `0452e03c`. Der Gate meldet ALLOW. Merge, Release und Live-Abnahme sind noch nicht ausgeführt.

## Tests und Werkzeuge

Die endgültige Social-Suite nach der Integration hat 337 bestandene Tests, 0 fehlgeschlagene und 0 ignorierte. Davon sind 336 Bibliothekstests und ein Integrationstest. `--include-ignored`, `TB_TEST_REQUIRE_DB=1` und die private Test-DSN waren gesetzt. Der neue Uploadtest führte yt-dlp, FFmpeg, HTTP und PostgreSQL tatsächlich aus. Cachetreffer und fehlgeschlagene DB-Registrierung wurden ebenfalls geprüft.

Der erste eigene Lauf hatte 318 bestandene Tests und einen Fehler im neuen echten Downloadtest. yt-dlp entfernte den reservierten Suffix `.part`. Nach der Korrektur auf `.tmp.mp4` ist dieser Test grün. Das ist keine Behauptung eines vorbestehenden Fehlers.

Der finale serialisierte Prüfauftrag hat Exit 0. Die API-Suite hat 58 bestandene Tests, 0 Fehler und 0 ignorierte; der Filter `social_media` ließ 1297 fachfremde Tests aus. Die Adminsuite hat 15 bestandene Tests und 0 übersprungene. Insgesamt: 410 bestanden, 0 fehlgeschlagen, 0 ignoriert. Clippy auf beiden betroffenen Crates mit `--all-targets -j 2` war erfolgreich und meldete 39 Warnungsüberschriften inklusive Zusammenfassungen. Ohne Baselinevergleich werden diese nicht als Altfehler bezeichnet. Adminbuild, Dashboardbuild und der für das Release benötigte Websitebuild sind erfolgreich.

```sh
export PATH=/home/nathanael/.cargo/bin:$PATH
export SQLX_OFFLINE=true
export TB_TEST_DATABASE_URL='postgresql:///postgres?host=/tmp/tb-social-aufraeumen-1-pg&user=tb_test'
export TB_TEST_REQUIRE_DB=1
cargo test --manifest-path /home/nathanael/.worktrees/fix-social-aufraeumen-1/rust/Cargo.toml -p tb-social-media -j 2 -- --include-ignored
cargo test --manifest-path /home/nathanael/.worktrees/fix-social-aufraeumen-1/rust/Cargo.toml -p tb-dashboard-api social_media -j 2 -- --include-ignored
cargo clippy --manifest-path /home/nathanael/.worktrees/fix-social-aufraeumen-1/rust/Cargo.toml -p tb-social-media -p tb-dashboard-api --all-targets -j 2
npm --prefix /home/nathanael/.worktrees/fix-social-aufraeumen-1/bot/admin_dashboard test
npm --prefix /home/nathanael/.worktrees/fix-social-aufraeumen-1/bot/admin_dashboard run build
npm --prefix /home/nathanael/.worktrees/fix-social-aufraeumen-1/bot/dashboard_v2 run build
npm --prefix /home/nathanael/.worktrees/fix-social-aufraeumen-1/website run build
```

TESTNACHWEIS[TW-1]: 410 passed, 0 ignored | Baseline: nicht gemessen, keine Altfehlerbehauptung

`rustfmt --check --edition 2021 --config skip_children=true` auf sämtlichen neun geänderten Rust-Quelldateien: Exit 0 nach der Integration. `git diff --check origin/main`: Exit 0.

## Browserprobe

`visual.mjs` prüft die echte Detailseite mit lokalen API-Fixtures. Ladezustand und fehlende Twitch-ID sperren den Schalter. Ein umbenannter Login mit unveränderter Twitch-ID behält den Freigabestatus. Der Klick aus der Detailseite sendet `{ "twitch_user_id": "123456789", "granted": false }`. Die DOM-Breite entspricht in allen drei Zuständen den 1440 Pixeln des Viewports. Bilder: [Laden](loading.png), [fehlende ID](missing-id.png), [bereit](ready.png). Kein öffentlicher Post und keine Änderung einer echten Freigabe wurden ausgelöst.

## SQLx und Schema

Keine eigene Schemaänderung, keine neue eigene Migration und kein eigenes Snapshotdelta. Der tote Upload-Querycache wurde entfernt; der Cache der gemeinsam verwendeten Registrierung bleibt erhalten. Die inzwischen upstream integrierte Verbindungsmigration, ihre Caches und ihr Schema-Snapshot wurden unverändert übernommen. Die Rustprüfungen laufen mit `SQLX_OFFLINE=true`.

## Wirkung und Zwillingssuche

1. Admin-PUT: Detailseite, Hook und Client verwenden dieselbe Twitch-ID. Der Browser zeigt das Ergebnis, und HTTP-Fehler werden als fehlgeschlagene Aktion behandelt. Der Vertragstest prüft Freigabe und Entzug sowie den CSRF-Header.
2. Download: Prep, Vorschau und Upload rufen `download_atomic` auf. Der Upload verwendet zusätzlich `register_local_file`, propagiert DB-Fehler und loggt `atomic_clip_download_ready`. Die Tests prüfen vollständige Bytes, eindeutige konkurrierende Temporärpfade, entfernte Teil-Dateien und erneute Registrierung eines Cachetreffers.
3. Fetch: `StreamerFetchResult.error` wird als HTTP 502 mit `success: false` und erhaltenem Kontingent weitergegeben. Null Clips ohne Fehler bleiben Erfolg. Die Antworttests prüfen beide Fälle.

Zwillingssuche per Graphify und anschließend gezielter Quellsuche belegt die gemeinsamen Downloadaufrufer und die gemeinsam berechneten Renderpfade. Retention verwendet konkrete DB-zugeordnete Dateipfade, Clipfrist und Clipzustand. Sie durchsucht keine Verzeichnisse nach Dateialter. Ein Dateifehler wird mit Clip-ID und Pfad geloggt; die Zeile bleibt für den nächsten Lauf erhalten.

WIRKUNGSPRUEFUNG[WP-1]: 0 offene Befunde im Auftragsdiff | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 3/3 geprüft

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: geänderte Architekturtexte und neue Aufgabenprosa

## Live-Grenzen

Noch kein Deploy. Die vorbereitende Probe des laufenden Dashboards liefert `authenticated: false` und keine Adminsitzung. Eine echte Freigabe wurde deshalb nicht geändert. Lesen von `/proc/1878256/exe` und `/proc/1875039/exe` ist für diese Sitzung mit EACCES gesperrt. Die vorgeschriebenen Scoped-Wrapper werden nicht durch generisches sudo ersetzt. Beide Grenzen müssen bei der späteren Live-Abnahme ausdrücklich erhalten bleiben; die lokalen Fixtures sind kein Ersatz.

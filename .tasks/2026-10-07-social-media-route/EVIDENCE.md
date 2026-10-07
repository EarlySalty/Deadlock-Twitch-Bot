# Prüfung

## Vorher, 2026-10-07

Live-Release: 85d8ec6307f0ce91b801bc228769fe3e3e242629. Bot-PID 3338181, Dashboard-PID 3338314.

- `/social-media`: 303 auf `/twitch/auth/login?next=%2Fsocial-media`.
- `/social-media-admin/xyz`: 303 auf `/twitch/auth/login?next=%2Fanalyse`.
- Terms: 200, text/html; SHA256 `17726662f9995f2111d841e21803fb1cee7e1174a49ee86da879dd65e905518b`.
- Privacy: 200, text/html; SHA256 `4f0a94eb17b9e359eb212a9b295baa895d14b62e4e06045aefbc3b02415b14cf`.
- `/social-media/oauth/callback/youtube?error=access_denied`: 302 auf `/social-media?oauth_error=provider_error`.
- OAuth-Start und API-Stats ohne Session: jeweils 401, application/json.

## Umsetzung und Integration

Auf cc20a312 rebasiert. Archivmodul, Archiv-Einstieg und Archivübersetzungen aus dem parallelen Auftrag erhalten. Doppelte Übersetzung beim Rebase bereinigt.

Rustfmt-Prüfung der sechs angefassten Rust-Dateien erfolgreich. Keine neuen Code-Kommentare.

## Prüfungen

- Originale Dashboard-Suite: 432 bestanden, 5 fehlgeschlagen, 0 übersprungen. Unveränderter Ausgangscommit 85d8ec63: dieselben Zahlen und dieselben fünf Fehler. Betroffen: drei Farbpalettenprüfungen, Gesamtbreite der Analyse-Shell und OBS-Hilfe.
- Dashboard nach Rebase: 434 bestanden, 5 fehlgeschlagen, 0 übersprungen. Baseline cc20a312: 433 bestanden, dieselben 5 fehlgeschlagen, 0 übersprungen. Ein neuer Routingtest erklärt den zusätzlichen bestandenen Test.
- Social-Studio-Browserprüfung vor und nach Rebase: 26 bestanden, 0 fehlgeschlagen, 0 übersprungen. Der zusätzliche Admin-Modus-Test des kombinierten Laufs startet nicht: `spawn geckodriver ENOENT`. Gesamtbrowserlauf deshalb 26 bestanden, 1 fehlgeschlagen. Screenshots: `/home/nathanael/.claude/sichtpruefung/social-media-route/`. Desktop und Mobil zeigen den neuen Namen. Browserprüfung umfasst Breiten von 320 bis 2560 Pixeln.
- Produktionsrouter mit gebautem Bundle: 1 bestanden, 0 fehlgeschlagen, 0 ignoriert. Root, Unterpfad, GET/POST-308 mit kodiertem Pfad und Query, Rechtstexte, OAuth, API, reservierte 404 und authentifizierte SPA geprüft. Wiederholung nach Rebase ebenfalls 1 bestanden, 0 fehlgeschlagen, 0 ignoriert. Dashboard-Build nach Rebase erfolgreich.
- Bot-Suite: 314 bestanden, 8 fehlgeschlagen, 0 ignoriert. Pristine 85d8ec63: dieselben 314 bestanden und dieselben acht Fehler. Sieben Patch-Feed-Tests scheitern an fehlendem `public.llm_usage`; zusätzlich scheitert der Silentban-Test.
- API-Suite: 1314 bestanden, 22 fehlgeschlagen, 0 ignoriert. Vergleich auf pristine 85d8ec63 läuft. Dies ist kein grüner Gesamtlauf.
- Striktes Clippy und pristine Baseline scheitern identisch bei `tb-raid/src/signup_denylist.rs:71` mit `result_unit_err`. Zusätzliche direkte Paketprüfung und Baseline: je 21 API-Lib- und acht Bot-Test-Lints. Clippy ist nicht grün.
- Caddy-Kandidat als Serviceuser validiert. Isolierter Caddy auf Port 4186 mit eigenem Admin-Port 20196: vier 308-Fälle bestanden, einschließlich kodiertem Unterpfad, Query und erneutem Altpräfix im Unterpfad. `route` erzwingt Rewrite vor Redirect.
- Caddy-Gate: `[gpt-6.1-sol] ALLOW: The redirect preserves path suffixes and query strings, and explicitly orders the rewrite before the redirect. No blocking defect found in the supplied diff.` Commit bf73126 nach master gepusht. Live-Caddy noch unverändert.

## Prüfkommandos

Rust-Kommandos verwenden `/home/nathanael/.cargo/bin/cargo`, jeweils `-j 2` und bei Tests `-- --include-ignored`. Volle Ausgaben liegen lokal in diesem Task-Ordner.

Testzugänge betreffen ausschließlich Wegwerfcontainer. Änderungsstand: `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33082/postgres`. Baseline: derselbe Wegwerfzugang auf Port 33083. Routerprüfung zusätzlich mit `DASHBOARD_V2_DIST_PATH=/home/nathanael/.worktrees/feat-social-media-route/bot/analytics/dashboard_v2/dist`.

- `cargo test --manifest-path <Worktree>/rust/Cargo.toml -p tb-bot -j 2 -- --include-ignored`
- `cargo test --manifest-path <Worktree>/rust/Cargo.toml -p tb-dashboard-api -j 2 -- --include-ignored`
- `cargo test --manifest-path <Worktree>/rust/Cargo.toml -p tb-dashboard-api --test social_media_routes -j 2 -- --include-ignored`
- `cargo clippy --manifest-path <Worktree>/rust/Cargo.toml -p tb-dashboard-api -p tb-bot --all-targets -j 2 -- -D warnings`; zusätzlicher Vergleich mit `--no-deps`.
- `npm --prefix <Worktree>/bot/dashboard_v2 test`, `npm --prefix <Worktree>/bot/dashboard_v2 run build`, `node --test tests/socialStudio.browser.test.mjs` im Dashboard-Verzeichnis.

## Ausstehend

Bot-Gate, main-Push, Release-Build und Deploy, minimaler Live-Caddy-Abgleich mit Reload, Live-Beweis und Aufräumen.

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
- API-Suite: 1314 bestanden, 22 fehlgeschlagen, 0 ignoriert. Pristine 85d8ec63: dieselben 1314 bestanden, dieselben 22 fehlgeschlagen, 0 ignoriert. Dies ist kein grüner Gesamtlauf.
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

## Release

Bot-Änderung nach Gate-ALLOW nach main gepusht: 10dacbc2376a63f6d91869afe83b1ac8bb615eec. Vor dem Deploy frisch gefetchtes origin/main ist identisch.

Eigenständiger sauberer Release-Clone unter `/home/nathanael/.worktrees/twitch-release-social-media-route`, internes `.git`, keine übernommenen Binärdateien. Rust-Releasebuild erfolgreich in 24 Minuten 14 Sekunden, `-j 2`. Alle acht benötigten Binärdateien melden mit `--build-revision` exakt den genannten SHA ohne `-dirty`. Der neue Anker `Social-Media-Manager` ist im Bot-Binary enthalten.

Alle drei Release-Frontendbuilds erfolgreich. Gebautes Dashboard enthält den neuen Namen und `href:/social-media`. Index-SHA256: `a7e71950e5fb8f67307044203fc5a9713f2a309fc5163a48844633bdd3ac7172`.

Nach Rebase auch die beiden neuen SPA-Unit-Tests einzeln erfolgreich: jeweils 1 bestanden, 0 fehlgeschlagen, 0 ignoriert, 1339 ausgefiltert. Striktes Clippy nach Rebase bleibt beim bereits gemessenen `result_unit_err` rot.

## Live, 2026-10-07

`deploy-twitch-release 10dacbc2376a63f6d91869afe83b1ac8bb615eec /home/nathanael/.worktrees/twitch-release-social-media-route`: Exit 0. Der Wrapper aktivierte den Release, prüfte Migration und Rollenrechte und startete Dashboard, Bot, Coaching-Watch und Kategorie-Collector neu. Current zeigt auf `releases/10dacbc2376a63f6d91869afe83b1ac8bb615eec`. Die acht installierten ELF-Herkunftsmarken stimmen mit dem SHA überein.

PID nach dem eigenen Deploy: Bot 3795772, Dashboard 3795639, Audit 3796687, Collector 3796733. Spätere eigene Probe: Bot 3816589, Dashboard 3818143, Audit 3821592, Collector unverändert. Öffentliche Prozesskommandos zeigen jeweils das richtige Release-Binary, keine bash-Wrapper. NRestarts jeweils 0, Result success. Es wurde nicht mit anderen Sessions koordiniert.

Fehlerjournal seit 2026-10-07 12:06:59 für die vier Dienste und Caddy: 0 Bytes. Filter: `journalctl -p err`, keine Suche nach Fehlerworten.

Caddy minimal aus dem Live-Stand abgeleitet, vor Installation als Serviceuser validiert, nach `/etc/caddy/Caddyfile` installiert und über Admin-Port 2019 neu geladen. Bestehende fremde Routen erhalten. Die laufende Admin-Konfiguration enthält `^/social-media-admin` als Rewrite-Ausdruck.

Elf öffentliche Live-Prüfungen bestanden:

- Manager-Root und Unterpfad: 303 zum Login mit passendem Rückkehrziel.
- Alter Unterpfad sowie kodierter Unterpfad mit Query: 308 auf den entsprechenden neuen Pfad.
- Terms und Privacy: HTML 200 und exakt dieselben SHA256 wie zuvor.
- OAuth-Fehlercallback: unverändert 302 mit `oauth_error=provider_error`.
- OAuth-Start und API-Stats: unverändert JSON 401 mit identischem Body-SHA256.
- Unbekannter API-Pfad: 404, keine SPA.
- Öffentlich ausgeliefertes Release-JS: JavaScript 200, neuer Titel und kanonischer Navigationslink enthalten. SHA256 `11bf83089343126287be57bde2fc96345c258d2188fe916a1d68074dab2bc775`.

## Abschlussfreigabe

Unprivilegierte `readlink /proc/<PID>/exe`-Aufrufe der Worker-Sitzung schlugen mit Exit 1 fehl. Die Sitzung änderte dafür keine Rechte und nutzte kein generisches sudo.

Der Auftraggeber hat am 2026-10-07 den Ersatznachweis ausdrücklich für diesen Auftrag freigegeben und den `/proc`-Abgleich selbst vorgenommen: Bot PID 3816589, Dashboard PID 3818143, Audit PID 3821592, Collector PID 3796733. Alle vier exe-Links zeigen nach dessen eigener Prüfung auf `/opt/deadlock/twitch/releases/10dacbc2376a63f6d91869afe83b1ac8bb615eec/rust/target/release/`, ohne `(deleted)`. Current zeigt auf denselben SHA. Dieser Teilnachweis wird als freigegebener Nachweis des Auftraggebers übernommen, nicht als eigene privilegierte Messung ausgegeben.

Kein erneuter Deploy erforderlich oder beauftragt. Verbleibend: reine Nachweiscommits nach main bringen, Branch-Abstammung prüfen, eigenen Bot-Branch samt Remote-Branch und Worktree entfernen und selbst settlen. Caddy-Branch und -Worktree, beide Baseline-Worktrees und Testcontainer sind bereits entfernt; Docker-Volumes erhalten. Test-Caddy ausschließlich über eigenen Admin-Port 20196 gestoppt.

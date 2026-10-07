# Prüfung

## Vorher, 2026-10-07

Live-Release: 85d8ec6307f0ce91b801bc228769fe3e3e242629. Bot-PID 3338181, Dashboard-PID 3338314.

- `/social-media`: 303 auf `/twitch/auth/login?next=%2Fsocial-media`.
- `/social-media-admin/xyz`: 303 auf `/twitch/auth/login?next=%2Fanalyse`.
- Terms: 200, text/html; SHA256 `17726662f9995f2111d841e21803fb1cee7e1174a49ee86da879dd65e905518b`.
- Privacy: 200, text/html; SHA256 `4f0a94eb17b9e359eb212a9b295baa895d14b62e4e06045aefbc3b02415b14cf`.
- `/social-media/oauth/callback/youtube?error=access_denied`: 302 auf `/social-media?oauth_error=provider_error`.
- OAuth-Start und API-Stats ohne Session: jeweils 401, application/json.

## Bisherige Prüfung

`npm test`: 432 bestanden, 5 fehlgeschlagen, 0 übersprungen. Unveränderter Baseline-Worktree 85d8ec63: dieselben 432 bestanden, dieselben 5 fehlgeschlagen. Betroffen: drei Farbpalettenprüfungen, Gesamtbreite der Analyse-Shell und OBS-Hilfe.

Dashboard-Produktionsbuild erfolgreich. Neue Namen im gebauten Bundle und in den Screenshots unter `/home/nathanael/.claude/sichtpruefung/social-media-route/`. Desktop und mobile Ansichten zeigen Social-Media-Manager in Kopf und Navigation.

Isolierter Caddy auf 4186, Admin-Port 20196: vier 308-Fälle bestanden, einschließlich kodiertem Unterpfad, Query und wiederholtem Altpräfix im Unterpfad. `route` verhindert die automatische Sortierung von `redir` vor `uri`.

Striktes clippy meldet `tb-raid/src/signup_denylist.rs:71`, `result_unit_err`; Baseline-Messung läuft. Rust-Tests laufen mit dem isolierten Testcontainer `tb-test-social-media-route`, Port 33082. Das Produktionsbundle wird über DASHBOARD_V2_DIST_PATH eingebunden.

# Twitch-Command-Benutzerziele und Rank-Fallback

> Historischer Zwischenstand des ersten Teilauftrags. Der aktuelle Gesamtstand einschließlich direkter Kontoverknüpfung und Deploy-Blockade steht in `../2026-09-18-player-connect/REPORT.md`.

Stand: 2026-09-17. Auftrag: optionale @user-Ziele für !rank, !watchtime und vergleichbare Spielerstatistiken; Rank-Fallback über Deadlock API und Namensauflösung.

## Ablage und Auslieferungsstatus

- Worktree: /home/nathanael/repos/tb-chat-user-targets-20260917
- Branch: feature/chat-command-user-targets-20260917
- Ausgangspunkt: origin/main, 4a377acf
- Änderungen liegen im Working Tree, noch nicht committed/gepusht oder deployed.
- Keine Änderungen am schmutzigen Haupt-Worktree, keine Datenbankmigration, keine Produktions-DB-Schreibzugriffe, kein Service-Neustart und keine echten Twitch-Chat-Nachrichten.

## Umsetzung

command_target.rs: genau ein Twitch-Login, mit/ohne @, case-insensitive, optionale abschließende Mention-Satzzeichen. Vorhandene Event-IDs/Mention-Fragmente vor Helix; Helix maximal 3 Sekunden. Keine Mutation von Absender oder Kanal.

commands.rs: !watchtime akzeptiert @user; ohne Ziel weiter eigener Zuschauerstand. Analytics bleibt ID-basiert und auf den Aufrufkanal begrenzt. Cooldown bleibt (Kanal-ID, Absender-ID), also nicht durch wechselnde Ziele umgehbar. Die acht Spielerstatistiken rank/wins/winrate/mmr/live/lastmatch/streak/mostplayed und die bestehenden Aliase akzeptieren dieselben Ziele; ohne Ziel weiterhin Streamer. Die Schalter des Aufrufkanals bleiben maßgeblich.

rank_lookup.rs: vorhandene verifizierte Steam-Verknüpfung und GC-Rang zuerst. Bei fehlendem Rang/Bot-Freundschaft oder GC-Ausfall öffentliche Rangabfrage für den bekannten bevorzugten bestätigten Steam-Account. Unbestätigte Links und DB-Fehler lösen keine alternative Personensuche aus. Explizite Ziele ohne Verknüpfung erlauben Steam-Namenssuche: nur ein exakter eindeutiger Treffer darf als ausdrücklich unbestätigter Steam-Namensfund angezeigt werden. Mehrdeutige/ähnliche/abgeschnittene Resultate ergeben Auswahlvorschläge, keine automatische Zuordnung. !rank steam:<Account-ID/SteamID64> erlaubt direkte rein lesende Auswahl.

API-Ränge werden als Stand des letzten erfassten Ranked-Matches markiert, nicht als MMR-Schätzung. Geschützte Konten, fehlende Rangdaten, HTTP-/Schemafehler und API-Limits werden getrennt behandelt. Öffentliche Antworten: Single-flight, begrenzter Cache, 16 HTTP-Requests je Minute pro Engine, Retry-After bei HTTP 429, 3-Sekunden-Request-Timeout. Keine Identitäts-Verknüpfung wird angelegt oder verändert.

Befehls-Katalog sowie technische und Benutzer-Hilfe sind aktualisiert. Bestehender Overlay-Datenpfad bleibt unverändert.

API-Vertrag geprüft: https://api.deadlock-api.com/openapi.json
Verwendete Pfade: /v1/players/steam-search, /v1/players/{account_id}/rank, /v1/assets/ranks.

## Verifikation

Toolchain /opt/deadlock/twitch/toolchains/stable/bin; SQLX_OFFLINE=true.
CARGO_TARGET_DIR=/home/nathanael/repos/twitch-rank-friend-state/rust/target (nur Build-Cache).

1. Feature-/Regressionstestlauf: 39 bestanden, 0 fehlgeschlagen, 0 ignoriert (31 neue Tests plus 8 bestehende Watchtime-Tests).
   cargo test -p tb-chat --lib -j 2 -- command_target::tests rank_lookup::tests commands::tests::target_ commands::tests::watchtime --test-threads=2
   Log: .tasks/chat-user-targets-feature-tests.log
2. cargo check -p tb-bot -j 2: Exit 0.
   Log: .tasks/chat-user-targets-bot-check-final.log
3. cargo clippy -p tb-chat --all-targets --no-deps -j 2: Exit 0. Nur vorhandene Warnungen in title_ai.rs, promos.rs und zuschauer_register.rs; keine Warnungen in den neuen Dateien.
   Log: .tasks/chat-user-targets-clippy-final.log
4. git diff --check: Exit 0.
5. Vollständiger cargo-test-Lauf für tb-chat NICHT als erfolgreich bewertet: Tool-Zeitlimit im Bereich unveränderter Moderationstests mit über 60 Sekunden Laufzeit erreicht. Keine Gesamtsuite-Erfolgsaussage. Log: .tasks/chat-user-targets-full-tests.log.

Neue Datenbanktests verwenden private PostgreSQL-16-Prozesse mit temporären Unix-Sockets. HTTP-Integrationstests verwenden Wiremock; kein Live-End-to-End-Test gegen einen produktiven Twitch-Kanal wurde durchgeführt.

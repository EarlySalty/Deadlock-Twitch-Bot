status: aktiv (2026-09-29)

# Integrationsnachweis Paket D

## Stand

Branch `feat/twitch-patch-integration-20260928` auf `origin/main` `cf3d77085ed350554914b14c3d9981d37b95903a`. Die geprüften Spitzen A `b94eab52`, B `2163de98`, C `c61f168c` und der zum Integrationsbeginn neueste Orchestrationsstand `4766b166` wurden ohne doppelte A-Basis integriert. Das unabhängige Review liegt nach der Selbstprüfung beim Orchestrator.

Der Bot startet den Website-Poll nur mit vollständigem Chat-Runtime-Port und Helix. Er reicht Artikel ohne HTTP-Selfcall an denselben `PatchReceiver` weiter, der auch am geschützten internen Router hängt. Die ChatAPI kommt aus der kanalgeprüften und Timeout-erfassenden Runtime, die Suppression aus deren kombinierter Prüfung. Der Receiver übergibt die unveränderte Beobachtungszeit an den synchronen Guard nach dem App-Token-Abruf und unmittelbar vor dem Twitch-POST. Ablauf führt zu `skipped` mit `event_expired_before_post` ohne POST.

## Verifikation

Alle Befehle liefen im Verzeichnis `rust/` mit `PATH="$HOME/.cargo/bin:$PATH"`, `SQLX_OFFLINE=true` und `CARGO_BUILD_JOBS=2`, soweit Cargo verwendet wurde.

- `cargo check -q -p tb-bot -p tb-internal-api -p tb-chat -p tb-transport-twitch`: erfolgreich.
- `cargo clippy -q -p tb-bot -p tb-internal-api -p tb-chat -p tb-transport-twitch --all-targets --all-features`: erfolgreich, vorhandene Warnungen in anderen Modulen und eine Warnung zur mehrfach eingebundenen PostgreSQL-Testhilfe.
- `cargo test -q -p tb-internal-api -p tb-transport-twitch -- --include-ignored`: 452 bestanden, 0 fehlgeschlagen, 0 ignoriert. Der Test mit angehaltenem OAuth-App-Token-Abruf setzt die Testuhr nach Beginn des Sendeversuchs über `observed_at + 120 Sekunden`. Danach: 0 Twitch-POSTs und terminaler Skip in PostgreSQL.
- `cargo test -q -p tb-bot --bin tb-bot -- --include-ignored --test-threads=2`: 301 bestanden, 1 fehlgeschlagen, 0 ignoriert. Das isolierte Regressionstest-Fehlschlagen `regression_silentban_command_wirkt_in_autoban_pipeline_nach_rename` ist ohne die uncommittierten D-Wiring-Änderungen ebenfalls 0 bestanden, 1 fehlgeschlagen. Erwartet wurden 2 Bestätigungen, erhalten wurde 1. Die D-Integration ist in diesem Lauf grün.
- `cargo test -q -p tb-chat source_only -- --test-threads=2`: 6 bestanden, 0 fehlgeschlagen, 0 ignoriert. Die unselektierte Suite mit 906 Tests wurde nach 480 Sekunden abgebrochen; mehrere fremde Tests waren rot und die Moderationstests gegen einen absichtlich unerreichbaren PostgreSQL-Port hingen auch isoliert länger als 90 Sekunden. Kein grüner Gesamtsuite-Nachweis. Drei ignorierte Live-KI-Tests wurden bewusst nicht mit `--include-ignored` ausgeführt.
- `cargo fmt --all -- --check`: rot, unter anderem in der gegenüber `origin/main` unveränderten Datei `bin/tb-bot/src/ad_manager_wiring.rs`. Deren Inhalt weicht schon auf `origin/main` von rustfmt ab. Kein globales Formatieren. `rustfmt --edition 2021 --check --config skip_children=true` ist für `patch_feed.rs`, `patch_announcement.rs` und `patch_announcement_tests.rs` grün; geänderte Wiring-Blöcke sind rustfmt-konform.
- `git diff --check`: erfolgreich.

TESTNACHWEIS[TW-1]: 759 passed, 0 ignored | Baseline: 1 rot

## PostgreSQL-Ende-zu-Ende-Test

`feed_to_source_only_receiver_uses_isolated_postgres_runtime_role` startet PostgreSQL 16 ohne Produktionsverbindung, migriert `rust/migrations/20260928120000_patch_announcements.sql`, wendet `ops/systemd/twitch-runtime-roles.sql` an und verbindet den Poller sowie Receiver separat als `current_user = twitchbot`. Website und Twitch werden ausschließlich lokal gemockt. Die reale Trigger- und Rollenmatrix wurde geprüft:

1. Ein leerer erster Index startet keinen historischen Cursor. Der erste nichtleere Index markiert ältere IDs historisch und erzeugt keine Empfänger oder Chat-POSTs.
2. Die neue ID 286 speichert Beobachtung, stabile Broadcaster-ID 42 und Stream-ID `s42` über den Datenbanktrigger. Nach geprüfter Artikel-URL sendet die echte Helix-Source-only-HTTP-Implementierung durch ChannelPolicy, TimeoutTracking und kombinierte Suppression einmal an den lokalen Mock.
3. Doppel-Poll sendet nicht erneut. Für ID 287 bleibt der vor Partnerbeitritt gespeicherte Empfängersnapshot unverändert; ein Spielwechsel von 42 verhindert die Zustellung. Die später erstmals publizierte kleinere ID 283 erreicht dagegen den nun berechtigten Partner 43.
4. Die 121 Sekunden alte Pending-ID 288 wird terminal `expired_timeout`, die danach neue ID 289 wird verarbeitet. Ein abgebrochener POST von ID 290 hinterlässt `attempted_at` und wird beim nächsten Poll nicht wiederholt. Ein nicht abrufbarer Artikel 291 erreicht den Receiver nicht.
5. `twitchbot` kann `observed_at` nicht verändern (SQLSTATE 42501); Dashboard und Legacy besitzen die erwarteten eingeschränkten Leserechte.

## Übergabegrenze

Keine Nachricht ging an echte Twitch-Chats. Kein Merge nach main, keine Produktivmigration, kein Release-Build, kein Restart. Vor dem Dienststart ist die Migration auf der echten Twitch-Datenbank anzuwenden und die Rollenmatrix anschließend erneut aufzubauen. Nach unabhängiger Review und lokalem Merge-Gate kann der Orchestrator den aktuellen `origin/main`-Stand über den Deploy-Wrapper ausrollen und read-only prüfen: Website-Index erreichbar, Dienst aktiv, erster vorhandener Index historisch ohne Zustellung, neuer echter Artikel erst bei tatsächlicher Veröffentlichung. Eine reale Zustellung ist vor dem nächsten echten Patch nicht belegbar.

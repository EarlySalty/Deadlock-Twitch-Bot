# Prüfprotokoll: persönliche Discord-Links und Referral-Credits

Arbeitsbasis: `3dbf95f2a83e596c4d331decb123b426068b554b`. Branch: `codex/qualified-discord-invites`. Graphify-Befund, Schnittstellen und fachliche Grenzen stehen in `qualified-invites.md`.

PR: https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/999
Gekoppelte Discord-PR: https://github.com/EarlySalty/Deadlock-Bots/pull/466

## Lokale Prüfungen

Cargo 1.97.1 und SQLx-Offline-Modus. Die Prüfungen verwenden keine Produktionsdatenbank und keine Produktionssecrets.

- `cargo fmt` wurde für tb-chat, tb-transport-discord, tb-bot, tb-internal-api, tb-raid, tb-analytics und tb-domain ausgeführt. Der abschließende explizite `rustfmt --check --edition 2021 --config skip_children=true` über alle 19 geänderten und neuen Rust-Dateien besteht, einschließlich der per `include!` eingebundenen Fixtures.
- `cargo clippy --locked` für dieselben sieben Pakete, `--all-targets -j 2`: erneut erfolgreich, Exit 0. Vorbestehende Warnungen betreffen unter anderem Chat-, Session-Detail-, Analytics- und Test-Harness-Code. Es wird keine warnungsfreie Gesamtsuite behauptet.
- `cargo test --locked -p tb-chat --lib dldc -j 2`: erneut drei Tests bestanden. Sie prüfen echte Twitch-IDs, den Broadcaster-Kanallink, den persönlichen Viewer-Link und den ausschließlich aus einer URL bestehenden Chattext.
- `cargo test --locked -p tb-transport-discord --lib -j 2`: erneut alle 35 Tests bestanden. Enthalten sind persönliche Links, Kanal-Fallbacks, stabile Idempotenzschlüssel, Token-/Loopback-Transport, abgewiesene Antworten und der Schutz vor Tokenweitergabe bei Redirects.
- Im Implementierungslauf bestand `cargo test -p tb-raid --test partner_setup` mit allen 32 Tests auf einer isolierten Postgres-Datenbank, darunter erste Aktivierung, ungültige Claims, Mehrfachaufrufe und atomarer Rollback.
- Die beiden neuen Verifikations-/Referral-Tests bestanden ebenfalls im Implementierungslauf. Die zusätzliche Handler-Fixture enthält jetzt die echte neue Credit-Migration. Der erweiterte CI-Gate prüft beide produktiven Aktivierungswege erneut.
- `rustup run stable rust/scripts/test-fresh-schema.sh`: abschließend Exit 0, ein Test bestanden. Alle echten Migrationen liefen auf einer frischen, nur an Loopback gebundenen Wegwerf-Datenbank; der Vergleich mit `fresh_schema_snapshot.txt` bestand ohne `UPDATE_SCHEMA_SNAPSHOT`. Der Container wurde anschließend entfernt.

## Nachgewiesene Basisfehler

Der vollständige tb-internal-api-Testlauf wurde im Implementierungslauf in einem separaten, unveränderten Detached-Worktree auf `3dbf95f2` mit derselben isolierten Testdatenbank wiederholt: 307 bestanden, vier fehlgeschlagen. Identische Fehler existieren bereits vor diesem Paket:

- `handlers::session_detail::tests::session_mit_allen_feldern_200`: HTTP 400 statt 200.
- `handlers::session_detail::tests::session_ohne_optional_felder_null`: HTTP 400 statt 200.
- `handlers::session_detail::tests::top_chatters_limit_10`: fehlender erwarteter Antwortwert.
- `handlers::streamers::tests::list_returns_200`: HTTP 500 statt 200.

Diese Fehler bleiben als Bestandsbefund offen. Ein grüner Feature- oder PR-Gate ersetzt keine grüne vollständige tb-internal-api-Suite.

## PR-CI

Der ursprüngliche PR-Head `f548bc4bd24d1f2065521446558447eb90d0fa5a` bestand die vorhandenen Rust-SQLx-Gates einschließlich Workspace-Build, Schema-/Cacheprüfung und Auth-/Broker-Regressionen sowie die Frontend-, Scope- und Brain-Fixture-Prüfungen. Rust-Lauf: https://github.com/EarlySalty/Deadlock-Twitch-Bot/actions/runs/36299794926

Der bestehende `schema-gate` führt jetzt zusätzlich die persönlichen Chatlinks, den kompletten Discord-Transport, die Partner-Setup-Suite, das Referral-Zeitfenster und die internen Verifikationsfälle aus. Er verwendet seine bereits vorhandene isolierte PostgreSQL-Serviceinstanz. `TB_TEST_DATABASE_URL` und `TB_TEST_REQUIRE_DB=1` verhindern beim Partner-Setup ein stilles Überspringen der Datenbankprüfungen. Der übergeordnete Required-Gate hängt weiterhin von diesem Schema-Gate ab.

Endgültiger Head-SHA, GitHub-Actions-Links und die Ergebnisse des erweiterten Gates stehen in der PR-Beschreibung. Beide PRs bleiben Draft; das semantische Modellreview ist dadurch übersprungen und bleibt offen. Es wurden weder gemergt noch deployt, keine Produktionsmigration ausgeführt und kein Botdienst neu gestartet.

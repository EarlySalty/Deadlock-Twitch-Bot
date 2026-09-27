# Prüfprotokoll: persönliche Discord-Links und Referral-Credits

Arbeitsbasis: `3dbf95f2`. Branch: `codex/qualified-discord-invites`. Graphify-Befund, Schnittstellen und fachliche Grenzen stehen in `qualified-invites.md`.

## Ausgeführt

- `cargo fmt` für tb-chat, tb-transport-discord, tb-bot, tb-internal-api, tb-raid, tb-analytics und tb-domain. Geänderte und neue Rust-Dateien wurden zusätzlich explizit mit rustfmt geprüft. Unbeteiligte Formatierungsänderungen wurden zurückgenommen.
- `cargo clippy` für dieselben Pakete mit `--all-targets -j 2`: erfolgreich; vorbestehende Warnungen außerhalb der neuen Implementierung bleiben sichtbar.
- `cargo test -p tb-chat --lib dldc`: drei Tests bestanden. Sie prüfen echte Twitch-IDs statt Namens-/Badge-Vermutungen und den ausschließlich aus einer URL bestehenden Chattext.
- `cargo test -p tb-transport-discord --lib`: alle 35 Tests bestanden, einschließlich neuer persönlicher Links, Kanal-Fallbacks, stabiler Idempotenzschlüssel, Token-/Loopback-Transport und abgewiesener Antworten.
- `cargo test -p tb-raid --test partner_setup`: alle 32 Tests mit einer isolierten Postgres-Datenbank bestanden, darunter drei neue Tests für erste Aktivierung, ungültige Claims, Mehrfachaufrufe und atomaren Rollback.
- `cargo test -p tb-db --test fresh_migrations_schema fresh_migrations_match_committed_schema_snapshot` mit dem vorhandenen Snapshot-Aktualisierungsmodus: vollständige frische Migration erfolgreich; der Schema-Snapshot enthält die neue Credit-Tabelle. Ein abschließender Lauf ohne Aktualisierungsmodus folgt vor der Abschlussmeldung.
- Die beiden neuen Verifikations-/Referral-Tests bestanden im vollständigen tb-internal-api-Lauf. Eine zusätzliche bestehende Handler-Fixture wurde anschließend um die echte neue Credit-Migration ergänzt und wird erneut geprüft.

## Nachgewiesene Basisfehler

Der vollständige tb-internal-api-Testlauf wurde in einem separaten, unveränderten Detached-Worktree auf `3dbf95f2` mit derselben isolierten Testdatenbank wiederholt. Ergebnis: 307 bestanden, vier fehlgeschlagen. Identische Fehler existieren bereits vor diesem Paket:

- `handlers::session_detail::tests::session_mit_allen_feldern_200`: HTTP 400 statt 200.
- `handlers::session_detail::tests::session_ohne_optional_felder_null`: HTTP 400 statt 200.
- `handlers::session_detail::tests::top_chatters_limit_10`: fehlender erwarteter Antwortwert.
- `handlers::streamers::tests::list_returns_200`: HTTP 500 statt 200.

Diese Fehler dürfen nicht als grüne Gesamt-Suite dargestellt werden. Die neuen Referral-Aktivierungstests sind davon zu unterscheiden. Weitere Regressionsergebnisse und CI-Befunde werden im Abschluss ergänzt.

## Umgebung und Abschluss

Cargo 1.97.1, SQLx-Offline-Modus. Die Datenbanktests verwenden ausschließlich eine eigene Loopback-Testinstanz; der vorhandene neuere TestPostgres-Harness startet darüber hinaus eigene temporäre Postgres-Prozesse. Keine Produktionsmigration und kein produktiver Dienstneustart wurden ausgeführt.

PR-Link, finaler Head-SHA, GitHub-Actions-Links, gemeinsamer Deadlock-Bots-PR und offene Punkte werden in der PR-Beschreibung vermerkt. Beide PRs bleiben Draft; insbesondere der gekoppelte Discord-Release-Gate darf keinen automatischen Merge auslösen.

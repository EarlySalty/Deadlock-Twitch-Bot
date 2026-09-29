status: erledigt (2026-09-29)

# Integrationsnachweis Paket D

## Stand und Ergebnis

Arbeitsbasis: Branch `feat/twitch-patch-integration-20260928`, Startspitze `22699c2430a0ae3c3476e451890a13341ed23b2b`, `origin/main` `cf3d77085ed350554914b14c3d9981d37b95903a`. Die geprüften Spitzen A `b94eab52`, B `2163de98`, C `c61f168c` und Orchestrationsstand `4766b166` waren bereits integriert.

D1: Der privilegierte interne HTTP-Einstieg ist entfernt. Der Website-Feed reicht geprüfte Artikel direkt an denselben `PatchReceiver` weiter. Der Receiver verlangt eine passende Beobachtung mit Status `pending` und identischem `observed_at`. Das Feed-Ledger nutzt eine wiederholbare `REPEATABLE READ`-Transaktion; `statement_timestamp()` wird beim ersten Snapshot-Read erfasst, sodass Beobachtungszeit und Empfängertrigger denselben PostgreSQL-Snapshot verwenden.

D2: Die Patch-Suppression propagiert Datenbankfehler bis zum Sender. Fehler beenden die Prüfung ohne Chat-POST. Die allgemeine Chat-Policy bleibt unverändert.

D3: Twitch-Streams-Abfragen setzen `first=100` für Batches mit bis zu 100 stabilen Broadcaster-IDs.

D4: Die 120-Sekunden-Frist bleibt bis zum Guard unmittelbar vor dem Twitch-POST erhalten, auch wenn vorher ein App-Token abgerufen wird.

D5: HTTP-200-Antworten mit unlesbarem oder fehlendem Ergebnis behalten Status 200 und eine redigierte Fehlerklasse. Ungewisse Ergebnisse werden nicht wiederholt.

D6: Streamwechsel, Spielwechsel, abgelaufene Events sowie nicht sendbare oder unterdrückte Ziele erhalten terminale Skip-Gründe pro Event und Broadcaster-ID; die Gründe werden protokolliert. Der Ende-zu-Ende-Test erwartet diese gespeicherte Diagnose.

Keine neue Migration. Website und Twitch sind in Tests lokal gemockt. Kein Produktivdatenbankzugriff und keine echte Twitch-Nachricht.

## Verifikation

Alle Cargo-Befehle liefen aus `rust/` mit `PATH="$HOME/.cargo/bin:$PATH" SQLX_OFFLINE=true CARGO_BUILD_JOBS=2`.

- `cargo check -q -p tb-bot -p tb-internal-api -p tb-chat -p tb-transport-twitch --all-targets`: erfolgreich.
- `cargo clippy -q -p tb-bot -p tb-internal-api -p tb-chat -p tb-transport-twitch --all-targets --all-features`: erfolgreich. Verbleibende Warnungen liegen in unveränderten Modulen.
- `cargo test -q -p tb-internal-api -p tb-transport-twitch -- --include-ignored --test-threads=2`: 455 bestanden, 0 fehlgeschlagen, 0 ignoriert. Enthält isolierte PostgreSQL-16-Tests der Suppressions- und Ledgerpfade.
- `cargo test -q -p tb-chat combined_ -- --include-ignored --test-threads=2`: 3 bestanden, 0 fehlgeschlagen, 0 ignoriert, 903 gefiltert.
- `cargo test -q -p tb-chat source_only -- --include-ignored --test-threads=2`: 6 bestanden, 0 fehlgeschlagen, 0 ignoriert, 900 gefiltert.
- `cargo test -q -p tb-bot --bin tb-bot -- --include-ignored --test-threads=2`: 301 bestanden, 1 fehlgeschlagen, 0 ignoriert. Der einzige Fehler `regression_silentban_command_wirkt_in_autoban_pipeline_nach_rename` ist der bereits auf der unveränderten Basis reproduzierte Baseline-Fehler. Der Feed-Ende-zu-Ende-Test und die D6-Skip-Diagnose sind grün.
- `cargo test -q -p tb-bot --bin tb-bot patch_feed::tests::feed_to_source_only_receiver_uses_isolated_postgres_runtime_role -- --include-ignored --test-threads=2`: 1 bestanden, 0 fehlgeschlagen, 0 ignoriert, 301 gefiltert. Isoliertes PostgreSQL 16, echte Patch-Migration und Rollenmatrix, Website und Twitch lokal gemockt.
- `git diff --check`: erfolgreich. Geänderte Bereiche sind rustfmt-konform. `cargo fmt --all -- --check` bleibt wegen Repository-weitem Formatierungsdrift rot; kein globales Formatieren angewendet.

TESTNACHWEIS[TW-1]: 766 passed, 0 ignored | Baseline: 1 rot

## Wirkung und Übergabegrenze

Geprüfte Fremddienst-Pfade: Feed-Index, Artikelabruf, Twitch-OAuth, Twitch-Helix-Streams und Twitch-Chat-POST. Fehler, Timeouts und ungewisse POST-Ergebnisse lösen keinen automatischen zweiten Chat-POST aus. Zwillingssuche der Guard- und Transport-Aufrufe ist per Repo-Suche dokumentiert; der Live-Sendepfad nutzt den einzigen geprüften Source-only-POST.

Kein Merge nach `main`, keine Produktivmigration, kein Deploy, kein Restart und kein echter Twitch-Test. Nach Gate-Prüfung wird ausschließlich der eigene Feature-Branch gepusht.

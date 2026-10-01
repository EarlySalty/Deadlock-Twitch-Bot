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

## Schlussintegration auf `origin/main` vom 29.09.2026

Feature-Spitze vor diesem Nachweis: `bde167c0`; frisches Remote-`origin/main`: `d828481624d53408e0c0a4c3ed1a8e4a6d421c40`, bereits im Feature gemergt, 0 Commits hinter Main. Die finalen Review-Unterlagen wurden mit `7075b094` aus dem Orchestrationszweig übernommen; diese `EVIDENCE.md` blieb erhalten. Die vorhandene Änderung an `rust/bin/tb-bot/src/main.rs` ordnet `mod patch_feed` nach `mod partner_recruit` und gehört zum Feature-Branch.

Alle folgenden Cargo-Läufe erfolgten aus `rust/` mit `PATH=/home/nathanael/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:$HOME/.cargo/bin:$PATH SQLX_OFFLINE=true CARGO_BUILD_JOBS=2`:

| Befehl | Exit | Ergebnis |
| --- | ---: | --- |
| `cargo check -q -p tb-bot -p tb-internal-api -p tb-chat -p tb-transport-twitch --all-targets` | 0 | gemeinsamer Rust-Pfad kompiliert |
| `cargo clippy -q -p tb-bot -p tb-internal-api -p tb-chat -p tb-transport-twitch --all-targets --all-features` | 0 | keine blockierende Diagnose |
| `cargo test -q -p tb-internal-api -p tb-transport-twitch -- --include-ignored --test-threads=2` | 0 | 458 bestanden, 0 fehlgeschlagen, 0 ignoriert |
| `cargo test -q -p tb-chat combined_ -- --include-ignored --test-threads=2` | 0 | 3 bestanden, 0 fehlgeschlagen, 0 ignoriert, 903 im Bibliothekstest gefiltert |
| `cargo test -q -p tb-chat source_only -- --include-ignored --test-threads=2` | 0 | 6 bestanden, 0 fehlgeschlagen, 0 ignoriert, 900 im Bibliothekstest gefiltert |
| `cargo test -q -p tb-bot --bin tb-bot patch_feed::tests::feed_to_source_only_receiver_uses_isolated_postgres_runtime_role -- --include-ignored --test-threads=2` | 0 | 1 bestanden, 0 fehlgeschlagen, 0 ignoriert, 307 gefiltert; PostgreSQL 16 mit echter Migration und Rollenmatrix unter `twitchbot`, HTTP und Twitch lokal gemockt |
| `cargo test -q -p tb-bot --bin tb-bot -- --include-ignored --test-threads=2` | 101 | 307 bestanden, 1 fehlgeschlagen, 0 ignoriert; allein `regression_silentban_command_wirkt_in_autoban_pipeline_nach_rename`, Ist 1 statt Soll 2 |

Für den roten Bot-Test wurde `origin/main` `d8284816` als unveränderter Quellarchiv-Auszug isoliert unter `/tmp` mit derselben Rust-Version, `SQLX_OFFLINE=true`, `CARGO_BUILD_JOBS=2`, Testfilter, `--include-ignored` und `--test-threads=2` geprüft. Der erfolgreiche Baseline-Teststart verwendete `GIT_DIR=/home/nathanael/repos/Deadlock-Twitch-Bot/.git` und `GIT_WORK_TREE=<isolierter Archivpfad>` ausschließlich für das Build-Revision-Skript sowie `CARGO_TARGET_DIR=<Integrations-Worktree>/rust/target`. Ergebnis: Exit 101, 0 bestanden, derselbe eine Test fehlgeschlagen, Ist 1 statt Soll 2, 288 gefiltert. Ein erster Archivlauf ohne Git-Metadaten endete vor dem Test im Build-Revision-Skript mit Exit 101 und zählt nicht als Basismessung. Nach der Basismessung wurde `tb-internal-api` aus dem gemeinsamen Debug-Cache entfernt und der isolierte Feature-Ende-zu-Ende-Fall erneut ausgeführt: Exit 0, 1 bestanden, 0 fehlgeschlagen. Das Archiv wurde gelöscht. Die gültige Gegenprobe belegt diesen einen Bot-Fehler auf aktuellem Main, nicht die Vollsuite auf Main.

Dateiformatprüfung mit Rustfmt 1.97.1 (`--edition 2021 --emit stdout`) über 15 geänderte Rust-Dateien und Zeilenvergleich zu `origin/main`: Exit 0 für eigene Formatabweichungen, 0 neue Abweichungen. Diese sechs Dateien sind bereits auf Main formatabweichend, jeweils außerhalb der Feature-Hunks: `rust/bin/tb-bot/src/chat_wiring.rs`, `rust/bin/tb-bot/src/main.rs`, `rust/crates/tb-chat/src/api.rs`, `rust/crates/tb-chat/src/promos.rs`, `rust/crates/tb-chat/src/timeout_tracking.rs`, `rust/crates/tb-transport-twitch/src/chat.rs`. `cargo fmt --all -- --check`: Exit 1, 258 betroffene Workspace-Dateien; kein globales Umformatieren. Der öffentliche Website-Vertrag wurde am 29.09. nur lesend geprüft: `GET /patchnotes/index.json` HTTP 200, 46 Einträge, neueste ID 285 mit `id`, `posted_at` und kanonischer DE-`url`; `GET /patchnotes/patch-285/meta.json` HTTP 200 mit `source_url`. Die Implementierung fordert die feste HTTPS-Herkunft und passende Einzelmetadaten; kein echter Twitch-POST.

Migrationsdatei des kombinierten Features: `rust/migrations/20260928120000_patch_announcements.sql`; zugehörige Rechte: `ops/systemd/twitch-runtime-roles.sql`. Der isolierte PostgreSQL-16-Fall führte beides aus und prüfte `current_user=twitchbot`; `twitchbot` hat begrenzte Schreibrechte, `twitchdash` Leserechte und `twitchlegacy` keine neuen Rechte. Auf Produktion angewandt: **nein**. Der E1-Einwand einer möglicherweise während einer längeren Feed-Lücke verpassten frischen Veröffentlichung ist in `REVIEW.md` als bekannter False Negative dokumentiert. Mangels belastbarer Website-Publikationszeit bleibt der Sendepfad zum Schutz vor historischem Replay geschlossen.

TESTNACHWEIS[TW-1]: 774 passed, 0 ignored | Baseline: 1 rot

Selbstprüfung: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/twitch-patch-integration-20260928 --base main --head feat/twitch-patch-integration-20260928`: Exit 0, **ALLOW**, kein Merge-blockierender Defekt. Der einzige NIT betrifft die in `REVIEW.md` entschiedene E1-Ausfallgrenze: Ein kurz vor Wiederkehr des Feeds veröffentlichter Patch kann nach 120 Sekunden Lücke verpasst werden. Keine heuristische Öffnung des Chat-Sendepfads.

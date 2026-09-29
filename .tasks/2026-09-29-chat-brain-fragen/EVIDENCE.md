# Nachweis: Chat-Fragen an das Brain

status: aktiv (2026-09-29)

## Verdrahtung

Die Chat-Pipeline prüft Partnerkanal, Moderation und Spam vor der Bot-Erwähnung. Antworten gehen als Twitch-Reply durch Kanal-Policy und Timeout-Tracking. Ohne `bot.brain_client.mode = "typed"` bleibt der neue Pfad aus; der Default ist `legacy`. Der Dienstzugang kommt aus `settings.internal_api.token`. Das Dashboard liest den geschützten Wert `TWITCH_INTERNAL_API_TOKEN` über `uplink_config::brain_service_token()` (`tb-dashboard/src/main.rs:427`, `tb-dashboard-api/src/uplink_config.rs:80`); `tb-config/src/global.rs:371` bindet denselben Schlüsselnamen für den Bot an `settings.internal_api.token`. Ein zusätzlicher Geheimnisname wurde nicht eingeführt. Der Adapter übergibt diesen Wert an `AsyncBrainClient::new_local` (`tb-knowledge/src/brain.rs:39`).

Nachrichten mit `!` am Anfang bleiben beim bestehenden Command-Handler (`tb-chat/src/commands.rs:462`), auch wenn später `@Bot` steht. Bei echten Bot-Fragen ohne Brain-Antwort bleibt der Chat stumm; dafür werden nachgelagerte Antwortmodule übersprungen.

Die Datenbankmigration legt `tb_chat_brain_answers` mit stabilen Twitch-IDs, Frage, Antwort, Status und Dauer an. Quoten werden vor dem Eintrag in einer Datenbanktransaktion über Instanzen und Neustarts hinweg geprüft. Der Bot kann schreiben, das Dashboard nur lesen.

## Prüfungen

- `PATH=/home/nathanael/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:$PATH cargo test --manifest-path rust/Cargo.toml -p tb-bot --bin tb-bot brain_chat_wiring -- --nocapture`: 8 bestanden, 0 fehlgeschlagen, 0 ignoriert, 289 herausgefiltert. Enthält isolierte PostgreSQL-Läufe für Migration, Rechte, doppelte Nachrichten und Quoten nach einem neuen Prozesszustand sowie die Freigabe einer Reservierung ohne Protokolleintrag, die Entfernung internationaler Domains, die Kürzung an Satzgrenzen ohne abgeschnittene Versionsnummer, den Verzicht auf Fragmente ohne Satzgrenze und den Erhalt von Chatbefehlen mit Bot-Erwähnung.
- `cargo test --manifest-path rust/Cargo.toml -p tb-config --lib`: 10 bestanden, 0 fehlgeschlagen, 0 ignoriert, 0 herausgefiltert. Enthält den Legacy-Default und die Typed-Validierung.
- `cargo test -p tb-transport-twitch send_chat_reply`: 1 bestanden, 0 fehlgeschlagen, 0 ignoriert. Prüft die Twitch-Helix-Anfrage mit `reply_parent_message_id`.
- Die vollständige Bot-Binary-Suite ergab 293 bestanden und einen Fehler in `chat_wiring::chat_notification_tests::regression_silentban_command_wirkt_in_autoban_pipeline_nach_rename`. Derselbe Test am unveränderten Basis-Commit `cf3d7708`: 0 bestanden, 1 fehlgeschlagen, 0 ignoriert. Die Umschaltbestätigung erwartet zwei Sendungen, beobachtet eine.
- Ein gemeinsamer Lauf von `tb-config`, `tb-chat`, `tb-transport-twitch` und `tb-knowledge` wurde nach mehr als zehn Minuten mit 33 Fehlern abgebrochen; kein grüner Nachweis. Seine 33 gescheiterten Testnamen wurden auf dem unveränderten `origin/main`-Stand `255dedf5` und auf diesem Feature-Stand einzeln mit `--exact` wiederholt: jeweils 0 bestanden, 33 fehlgeschlagen, 0 ignoriert. Ein Baseline-Test benötigte dabei 30 Sekunden und wurde mit längerer Frist erneut ausgeführt. Der Player-Command-Test wurde zusätzlich auf dem früheren Basis-Commit `cf3d7708` mit demselben `Option::unwrap()`-Fehler in `player_link_command_tests.rs:59` reproduziert. Der gemeinsame Lauf selbst bleibt abgebrochen.
- Geänderte Rust-Dateien bestehen den gezielten `rustfmt --check`. `cargo clippy -p tb-config -p tb-chat -p tb-transport-twitch -p tb-bot --all-targets` läuft mit bereits vorhandenen Warnungen durch. Derselbe Lauf mit `-- -D warnings` bricht bei unveränderten Stellen in `tb-chat/src/scam_pitch.rs:1443` und `title_ai.rs:436` ab.

## Offen

Ein Brain-Antwortdienst ist vor G5 nicht auf main live. Es gab weder Brain-Deploy noch G5-Umschaltung oder Live-Chat-Beweis. Ein eigener Kanal-Schalter für Brain existiert im vorhandenen Einstellungsweg nicht; der globale Schalter und die Partnerkanal-Policy begrenzen den Pfad.

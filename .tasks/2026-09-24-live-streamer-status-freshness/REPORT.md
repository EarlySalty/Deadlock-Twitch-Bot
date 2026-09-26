# Frischer Live-Nachweis für Streamer-Rolleninhaber

Der Discord-Bot prüft die Streamer-Rolle bei jeder Panel-Aktion per Discord REST, ebenso aktuelle Voice-Lane und Owner. Der bestehende Twitch-Diagnose-Endpunkt löst die Discord-ID nur über `twitch_streamer_identities` in die Twitch-User-ID auf. Die genaue Cross-Repo-Grenze, Last und Rollenentzug stehen in [CROSS-REPO-CONTRACT.md](CROSS-REPO-CONTRACT.md).

Für einen verknüpften Rolleninhaber mit frischem positivem Poller-Tick bleibt der vorhandene Live-Nachweis erhalten. Fehlt er, fragt die Diagnose-Route den bereits verwendeten Helix-App-Client gezielt nach `user_id`, unabhängig von Partnerstatus, Sprache oder Spiel. Die Antwort wird nur bei derselben Twitch-ID und einem gültigen Stream samt Login als Live-Nachweis gewertet. Sie erhält einen UTC-Zeitpunkt des erfolgreichen Abrufs. Offline, ungültige Verknüpfung, fehlender Helix-Client, API-Fehler und Timeout bleiben ohne positiven Zeitstempel. Die zusätzliche Last ist auf 1,2 Sekunden pro Aufruf, acht parallele Aufrufe, 60 Starts pro Minute und einen 30-Sekunden-Cache mit 256 IDs begrenzt. Es gibt keinen neuen Poller, keine dauerhafte Rollen-Kopie, keinen zweiten OAuth-Weg, keine Migration und keine neue Konfiguration.

## Lokale Prüfung

- `cargo check -p tb-internal-api --jobs 2`: grün.
- `cargo test -p tb-internal-api --lib handlers::diagnose::tests --jobs 2`: 10 bestanden, 0 fehlgeschlagen. Enthalten sind ID-Bindung, beliebiges Spiel, Frische, Cache und Helix-Budget. Der Helix-Transport selbst hat bereits einen Test für `user_id`-Abfragen im Transport-Crate.
- `rustfmt` auf geänderten Rust-Dateien und `git diff --check`: grün.
- Der neue Analytics-DB-Test `discord_live_nachweis_benutzt_nur_die_verknuepfte_twitch_id` lief gegen einen eigenen Timescale-Wegwerfcontainer: 1 bestanden, 0 fehlgeschlagen, 0 ausgeschlossen. Der Container wurde nach dem Test entfernt. Ein vollständiger Workspace- oder Live-Test wird nicht behauptet.

Der frühere Zwischenstand dieses PR war zu Recht blockiert, weil der Poller nur Partner und ausdrücklich überwachte Konten erfasst. Der begrenzte On-Demand-Helix-Pfad schließt diese Lücke für verknüpfte Rolleninhaber. Unabhängiges Rust-/Security-Review und lokales `gate_hook.py --review` ergaben ALLOW mit nicht blockierenden Hinweisen. Discord muss nach Twitch ausgerollt werden, weil der Consumer ohne `last_seen_at` sicher gesperrt bleibt. Kein Merge oder Deploy aus diesem Branch.

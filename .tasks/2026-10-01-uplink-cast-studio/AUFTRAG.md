status: aktiv | Datum: 2026-10-01

# Uplink Cast Studio, Branch-Abschluss

## Ziel

Den ursprünglichen eigenen Arbeitsstand von `feat/uplink-cast-studio-20260912` auf dem benannten isolierten Luna-Branch prüfen und nur noch nötige Änderungen für denselben Cast-Studio-Umfang übernehmen. Der Cast-Studio-Einstieg muss über den angemeldeten Uplink-Dashboardpfad erreichbar sein.

## Wiederhergestellter Umfang

Der Source-Head `1269ad60c4743e9bf69397f666dd0dbbc7b48dfa` enthält die Cast-Studio-Ansicht, Quellen- und Szenenverwaltung, Preview-/Program-Steuerung und den Rust-Proxy zum Uplink-Relay. Der ursprüngliche Produktauftrag ist in den Commit-Metadaten und vorhandenen `.tasks`-Unterlagen nicht enthalten. Die präzisen Produkt-Akzeptanzkriterien bleiben bis zur unabhängigen Intent-Abnahme offen.

## Befund und Arbeit

- Aktuelles Twitch-`origin/main` ist `14bc1f479e32394fe2977f8c8ef85e6c5bff66e1`; lokales `main` `d828481624d53408e0c0a4c3ed1a8e4a6d421c40` ist veraltet. Source-Commit `1269ad60c4743e9bf69397f666dd0dbbc7b48dfa` ist kein Vorfahr von aktuellem Main. Main besitzt die normalen Uplink-Routen, aber weder `/twitch/api/v2/uplink/cast*` noch Cast-Studio-UI/API-Dateien. Der eigene Featurepfad fehlt weiterhin.
- Read-only Ownership-Abgleich am 2026-10-01: lokaler PR-#1035-Head `671a460afa102afc38fb0acffaeeda04c935dffb` ist Vorfahr des Remote-Heads `03c69ee4990bb33df305cb9b0b3992b9dec0528f`; der Remote-Head liegt 29 Commits voraus. PR #1035 enthält keine Cast-Studio-Implementierung. Gemeinsame Dateipfade sind `rust/crates/tb-dashboard-api/Cargo.toml` und `src/lib.rs`: das Manifest ergänzt `tb-transport-discord`, der Router registriert `/social-media/api/clips/{clip_db_id}/clip-contest`. Cast Studio registriert getrennte `/twitch/api/v2/uplink/cast*`-Routen und `/twitch/uplink/studio`. Keine gemeinsame Route oder Chat-/Dock-/Subscription-Schnittstelle; die Pfadüberschneidung begründet keine Gruppenaufnahme oder Ownership-Übertragung.
- Runtime-Abgleich: `deadlock-twitch-bot-rust.service` und `deadlock-twitch-dashboard-rust.service` laufen auf Release/Commit `f4034597517f2921ad250738b964dd4c712f9448`; dessen Router hat keine Cast-Routen. `rs-relay.service` läuft auf Release-Verzeichnis `6e6ae0d`; diese Kennung ist kein Commit im lokalen `rs-relay`-Git. Der saubere `rs-relay`-Stand `27697ce7bb5e5488053ce1705699d4f1eafc8f61` enthält in `src/api/mod.rs:127-180` keine `/v1/me/cast`-Routen. Die authentifizierte Live-Route wurde nicht abgefragt; die Relay-Kompatibilität bleibt Integrationsblocker. Es wurde kein `rs-relay`-Code geändert.
- Die UI verlinkt `/twitch/uplink/studio`, der Rust-SPA-Router registrierte bisher nur `/twitch/uplink`. Ergänze die verschachtelte Shell-Route mit demselben Session-/Partner-Gate und einen Regressionstest.
- Keine weiteren Produktpfade außerhalb des bestehenden Branchumfangs erweitern.

## Grenzen und Abschlusskriterien

- Produktive Änderungen ausschließlich Rust. Keine Secrets/ENV lesen, keine Source-Worktree-Dateien verändern.
- Keine Merge-auf-main-, Deploy-, Restart-, Datenbank- oder Produktionsschritte während des TokenDB-Live-Holds. Der frühere Host-Resource-Hold ist aufgehoben. Keine schweren Cargo-Läufe, solange ein PR-1035-Cargo-Lock gehalten wird; bei der letzten Prüfung war kein solcher Lockholder sichtbar, nur ein Cargo-Check mit eigenem Brain-Worktree-Target-Verzeichnis. Vor jedem schweren Lauf den Lockstatus erneut prüfen.
- Gate nur mit `gate_hook.py --review --repo /home/nathanael/repos/Deadlock-Twitch-Bot --base origin/main --head codex/luna-dispatch/deadlock-twitch-bot/feat-uplink-cast-studio-20260912-1269ad60`; `ALLOW` nicht umgehen.
- Vor Abschluss unabhängige Intent-Abnahme und lokales Gate auf dem Cast-Studio-Stand organisieren. Keine Gruppenabnahme oder Integration in PR #1035 ohne tatsächliche Vertragsüberschneidung und ausdrückliche Vereinbarung. Keine fremden Branches integrieren.
- Arbeitsstand und Hold-Grenzen in der externen Branchstatusdatei dokumentieren. Abschluss hier erst nach Gate-Urteil und Abnahme; Produktionsabschluss bleibt gesperrt.
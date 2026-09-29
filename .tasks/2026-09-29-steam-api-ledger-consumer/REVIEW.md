status: aktiv | 2026-09-29

# C2: lokales Review-Gate

## Runde 1: BLOCK

1. `rust/crates/tb-engagement/src/steam_web_api.rs:244`: Eine vor dem Steam-Aufruf gespeicherte, noch nicht beantwortete Reservierung blockierte nach einem Neustart dauerhaft. Behoben durch eine bestätigte Beobachtung mit unbekanntem HTTP-Status und eine dauerhaft gespeicherte Pause von mindestens 24 Stunden. Nach Ablauf wird die Datei entfernt und der Abruf kann erneut reserviert werden. Tests decken Neustart, fehlgeschlagene Speicherung und Freigabe nach Ablauf ab.
2. `rust/crates/tb-engagement/src/steam_web_api.rs:9`: Relativer Dateipfad hängt vom Arbeitsverzeichnis ab. Behoben mit dem absoluten Pfad im persistenten systemd-StateDirectory des Twitch-Bots.

Runde 2: ausstehend.

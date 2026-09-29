status: aktiv | 2026-09-29

# C2: lokales Review-Gate

## Runde 1: BLOCK

1. `rust/crates/tb-engagement/src/steam_web_api.rs:244`: Eine vor dem Steam-Aufruf gespeicherte, noch nicht beantwortete Reservierung blockierte nach einem Neustart dauerhaft. Behoben durch eine bestätigte Beobachtung mit unbekanntem HTTP-Status und eine dauerhaft gespeicherte Pause von mindestens 24 Stunden. Nach Ablauf wird die Datei entfernt und der Abruf kann erneut reserviert werden. Tests decken Neustart, fehlgeschlagene Speicherung und Freigabe nach Ablauf ab.
2. `rust/crates/tb-engagement/src/steam_web_api.rs:9`: Relativer Dateipfad hängt vom Arbeitsverzeichnis ab. Behoben mit dem absoluten Pfad im persistenten systemd-StateDirectory des Twitch-Bots.

## Runde 2: BLOCK

1. `rust/crates/tb-engagement/src/steam_web_api.rs:244`: Bei fehlgeschlagener Statusspeicherung wurde der tatsächliche Status trotzdem an den Dienst gesendet. Eine verlorene Antwort darauf konnte nach Neustart zu einem widersprüchlichen Nachtrag und dauerhafter Sperre führen. Behoben: Status zuerst speichern, bei Schreibfehler den bekannten Status im laufenden Prozess halten und später mit derselben Reservierung nachtragen. Nach Neustart ohne gespeicherten Antwortstatus meldet der Bot den unbekannten Status, ohne vorher eine abweichende Beobachtung gesendet zu haben. Die 24-Stunden-Pause bleibt über Neustarts erhalten. Tests decken beide Wiederherstellungswege ab.

Runde 3: ALLOW. Das lokale Gate bestätigt die Wiederherstellung der offenen Reservierung. Unabhängiges Paket-Review und Produktivfreigabe stehen noch aus.

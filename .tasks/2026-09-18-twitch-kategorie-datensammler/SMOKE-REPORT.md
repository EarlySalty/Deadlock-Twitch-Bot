# Unabhängiger Live-Smoke-Test: anonymer Chattransport

Datum: 18.09.2026, Vormittag Europe/Berlin. Getestet wurde ein kompilierter Zwischenstand des neuen tb-monitoring::anon_chat, NICHT ein bereits fertig deployter Collector.

## Testaufbau
- Probe unter diesem Taskordner/probe importiert denselben neuen tb-monitoring-Transport und den bestehenden tb-engagement::irc_message::parse_privmsg.
- Kein eigener IRC-/Helix-Client, keine Zugangsdaten, keine ENV-Konfiguration, kein ChatApi/Send-/Moderations-/Whisper-Aufruf.
- Nur Zähler und Namen bekannter IRC-Tags werden ausgegeben. Keine Nachrichtentexte oder Chatter-Identitäten im Log/Storage.
- Live-Testkanal xdgaratv wurde per read-only Postgres-Abfrage gewählt: twitch_live_state, is_live=1, last_game=Deadlock, Beobachtung jünger als3Minuten. Letzte bestätigte Sichtung vor dem Test: 2026-09-18T09:42:51+00:00, viewer_count9.

Build (Exit0):
`cargo build --manifest-path ../.tasks/2026-09-18-twitch-kategorie-datensammler/probe/Cargo.toml --target-dir target -j2`
Ausgeführt mit /home/nathanael/.cargo/bin/cargo im Feature-Worktree/rust. Build-Job j-1789724396-151, stderr endet mit Finished dev profile in2m38s. Jobprotokoll unter ~/.local/state/codex-mcp/jobs/j-1789724396-151/.

## Ergebnisse
Erster Lauf8Sekunden: 1Protokoll-JOIN, 0Chatnachrichten, 0Drops, 0Reconnects. Bestätigt Verbindung/Handshake, nicht Taglieferung.

Zweiter Lauf30Sekunden (Exit0):
```
duration_seconds=30
protocol_join_writes=1
received_privmsgs=1
dropped_privmsgs=0
reconnects=0
observed_tag_names=["room-id", "user-id", "id", "tmi-sent-ts", "emotes", "badges", "display-name"]
raw_message_storage=false; authenticated_chat=false; outgoing_chat_api=false
```

## Aussage und Grenze
Anonyme justinfan-Aufnahme einschließlich IRCv3-Tags funktioniert in diesem realen Einzelkanaltest. Der Testprozess ist nach jedem Lauf beendet; kein dauerhafter Sammler dadurch gestartet. Ein empfangenes Ereignis belegt weder kategorieweite Vollständigkeit noch90Tage-Retention, zuverlässiges Sharding oder einen fertigen Dashboard-/DB-Pfad. Nach abschließenden Transportkorrekturen und gebautem Collector ist die Abnahme des vollständigen Datenwegs weiterhin erforderlich. Die Rohtexte des Tests wurden nicht gespeichert.

status: erledigt
datum: 2026-08-11

# Research

## Befund

- `tb-monitoring/src/announce/sink.rs` setzt beim 5-Minuten-Bucketwechsel einen Edit ab.
- Fehler des Edits werden mit Login, Message-ID und Fehlversuchszahl geloggt.
- Ein erfolgreicher Edit wird nur bei vorherigen Fehlern geloggt. Normale erfolgreiche Refreshes bleiben unsichtbar.
- Das gerenderte Embed enthält die tatsächlich verwendete Preview-URL inklusive Cache-Buster.

## Hypothese

Der Discord-Zustand lässt sich beim nächsten Auftreten nur dann sauber einordnen, wenn jeder tatsächlich abgeschickte Refresh mit Bucket, Message-ID, Preview-URL und vorherigen Fehlversuchen sichtbar ist. Das Logging verändert weder Refresh-Intervall noch Payload.

## Scope

Nur der erfolgreiche Live-Sync-Log im Twitch-Bot. Broker und Discord-Adapter bleiben unverändert.

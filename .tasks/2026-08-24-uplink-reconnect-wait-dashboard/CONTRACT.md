status: erledigt
datum: 2026-08-24
klasse: mittel
repo: Deadlock-Twitch-Bot

# Contract: Uplink-Einstellung fuer Internetabrisse

## Ziel

Freigeschaltete Streamer koennen im bestehenden `/twitch/uplink` die bereits
live vorhandene Relay-Wartezeit nach einem unerwarteten Internetabriss sehen
und speichern.

## Anforderungen

- Der gespeicherte Wert und die Obergrenze kommen aus `GET /me`.
- Die UI sagt ausdruecklich: unerwarteter Abriss; normales OBS-Stoppen raeumt sofort auf.
- Speichern laeuft ueber den authentifizierten Dashboard-Proxy.
- 0 ist gueltig; die Relay-Obergrenze wird nicht im Frontend nachgebaut.

## Invarianten

- Keine Stream-Keys im neuen Code.
- Kein Eingriff in Relay-Lifecycle, Migrationen, Ziele, Zeitplan oder Admin-Funktionen.
- Bestehende Tests bleiben erhalten.

## Erlaubter Änderungsbereich

- `bot/dashboard_v2/package.json`
- `bot/dashboard_v2/src/api/uplink.ts`
- `bot/dashboard_v2/src/pages/Uplink.tsx`
- `bot/dashboard_v2/tests/uplinkReconnectWait.test.ts`
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs`
- `rust/crates/tb-dashboard-api/src/lib.rs`
- `.tasks/2026-08-24-uplink-reconnect-wait-dashboard/`

## Nicht-Ziele

- Keine Aenderung an `/home/nathanael/repos/rs-relay`; dessen API war bereits live.
- Keine neue Dashboard-Seite.

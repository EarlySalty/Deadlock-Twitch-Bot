status: erledigt
datum: 2026-08-24
klasse: mittel
repo: Deadlock-Twitch-Bot

# Contract: Uplink-Einstellung fuer Internetabrisse

## Ziel

Streamer koennen im bestehenden Uplink-Dashboard die Wartezeit nach einem unerwarteten Internetabriss sehen und speichern.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Wenn ein Uplink-Zugang freigeschaltet ist, zeigt die Uplink-Seite die gespeicherte Wartezeit und die vom Server gelieferte Obergrenze in Sekunden.
- REQ-02: Die Beschreibung sagt explizit, dass die Einstellung nur fuer einen unerwarteten Internetabriss gilt und ein normales Stoppen in OBS sofort aufraeumt.
- REQ-03: Das Speichern nutzt den bestehenden authentifizierten Dashboard-Proxy und zeigt danach den vom Server tatsaechlich gesetzten, geklemmten Wert an.
- REQ-04: Der Wert 0 ist als gueltige Einstellung speicherbar; Werte ausserhalb der Servergrenze werden nicht lokal erfunden, sondern als Serverantwort uebernommen.

## Invarianten (darf sich nicht aendern)

- INV-01: Der Relay-Server bleibt die Quelle fuer den aktuellen Wert und die Obergrenze; das Frontend dupliziert keine Produktgrenze.
- INV-02: Ein normales OBS-Stoppen wird nicht ueber die Wartezeit verzoegert.
- INV-03: Stream-Schluessel bleiben serverseitig und werden nie im Browser gespeichert oder ausgegeben.
- INV-04: Bestehende Uplink-Ziele, Zeitplaene, Metriken und Admin-Funktionen bleiben unveraendert.
- INV-05: Bestehende Tests werden nicht geloescht oder abgeschwaecht.

## Nicht-Ziele

- Keine Aenderung am Relay-Lebenszyklus oder an der Migration; diese API ist bereits live.
- Keine neue Standalone-Seite und kein neues Dashboard.
- Keine Einstellung fuer ein absichtliches OBS-Stoppen.

## Erlaubter Änderungsbereich

- `bot/dashboard_v2/src/api/uplink.ts`
- `bot/dashboard_v2/src/pages/Uplink.tsx`
- `bot/dashboard_v2/src/pages/uplinkModel.ts`
- `bot/dashboard_v2/tests/uplinkTab.test.ts`
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs`
- `rust/crates/tb-dashboard-api/src/lib.rs`
- `.tasks/2026-08-24-uplink-reconnect-wait-dashboard/`

## Verbotene Änderungen

- Keine Secrets, ENV-Dateien, Lint-Konfiguration oder Datenbankmigrationen.
- Keine Änderung an `/home/nathanael/repos/rs-relay` in diesem Task.
- Keine Änderung an anderen Dashboard-Bereichen.

## Offene Produktfragen

- keine

## Amendments

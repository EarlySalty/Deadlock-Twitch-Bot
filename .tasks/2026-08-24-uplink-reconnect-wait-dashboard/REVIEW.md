status: erledigt
datum: 2026-08-24
reviewer: Hauptsession (read-only Gegenpruefung)

# Review: Uplink-Wartezeit nach Internetabriss

## Befund

Keine blockierenden Befunde und keine Nits im geaenderten Scope.

## Geprueft

- `bot/dashboard_v2/src/pages/Uplink.tsx:544-611` zeigt die Einstellung nur im
  freigeschalteten Streamer-Bereich und haelt ungespeicherte Eingaben trotz
  `/me`-Polling lokal.
- `bot/dashboard_v2/src/api/uplink.ts:42-66` nutzt den bestehenden
  Cookie-/JSON-Pfad; Stream-Keys werden nicht in den neuen Code eingefuehrt.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:394-417` leitet nur die
  authentifizierte Partner-ID und den Wert an das Relay weiter.
- `rust/crates/tb-dashboard-api/src/lib.rs:468-475` registriert ausschliesslich
  die neue PUT-Route im bestehenden Uplink-Router.
- `bot/dashboard_v2/tests/uplinkTab.test.ts:535-577` deckt 0 Sekunden, keine
  Frontend-Klemmung, die Nur-Abriss-Erklaerung und den Proxy-Rumpf ab.

## Verifikation

- Frontend: 94/94 Tests, Build erfolgreich, Lint 0 Fehler.
- Rust: Wiremock-Proxy-Test 1/1, Clippy `-D warnings` erfolgreich.
- Diff-Check: sauber.
- Visuelle Browserabnahme war wegen fehlendem T3-/Browser-Host nicht moeglich;
  die UI wurde statisch gegen den Build und den Contract geprueft.

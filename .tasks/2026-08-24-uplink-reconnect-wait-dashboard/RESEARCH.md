status: aktiv
datum: 2026-08-24
klasse: mittel

## Auftrag

Die Uplink-Seite soll die bereits vorhandene Relay-Einstellung fuer Internetabrisse im bestehenden Streamer-Dashboard bedienbar machen.

## Beobachtungen (belegt, Datei:Zeile)

- `../../repos/rs-relay/src/api/user.rs:41-67` liefert `reconnect_wait_s` und `reconnect_wait_max_s` bereits in `GET /v1/me`; `:396-441` implementiert `PUT /v1/me/reconnect-wait`, klemmt serverseitig und beschreibt, dass ein OBS-Stop sofort aufraeumt.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:357-366` proxied den persoenlichen `/me`-Abruf; `:477-518` zeigt das bestehende Muster fuer authentifizierte persoenliche PUT-Proxy-Aufrufe mit `streamer_id`.
- `rust/crates/tb-dashboard-api/src/lib.rs:467-484` registriert die bestehenden Uplink-Streamer-Routen; der neue Proxy gehoert in denselben Router.
- `bot/dashboard_v2/src/api/uplink.ts:21-45` modelliert `UplinkMe` und den GET; `:85-87` nutzt fuer Mutationen den gemeinsamen JSON-/Cookie-Request.
- `bot/dashboard_v2/src/pages/Uplink.tsx:760-796` laedt `/me` regelmaessig und invalidiert den Query nach Aenderungen; `:890-927` ist der bestehende Streamer-Bereich fuer OBS, Ziele, Zeitplan und Status.
- `bot/dashboard_v2/src/pages/Uplink.tsx:356-447` ist die bestehende UI-/Mutation-Struktur fuer eine gespeicherte Uplink-Einstellung; `bot/dashboard_v2/tests/uplinkTab.test.ts:353-405` und `:513-535` belegen die Testkonventionen fuer Modell und API.

## Hypothesen (unbelegt — nie als Fakt weiterreichen)

- Keine; die Relay-Antwort und der Proxy-Vertrag sind im Bestand belegt.

## Wahrscheinlich zu ändernde Dateien

- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs` — authentifizierter Proxy zum bereits vorhandenen Relay-PUT.
- `rust/crates/tb-dashboard-api/src/lib.rs` — PUT-Route im bestehenden Uplink-Router.
- `bot/dashboard_v2/src/api/uplink.ts` — Response-Typ und Mutation.
- `bot/dashboard_v2/src/pages/Uplink.tsx` — Streamer-Karte mit Wert, Obergrenze, Erklärung und Speicherzustand.
- `bot/dashboard_v2/src/pages/uplinkModel.ts` und `bot/dashboard_v2/tests/uplinkTab.test.ts` — deterministische Anzeige-/Eingabelogik und Vertragstests.

## Risiken / Seiteneffekte

- Ein neuer Frontend-Build ohne den Proxy würde nur einen Fehler anzeigen; deshalb werden Proxy-Route und Frontend gemeinsam verifiziert.
- Das `/me`-Polling darf eine ungespeicherte Eingabe nicht überschreiben; die Karte hält lokale Bearbeitung bis Erfolg oder Abbruch fest.
- Das Frontend darf die Relay-Obergrenze nicht nachbauen; die Anzeige nimmt sie ausschließlich aus der Antwort.

## Offene Fragen

- keine

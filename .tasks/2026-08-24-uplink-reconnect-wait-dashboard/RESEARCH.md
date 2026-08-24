status: erledigt
datum: 2026-08-24
klasse: mittel

# Research

- `rs-relay/src/api/user.rs:41-67` liefert `reconnect_wait_s` und
  `reconnect_wait_max_s`; `:396-441` implementiert den bereits live deployten
  PUT und trennt normalen OBS-Stop vom Abrisspfad.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:190-247` ist der aktuelle
  auth-/Relay-Proxy; der neue Handler folgt demselben `State<PgPool>`- und
  `relay_json`-Muster.
- `bot/dashboard_v2/src/pages/Uplink.tsx:366-479` laedt `/me` regelmaessig;
  die neue Karte sitzt im freigeschalteten OBS-Bereich ab `:480`.
- `bot/dashboard_v2/src/api/uplink.ts:15-63` ist der aktuelle Uplink-Vertrag;
  die Mutation bleibt cookie-basiert und same-origin.

## Risiken

- Das Polling darf eine gerade bearbeitete Eingabe nicht ueberschreiben; die
  Karte behaelt den lokalen Entwurf bis zur Serverantwort.
- Das Frontend darf die Relay-Obergrenze nicht spiegeln; es zeigt nur den
  gelieferten Wert an.

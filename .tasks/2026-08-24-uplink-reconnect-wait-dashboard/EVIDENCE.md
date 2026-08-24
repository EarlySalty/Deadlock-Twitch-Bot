status: erledigt
datum: 2026-08-24

# Evidence

- `bot/dashboard_v2/src/api/uplink.ts:39-88`: Response-Felder, PUT-Proxy,
  serverseitige Eingabegrenze und Nur-Abriss-Erklaerung.
- `bot/dashboard_v2/src/pages/Uplink.tsx:366-479`: bestehender `/me`-Query;
  `:492-570`: Karte mit Entwurfsschutz, Erfolg/Fehler und Servergrenze.
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:241-267`: authentifizierter
  PUT zum Relay mit Partner-ID aus der Session.
- `rust/crates/tb-dashboard-api/src/lib.rs:486-493`: neue Route im bestehenden
  Uplink-Router.
- `bot/dashboard_v2/tests/uplinkReconnectWait.test.ts`: 0, ungeklemmt ausserhalb
  der Grenze, Textsemantik und JSON-Rumpf.

## Verifikation

- Frontend: 133/133 Tests gruen.
- Frontend: `npm run build` erfolgreich; `npm run lint` 0 Fehler, 16 bestehende Warnungen ausserhalb der Uplink-Dateien.
- Rust: Uplink-Testfilter 32/32 gruen.
- Rust: `cargo clippy --lib -- -D warnings -A clippy::redundant-guards` gruen.
- `git diff --check`: folgt vor Commit.
- Browser-/T3-Vorschau: Host in der Umgebung nicht verfuegbar; UI statisch gegen Quellpfad, Tests und gebauten Bundle geprueft.

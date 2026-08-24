status: erledigt
datum: 2026-08-24
klasse: mittel
research: RESEARCH.md

## Ziel

Fertig, wenn ein freigeschalteter Streamer im bestehenden `/twitch/uplink` die Relay-Wartezeit nach Internetabriss sieht, zwischen 0 und der Server-Obergrenze eingeben und speichern kann; normales OBS-Stoppen bleibt als sofortig erklärt.

## Nicht-Ziele

- Relay-Lifecycle, Migrationen, Zielverwaltung, Zeitplan, Admin-Ansicht und Stream-Key-Verarbeitung bleiben unangetastet.

## Milestones

### M1 — Vertragstests und Proxy-Vertrag
Änderungen: `bot/dashboard_v2/tests/uplinkTab.test.ts`, `bot/dashboard_v2/src/pages/uplinkModel.ts`, `bot/dashboard_v2/src/api/uplink.ts`, `rust/crates/tb-dashboard-api/src/handlers/uplink.rs`, `rust/crates/tb-dashboard-api/src/lib.rs`
Erwarteter Zwischenzustand: Der Test beschreibt die servergelieferte Obergrenze, 0 als gültigen Wert und die Nur-Abriss-Erklärung; der neue Proxy ist gegen das Relay verdrahtet.
Validierung: `npm --prefix /home/nathanael/.worktrees/tb-uplink-m5/bot/dashboard_v2 test -- --test-name-pattern='Uplink|Wartezeit|Abriss'`
Stop-Regel: Bei rotem Vertragstest oder unklarem Request-/Response-Vertrag nicht weiter in die UI.

### M2 — Streamer-Karte
Änderungen: `bot/dashboard_v2/src/pages/Uplink.tsx`
Erwarteter Zwischenzustand: Die Karte zeigt Wert und Servergrenze, schützt lokale Eingaben gegen Polling und zeigt Erfolg/Fehler sowie die normale Stop-Semantik.
Validierung: `npm --prefix /home/nathanael/.worktrees/tb-uplink-m5/bot/dashboard_v2 run build`
Stop-Regel: Kein Build bei TypeScript-/ESLint-Vertrag oder wenn der Speicherpfad nicht den Proxy nutzt.

### M3 — Gesamtprüfung und Abschluss
Änderungen: `.tasks/2026-08-24-uplink-reconnect-wait-dashboard/`
Erwarteter Zwischenzustand: Frontend- und Rust-Tests sind ausgeführt, Diff ist auf den Contract-Scope begrenzt und die Review-Prüfung bestätigt die Wirkung.
Validierung: `npm --prefix /home/nathanael/.worktrees/tb-uplink-m5/bot/dashboard_v2 test` und `npm --prefix /home/nathanael/.worktrees/tb-uplink-m5/bot/dashboard_v2 run lint`
Stop-Regel: Bei Fehlern Ursache beheben und denselben Milestone erneut prüfen.

## Verlauf

- 2026-08-24: M1 abgeschlossen — Contract-Test, API-Typen, Relay-Proxy und Route verdrahtet.
- 2026-08-24: M2 abgeschlossen — Streamer-Karte mit serverseitigem Wert, Obergrenze und Speicherzustand gebaut.
- 2026-08-24: M3 abgeschlossen — Frontend, Rust-Proxy, Clippy, Lint, Build und Diff-Pruefung verifiziert.

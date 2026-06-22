# Workflow

## 2026-06-22 — Overlay-Builder-Seite + Config-Params

- Start: delegierter GPT-Implementierungsworker; Scope auf `bot/dashboard_v2/src/**` plus eingebettete JS/CSS-Logik in `rust/crates/tb-dashboard-api/src/handlers/overlay.rs`; verbindliche Review-Regel: keine Commits, kein Push, Aenderungen bleiben uncommitted.
- Implementiert: `/twitch/overlay` liest clientseitig `rank`, `winrate`, `streak`, `live` und `pos`; Default bleibt alles sichtbar und unten links.
- Implementiert: neue Verwaltungssektion `OverlayBuilderSection` mit Toggles, Positionswahl, Live-Vorschau, kopierbarer URL und OBS-Schritten; eingebunden in `Verwaltung.tsx`.
- Tests erweitert: Overlay-HTML-Test prueft Positionsklassen und Flag-Logik.
- Verifikation: `npm --prefix bot/dashboard_v2 run build` gruen nach `npm ci --legacy-peer-deps`; `cargo build -p tb-dashboard-api` und `cargo test -p tb-dashboard-api` gruen aus `rust/`.
- Clippy: `cargo clippy -p tb-dashboard-api` exit 0; bestehende Warnungen in unberuehrten Crates/Dateien bleiben offen. Kein Commit gemaess Review-Regel.

## 2026-06-22 — SP2 Live-Overlay OBS Browser-Source

- Start: Scope auf `rust/crates/tb-dashboard-api`; verbindliche Review-Regel aus Auftrag: keine Commits, Änderungen bleiben uncommitted.
- Befund: Public-Routen liegen in `build_public_router`; vorhandene Resolver-Tabellen sind `twitch_streamers` (`twitch_login` -> `twitch_user_id`) und `twitch_streamer_identities` (`twitch_user_id` -> `discord_user_id`).
- Plan: eigener Overlay-Handler mit öffentlichem JSON-Endpunkt, self-contained HTML-Route, 30s In-Memory-Cache und env-konfigurierbarer Steam-Bot-Basis `STEAM_BOT_RANK_URL`.
- Implementiert: `/twitch/api/v2/public/overlay` und `/twitch/overlay` in `tb-dashboard-api`, inkl. 30s JSON-Cache pro Login, Steam-Bot-Abrufe gegen `/player-mmr-trend`, `/player-matches`, `/player-live` und OBS-HTML ohne externe Assets.
- Verifikation: `cargo build -p tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api` gruen.
- Clippy: `cargo clippy -p tb-dashboard-api` lief durch, meldete aber bestehende Warnungen in unberuehrten Crates/Dateien (`tb-raid`, `tb-social-media`, `tb-analytics`, sowie `tb-dashboard-api/src/handlers/admin_chat_action.rs` und `demo.rs`); gemaess Auftrag hier gestoppt und nicht bereinigt.
- Erweiterung: Overlay JSON gibt `badge_level` aus `current_badge` aus; HTML rendert Rang-Badge- und Live-Hero-Bilder nur ueber oeffentliche Deadlock-Asset-URLs, inkl. Valve-Attribution.
- Tests erweitert: JSON-Schema prueft `badge_level`; HTML-Test prueft Badge-URL-Logik und einmaligen `/v2/heroes?only_active=true`-Fetch.
- Verifikation Erweiterung: `cargo build -p tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api` gruen; `cargo clippy -p tb-dashboard-api` ohne neue Overlay-Lints, aber weiterhin mit vorbestehenden Warnungen in `tb-raid`, `tb-analytics`, `tb-social-media`, `admin_chat_action.rs` und `demo.rs`.

## 2026-06-17 — Dashboard-Login Callback-Portierung

- Start: `WORKFLOW.md` war nicht vorhanden; Datei fuer laufende Implementierung angelegt.
- Ausgangszustand: `main`, ungetracktes `website/testing/` vorhanden und bleibt unangetastet.
- Untersuchung begonnen: Rust-Dashboard-OAuth, Caddy-Callback-Routing und Raid-OAuth-Pfad.
- Branch: `fix/dashboard-login-callback-twitch`.
- Befund: Caddy leitet `/callback/twitch` aktuell auf Python `127.0.0.1:8765`; Python delegiert Raid-OAuth weiter an die interne Rust-API `127.0.0.1:8776/internal/twitch/v1/raid/oauth-callback`.
- Implementierung: Dashboard-Redirect-Default auf `/callback/twitch`, Rust-Dashboard-Route fuer `/callback/twitch`, State-gated Raid-Dispatch zur internen API.
- Verifikation begonnen: `cargo test -p tb-dashboard-api` gruen; Release-Build fuer `tb-dashboard` und `tb-bot` gruen. Breiter Clippy-Lauf zeigte bestehende Warnungen in Ziel-/Abhaengigkeits-Crates; Zielpaket-Warnungen werden minimal bereinigt bzw. begrenzt erlaubt.
- Finale Verifikation: `cargo test -p tb-dashboard-api`, `cargo clippy --no-deps -p tb-dashboard-api -p tb-dashboard -p tb-bot --all-targets -- -D warnings` und `cargo build --release -p tb-dashboard -p tb-bot` gruen.
- Hinweis: `cargo fmt` wurde wegen grosser Workspace-Formatierungswelle nicht als finaler Schritt beibehalten; Diff wurde wieder auf die fachlichen Aenderungen begrenzt.

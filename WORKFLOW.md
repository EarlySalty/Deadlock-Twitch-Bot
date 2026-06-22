# Workflow

## 2026-06-22 — Overlay-Builder-Rework (schick, GC-nativ)

- Start: Worktree `sp2/overlay-rework`; Scope strikt auf `rust/crates/tb-dashboard-api/src/handlers/overlay.rs`, `bot/dashboard_v2/src/components/verwaltung/OverlayBuilderSection.tsx`, `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx`, `WORKFLOW.md`. Vorgaenger-Scaffolding (Structs/Imports) in overlay.rs war uncommittet vorhanden, nicht kompilierbar.
- Datenschicht (TDD): pure Helfer `summarize_today` (Tagesgrenze Europe/Berlin, `now_utc` als Param), `compute_kd`, `build_recent` (newest-first, Cap 15), `summarize_matches` um last_match + most_played erweitert; alle ignorieren `not_scored`. `build_overlay_json` fuellt alle neuen `OverlayResponse`-Felder; 30s-Cache bleibt. Unused `hero_id` aus `SteamMatch` entfernt. Pure-fn Unit-Tests (#[test], keine DB).
- Render: `OVERLAY_HTML` neu — Glassmorphism-Karte, 3 Themes (dark/light/accent via `data-theme` + CSS-Custom-Properties, accent = Marken-Gradient `#06B6D4`→`#A855F7`), 2 Layouts (box/bar), 4 Positionen, opacity nur auf Karten-Hintergrund (`--bg-alpha`), Recent-Strip (26px Ring-Icons + Punkt-Fallback), pulsierender Live-Dot, deutsche Zahlformatierung (`56,7 %` / `1,80` / `4–2`), leere Module verstecken sich. Alle Modul-Flags (lastmatch/mostplayed Default 0), `recent_n` 1–15 Default 10. Render-Branch-Tests + bestehender Struktur-Test angepasst.
- Builder: `OverlayBuilderSection` erweitert — Stil-/Layout-Select, alle 11 Modul-Toggles, Verlauf-Slider, Deckkraft-Slider, Position; URL traegt alle Params; Vorschau-Hoehe je Layout. Sidebar: `toolNavItems` um Eintrag `Stream-Overlay` (MonitorPlay) nach `Verwaltung` ergaenzt (einzige geteilte Nav).
- Verifikation: `cargo build -p tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api` gruen (14 Overlay-Tests, davon DB-Tests gegen vorhandene TB_TEST_DATABASE_URL); `cargo clippy -p tb-dashboard-api` ohne neue Warnungen in overlay.rs (2 vorbestehende in admin_chat_action.rs/demo.rs bleiben); `npm --prefix bot/dashboard_v2 run build` (`tsc -b` + vite) gruen nach `npm ci --legacy-peer-deps`. Vier Commits (Daten/Render/Builder/Sidebar) auf `sp2/overlay-rework`, kein Push/Merge/Restart.

## 2026-06-22 — Overlay-Baukasten als eigene Seite

- Start: delegierter GPT-Implementierungsworker; Scope auf `bot/dashboard_v2/src/**`, `rust/crates/tb-dashboard-api/src/handlers/overlay.rs` und kleinen shared Helper in `spa.rs`; verbindliche Review-Regel: keine Commits, kein Push, Aenderungen bleiben uncommitted.
- Implementiert: `/twitch/overlay?streamer=<login>` liefert weiter das OBS-Render-HTML; `/twitch/overlay` ohne Streamer liefert den dashboard_v2-SPA-Index ueber gemeinsamen `spa`-Helper.
- Implementiert: eigene React-Seite fuer den Overlay-Baukasten, Route-Konstante und App-Routing; Verwaltung zeigt nur noch den Link zur neuen Seite.
- Verifikation: `npm --prefix bot/dashboard_v2 run build` gruen nach `npm ci --legacy-peer-deps`; `cargo build -p tb-dashboard-api` und `cargo test -p tb-dashboard-api` gruen; `cargo clippy -p tb-dashboard-api` exit 0 mit bestehenden Warnungen ausserhalb der geaenderten Overlay-/SPA-Stellen. Kein Commit gemaess Review-Regel.

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

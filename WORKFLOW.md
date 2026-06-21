# Workflow

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

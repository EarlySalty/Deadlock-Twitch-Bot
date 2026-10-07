# Prüfstand der Twitch-Pfadmigration

## Ausgangspunkt

Eigener Arbeitszweig `feat/twitch-pfade`, Twitch-Worktree `/home/nathanael/.worktrees/Deadlock-Twitch-Bot-twitch-pfade`, Ausgangsstand `10dacbc2`. Der gemeinsame Main-Checkout bleibt unverändert. Separate Caddy-Arbeit liegt unter `/home/nathanael/.worktrees/Caddy-twitch-pfade`, ebenfalls `feat/twitch-pfade`, Basis `origin/master` (`bf73126`).

## Bisher ausgeführt

- `cargo check -p tb-dashboard-api -p tb-bot -j 2` mit `/home/nathanael/.cargo/bin/cargo`: erfolgreich. Das veraltete System-Cargo konnte die Lockdateiversion 4 nicht lesen und wurde nicht weiter verwendet.
- Rustfmt ausschließlich für die geänderten Rust-Dateien ausgeführt; deren abschließender `rustfmt --check --edition 2021 --config skip_children=true` ist grün. Paketweiter `cargo fmt --check` scheitert an Formatierungen außerhalb dieser Änderung; der unveränderte Baseline-Checkout ist ebenfalls rot.
- `npm --prefix bot/dashboard_v2 test`: 9 Kalenderprüfungen bestanden; Hauptsuite 425 bestanden, 5 fehlgeschlagen, 0 übersprungen. Unveränderte Baseline in eigenem detached Worktree auf `10dacbc2`: dieselben 9 plus 425 bestanden, dieselben 5 fehlgeschlagen, 0 übersprungen. Die vier neu durch die Pfadänderung entstandenen Erwartungen wurden angepasst. Verbleibend: drei Markenpaletteprüfungen, Analytics-Gesamtbreite, OBS-Hilfetext.
- Dashboard-Produktionsbuild erfolgreich. Artefakte liegen tatsächlich unter `bot/analytics/dashboard_v2/dist`, nicht unter `bot/dashboard_v2/dist`. Website-Build ebenfalls erfolgreich.
- Endgültiger Rust-Routentest mit `DASHBOARD_V2_DIST_PATH=/home/nathanael/.worktrees/Deadlock-Twitch-Bot-twitch-pfade/bot/analytics/dashboard_v2/dist`: `cargo test -p tb-dashboard-api --test twitch_paths --test social_media_routes -j 2 -- --include-ignored`, 6 bestanden, 0 fehlgeschlagen, 0 ignoriert. Umfasst direkten alten/neuen Rechts- und Callbackzugriff, API-/CSRF-Parität, Seiten-308 mit Methode, Kodierung, Query und abschließendem Slash, eigenes Loginziel und reale Bundleauslieferung.
- Caddy-Adapt des eigenen Caddyfiles erfolgreich; vorhandene Warnungen zu X-Forwarded-Headern und Formatierung.
- Striktes Clippy mit `-D warnings` stoppt in der unveränderten Datei `tb-raid/src/signup_denylist.rs:71` wegen `Result<_, ()>`. Normaler Clippy-Lauf für `tb-dashboard-api` und `tb-bot` erfolgreich, mit Bestandswarnungen.
- Bestehende SPA-Unitprüfungen mit `cargo test -p tb-dashboard-api --lib handlers::spa -j 2 -- --include-ignored`: 15 bestanden, 0 fehlgeschlagen, 0 ignoriert, 1325 herausgefiltert. Keine DB-gegateten Tests als Produktionsbeweis behauptet.

## Sichtprüfung

Moli 1.1.14, eigener kurzlebiger lokaler HTTP-Fixtureprozess auf `127.0.0.1:18779`. Reales gebautes Dashboard, synthetische Admin-/API-Daten, echte Markenschriften. Keine persönlichen Cookies und kein anderer Browser.

Manager unter `/twitch/social-media?streamer=earlysalty&twitch_user_id=11`: Ansicht und Sidebar im Moli-Viewport geprüft. Laufzeitdiagnose: keine JavaScriptfehler; Dokumentbreite 1920, Viewport 1920, Schriften geladen. API-Aufrufe im neuen Namensraum beobachtet. Analytics-Screenshot ebenfalls angesehen; korrekte aktive Analyse-Navigation und leere Fixtureübersicht. Screenshots und Fixturecode sind unter `/home/nathanael/.claude/sichtpruefung/twitch-pfade-e83380af/` dauerhaft gesichert. Die DOM-Erhebung bestätigt die Sidebarziele `/twitch/analyse` und `/twitch/social-media`. Diese Fixtureprüfung ist kein Beleg für einen Live-Login und keine Chrome-/Firefox-/Safari-Kompatibilitätsaussage.

## Noch offen

Rebase auf `origin/main` (`001486736c739d1f7c0f9da4a5b63ab0b6702507`) abgeschlossen; die VOD-Arbeit ist enthalten. Beide ersten Gates ergaben ALLOW. Auch der abschließende Header-Routentest ist mit 6 bestandenen, 0 fehlgeschlagenen und 0 ignorierten Tests grün. Zweite Gateprüfung der Ergänzungen, Veröffentlichung, Deployment und Live-Nachweis stehen noch aus. Frühere Routentestläufe waren wegen des falschen Bundlepfads rot; nur die abschließenden Läufe mit dem tatsächlichen Buildverzeichnis werden gewertet.

## Caddy: fremde Änderungen schützen

Der Live-Caddyfile `/etc/caddy/Caddyfile` enthält zusätzliche Änderungen für `earlysalty.de`, die auf `origin/master` nicht vorhanden sind. Auch der gemeinsame Caddy-Checkout ist fremd geändert. Kein vollständiges Überkopieren des eigenen Worktree-Caddyfiles. Für den Live-Reload nur die eigenen Matcher-/Redirectänderungen auf den aktuellen Live-Stand anwenden, dann validieren. TLS-Daten und Caddy-State werden nie committed.

# Frischer Fixer: unbekannten Verbindungszustand bei Datenbankfehlern erhalten

## Rolle und Eigentum

Frischer Blatt-Fixer nach dem zweiten inhaltlichen Merge-Gate-BLOCK. Kein zusätzlicher Worker, kein eigener Review-Thread. Den bestehenden Worktree `/home/nathanael/.worktrees/tb-social-token-ablauf` und Branch `fix/social-token-ablauf` übernehmen. Haupt-Orchestrator: `d3a1741e-82bc-4a48-865b-2845c663dca7`. Der fremd veränderte Haupt-Checkout bleibt unangetastet.

Geprüfter Code-HEAD: `260bdfdc`. Darin ist der erste Gate-Fund zum Ablauf nicht erneuerbarer TikTok-/YouTube-Verbindungen behoben. `origin/main` wurde auf `e0b0dbaf` gefetcht und regulär in den bestehenden Branch rebased. Danach folgt der Dokumentationscommit zur Übergabe. Nicht erneut bauen oder den Ablauf-Fix zurücknehmen.

## Pflichtfix

Das unveränderte Gate-Modell `gpt-6.1-sol` urteilt erneut BLOCK:

```text
rust/crates/tb-social-media/src/credentials.rs:212 | BLOCKING: Metadata-query errors become None, producing connected: false for TikTok, YouTube, and Instagram. platforms_status_handler then returns HTTP 200. Propagate this query failure to the API error response.
```

1. Nach Graphify-Bestandssuche die Fehlerbehandlung des Statuslesers korrigieren. Der neue Metadata-Lesepfad darf bei einem SQL-Fehler keinen vermeintlich getrennten Zugang erzeugen. Den Fehler bis zur bestehenden API-Fehlerantwort weitergeben, damit die Dashboard-Karte ihren vorhandenen unbekannten und gesperrten Zustand zeigt. Bestehende Aufrufer und Zwillinge prüfen. Keine neue UI oder zusätzliche Versandstrecke bauen.
2. Betroffene Rust-/API-Prüfungen nachziehen. Ein gezielt fehlgeschlagener Metadata-Lesezugriff muss den API-Fehlerpfad statt HTTP 200 mit `connected: false` belegen. Ein tatsächlich nicht vorhandener Zugang bleibt von einem Lesefehler unterscheidbar. Die bekannten erneuerbaren und nicht erneuerbaren Ablauf-Fälle müssen erhalten bleiben.
3. Format, Clippy, betroffene Tests und Dashboard-Build ausführen. Die vollständige historische API-Baseline steht in `EVIDENCE.md`: 1309 passed, 22 failed, identische Fehlerliste. Keine vorhandenen Prüfungen still überspringen. Die Produktionsmigration `20261007120000_social_connection_expiry.sql` und die SHA384 in `PROD-MIGRATION.sql` bleiben unverändert.
4. Befunde und Nachweise aktualisieren. Der nächste unveränderte Merge-Gate muss wieder mit `gpt-6.1-sol` urteilen. Nach einem weiteren BLOCK erhält ein neuer Fixer aus der Pyramide die Liste, nicht dieser Implementierer. Keine alternative Review-Delegation.

## Erhaltene Nachweise

Der Ablauf-Fix liest im Sweep ausschließlich die Existenz des verschlüsselten Refresh-Feldes, keine Secrets. Instagram und nicht erneuerbare TikTok-/YouTube-Verbindungen verwenden ihr Access-Ende. Erneuerbare Verbindungen verwenden weiterhin ihre tatsächliche Refresh-Frist. `reauth_soon` und das Dashboard-Datum sind nachgezogen. `refresh_expires_at` bleibt das echte Anbieterfeld.

PostgreSQL-Proben verwenden die feste Uhr `2026-10-07T12:00:00Z`: genau sieben Tage ohne Warnung, sechs Tage mit Warnung, Eskalation genau beim Ablauf, unveränderte Vorfalls- und Versandzeiten nach Neustart. Der Bot-Adapter verwendet denselben Sweep gegen PostgreSQL und den lokalen Test-Broker; pro Plattform ein Request trotz Neustart und Eskalation. Kein Produktionszugang wurde als DM-Probe benutzt.

Aktuelle Bild- und DOM-Proben am nach dem Rebase gebauten Dashboard `index-C1sEecg0.js`: `instagram-20-days.png`, `nonrenewable-6-days.png`, `renewable-access-ended.png`, `nonrenewable-ended.png`, `ui-proof.json`. Die vier Proben hatten keine Seitenfehler oder horizontale Überbreite. Das ausdrücklich beauftragte Instagram-Fenster von weniger als 30 Tagen wurde nicht geändert. Reproduktion: `node .tasks/2026-10-07-social-token-ablauf/ui-proof.mjs`.

Die Wegwerf-DB `tb_social_token_ablauf` auf Port 33045 war am 7. Oktober 2026 verfügbar. Keinen fremden Container starten oder stoppen. Eigener fehlgeschlagener Container: `tb-social-token-ablauf-db`, bei endgültigem Cleanup prüfen und entfernen. Vor Wiederaufnahme die Verfügbarkeit erneut prüfen.

## Freigabe und Abschluss

Kein Merge, keine Produktionsmigration oder Deploy vor ALLOW. Nach ALLOW gilt der ursprüngliche Abschlussauftrag aus `TODO.md`: regulärer Merge, `git push origin HEAD:main`, manuelle Migration als `postgres`, sauberer Release-Build mit `-j 2`, acht Rust-Binaries und drei Frontends, Herkunft gegen aktuellen `origin/main`-SHA, eigenständiger Release-Clone, `deploy-twitch-release`, Neustarts, Live-Beweis und Cleanup. Ein weiterer Herkunftscheck erfolgt nach aktuellen Fetches, nicht anhand von Dateialter. Wertvolle ignorierte Logs vor Worktree-Löschung sichern. Kein Selbst-Settle vor belegtem Abschluss.

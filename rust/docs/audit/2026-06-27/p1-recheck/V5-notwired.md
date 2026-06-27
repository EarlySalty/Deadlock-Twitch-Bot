# V5 P1 Not-Wired Recheck

Datum: 2026-06-27

Rolle: Verifizierer + Git-Archaeologe. Scope: Read-only fuer Code und Git; keine Secrets; keine Code-Aenderung. Geschrieben wurde nur dieser Audit-Report plus WORKFLOW-Notiz.

Gepruefte Ausgangspunkte:

- `rust/docs/audit/2026-06-27/verified/C2-notwired.md`
- `rust/docs/audit/2026-06-27/findings/B5b-dashboard-analytics.md`
- `rust/docs/audit/2026-06-27/findings/B8-social-highlight.md`
- `rust/docs/cutover-backlog.md`
- `rust/docs/audit/2026-06-27/00-baseline.md`
- `rust/docs/04-cutover-plan.md`
- Grillme: `rust/docs/audit/2026-06-15-grillme-entscheidungen.md` und `_work/grillme-*`

## Summary

| Item | Verdict | Kurzurteil |
|---|---|---|
| B5b-05 `raid_network_analytics_handler` | ASK-USER | Nicht uebersehen: Git + Backlog belegen bewusste Deferred-/WIP-Entscheidung fuer das Legacy-/Admin-Wiring. Aber Handler und Baseline behaupten zugleich, die P2.130-Datensicht solle nicht verloren gehen bzw. sei behoben. Fix ist technisch klar, Produktentscheidung bleibt offen. |
| B8-009 `TokenRefreshWorker` | FIX-CLEAR | Uebersehen beim Social-Cutover: Worker wurde vorher als "noch nicht gespawnt" eingefuehrt, spaeter behauptet `tb-bot` "gesamte Pipeline / 6 Worker", laesst den Refresh-Worker aber aus. Social darf opt-in/dormant sein; wenn aktiviert, ist Token-Refresh Voraussetzung. |

## B5b-05 - Raid-Netzwerk-Analytics

Verdict: **ASK-USER**.

### Git-Archaeologie

Ausgefuehrt:

- `git log -S"raid_network_analytics_handler"`
- `git log --oneline -- rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs`
- `git blame -L 1,18 -- rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs`
- `git show --stat 6ef94aa`
- `git show --unified=12 6ef94aa -- rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs rust/crates/tb-dashboard-api/src/handlers/mod.rs`
- Spaetere Router-Historie: `git log --oneline -- rust/crates/tb-dashboard-api/src/lib.rs`, `git blame -L 1178,1188 -- rust/crates/tb-dashboard-api/src/lib.rs`, `git show --stat c8ada2a`

Ergebnis:

- Einziger Einfuehrungscommit ist `6ef94aa feat(dashboard-api): P2.118/P2.130 native Raid-Netzwerk-Analytics + Link-Fix`.
- Commit-Message `6ef94aa`: neuer admin-only Handler fuer Partner-Send/Receive-Balance, Leecher und Manual-Raid-Listing; **Routen-Registrierung des neuen Handlers als WIRING-TODO, Composition-Root tabu**.
- `git blame` zeigt den TODO-Dateikopf komplett aus `6ef94aa`: `raid_network_analytics.rs:14-15` sagt, Registrierung erfolge im Composition-Root und verweist auf WIRING-TODO.
- Spaeterer Commit `c8ada2a feat(cutover): Main-Domain-Dashboard-Routen nativ + Go-Live-Builder entfernt` setzt `/twitch/raid/analytics` bewusst auf SPA-Redirect. `git blame` fuer `lib.rs:1184-1185` zeigt diese Route aus `c8ada2a`.

Das spricht gegen "versehentlich nie gemerkt" und fuer bewusst nicht in derselben Runde verdrahtet.

### Aktueller Codezustand

- Handler existiert: `rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs:36`.
- Dateikopf sagt, die alte Datensicht solle als JSON erhalten bleiben: `raid_network_analytics.rs:4-8`.
- Vorgeschlagener Routenname im Code: `GET /twitch/raid/analytics/network` (`raid_network_analytics.rs:33`).
- Payload enthaelt die relevanten Keys: `partner_stats`, `leechers`, `manual_raids`, `date_min`, `date_max`, `total`, `active_partner_count` (`raid_network_analytics.rs:177-184`).
- Modul wird exportiert: `handlers/mod.rs:66`.
- Live registriert sind nur `/twitch/api/v2/raid-retention` und `/twitch/api/v2/raid-analytics` auf `raid_analytics::*` (`lib.rs:519-524`).
- `/twitch/raid/analytics` ist nur SPA-Redirect (`lib.rs:1184-1185`).
- `rg` findet `raid_network_analytics_handler` nur im Handler/Testmodul, keine Route.

### Intent / Backlog

- `rust/docs/cutover-backlog.md:16-23` dokumentiert `/twitch/raid/analytics` als alte SSR-Raid-Netzwerk/Sankey-Seite. Use-Case: "unklar / eher nutzlos", ggf. admin-relevant. Datenteil existiert in Rust mit WIRING-TODO. TODO spaeter: WIRING-TODO schliessen und an SPA-Admin-Sicht haengen, falls dedizierter Admin-View gewuenscht ist.
- `00-baseline.md:73` fuehrt alte `/twitch/partners` und Raid-Analytics-SSR als SPA/native Umlenkung "nur bei Bedarf wiederbauen".
- `00-baseline.md:281` markiert P2.130 `Native raid history/analytics` aber als **BEHOBEN** mit Commit `6ef94aa`.
- `00-baseline.md:334` nennt `Raid-Analytics SPA-Wiring` wiederum als offene Backlog-Frage.

Bewertung: Dokumentiert ist ein echter Widerspruch. Das Legacy-SSR-UI ist bewusst deferred. Die Daten-API ist dagegen als Soll/Behoben beschrieben, aber nicht erreichbar. Deshalb nicht `ALREADY-CLEAN`, nicht eindeutig `DEFERRED-BY-DESIGN` fuer den API-Teil, sondern **ASK-USER**: Soll die dedizierte Admin-Datensicht jetzt live gehen?

### Fix-Spec, falls Owner "ja" sagt

Minimaler API-Wire-up ohne UI-Neubau:

1. In `rust/crates/tb-dashboard-api/src/lib.rs:121` die `use handlers::{...}`-Liste um `raid_network_analytics` erweitern.
2. In `build_authed_router` eine Route registrieren, z.B. nahe `lib.rs:310-315` oder nahe den Raid-Analytics-Routen:

```rust
.route(
    "/twitch/raid/analytics/network",
    get(raid_network_analytics::raid_network_analytics_handler),
)
```

3. Nicht `/twitch/raid/analytics` selbst ersetzen, solange der Cutover-Backlog den SPA-Redirect als bewusste Legacy-SSR-Entscheidung fuehrt.
4. Falls die SPA es nutzen soll: Admin-/SPA-Sicht separat an diesen JSON-Endpunkt haengen.

## B8-009 - Social TokenRefreshWorker

Verdict: **FIX-CLEAR**.

### Git-Archaeologie

Ausgefuehrt:

- `git log -S"TokenRefreshWorker"`
- `git log --oneline -- rust/crates/tb-social-media/src/refresh_worker.rs`
- `git blame -L 1,12 -- rust/crates/tb-social-media/src/refresh_worker.rs`
- `git show --stat 60c8970`
- `git show --unified=12 60c8970 -- rust/crates/tb-social-media/src/lib.rs rust/crates/tb-social-media/src/refresh_worker.rs`
- Spaeteres Wiring: `git log --oneline -- rust/bin/tb-bot/src/main.rs`, `git blame -L 1185,1248 -- rust/bin/tb-bot/src/main.rs`, `git show --stat cde5a658`, `git show --unified=12 cde5a658 -- rust/bin/tb-bot/src/main.rs`

Ergebnis:

- Einfuehrungscommit `60c8970 tb-social-media: Token-Refresh + Refresh-Worker (Slice O3)` portiert `token_refresh_worker.py`.
- Commit-Message `60c8970`: `run = 60s-Delay + 5min-Loop (noch nicht gespawnt)`.
- `git blame` zeigt den Dateikopf aus `60c8970`: `refresh_worker.rs:4-5` sagt, der periodische `TokenRefreshWorker::run` werde vom Pipeline-Cutover gespawnt, **noch nicht verdrahtet**.
- Spaeterer Social-Cutover `cde5a658 tb-bot: Social-Media-Pipeline Cutover - 6 Worker + Clip-Fetcher live` behauptet "gesamte Social-Media-Posting-Pipeline" und "alle Worker", listet aber nur Retention, Approval, ReportDispatcher, Enrichment, Upload, Insights.
- `git blame` fuer `main.rs:1185-1244` zeigt den Block aus `cde5a658`; dort steht "sechs Hintergrund-Worker" und am Ende "6 Loops".
- Kein spaeterer Commit fuegt `TokenRefreshWorker` in `tb-bot` hinzu. `rg` findet `TokenRefreshWorker::new` nur in Tests von `refresh_worker.rs`.

Das ist kein bewusst dokumentierter Drop des Refresh-Workers. Bewusst gedroppt wurde nur Admin-Reauth per Discord-DM, nicht der eigentliche proaktive Token-Refresh.

### Aktueller Codezustand

- Worker existiert: `rust/crates/tb-social-media/src/refresh_worker.rs:24-32`.
- Loop existiert: `refresh_worker.rs:35-41`.
- `run_once` selektiert binnen 1h ablaufende Tokens mit Refresh-Token: `refresh_worker.rs:44-64`.
- `tb-bot` spawnt Retention, Approval, Reports, Enrichment: `rust/bin/tb-bot/src/main.rs:1193-1210`.
- `tb-bot` spawnt bei vorhandenem FieldCipher Upload und Insights: `main.rs:1215-1236`.
- Kein Spawn fuer `TokenRefreshWorker`.
- Uploader-Kommentare erwarten den Refresh-Worker:
  - `uploaders/mod.rs:4-6`: Uploader bekommen bereits frisches Access-Token, Refresh laeuft im `refresh_worker`.
  - `uploaders/youtube.rs:9-13`: Token-Refresh laeuft primaer proaktiv im `refresh_worker`; 401-Heilung ist nur Zusatz.
  - `upload_worker.rs:73-76`: Ohne vollstaendige Inline-Refresh-Daten bleibt Refresh nur proaktiv ueber `refresh_worker`.

### Intent / Backlog

- Grillme Block 15 (`rust/docs/audit/2026-06-15-grillme-entscheidungen.md:321-336`) sagt: keine Discord-DMs, Clip-Erstellung default aus, Transkription raus, Auto-Upload-Pipeline dormant solange Clip-Erstellung aus. Gleichzeitig steht `uploaders-1 YouTube-Token-Refresh` in der Fix-Liste.
- `00-baseline.md:32` sagt: Social-Media-Domaene portiert; Clip-/Upload-Worker bleiben data-/env-gated.
- `00-baseline.md:54` sagt: Auto-Upload/Clip-Creation/Transcription nicht default-aktiv; Social/Highlight-Worker sind opt-in und brauchen Aktivierungsdoku/Test.
- `00-baseline.md:333` fuehrt Social/Highlight Worker Aktivierung als offenen Pruefpunkt.
- `04-cutover-plan.md:179-182` definiert Schritt 8 Social-Media Upload-Pipeline; Erfolgskriterium: Clip-Upload auf mindestens einer Plattform und **OAuth-Refresh greift**.
- `docs/architecture/social-media.md:93` sagt ausdruecklich: `token_refresh_worker` muss laufen, sonst scheitern Uploads trotz verbundenem Konto.

Bewertung: Social ist opt-in/dormant, aber der Refresh-Worker ist Teil des funktionsfaehigen opt-in-Betriebs. Sobald FieldCipher/Plattform-OAuth aktiv ist, muss er neben Upload/Insights laufen. Daher **FIX-CLEAR**.

### Fix-Spec

In `rust/bin/tb-bot/src/main.rs:1215-1236`, im `Ok(cipher)`-Zweig direkt nach `let cipher = Arc::new(cipher);`:

```rust
let refresh_oauth = tb_social_media::oauth::OAuthManager::new(pool.clone(), cipher.clone());
let refresh = tb_social_media::refresh_worker::TokenRefreshWorker::new(
    pool.clone(),
    cipher.clone(),
    refresh_oauth,
);
tokio::spawn(async move { refresh.run().await });
```

Danach bestehende Upload-/Insights-Wires unveraendert weiter nutzen; falls `cipher` spaeter bewegt wird, fuer `InsightsWorker` `cipher.clone()` oder einen eigenen Clone verwenden. Den Log von "6 Loops" auf "7 Loops" bzw. genauer "4 Basis-Loops, 7 mit FieldCipher" anpassen.

Sinnvolle Verifikation nach Fix:

- `cargo build -p tb-bot`
- `cargo test -p tb-social-media refresh_worker`
- optional ein statischer `rg "TokenRefreshWorker::new" rust/bin/tb-bot rust/crates/tb-social-media` als Wire-up-Nachweis.

## Verifikation dieser Runde

Nur statisch/lesend:

- `rg`, `find`, `sed`, `nl`
- `git log`, `git blame`, `git show`

Nicht ausgefuehrt:

- keine Tests
- kein Build
- keine Services
- keine DB-Verbindung
- kein `git add`, `git commit`, `git push`, `git checkout`


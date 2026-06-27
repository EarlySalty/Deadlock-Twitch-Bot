# Audit-A: Baseline + Intent-Ledger + Re-Status

Stand: 2026-06-27, aktueller Arbeitsbaum/HEAD `97bcc56`.

Quellenregel: jede fachliche Aussage nennt Datei:Zeile oder Commit-Hash. Secrets wurden nicht gelesen; `.env`, `service_token.json` und `/proc/*/environ` wurden nicht geöffnet.

## 1. Python->Rust-Domänen-Map

| Python-Domäne | Erkennbares Rust-Pendant | Status / Notiz | Beleg |
|---|---|---|---|
| `bot/admin_dashboard` | `tb-dashboard-api`, `bin/tb-dashboard` | In native Axum/API-Dashboard zusammengeführt; Admin-System-, Config- und Streamer-Routen liegen in `tb-dashboard-api`. | `rust/crates/tb-dashboard-api/src/lib.rs:562`, `rust/crates/tb-dashboard-api/src/lib.rs:598`, `rust/bin/tb-dashboard/src/main.rs:47` |
| `bot/analytics` | `tb-analytics`, `tb-dashboard-api` | Query-/Business-Logik in `tb-analytics`; HTTP-Fläche im Dashboard-API-Crate. | `rust/crates/tb-analytics/src/lib.rs:1`, `rust/crates/tb-analytics/src/lib.rs:8`, `rust/crates/tb-dashboard-api/src/lib.rs:120` |
| `bot/api` | `tb-transport-twitch`, `tb-http-core` | Twitch/HTTP-Transport ersetzt Python-API-Client; Python-Quelle war `TwitchAPI`. | `bot/api/twitch_api.py:23`, `rust/crates/tb-transport-twitch/src/client.rs:106`, `rust/crates/tb-http-core/src/lib.rs:1` |
| `bot/bot_service` | `bin/tb-bot` | Headless Bot-Service ist Rust-Binary/Composition-Root. | `bot/bot_service/app.py:13`, `bot/bot_service/app.py:63`, `rust/bin/tb-bot/src/main.rs:1` |
| `bot/chat` | `tb-chat`, `tb-transport-twitch`, `bin/tb-bot` | Chat-Domäne portiert; Runtime-Wiring im Binary. | `bot/chat/bot.py:2204`, `rust/crates/tb-chat/src/lib.rs:1`, `rust/bin/tb-bot/src/chat_wiring.rs:33` |
| `bot/community` | Teilweise `tb-raid`, `tb-dashboard-api`, `tb-engagement` | Partner-Recruitment/Community-Logik ist gesplittet; einige externe Recruitment-Followups bleiben dokumentiert deferred. | `bot/community/partner_recruit.py:44`, `rust/crates/tb-raid/src/recruitment_messaging.rs:274`, `rust/docs/05-cleanup-decisions.md:102` |
| `bot/compat` | `tb-crypto`, `tb-config` | Python-Feldcrypto durch Rust-Crypto/Config ersetzt; Fernet nicht 1:1 nachgebaut. | `bot/compat/field_crypto.py:36`, `rust/docs/adr/0003-crypto-interop-or-reauth.md:18`, `rust/docs/adr/0003-crypto-interop-or-reauth.md:44` |
| `bot/core` | `tb-domain`, `tb-chat`, `tb-raid` | Gemeinsame Core-Helfer wurden in fachliche Crates aufgeteilt. | `bot/core/chat_bots.py:24`, `rust/crates/tb-domain/src/lib.rs:1`, `rust/crates/tb-chat/src/lib.rs:25` |
| `bot/dashboard` | `tb-dashboard-api`, `bin/tb-dashboard` | Flask/SSR/Proxy-Strangler durch native Axum-Fläche plus optionalen Fallback ersetzt. | `bot/dashboard/server_v2.py:86`, `bot/dashboard/server_v2.py:1085`, `rust/bin/tb-dashboard/src/main.rs:47`, `rust/crates/tb-dashboard-api/src/lib.rs:44` |
| `bot/dashboard_preview` | Kein klares Crate-Pendant | Preview-Frontend ist ein Kandidat für bewusst nicht portierte Legacy-Oberfläche. | `bot/dashboard_preview/src/App.tsx:369`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:33` |
| `bot/dashboard_service` | `bin/tb-dashboard`, `tb-dashboard-api` | Service-App in native Dashboard-Binary/API verschoben. | `bot/dashboard_service/app.py:110`, `rust/bin/tb-dashboard/src/main.rs:47`, `rust/bin/tb-dashboard/src/main.rs:66` |
| `bot/dashboard_v2` | `tb-dashboard-api` + Rust-Assets/SPA | V2-Frontend-Funktionalität ist in native JSON-Routen/SPA-Fläche überführt, nicht als Python-Server beibehalten. | `bot/dashboard_v2/src/App.tsx:378`, `rust/crates/tb-dashboard-api/src/lib.rs:77`, `rust/docs/04-cutover-plan.md:199` |
| `bot/engagement` | `tb-engagement`, `tb-llm`, `tb-dashboard-api` | Engagement-Pipeline portiert, aber AI/Shadow-Funktionen sind per Default kontrolliert. | `bot/engagement/pipeline.py:194`, `rust/crates/tb-engagement/src/lib.rs:1`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:399` |
| `bot/entitlements` | `tb-analytics`, `tb-dashboard-api` | Entitlement/Billing-Resolver liegt im Analytics-/Dashboard-Kontext. | `bot/entitlements/resolver.py:19`, `rust/crates/tb-analytics/src/billing/catalog.rs:496`, `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:715` |
| `bot/highlight_clipper` | `tb-highlight`, `bin/tb-bot` | Worker portiert, aber opt-in/standardmäßig aus. | `bot/highlight_clipper/worker.py:47`, `rust/bin/tb-bot/src/main.rs:1`, `rust/docs/audit/2026-06-19-rust-cutover-stabilisierung.md:6` |
| `bot/internal_api` | `tb-internal-api`, `bin/tb-bot` | Interne API nativ; Ports werden im Bot-Binary injiziert. | `bot/internal_api/app.py:83`, `bot/internal_api/app.py:935`, `rust/crates/tb-internal-api/src/lib.rs:38`, `rust/bin/tb-bot/src/main.rs:1513` |
| `bot/live_announce` | `tb-monitoring::announce`, `tb-dashboard-api` | Core-Announcement-Mechanik vorhanden; Go-Live-Builder wurde bewusst entfernt. | `bot/live_announce/template.py:175`, `rust/crates/tb-monitoring/src/announce/template.rs:847`, `rust/docs/cutover-backlog.md:25` |
| `bot/migrations` | `tb-db`, Rust-SQL-Migrationen | Python-Startup-Migrationen wurden durch saubere SQLx-Migrations ersetzt. | `rust/docs/adr/0002-db-sqlx-refinery-shared-schema.md:14`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:261`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:290` |
| `bot/monitoring` | `tb-monitoring`, `bin/tb-bot` | EventSub/Live-State/Chatter/Monitoring in Rust-Monitoring-Crate und Bot-Wiring. | `bot/monitoring/monitoring.py:45`, `rust/crates/tb-monitoring/src/lib.rs:1`, `rust/bin/tb-bot/src/eventsub_hooks.rs:198` |
| `bot/raid` | `tb-raid`, `tb-dashboard-api`, `tb-internal-api`, `bin/tb-bot` | Raid-Domäne breit aufgespalten: Kernlogik, Dashboard, Internal API und Wiring. | `bot/raid/bot.py:79`, `rust/crates/tb-raid/src/lib.rs:1`, `rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs:2`, `rust/crates/tb-internal-api/src/lib.rs:173` |
| `bot/runtime` | `tb-config`, `tb-observability`, `bin/tb-bot` | Runtime-Config/Wiring nicht 1:1, sondern in Composition-Root und Config/Observability-Crates verteilt. | `bot/runtime/bot_runtime.py:117`, `rust/bin/tb-bot/src/main.rs:1`, `rust/crates/tb-config/src/lib.rs:1`, `rust/crates/tb-observability/src/lib.rs:1` |
| `bot/social_media` | `tb-social-media`, `tb-dashboard-api`, `bin/tb-bot` | Social-Media-Domäne portiert; Clip-/Upload-Worker bleiben data-/env-gated. | `bot/social_media/upload_worker.py:28`, `rust/crates/tb-social-media/src/lib.rs:1`, `rust/crates/tb-social-media/src/lib.rs:42`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:321` |
| `bot/storage` | `tb-db`, SQLx in Fachcrates | Python-Pool/Startup-Maintenance durch SQLx/Migrations und crate-lokale Stores ersetzt. | `bot/storage/pg.py:522`, `rust/docs/05-cleanup-decisions.md:7`, `rust/docs/adr/0002-db-sqlx-refinery-shared-schema.md:14` |
| `bot/stream_coaching_audit` | Kein klares Rust-Pendant | Kandidat für nicht portiert/deferred; Grillme nennt Stream-Coaching-Audit als später. | `bot/stream_coaching_audit/service.py:168`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:423` |
| `bot/title_generator` | `tb-chat`, `tb-dashboard-api`, `tb-llm` | Titelgenerator in Chat-Commands, Dashboard-Titelhandler und LLM-Crate verteilt. | `bot/title_generator/insight_job.py:65`, `rust/crates/tb-dashboard-api/src/handlers/title.rs:130`, `rust/crates/tb-chat/src/title_jobs.rs:1`, `rust/crates/tb-llm/src/lib.rs:1` |
| `bot/__pycache__` | Kein Pendant | Build-Artefakt, keine Domäne. | `bot/__pycache__`, `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:4373` |

**Kandidaten ohne klares Rust-Pendant:** `bot/dashboard_preview` (`bot/dashboard_preview/src/App.tsx:369`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:33`), `bot/stream_coaching_audit` (`bot/stream_coaching_audit/service.py:168`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:423`), Teile von `bot/community`/VoiceReaction (`rust/docs/audit/2026-06-15-grillme-entscheidungen.md:312`), Go-Live-Builder-Anteile aus `bot/live_announce` (`rust/docs/cutover-backlog.md:25`).

## 2. Intent-Ledger: bewusste Abweichungen

| Kategorie | Abweichung | Begründung / Entscheidung | Beleg |
|---|---|---|---|
| dropped | Dashboard Preview/NOAUTH-Debug nicht 1:1 portiert. | Owner-Entscheidung: natives Dashboard statt altem Debug/Preview-Pfad. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:29`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:33` |
| dropped | Go-Live-/Live-Announcement-Builder entfernt. | User-Entscheidung vom 2026-06-23: Builder raus; Auto-Post/Role-Ping bleibt. | `rust/docs/cutover-backlog.md:25`, `rust/docs/cutover-backlog.md:31`, Commit `c8ada2a` |
| dropped | Eigene PDF-Rechnungen/Gutschrift-PDFs/Affiliate-Payout-Detail nicht portiert. | Billing läuft über Stripe-hosted/Connect; eigene PDF-/Payout-Schicht nicht Ziel. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:49`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:57`, `rust/docs/04-cutover-plan.md:352` |
| dropped | Windows-Keyring/alte `abbo_entry`/Python-Token-Tools nicht portiert. | Linux/systemd/Infisical-Betrieb; alte Windows-/Manuellpfade entfallen. | `rust/docs/05-cleanup-decisions.md:59`, `rust/docs/05-cleanup-decisions.md:66`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:373` |
| dropped | EventSub WebSocket-U66-Pool nicht portiert. | ADR setzt Webhook-only; WS-Pool war Test/Notfallfläche. | `rust/docs/adr/0004-eventsub-webhook-only.md:17`, `rust/docs/adr/0004-eventsub-webhook-only.md:24`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:124` |
| dropped | Eigener Discord-Gateway/Bot nicht in Rust. | Discord läuft über Master-Broker/Bridge, nicht als eigener Gateway-Client. | `rust/docs/adr/0001-discord-via-bridge.md:17`, `rust/docs/adr/0001-discord-via-bridge.md:34`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:101` |
| dropped | Discord-Raid-Commands und `!raid_enable`-OAuth-Chatkommando nicht portiert. | OAuth/Raid-Aktivierung läuft über Dashboard/Flow; alte Commands bewusst raus. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:159`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:171`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:195` |
| dropped | Discord-DM-Aktivierungen/Community-Buttons/Leaderboard-Buttons nicht portiert. | Discord-Aktionen werden reduziert bzw. durch Broker/Statusflächen ersetzt. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:181`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:303`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:324` |
| dropped | One-shot-Python-Migrationen nicht portiert. | Finales Schema liegt in SQL-Migrationen; Startup-Ensure-Schema nicht 1:1. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:290`, `rust/docs/05-cleanup-decisions.md:7`, `rust/docs/adr/0002-db-sqlx-refinery-shared-schema.md:14` |
| dropped | OpenAI-Pfade nicht portiert. | Rust-Ziel: MiniMax + Anthropic; OpenAI raus. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:365`, `rust/crates/tb-llm/src/lib.rs:1` |
| dropped | Auto-Upload/Clip-Creation/Transcription nicht default-aktiv. | Social/Highlight-Worker sind opt-in und brauchen Aktivierungsdoku/Test. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:329`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:337`, `rust/docs/audit/2026-06-19-rust-cutover-stabilisierung.md:37` |
| dropped | `manual_verified`/Legacy-Fallback entfernt. | Commit bereinigt manuelle Verifikation und alte Fallback-Env. | Commit `5210d0d`, Commit `6114776` |
| dropped | Scam-Guard deutsche FP-Antwort reverted. | Owner wollte keine Auto-Ban-Erweiterung für deutsche FP-Fälle. | Commit `544e906`, Commit `9b32c1b`, Commit `0c463cc` |
| changed | Dashboard wird als native Rust/API-Fläche statt Flask/SSR geführt. | Strangler/Cutover ersetzt Python-Server schrittweise durch native Routen. | `rust/docs/04-cutover-plan.md:1`, `rust/docs/04-cutover-plan.md:97`, `rust/bin/tb-dashboard/src/main.rs:47` |
| changed | Dashboard-Auth härter als Python: Session-Gate, CSRF, Same-Origin, Fingerprint. | Bewusste Security-Modernisierung statt 1:1 Legacy-Cookies. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:64`, `rust/crates/tb-dashboard-api/src/auth/csrf.rs:3`, `rust/crates/tb-dashboard-api/src/auth/session.rs:401` |
| changed | DB-Zugriff: SQLx-Migrationen statt `ensure_schema`/Pool-Godclass. | Sauberer Rust-Schema-Stand; Godclasses splitten. | `rust/docs/05-cleanup-decisions.md:7`, `rust/docs/05-cleanup-decisions.md:19`, `rust/docs/adr/0002-db-sqlx-refinery-shared-schema.md:14` |
| changed | Crypto-Interop bevorzugt AES oder Reauth, nicht Fernet-Rebuild. | Falls Python-Fernet nicht nutzbar: bewusster Relogin statt unsauberer Nachbau. | `rust/docs/adr/0003-crypto-interop-or-reauth.md:18`, `rust/docs/adr/0003-crypto-interop-or-reauth.md:44` |
| changed | Bot-Token wird nach OAuth in Infisical zurückgeschrieben. | Python schrieb den Bot-Token nicht; Rust verbessert Betriebsparität. | `rust/docs/adr/0005-bot-token-infisical-writeback.md:8`, `rust/docs/adr/0005-bot-token-infisical-writeback.md:35`, `rust/docs/adr/0005-bot-token-infisical-writeback.md:93` |
| changed | Billing/Entitlements minimal und Stripe-hosted. | Trial/Entitlement-Lesen bleiben, eigene UI/Sync-Produkte nicht 1:1. | `rust/docs/04-cutover-plan.md:352`, `rust/docs/04-cutover-plan.md:363`, `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:58` |
| changed | EventSub wird nativ per Webhook empfangen, nicht Python-Bridge/WS. | Native Receiver ersetzt Bridge; Revocations werden geloggt. | `rust/docs/04-cutover-plan.md:446`, `rust/docs/04-cutover-plan.md:484`, `rust/crates/tb-monitoring/src/lib.rs:1` |
| changed | Raid/Chat-Auth nutzt zentrale OAuth-/Scope-Logik statt altem Chatkommando. | Scope-/Auth-Parität später gezielt nachgezogen. | Commit `259adbb`, `rust/bin/tb-bot/src/raid_oauth_impl.rs:1599`, `rust/bin/tb-bot/src/wiring.rs:192` |
| changed | Lurker-Tax/Promo-Suppression persistiert und gate-orientiert. | Mehr Betriebsrobustheit gegenüber nur In-Memory-Python. | Commit `66ba3cc`, `rust/bin/tb-bot/src/chat_wiring.rs:550`, `rust/bin/tb-bot/src/chat_wiring.rs:570` |
| changed | IRC/Chatter-Presence später erweitert. | Python-Parität für anonyme IRC-Breite und Silent-Errors nachgezogen. | Commit `baede53`, Commit `b8bcda4`, Commit `2074be2` |
| changed | Partner-Raid-Score/Retention/Chatters laufen als periodische Rust-Tasks. | Monitoring/Score-Snapshots nicht mehr Python-Loop. | Commit `8685dcd`, Commit `b8bcda4`, `rust/bin/tb-bot/src/main.rs:794` |
| changed | Admin-Streamer- und Legacy-Formrouten sind nativ. | Frühere P0/P1-Admin-Flächen wurden als Rust-Handlers ergänzt. | Commit `2b1ebb3`, Commit `3619f86`, `rust/crates/tb-dashboard-api/src/handlers/admin_legacy_streamers.rs:57` |
| changed | Market-/Website-/Admin-Discord-Routen sind nativ statt Python-Fallback. | Spätere Commits schließen Hauptdomain-/Market-/Discord-Login-Parität. | Commit `3592c56`, Commit `654659f`, Commit `c8ada2a` |
| replaced | systemd/Infisical/journald ersetzen PID-Lock, Hot-Reload und lokale Secret-Dateien. | Rust-Betrieb folgt Service-/Secret-Manager-Architektur. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:373`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:379` |
| replaced | Discord-DMs durch Broker-/Dashboard-/Internal-API-Flächen ersetzt. | Kein eigener Discord-Client; reduzierte DM-Abhängigkeit. | `rust/docs/adr/0001-discord-via-bridge.md:17`, `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:1`, `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:9` |
| replaced | Affiliate minimal über Stripe Connect statt eigener Payout-Strecke. | Nur Kern/Minimal-Flow bleibt. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:53`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:57` |
| replaced | Dashboard-SSR-Altseiten werden auf SPA/native Routen umgelegt. | Alte `/twitch/partners` und Raid-Analytics-SSR nur bei Bedarf wiederbauen. | `rust/docs/cutover-backlog.md:3`, `rust/docs/cutover-backlog.md:16` |
| replaced | Python-Mixins/Godclasses werden fachlich gesplittet. | Rust-Crates tragen Ownership statt Mixin-Vererbung. | `rust/docs/05-cleanup-decisions.md:19`, `rust/crates/tb-chat/src/lib.rs:25`, `rust/crates/tb-raid/src/lib.rs:30` |
| replaced | Promo-/Announcement-Admin wurde als native Config-Endpunkte umgesetzt. | Spätere Rust-Routen ersetzen alte Admin-API-Handler, ohne alte Flask-Form. | `rust/crates/tb-dashboard-api/src/handlers/admin_promo_mode.rs:1`, `rust/crates/tb-dashboard-api/src/handlers/admin_announcements.rs:1`, `rust/crates/tb-dashboard-api/src/handlers/admin_config.rs:82` |
| replaced | Reauth-All SQL-Operation existiert ohne Discord-DM-Schleife. | P3.7-Port bewusst ohne DM; Port muss injiziert sein, sonst 503. | `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:1`, `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:9`, `rust/bin/tb-bot/src/main.rs:1513` |
| deferred | VoiceReaction/Community-Buttons Phase 6g. | Dokumentiert als späterer Scope. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:312`, `rust/docs/04-cutover-plan.md:403` |
| deferred | Externe Recruitment-Followups bei Raid-Arrival. | Flags/Mechanik erst später vollständig; Maintenance-Loop inzwischen teils da. | `rust/docs/05-cleanup-decisions.md:102`, `rust/docs/05-cleanup-decisions.md:122`, `rust/bin/tb-bot/src/main.rs:794` |
| deferred | Stream-Coaching-Audit. | Als späterer Punkt dokumentiert. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:423` |

## 3. Re-Status der letzten Vollaudit

Quelle der alten Befunde: `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:11` nennt P0=1, P1=57, P2=145, P3=29.

### P0/P1 vollständig

| ID | Kurztitel | Severity | Aktueller Status | Beleg |
|---|---:|---|---|---|
| P0.1 | `add_streamer` fehlt | P0 | BEHOBEN | Commit `2b1ebb3`; `rust/crates/tb-dashboard-api/src/handlers/admin_legacy_streamers.rs:57` |
| P1.1 | Outbound-Suppression Channel-Settings | P1 | BEHOBEN | Commit `a7ce86b`; `rust/bin/tb-bot/src/chat_wiring.rs:550` |
| P1.2 | Auto-Mod-Retry nach Mod-Verlust | P1 | BEHOBEN | Commit `2b60cdc`; `rust/bin/tb-bot/src/main.rs:441` |
| P1.3 | Scam-Server-Warnung | P1 | FP | Bewusst reverted nach Owner-Wunsch: Commits `544e906`, `9b32c1b`, `0c463cc` |
| P1.4 | Lurker-Tax Bot-Token-Fallback | P1 | BEHOBEN | Commit `a7ce86b`; `rust/bin/tb-bot/src/chat_wiring.rs:572` |
| P1.5 | bekannte Chatbots exkludieren | P1 | BEHOBEN | Commit `a7ce86b`; `rust/crates/tb-chat/src/promos.rs:1652` |
| P1.6 | OAuth-Prefix Strip Bot-Token | P1 | BEHOBEN | Commit `a7ce86b`; `rust/crates/tb-chat/src/token.rs:1274` |
| P1.7 | Followers Total mit User-Token | P1 | BEHOBEN | Commit `f7d3aab`; `rust/bin/tb-bot/src/wiring.rs:168`, `rust/bin/tb-bot/src/main.rs:360` |
| P1.8 | Auto-Scope Partner/Auth-Row | P1 | BEHOBEN | Commit `259adbb`; `rust/bin/tb-bot/src/raid_oauth_impl.rs:1599` |
| P1.9 | Partner Raid-Score Refresh | P1 | BEHOBEN | Commit `e664748`; `rust/bin/tb-bot/src/main.rs:804` |
| P1.10 | Effective Viewer Count Fallback | P1 | BEHOBEN | Commit `6f54df1`; `rust/bin/tb-bot/src/raid_arrival_wiring.rs:681` |
| P1.11 | Partner Network-Raid Delivery | P1 | BEHOBEN | `rust/bin/tb-bot/src/raid_arrival_wiring.rs:372`, `rust/bin/tb-bot/src/raid_arrival_wiring.rs:881` |
| P1.12 | Chat Raid/Unraid Correlation | P1 | BEHOBEN | `rust/bin/tb-bot/src/chat_wiring.rs:1256`, `rust/bin/tb-bot/src/eventsub_hooks.rs:198` |
| P1.13 | Expected-Partner Override | P1 | BEHOBEN | Commit `e664748`; `rust/crates/tb-raid/src/arrival_confirmation.rs:1073` |
| P1.14 | External Bot-Ban Due Loop | P1 | BEHOBEN | `rust/bin/tb-bot/src/main.rs:794`, `rust/bin/tb-bot/src/main.rs:802` |
| P1.15 | Live-Announcement UI-Normalisierung | P1 | OFFEN | `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:5`, `rust/crates/tb-monitoring/src/announce/sink.rs:122` |
| P1.16 | Mention-Sanitize Mixed Case | P1 | BEHOBEN | Commit `2b60cdc`; `rust/crates/tb-monitoring/src/announce/template.rs:847` |
| P1.17 | Revocation Side Effects | P1 | BEHOBEN | Commit `2b60cdc`; `rust/bin/tb-bot/src/main.rs:1010` |
| P1.18 | Revocation Untrack/Restart | P1 | BEHOBEN | Commit `2b60cdc`; `rust/crates/tb-monitoring/src/webhook_receiver.rs:330` |
| P1.19 | Follower Fetch App-Token-only | P1 | BEHOBEN | Commit `21c3603`; `rust/bin/tb-bot/src/wiring.rs:232` |
| P1.20 | Core-Sub Revocation Resubscribe | P1 | BEHOBEN | Commit `2b60cdc`; `rust/bin/tb-bot/src/main.rs:1010` |
| P1.21 | Analytics Subs/Ads Loop | P1 | BEHOBEN | Commit `45c7673`; `rust/bin/tb-bot/src/main.rs:1070` |
| P1.22 | Ad-Schedule Snapshot Poller | P1 | BEHOBEN | Commit `45c7673`; `rust/crates/tb-analytics/src/ads_schedule_collector.rs:10` |
| P1.23 | Presence Ticks | P1 | BEHOBEN | Commit `b8bcda4`; `rust/crates/tb-monitoring/src/chatters_poller.rs:1` |
| P1.24 | Raid Retention Hourly Loop | P1 | BEHOBEN | Commit `b8bcda4`; `rust/crates/tb-monitoring/tests/raid_retention.rs:1` |
| P1.25 | EventSub Telemetry DateTime/Text | P1 | BEHOBEN | Commit `2b60cdc`; `rust/crates/tb-monitoring/tests/support/mod.rs:210` |
| P1.26 | Affiliate Portal HTML | P1 | BEHOBEN | Commit `65533b1`; `rust/crates/tb-dashboard-api/src/handlers/affiliate_portal.rs:160` |
| P1.27 | `streamCount` Key | P1 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/overview.rs:320` |
| P1.28 | Demo AI Schema | P1 | BEHOBEN | Commit `c9e0a44`; `rust/crates/tb-dashboard-api/src/handlers/demo.rs:258` |
| P1.29 | Admin OAuth Scopes Endpoint | P1 | BEHOBEN | Commit `65533b1`; `rust/crates/tb-dashboard-api/src/handlers/system/oauth_scopes.rs:1` |
| P1.30 | Affiliate Gutschrift Trigger | P1 | FP | Eigene Gutschrift/PDF-Strecke bewusst nicht portiert: `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:57`, `rust/docs/04-cutover-plan.md:363` |
| P1.31 | Roadmap CRUD | P1 | BEHOBEN | Commit `65533b1`; `rust/crates/tb-dashboard-api/src/lib.rs:694` |
| P1.32 | Changelog CSRF Origin/Header | P1 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:2119` |
| P1.33 | Raid Metric Null Login | P1 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:1026` |
| P1.34 | Admin Streamer Session Keys | P1 | BEHOBEN | Commit `c9e0a44`; `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:1018` |
| P1.35 | Heatmap/Stats Timestamptz/Text | P1 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/performance.rs:506` |
| P1.36 | Viewer Segments Home Channel | P1 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/viewers.rs:1378` |
| P1.37 | `abbo_cancel` GET | P1 | BEHOBEN | Commit `c2ad150`; `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:313` |
| P1.38 | `abbo_cancel` CSRF | P1 | BEHOBEN | Commit `c2ad150`; `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:318` |
| P1.39 | Forward Auth schwach | P1 | BEHOBEN | Commit `0252003`; `rust/crates/tb-dashboard-api/src/handlers/forward_auth.rs:20` |
| P1.40 | Promo Self-Service Handler | P1 | BEHOBEN | `rust/crates/tb-dashboard-api/src/handlers/admin_promo_mode.rs:1` |
| P1.41 | Custom Promo Message Editor | P1 | BEHOBEN | `rust/crates/tb-dashboard-api/src/handlers/admin_announcements.rs:1` |
| P1.42 | Promo-Settings Route | P1 | BEHOBEN | `rust/crates/tb-dashboard-api/src/handlers/admin_promo_mode.rs:20` |
| P1.43 | Promo-Message Route | P1 | BEHOBEN | `rust/crates/tb-dashboard-api/src/handlers/admin_announcements.rs:39` |
| P1.44 | Checkout Preview | P1 | BEHOBEN | Commit `c2ad150`; `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:572` |
| P1.45 | Trial Start X-CSRF | P1 | BEHOBEN | `rust/crates/tb-dashboard-api/src/auth/partner_gate.rs:223`, `rust/crates/tb-dashboard-api/src/lib.rs:553` |
| P1.46 | Add/Remove Legacy Form Aliases | P1 | BEHOBEN | Commit `2b1ebb3`; `rust/crates/tb-dashboard-api/src/handlers/admin_legacy_streamers.rs:124` |
| P1.47 | Live Announcement Dashboard | P1 | FP | Builder bewusst entfernt: `rust/docs/cutover-backlog.md:25`, Commit `c8ada2a` |
| P1.48 | Config Save Path Guard | P1 | BEHOBEN | Commit `4641e33`; `rust/crates/tb-monitoring/src/announce/template.rs:847` |
| P1.49 | Live Announcement Builder | P1 | FP | Builder bewusst entfernt: `rust/docs/cutover-backlog.md:25`, `rust/docs/cutover-backlog.md:31` |
| P1.50 | Annual `bonus_months` | P1 | BEHOBEN | Commit `89162cb`; `rust/crates/tb-analytics/src/stripe/webhook_apply.rs:626` |
| P1.51 | Raid Auth Route | P1 | BEHOBEN | Commit `65533b1`; `rust/crates/tb-dashboard-api/src/handlers/raid_pages.rs:1` |
| P1.52 | Raid Go Route | P1 | BEHOBEN | Commit `65533b1`; `rust/crates/tb-dashboard-api/src/handlers/raid_pages.rs:82` |
| P1.53 | Partner Login Cookie | P1 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/handlers/partner_login.rs:207` |
| P1.54 | Durable Partner Session | P1 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/auth/session.rs:2598` |
| P1.55 | Shared Discord Admin Callback | P1 | BEHOBEN | Commit `654659f`; `rust/bin/tb-dashboard/src/main.rs:97` |
| P1.56 | OAuth Login reaktiviert Partner | P1 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:341` |
| P1.57 | Discord Admin Login Flow | P1 | BEHOBEN | Commit `654659f`; `rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs:1` |

### P2 vollständig

| ID | Kurztitel | Severity | Aktueller Status | Beleg |
|---|---:|---|---|---|
| P2.1 | Service-Warning Tuning-Env | P2 | OFFEN | Nur feste Log-Konstanten sichtbar: `rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:47`, alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:957` |
| P2.2 | Escalation-Timeout Chatnachricht | P2 | OFFEN | Aktueller Sendepfad deckt Timeout-Pitch, nicht Escalation-Notice: `rust/bin/tb-bot/src/chat_wiring.rs:1220`, alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:973` |
| P2.3 | `manual_partner_opt_out` Outbound-Guard | P2 | OFFEN | Opt-out ist in Admin-Chat-Action gegatet, aber kein globaler Outbound-Guard belegt: `rust/crates/tb-dashboard-api/src/handlers/admin_chat_action.rs:186`, alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:989` |
| P2.4 | Service-Warning Log Flush | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.5 | Auto-Ban Notice respektiert Opt-out | P2 | OFFEN | Alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:1021`; kein P2.5-Codeanker im aktuellen `rg` |
| P2.6 | Lurker DB Text/Timestamptz | P2 | BEHOBEN | Commit `4641e33`; `rust/crates/tb-monitoring/tests/write_core.rs:291` |
| P2.7 | Lurker DB Write-Fallback | P2 | BEHOBEN | Commit `4641e33`; `rust/crates/tb-monitoring/src/sessions/store.rs:221` |
| P2.8 | Promo Suppression Expiry | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.9 | Targeted Promo Suppression | P2 | BEHOBEN | Commit `66ba3cc`; `rust/bin/tb-bot/src/chat_wiring.rs:550` |
| P2.10 | Stale EventSub Cleanup 403 | P2 | OFFEN | Logik existiert, Wiring-TODO offen: `rust/crates/tb-monitoring/src/subscriptions.rs:1379` |
| P2.11 | Raid-enabled-only Channels joinen | P2 | BEHOBEN | `rust/bin/tb-bot/src/chat_wiring.rs:2295` |
| P2.12 | Invite Cooldown erst nach Send | P2 | BEHOBEN | Commit `47befde` |
| P2.13 | Invite koppelt Last-Promo | P2 | BEHOBEN | Commit `47befde` |
| P2.14 | Eager Invite Backfill | P2 | BEHOBEN | `rust/bin/tb-bot/src/main.rs:953`, `rust/bin/tb-bot/src/chat_wiring.rs:1954` |
| P2.15 | Chat-Notification Sub-Fallback | P2 | BEHOBEN | `rust/bin/tb-bot/src/chat_wiring.rs:1266` |
| P2.16 | `!uban` restart-sicher | P2 | BEHOBEN | Commit `e56478f` |
| P2.17 | `!raid started` nennt Ziel | P2 | BEHOBEN | Commit `47befde` |
| P2.18 | Lurker-Tax Announcement/Opt-out | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.19 | Lurker Aggregation Identity-Key | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.20 | New-Chatter-Gate API-Viewer | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.21 | Invite Sent schreibt `last_sent_at` | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.22 | Lurker eigene Channelquelle | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.23 | Lurker nicht Promo-Block-gated | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.24 | Promo Channel Allowed Gate | P2 | BEHOBEN | Commit `66ba3cc` |
| P2.25 | Helix `/users` Bot-ID Fallback | P2 | BEHOBEN | Commit `e56478f` |
| P2.26 | Helix transient retry | P2 | BEHOBEN | Commit `14a0909`; `rust/crates/tb-transport-twitch/src/client.rs:480` |
| P2.27 | Helix 403 kein retry | P2 | BEHOBEN | Commit `14a0909`; `rust/crates/tb-transport-twitch/src/client.rs:555` |
| P2.28 | Clear failure clears pause | P2 | BEHOBEN | Commit `305eb2f`; `rust/crates/tb-raid/tests/token_blacklist.rs:188` |
| P2.29 | Stale pending cleanup sweep | P2 | BEHOBEN | Commit `305eb2f`; `rust/crates/tb-raid/src/pending_raids.rs:316` |
| P2.30 | Orphan replay pending raid | P2 | BEHOBEN | Commit `305eb2f`; `rust/crates/tb-raid/src/auto_raid_pipeline.rs:272` |
| P2.31 | Pending timeout diagnostics | P2 | BEHOBEN | Commit `305eb2f`; `rust/crates/tb-raid/src/pending_raids.rs:153` |
| P2.32 | Discord-linked identity source | P2 | BEHOBEN | Commit `259adbb` |
| P2.33 | Invalid-client circuit breaker | P2 | BEHOBEN | Commit `14a0909`; `rust/crates/tb-transport-twitch/src/user_token.rs:148` |
| P2.34 | Bulk reauth primitive | P2 | BEHOBEN | Commit `28f1bb4`; `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:20` |
| P2.35 | Observability/retry gap | P2 | BEHOBEN | Commit `c7f556c` |
| P2.36 | Open-session fallback | P2 | BEHOBEN | Commit `6f54df1`; `rust/bin/tb-bot/src/confirm_resolver.rs:95` |
| P2.37 | Open-session duplicate | P2 | BEHOBEN | Commit `6f54df1`; `rust/bin/tb-bot/src/confirm_resolver.rs:95` |
| P2.38 | TEXT-tolerante session timestamps | P2 | BEHOBEN | Commit `305eb2f`; `rust/crates/tb-monitoring/src/sessions/store.rs:221` |
| P2.39 | Auth login-fallback gate | P2 | BEHOBEN | Commit `259adbb` |
| P2.40 | Score refresh no stale scores | P2 | BEHOBEN | Commit `21c3603`; `rust/crates/tb-raid/src/score_store.rs:197` |
| P2.41 | Score fairness/detail | P2 | BEHOBEN | Commit `21c3603`; `rust/crates/tb-raid/src/scoring.rs:4` |
| P2.42 | Score observability | P2 | BEHOBEN | Commit `7120028` |
| P2.43 | Score refresh wiring | P2 | BEHOBEN | Commit `21c3603` |
| P2.44 | Raid score cache status | P2 | BEHOBEN | Commit `7120028` |
| P2.45 | Score retry diagnostics | P2 | BEHOBEN | Commit `7120028` |
| P2.46 | Token normalization | P2 | BEHOBEN | `rust/bin/tb-bot/src/wiring.rs:192` |
| P2.47 | External recruitment blacklist due | P2 | BEHOBEN | `rust/bin/tb-bot/src/main.rs:794`, `rust/bin/tb-bot/src/main.rs:802` |
| P2.48 | Raid auth refresh/gate | P2 | BEHOBEN | Commit `28f1bb4` |
| P2.49 | Raid EventSub lifecycle | P2 | BEHOBEN | `rust/bin/tb-bot/src/main.rs:699` |
| P2.50 | Followers bot-token scope gate | P2 | BEHOBEN | `rust/bin/tb-bot/src/wiring.rs:220` |
| P2.51 | EventSub context fallbacks | P2 | BEHOBEN | Commit `4641e33`; `rust/docs/audit/_work/implementation-plan-2026-06-21.md:458` |
| P2.52 | Durable `channel.raid` inbox | P2 | BEHOBEN | Commit `4641e33`; `rust/bin/tb-bot/src/eventsub_hooks.rs:198` |
| P2.53 | EventSub login resolution | P2 | BEHOBEN | Commit `4641e33` |
| P2.54 | Durable inbox duplicate | P2 | BEHOBEN | Commit `4641e33`; `rust/docs/audit/_work/implementation-plan-2026-06-21.md:468` |
| P2.55 | Go-live clears offline throttle | P2 | BEHOBEN | Commit `4641e33`; `rust/crates/tb-monitoring/src/handlers.rs:140` |
| P2.56 | Moderator telemetry broadcaster-token | P2 | BEHOBEN | `rust/bin/tb-bot/src/main.rs:469`, `rust/crates/tb-monitoring/src/subscriptions.rs:877` |
| P2.57 | Bot self-timeout -> TimeoutGuard | P2 | BEHOBEN | `rust/bin/tb-bot/src/main.rs:957`, `rust/bin/tb-bot/src/chat_wiring.rs:398` |
| P2.58 | Raid readiness waits enabled | P2 | BEHOBEN | `rust/bin/tb-bot/src/main.rs:699`, `rust/bin/tb-bot/src/raid_adapters.rs:375` |
| P2.59 | Storage write retry SQLSTATEs | P2 | BEHOBEN | Commit `4641e33` |
| P2.60 | Monitoring rest parity | P2 | BEHOBEN | Commit `4641e33` |
| P2.61 | Chatters poller identity | P2 | BEHOBEN | Commit `b8bcda4`; `rust/crates/tb-monitoring/src/chatters_poller.rs:1` |
| P2.62 | Observability stale pending | P2 | BEHOBEN | Commit `c7f556c` |
| P2.63 | Subs snapshot poller | P2 | BEHOBEN | Commit `45c7673` |
| P2.64 | Chatters poller wiring | P2 | BEHOBEN | Commit `2074be2` |
| P2.65 | Presence logger parity | P2 | BEHOBEN | Commit `b8bcda4`; `rust/crates/tb-monitoring/src/irc_lurker.rs:632` |
| P2.66 | Social admin SPA | P2 | BEHOBEN | `rust/crates/tb-social-media/src/lib.rs:42` |
| P2.67 | Social website | P2 | BEHOBEN | `rust/crates/tb-social-media/src/lib.rs:1` |
| P2.68 | Bot filters in analytics | P2 | BEHOBEN | Commit `7f8f113` |
| P2.69 | Audience/session aggregation | P2 | BEHOBEN | Commit `f16dd28` |
| P2.70 | Usage ledger | P2 | BEHOBEN | Commit `45c7673` |
| P2.71 | Stream report | P2 | BEHOBEN | Commit `f16dd28` |
| P2.72 | Demo data schema | P2 | BEHOBEN | Commit `c9e0a44` |
| P2.73 | Demo AI data | P2 | BEHOBEN | Commit `c9e0a44` |
| P2.74 | System static endpoint | P2 | BEHOBEN | Commit `65533b1` |
| P2.75 | Admin DB health | P2 | BEHOBEN | Commit `d4722ad` |
| P2.76 | Admin system health | P2 | BEHOBEN | Commit `d4722ad` |
| P2.77 | OAuth query UI/API | P2 | BEHOBEN | Commit `65533b1` |
| P2.78 | OAuth scope payload | P2 | BEHOBEN | Commit `65533b1`; `rust/crates/tb-dashboard-api/src/handlers/system/oauth_scopes.rs:87` |
| P2.79 | OAuth scope security | P2 | BEHOBEN | Commit `65533b1` |
| P2.80 | Admin streamer views | P2 | BEHOBEN | Commit `c9e0a44`; `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:196` |
| P2.81 | System errors masking | P2 | BEHOBEN | Commit `65533b1`; `rust/crates/tb-dashboard-api/src/handlers/system/errors.rs:264` |
| P2.82 | DB diagnostics | P2 | BEHOBEN | Commit `d4722ad` |
| P2.83 | Internal home stats | P2 | BEHOBEN | Commit `0b5c475` |
| P2.84 | Admin system snapshots | P2 | BEHOBEN | Commit `d4722ad` |
| P2.85 | Dashboard hardening | P2 | BEHOBEN | Commit `5e2ae4b` |
| P2.86 | Rate limit security | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/auth/security.rs:241` |
| P2.87 | Admin system details | P2 | BEHOBEN | Commit `d4722ad` |
| P2.88 | Title performance/admin detail | P2 | BEHOBEN | Commit `d4722ad` |
| P2.89 | Monitoring write core | P2 | BEHOBEN | Commit `4641e33` |
| P2.90 | Dashboard readout parity | P2 | BEHOBEN | Commit `d4722ad` |
| P2.91 | Transport OAuth cooldown | P2 | BEHOBEN | Commit `14a0909`; `rust/crates/tb-transport-twitch/src/user_token.rs:41` |
| P2.92 | Audience demographics | P2 | BEHOBEN | Commit `7f8f113` |
| P2.93 | Follower funnel | P2 | BEHOBEN | Commit `f16dd28` |
| P2.94 | Audience overlap | P2 | BEHOBEN | Commit `f16dd28` |
| P2.95 | Audience bot filter | P2 | BEHOBEN | Commit `7f8f113` |
| P2.96 | Audience null handling | P2 | BEHOBEN | Commit `7f8f113` |
| P2.97 | Audience demographics ordering | P2 | BEHOBEN | Commit `7f8f113` |
| P2.98 | Raid analytics aggregation | P2 | BEHOBEN | Commit `f16dd28` |
| P2.99 | Dashboard readout misc | P2 | BEHOBEN | Commit `d4722ad` |
| P2.100 | Verification-result Discord DM | P2 | OFFEN | Kein aktueller P2.100-Codeanker; Discord-DM-Verzicht nur für Reauth belegt: `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:9`, alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:2525` |
| P2.101 | Title rank/live context | P2 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/title.rs:130` |
| P2.102 | `include_live` lädt Live-State | P2 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/title.rs:241` |
| P2.103 | Stripe checkout legal text | P2 | BEHOBEN | Commit `c2ad150`; `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:58` |
| P2.104 | Market route group | P2 | BEHOBEN | Commit `3592c56`; `rust/crates/tb-dashboard-api/src/handlers/market.rs:6` |
| P2.105 | Market data aggregation | P2 | BEHOBEN | Commit `3592c56`; `rust/crates/tb-dashboard-api/src/handlers/market.rs:137` |
| P2.106 | Market share wrapper | P2 | BEHOBEN | Commit `3592c56`; `rust/crates/tb-dashboard-api/src/handlers/market.rs:461` |
| P2.107 | Discord validate-session fallback | P2 | FP | Native Discord-Admin-Session ersetzt externen Fallback: Commit `654659f`; `rust/bin/tb-dashboard/src/main.rs:97`, `rust/crates/tb-dashboard-api/src/auth/session.rs:19` |
| P2.108 | Default security headers | P2 | BEHOBEN | `rust/crates/tb-dashboard-api/src/lib.rs:44`, `rust/crates/tb-dashboard-api/src/lib.rs:1256` |
| P2.109 | Lurker-tax readiness warning | P2 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs:80` |
| P2.110 | Promo toggle/config route | P2 | BEHOBEN | `rust/crates/tb-dashboard-api/src/handlers/admin_promo_mode.rs:20` |
| P2.111 | Lurker-tax paid gate | P2 | FP | Lurker-Tax bewusst als eigenständige Partner-Setting-Fläche: `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:219`, `rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs:80` |
| P2.112 | Admin Discord profile write | P2 | BEHOBEN | Commit `2b1ebb3`; `rust/crates/tb-dashboard-api/src/handlers/admin_legacy_streamers.rs:187` |
| P2.113 | Stripe sync self-heal | P2 | BEHOBEN | Commit `70f09b8`; `rust/crates/tb-dashboard-api/src/handlers/billing_stripe_sync.rs:23` |
| P2.114 | Stripe sync response maps | P2 | BEHOBEN | Commit `e34b0ce`; `rust/crates/tb-dashboard-api/src/handlers/billing_stripe_sync.rs:33` |
| P2.115 | DACH market page | P2 | BEHOBEN | Commit `3592c56`; `rust/crates/tb-dashboard-api/src/handlers/market.rs:53` |
| P2.116 | Market data endpoint | P2 | BEHOBEN | Commit `3592c56`; `rust/crates/tb-dashboard-api/src/handlers/market.rs:137` |
| P2.117 | Live announcement routes | P2 | FP | Go-Live Builder bewusst entfernt: `rust/docs/cutover-backlog.md:25`, `rust/docs/cutover-backlog.md:31` |
| P2.118 | Raid web pages/admin login | P2 | BEHOBEN | Commit `6ef94aa`; `rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs:2` |
| P2.119 | Internal admin owner gate | P2 | BEHOBEN | Commit `cf0e9e2`; `rust/crates/tb-internal-api/src/handlers/python_stubs.rs:202` |
| P2.120 | Admin partner chat action | P2 | BEHOBEN | Commit `2b1ebb3`; `rust/crates/tb-dashboard-api/src/handlers/admin_chat_action.rs:1` |
| P2.121 | Discord link duplicate | P2 | BEHOBEN | Commit `2b1ebb3`; `rust/crates/tb-dashboard-api/src/handlers/admin_legacy_streamers.rs:187` |
| P2.122 | Live announcement config | P2 | FP | Builder bewusst entfernt: `rust/docs/cutover-backlog.md:25`, `rust/docs/cutover-backlog.md:31` |
| P2.123 | Live announcement save | P2 | FP | Builder bewusst entfernt: `rust/docs/cutover-backlog.md:25`, `rust/docs/cutover-backlog.md:31` |
| P2.124 | Leaderboard Discord IDs | P2 | BEHOBEN | Commit `0b5c475`; `rust/crates/tb-dashboard-api/src/handlers/leaderboard.rs:96` |
| P2.125 | Catalog payment keys | P2 | BEHOBEN | Commit `c2ad150`; `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:715` |
| P2.126 | Stripe vault override layer | P2 | BEHOBEN | Commit `89162cb`; `rust/crates/tb-analytics/src/billing/catalog.rs:496` |
| P2.127 | Score refresh on billing change | P2 | BEHOBEN | Commit `89162cb`; `rust/crates/tb-analytics/src/stripe/webhook_apply.rs:445` |
| P2.128 | Subscription sync score refresh | P2 | BEHOBEN | Commit `89162cb`; `rust/crates/tb-dashboard-api/src/handlers/billing_webhook.rs:219` |
| P2.129 | Manual plan score refresh | P2 | BEHOBEN | Commit `89162cb`; `rust/crates/tb-dashboard-api/src/handlers/admin_manual_plan.rs:232` |
| P2.130 | Native raid history/analytics | P2 | BEHOBEN | Commit `6ef94aa`; `rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs:2` |
| P2.131 | Partner session type mismatch | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/auth/session.rs:2598` |
| P2.132 | Admin/Affiliate OAuth states | P2 | OFFEN | Discord-Admin ist nativ, Affiliate-State nicht belegt: `rust/bin/tb-dashboard/src/main.rs:97`, alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3039` |
| P2.133 | Partner auth rate limiting | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/lib.rs:822` |
| P2.134 | Admin session Same-Origin | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/handlers/partner_login.rs:87` |
| P2.135 | Partner-resolution gate | P2 | FP | Bewusste Auth-/Partner-Gate-Härtung: `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:64`, `rust/crates/tb-dashboard-api/src/auth/session.rs:1376` |
| P2.136 | Rate-limit duplicate | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/lib.rs:822` |
| P2.137 | Redirect URI validation | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:668` |
| P2.138 | OAuth start rate-limit | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/lib.rs:731` |
| P2.139 | OAuth context-token CSRF | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:39` |
| P2.140 | OAuth callback rate-limit | P2 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/lib.rs:731` |
| P2.141 | Text length chars not bytes | P2 | BEHOBEN | Commit `cf0e9e2`; `rust/crates/tb-internal-api/src/handlers/telemetry_routes.rs:1191` |
| P2.142 | Mark-member loose coercion | P2 | BEHOBEN | Commit `cf0e9e2`; `rust/crates/tb-internal-api/src/handlers/streamers.rs:1290` |
| P2.143 | Internal streamers idempotency | P2 | BEHOBEN | Commit `cf0e9e2`; `rust/crates/tb-internal-api/src/handlers/streamers.rs:422` |
| P2.144 | Discord log idempotency Non-ASCII | P2 | BEHOBEN | Commit `cf0e9e2`; `rust/crates/tb-internal-api/src/handlers/self_explainer_log.rs:129` |
| P2.145 | Debug eventsub-processing route | P2 | BEHOBEN | Commit `cf0e9e2`; `rust/crates/tb-internal-api/src/handlers/telemetry_routes.rs:1270` |

### P3-Zählung und echte Funktionsverluste

P3 gesamt: 29 (`rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:11`). Einzelprüfung nur für P3 mit realem Funktionsverlust:

| ID | Kurztitel | Severity | Aktueller Status | Beleg |
|---|---:|---|---|---|
| P3.4 | Unauthorized feedback | P3 | BEHOBEN | Commit `47befde` |
| P3.5 | Raid history endpoint | P3 | BEHOBEN | Commit `d4722ad`; `rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs:2` |
| P3.7 | Discord bulk `reauth_all` | P3 | OFFEN | Handler existiert, Port in Bot noch TODO: `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:1`, `rust/bin/tb-bot/src/main.rs:1513` |
| P3.13 | Blacklist-Raid Whisper | P3 | OFFEN | Alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3459`; aktueller Raid-Blacklist-Code filtert, aber kein Whisper-Beleg: `rust/crates/tb-raid/src/target_resolution.rs:66` |
| P3.18 | X-Admin-token/Auth-Pfad | P3 | BEHOBEN | Commit `86c8161`; `rust/crates/tb-dashboard-api/src/auth/csrf.rs:135` |
| P3.20 | `manualPartnerOptOut` Detail | P3 | BEHOBEN | Commit `c9e0a44`; `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:1018` |
| P3.22 | Runtime Bot-Logins ausgeschlossen | P3 | OFFEN | Alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3604`; kein aktueller P3.22-Codeanker im `rg` |
| P3.23 | Billing route details | P3 | BEHOBEN | Commit `c2ad150`; `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:715` |
| P3.24 | Billing profile route | P3 | BEHOBEN | Commit `c2ad150`; `rust/crates/tb-dashboard-api/src/handlers/billing_profile.rs:11` |
| P3.28 | Port-bind retry/backoff | P3 | OFFEN | Alter Befund `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3701`; kein aktueller Backoff-Beleg im `rg` |
| P3.29 | Dashboard misc readout | P3 | BEHOBEN | Commit `d4722ad` |

## 4. Offene-Punkte-Startliste

| Offener Punkt | Status/Notiz | Beleg |
|---|---|---|
| Live-Announcement UI->Template-Normalisierung | P1.15 bleibt offen; Dashboard-Config-Mapping ist nur Stub/Wrapper. | `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:5`, `rust/crates/tb-monitoring/src/announce/sink.rs:122` |
| EventSub stale cleanup | P2.10 bleibt offen; `cleanup_stale` hat explizites Wiring-TODO. | `rust/crates/tb-monitoring/src/subscriptions.rs:1379` |
| Service-Warning Tuning Env | P2.1 bleibt offen; nur feste Dashboard-Log-Konstanten belegt. | `rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:47` |
| Escalation-Timeout-Chatnachricht | P2.2 bleibt offen. | `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:973`, `rust/bin/tb-bot/src/chat_wiring.rs:1220` |
| Globaler Outbound Opt-out/Suppression-Guard | P2.3/P2.5 bleiben als globaler Sendepfad-Risiko offen. | `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:989`, `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:1021` |
| Verification-result Discord-DM | P2.100 bleibt offen/nicht bewusst als Drop belegt. | `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:2525` |
| Affiliate OAuth-State-Parität | P2.132 bleibt offen, Discord-Admin nativ, Affiliate-State nicht verifiziert. | `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3039`, `rust/bin/tb-dashboard/src/main.rs:97` |
| Reauth-All Composition-Wiring | P3.7: Handler vorhanden, Bot injiziert Port noch nicht. | `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:1`, `rust/bin/tb-bot/src/main.rs:1513` |
| Blacklist-Raid Whisper | P3.13: Raid-Blacklist-Filter vorhanden, Whisper-Benachrichtigung nicht belegt. | `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3459`, `rust/crates/tb-raid/src/target_resolution.rs:66` |
| Port-bind Retry/Backoff | P3.28 bleibt offen. | `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3701` |
| External Recruitment Followups | Deferred-Liste nennt Arrival-Followups Phase 6g; Due-Maintenance ist inzwischen verdrahtet, Followup-Scope bleibt Prüfpunkt. | `rust/docs/05-cleanup-decisions.md:102`, `rust/docs/05-cleanup-decisions.md:122`, `rust/bin/tb-bot/src/main.rs:794` |
| VoiceReaction | Phase 6g/deferred. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:312`, `rust/docs/04-cutover-plan.md:403` |
| Engagement-Feed/Shadow Review | Späterer Follow-up nach Chat-Flip. | `rust/docs/04-cutover-plan.md:403`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:399` |
| Social/Highlight Worker Aktivierung | Worker sind opt-in; Aktivierung braucht Doku/Test. | `rust/docs/audit/2026-06-19-rust-cutover-stabilisierung.md:6`, `rust/docs/audit/2026-06-19-rust-cutover-stabilisierung.md:37` |
| Raid-Analytics SPA-Wiring | Backlog nennt alte SSR-Redirects und offene SPA-Wiring-Frage. | `rust/docs/cutover-backlog.md:16`, `rust/docs/cutover-backlog.md:23` |
| Stream-Coaching-Audit | Als späterer Punkt dokumentiert; kein klares Rust-Pendant in Abschnitt 1. | `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:423`, `bot/stream_coaching_audit/service.py:168` |

## Top-Risiken / Lücken für Tiefenprüfung

1. `P1.15` Live-Announcement-Normalisierung ist der einzige verbliebene P1-Open-Status und verdient eine gezielte UI->Template-Diff-Prüfung (`rust/crates/tb-monitoring/src/announce/dashboard_config.rs:5`).
2. `P2.10` EventSub-Stale-Cleanup hat eine implementierte Funktion, aber explizit kein periodisches Wiring (`rust/crates/tb-monitoring/src/subscriptions.rs:1379`).
3. Globale Outbound-Gates (`P2.3/P2.5`) sind riskant, weil einzelne Admin-/Promo-Pfade Guards haben, aber kein einheitlicher Sendepfad-Beleg existiert (`rust/crates/tb-dashboard-api/src/handlers/admin_chat_action.rs:186`).
4. `P3.7` kann im aktuellen Internal-API-Pfad 503 liefern, wenn der Reauth-Port nicht injiziert wird (`rust/bin/tb-bot/src/main.rs:1513`).
5. Discord-DM-/Whisper-Abweichungen sind teils bewusst, teils nicht sauber dokumentiert; `P2.100` und `P3.13` sollten gegen Owner-Intent geklärt werden (`rust/crates/tb-internal-api/src/handlers/reauth_all.rs:9`, `rust/docs/audit/2026-06-21-py-rust-parity-fullaudit.md:3459`).
6. Worker-Aktivierung für Highlight/Social/Transcription ist bewusst aus; vor Aktivierung fehlen dokumentierte Tests und Betriebsentscheidungen (`rust/docs/audit/2026-06-19-rust-cutover-stabilisierung.md:37`).
7. `bot/stream_coaching_audit` und VoiceReaction bleiben klare Nicht-Pendant-/Deferred-Kandidaten (`rust/docs/audit/2026-06-15-grillme-entscheidungen.md:312`, `rust/docs/audit/2026-06-15-grillme-entscheidungen.md:423`).
8. Alte Backlog-Notizen enthalten stale und echte offene Punkte gemischt; ein Cleanup-Pass sollte Backlog, Audit-Status und Code-TODOs synchronisieren (`rust/docs/05-cleanup-decisions.md:124`, `rust/docs/cutover-backlog.md:16`).

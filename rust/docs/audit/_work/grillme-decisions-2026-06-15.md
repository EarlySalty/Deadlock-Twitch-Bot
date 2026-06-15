# Grillme-Entscheidungen — Python→Rust Parity-Audit Twitch-Bot (2026-06-15)

Lebende Mitschrift der User-Entscheidungen pro Block. Legende: BUILD-P0/P1/P2 = nativ nachbauen (Priorität), DROP = nicht portieren, KLÄREN = offen/später, MODERN = bauen mit bewusster Modernisierung.

---

## Block 1 — Dashboard Admin/Partner-Verwaltung

**Rahmen:** Nativer Nachbau steht fest (Welle D, Pflicht-Block #1). Architektur-Richtung (in Impl-Planung zu fixieren): Admin als **SPA + native JSON-Endpoints**, NICHT server-gerendertes HTML 1:1 kopieren — gleiches Verhalten, weniger Code.

### BUILD-P0 (Pflicht)
- `live-1` + `dashboard-composition-root-06` — Admin-Oberfläche `/twitch` + `/twitch/admin` SPA + `validate_admin_session` (forward_auth)
- `dashboard-pages-misc-4` + `dashboard-live-announcement-mixin-4` — Entry-/Redirect-/Auth-Routen (sonst lädt die SPA nicht)
- `live-2` + `live-8` — `add_streamer` + Varianten (add_any/url/login)
- `live-3` — `verify` (Partner-Status permanent/30T/failed/clear)
- `live-4` — `archive`/`unarchive`/`remove` (Partner deaktivieren)
- `live-7` — `manual-plan` save/clear (Billing-Override)
- `live-5` — `discord_flag` + `discord_link`

### BUILD-P1
- `dashboard-live-announcement-mixin-2` — Title-Generator-Routen suggest/insights (nur Route aufs lebende Backend; Memory: Title-Gen komplett+live)
- `live-6` — Owner-Chat-Action **MODERN/erweiterbar**: Partner können eigene Aktionen/Inhalte neben den unsrigen einbauen
- `dashboard-live-announcement-mixin-5` (Teil) — **Lurker-Tax**-Einstellung bauen, aber **UI woanders platzieren** (Ort TBD)

### BUILD-P2
- `raidpages-1` `/twitch/raid/history`, `raidpages-2` `/twitch/raid/analytics`, `raidpages-3` raid-oauth-Seiten, `raidpages-4` `/twitch/raid/requirements`
- `dashboard-pages-misc-5` — `system/errors`

### DROP
- `dashboard-live-announcement-mixin-1` — Live-Announcement-Builder (gesamtes Mixin/UI)
- `dashboard-live-announcement-mixin-5` (Teil) — **Promo** + Promo-Message-Einstellungen

### BUILD-P2 (2. Runde aufgelöst)
- `dashboard-core-stats-1` (+ `dashboard-core-templates-4`) — `/twitch/partners` Admin-Cross-Partner-Statistik
- `mixin-3` + `dashboard-pages-misc-1` — Market-Research `/twitch/market`
- `dashboard-pages-misc-3` — Roadmap-Kanban `/twitch/admin/roadmap`
- `dashboard-composition-root-07` — Demo-Embed `/demo/*` (öffentlicher Marketing-Demo; Caddy-Route auf totes 8765 mit umbiegen)

### DROP (2. Runde)
- `dashboard-spa-artefakte-1` — dashboard_preview Dev-Sandbox (nie ausgerollt)
- `dashboard-composition-root-09` — NOAUTH-Debug-Modus (Security, kein Nutzen)

**→ Block 1 ABGESCHLOSSEN.**

---

## Block 2 — Affiliate / Billing / Stripe

### 2A Abo/Billing — BUILD (Umsatz-Pfad, Stripe-hosted wo möglich)
- BUILD: `billing-mixin-1`+`abbo-billing-2` Stripe-Webhook → Entitlements (Quelle der Wahrheit fürs Bezahlt-Sein); `abbo-billing-3` Checkout-Start (Redirect zu Stripe); `abbo-billing-1` `/twitch/abbo`; `abbo-billing-4` Kündigen; `billing-mixin-2`+`abbo-billing-10/11` Katalog/Readiness/Product-Price-Sync; `billing-mixin-5/6` Customer-Prefill/Price-Map; `billing-mixin-7` Checkout-Redirect Host-Allowlist (Security)
- STRIPE-HOSTED (DROP eigene Engine): `billing-mixin-3` Invoice-Render, `abbo-billing-5/12` Rechnungs-Download/-Seite, `abbo-billing-6` Rechnungsdaten → Stripe Customer-Portal + hosted invoices nutzen
- Promo/Lurker-Tax-Toggles (`abbo-billing-7/8/9`) → siehe Block 1 (Lurker-Tax bauen, Promo raus)

### 2B Affiliate-Programm — NEUBAUEN, aber MINIMAL über Stripe
**Directive (User):** Affiliate bleibt, aber so wenig wie möglich selbst bauen — Auszahlung/Gutschriften laufen über **Stripe Connect**. Erst das Nötigste (MVP), dann ausbauen.
- BUILD (minimal/MVP): `affiliate-portal-01` Affiliate-Login, `affiliate-portal-02` Stripe-Connect-Onboarding, `affiliate-shim-2`/`affiliate-portal-03` Provisions-Erfassung (30% bei Zahlung), `affiliate-portal-06` Read-API/Portal, `affiliate-portal-05` PII nur soweit für Stripe-Connect/Payout nötig
- DROP / via Stripe Connect: `affiliate-portal-04` + `affiliate-shim-3` + `api_admin-3` + `affiliate-portal-07` — eigene Gutschrift-PDF-Engine (fpdf2) + USt/Kleinunternehmer-Logik + 6h-Loop + manuelle Auszahlung → Stripe Connect Payout/Tax übernimmt das
- Themenfremd (woanders entscheiden): `affiliate-shim-6` (Raid-Dashboard-Seiten → Block 1/7), `analytics-streamer-affiliate-audit-1/2/3` (Streamer-Admin-Detail → Block 1/10)

**→ Block 2 ABGESCHLOSSEN.** (User: „Entscheidungen soweit gut")

---

## Block 3 — Dashboard-Auth / Login / Session
**Directive (User):** Bauen, aber besser/modernisiert (Industriestandard). „Sicherheit ist kein Luxus sondern notwendig", solange sauber gebaut + funktional.

### BUILD (modernisiert, Industriestandard)
- `auth-core-3` Discord-Link-Flow (Partner verknüpft Discord)
- `auth-core-4` + `shim-1` + `shim-5` — Partner-Einmal-Login (HMAC One-Time-Token) + PartnerLoginToken/Access/Binding-Services
- **Session/Cookie:** „eingeloggt bleiben"-UX identisch, aber Industriestandard-Implementierung (HttpOnly/Secure/SameSite signierte Session-Cookies, saubere Expiry + Silent-Refresh statt Eigenbau)

### SECURITY (wieder einführen, Industriestandard)
- `auth-core-6` echtes CSRF-Token (statt immer null)
- `legal-self-explainer-4` + ALLE Schreib-Aktionen (Partner add/remove, verify, manual-plan, Billing) → CSRF-Schutz
- Grundsatz: Security industrie-standard, solange Funktion sauber bleibt

### FIX (Zustand)
- `shim-3` Partner-Access-Cookie konsumieren (sonst Partner ausgesperrt)
- `auth-core-8` Admin+Partner-Doppel-Cookie-Merge
- `auth-core-9` Session-Gate bei jedem Request → **Rust-Verhalten behalten** (sicherer, bewusste Modernisierung)
- `shim-7` + `dashboard_service-app-bootstrap-2` Strangler-Fallback auf totes 8765 → abschalten/umbiegen, sobald Routen nativ

### DROP
- (keine)

### KORREKTUR (User)
- `shim-2` Device-/Canvas-Fingerprinting nach Admin-Login → **BUILD** (Teil der Admin-Login-Härtung). User: „Admin ist wichtig, da Security zu haben wäre super." → Admin-Auth bekommt höchste Security-Priorität: CSRF + signierte Sessions + Fingerprinting.

**→ Block 3 ABGESCHLOSSEN** (Admin-Security = Priorität).

---

## Block 4 — Token-Lifecycle (api)
**Directive (User):** „Sieht gut aus." Build-Suite approved.

### BUILD (Token-Ausfall-Reaktionen, via Discord-Broker)
- `token-lifecycle-1` Admin-Channel-Embed bei Token-Fehler (1×/Streamer)
- `token-lifecycle-2` User-DM an Streamer + persistenter Reconnect-Button
- `token-lifecycle-5` + `-6` Bot-in-Channel-gebannt → Auto-Opt-out + Recovery-DM + automatische Aufhebung bei Health-Restore
- `token-lifecycle-3` 7-Tage-Grace → Streamer-Rollenentzug + Reminder-DM + Admin-Notify (stündlicher Scheduler) — **Policy bleibt**
- `token-lifecycle-4` Blacklist-Cleanup >30 Tage (Scheduler 3.5h)
- `token-lifecycle-8` `generate_oauth_tokens` (Bot-Token authorization_code-Exchange) → **behalten**: nötig, um Bot-Token mit NEUEN Scopes neu zu autorisieren (Refresh kann Scopes nicht ändern)

### SCHEMA
- `token-lifecycle-7` Spalten grace_expires_at/user_dm_sent/reminder_sent/role_removed in Rust-Migrations sicherstellen

### DROP
- `token-lifecycle-9` Windows-Keyring-Token-Persistenz (Linux-Umgebung, irrelevant)

**Architektur-Hinweis:** Alle Discord-Reaktionen (DM/Rolle/Embed) laufen über den zentralen Discord-Bot/Broker (Twitch-Bot hat keinen Discord-Zugang).

**→ Block 4 ABGESCHLOSSEN.**

---

## Block 5 — EventSub-/Telemetrie-Subscriptions + Inbox-Robustheit
**Directive (User):** „Rest sieht gut aus und muss migriert werden." U66 (WS) droppen, da webhook-only.

### BUILD — verlorene Daten-Subscriptions
- `eventsub-mixin-1` `channel.chat.user_first_message`-Sub (First-Message-Events)
- `eventsub-mixin-2` `follow`/`ban`/`unban`/`shoutout`-Subs (Follower-Funnel, Ban-Analytics, Shoutouts)
- `eventsub-mixin-4` Neue Partner prompt subscriben (statt erst 6h-Reconcile)

### FIX/BUILD — Inbox-/Dead-Letter-Robustheit
- `inbox-guard-1` Dead-Letter-Alarm verdrahten
- `inbox-guard-2` + `telemetry-3` Requeue-Endpoint an natives `requeue_dead_letter` anschließen
- `inbox-guard-3` + `telemetry-1` + `app-routing-1` Dead-Letter-Debug/Snapshot-Endpoint bauen
- `monitoring-poll-1` Capacity-Snapshot-Zeitreihe + Retention; `eventsub-mixin-3` Offline-Seiteneffekt-Reihenfolge; `inbox-guard-4` Storage-Retry-Wrapper → mitmigrieren

### DROP (bestätigt) — webhook-only (ADR-0004)
- `U66-1` + `U66-2` EventSub-WebSocket-Transport + Pool, `api-helix-5` subscribe_eventsub_websocket

### Verschoben
- `api-helix-2/3/4` (Helix ads/subscriptions/chatters) → Block 6 · `streamers-crud-4/5` → Block 10 · `eventsub-mixin-6` (Go-Live-Werbefrei-Pitch) → Chat/Promo-Block

**→ Block 5 ABGESCHLOSSEN.**

---

## Block 6 — Analytics-Poller
**Directive (User):** „Alles fixen / migrieren."

### BUILD
- `analytics-api_v2-mixin-1` + `api-helix-4` Chatters-Lurker-Poller (30s `/chat/chatters`) → Lurker-Daten
- `analytics-api_v2-mixin-2` + `api-helix-2/3` Subs/Ads-Snapshot-Poller (6h) → Monetization/Abo/Ad-Panel
- `demo-data-1` öffentlicher Demo → migrieren, aber **lean** (Demo-Daten schlank neu aufbauen, NICHT `demo_data.py` 3589 LOC verbatim portieren) — konsistent mit „weniger Code"

### KEEP (bewusster Rust-Fix, kein Handlungsbedarf)
- `analytics-insights-1` Monetization echte Bits/Subs-Zahlen (Rust > Python, Migrations-Fix)

**→ Block 6 ABGESCHLOSSEN.**

---

## Block 7 — Raid-Subsystem  (TEILWEISE — Drops + Discord-Befehle noch offen)

### BUILD — funktionaler Kern
- `raid-arrival-tracking-01` chat.notification-Raidmeldung → Arrival-Runtime
- `raid-scores-tracking-1` resolve_partner_raid_tracking_for_session
- `raid-auth-01` + `raid-core-2` proaktiver Hintergrund-Token-Refresh
- `raid-arrival-tracking-02/03/05` + `raid-facades-1` Unraid-Handling + Source-Self-Unraid-Cancel + Pending-Timeout-Sweeper
- `raid-signal-pending-01` PendingRaid.target_stream_data-Feld
- `raid-auth-04` **FIX (geklärt):** Re-Auth reaktiviert Partner **auch OHNE discord-id** (raid_bot_enabled/opt_out/backfill nicht an discord-id koppeln)

### DEFER → Phase 6g (Recruiting) — keine Entscheidung jetzt, mehr Daten nötig
- `raid-blacklist-partner-setup-1`, `raid-facades-3`, `raid-recruit-msg-1/2/3`, `raid-pipeline-stores-04` → bei 6g sehr detailliert besprechen

### Discord-Befehle (aufgelöst)
- `raid-core-4` (/raid_status,/raid_history) + `raid-core-7` (/traid,/raid_enable,/check-auth,/check-scopes) → **DROP** (unnötig; Dashboard + Website-Flow decken ab)
- `raid-core-5` (/reauth_all) + `raid-core-6` (/sendchatpromo,!tte) → **BUILD ins Admin-Dashboard, aus Discord entfernen**
- `raid-views-arrival-1` Aktivierungs-DM → **VERIFY** ob zentraler Discord-Bot (Deadlock-Bots) die Views noch bedient

### Diagnostik (aufgelöst)
- `raid-cand-1` Follower-Tie-Break → **BUILD/FIX** (war gute Lösung; Follower-Anreicherung + Tie-Break wiederherstellen, ggf. modernisieren)
- `raid-signal-pending-03` iter-API → **BUILD** (Source-Unraid-Cancel-Abhängigkeit)
- `raid-pipeline-stores-03` + `raid-signal-pending-02` → **DROP** (Rust löst es sauberer, kein Funktionsverlust)
- `raid-facades-2` + `raid-runtime-glue-2` Observability → **BUILD ins Admin-Dashboard, niedrige Prio (P3)**

### Aufgelöst (Rest)
- `raid-partner-delivery-02` → **KEEP Rust** (DB-Fallback: `!raid` auch kurz nach Stream-Ende; bewusste moderne Entscheidung, als solche dokumentieren)
- `raid-views-arrival-1` (Discord-Aktivierungs-DM `views.py`) → bleibt **DROP** (durch Website-/streamer-Flow ersetzt). **Klarstellung:** Die vom User gemeinte **Raid-Arrival-Analyse** (bleiben Viewer nach Raid, wie lange, wer) ist eine andere Sache und wird gebaut: `raid-arrival-tracking-01` + `raid-scores-tracking-1` + natives Dashboard-Raid-Analytics (`raid_analytics.rs`; SQLX-Fixes in Block 16).

**→ Block 7 ABGESCHLOSSEN.**

---

## Block 8 — Chat-Connection
**Directive (User):** `!raid_enable` gibt's nicht mehr („in oder raus"). IRC-Chat WICHTIG (Lurken in fremden Chats → Daten sammeln) — nicht killen. Rest fixen.

### FIX
- `connection-subscriptions-1` Kanal-Selektion an „Partner in/raus" anpassen (raid_enabled-only-Konzept entfällt mit `!raid_enable`)
- `connection-subscriptions-2` 403-Mod-Retry-Recovery beim Join
- `connection-subscriptions-3` Bot-Ban-Blacklisting beim Join
- `ban-sweep-lurker-02` is_partner-Predicate in Roster-Query nachziehen

### KEEP/BUILD — IRC-Transport (Daten-Sammlung fremder Chats)
- `chat-event-pipeline-05` TwitchIO-IRC-Transport NICHT droppen → behalten. **IRC = Chat-LESEN in Nicht-Partner-Kanälen ≠ EventSub-WebSocket (U66, bleibt gedroppt). Kein Widerspruch.**

### BUILD
- `chat-event-pipeline-01` Sub/Resub/Gift-Telemetrie aus chat.notification
- `chat-event-pipeline-02` Raid-Korrelation aus chat.notification (= `raid-arrival-tracking-01`)
- `ban-sweep-lurker-03` Passive-Lurker-Tracking (`lurker_policy.py`)
- `chat-event-pipeline-06` Streamer-Invite-Erzeugung via Broker (Onboarding)

### KEEP Rust (Verbesserung)
- `ban-sweep-lurker-01` Rust schreibt aufgelöste numerische `chatter_id` in global_ban → behalten (robusteres Matching als Login)

### ACCEPT / DROP (alt, Rust funktioniert)
- `connection-subscriptions-5/6` Dormant-State/part_channels-Trigger-Modell → Rust-Webhook-Modell ok
- `chat-event-pipeline-04` Fun-Response (dormant, Feature aus) → dormant lassen
- `chat-commands-tokens-04` `!raid_enable` OAuth-Link → **DROP** (Command entfällt, „in oder raus")

**Ban-Sweep-Semantik (User):** Raid-Ban = wer keinen Bock auf uns hat → in Ruhe lassen (kein Raid). Ban-Sweep = harte Version: bei echtem Konflikt netzwerkweiter Ausschluss.

**→ Block 8 ABGESCHLOSSEN.**

---

## Block 9 — Chat-Commands / Promos / Lurker-Tax / Scam
### FIX / BUILD
- `promos-engine-6` **FIX (smart):** volle Plan-Snapshot-Resolution — Plan-Ablauf (`manual_plan_expires_at`) UND **Promo-Disable-Entitlement** respektieren. **User-Betonung: Pläne, die Promo abschalten, dürfen NICHT ignoriert werden.** (verknüpft mit Entitlement-Katalog → Block 16)
- `chat-commands-tokens-06` **BUILD:** `!lurkersteuer_off` Chat-Befehl + **Dashboard-Toggle für ALLE Partner, default DEAKTIVIERT** (Lurker-Tax opt-in — bewusste Zustands-Entscheidung)
- `chat-commands-tokens-05` `!clip` Bot-Token-Fallback → fixen
- `chat-commands-tokens-07` `!clip`-Fehlertext + tote `!raid_enable`-Referenz raus → fixen
- `chat-commands-tokens-09` `TWITCH_BOT_TOKEN_FILE`-Loader → verifizieren ob Prod-genutzt, dann unterstützen
- `scam-pitch-spam-review-3` **BUILD:** forensischer Audit-Trail (Datei `twitch_service_warnings.log`) behalten
- `scam-pitch-spam-review-4` **BUILD:** MiniMax-Usage-Ledger weiterführen
- `scam-pitch-spam-review-5` Timeout-Reason → Rust akzeptieren (klarer)

### KEEP Rust + TODO
- `promos-engine-4` Lurker-Tax-Scope → **erstmal nur Deadlock-live** (Rust). **TODO:** später systemweit (jeder Live-Stream) ausbauen, wenn gut angenommen.

**→ Block 9 ABGESCHLOSSEN.**

---

## Block 10 — Internal-API Streamer-CRUD + Loopback-Guard
### FIX — Streamer-CRUD-Lifecycle vervollständigen (Backend zu Block-1-Aktionen)
- `streamers-crud-1` Remove: Discord-Rolle entfernen + Raid-Auth-Disable + Identity-Upsert (nicht nur `archived`)
- `streamers-crud-2` Verify→Partner: Promote + Kategorie-Stats-Backfill
- `streamers-crud-3` Verify clear/failed: Departner + Rolle entfernen (+ DM bei failed)
- `streamers-crud-4/5` Kontext-Meldungen, History/Reactivate, `require_link`+`next_link_check_at`, EventSub-Supervisor-Trigger, Stats-Backfill
- `streamers-crud-6` List-Response-Shape → an Live-Consumer angleichen (verify)

### FIX — Security-Härtung (Industriestandard)
- `policy-contracts-idempotenz-1` + `app-routing-2/3` + `policy-…-2` Loopback-Origin+Peer-Guard + Token-Vergleich exakt wie Python
- `policy-contracts-idempotenz-3` JSON-Serialisierungs-Parität (datetime/Decimal/UUID/set)

### BUILD
- `internal_api-runner-client-2` **Split-Deployment-Härtung** (Start verweigern bei Rolle≠twitch_worker / Port≠8776) — User: sinnvoll
- `streamers-crud-8` Bot-Chat-Send via **Bot-Token-Bridge** → QUERSCHNITTS-CAPABILITY (bedient auch `live-6`/Block 1 + Chat-Send allgemein)

### Schon abgedeckt / minor
- debug/eventsub (`app-routing-1`,`telemetry-1/3`) → Block 5 · `/raid/requirements` (`raid-routes-01`) → Block 7 (gedroppt)
- `app-routing-5` DB-Fingerprint-Logging, `internal_api-runner-client-3` Thin-Subclass → accept/low

**→ Block 10 ABGESCHLOSSEN.**

---

## QUERSCHNITTS-DIREKTIVE (User, ab Block 11)
**SQL/Schema sauber & idiomatisch nativ in Rust bauen — NICHT Pythons `ensure_schema` verbatim/„cringe-debug" nachbauen.** Gilt für die gesamte Schema-/Migrations-/SQL-Schicht: versionierte sqlx-Migrations mit sauberem DDL statt imperativem Python-Konstrukt. Gleiches Ziel-Schema, sauberer Weg.

---

## Block 11 — Storage / Crypto / Schema / Sessions

### BUILD — Fundament (sauber nativ, nicht Python-Port)
- `storage-core-pool-rows-pg-1` Rust-Migrations VOLLSTÄNDIG: ~60 Tabellen + 2 Views + Identity-Sync-Trigger + Timescale-Hypertable — clean Rust
- `storage-core-pool-rows-pg-3` Serial-Sequenzen + Flag-Coercion (User deferred → mein Ermessen: bauen)
- `storage-core-pool-rows-pg-4` Write-TX-Retry bei serialization_failure/deadlock (DB-Resilienz)

### BUILD — Login-Infra (Block-3-Dependency)
- `storage-sessions-fernet-crypto-2` **Session-Erstellung beim OAuth-Login** (ohne das kein Login)
- `storage-sessions-fernet-crypto-1` Login-**Rate-Limiter** (Brute-Force-Schutz)

### BUILD — Partner-Registry Write/Reactivate (Overlap Block 10, notwendig)
- `storage-partner-registry-4/6/7` — `-7`: Hard-Kills (blocked/bot_banned) bei Reactivate respektieren

### KEEP / DROP
- `fernet-crypto-4` Session-TTL **hardkodiert 6h lassen** (kein Env-Override)
- `fernet-crypto-5` Keyring-Fallback **DROP** — Keys kommen aus dem Infisical-Tresor
- `storage-small-modules…raidpause-1` Admin-Auto-Raid-Pause **DROP** (in Python tot, null Caller)
- `partner-registry-8/9` accept · `storage-core-pool-rows-pg-2` Observability low-prio (mit Block 7) — User deferred

**→ Block 11 ABGESCHLOSSEN.**

---

## Block 12 — Migrations: Schema-Ownership-Cutover  (User: „klingt gut")
### BUILD — Schema-Endzustand in die sauberen Rust-Migrations
- `social-media-phase0-4-schema-1` Social-Media-Tabellen (Phase 0–4) — sonst bricht frische DB
- `social-media-phase0-4-schema-2` OAuth-Persistenz `consumed_at` TIMESTAMPTZ + 2 Indizes
- `social-media-phase0-4-schema-3` Auto-Approve-Settings-Keys (Schema-Parität)
- `exp-migrate-1/2`, `migrations-infra-oneshots-1` End-Tabellen (exp_sessions/exp_snapshots, viewer_presence_ticks) in Migrations sicherstellen
### DROP — One-Shot-Transform-Werkzeuge (bereits gelaufen, Endzustand in Migrations)
- `migrations-infra-oneshots-2` (Hypertable-Umbau), `-3` (Crypto-Cleanup) + Backfill-Skripte → nicht als Code portieren

**→ Block 12 ABGESCHLOSSEN.**

---

## Block 13 — Community
- `partner_recruit-1/2/3/6` (remove/add/verify-Lifecycle) → **abgedeckt durch Block 10** (Streamer-CRUD, voller Lifecycle)
- `partner_recruit-5` (Notify-Channel, Forcecheck `_tick`, Discord-Invite-Cache) → **DEFER Phase 6g** (Recruiting)
- `community/leaderboard #2` interaktiver Discord-Leaderboard (9 Buttons) → **DROP aus Discord**
- Leaderboard **sauber neu im Dashboard** (Web), inkl. `community/leaderboard #1` Stat-Felder (retention/chat/discovery/content_performance) → **BUILD, Low-Prio**

**→ Block 13 ABGESCHLOSSEN.**

---

## Block 14 — voice_reaction
Komplettes Subsystem (~2900 LOC: Audio-Capture/Whisper/Conversation-Brain/Scheduler/Persistenz), in Python **default-AUS**, gekoppelt an Outreach/Recruiting.
- `voice_reaction-1/2/3/4` → **DEFER → Phase 6g** (nicht jetzt bauen, **für später festhalten**, mit Recruiting gemeinsam entscheiden). NICHT droppen.

**→ Block 14 ABGESCHLOSSEN.**

---

## Block 15 — social_media
**Directive (User):** Keine Discord-DMs mehr. Clip-ERSTELLUNG erstmal ganz AUS (Clips 0 Qualität, „geht nicht"). Clip-System baubar, aber Erstellung deaktiviert. Transkription raus (für Clips überflüssig).

### DROP — Discord-DMs
- `approval-1`, `social_media_unit47-1` Clip-Approval-DM · `social-token-refresh-worker-nachtrag-1`, `social_media-oauth-creds-3` Admin-Reauth-DM → keine Discord-DMs

### OFF / DROP
- **Clip-ERSTELLUNG (Highlight-Clipper, auto-Clips) → default-AUS** (Qualität schlecht); System-Infra baubar, Erstellung deaktiviert → revisit Block 20
- `transcription-1` faster_whisper + `transcription-3` seed_vocab → **DROP/OFF** (überflüssig, Clip-Pipeline aus)
- Social-Media-Auto-Upload-Pipeline bleibt **dormant**, solange Clip-Erstellung aus

### ACCEPT (konsistent mit früheren Blöcken)
- `llm-layer-1` MiniMax-Ledger weiterführen · `llm-layer-2`+Keyring → Infisical/Env · `social-analytics-1` Wochenreport-DM drop (B10-DMs)

### FIX low-prio (greifen erst bei reaktivierter Pipeline)
- `social_media-2` Unauth→Login-Redirect · `social_media-3/5` Audit-Felder/twitch_user_id-Backfill · `uploaders-1` YouTube-Token-Refresh · `social_media-1` Layout · `social_media-4` libmagic-MIME

**→ Block 15 ABGESCHLOSSEN.**

---

## Block 16 — Analytics-Divergenzen
### ACCEPT — bewusste Rust-Änderungen
- `viewers-1` + `overview-raidmetrics-03` → **bewusstes neues Design** (Tagesperformance frei, Rest hinter Paywall). KEIN Python-Restore. **CAVEAT: server-seitige Paywall-Durchsetzung verifizieren** (echter API-Gate, nicht nur UI-versteckt — sonst per Direkt-Request umgehbar).
- `coaching-1`, `raid-stack-3` Rundung (banker's vs half-away) → **Rust akzeptieren** (nur bei exakt ,5, ±1 letzte Stelle, vernachlässigbar)
- `perf-4` NULLS-Order, `internal_home-2` Logfile-Count, `coaching-2`/`backend-02` (Insight-Texte) → Rust ok

### FIX — Korrektheit (unabhängig vom Paywall-Design)
- `viewers-2/3` Bot-/Streamer-Selbst-Exklusion (Bots zählen sonst als Viewer)
- `api_admin-4` + `mixin-5` + `ai-history-…-04` CSRF/Rate-Limit/Auth-Gate (konsistent Block 3)
- null-statt-0 + 403-Shape: `mixin-7`,`perf-3`,`viewers-5`,`audience-3`,`overview-…-04`,`post-stream-3` (Frontend-Korrektheit)
- `analytics-perf-6` Entitlement-Gate (Streamer statt Auth-User), `internal_home-1` Twitch-display_name, `post-stream-1` Rating-Key

### BUILD
- `api_chat_deep-1` MiniMax-Chat-Deep-Analyse → bauen

### DROP / abgedeckt
- `analytics-backend-03` AnalyticsBackend → **nicht bauen** (toter Code schon in Python, keine Aufrufer)
- `U14-1` Roadmap → Block 1 (P2) · `api_admin-3` Gutschriften → Block 2 · MiniMax-Ledger (`post-stream-2`) → Block 9

**→ Block 16 ABGESCHLOSSEN.**

---

## QUERSCHNITTS-DIREKTIVE 2 (User, ab Block 17) — LLM-Provider
**OpenAI raus. Nur Anthropic (Premium/ai_full) + MiniMax (Primär — „alles über MiniMax betreiben").** Gilt für ALLE AI-Features: chat_deep, scam-review, title-gen, engagement, coaching, post-stream-AI. (Transkription/Whisper bereits gedroppt → kein OpenAI-Whisper.)

---

## Block 17 — core_runtime
### BUILD — LLM-Clients
- `core_runtime-3` llm_providers → MiniMax + Anthropic-Clients, **OpenAI droppen**
### DROP — Rust/systemd/Infisical lösen das (= Modernisierung, kein Verlust)
- **Hot-Reload** (`core_runtime-03`, `base-composition-8`, `core_runtime-2`) → DROP (Rust kann nicht modul-hot-reloaden ohne Plugin/dylib-Umbau; systemd-Restart reicht, geringer Impact)
- PID-Lock (`runtime_lock`/`runtime_pid_lock`) → systemd · Keyring (`core_runtime-06`, field_crypto) → Infisical · `core_runtime-5` partner_utils → toter Code
### BUILD — funktional/Security/Ops
- Role/Port-Hardening (`runtime_mode`/`enforce_internal_api_runtime`, konsistent Block 10) · Discord-**Alert-Channel** (`base-composition-7`, via Broker) · **Graceful Shutdown** (`core_runtime-3`) · **IRC-Lurker verdrahten** (`base-composition-6`+`core_runtime-4`, konsistent Block 8) · Scout-Vollständigkeit (`base-composition-5`) + `_sync_missing_user_ids`-Backfill
### ACCEPT (Rust-nativ)
- File-Logging → **journald** (kein Datei-Logging) · http_client → reqwest · Exit-Codes · Role-Sync-Gates (`core_runtime-04/05`) · `promo_mode`-Helfer

**→ Block 17 ABGESCHLOSSEN.**

---

## Block 18 — monitoring-Rest  (User: +)
### FIX — Security/Robustheit
- `65.2` Webhook-Replay-Schutz (>600s → 403) · `65.3` Dispatch-Ready-Gate · `monitoring-poll-2` `is_auth_blocked`-Circuit-Breaker
### FIX — Korrektheit
- `partner_ops-1` Reconcile auf `is_partner_active` (statt `status='active'`-Superset) · `partner_ops-2` Debounce-Set · `monitoring-poll-3` Sprachfilter (DE + */any-Bypass) · `embeds_mixin-2` Thumbnail-Fallback
### ACCEPT (Rust korrekt/bewusst)
- `embeds_mixin-3` Umlaut · `embeds_mixin-4` keine ungenutzte Ping-Rolle anlegen · `monitoring-poll-4` language-Spalte · `sessions-3` Dev-Obs
### Abgedeckt
- `sessions-2` → Block 7

**→ Block 18 ABGESCHLOSSEN.**

---

## Block 19 — engagement-KI
**Directive (User):** Engagement-KI BAUEN, aber **default deaktiviert**. Steuerung über **Admin-Dashboard** (ein/aus). **Shadow-Modus = Antworten gehen nach Discord** (Review), nicht live in Twitch-Chat. Aktives Arbeitsfeld („müssen noch dran arbeiten").
### BUILD
- Engagement-KI mit **Admin-Dashboard-Toggle** (ein/aus), **Shadow→Discord-Output**, **default AUS**
- Divergenz-Fixes: `senderauth-03` DDL `expires_at`→TIMESTAMPTZ (clean SQL) · `threads-conv-rhythm-1` TZ · `senderauth-01` Admin-Attribution · `pipeline-kern-4` channel_login-Lowercase
- `minimax-persona-style-01` MiniMax-Ledger weiterführen
### DEFER / TODO
- Stream-Transkription (`bg-78-2`): OpenAI **jetzt raus**. **TODO:** später Whisper über **anderen Weg (nicht OpenAI direkt)** wieder ergänzen.
### ACCEPT
- `bg-78-1` Eager-Start · `pipeline-kern-2` Operator-Log · `persona-style-02` Tie-Break → Rust ok

**→ Block 19 ABGESCHLOSSEN.**

---

## Block 20 — kleine Subsysteme  (FINAL)
### entitlements — Plan-Korrektheit (verknüpft Block 2/9/16)
- `entitlements-1/2/3` → **FIX** (Plan-Normalisierung, `expiresAt`-Parsing, vollständiger PlanSnapshot status/manual_override/billing)
### title_generator (fast live)
- `title_generator-1` → **FIX** `get_latest_insights` (Dashboard) · `title_ai-1` MiniMax-Ledger weiterführen · `title_ai-2` Keyring→Infisical · `steam_lookup-1` → **Rust akzeptieren** (Python-Lookup tot)
### highlight_clipper
- `highlight_clipper-1` → **OFF** (Clip-Erstellung deaktiviert, konsistent Block 15)
### live_announce — nur Core-Embed behalten
- `live_announce-template-1/3/4` → **DROP** (manueller Builder + Config-Felder Button/Ping-Rollen-Naming/Editor-Roles — überflüssig). **Core Discord-Go-Live-Embed-Rendering bleibt live.** CAVEAT: #222-Ping-Rollen-Mention separat verifizieren (läuft weiter).
### stream_coaching_audit
- `stream_coaching_audit-1` → **PORTIEREN** (nicht droppen), Weiterbau später (Future-Work; Whisper/YouTube-Abhängigkeit koppelt an „Whisper über anderen Weg"-TODO aus Block 19)

**→ Block 20 ABGESCHLOSSEN.  ✅ GRILLME KOMPLETT (20/20 Blocks, alle 309 Entscheidungen).**

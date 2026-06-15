# Implementierungs-DAG — Twitch-Bot Python→Rust Cutover (Stand 2026-06-15)

**Ziel:** Vollständiger Cutover des Twitch-Bots von Python (Legacy 8765) auf Rust (`/rust`).
Maßstab: **1:1-Funktionsparität** mit Python — ABER an unserer Schema-/Cutover-Umstellung
scheiternde Bugs werden gefixt statt mitgeschleppt, und ineffizienter/duplizierter Code im
Scope wird modernisiert (nicht blind übersetzt). Original-Python bleibt **unverändert**; aller
neue Code ausschließlich unter `/rust`.

**Entscheidungs-Quelle (SSOT für alle Drop/Keep/Build-Calls):**
`/home/naniadm/Documents/Deadlock-Twitch-Bot/rust/docs/audit/2026-06-15-grillme-entscheidungen.md`
Begleitend: `audit/2026-06-14-opus-vollaudit-parity.md`, `audit/2026-06-13-python-rust-port-audit.md`.

---

## Querschnitts-Direktiven (gelten für ALLE Tickets)

1. **SQL clean (ADR-0002):** Ziel-Schema als versionierte, idempotente `sqlx`-Migrationen unter
   `rust/migrations/` — NICHT Pythons imperatives `ensure_schema` verbatim. SSOT = `rust/migrations/`,
   getrennt von Python `schema_version`. Frische DB ist allein durch `run_migrations()` aufsetzbar.
2. **MiniMax-only LLM:** Genau zwei Provider — **MiniMax** (Primär, "alles über MiniMax") +
   **Anthropic** (Premium/`ai_full`). **OpenAI raus** (inkl. Whisper-Transkription). Jeder MiniMax-Call
   verbucht Tokens ins gemeinsame Usage-Ledger (`source='twitch-bot'`, `purpose=...`).
3. **Security = Industriestandard, Admin höchste Prio:** Echtes sessiongebundenes CSRF auf ALLE
   Write-Actions, gehärtete HttpOnly/Secure/SameSite-signierte Cookies, Login-Rate-Limiter,
   Loopback-Origin/Peer-Guard, Admin-Device-/Canvas-Fingerprinting.
4. **Infisical:** Alle Secrets aus Infisical/Env, **kein Keyring**, niemals in Logs/Chat/Dateien.
5. **Keine Discord-DMs an User** — außer den explizit erlaubten Token-Lifecycle-Streamer-DMs (F4).
   Social-Media/Approval/Clip-Approval/Admin-Reauth/Wochenreport: **kein DM-Versand**. Alle anderen
   Discord-Aktionen ausschließlich über den **Master-Broker** (8770, ADR-0001).
6. **Discord-Befehle → Dashboard:** `/reauth_all`, `/sendchatpromo`, `!tte`, Owner-Chat-Action,
   Lurker-Tax etc. wandern aus Discord ins Admin-/Partner-Dashboard.

---

## Arbeitsweise (verbindlich)

- **Ein Worktree pro Arbeitspaket** (`git worktree add ../tb-<ticket> -b <branch>`), verhindert
  Index-Klau bei Parallel-Commits. `EnterWorktree`/`isolation: "worktree"` bevorzugen.
- **TDD strikt:** Test zuerst (RED) → minimaler Code (GREEN) → Refactor. Jedes Ticket trägt seinen
  RED-Test als Orakel (Python-Verhalten als Referenz).
- **Inkrementell committen + pushen:** jeder Commit lauffähig + verifiziert (Compiler/Linter/Tests).
  Nichts Kaputtes pushen. Trailer + CHANGELOG-Pflicht beachten.
- **Vertikal (Tracer Bullets):** DB→Backend→Handler→SPA durchgängig, nie schichtenweise.
- Nach jeder Bot-Änderung neu starten; echte Live-Signale (Logs, curl, DB-Asserts) bevorzugen.

---

## Phasen-Übersicht

| Phase | Inhalt | Tickets |
|-------|--------|---------|
| **Phase 0 — Foundation** | Migrations, LLM-Layer, Bridges, Session-Issuance, Security, Stripe-Client/-Katalog, Discord-Role-Port, Helix-Wrapper, Inbox-Demux-Foundation, Bot-Token-Bridge | 24 |
| **Phase 1 — P0** | Dashboard-Live-Wiederherstellung: Schema-Cutover, Session/Login/CSRF, Admin-SPA + Aktionen, Billing/Affiliate-Kern, Loopback/Token-Guards | 35 |
| **Phase 2 — P1** | Funktionaler Kern: Token-Lifecycle, EventSub/Telemetrie, Raid-Subsystem, Chat-Connection, Promos, Analytics-Paywall, Engagement-Shadow, core_runtime | 78 |
| **Phase 3 — P2** | Divergenz-/Shape-Fixes, Poller, Stats, Social-Media-Fixes, monitoring-Rest, kleine Subsysteme, OFF-Schalter | 56 |
| **Deferred** | VERIFY-Restbestätigungen + P3-Observability | 2 |
| **Summe** | | **195** |

*(Phasenzuordnung folgt dem `phase`-Feld; einige P1/P2-Tickets sind innerhalb der Phase
nach `dependsOn` einsortiert. Foundation-Tickets mit `phase:"Foundation"` stehen immer zuerst.)*

---

# PHASE 0 — FOUNDATION (24)

> Querschnitts-Capabilities. Alles hier blockiert mehrere Blocks. Strikt topologisch.

### Schicht 0a — keine Dependencies (parallel startbar)

#### F1-clean-migrations · BUILD · XL
- **Scope:** Gesamtes Prod-Ziel-Schema (~60 Tabellen, 2 Views, Identity-Sync-Trigger,
  Timescale-Hypertable, social_media Phase 0-4, oauth `consumed_at`+Indizes, token-lifecycle-Spalten,
  exp_*/viewer_presence_ticks) clean-nativ als versionierte sqlx-Migrationen. One-Shot-Transforms nur
  als Endzustand, nicht als Code.
- **Files:** `rust/migrations/*.sql`, `crates/tb-db/src/migrate.rs`, `tb-db/tests/{hermetic,prod_contract}.rs`
- **dependsOn:** — | **Test:** hermetic gegen leere DB → alle Objekte existieren; prod_contract = kein Drift
- **DoD:** Fresh-DB allein via `run_migrations()`; idempotent re-runnbar; `cargo test -p tb-db` grün

#### F4-discord-broker-actions · BUILD · M
- **Scope:** `BrokerRelay` um User-DM (nur Token-Lifecycle) + Alert-Channel-Embed erweitern,
  Idempotency-Key + Retry. **Keine** DM-Verdrahtung in social-media/approval/Wochenreport.
- **Files:** `crates/tb-transport-discord/src/{relay,backend,noop,lib}.rs`
- **dependsOn:** — | **Test:** wiremock send_user_dm/send_alert_embed; Negativ-Guard kein DM in Approval
- **DoD:** noop implementiert neue Trait-Methoden; `cargo test -p tb-transport-discord` grün

#### B2-F1-stripe-client · BUILD · L
- **Scope:** Nativer Stripe-HTTP-Client (Checkout/Subscription/Portal/Webhook-Sig HMAC-SHA256/
  Product-Price/Connect-OAuth+Transfer). Secrets via Infisical, nie geloggt.
- **Files:** `crates/tb-analytics/src/stripe/{mod,client,webhook_sig}.rs`
- **dependsOn:** — | **Test:** webhook_sig Referenzvektor true / Tampering false; wiremock checkout
- **DoD:** `cargo test -p tb-analytics` grün; Referenzvektor akzeptiert

#### B2-F2-billing-catalog-consts · BUILD · M
- **Scope:** BILLING_PLANS (8), CYCLE_DISCOUNTS, PRICE_ID_DEFAULTS als Rust-Konstanten;
  effective_monthly/lookup_key-Schema.
- **Files:** `crates/tb-analytics/src/billing/{catalog,mod}.rs`
- **dependsOn:** — | **Test:** Katalogwerte wert-identisch zu Python für 8×3
- **DoD:** `cargo test` grün

#### B6-HELIX-WRAP · BUILD · M
- **Scope:** HelixClient-Methoden `get_chatters` (Cursor-Pagination), `get_broadcaster_subscriptions`,
  `get_ad_schedule`; typisierte Results, NotModerator-Fehler.
- **Files:** `crates/tb-transport-twitch/src/{chat,streams,lib}.rs`
- **dependsOn:** bot-token-bridge (F3) | **Test:** wiremock Pagination + 403→NotModerator
- **DoD:** `cargo test -p tb-transport-twitch` grün, clippy clean

#### B10-FND-discord-role-removal · BUILD · M
- **Scope:** Broker `remove_member_role` + Port `revoke_streamer_role`; Fehler nur loggen.
- **Files:** `crates/tb-transport-discord/src/relay.rs`, `crates/tb-raid/src/partner_setup.rs`
- **dependsOn:** — | **Test:** wiremock remove-role + Idempotency-Key
- **DoD:** Trait-Methode verfügbar; `cargo test -p tb-transport-discord -p tb-raid` grün

#### B10-FND-bot-token-bridge / F3-bot-token-bridge · BUILD · L
- **Scope:** Live-rotierter Bot-User-Token der internen API als Shared-State/Provider-Port
  zugänglich; Handler können `send_chat_message`/`send_announcement` aufrufen. Ersetzt den
  chat-action-Stub durch echtes Senden; SendOutcome::Dropped sauber durchreichen.
- **Files:** `bin/tb-bot/src/{main,chat_wiring}.rs`, `crates/tb-internal-api/src/{lib,handlers/python_stubs,handlers/streamers}.rs`, `crates/tb-transport-twitch/src/chat.rs`, `crates/tb-chat/src/token.rs`
- **dependsOn:** F1 | **Test:** Provider liefert Token + Rotation; chat-action sendet 1× send_chat_message
- **DoD:** Token nie geloggt; `cargo test -p tb-internal-api -p tb-chat -p tb-bot` grün

#### B5/B8 Inbox-/Dispatch-Foundations
- **B8-00** · BUILD · M — `channel.chat.notification`-Zweig in `route()`, notice_type-Demux.
  Files: `crates/tb-monitoring/src/{dispatch,subscriptions}.rs`. dependsOn: —.
- **B7-08-pending-iter-api** · BUILD · S — read-only iter/values am PendingRaidStore.
  Files: `crates/tb-raid/src/pending_raids.rs`. dependsOn: —.
- **B7-05-pendingraid-target-stream-data** · BUILD · S — Feld `target_stream_data`.
  Files: `crates/tb-raid/src/pending_raids.rs`. dependsOn: —.

### Schicht 0b — auf F1 (Migrations)

#### F2-llm-clients / B17-LLM / B19-minimax-ledger / b20-minimax-ledger · BUILD · L+M
- **Scope:** Eine tiefe LLM-Crate (`tb-llm`): MiniMax primär + Anthropic premium, OpenAI raus
  (Transcribe/Whisper entfernt). Konsolidierter Key-Resolver (kein Dup). MiniMax-Usage-Ledger-Writer
  (best-effort, `purpose` je Feature) — Foundation für engagement/title/scam.
- **Files:** `crates/tb-llm/**`, `crates/tb-engagement/src/{minimax_chat,claude_chat,pipeline,transcribe,whisper,background}.rs`, `crates/tb-social-media/src/{whisper,llm}.rs`, `crates/tb-chat/src/title_ai.rs`, `crates/tb-analytics/src/post_stream.rs`, `crates/tb-config/src/lib.rs`
- **dependsOn:** F1 | **Test:** kein openai-Provider-Pfad; record_usage schreibt 1 Ledger-Zeile; wiremock MiniMax/Anthropic
- **DoD:** genau 2 Provider; grep `openai` über crates/bin → kein aktiver Pfad; Ledger je Feature abfragbar

#### B11-MIG-1 / F1 (Baseline-Variante) · BUILD · XL
- **Scope:** Eingefrorenes Prod-Schema (~60 Tab + 2 Views + 2 Identity-Trigger + Timescale) als
  versionierte idempotente Migration; Views/Trigger via CREATE OR REPLACE.
- **Files:** `rust/migrations/0001_baseline.sql`, `0002_views_triggers.sql`, `crates/tb-db/src/migrate.rs`, `bin/tb-bot/src/main.rs`
- **dependsOn:** — (Teil von F1) | **Test:** information_schema/pg_views/pg_trigger; 2. Lauf no-op
- **DoD:** Fresh-DB Boot; Schema-Contract-Test grün

#### B11-MIG-2 · BUILD · M — Startup-Maintenance (Serial-Alignment/Bool-Coercion/Live-State-Dedup/Unique-Index).
Files: `crates/tb-db/src/maintenance.rs`,`lib.rs`,`bin/tb-bot/src/main.rs`. dependsOn: B11-MIG-1.

#### F5-dashboard-session-creation · BUILD · L
- **Scope:** OAuth-Callback-Handler: Code-Tausch, Partner-Gate beim Login, `dashboard_sessions`-Zeile
  (Fernet/AES-GCM-Payload, session_type twitch/discord_admin), Cookie-Issuance, HMAC-One-Time-Token,
  Doppel-Cookie-Merge. (Cookie-Härtung in F6.)
- **Files:** `crates/tb-dashboard-api/src/auth/{session,fernet,level}.rs`, `handlers/auth_status.rs`, `lib.rs`, `bin/tb-dashboard/src/main.rs`
- **dependsOn:** F1 | **Test:** Callback erlaubter Partner → 1 Session-Zeile + Set-Cookie; geblockt → keine
- **DoD:** SPA kann sich einloggen; `cargo test -p tb-dashboard-api` grün

### Schicht 0c — auf F1+F5

#### F6-security-layer / session-crypto / login-infra · BUILD · L
- **Scope:** (1) Echtes sessiongebundenes CSRF (kein null), Gate auf alle Writes; (2) Cookie-Härtung
  HttpOnly/Secure/SameSite + Silent-Refresh; (3) Login-Rate-Limiter (Middleware); (4) Admin-Härtung
  Device-/Canvas-Fingerprinting + Loopback-Origin/Peer-Guard.
- **Files:** `crates/tb-http-core/src/middleware/{auth,loopback,mod}.rs`, `crates/tb-dashboard-api/src/auth/{session,level}.rs`, `handlers/{auth_status,admin_config}.rs`
- **dependsOn:** F1, F5 | **Test:** Write ohne CSRF→403; csrfToken≠null; N+1 Login→429; Cookie-Flags; fremder Origin→403
- **DoD:** kein null-CSRF; Loopback-Schutz intakt; `cargo test -p tb-http-core -p tb-dashboard-api` grün

> **Foundation-Token-Lifecycle-Schema (P0, gehört in F1-Migrationsblock):**
> **TL-7** · BUILD · M — Blacklist-Spalten (grace_expires_at/user_dm_sent/reminder_sent/role_removed)
> + technical_pause_reason + bot_banned-Backfill als idempotente sqlx-Migration.
> Files: `migrations/`, `crates/tb-db/src/migrate.rs`. dependsOn: F1. Test: hermetic Fresh-DB + Backfill.

---

# PHASE 1 — P0 (Dashboard-Live-Wiederherstellung) (35)

> Ziel: Mit abgeschaltetem Python 8765 liefern alle Haupt-Einstiegspunkte, Login, Admin-Aktionen
> und Billing/Affiliate nativ 200/302 statt 502. Schema-Cutover vollständig.

### 1.1 — Schema-Cutover (Block 12)
Reihenfolge: **M12-1 → M12-2 → M12-6** (M12-3/4/5 sind P2, siehe Phase 3).
- **M12-1** · BUILD · L — Social-Media-Schema (Phase 0-4) in clean-migrations. dependsOn: F1.
  Files: `rust/migrations/<ts>_social_media_phase0_4.sql`, `crates/tb-social-media/src/schema.rs`.
- **M12-2** · BUILD · M — `oauth_state_tokens`-Härtung (consumed_at TIMESTAMPTZ, expires_at TIMESTAMPTZ NOT NULL,
  2 Indizes) — fixt Cutover-Schema-Bug. dependsOn: F1.
- **B19-senderauth-03-expires-ddl** · VERIFY+FIX · S — expires_at-Live-Typ prüfen, TIMESTAMPTZ in Migration. dependsOn: F1.
- **M12-6** · VERIFY · M — Fresh-DB-Boot deckt alle Worker-Schreibpfade. dependsOn: M12-1..5.

### 1.2 — Session/Login/CSRF nativ (Block 3 + Block 11)
Topologisch: **B3-1 → B3-2 → {B3-3, B3-4, B3-6, B3-10} ; B3-4 → B3-5 ; B3-2 → B3-8 → B3-9 ; B3-2 → B3-11**
- **B3-1 / B11-SESS-2** · BUILD · L/XL — Session-Create (twitch/discord_admin/partner_access) +
  OAuth-State save/pop (atomar single-use). Files: `auth/{session,oauth_login}.rs`,`handlers/auth_login.rs`,`lib.rs`,`crates/tb-crypto`. dependsOn: F1, F6.
- **B3-2** · BUILD · XL — native Twitch-OAuth Login/Callback/Logout. dependsOn: B3-1, F6.
- **B3-3 / B11-SESS-1** · BUILD · L/M — Login-Rate-Limiter (pg_advisory Sliding-Window). dependsOn: F1, B3-2.
- **B3-4** · BUILD · M — CSRF-Token-Subsystem + auth-status-Einspeisung. dependsOn: F6, B3-1.
- **B3-5** · BUILD · L — CSRF auf allen Write-Routen erzwingen. dependsOn: B3-4, F6.
- **B3-10** · BUILD · M — Device-/Canvas-Fingerprinting nach Discord-Admin-Login. dependsOn: B3-2, B3-1.

### 1.3 — Loopback/Token-Guards (Block 10/17, P0)
- **B10-FIX-loopback-origin-guard** · FIX · M — Origin-Header-Prüfung in loopback_only. dependsOn: F6.
- **B10-FIX-token-trim** · FIX · S — Internal-Token-Vergleich trimmt presented+expected. dependsOn: F6.
- **B17-PORTGUARD** · BUILD · S — PORT≠8766 erzwingen. dependsOn: —.
- **B10-BUILD-split-deploy-port-assert** (P2, hier vermerkt) — Port==8776-Assert. dependsOn: —.

### 1.4 — Admin-CSRF (Block 16, P0)
- **B16-FIX-CSRF-ADMIN** · FIX · M — CSRF auf 6 Admin-Schreib-Handlern; csrfToken non-null. dependsOn: F6.
- **B16-FIX-CSRF-INTERNALHOME** · FIX · S — Rate-Limit + Same-Origin auf Changelog-Create. dependsOn: F6, B16-FIX-CSRF-ADMIN.

### 1.5 — Entry/Admin-SPA + Admin-Aktionen (Block 1)
Topologisch: **B1-ENTRY ; B1-ADMIN-FORWARD-AUTH → B1-ADMIN-SPA → {add/verify/archive/discord/manual-plan} ; B1-VERIFY → B1-VERIFY-LIFECYCLE**
- **B1-ENTRY** · BUILD · S — native Entry/Redirect-Routen (/, /twitch, /twitch/stats). dependsOn: —.
- **B1-ADMIN-FORWARD-AUTH** · BUILD · M — forward_auth validate_admin_session. dependsOn: B3-1/login-infra, F6.
- **B1-ADMIN-SPA** · BUILD · L — Admin-SPA /twitch + /twitch/admin. dependsOn: B1-ADMIN-FORWARD-AUTH, F6.
- **B1-ADD-STREAMER** · BUILD · M — add_streamer(+Varianten). dependsOn: B1-ADMIN-FORWARD-AUTH, partner-registry-write, F6.
- **B1-VERIFY** · BUILD · S — verify permanent/30T. dependsOn: B1-ADMIN-FORWARD-AUTH, F6.
- **B1-ARCHIVE** · BUILD · S — archive/unarchive/remove. dependsOn: B1-ADMIN-FORWARD-AUTH, F6.
- **B1-DISCORD-LINK** · BUILD · S — discord_flag + discord_link. dependsOn: B1-ADMIN-FORWARD-AUTH, F6.
- **B1-MANUAL-PLAN** · BUILD · M — Manual-Plan save/clear Write-Backend. dependsOn: B1-ADMIN-FORWARD-AUTH, F1, F6.

### 1.6 — Partner-Registry-Write-Foundation (Block 10, Cross-Block)
- **B10-FND-partner-departner-reactivate / partner-registry-write** · BUILD · L —
  departner_active_partner + reactivate_partner (Hard-Kill-Respekt). dependsOn: F1.
  Files: `crates/tb-raid/src/partner_setup.rs`, `crates/tb-analytics/src/streamers_crud.rs`.

### 1.7 — Billing/Affiliate-Kern (Block 2)
Topologisch: **B2-P0-checkout-allowlist ; {B2-F1,B2-F2} → B2-P0-stripe-webhook → {cancel, billing-json-apis} ; billing-json-apis → abbo-page-routes ; webhook → affiliate-commission ; affiliate-oauth-login → affiliate-connect-onboarding → VERIFY**
- **B2-P0-checkout-allowlist** · BUILD · S — SSRF-Redirect-Allowlist. dependsOn: F6.
- **B2-P0-stripe-webhook** · BUILD · XL — Webhook→Entitlements (Quelle der Wahrheit). dependsOn: B2-F1, B2-F2, F1.
- **B2-P0-checkout-start** · BUILD · L — /abbo/bezahlen Checkout-Redirect. dependsOn: B2-F1, B2-F2, B2-P0-checkout-allowlist, F6.
- **B2-P0-cancel-subscription** · BUILD · M — /abbo/kündigen Portal+Fallback. dependsOn: B2-F1, B2-P0-stripe-webhook.
- **B2-P0-billing-json-apis** · BUILD · M — catalog/readiness/checkout-preview/invoice-preview. dependsOn: B2-F2, B2-P0-stripe-webhook.
- **B2-P0-abbo-page-routes** · BUILD · M — /abbo + /pricing SPA + Aliasse. dependsOn: B2-P0-billing-json-apis.
- **B2-P0-affiliate-oauth-login** · BUILD · L — Affiliate-OAuth Login/Callback/Session. dependsOn: F6, F1.
- **B2-P0-affiliate-connect-onboarding** · BUILD · L — Stripe-Connect Onboarding+Replay. dependsOn: B2-F1, B2-P0-affiliate-oauth-login, B2-P1-affiliate-commission.
- **B2-VERIFY-cutover-python-off** · VERIFY · S — alle Block-2-Routen nativ ohne Python. dependsOn: alle P0/P1-Block-2.

> Begleit-P1 für Block 2 (in Phase 2 gelistet, hier referenziert weil B2-P0-affiliate-connect davon abhängt):
> **B2-P1-affiliate-commission** (30% bei invoice.payment_succeeded).

---

# PHASE 2 — P1 (Funktionaler Kern) (78)

### 2.1 — Token-Lifecycle (Block 4)
Topologisch: **TL-7(P0,Phase0) → {TL-1, TL-2, TL-4, TL-5} ; TL-2 → TL-3 ; TL-5 → TL-6 ; TL-8 unabhängig**
- **TL-1** · BUILD · M — Admin-Channel-Embed bei Token-Fehler, 1×/Streamer. dependsOn: TL-7.
- **TL-2** · BUILD · L — User-DM + Reconnect-Button, user_dm_sent=1. dependsOn: F4, TL-7.
- **TL-3** · BUILD · L — 7-Tage-Grace-Sweep (Rollenentzug/opt-out/Reminder, stündlich). dependsOn: TL-2, TL-7, F4.
- **TL-4** · BUILD · S — Blacklist-Cleanup >30d (3.5h). dependsOn: TL-7.
- **TL-5** · BUILD · L — Bot-Ban-Opt-out + Recovery-DM (ohne needs_reauth). dependsOn: F4, TL-7.
- **TL-6** · BUILD · L — Auto-Restore nach Health-Restore. dependsOn: TL-5.
- **TL-8** · BUILD · S — generate_oauth_tokens (authorization_code-Exchange). dependsOn: —.

### 2.2 — Raid-Subsystem (Block 7)
Topologisch: **B7-01 → {B7-02, B7-13} ; {B7-01,B7-08} → B7-03 ; B7-10 → B7-11 → B7-12 ; standalone: B7-04, B7-05, B7-06, B7-07, B7-14**
- **B7-01** · BUILD · M — chat.notification(raid) → Arrival-Runtime. dependsOn: —.
- **B7-02** · BUILD · S — unraid Ziel-seitiger Withdraw. dependsOn: B7-01.
- **B7-03** · BUILD · M — Source-Self-Unraid storniert Pendings. dependsOn: B7-01, B7-08.
- **B7-06** · BUILD · L — resolve_partner_raid_tracking_for_session. dependsOn: F1.
- **B7-07** · BUILD · M — Proaktiver Token-Refresh (30min). dependsOn: F3/bot-token-bridge.
- **B7-10** · FIX · M — Re-Auth ohne discord-id reaktiviert Partner. dependsOn: F6.
- **B7-11** · BUILD · M — Massen-Re-Auth ins Dashboard. dependsOn: F6, B7-10.
- **B7-14** · VERIFY · S — zentraler Discord-Bot bedient Raid-Auth-Views. dependsOn: —.

### 2.3 — EventSub/Telemetrie (Block 5 + Block 8 Dispatch)
- **B5-01-first-message-sub** · BUILD · S — user_first_message-Subscription. dependsOn: —.
- **B5-02-mod-telemetry-subs** · BUILD · M — follow/ban/unban/shoutout-Subs. dependsOn: —.
- **B5-03-prompt-subscribe-new-partner** · BUILD · M — sofort subscriben statt 6h. dependsOn: B5-02.
- **B5-04-deadletter-alarm-broker** · FIX · M — Dead-Letter-Hook → Broker-Alarm + Supervisor-Wakeup. dependsOn: —.
- **B8-01** · BUILD · M — Sub/Resub/Gift-Telemetrie aus chat.notification. dependsOn: B8-00.
- **B8-02 / B7-relevant** · BUILD · L — Raid/Unraid-Korrelation aus chat.notification. dependsOn: B8-00.
- **B8-03** · FIX · S — raid_enabled-Kanäle in Chat-Reconcile. dependsOn: —.
- **B8-04** · FIX · M — 403-Mod-Retry-Recovery beim Join. dependsOn: F3/bot-token-bridge.
- **B8-05** · BUILD · M — Bot-Ban-Blacklisting + Broker-Notify. dependsOn: B8-04.

### 2.4 — Internal-API Streamer-CRUD (Block 10)
Topologisch: **{B10-FND-*} → {remove-lifecycle, verify-promote, verify-clear-failed, archive-reactivate} ; B10-FND-bot-token-bridge → chat-action**
- **B10-FIX-remove-lifecycle** · FIX · M. dependsOn: B10-FND-partner-departner-reactivate, B10-FND-discord-role-removal.
- **B10-FIX-verify-promote** · FIX · L. dependsOn: B10-FND-discord-role-removal.
- **B10-FIX-verify-clear-failed** · FIX · M. dependsOn: B10-FND-partner-departner-reactivate, B10-FND-discord-role-removal.
- **B10-FIX-archive-reactivate-messages** · FIX · M. dependsOn: B10-FND-partner-departner-reactivate.
- **B10-FIX-add-require-link-backfill** · FIX · M. dependsOn: —.
- **B10-BUILD-chat-action** · BUILD · M. dependsOn: B10-FND-bot-token-bridge.
- **B10-FIX-json-serializer-parity** · FIX · S. dependsOn: —.
- **B1-VERIFY-LIFECYCLE** · FIX · M — verify clear/failed nativ statt 503. dependsOn: partner-registry-write, broker, B1-VERIFY.
- **B1-OWNER-CHAT-ACTION** · BUILD · L — MODERN erweiterbare Owner-Chat-Aktion. dependsOn: F3, broker, B1-ADMIN-FORWARD-AUTH.
- **B1-TITLE-ROUTES** · BUILD · M — title/suggest + insights HTTP-Wrapper. dependsOn: login-infra.
- **B1-LURKER-TAX** · BUILD · S — Lurker-Tax-Setting. dependsOn: login-infra.

### 2.5 — Chat-Connection-Foundation (Block 8 Dispatch)
- **B8-00** (Foundation, Phase 0) — Demux-Hook.

### 2.6 — Promos/Lurker-Tax/Scam (Block 9, P1)
- **B9-FIX-promo-plan-expiry** · FIX · M — Plan-Ablauf bei Promo-Disable+Lurker-Tax. dependsOn: F1.
- **B9-BUILD-lurkertax-off-cmd** · BUILD · M — !lurkersteuer_off. dependsOn: F1.
- **B9-BUILD-lurkertax-toggle-dashboard** · BUILD · M — Dashboard-Toggle (default aus). dependsOn: F6, F1.
- **B9-FIX-lurkertax-channel-query** · FIX · M — eigene Live-Query ohne Game-Filter. dependsOn: F1.

### 2.7 — Analytics-Paywall (Block 16, P1)
- **B16-FIX-VIEWER-GATE** · FIX · S — extended_gate auf 5 Viewer-Endpoints. dependsOn: —.
- **B16-FIX-OVERVIEW-WINDOW** · FIX · M — Free-Plan-Tagesform. dependsOn: —.
- **B16-FIX-VIEWER-EXCLUSION** · FIX · M — Streamer-Self + Bot-Exklusion. dependsOn: —.
- **B16-VERIFY-PAYWALL** · VERIFY · S. dependsOn: B16-FIX-VIEWER-GATE, B16-FIX-OVERVIEW-WINDOW.
- **B16-VERIFY-CHATDEEP** · VERIFY · S. dependsOn: —.
- **B16-VERIFY-V2AUTH-SHAPE** · VERIFY · S. dependsOn: F6.

### 2.8 — Engagement-Shadow-Mode (Block 19, P1)
Topologisch: **B19-shadow-mode → {B19-shadow-discord-out, B19-dash-mode-toggle} ; B19-senderauth-01-admin-attr standalone**
- **B19-shadow-mode** · BUILD · M — Output-Modus live|shadow|off (Schema+Pipeline). dependsOn: F1.
- **B19-shadow-discord-out** · BUILD · M — Shadow → Discord-Review via Broker. dependsOn: B19-shadow-mode, F3.
- **B19-dash-mode-toggle** · BUILD · M — Modus-Schalter im Dashboard. dependsOn: B19-shadow-mode, F6.
- **B19-senderauth-01-admin-attr** · FIX · S — Twitch-Admin behält actor_login/id. dependsOn: —.
- **B19-minimax-ledger** · BUILD · M (Foundation) — Ledger-Helper + Engagement-Recording. dependsOn: minimax-ledger.

### 2.9 — Storage/Schema-Mutationen + core_runtime (Block 11 + 17, P1)
- **B11-PR-4** · BUILD · L — Admin-Streamer-Mutationen nativ statt Proxy. dependsOn: F6.
- **B11-PR-6** · BUILD · M — reactivate_partner Dashboard-Unarchive. dependsOn: B11-PR-4.
- **B11-PR-7** · FIX · S — Hard-Pause-Guard im OAuth-Followup. dependsOn: —.
- **B17-GRACEFUL** · BUILD · M — Graceful Shutdown SIGTERM. dependsOn: —.
- **B17-SCOUT-PRIME** · BUILD · L — Session-Priming + Chat-Join/Heal monitored-only. dependsOn: —.
- **B17-USERID-BACKFILL** · BUILD · M — Startup-Backfill twitch_user_id. dependsOn: F1.

### 2.10 — Poller (Block 6, P1)
- **B6-CHATTERS-POLLER** · BUILD · L — Lurker-Poller (30s). dependsOn: B6-HELIX-WRAP, F3, F1.
- **B6-DEMO-LEAN** · BUILD · L — öffentliches Demo-Dashboard nativ+lean. dependsOn: —.

### 2.11 — monitoring-Rest (Block 18, P1)
- **B18-1-replay-window** · FIX · S — Timestamp-Replay-Fenster >600s→403. dependsOn: —.
- **B18-3-auth-block-circuit-breaker** · FIX · M — is_auth_blocked-Backoff. dependsOn: —.
- **B18-4-reconcile-is-partner-active** · FIX · S — Reconcile auf is_partner_active. dependsOn: F1.

### 2.12 — Billing/Affiliate-P1 (Block 2)
- **B2-P1-billing-profiles** · BUILD · M. dependsOn: B2-F1, F1.
- **B2-P1-stripe-sync-products** · BUILD · M. dependsOn: B2-F1, B2-F2, F6.
- **B2-P1-admin-manual-plan** · BUILD · S. dependsOn: F6, F1.
- **B2-P1-affiliate-commission** · BUILD · L. dependsOn: B2-F1, B2-P0-stripe-webhook, F1.
- **B2-P1-affiliate-pii-write** · BUILD · M. dependsOn: F1.
- **B2-P1-affiliate-user-api** · BUILD · L. dependsOn: B2-P0-affiliate-oauth-login, B2-P1-affiliate-pii-write.

### 2.13 — Discord-Link + kleine Subsysteme (Block 3/20, P1)
- **B3-6** · BUILD · L — Discord-Link-Flow nativ. dependsOn: B3-1, B3-2.
- **B3-7** · FIX · S — CSRF auf Admin-JSON (announcements/legal). dependsOn: B3-5.
- **B3-8** · BUILD · XL — Partner-Einmal-Login (HMAC One-Time-Token). dependsOn: F1, session-crypto, B3-2.
- **B3-9** · FIX · M — Partner-Access-Cookie im Extractor. dependsOn: B3-8.
- **B20-ent-1-normalize** · FIX · S. dependsOn: —.
- **B20-ent-2-expiresat** · FIX · S. dependsOn: —.
- **b20-title-insights-read** · FIX · M (P0) — get_latest_insights + /title/insights nativ. dependsOn: F6.
- **b20-clipper-off-switch** · OFF · M — Clip-Erstellung default-AUS. dependsOn: —.
- **b20-liveannounce-ping-verify** · VERIFY · S — #222 Ping-Rolle. dependsOn: —.
- **B7-14 / B14-verify** (s. 2.2).

### 2.14 — Internal-Home-Fixes (Block 16, P2 vorgezogen wo P1-Block)
- (siehe Phase 3 für die P2-Shape-Fixes)

---

# PHASE 3 — P2 (Divergenz-/Shape-Fixes, Poller, Stats, OFF) (56)

### 3.1 — Block 12 Schema-Rest (P2)
- **M12-3** · BUILD · S — Auto-Approve-Default-Zeilen seeden. dependsOn: M12-1.
- **M12-4** · BUILD · M — exp-Tracking-Tabellen. dependsOn: F1.
- **M12-5** · BUILD · S — viewer_presence_ticks. dependsOn: F1.

### 3.2 — Block 1 Raid-/Markt-/Stats-Seiten (P2)
- **B1-RAID-HISTORY** · BUILD · M. dependsOn: B1-ADMIN-SPA.
- **B1-RAID-ANALYTICS-PAGE** · BUILD · M. dependsOn: B1-ADMIN-SPA.
- **B1-RAID-OAUTH-PAGE** · BUILD · M. dependsOn: F6, B1-ADMIN-SPA.
- **B1-RAID-REQUIREMENTS** · BUILD · M. dependsOn: broker, B1-ADMIN-SPA.
- **B1-RAID-REQUIREMENTS-PAGE-LINK** · VERIFY · S. dependsOn: B1-RAID-REQUIREMENTS.
- **B1-RAID-REQUIREMENTS-PANEL** · BUILD · S. dependsOn: B1-RAID-REQUIREMENTS, B1-ADMIN-SPA.
- **B1-RAID-REQUIREMENTS-PATH** · VERIFY · S. dependsOn: B1-RAID-OAUTH-PAGE, B1-RAID-REQUIREMENTS-PANEL.
- **B1-PARTNER-STATS-PAGE** · BUILD · M. dependsOn: B1-ADMIN-SPA.
- **B1-MARKET** · BUILD · L. dependsOn: B1-ADMIN-FORWARD-AUTH.
- **B1-ROADMAP-PAGE** · BUILD · S. dependsOn: B1-ADMIN-SPA.
- **B1-DEMO-EMBED** · BUILD · M. dependsOn: B1-ADMIN-SPA.
- **B1-SYSTEM-ERRORS** · FIX · S. dependsOn: —.

### 3.3 — Block 5 Inbox-Robustheit (P2)
- **B5-07-debug-snapshot-endpoint** · BUILD · M. dependsOn: —.
- **B5-05-requeue-endpoint-wire** · FIX · M. dependsOn: B5-07-debug-snapshot-endpoint.
- **B5-06-storage-retry-wrapper** · FIX · S. dependsOn: —.
- **B5-08-capacity-snapshot-timeseries** · FIX · M. dependsOn: —.
- **B5-09-offline-side-effect-order** · FIX · S. dependsOn: —.

### 3.4 — Block 6 Snapshot-Poller (P2)
- **B6-SNAPSHOT-POLLER** · BUILD · M. dependsOn: B6-HELIX-WRAP, F1.

### 3.5 — Block 7 Raid-Rest (P2)
- **B7-04-pending-timeout-sweeper** · BUILD · S. dependsOn: —.
- **B7-09-fairness-follower-tiebreak** · FIX · M. dependsOn: —.
- **B7-12-sendchatpromo-dashboard** · BUILD · S. dependsOn: F6, B7-11.
- **B7-13-raid-observability-dashboard** · BUILD · M (P3). dependsOn: B7-01.

### 3.6 — Block 8 Chat-Rest (P2)
- **B8-06** · FIX · S — is_partner-Predikat. dependsOn: —.
- **B8-07** · BUILD · M — Passive-Lurker-Tracking. dependsOn: B8-03.
- **B8-08** · BUILD · M — Streamer-Invite-Erzeugung via Broker. dependsOn: —.
- **B8-09** · FIX · S — write_back-Kommentar korrigieren. dependsOn: —.
- **B8-10** · VERIFY · S — IRC-Lurker-Modul erhalten. dependsOn: —.

### 3.7 — Block 9 Clip/Scam-Rest (P2)
- **B9-FIX-clip-fallback** · FIX · S. dependsOn: F3.
- **B9-FIX-clip-texts** · FIX · S. dependsOn: —.
- **B9-BUILD-service-warning-audit-trail** · BUILD · S. dependsOn: —.
- **B9-BUILD-minimax-usage-ledger** · BUILD · S. dependsOn: —.
- **B9-FIX-timeout-reason** · FIX · S. dependsOn: —.
- **B9-VERIFY-bot-token-file** · VERIFY · S. dependsOn: F3.

### 3.8 — Block 10/11 Rest (P2)
- **B10-VERIFY-list-shape** · VERIFY · S. dependsOn: —.
- **B10-FIX-discord-flag-no-userid** · FIX · S. dependsOn: —.
- **B11-MIG-3** · BUILD · S — Write-Retry 40001/40P01. dependsOn: —.

### 3.9 — Block 13 Community (P2)
- **B13-1** · BUILD · M — 4 Stats-Sektionen in compute_stats. dependsOn: F1.
- **B13-2** · BUILD · L — Dashboard-Web-Leaderboard. dependsOn: B13-1, F6.

### 3.10 — Block 15 social_media (P2)
Topologisch: **B15-OFF-clipgen → {B15-OFF-transcription, B15-FIX-yt-refresh}**
- **B15-OFF-clipgen** · OFF · S — Clip-Erstellung default-AUS. dependsOn: —.
- **B15-OFF-transcription** · OFF · S — OpenAI-Whisper raus. dependsOn: B15-OFF-clipgen.
- **B15-FIX-index-redirect** · FIX · S. dependsOn: F6.
- **B15-FIX-fetch-layout** · FIX · S. dependsOn: F1.
- **B15-FIX-audit-fields** · FIX · S. dependsOn: F6.
- **B15-FIX-mime** · FIX · M. dependsOn: —.
- **B15-FIX-userid-backfill** · FIX · S. dependsOn: F1.
- **B15-FIX-yt-refresh** · FIX · M. dependsOn: B15-OFF-clipgen.

### 3.11 — Block 16 Shape-Fixes (P2)
- **B16-FIX-AIHISTORY-ERRORSHAPE** · FIX · S. dependsOn: —.
- **B16-FIX-INTERNALHOME-DISPLAYNAME** · FIX · S. dependsOn: —.
- **B16-FIX-SHAPE-PARITY** · FIX · M. dependsOn: —.

### 3.12 — Block 18 monitoring-Rest (P2)
- **B18-2-dispatch-ready-gate** · FIX · S. dependsOn: B18-1-replay-window.
- **B18-5-refresh-inflight-dedupe** · FIX · M. dependsOn: B18-4-reconcile-is-partner-active.
- **B18-6-language-filter-parser** · FIX · S. dependsOn: —.
- **B18-7-thumbnail-fallback** · FIX · S. dependsOn: —.

### 3.13 — Block 19/20 Rest (P2)
- **B19-rhythm-1-tz** · FIX · S. dependsOn: —.
- **B19-pipeline-kern-4-lowercase** · FIX · S. dependsOn: —.
- **b20-ent-3-snapshot-fields** · FIX · M. dependsOn: b20-ent-2-expiresat.

---

# DEFERRED (Future / niedrigste Prio)

- **B7-13-raid-observability-dashboard** (P3-Observability, optional gebatchter Writer).
- VERIFY-Tickets ohne Code-Change, die erst nach Live-Cutover sinnvoll abschließbar sind
  (B2-VERIFY-cutover-python-off, M12-6, B7-14) — laufen als Schluss-Gate jeder Phase.

---

# KRITISCHER PFAD

Die längste blockierende Kette (jeder Knoten schaltet viele nachgelagerte Tickets frei):

```
F1-clean-migrations (XL)
   └─> F5-dashboard-session-creation (L)
          └─> F6-security-layer (L)
                 └─> B3-1/B11-SESS-2 (Session-Create, XL)
                        └─> B3-2 (OAuth-Login/Callback, XL)
                               └─> B3-8 (Partner-Einmal-Login, XL)
                                      └─> B1-ADMIN-FORWARD-AUTH (M)
                                             └─> B1-ADMIN-SPA (L)
                                                    └─> Block-1-Admin-Aktionen + Raid-/Markt-Seiten
```

Parallel-kritisch (Billing): `F1 → {B2-F1, B2-F2} → B2-P0-stripe-webhook (XL) → affiliate-commission → connect-onboarding → B2-VERIFY`.

**Engste Engpässe (Reihenfolge der Aufmerksamkeit):**
1. **F1** — blockiert ~ alles (Session, Login, Billing, Token-Lifecycle, Poller, Engagement, Stats).
2. **F6-security-layer** — blockiert jeden Write-Pfad (CSRF), Admin-SPA, Billing-Starts, Dashboard-Toggles.
3. **B3-2 (OAuth-Login)** — ohne nativen Login kein Cutover; blockiert B3-6/8/9/10, Admin-Forward-Auth.
4. **F3-bot-token-bridge** — schaltet Chat-Send, !clip-Fallback, Owner-Chat-Action, Mod-Retry, IRC-Lurker, Snapshot/Chatters-Poller frei.
5. **B8-00 (Demux)** — schaltet B8-01/02 + Raid-Korrelation (B7) frei.

---

# Foundation → freigeschaltete Blocks

| Foundation-Ticket | Schaltet frei |
|-------------------|---------------|
| **F1-clean-migrations** | Block 4 (TL-7), 11, 12 gesamt; alle DB-lesenden/schreibenden P0/P1; Engagement-Schema; Stats; Poller |
| **F2/B17-LLM/Ledger** | chat_deep, scam-review, title-gen, engagement, post-stream-AI; OpenAI-Entfernung |
| **F3/B10-FND-bot-token-bridge** | F-chat-action, B1-OWNER-CHAT-ACTION, B9-clip-fallback, B8-04 Mod-Retry, B6-Poller, B17-IRC-Lurker, B7-07 |
| **F4-discord-broker-actions** | TL-1/2/3/5 (DM+Alert), B7-Requirements, B8-05 Notify, B17-Alert-Channel |
| **F5-session-creation** | F6, B3-1/2, gesamter Login-/Admin-Auth-Pfad |
| **F6-security-layer** | Alle Write-Routen (CSRF), Admin-SPA, Billing-Starts, Dashboard-Toggles, Loopback-Guards |
| **B2-F1/F2 (Stripe/Katalog)** | gesamter Block 2 (Webhook, Checkout, Cancel, Sync, Commission) |
| **B6-HELIX-WRAP** | B6-CHATTERS-POLLER, B6-SNAPSHOT-POLLER, B8-04 Mod-Retry |
| **B10-FND-partner-departner-reactivate** | B10-remove/verify-clear/archive, B11-PR-6, B1-VERIFY-LIFECYCLE |
| **B10-FND-discord-role-removal** | B10-remove-lifecycle, verify-promote, verify-clear-failed |
| **B8-00 (Demux)** | B8-01 (Sub-Telemetrie), B8-02 (Raid-Korrelation) |
| **B19-shadow-mode** | B19-shadow-discord-out, B19-dash-mode-toggle |

---

*Erstellt 2026-06-15. Tickets: 195 (24 Foundation / 35 P0 / 78 P1 / 56 P2 / 2 Deferred).
Phasenzuordnung nach `phase`-Feld + topologischer Sortierung innerhalb der Phase.*

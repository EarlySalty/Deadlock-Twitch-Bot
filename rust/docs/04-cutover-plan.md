# 04 — Strangler-Fig Cutover-Plan

**Prinzip:** Fundament zuerst (kein Verhaltenswechsel), dann risikoarme Blätter (read-only),
zuletzt vertragskritisch/mutierend (OAuth, Billing, EventSub-Schreibpfad). Die Reihenfolge ist
nach **Schadenspotenzial bei Fehler** sortiert, nicht nach Code-Nähe. Während der ganzen
Migration teilen beide Prozesse die DB → jeder Schritt muss am Schema **lesend rückwärts­kompatibel**
bleiben.

> **Discord** ist kein eigener Cutover-Schritt: Es läuft über alle Phasen hinweg als
> `BrokerRelay` (Master-Broker 8770). Die Discord-Sends der jeweiligen Phase (Live-Embeds in
> Monitoring, Rollen-Sync/Invites in Raid) nutzen von Anfang an das Relay. Siehe ADR 0001.

## Schritt 0 — Foundation *(kein Live-Cutover)*

- **Live:** `tb-error/domain/config/db/crypto/transport-*` gebaut + getestet; Migrations als SSOT
  eingelesen und **read-only** gegen das Prod-Schema verifiziert.
- **Erfolg:** Rust verbindet auf dieselbe DB, liest alle Owner-Tabellen; Crypto-Interop-Test gegen
  bestehende `twitch_raid_auth`/`dashboard_sessions`-Blobs grün (oder Re-Auth-Entscheid getroffen).
- **Rollback:** nichts live — Crate verwerfen.

## Schritt 1 — Read-only Analytics-GET (`/twitch/api/v2/*`)

> **In Slices zerlegt:** **1a** = die drei **public** GET-Endpoints (`/public/recent-bans`,
> `/public/recent-raids`, `/public/network`) — kein Auth, pure DB-Reads. Code + hermetische Tests
> fertig (`tb-analytics` + `tb-dashboard-api` + Binary `tb-dashboard`, Bind 127.0.0.1:8767), Proxy-Flip
> noch offen (go-live). **1b** = Auth-Layer (Session-Cookie/Admin-Token/IDOR/Plan-Gating) +
> streamer-scoped Analytics-Routen — eigener Plan. Begründung des Schnitts: 1a vermeidet die gesamte
> Auth-Komplexität und ist der risikoärmste erste Live-Schritt.

- **Live:** Rust `tb-dashboard-api` beantwortet die GET-Analytics-Routen; Reverse-Proxy schaltet
  diese Pfade auf Rust. POST/Admin/Billing bleiben Python.
- **Erfolg:** Shadow-Diff (Python vs Rust JSON) unter Toleranz auf 10–20 Streamern; Frontend zeigt
  identische Werte; p95-Latenz ≤ Python.
- **Rollback:** Proxy-Regel zurück auf Python (1 Reload), kein DB-Schreibpfad berührt.

## Schritt 2 — Public + Read-only Admin-Reads

- **Live:** `public/*`, `admin/streamers`, `system/*` (außer `system/query`) auf Rust.
- **Erfolg:** admin_dashboard-Views identisch; Health/EventSub-Status korrekt.
- **Rollback:** Proxy-Regel zurück.

## Schritt 3 — Interne API 8776 (idempotent)

- **Live:** Rust hält 8776; `tb-bot` (noch ohne Chat/Monitoring) bedient Streamer-CRUD/Global-Ban/
  Raid-Blacklist gegen die DB. `dashboard_service` ruft weiter dieselben Pfade.
- **Erfolg:** Streamer hinzufügen/entfernen über Dashboard funktioniert; Idempotency-Dedup greift;
  `healthz`-Fingerprint matcht.
- **Rollback:** Python-internal-api wieder auf 8776 binden; Outbox-Retry fängt verlorene Calls.

## Schritt 4 — Monitoring *(heikelster DB-Schreibpfad)*

> **Stand: Code komplett (4a–4e gebaut + getestet), Cutover user-gated.** Alles ist hinter
> `TB_MONITORING_POLL_ENABLED` (default aus) — `tb-bot` bedient den EventSub-Dispatch-Endpoint
> sofort, schreibt aber erst beim Flip als Poll-Writer.

- **Live:** Rust übernimmt Stream-Poll + EventSub-Inbox-Verarbeitung + Session-Lifecycle +
  Live-State-Schreiben + Live-Embeds (via Relay). **Python-Monitoring AUS** (sonst Doppel-Insert).
- **Erfolg:** Sessions starten/enden korrekt; `twitch_live_state` konsistent; keine doppelten
  `*_events`-Rows; Capacity-Snapshot plausibel.
- **Rollback:** Python-Monitoring reaktivieren, `TB_MONITORING_POLL_ENABLED=0` + Restart —
  **Wartungsfenster nötig.**

### Flip-Checkliste (Wartungsfenster) — Stand 2026-06-10: vorbereitet

Der Flip ist als Service-Paar umgesetzt (Monitoring **und** Raid gemeinsam,
Schritt 4+6):

1. **Rust-Service:** `deadlock-twitch-bot-rust.service` (User-Unit) startet
   `rust/scripts/run_tb_bot_service.sh` — lädt Secrets via
   `export_infisical_env.py` (wie der Python-Worker; inkl. `DB_MASTER_KEY_V1`
   für die Raid-Token), setzt Callback-URL/Notify-Channel/Target-Game und
   `TB_MONITORING_POLL_ENABLED=1`, exec't `rust/target/release/tb-bot`.
2. **Python-Gate:** `TWITCH_RUST_MONITORING_TAKEOVER=1` (Drop-in
   `20-rust-takeover.conf`) — der Worker startet Poll-Loop, EventSub-Verarbeitung
   und interne API (8776) dann NICHT; Chat/Social/Wartungs-Loops laufen weiter.
   Zusätzlich `TWITCH_INTERNAL_API_LEGACY_PORT=8779`: die Python-interne-API
   läuft auf diesem Seitenport weiter, der Rust-Router (8776) proxyt alle
   noch nicht portierten Routen dorthin (Fallback in `tb-internal-api`,
   `TB_INTERNAL_API_LEGACY_FALLBACK_URL` im Run-Skript).
3. **Flip:** `systemctl --user restart deadlock-twitch-bot` (gibt 8776 frei) →
   `systemctl --user enable --now deadlock-twitch-bot-rust`. Die Dashboard-Bridge
   liefert ab sofort an Rust (gleicher Vertrag, `POST /eventsub/dispatch`);
   gepufferte Outbox-Events laufen nach.
4. Subscriptions: bestehende Webhook-Subscriptions liefern unverändert an dieselbe
   Callback-URL — keine Neuanlage nötig. `SubscriptionManager.rehydrate()` übernimmt das
   Tracking beim Start.
5. **Verifikation:** Live-Streamer geht online → Session öffnet, Embed postet, `stream.offline`
   wird subscribed; offline → Session schließt mit Kennzahlen, Embed wird zum VOD-Overlay,
   Score-Refresh läuft, Auto-Raid-Pfad loggt Entscheidung. Inbox-Dead-Letters = 0,
   Guard-Dedup greift (Log).
6. **Rollback:** `deadlock-twitch-bot-rust` stoppen, Drop-in `20-rust-takeover.conf`
   entfernen, `daemon-reload`, Python-Worker neu starten.

Pre-Cutover-Gate bestanden (2026-06-10): Score-Cross-Check Rust vs. Python auf
Prod-Daten — keine Formelabweichungen (Details im Commit „Score-Cross-Check").

### Kopplungen beim Flip — Status 2026-06-10

1. **Raid-Flows** — ✅ GELÖST: Auto-Raid, `channel.raid`-Arrival, Score-Refresh
   und Blacklist-Raid-Guard laufen echt im Rust-`RaidEventSubHooks`.
   Interim-Lücke: der Streamer-**Whisper** des Blacklist-Guards folgt mit dem
   Chat-Cutover (Bot-Token gehört dem Python-Chat-Prozess); Cancel + Log aktiv.
2. **Telemetrie-Subscription-Anlage für NEUE Partner** (moderator-gated Subs wie
   `channel.follow`/`channel.moderate`, braucht Bot-/Broadcaster-Token) — ⏳ offen
   bis Chat-Phase; Bestand liefert weiter, Rust legt Core-Subs (App-Token) +
   `channel.raid`-Subs an.
3. **Live-Ping-Rollen-Erstellung** (Discord-Gateway) — ⏳ bestehende Rollen-IDs
   nutzt Rust; Neuanlage später via Broker.
4. **Partner-Lifecycle-Ops** Auto-Archiv/Auto-Unarchive — ⏳ no-op wie geplant.
5. **Offline-Seiteneffekte** — ✅ Engagement-Auto-Off + Global-Ban-Sweep-Scheduling
   laufen im Rust-Hook; Post-Stream-Analyse bleibt DB-getrieben im Python-Worker
   (Backfill-/Retry-Job, findet auch Rust-Sessions); Re-Auth-Reminder pausiert
   im Interim (hing am Python-Go-Live-Pfad).
6. **Partner-Rekrutierung + Invite-Refresh** — Invite-Refresh läuft weiter
   (eigener Task im Worker, nicht am Poll-Tick); Rekrutierungs-Anhängsel des
   Poll-Ticks pausiert bis Outreach-Phase (6g).
7. **Manueller `!raid`-Chat-Command** — ✅ GELÖST (6h, 10.6. nachmittags):
   der Python-Chat-Command ist ein dünner Proxy auf
   `POST /internal/twitch/v1/raid/manual` (Rust) — eine Pipeline, ein
   Pending-Store, eine Suppression für Auto- und Manual-Raids. Fallback auf
   den lokalen Python-Pfad nur, wenn der Endpoint nicht erreichbar ist
   (Rollback-Fall).
8. **Nicht portierte interne-API-Routen** — ✅ GELÖST (10.6. abends, nach
   Prod-Bug): das Takeover-Gate schaltete die KOMPLETTE Python-interne-API ab,
   Rust implementiert aber nur die migrierten Routen — Raid-OAuth
   (`/raid/auth-url`, `/raid/go-url`, `/raid/oauth-callback`,
   `/raid/requirements`, `/raid/auth-state`, `/raid/block-state`),
   Raid-Blacklist, Analytics/Stats/Sessions/Live/Debug und
   Self-Explainer-Log liefen ins Leere (Streamer-Onboarding antwortete
   „Raid bot not initialized"). Fix: Strangler-Fig-Fallback — die
   Python-interne-API läuft auf Seitenport 8779 weiter, der Rust-Router
   proxyt unbekannte Routen 1:1 dorthin (`handlers/legacy_proxy.rs`). Jede
   künftig nativ implementierte Route schattet den Proxy automatisch aus.
   Beim Chat-Cutover (Schritt 5) prüfen, welche Routen der Worker dann noch
   braucht.
9. **Paritäts-Drift in Rust-nativen Routen** — ✅ ENTSCHÄRFT (10.6. spät,
   Audit nach dem Prod-Bug): die nativ bedienten `/streamers`-Mutationen
   wichen vom Python-Vertrag ab — snake_case-Bodies der Bestandskonsumenten
   vs. camelCase-Deserialisierung (discord-flag → 422, discord-profile →
   Felder still verworfen + `is_on_discord` auf 0 gezwungen; betroffen auch
   der Discord-Link-Matcher im Deadlock-Bots-Repo), `mode="toggle"`
   (Dashboard-Default beim Archivieren) → 400, verify ignorierte den
   `mode`-Body komplett, chat-action nutzte einen veralteten
   `TWITCH_BOT_TOKEN`-Env-Snapshot (Rotation gehört dem Python-Chat) → 401.
   Fix: kompletter `/streamers`-Baum (inkl. GET — axum würde sonst bei
   Methoden-Mismatch 405 statt Fallback liefern) läuft über den
   Legacy-Proxy; nativ bleiben healthz, eventsub/dispatch, raid/manual,
   globalban. Zusätzlich healthz-`analyticsDbFingerprint` auf das
   Python-Verfahren umgestellt (PBKDF2-Identitäts-Hash statt
   Schema-Fingerprint) — der falsche „different analytics
   databases"-Alarm des Dashboards ist damit weg. Re-Aktivierung nativer
   Routen nur mit Vertragstests gegen die Python-Antworten.

## Schritt 5 — Chat (IRC + Moderation + Promo)

- **Live:** Rust verbindet IRC; Moderation/Auto-Ban/Global-Ban/Promo; Bot-Token wandert in
  `tb-transport-twitch`. Python-Chat aus.
- **Erfolg:** Bans/Timeouts greifen; Promo-Cooldowns eingehalten; Lurker-Tracking schreibt
  `session_chatters`; kein Doppel-Promo (Lock).
- **Rollback:** Python-Chat reaktivieren; Token-Ownership zurück.

## Schritt 6 — Raid (OAuth + Auto-Raid)

- **Live:** Rust `RaidAuth` (DB-only State), Scoring, Auto-Raid-Orchestrator, Rollen-Sync/Invites
  (via Relay). AES-GCM-Interop aus Schritt 0 vorausgesetzt.
- **Erfolg:** OAuth-Flow neu durchlaufbar; bestehende verschlüsselte Tokens lesbar/refreshbar;
  Auto-Raid feuert korrekt; keine Raids auf Blacklist.
- **Rollback:** Python-Raid reaktivieren — solange beide dieselben Blobs lesen können, gefahrlos.

## Schritt 7 — Billing + Stripe-Webhook *(Geld, vertragskritisch)*

- **Live:** Rust `tb-billing`; Stripe-Webhook mit Signatur-Verify; Subscription-Sync; Trial;
  Gutschrift-PDF.
- **Erfolg:** Test-Mode-Checkout + Webhook-Roundtrip korrekt; `streamer_plans` synct; Idempotenz
  über `twitch_billing_events`; PDF byte-plausibel.
- **Rollback:** Webhook-Endpoint zurück auf Python; Stripe retried Webhooks automatisch → kein
  Datenverlust.

## Schritt 8 — Social-Media (Upload-Pipeline)

- **Live:** Rust Clip-Fetch/Enrichment/Upload; Whisper als Sidecar oder `whisper-rs`.
- **Erfolg:** Clip-Upload auf mind. einer Plattform end-to-end; OAuth-Refresh greift.
- **Rollback:** Python-Worker reaktivieren (Queue ist DB-gestützt, idempotent).

## Schritt 9 — Mutierende Admin/Config + Legacy-Form-Actions *(zuletzt)*

- **Live:** `config/*` (POST), `system/query` (mit Guard), Legacy-Form-Actions + JSON-CSRF-Endpoint.
- **Erfolg:** Admin-Aktionen wirken; CSRF hält; alte Form-Pfade liefern erwartete Responses (oder
  Frontend ist bereits auf JSON umgestellt).
- **Rollback:** Proxy zurück; Legacy-HTML-Render bleibt bis admin_dashboard migriert ist.

## Begründung der Reihenfolge

Read-only-Analytics (1–2) ist der sicherste Einstieg — bei Fehler nur falsche Zahlen, kein Schaden,
sofort per Proxy rückrollbar. Monitoring (4) ist der heikelste Schreibpfad und kommt, sobald das
Fundament steht. Geld (Billing, 7) und externe Side-Effects (Social-Uploads, 8) zuletzt, weil
Fehler dort teuer/sichtbar sind — und Stripe bzw. die DB-Queue ohnehin Retry-Sicherheit bieten.

## Stand 11.6. abends — Welle-B-Korrektur + OAuth-Callback + Halbport-Audit

- **#140:** Die in #137 deklarierten 12 Routen waren nie einkompiliert/registriert
  (Geisterlandung). Korrigiert: 7 Routen wirklich nativ und im authed Live-Diff
  gegen 8779 byte-identisch verifiziert (raid/auth-url, auth-state, block-state,
  go-url; live/active-announcements, link-click; analytics/comparison). Geteilter
  Idempotenz-Layer (`tb-internal-api/src/idempotency.rs`) nach `app.py`-Vertrag.
  Regel ab jetzt: „nativ" erst nach Router-Registrierung + Live-Diff.
- **OAuth-Callback komplett portiert** (Code-Tausch + Helix-Token-Owner-Lookup +
  Mismatch-/Scope-Checks + verschlüsselter AuthWriter-Persist + Idempotenz im
  Handler). Bewusst NICHT verdrahtet: Python startet nach `save_auth` die
  Background-Followups `complete_setup_for_streamer` (Moderator-Einsetzung mit
  dem Streamer-Token, Chat-Begrüßung über den Python-Chat-Bot, sofortige
  stream.offline-Subscription, Trial-Timer) bzw. `sync_partner_state_after_auth`
  — die sind noch nicht nativ. Der echte Streamer-Flow läuft ohnehin über
  Dashboard 8765 (Caddy `/callback/twitch` → 8765, in-process). Flip-Bedingung:
  Followups portieren oder delegieren.
- **Halbport-Audit (84 Agenten):** Drei stille No-Ops im Arrival-Wiring gefixt
  (Sekundär-Signal-Update, Orphan-Chat-Notification-Store mit 15s-Promotion,
  verschlucktes Score-Tracking-Resultat) + periodischer 5-Minuten-Voll-Refresh
  aller Partner-Scores ergänzt (war nur event-driven). Typ-Drift-Nachzieher
  gegen das frisch gedumpte Prod-Schema (INT4/TEXT-Decodes + Test-DDLs).
- **tb-dashboard-api (Welle D) — Audit-Urteil: Teil-Neubau nötig.** Blocker vor
  dem Cutover: Partner-Session-Auth (Fernet-Cookie + AuthLevel::Partner) fehlt
  komplett; ~40+ v2-Routen fehlen (Strangler-Proxy oder Port nötig);
  auth-status liefert 2 statt ~18 Felder; /streamers-Shape inkompatibel;
  eventsub-KPIs (websocket_status/capacity) sind Platzhalter. Sofort gefixt:
  recent-raids ohne success-Filter + LIMIT 20→10, CORS-permissive auf
  authed/admin-Routen entfernt (nur public behält CORS wie Python).

## Stand 12.6. — Legal-Komplex nativ + erster tb-dashboard-Live-Betrieb

- **Legal komplett portiert** (`tb-dashboard-api/src/handlers/legal.rs`): vier Seiten
  (impressum/datenschutz/agb/**neu: sicherheit**), Turnstile-Gate (HMAC-SHA256-Cookie,
  Verify in konstanter Zeit), UA-Blockliste, JSON-Override via `TB_LEGAL_PAGES_PATH`
  (Default identisch zu Python: `data/admin_dashboard/legal_pages.json`), robots.txt
  als Parity-Artefakt (Caddy serviert `/robots.txt` real statisch aus dem Landing-Build).
- **Live-Diff bestanden:** alle 5 Renderings (4 Seiten + Gate-Page) byte-identisch
  gegen Python verifiziert — offline via Dev-Test `dump_rendered_pages_fuer_live_diff`
  (`cargo test -p tb-dashboard-api -- --ignored`) plus HTTP-Diff gegen 8765. Einzige
  bekannte Abweichung: Rust percent-encodet `next` im Gate-Redirect-Location (`%2F`),
  Python nicht — semantisch identisch, beide Handler normalisieren gegen die Allowlist.
- **Cutover vollzogen:** neuer Service `deadlock-twitch-dashboard-rust.service`
  (Start-Skript `rust/scripts/run_tb_dashboard_service.sh`, Infisical-Env wie tb-bot).
  Caddy-Flip: `/twitch/{impressum,datenschutz,agb,sicherheit}` + `/twitch/legal/{access,verify}`
  → 8769. Rollback: `Caddyfile.bak-legal-cutover` zurückkopieren + `docker restart caddy`.
- **Port-Realität:** Der Doku-Plan-Port 8767 für tb-dashboard ist seit jeher vom
  Turnier-Public-Cog (Deadlock-Bots `main_bot.py`) belegt → tb-dashboard läuft auf
  **8769** (`DASHBOARD_PORT`). Doku-Stellen mit „8767" sind damit historisch.
- **Inhalt:** AGB von Shop-only auf vollständige Bot-Nutzungsbedingungen erweitert
  (§1–§14: kostenlose Basisnutzung, Partnerprogramm/OAuth, Auto-Moderation + Bannliste
  mit Einspruchsweg, KI-Funktionen, Pflichten, Haftung für unentgeltliche Leistungen)
  + neue öffentliche Seite `/twitch/sicherheit` (ungegated, indexierbar). Python-Mixin
  bleibt synchroner Fallback — Inhaltsänderungen in beiden Defaults nachziehen.
- **Hinweis Welle D:** Die v2-/Session-Auth-Blocker aus dem 11.6.-Audit bleiben
  unverändert offen; der Legal-Cutover ist davon unabhängig (kein Partner-Auth,
  keine DB-Writes — nur Pool-Connect beim Start).

## Stand 12.6. nachts — OAuth-Callback-Flip (Followups nativ)

- **Followups portiert** (`tb-raid/src/partner_setup.rs` + Composition
  `tb-bot/src/oauth_followups.rs`): `complete_setup_for_streamer` +
  `sync_partner_state_after_auth` inkl. `promote_streamer_to_partner`
  (Identity-Upsert mit Steal, Related-Tables-Normalisierung mit
  Savepoint-je-Statement, clear_source, Stats-Backfill) und
  `_record_first_login` (COALESCE-idempotent). 13 DB-Tests gegen prod-treue
  DDL (alle `twitch_partners`-Timestamps TEXT, Flags INTEGER).
- **Externe Seiteneffekte:** Moderator-Einsetzung nativ via Helix
  `POST /moderation/moderators` mit Streamer-Token
  (`tb-transport-twitch/src/moderation.rs`); Discord-Display-Name +
  Streamer-Rolle via Master-Broker (neue Broker-Route `resolve-user`,
  Deadlock-Bots #118, + bestehendes `member/add-role`); Chat-Begrüßung
  delegiert an den Python-Chat via `POST /streamers/{login}/chat-action`
  auf 8779 (Interim bis Welle B; Join übernimmt der periodische
  Partner-Join-Loop des Python-Chats, die Nachrichten gehen Helix-seitig).
  `stream_went_live_fn` entfällt: der native Poll (15 s) ersetzt den
  Sofort-Sub-Call (Python-Lücke war 15–45 min).
- **Bot-Identität:** `TWITCH_BOT_USER_ID=1422558159`
  (deutschedeadlockcommunity, via Chat-Log + Helix verifiziert) als
  Run-Skript-Default — Python löst die ID zur Laufzeit aus dem Chat-Token,
  den Rust noch nicht besitzt.
- **Flip vollzogen:** `POST /raid/oauth-callback` nativ registriert
  (schattet den 8779-Proxy aus). Verifikation: Fehlerpfade (error-Param,
  redirect_mismatch, leerer Body, unbekannter State) live gegen Python
  gedifft — semantisch identisch (Byte-Differenz nur JSON-Key-Reihenfolge +
  Whitespace; Konsumenten parsen feldweise); Idempotenz-Replay live grün
  (`x-idempotency-replayed`, identischer Body). Der Erfolgspfad ist nicht
  live-diffbar (Single-Use-State) — abgedeckt durch die Port-Tests aus #145
  + Followup-Tests. Echter Streamer-Flow läuft weiterhin über Dashboard
  8765 in-process (Welle D).
- **Python-Bug gefunden (bewusste Rust-Abweichung):**
  `promote_streamer_to_partner` wipet bei jeder Re-Promotion
  `require_discord_link`/`silent_ban`/`silent_raid` auf 0,
  `live_ping_role_id` auf NULL, `live_ping_enabled` auf 1 — die
  `_row_value`-Defaults machen die vorgesehenen active_row-Fallbacks zu
  totem Code (Folge der Partner-DB-Konsolidierung: die Quell-Spalten sind
  Literale). Rust bewahrt die Werte des aktiven Partners. Python-Fix steht
  aus (betrifft Re-Auth über Dashboard 8765 + /streamers-Verify-Pfade).
- **Nebenbefund:** `bot/billing/catalog.py:21` — Kommentar sagt
  „45-day free trial", Konstante ist 30. Laufzeitwert 30 ist korrekt.

## Stand 12.6. spät — Analytics-Triple nativ (/stats, /analytics/streamer, /sessions)

- **Drei Routen nativ** (Worktree-Drafts von Sonnet-Agents + hartes Review +
  Live-Diff): `handlers/session_detail.rs` (dynamischer SELECT-*-Mapper —
  künftige Spalten kommen wie in Pythons `_row_to_dict` automatisch mit),
  `handlers/stats_native.rs`, `handlers/streamer_analytics_native.rs`.
  Die alten shape-inkompatiblen Versuche in `streamers.rs` bleiben
  unregistriert (Referenz).
- **Verifikation:** sessions/:id 4 echte Prod-Sessions + 404/400 = 0 Diffs.
  /stats nach Fixrunde: Top-Level-Keymenge identisch, monetization +
  eventsub-DB-Teile 0 Diffs, verbleibende Diffs = Live-Daten-Drift zwischen
  den Requests. analytics/streamer: beide Empty-Varianten byte-gleich.
- **Review-Funde in den Drafts (Typ-Drift-Klasse, vor dem Commit gefixt):**
  (a) Partner-State-View liefert INTEGER/TEXT — bool/DateTime-Decode schlug
  still fehl (`unwrap_or(None)`) → alle `is_partner=0`; (b) Monetization-
  Cutoff als String gegen TIMESTAMPTZ gebunden → Sektion fehlte still
  (sqlx sendet konkretes TEXT, psycopg „unknown"); (c) Agent-Test-DDLs waren
  NICHT prod-treu (BOOLEAN/TIMESTAMPTZ statt INTEGER/TEXT) und versteckten
  beides — DDLs korrigiert; (d) /stats-Vertrag des Extraktor-Agents enthielt
  4 Fremd-Sektionen, echte Quelle ist `_dashboard_stats` (tracked/category/
  avg_* + eventsub + monetization, je try/except).
- **Zwei Python-Prod-Bugs dokumentiert:**
  (1) `analytics/streamer` crasht in Python seit der Postgres-Migration für
  jeden Streamer MIT Daten — `TIME(s.started_at)` in
  `backend_extended.py:_get_session_list` ist SQLite-Syntax →
  `{"error":"Internal error","empty":true}`; das Streamer-Dashboard zeigte
  deshalb nie Analytics. Der native Port repariert das.
  (2) Pythons Monetization-subs-Query `is_gift=1` gegen die BOOLEAN-Spalte
  schlägt fehl → subs immer 0; Rust nutzt `is_gift IS TRUE` (bewusste
  Abweichung, liefert echte Werte).
- **EventSub-`active_*`-Listen:** kommen aus In-Process-State — Python 8779
  liefert seit dem Takeover Stales, Rust leere Listen bis der
  `EventSubStatsSource`-Port an den SubscriptionManager verdrahtet ist
  (Followup). Die DB-Capacity-Teile sind paritätisch.

### Restfläche interne API nach 12.6. (Welle A im Wesentlichen abgeschlossen)

Nativ (21): healthz, eventsub/dispatch, raid/manual, chat/command,
discord-invite, globalban×4, raid/blacklist×4, streamers/link-candidates,
market-share, discord/self-explainer-log, raid/auth-url+auth-state+
block-state+go-url+oauth-callback, live/active-announcements+link-click,
analytics/comparison, **/stats, /analytics/streamer/:login,
/sessions/:session_id**.

Proxied (14, mit Grund): /streamers-Baum (GET/POST/DELETE + verify/archive/
discord-flag/discord-profile/chat-action — Partner-Lifecycle promote/departner
+ Discord-DM/Rollen-Sync + rotierter Bot-Token; Lifecycle-Bausteine existieren
seit dem Followup-Port in tb-raid::partner_setup!), raid/requirements
(Discord-DM → Broker send-dm wäre jetzt möglich), debug/observability +
debug/eventsub-processing + debug/chatters (Python-In-Process-State),
eventsub/processing/requeue (dito).

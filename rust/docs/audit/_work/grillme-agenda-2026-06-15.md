# Grillme-Agenda — Python→Rust Parity-Audit Twitch-Bot (2026-06-15)

## Methodik

Quelle: `findings-2026-06-15.json` (501 verifizierte Audit-Befunde) + `openquestions-2026-06-15.json` (360 offene Fragen).
Fokus dieser Agenda: die **309 Befunde mit `needsDecision==true`** (übrige Befunde + Open Questions dienen als Beleg/Kontext).
Geclustert wird nach **Entscheidungsachse / Feature-Bereich**, nicht nach Subsystem. Jeder der 309 Befunde liegt in genau **einem** Cluster (Abdeckung am Ende).

**Zielbild:** 1:1-Port (gleiche Funktionen, gleicher An/Aus-Zustand). Bugs, die an der Schema-/Cutover-Umstellung scheitern, werden gefixt statt mitgeschleppt. Pro Punkt entscheidet der User: **portieren / fixen / droppen / aus-lassen / klären**.

**Bereits entschieden (nicht mehr zur Debatte, nur Kontext):**
- Das **Dashboard** (Login/Abo/Stripe/Admin — alle live 502, weil Python 8765/8769 tot) wird **NATIV in Rust nachgebaut** (Welle D, Pflicht-Block #1). Kein Python-Rollback.
- Korrigierte Audit-Annahmen: `social_media` ist **fast voll portiert** (keine große Lücke); das **Dashboard ist schlimmer als angenommen** (live komplett tot).

**Gesamt-Rollup der 309 Entscheidungs-Befunde:** 5 critical · 50 high · 79 medium · 175 low.
**Nach Klasse:** A-live 17 · B-cutover 27 · C-lost 115 · D-divergence 114 · E-state 36.

Cluster sind nach realer Wirkung priorisiert: A-live & crit/high zuerst, dann B-cutover, dann C/D/E.

---

## Cluster 1 — Dashboard Admin/Partner-Verwaltung — LIVE tot (HTML-Seiten + Schreib-Aktionen)

- **Theme-Klasse:** A-live-regression / B-cutover-blocker (Dashboard nativ-Nachbau, Scope)
- **Findings:** 27  ·  **Schwere:** 3 crit / 12 high / 10 med / 2 low
- **Betroffene Subsysteme:** dashboard
- **Kern-Entscheidung:** Welche Admin-Dashboard-Seiten und Schreib-Aktionen muss der native Rust-Nachbau (Welle D) in welcher Reihenfolge abdecken, damit Partner-Verwaltung wieder funktioniert?

**Wichtigste Einzel-Entscheidungen:**

- `[live-1]` **CRIT/A** — Komplette /twitch HTML-Admin-Seite (live.py index) liefert real HTTP 502 — Python-Backend tot, kein natives Rust-Pendant
    - *Wirkung:* Admin oeffnet /twitch oder /twitch/admin und bekommt nur {"error":"legacy_upstream_unavailable"} (HTTP 502). Die gesamte Partner-Verwaltungs-Oberflae…
- `[live-2]` **CRIT/A** — Schreib-Aktion add_streamer (Partner hinzufuegen) ohne natives Pendant — POST /twitch/add_streamer = 502
    - *Wirkung:* Neuen Twitch-Partner ueber das Admin-Dashboard hinzufuegen ist nicht moeglich; das Formular postet auf /twitch/add_streamer, das per Proxy 502 liefer…
- `[dashboard-composition-root-06]` **CRIT/A** — Admin-SPA (/twitch/admin) + Admin-forward_auth (validate_admin_session) ohne nativen Port — Admin-Host komplett tot
    - *Wirkung:* Die Admin-Oberflaeche ist komplett aus: /twitch/admin -> 502 (Live-Probe auf 8769), und der Admin-Host (Caddyfile:602-733) proxied /twitch/admin + fo…
- `[dashboard-live-announcement-mixin-1]` **HIGH/A** — Live-Announcement-Builder (gesamtes Mixin) nicht nativ portiert -> 502 live
    - *Wirkung:* Der komplette Go-Live-Announcement-Builder (Seite /twitch/live-announcement + APIs /twitch/api/live-announcement/{config,preview,test}) hat keinen Ru…
- `[dashboard-live-announcement-mixin-2]` **HIGH/A** — Dashboard-Title-Generator-Routen (suggest/insights) tot trotz vorhandenem Backend
    - *Wirkung:* POST /twitch/api/v2/title/suggest und GET /twitch/api/v2/title/insights sind nicht nativ registriert -> Proxy -> 502 (verifiziert: title/insights ->…
- `[live-3]` **HIGH/A** — Schreib-Aktion verify (Partner-Verifizierung permanent/30-Tage/failed/clear) ohne Pendant — POST /twitch/verify = 502
    - *Wirkung:* Admin kann den Verifizierungs-Status eines Partners (Permanent / 30 Tage / Verifizierung fehlgeschlagen / Kein Partner) nicht mehr setzen — Formular…
- `[dashboard-pages-misc-1]` **HIGH/B** — Markt-Research-Dashboard (/twitch/market + /twitch/api/market_data) ohne natives Rust-Pendant
    - *Wirkung:* Das interne Markt-Research-Dashboard (DACH-Marktvolumen, Meta-Snapshot, Sentiment, Viewer-Overlap, Question-Radar, Live-Channels) wird ausschliesslic…
- `[dashboard-core-stats-1]` **MEDI/B** — Partner-Stats-HTML-Seite (/twitch/partners) ohne Rust-Port -> 502 bei totem Python
    - *Wirkung:* Ruft jemand die alte server-gerenderte Partner-Stats-Seite (Top-Partner/Kategorie-Tabellen, Stunden-/Wochentag-Charts, User-Fokus mit Shared-Audience…

**Empfehlung (Default):** **Portieren/nativ nachbauen (Pflicht-Block #1).** Höchste reale Wirkung: gesamte Partner-Verwaltung liefert live 502. Scope = alle live.py-Schreib-Aktionen + Live-Announcement-Builder + Entry-Routen zuerst; Market-Research/Roadmap-HTML als P2 nachziehen.

---

## Cluster 2 — Affiliate-/Billing-/Stripe-Programm — Welle-C-Drops (Provisionen, Gutschrift-PDF, Stripe-Connect)

- **Theme-Klasse:** B-cutover-blocker / C-lost-subsystem (Billing-Welle-C)
- **Findings:** 14  ·  **Schwere:** 2 crit / 8 high / 4 med / 0 low
- **Betroffene Subsysteme:** analytics, dashboard
- **Kern-Entscheidung:** Wird das Affiliate-Programm (30% Provision, Stripe-Connect-Onboarding, monatliche Gutschrift-PDFs, USt-Logik) in Rust nachgebaut, oder als Billing-Welle-C-Feature gedroppt/zurückgestellt?

**Wichtigste Einzel-Entscheidungen:**

- `[affiliate-shim+streamer-admin-1]` **CRIT/C** — Affiliate-Partner-Portal komplett tot: Strangler-Proxy zeigt auf abgeschaltetes Python 8765 (502)
    - *Wirkung:* Affiliates erreichen ihr Portal nicht: /twitch/affiliate/portal, /twitch/auth/affiliate/login, /twitch/api/affiliate/me, /twitch/affiliate/claim, Str…
- `[affiliate-shim+streamer-admin-2]` **CRIT/C** — Provisions-Erzeugung bei Stripe-Zahlung nicht portiert — kein Rust-Webhook ruft _affiliate_process_commission
    - *Wirkung:* Bei einer Streamer-Abo-Zahlung wird kein Affiliate-Commission-Datensatz mehr angelegt (30% Provision, Pending-Cap 5000ct, Idempotenz via stripe_event…
- `[affiliate-portal-03]` **HIGH/B** — Provisions-Verbuchung beim Stripe-invoice.payment_succeeded-Webhook nur in Python
    - *Wirkung:* Die Kernlogik des Affiliate-Programms — bei jeder erfolgreichen Abo-Zahlung 30% Provision (_COMMISSION_RATE=0.30) verbuchen, Pending-Cap 5000 Cent (_…
- `[affiliate-portal-04]` **HIGH/B** — Monatliche Gutschrift-Generierung + 6h-Background-Loop nur in Python
    - *Wirkung:* Die PDF-Gutschrift-Erzeugung (fpdf2, USt/Kleinunternehmer-Logik, GS-Nummern-Counter, Email-Versand) und der 6h-Hintergrund-Loop, der faellige Periode…
- `[affiliate-portal-02]` **HIGH/B** — Stripe-Connect-Onboarding (connect + callback) nur via Python-Proxy
    - *Wirkung:* Stripe-Connect-Strecke (Authorize-Redirect, Token-Exchange, stripe_account_id-Persist + Replay aller pending Provisionen) ist nicht nativ in Rust. Pf…
- `[affiliate-portal-05]` **HIGH/B** — PII-Schreibpfad (save_pii/Verschluesselung/migrate_from_plaintext) nicht in Rust portiert
    - *Wirkung:* Rust portiert nur den PII-LESE-Pfad (vom Admin-Detail genutzt). Der Schreibpfad — Verschluesselung der Stammdaten (full_name/email/Adresse/tax_id) be…
- `[billing-mixin-3]` **HIGH/B** — Checkout-Preview + Invoice-Preview/Render (HTML-Rechnung) nicht portiert
    - *Wirkung:* POST /twitch/api/billing/checkout-preview und /twitch/api/billing/invoice-preview sowie die abbo-Rechnungsseite (/twitch/abbo/rechnung) laufen über P…
- `[affiliate-shim+streamer-admin-3]` **HIGH/C** — Gutschrift-Generierung, Stripe-Auszahlung und 6h-Background-Loop nicht portiert
    - *Wirkung:* Es werden keine neuen Gutschrift-PDFs mehr erzeugt, keine faelligen Perioden abgearbeitet (run_pending/due_periods/generate_monthly), keine Stripe-Tr…

**Empfehlung (Default):** **Klären (Welle-C-Entscheidung).** Kein live laufender Provisions-Pfad mehr → bei aktiven Affiliates Geld-/Rechtsrelevanz (USt, Gutschriften). Default: nativ in Welle C nachbauen, NICHT still droppen; bis dahin als bekannter Ausfall markieren.

---

## Cluster 3 — Dashboard-Auth / Session / Partner-Login / Fingerprint (Login-Flow Cutover)

- **Theme-Klasse:** A/B/C/D/E gemischt (Dashboard nativ-Nachbau, Auth-Schicht)
- **Findings:** 18  ·  **Schwere:** 0 crit / 4 high / 5 med / 9 low
- **Betroffene Subsysteme:** dashboard, dashboard_service
- **Kern-Entscheidung:** Welche Auth-Bausteine des Dashboards (Discord-Link, Partner-One-Time-Login, CSRF, Fingerprint, Strangler-Fallback) gehören in den nativen Nachbau und welche werden bewusst gedroppt?

**Wichtigste Einzel-Entscheidungen:**

- `[auth-core-3]` **HIGH/A** — Discord-Link-Flow (Twitch-Streamer verknuepft Discord) nicht portiert: 502
    - *Wirkung:* Live verifiziert: /twitch/auth/discord/link/login gibt 502. Ein eingeloggter Partner kann seinen Discord-Account nicht (mehr) ueber das Dashboard ver…
- `[auth-core-4]` **HIGH/C** — Partner-Login-Token-Service (signierte One-Time-Login-Tokens) komplett unportiert
    - *Wirkung:* Der HMAC-signierte Partner-Login-Token-Flow (issue/consume, Autorisierungsmodus noauth/localhost/admin_header/admin_session) hat kein Rust-Pendant. R…
- `[auth-partner-fingerprint-shim-1]` **HIGH/C** — Partner-Einmal-Login-Flow (auth_partner_link + auth_partner_login) nicht portiert und Proxy-Ziel tot
    - *Wirkung:* POST /twitch/auth/partner/link und POST /twitch/auth/partner/login existieren nicht nativ in Rust. tb-dashboard reicht sie an den Strangler-Proxy (TB…
- `[auth-partner-fingerprint-shim-7]` **HIGH/B** — Strangler-Fallback-Proxy zeigt auf totes Python (8765) — gesamter Dashboard-Login-Flow liefert live 502
    - *Wirkung:* run_tb_dashboard_service.sh:66 setzt TB_DASHBOARD_LEGACY_FALLBACK_URL default auf http://127.0.0.1:8765 → Proxy AKTIV, aber kein Prozess lauscht auf…
- `[dashboard_service-app-bootstrap-2]` **MEDI/B** — tb-dashboard Strangler-Fallback zeigt weiter auf das tote Python 8765 — jede nicht nativ portierte v2-Route faellt auf 502 statt auf einen lebenden Upstream
    - *Wirkung:* Jede Dashboard-Route, die in tb-dashboard-api/lib.rs NICHT nativ registriert ist, wird vom Fallback-Proxy an http://127.0.0.1:8765 weitergereicht — d…
- `[auth-core-6]` **MEDI/D** — auth-status liefert csrfToken/csrf_token immer null statt echtem CSRF-Token
    - *Wirkung:* Python fuellt csrfToken mit einem echten, an die Session gebundenen CSRF-Token (generiert falls fehlend). Rust setzt csrfToken und csrf_token fix auf…
- `[auth-partner-fingerprint-shim-3]` **MEDI/E** — Partner-Access-Cookie (twitch_dash_session_partner) vom Rust-Auth-Level nicht konsumiert
    - *Wirkung:* In Python gewaehrt eine gueltige Partner-Access-Session (Cookie twitch_dash_session_partner, auth_type=partner_token) auth_level='partner'. Der Rust-…
- `[auth-partner-fingerprint-shim-2]` **MEDI/C** — Geraete-/Canvas-Fingerprint-Sammlung nach Discord-Admin-Login (fingerprint_page/submit) nicht portiert
    - *Wirkung:* Nach erfolgreichem Discord-Admin-Login setzt Python fp_pending=True und leitet auf GET /twitch/auth/fingerprint weiter (auth_mixin.py:1603); die JS-S…

**Empfehlung (Default):** **Portieren mit Modernisierung.** Discord-Link + Partner-Login + CSRF sind Pflicht für funktionierenden Login. Fingerprint-Sammlung + NOAUTH-Debug + Strangler-Fallback auf totes 8765 droppen/entfernen. Auth-Level-State-Mismatches (Partner-Cookie, Doppel-Cookie) fixen.

---

## Cluster 4 — Token-Lifecycle: Bot-Ban-Recovery, Grace-Periods, Token-Error-DMs (api)

- **Theme-Klasse:** B-cutover-blocker / C-lost-subsystem
- **Findings:** 9  ·  **Schwere:** 0 crit / 5 high / 2 med / 2 low
- **Betroffene Subsysteme:** api
- **Kern-Entscheidung:** Werden die Token-Fehler-Reaktionen (Admin-Embed, User-DM mit Reconnect-Button, Bot-Ban-Opt-out + Auto-Recovery, 7-Tage-Grace-Rollenentzug, Blacklist-Cleanup) in Rust nachgebaut oder gedroppt?

**Wichtigste Einzel-Entscheidungen:**

- `[token-lifecycle-5]` **HIGH/B** — handle_bot_banned_channel + Recovery-DM (Channel-seitiger Bot-Ban-Opt-out) nicht portiert
    - *Wirkung:* Wird der Bot in einem Channel gebannt/blockiert (chat/connection.py + moderation.py erkennen das), ruft Python handle_bot_banned_channel: _mark_partn…
- `[token-lifecycle-1]` **HIGH/B** — notify_token_error (Admin-Channel-Embed) nicht in Rust portiert
    - *Wirkung:* Bei invalid_grant/invalid Streamer-Token postet Python ein Embed in den Admin-Channel 1374364800817303632 (einmal pro Streamer via notified-Flag) und…
- `[token-lifecycle-2]` **HIGH/B** — User-DM bei Token-Fehler (_send_user_dm_token_error) + RaidAuthGenerateView-Button nicht portiert
    - *Wirkung:* Python schickt dem betroffenen Streamer eine DM mit konkreten Ursachen/Loesung + persistentem Reconnect-Button (RaidAuthGenerateView) und markiert us…
- `[token-lifecycle-3]` **HIGH/C** — check_grace_periods (Rollenentzug + manual_opt_out nach 7 Tagen) nicht portiert und nicht geschedult
    - *Wirkung:* Python prueft stuendlich (bot/raid/bot.py:307) abgelaufene Grace-Periods: schickt Reminder-DM + Admin-Notification, entzieht die Streamer-Rolle via D…
- `[token-lifecycle-6]` **HIGH/C** — restore_bot_banned_channel (Aufhebung der Bot-Ban-Pause nach Health-Restore) nicht portiert
    - *Wirkung:* Python hebt den technischen Bot-Ban-Opt-out automatisch wieder auf, sobald der Bot wieder gesund ist (analytics/mixin.py:183 ruft restore_bot_banned_…
- `[token-lifecycle-4]` **MEDI/C** — cleanup_old_entries (Blacklist-Aufraeumung >30 Tage) nicht portiert und nicht geschedult
    - *Wirkung:* Python loescht alle 3.5h (bot/raid/bot.py:302) Blacklist-Eintraege mit last_error_at aelter als 30 Tage. In Rust kein Pendant und kein Scheduler. Nac…
- `[token-lifecycle-7]` **MEDI/C** — _migrate_db Idempotente Schema-Migration + bot_banned-Backfill nicht in Rust repliziert
    - *Wirkung:* Pythons TokenErrorHandler.__init__ fuehrt eine idempotente Migration aus: fuegt grace_expires_at/user_dm_sent/reminder_sent/role_removed zu twitch_to…

**Empfehlung (Default):** **Portieren.** Ohne diese laufen Token-Ausfälle still ins Leere: kein Admin-Alert, kein User-Reconnect-Pfad, gebannte Bots bleiben tot, Grace-Period-Rollen werden nie entzogen. Hohe Betriebs-Wirkung; Scheduler (stündlich/3.5h) mitportieren.

---

## Cluster 5 — EventSub-/Telemetrie-Subscriptions + WS-Transport + Dead-Letter (Cutover-Verlust)

- **Theme-Klasse:** C-lost-subsystem / D-divergence
- **Findings:** 20  ·  **Schwere:** 0 crit / 2 high / 7 med / 11 low
- **Betroffene Subsysteme:** api, internal_api, monitoring
- **Kern-Entscheidung:** Welche EventSub-Subscriptions (first_message, follow/ban/unban/shoutout) und Transport-/Inbox-Mechanismen (WS-Fallback, Dead-Letter-Hook, Requeue, Helix-Subs/Ads/Chatters) werden nachgebaut, welche bleiben bewusst aus?

**Wichtigste Einzel-Entscheidungen:**

- `[eventsub-mixin-1]` **HIGH/C** — channel.chat.user_first_message-Subscription wird in Rust nie angelegt
    - *Wirkung:* first_message-Events erreichen Rust nicht mehr, sobald die alt-Python-erstellten Twitch-Subs auslaufen. Folge: keine twitch_first_message_events-Inse…
- `[eventsub-mixin-2]` **HIGH/C** — Moderator-Telemetrie-Subs (ban/unban/shoutout/follow) in Rust bewusst weggelassen
    - *Wirkung:* channel.follow/ban/unban/shoutout-Events werden nicht mehr abonniert -> twitch_follow_events (Follower-Funnel #176), twitch_ban_events (Ban-Analytics…
- `[api-helix-4]` **MEDI/C** — Helix /chat/chatters (get_chatters / get_chatters_result) hat kein Rust-Pendant
    - *Wirkung:* Python pollt /chat/chatters per Bot-/Streamer-Token (Scope moderator:read:chatters), inkl. Cursor-Pagination, Mod-Self-Heal bei 403 und Observability…
- `[api-helix-3]` **MEDI/C** — Helix /subscriptions (get_broadcaster_subscriptions / _result) hat kein Rust-Pendant
    - *Wirkung:* Python liest die Abo-Daten eines Broadcasters per User-Token (Scope channel:read:subscriptions) fuer die Analytics-Insights (mixin.py:543). In Rust g…
- `[api-helix-2]` **MEDI/C** — Helix /channels/ads (get_ad_schedule / get_ad_schedule_result) hat kein Rust-Pendant
    - *Wirkung:* Python holt den Ad-Schedule eines Broadcasters per User-Token (Scope channel:read:ads) und schreibt ihn in twitch_ads_schedule_snapshot (Producer in…
- `[inbox-guard-1]` **MEDI/D** — Dead-Letter-Hook im Rust-Produktivpfad nicht verdrahtet (kein kritischer Alarm, kein Supervisor-Wakeup)
    - *Wirkung:* Wenn ein EventSub-Auftrag (stream.online/offline, channel.update, channel.raid) nach 5 Versuchen dead-lettert, schreibt Rust nur ein tracing::error!…
- `[inbox-guard-2]` **MEDI/C** — Requeue-Endpoint ist No-op-Stub — funktionaler Store-Requeue nicht erreichbar
    - *Wirkung:* Der Admin-Endpoint POST /eventsub/processing/requeue holt einen Dead-Letter-Auftrag NICHT zurück in die Queue (antwortet pauschal ok:true, requeued:0…
- `[U66-1]` **LOW/C** — EventSub-WebSocket-Transport komplett gedroppt (kein Rust-Port, kein Proxy)
    - *Wirkung:* Faellt die Webhook-Strecke aus oder ist sie nicht konfiguriert, hat Rust KEINEN EventSub-Transport mehr — im Python-Bot waere automatisch der WS-Fall…

**Empfehlung (Default):** **Teilweise portieren + klären.** first_message-Sub + Dead-Letter-Alarm/Requeue portieren (Datensammlung + Betriebssicht). Mod-Telemetrie-Subs (follow/ban/unban) und WS-Transport sind dokumentierte ADR-Drops → mit User bestätigen, ob die abhängigen Analytics (Follower-Funnel, Ban-Analytics) wirklich aufgegeben werden.

---

## Cluster 6 — Analytics-Datensammlung verloren: Chatters-Lurker-Poller + Subs/Ads-Snapshots

- **Theme-Klasse:** C-lost-subsystem
- **Findings:** 2  ·  **Schwere:** 0 crit / 1 high / 1 med / 0 low
- **Betroffene Subsysteme:** analytics
- **Kern-Entscheidung:** Werden die periodischen Helix-Poller (Chatters→Lurker alle 30s, Subscriptions/Ads alle 6h) in Rust nachgebaut, oder bleiben die Snapshot-Tabellen dauerhaft leer?

**Wichtigste Einzel-Entscheidungen:**

- `[analytics-api_v2-mixin-1]` **HIGH/C** — Helix-Chatters-Lurker-Poller (collect_chatters_data) hat keinen Rust-Port und keinen laufenden Python-Fallback
    - *Wirkung:* Stille Lurker (Zuschauer die nie tippen) werden nicht mehr per Helix GET /chat/chatters alle 30s erfasst. twitch_session_chatters-Lurker-Zeilen (seen…
- `[analytics-api_v2-mixin-2]` **MEDI/C** — Subs/Ads-Snapshot-Poller (collect_analytics_data, 6h) nicht portiert; Snapshot-Tabellen werden gelesen aber nie befüllt
    - *Wirkung:* Der 6-stündliche Helix-Poll von get_broadcaster_subscriptions / get_ad_schedule (Scopes channel:read:subscriptions / channel:read:ads) entfällt. twit…

**Empfehlung (Default):** **Portieren.** Ohne den Chatters-Poller werden stille Lurker nicht mehr erfasst; Subs/Ads-Snapshot-Tabellen werden gelesen, aber nie befüllt → tote Analytics-Panels. Eng gekoppelt an die Helix-Wrapper-Lücken in C05.

---

## Cluster 7 — Raid-Subsystem: verlorene Tracking-/Arrival-/Recruitment-Mechanik + Token-Wartung

- **Theme-Klasse:** C-lost-subsystem / D-divergence / E-state-mismatch
- **Findings:** 33  ·  **Schwere:** 0 crit / 4 high / 10 med / 19 low
- **Betroffene Subsysteme:** raid
- **Kern-Entscheidung:** Welche Raid-Nebenpfade (channel.chat.notification-Sekundär-Confirm, Unraid-Withdraw, Deadlock-Score-Resolve, proaktiver Token-Refresh, Bot-Ban-Check-Drain, Discord-Befehle, Requirements-DM) werden nachgebaut, gefixt oder als dormant akzeptiert?

**Wichtigste Einzel-Entscheidungen:**

- `[raid-arrival-tracking-01]` **HIGH/C** — channel.chat.notification-Raidmeldung wird nie an die Arrival-Runtime dispatcht (kein Rust-Port, kein Python-Proxy)
    - *Wirkung:* Raids, die nur ueber die Chat-Ankuendigung (channel.chat.notification, notice_type=raid) sichtbar werden, ohne vorheriges/begleitendes EventSub chann…
- `[raid-scores-tracking-1]` **HIGH/C** — resolve_partner_raid_tracking_for_session nicht portiert — Deadlock-Raid-Tracking-Zeilen werden nie aufgeloest
    - *Wirkung:* Bei einem bestaetigten Partner-Raid, der WAEHREND Deadlock ankam, schreibt track_confirmed die Zeile mit deadlock_continued_until=NULL, deadlock_cont…
- `[raid-blacklist-partner-setup-1]` **HIGH/C** — Bot-Ban-Check-Drain (process_due_external_target_ban_checks) hat keinen Rust-Orchestrator — geplante Checks laufen ins Leere
    - *Wirkung:* Nach jeder externen Recruitment-Nachricht wird ein Bot-Ban-Check in twitch_external_bot_ban_check_pending eingeplant (run_after = now+1h). In Python…
- `[raid-views-arrival-1]` **HIGH/C** — Discord-Raid-Auth-Views (views.py) ohne Rust-Port; requirements-DM dauerhaft 503
    - *Wirkung:* Der POST /raid/requirements-Endpoint des Rust-Bots liefert bewusst 503 statt die Aktivierungs-DM (Anforderungs-Embed + 'Bot fuer deinen Kanal aktivie…
- `[raid-core-2]` **MEDI/C** — Proaktiver Hintergrund-Token-Refresh (_periodic_cleanup -> refresh_all_tokens) nicht portiert
    - *Wirkung:* Python refresht alle 30min proaktiv alle raid_enabled-/nicht-reauth-Tokens, die in <2h ablaufen (refresh_all_tokens, sequentiell, mit Cooldown/Blackl…
- `[raid-facades-1]` **MEDI/C** — channel.chat.notification raid/unraid Signalpfad nicht verdrahtet (Sekundär-Confirm, Orphan-Korrelation, Source-Self-Unraid-Cancel dormant)
    - *Wirkung:* Twitch liefert pro Raid sowohl channel.raid (ans Ziel) als auch eine channel.chat.notification (Raid-Notice). Python nutzt die Chat-Notice als zweite…
- `[raid-partner-delivery-02]` **MEDI/D** — Manuelles !raid raidet in Rust auch nach Stream-Ende (DB-Fallback), Python verlangt Live-Quelle
    - *Wirkung:* Python: Tippt jemand !raid, waehrend der Broadcaster laut Twitch-API offline ist, antwortet der Bot mit source_not_live und raidet NICHT. Rust: greif…
- `[raid-auth-04]` **MEDI/E** — Re-Auth ohne discord-id im OAuth-State reaktiviert Partner nicht (raid_bot_enabled/opt_out/backfill)
    - *Wirkung:* Re-autorisiert ein bestehender Streamer ueber einen Link OHNE discord:<id> im State (z.B. plainer Re-Auth/Website), schreibt Rust zwar das Token und…

**Empfehlung (Default):** **Gemischt: Kern-Pfade fixen, Diagnostik/dormante droppen.** Proaktiver Token-Refresh + Bot-Ban-Check-Drain + chat.notification-Sekundär-Confirm + Deadlock-Score-Resolve portieren (sonst stille Daten-/Token-Lücken). Discord-/streamer-Flow-ersetzte Befehle (/traid etc.) und Voice-Reaction-Followup als bewusste Drops bestätigen; Observability-Stubs droppen.

---

## Cluster 8 — Chat-Connection: Mod-Recovery, Ban-Blacklist, Kanal-Selektion, Subscription-Ableitung

- **Theme-Klasse:** C-lost-subsystem / D-divergence
- **Findings:** 14  ·  **Schwere:** 0 crit / 1 high / 4 med / 9 low
- **Betroffene Subsysteme:** chat
- **Kern-Entscheidung:** Welche Chat-Join-/Connection-Mechaniken (403-Mod-Self-Heal, Bot-Ban-Blacklisting, raid_enabled-Kanal-Selektion, Subscription-Events aus channel.chat.notification, IRC-Restart) werden nachgebaut?

**Wichtigste Einzel-Entscheidungen:**

- `[chat:connection-subscriptions-2]` **HIGH/C** — 403-Mod-Retry-Recovery beim Join nicht portiert
    - *Wirkung:* In Python: schlaegt die Chat-Subscription mit 403 'subscription missing proper authorization' fehl, setzt der Bot sich ueber den Streamer-Token selbs…
- `[chat:connection-subscriptions-3]` **MEDI/C** — Bot-Ban-Blacklisting beim Chat-Join nicht portiert
    - *Wirkung:* In Python: erkennt der Bot beim Mod-Setzen/Subscriben eine 'user is banned'-Antwort, traegt er den Kanal auf die Raid-Blacklist und stoesst Opt-out/O…
- `[chat:connection-subscriptions-1]` **MEDI/D** — Kanal-Selektion fuer Chat-Subscriptions weicht ab: raid_enabled-only-Kanaele fallen weg, needs_reauth-Filter neu
    - *Wirkung:* Ein Streamer, der NICHT is_partner_active ist, aber raid_enabled=TRUE und den channel:bot-Scope erteilt hat, bekommt in Python Chat-Subscriptions (Da…
- `[chat-event-pipeline-01]` **MEDI/D** — event_chat_notification (Subscription-Events sub/resub/sub_gift/community_sub_gift) wird in Rust nicht aus channel.chat.notification gespeist
    - *Wirkung:* Python leitet aus channel.chat.notification (notice_type sub/resub/sub_gift/community_sub_gift) Subscription-Events ab und ruft raid_bot.on_chat_subs…
- `[chat-event-pipeline-02]` **MEDI/D** — Raid-/Unraid-/Self-Unraid-Korrelation aus channel.chat.notification: chat.notification-Quelle im nativen Dispatch nicht verdrahtet
    - *Wirkung:* Python nutzt channel.chat.notification (notice_type=raid/unraid) als Sekundaer-/Bestaetigungssignal fuer die Raid-Arrival-Korrelation und fuer Self-U…
- `[chat:connection-subscriptions-4]` **LOW/C** — chat_join-Observability-Flow (Events + Counter + Diagnostik) nicht portiert
    - *Wirkung:* Python schreibt pro Join-Versuch ein observability_event mit terminaler Entscheidung (joined/missing_bot_scope/stale_removed_channel/mod_retry_cooldo…
- `[chat-event-pipeline-06]` **LOW/C** — Streamer-Invite-Erzeugung (_ensure_partner_invites, _create_streamer_invite via Broker) hat kein verdrahtetes natives Pendant
    - *Wirkung:* Python erzeugt beim Bot-Start fuer jeden aktiven Partner ohne Invite einen Discord-Invite ueber den Broker (8770) und persistiert ihn in twitch_strea…

**Empfehlung (Default):** **Portieren (Kern), Diagnostik droppen.** 403-Mod-Retry + Bot-Ban-Blacklisting + Kanal-Selektions-Parität portieren (sonst Joins scheitern still, Datensammlung lückenhaft). Subscription-Ableitung aus chat.notification gehört mit C07 zusammen. Reine Observability-/Invite-Glue droppen.

---

## Cluster 9 — Chat-Commands / Promos / Lurker-Tax / Scam-Spam-Review (Verhaltens-Divergenzen)

- **Theme-Klasse:** C-lost-subsystem / D-divergence
- **Findings:** 10  ·  **Schwere:** 0 crit / 0 high / 2 med / 8 low
- **Betroffene Subsysteme:** chat
- **Kern-Entscheidung:** Welche Chat-Command-Divergenzen (!raid_enable-OAuth-Link, !clip-Fallback, !lurkersteuer_off, Lurker-Tax-Kanalmenge, Plan-Ablauf, Timeout-Reason) werden gefixt und welche akzeptiert?

**Wichtigste Einzel-Entscheidungen:**

- `[chat-commands-tokens-04]` **MEDI/D** — !raid_enable ohne Auth-Row: kein OAuth-Link im Chat
    - *Wirkung:* Hat ein Partner noch keine twitch_raid_auth-Zeile, generiert Python einen konkreten OAuth-Link (auth_manager.generate_auth_url) und postet ihn samt F…
- `[promos-engine-4]` **MEDI/D** — Lurker-Tax laeuft in Rust nur fuer Deadlock-live Kanaele statt fuer jeden live Kanal mit aktiver Session
    - *Wirkung:* Python holt fuer die Lurker-Tax eine EIGENE Kanalliste ohne Game-Filter (jeder live Kanal mit aktiver Session). Rust ruft die Lurker-Tax-Erinnerung i…
- `[promos-engine-6]` **LOW/D** — Plan-Ablauf (manual_plan_expires_at)/Snapshot-Resolution fehlt — abgelaufene Plaene wirken in Rust weiter
    - *Wirkung:* Python loest das Plan-Entitlement ueber resolve_plan_snapshot_for_refs auf (beruecksichtigt manual_plan_expires_at, Bundles, kanonische IDs). Rust pr…
- `[chat-commands-tokens-05]` **LOW/D** — !clip: kein Bot-Token-Fallback nach fehlendem Broadcaster-Token
    - *Wirkung:* Python versucht zuerst den Broadcaster-Token, dann als Fallback den Bot-eigenen Token (_token_manager.get_valid_token) und sendet erst danach 'OAuth…
- `[chat-commands-tokens-06]` **LOW/C** — !lurkersteuer_off / !lurkersteuer_aus / !lurker_tax_off nicht portiert
    - *Wirkung:* Python bietet dem Broadcaster einen Chat-Befehl, die Lurker-Steuer dauerhaft zu deaktivieren (Schreibpfad auf streamer_plans.lurker_tax_enabled, mit…
- `[scam-pitch-spam-review-5]` **LOW/D** — Twitch-Timeout-Reason bei Eskalation weicht ab (Reason-String)
    - *Wirkung:* Beim Eskalations-Timeout übergibt Python an die Twitch-Timeout-API den Reason 'Service-Pitch / Spam Escalation'; Rust übergibt 'Account-Takeover-Verd…

**Empfehlung (Default):** **Überwiegend fixen.** !raid_enable-OAuth-Link, !clip-Bot-Token-Fallback, Lurker-Tax-Kanalmenge und abgelaufene-Plan-Auflösung sind echte Verhaltens-Regressionen → fixen. Wortlaut-/Audit-Trail-Unterschiede sind kosmetisch → niedrige Prio.

---

## Cluster 10 — Internal-API Streamer-CRUD-Lifecycle + Loopback-Guard (Schema-/Vertrags-Drift)

- **Theme-Klasse:** A-live-regression / D-divergence
- **Findings:** 16  ·  **Schwere:** 0 crit / 3 high / 4 med / 9 low
- **Betroffene Subsysteme:** internal_api
- **Kern-Entscheidung:** Bringen wir die internen Streamer-CRUD-Routen (DELETE/verify/archive/add) auf vollen Departner-/Promote-Lifecycle inkl. Discord-Rollen-Sync/Stats-Backfill, und härten wir den Loopback-Guard?

**Wichtigste Einzel-Entscheidungen:**

- `[streamers-crud-1]` **HIGH/D** — DELETE /streamers/:login departnert aktive Partner NICHT (kein Status-Wechsel, keine Discord-Rolle entfernt)
    - *Wirkung:* Python ruft departner_active_partner() (Status->archived, Identity-Upsert, Raid-Auth-Disable) und entfernt die Discord-Streamer-Rolle. Rust setzt nur…
- `[streamers-crud-2]` **HIGH/D** — POST /streamers/:login/verify (permanent/temp) promotet keine Nicht-Partner und unterlaesst DM/Rollen-Sync/Stats-Backfill
    - *Wirkung:* Python promotet bei permanent/temp via promote_streamer_to_partner AUCH einen reinen twitch_streamers-Eintrag zum Partner, backfillt Kategorie-Stats…
- `[streamers-crud-3]` **HIGH/D** — verify mode=clear/failed antwortet 503 statt vollem Departner-Lifecycle (+ Rollen-Removal / Fehler-DM)
    - *Wirkung:* Python: mode=clear departnert (departner_active_partner clear_verification=True) und entfernt die Streamer-Rolle, keine DM; mode=failed departnert, e…
- `[policy-contracts-idempotenz-1]` **MEDI/A** — Loopback-Middleware prueft Origin-Header nicht (is_loopback_origin fehlt)
    - *Wirkung:* Python verlangt fuer jeden internen Request _is_loopback_origin(Origin) AND _is_loopback_host(peer): ist ein Origin-Header gesetzt, muss er http/http…
- `[streamers-crud-4]` **MEDI/D** — POST /streamers/:login/archive verliert alle spezifischen Meldungen, History/Reactivate-Pfad und EventSub-Supervisor-Trigger
    - *Wirkung:* Python liefert kontextspezifische Meldungen ('X archiviert', 'X reaktiviert', 'X dauerhaft blockiert', 'X entsperrt', 'X ist bereits archiviert (seit…
- `[streamers-crud-5]` **MEDI/D** — POST /streamers verwirft require_link/next_link_check_at, kein Stats-Backfill, kein EventSub-Supervisor; abweichende Fehlerantworten (503/422)
    - *Wirkung:* Python _cmd_add ruft upsert_non_partner_streamer(require_discord_link=int(require_link), next_link_check_at=now+30d, is_monitored_only=0) und backfil…
- `[streamers-crud-8]` **LOW/C** — POST /streamers/:login/chat-action ist 503-Stub (Partner-Chat-Aktion nicht portiert)
    - *Wirkung:* Python sendet via Bot-Chat eine Nachricht/Action/Announcement an den Partner-Kanal und meldet '<Label> an <login> gesendet'. Rust antwortet 503 (Bot-…
- `[app-routing-2]` **LOW/D** — Loopback-Guard prüft in Rust nur Peer-IP, nicht den Origin-Header
    - *Wirkung:* Python verlangt für den Durchlass BEIDES: Origin-Header abwesend ODER loopback-http(s)-URL ohne Userinfo, UND loopback-Peer-Host. Ein loopback-Reques…

**Empfehlung (Default):** **Fixen (Migrations-Bugs).** DELETE/verify departnern nicht voll, promoten keine Nicht-Partner, lassen DM/Rollen-Sync/Stats-Backfill aus → echte Lifecycle-Regression an der Schema-Umstellung. Loopback-Origin-Guard härten. Chat-Action-503-Stub gehört an die Bot-Token-Bridge (mit C01-live-6).

---

## Cluster 11 — Storage / Crypto / Schema-Bootstrap / Sessions (Schema-Ownership + Härtung)

- **Theme-Klasse:** B-cutover-blocker / C-lost-subsystem / D / E
- **Findings:** 14  ·  **Schwere:** 0 crit / 4 high / 4 med / 6 low
- **Betroffene Subsysteme:** storage
- **Kern-Entscheidung:** Wer ist nach dem Cutover Schema-Owner (ensure_schema, Startup-Maintenance), und welche Storage-Härtung (durabler Rate-Limiter, Session-API, Crypto-Key-Mgmt, TX-Retry) wird nachgebaut?

**Wichtigste Einzel-Entscheidungen:**

- `[storage-core-pool-rows-pg-1]` **HIGH/C** — Runtime-Schema-Bootstrap (ensure_schema) nicht portiert; Python (Schema-Owner) ist aus
    - *Wirkung:* Pythons ensure_schema legt idempotent ~60 Tabellen, 2 Views (twitch_partners_all_state / twitch_streamers_partner_state), 2 Identitaets-Sync-Trigger,…
- `[storage-partner-registry-4]` **HIGH/C** — Dashboard-Admin-Streamer-Mutationen (verwaltung: verify/archive/block) hängen am toten Legacy-Proxy
    - *Wirkung:* Die Admin-Dashboard-Streamer-Verwaltung registriert in Rust nativ nur GET /twitch/api/admin/streamers (+/:login). Alle Schreibaktionen (verify, archi…
- `[storage-sessions-fernet-crypto-1]` **HIGH/B** — Durabler Login-Rate-Limiter (reserve_rate_limit_slot) nicht nach Rust portiert
    - *Wirkung:* Der atomare Sliding-Window-Limiter fuer Dashboard-Auth-Login (DashboardAuthRateLimitStore.allow_request -> reserve_rate_limit_slot, session_type rate…
- `[storage-sessions-fernet-crypto-2]` **HIGH/B** — Generische Session-Schreib-API (upsert_session) + OAuth-State (pop_session) nur in Python
    - *Wirkung:* Rust portiert upsert_session NUR inline fuer den Sliding-Refresh innerhalb von maybe_refresh_session. Die eigentliche Session-Erstellung beim OAuth-L…
- `[storage-partner-registry-7]` **MEDI/D** — OAuth-Re-Auth hebt technical_pause_reason bedingungslos auf — Hard-Pause (bot_banned) Guard aus reactivate_partner_after_valid_auth fehlt
    - *Wirkung:* Python reactivate_partner_after_valid_auth ist ein no-op für technical_pause_reason in {blocked, bot_banned} (Hard-Kills bleiben bestehen). Der nativ…
- `[storage-core-pool-rows-pg-3]` **MEDI/C** — Startup-Maintenance (Serial-Alignment, Boolean-Coercion, Live-State-Dedup, Index-Enforcement) nicht portiert
    - *Wirkung:* Python richtete bei jedem Start einmalig pro DB Serial-Sequenzen aus (twitch_stream_sessions/raid_history/clip_fetch_history/clips_social_media), coe…
- `[storage-partner-registry-6]` **MEDI/C** — reactivate_partner (Dashboard-Unarchive departnerter Partner) hat kein Rust-Pendant
    - *Wirkung:* Python reactivate_partner re-promotet einen departnered/archived Partner aus der Historie (löscht Quelle, restauriert raid_auth bei nicht-departnered…
- `[storage-core-pool-rows-pg-4]` **LOW/D** — Kein Transaktions-Retry bei Serialisierungs-/Deadlock-Fehlern (40001/40P01)
    - *Wirkung:* Python kapselt Write-Transaktionen mit bounded Retry (3 Versuche, exponentielles Backoff 0.10-0.75s) bei SQLSTATE 40001 (serialization_failure) und 4…

**Empfehlung (Default):** **Portieren (Bootstrap + Härtung).** Python war Schema-Owner und ist aus → Rust muss ensure_schema/Startup-Maintenance übernehmen oder es wird ein expliziter Migrationsbesitzer benannt. Durabler Login-Rate-Limiter + Session-Schreib-API + TX-Retry portieren. Windows-keyring-Pfade droppen (Linux-Prod).

---

## Cluster 12 — Migrations: Schema-Ownership-Cutover (Social-Media-Schema, One-Shots, exp-Tabellen)

- **Theme-Klasse:** B-cutover-blocker / C-lost-subsystem / E
- **Findings:** 8  ·  **Schwere:** 0 crit / 1 high / 2 med / 5 low
- **Betroffene Subsysteme:** migrations
- **Kern-Entscheidung:** Wird das DB-Schema (besonders Social-Media Phase 0-4) auf einer frischen DB ohne Python-Migrationen garantiert erzeugt, oder hängt es dauerhaft an bereits gelaufenen Python-One-Shots?

**Wichtigste Einzel-Entscheidungen:**

- `[social-media-phase0-4-schema-1]` **HIGH/B** — ensure_schema wird zur Laufzeit nie aufgerufen — Schema haengt allein an Python-Migrationen
    - *Wirkung:* Auf einer DB ohne die Python-Migrationen existieren die Social-Media-Tabellen nicht. Die Rust-Worker (retention/approval/reports/enrichment/upload/in…
- `[social-media-phase0-4-schema-2]` **MEDI/C** — Phase-0 oauth_state_tokens-Haertung komplett nicht portiert (consumed_at-Spalte, TIMESTAMPTZ-Coercion, expires_at NOT NULL, 2 Indizes)
    - *Wirkung:* Die Social-Media-OAuth-Persistenz (oauth.rs) schreibt/liest consumed_at und erwartet TIMESTAMPTZ-Spalten + die zwei Indizes. Ohne die Python-Phase-0-…
- `[social-media-phase0-4-schema-3]` **MEDI/E** — Phase-4 Auto-Approve-Default-Seeding nicht portiert (3 social_media_settings-Zeilen auto_approve_*=false)
    - *Wirkung:* Funktional kein Daten-Drift: get_auto_approve_settings liefert bei fehlenden Keys ebenfalls false (settings.rs:79 unwrap_or(false)). Aber der DB-Zust…
- `[migrations-infra-oneshots-1]` **LOW/C** — Kein Rust-Migrations-Pendant für CREATE twitch_viewer_presence_ticks + Index
    - *Wirkung:* In der laufenden Prod-DB existiert die Tabelle bereits (Python-Lauf), daher KEINE Live-Wirkung — Rust liest produktiv aus twitch_viewer_presence_tick…
- `[exp-migrate-1]` **LOW/C** — exp_tables_migrate.py: keine produktive Rust-Migration fuer exp_sessions/exp_snapshots/exp_game_transitions
    - *Wirkung:* Kein Live-Impact: Tabellen existieren bereits in der Prod-DB (vom einmaligen Python-Migrate). Rust-Laufzeit (exp_sessions.rs) setzt sie korrekt vorau…
- `[exp-migrate-2]` **LOW/C** — exp_backfill.py: kein Rust-Pendant des einmaligen Historie-Backfills in die exp_*-Tabellen
    - *Wirkung:* Kein Live-Impact: Reines One-Shot-Werkzeug, das bereits einmalig lief und die historischen Sessions in exp_sessions/exp_snapshots eingespielt hat (id…

**Empfehlung (Default):** **Fixen (Cutover-Blocker für frische DB).** Social-Media-ensure_schema wird zur Laufzeit nie aufgerufen → auf neuer DB starten die Worker leer. Produktiven Rust-Schema-Pfad verdrahten. Reine One-Shots (presence-ticks, hypertable, legacy-token-drop, exp-backfill) sind in Prod bereits angewandt → als dokumentierte Drops akzeptieren.

---

## Cluster 13 — Community: Admin-Add/Remove-Lifecycle + Discord-Leaderboard (!twl)

- **Theme-Klasse:** B-cutover-blocker / C / D / E
- **Findings:** 7  ·  **Schwere:** 0 crit / 1 high / 3 med / 3 low
- **Betroffene Subsysteme:** community
- **Kern-Entscheidung:** Bekommt der native Add/Remove-Streamer-Pfad den vollen Departner-/Stats-Backfill-/Rollen-Lifecycle, und wird der interaktive Discord-!twl-Leaderboard-Layer nachgebaut oder gedroppt?

**Wichtigste Einzel-Entscheidungen:**

- `[community/admin + partner_recruit-1]` **MEDI/B** — _cmd_remove: Departner-Lifecycle + Clip-Cascade-Delete fehlen im nativen remove_streamer
    - *Wirkung:* Wird ein aktiver Partner über remove entfernt, departnert Python ihn in twitch_partners (status='departnered', departnered_at, admin_archived_at=NULL…
- `[community/admin + partner_recruit-2]` **MEDI/B** — _cmd_remove: Discord-Streamer-Rollen-Entzug fehlt
    - *Wirkung:* Beim Entfernen eines archivierten Partners mit Discord-User-ID entzieht Python die Discord-Streamer-Rolle und meldet '(Streamer-Rolle entfernt)'. Der…
- `[community/leaderboard #2]` **HIGH/E** — Discord-!twl-Interaktiv-Layer (View/Options/Filter/Sort/Embed) ohne Rust-Pendant; Python-Host laut #192 abgeschaltet
    - *Wirkung:* Der komplette interaktive Leaderboard-Layer — LeaderboardOptions (Filter/Sort-Cycling, clamp, reset), TwitchLeaderboardView (9 Buttons: Sortierung/Re…
- `[community/leaderboard #1]` **MEDI/D** — Nativer /stats-Pfad lässt vier _compute_stats-Sektionen fallen (retention/chat/discovery/content_performance)
    - *Wirkung:* Die Python-Funktion _compute_stats setzt out['retention'], out['chat'], out['discovery'] und out['content_performance'] aus einem zweiten DB-Block (S…
- `[community/admin + partner_recruit-3]` **LOW/B** — _cmd_add: backfill_tracked_stats_from_category + EventSub-Supervisor-Neustart fehlen
    - *Wirkung:* Python kopiert beim Hinzufügen eines neuen Streamers historische Kategorie-Stats nach twitch_stats_tracked (Partner-Dashboard startet nicht bei 0) un…
- `[community/admin + partner_recruit-5]` **LOW/C** — admin.py Invite-/Channel-/Forcecheck-Helfer ohne 1:1-Rust-Pendant
    - *Wirkung:* Die Python-Dashboard-Glue zum Setzen des Notify-Channels, manuellen Forcecheck (_tick) und zum Schreiben/Auffrischen des Discord-Invite-Code-Caches e…

**Empfehlung (Default):** **Add/Remove fixen, !twl klären.** Departner-Lifecycle + Clip-Cascade + Rollen-Entzug + Stats-Backfill fixen (deckt sich mit C10). Der interaktive !twl-View (9 Buttons) hängt am abgeschalteten Python-Host → User vorlegen, ob das Feature zurückkommt oder gedroppt wird; /stats-Sektions-Lücken (retention/chat/discovery) fixen.

---

## Cluster 14 — voice_reaction — komplettes Subsystem (default-off, unportiert)

- **Theme-Klasse:** C-lost-subsystem
- **Findings:** 7  ·  **Schwere:** 0 crit / 0 high / 0 med / 7 low
- **Betroffene Subsysteme:** community
- **Kern-Entscheidung:** Wird das Voice-Reaction-Konversations-Subsystem (Brain/Scheduler/State/Audit, Discord-Pfad) je nach Rust portiert oder als toter Pfad endgültig gestrichen?

**Wichtigste Einzel-Entscheidungen:**

- `[voice_reaction-1]` **LOW/C** — Voice-Reaction-Scheduler (scheduler.py) komplett ohne Rust-Port
    - *Wirkung:* Der gesamte konversationelle Kern fehlt in Rust: PriorityQueue-Worker-Pool, Voice-Trigger (Capture→Whisper→Brain), Chat-Trigger, _run_brain_call (Cha…
- `[voice_reaction-2]` **LOW/C** — Voice-Reaction-State-Store (state_store.py) + DB-Tabelle ohne Rust-Port
    - *Wirkung:* Alle 7 Persistenz-Funktionen fehlen in Rust (open_conversation mit ON CONFLICT DO NOTHING + outreach.conversation_status='open', append_message mit m…
- `[voice_reaction-3]` **LOW/C** — Voice-Reaction-Audit-Log (audit_log.py) + Tabelle ohne Rust-Port
    - *Wirkung:* audit() (best-effort INSERT in twitch_partner_outreach_audit mit/ohne correlation_id::uuid, RETURNING id) und new_correlation_id() fehlen in Rust; ke…
- `[voice_reaction-4]` **LOW/C** — Voice-Reaction-Mixin (mixin.py) Bot-Hooks ohne Rust-Port
    - *Wirkung:* _ensure_voice_reaction_started (Lazy-Brain-Bau, bot_login-Fallback aus nick, Transcriber-/Live-Check-Builder), _open_conversation (von partner_recrui…

**Empfehlung (Default):** **Aus-lassen / droppen (User-Bestätigung).** Default-AUS in Python (VOICE_REACTION_ENABLED=false), nirgends live aktiv. Kein Port nötig solange das Feature aus bleiben soll. Endgültig droppen, falls keine Reaktivierung geplant ist — sonst als bewusst zurückgestellt markieren.

---

## Cluster 15 — social_media — Rest-Lücken (Approval-DM, Whisper-Transkription, Uploader-Refresh, register_clip)

- **Theme-Klasse:** C-lost-subsystem / D / E
- **Findings:** 20  ·  **Schwere:** 0 crit / 0 high / 6 med / 14 low
- **Betroffene Subsysteme:** social_media
- **Kern-Entscheidung:** Welche der verbleibenden social_media-Lücken (Discord-Approval-DM-Flow, lokale faster_whisper-Engine, YouTube-Token-Refresh, register_clip-Layout/Backfill) werden gefixt — social_media gilt sonst als fast voll portiert?

**Wichtigste Einzel-Entscheidungen:**

- `[approval-1]` **MEDI/C** — Discord-DM-Approval-Flow (Admin bekommt Clip zur Freigabe per DM) nicht portiert
    - *Wirkung:* In Python schickte der Bot dem Admin pro freigabebedürftigem Clip eine Discord-DM mit Embed + Buttons (Posten/Bearbeiten/Skip) + Plattform-Auswahl; d…
- `[transcription-1]` **MEDI/C** — Lokale faster_whisper-Engine (Python-Default fuer social_media) nicht portiert; ohne OPENAI_API_KEY transkribiert Rust gar nicht
    - *Wirkung:* In Python ist der Default-Transcriber der social_media-Enrichment-Pipeline faster_whisper (lokal, kein API-Key noetig) - ohne OPENAI_API_KEY werden C…
- `[social_media_unit47-1]` **MEDI/C** — Approval-DM-Versand (_dispatch_pending_dms) nicht portiert — Discord-Approval-Kanal fehlt
    - *Wirkung:* Der Python-Approval-Worker macht zwei Dinge pro Loop: (1) Approval-DMs an den Admin verschicken fuer Clips mit fertigem Enrichment (iter_clips_needin…
- `[uploaders-1]` **MEDI/D** — YouTube-Upload ohne automatischen Token-Refresh (google-Client vs. Roh-HTTP)
    - *Wirkung:* Python baut in authenticate() ein google.oauth2.Credentials-Objekt mit access_token, refresh_token, client_id, client_secret und token_uri. Der googl…
- `[social_media-1]` **MEDI/D** — register_clip (Fetch-Pfad) ruft apply_default_layout nicht auf
    - *Wirkung:* Python ruft in register_clip nach JEDEM Register (existierender Clip Z.67 UND neuer Clip Z.110-111) apply_default_layout(clip_db_id, streamer_login)…
- `[social-token-refresh-worker-nachtrag-1]` **MEDI/D** — Admin-Reauth-Discord-DM-Subsystem komplett fehlend im Rust-Port
    - *Wirkung:* Bei dauerhaftem (nicht-transientem) Refresh-Fehler schickt Python eine Discord-DM an den Admin ('Social Media Re-Auth erforderlich', mit 24h-Dedup vi…
- `[social_media-5]` **LOW/D** — ensure_monitored_streamer backfillt twitch_user_id (Python nicht)
    - *Wirkung:* Python register_clip stellt den Streamer in twitch_streamers per INSERT ... ON CONFLICT (twitch_login) DO UPDATE SET is_monitored_only = COALESCE(...…
- `[social_media-4]` **LOW/D** — MP4-Upload-Validierung ohne libmagic-MIME-Check
    - *Wirkung:* Python _validate_uploaded_mp4 prueft (falls libmagic verfuegbar) zuerst den MIME-Typ und lehnt alles ab, was nicht video/mp4 bzw. application/mp4 ist…

**Empfehlung (Default):** **Gezielt fixen.** social_media ist fast vollständig portiert; offene Bugs an der Cutover-Umstellung fixen: Approval-DM-Flow (sonst kein Freigabe-Kanal), faster_whisper-Default (sonst ohne OPENAI_API_KEY keine Transkription), YouTube-Token-Refresh, register_clip-Layout. MiniMax-Ledger-Seiteneffekte sind niedrige Prio.

---

## Cluster 16 — Analytics-Divergenzen: Bezahlschranken, Response-Shapes, Rundung, Viewer-Exklusion

- **Theme-Klasse:** D-divergence / A / E
- **Findings:** 32  ·  **Schwere:** 0 crit / 4 high / 7 med / 21 low
- **Betroffene Subsysteme:** analytics
- **Kern-Entscheidung:** Welche Analytics-Abweichungen sind echte Bugs (fehlende Extended-Plan-Gates, Streamer-/Bot-Selbst-Exklusion, Free-Plan-Tagesform, CSRF) vs. tolerierbare Drift (Rundung, null-vs-0, Beschreibungstexte, Roadmap/chat-deep-Ports)?

**Wichtigste Einzel-Entscheidungen:**

- `[viewers-1]` **HIGH/D** — Bezahlschranke (_require_extended_plan) fehlt auf allen 5 Viewer-Endpoints
    - *Wirkung:* Jeder eingeloggte Partner OHNE Extended-Plan/Trial bekommt viewer-directory, viewer-detail, viewer-segments, viewer-timeline und viewer-timeline/prof…
- `[viewers-2]` **MEDI/D** — Streamer-Selbst-Exklusion fehlt in viewer-directory und viewer-segments
    - *Wirkung:* Schreibt der Streamer selbst im eigenen Chat, taucht er in viewer-directory und viewer-segments als eigener Viewer auf (oft als Top-Viewer) und verfa…
- `[viewers-3]` **MEDI/D** — Dynamische Bot-Service-Account-Exklusion (bot_login/chat-bot/raid-bot) nicht portiert
    - *Wirkung:* Die eigenen Bot-Konten des Systems (Antwort-Bot, Raid-Bot) sind nur ausgeschlossen, falls ihr Login zufaellig in der statischen 10er-Liste steht. And…
- `[overview-raidmetrics-03]` **HIGH/D** — overview-Handler implementiert die kostenlose Tagesform (window=last_stream) nicht
    - *Wirkung:* Python klemmt für Free-Plan-Streamer (kein analytics.basic/extended) das Fenster auf den letzten Stream (since = MAX(started_at), Vorperiode leer) un…
- `[api_admin-4]` **MEDI/E** — CSRF-Prüfung bei allen Admin-POST-Handlern bewusst entfernt (Python prüft, Rust nicht)
    - *Wirkung:* Jeder schreibende Admin-Endpoint verifiziert in Python ein CSRF-Token (Header X-CSRF-Token bzw. payload.csrf_token) und antwortet sonst 403 invalid_c…
- `[U14-1]` **HIGH/B** — DB-backed v2-Roadmap-CRUD (GET public + POST/PATCH/DELETE admin) hat keinen nativen Rust-Port
    - *Wirkung:* Der Kanban-Roadmap-Editor im Admin-Dashboard (pages.py: loadRoadmap/addItem/handleDrop/deleteItem -> fetch /twitch/api/v2/roadmap[/{id}]) funktionier…
- `[api_chat_deep-1]` **MEDI/B** — chat-deep-minimax (MiniMax-LLM-Deep-Analyse) nicht nach Rust portiert
    - *Wirkung:* Der Endpunkt /twitch/api/v2/chat-deep-minimax (KI-Chat-Analyse pro Session via MiniMax: Kategorisierung Greeting/Question/Reaction/Hype/Game-Related/…
- `[analytics-backend-02]` **MEDI/A** — Insight-Beschreibungstexte weichen vollständig vom Python-Original ab
    - *Wirkung:* Titel der Insights stimmen, aber alle description-Texte sind in Rust neu formuliert. Beispiele: Low-Retention Python 'Deine 10-Minuten-Retention lieg…

**Empfehlung (Default):** **Bugs fixen, Drift akzeptieren.** Pflicht-Fixes: fehlende Bezahlschranke auf 5 Viewer-Endpoints, fehlende Streamer-/Bot-Selbst-Exklusion, Free-Plan-Tagesform, CSRF auf Admin-POSTs. Roadmap-CRUD + chat-deep-MiniMax nachbauen oder droppen klären. Rundung/null-vs-0/Beschreibungstexte sind tolerierbare Drift → aus-lassen.

---

## Cluster 17 — core_runtime: Hardening, Lifecycle, Hot-Reload, Datei-Logging, Secret-Pfade

- **Theme-Klasse:** C-lost-subsystem / D / E
- **Findings:** 26  ·  **Schwere:** 0 crit / 0 high / 3 med / 23 low
- **Betroffene Subsysteme:** core_runtime
- **Kern-Entscheidung:** Welche Runtime-Bausteine (PID/Single-Instance-Lock, Rollen/Port-Guard, Hot-Reload, Datei-Logging, graceful Shutdown, keyring-Secrets, resilienter DNS) gehören in den Rust-Betrieb und welche sind Windows-/Legacy-Altlast?

**Wichtigste Einzel-Entscheidungen:**

- `[core_runtime-02]` **MEDI/D** — Kein Datei-Logging in Rust (RotatingFile twitch_bot.log / twitch_dashboard.log fehlt)
    - *Wirkung:* Python schreibt Bot-Logs in eine rotierende Datei (5MB, 5 Backups) und trennt je Runtime-Rolle (Bot vs Dashboard) in zwei Dateien. Rust loggt ausschl…
- `[core_runtime-03]` **MEDI/E** — Hot-Reload-Subsystem (reload_manager/mixin + /twitch-reload, /twitch-status) ohne Rust-Pendant
    - *Wirkung:* In Python sind zwei Admin-Slash-Commands aktiv (/twitch-reload zum Heiss-Neuladen einzelner Subsysteme ohne Bot-Neustart, /twitch-status fuer Loop-St…
- `[base-composition-5]` **MEDI/C** — Scout: Session-Priming + Chat-Bot-Heal/Join/Part der monitored-only Kanaele fehlt im Rust-Scout
    - *Wirkung:* Python-Scout primt fuer neu entdeckte monitored-only Kanaele sofort eine Stream-Session (_prime_monitored_only_sessions), bevor der Chat sie joint, u…
- `[core_runtime-1]` **LOW/C** — Startup-Backfill fehlender twitch_user_id (_sync_missing_user_ids) ohne Rust-Pendant
    - *Wirkung:* Python fuellt beim Hochfahren in twitch_streamers fehlende twitch_user_id nach: Phase 1 aus twitch_raid_auth per UPDATE (offline), Phase 2 verbleiben…
- `[base-composition-8]` **LOW/E** — Reload-Manager (Hot-Reload der Subsysteme) hat keinen Rust-Port
    - *Wirkung:* Python registriert einen TwitchReloadManager mit 7 Subsystemen (analytics/community/social/monitoring/chat/dashboard/raid) inkl. hot_reloadable-Flags…
- `[core_runtime-06]` **LOW/C** — Keyring-Secret-Pfad (secret_store) ohne Rust-Pendant
    - *Wirkung:* Python kann Secrets aus dem Windows Credential Manager lesen (keyring_enabled() = True nur unter Windows bzw. via DEADLOCK_ENABLE_KEYRING). Auf dem L…
- `[core_runtime-3]` **LOW/D** — Kein graceful Runtime-Stop/Cleanup-Pendant (stop_runtime/_cleanup_runtime_components)
    - *Wirkung:* Python hat eine detaillierte Shutdown-Sequenz (HighlightClipper stop, managed bg-tasks cancel, Loops cancel, Social-Worker stop, IRC-Lurker stop, Cha…
- `[base-composition-2]` **LOW/C** — Discord-Invite-Code-Sammlung/-Persistenz (_refresh_guild_invites/_load_invite_codes_from_db) nicht portiert
    - *Wirkung:* Python sammelt beim Start alle Guild-Invite-Codes (mit 429-Retry/Backoff), cached sie im RAM (_invite_codes) und persistiert sie in discord_invite_co…

**Empfehlung (Default):** **Selektiv portieren, Legacy droppen.** Datei-Logging (rotierend) + graceful Shutdown + Single-Instance-Lock haben Betriebswert → portieren. Hot-Reload-Manager, keyring-Secret-Pfade (Windows), All-Guilds-Rollen-Fallback und tote partner_utils droppen. Scout-Session-Priming prüfen (mögliche Datensammlungs-Lücke).

---

## Cluster 18 — monitoring-Rest: Poll-Loop, Circuit-Breaker, Embeds, Replay-Schutz, Reconcile-Set

- **Theme-Klasse:** D-divergence / E
- **Findings:** 11  ·  **Schwere:** 0 crit / 0 high / 3 med / 8 low
- **Betroffene Subsysteme:** monitoring
- **Kern-Entscheidung:** Welche Monitoring-Divergenzen sind sicherheits-/datenrelevant (Auth-Circuit-Breaker, 600s-Replay-Fenster, Capacity-Snapshot, Reconcile-Partner-Set) vs. kosmetisch (Embed-Umlaut, Footer)?

**Wichtigste Einzel-Entscheidungen:**

- `[65.2]` **MEDI/D** — Timestamp-Replay-Fenster (_is_message_too_old, 600s → 403) fehlt im nativen Receiver
    - *Wirkung:* Python lehnt jede Nachricht ab (403), deren Twitch-Eventsub-Message-Timestamp mehr als 600s in Vergangenheit ODER Zukunft liegt bzw. nicht parsebar i…
- `[monitoring-poll-1]` **MEDI/D** — EventSub-Capacity-Snapshot wird im Rust-Poll-Tick nicht geschrieben (keine periodische Zeitreihe, kein Retention-Cleanup)
    - *Wirkung:* Die Admin-Dashboard-Historie der EventSub-Kapazitaet (twitch_eventsub_capacity_snapshot) wird im Rust-Betrieb nur sporadisch befuellt (nur wenn ein P…
- `[monitoring-poll-2]` **LOW/D** — is_auth_blocked()-Circuit-Breaker fehlt im Rust-Tick (kein Backoff bei Twitch-Auth-Fehlern)
    - *Wirkung:* Wenn Twitch-Auth bricht (z. B. ungueltige App-Credentials), stoppt der Python-Bot fuer die Cooldown-Dauer alle Helix-Calls. Der Rust-Tick hat keinen…
- `[partner_ops-1]` **MEDI/E** — 300s-Voll-Reconcile laedt Partner-Superset (status='active') statt Python is_partner_active=1
    - *Wirkung:* Der periodische Voll-Refresh berechnet alle 300s Raid-Scores fuer einen groesseren Partner-Set als Python: status='active' umfasst auch Partner, die…
- `[eventsub-mixin-3]` **LOW/D** — Offline-Seiteneffekte (Engagement-Off, Global-Ban-Sweep) laufen erst nach Throttle/State statt zuerst
    - *Wirkung:* Bei einem als Duplikat gedrosselten stream.offline (innerhalb 120s) laufen in Rust Engagement-Auto-Off und Global-Ban-Sweep-Scheduling NICHT, in Pyth…
- `[eventsub-mixin-4]` **LOW/D** — Neue Partner bekommen EventSub-Core/Raid-Subs erst beim 6h-Reconcile statt prompt
    - *Wirkung:* Ein neu onboardeter Partner bekommt channel.update/channel.raid-Subs erst beim naechsten 6h-Tick. Go-Live/Offline-Erkennung ist abgedeckt, da der 15s…
- `[partner_ops-2]` **LOW/D** — Debounce/Dedupe-Pending-Set aus partner_ops.py nicht portiert
    - *Wirkung:* Python deduppt In-Flight-Refreshes ueber ein gemeinsames pending-Set (key = user_id bzw. 'all:{trigger}'): ein bereits laufender Refresh fuer denselb…
- `[embeds_mixin-4]` **LOW/D** — Bei use_streamer_ping_role=false legt Python die Live-Ping-Rolle dennoch an, Rust nicht
    - *Wirkung:* Partner mit live_ping_enabled=1 aber use_streamer_ping_role=false: Python legt die Discord-Rolle still an und persistiert die role_id (kein Ping), Ru…

**Empfehlung (Default):** **Sicherheits-/Daten-Fixes, Kosmetik aus-lassen.** is_auth_blocked-Circuit-Breaker + 600s-Replay-Fenster + EventSub-Capacity-Snapshot-Zeitreihe fixen. Reconcile-Partner-Set-Mismatch (status='active' vs is_partner_active) angleichen. Embed-Umlaut/Footer/Thumbnail-Fallback sind niedrige Prio.

---

## Cluster 19 — engagement-KI — Divergenzen (Ledger, Loop-Start, Transcriber, DDL-Typ, Tie-Break)

- **Theme-Klasse:** D-divergence / E
- **Findings:** 9  ·  **Schwere:** 0 crit / 0 high / 0 med / 9 low
- **Betroffene Subsysteme:** engagement
- **Kern-Entscheidung:** Sind die engagement-KI-Abweichungen tolerierbar, oder gibt es Schema-/Verhaltens-Bugs (oauth_state_tokens-DDL-Typ-Drift, eager-vs-lazy-Loop-Start, Transcriber-Wahl), die gefixt werden müssen?

**Wichtigste Einzel-Entscheidungen:**

- `[engagement-senderauth-03]` **LOW/E** — oauth_state_tokens.expires_at: Python-CREATE-DDL ist TEXT, Rust bindet/liest DateTime<Utc>
    - *Wirkung:* Die Python-DDL deklariert expires_at als TEXT und schreibt ISO-Strings. Rust bindet beim Schreiben ein DateTime<Utc> (TIMESTAMPTZ-OID) und dekodiert…
- `[engagement-bg-78-1]` **LOW/E** — Background-Loops starten in Rust eager beim Prozess-Start, in Python lazy beim ersten Pipeline-Aufruf
    - *Wirkung:* Python startet die acht Background-Loops erst, wenn die Engagement-Pipeline mindestens eine Partner-Message verarbeitet hat (handle() ruft ensure_sta…
- `[engagement-bg-78-2]` **LOW/D** — Stream-Transkript-Loop unterstuetzt in Rust nur OpenAI, Python waehlbare Engine via ENGAGEMENT_TRANSCRIBER
    - *Wirkung:* Python kann die Stream-Transkription per ENGAGEMENT_TRANSCRIBER auf faster_whisper (lokal) oder none umstellen; Default ist openai_api. Rust kennt nu…
- `[engagement-pipeline-kern-2]` **LOW/D** — Per-Decision-Operator-Log (log.info) im Rust-handle nicht portiert
    - *Wirkung:* Python emittiert fuer jede Nicht-DISABLED-Entscheidung eine INFO-Log-Zeile (decision/channel/user/tokens/latency/text-Auszug) zusaetzlich zum DB-Inse…
- `[engagement-minimax-persona-style-01]` **LOW/D** — MiniMax-Usage-Ledger-Seiteneffekt in generate() im Rust-Port weggefallen
    - *Wirkung:* Python schreibt nach jedem generate()-Call den Token-Verbrauch (prompt/completion) best-effort in das gemeinsame MiniMax-Ledger (~/Documents/.claude/…
- `[engagement-senderauth-01]` **LOW/D** — Admin via Twitch-Login (earlysalty) verliert actor_login + actor_id im Engagement-Dashboard
    - *Wirkung:* Python extrahiert actor_id/actor_login IMMER aus der Session — auch bei auth_level='admin'. Ein per Twitch-OAuth eingeloggter Admin (Login 'earlysalt…

**Empfehlung (Default):** **Überwiegend aus-lassen, DDL-Typ-Drift fixen.** oauth_state_tokens.expires_at TEXT-vs-TIMESTAMPTZ ist echte Schema-Drift → fixen (gehört zu C12). MiniMax-Ledger-Seiteneffekte, Per-Decision-Log, eager-Loop-Start, Tie-Break-Reihenfolge sind tolerierbar → aus-lassen.

---

## Cluster 20 — Kleine Subsysteme & Drift: Live-Announce-Template, Entitlements, Title-Generator, Highlight-Clipper, Coaching-Audit

- **Theme-Klasse:** C-lost-subsystem / D / E
- **Findings:** 12  ·  **Schwere:** 0 crit / 0 high / 2 med / 10 low
- **Betroffene Subsysteme:** entitlements, highlight_clipper, live_announce, stream_coaching_audit, title_generator
- **Kern-Entscheidung:** Welche Rest-Lücken in kleinen Subsystemen sind funktionsrelevant (Live-Announce-Validierung, Title-Insights-Lesepfad, Highlight-Clipper-Selbstheilung) vs. bewusste Drift/Tote-Pfade (Coaching-Audit-CLI, MiniMax-Ledger, keyring)?

**Wichtigste Einzel-Entscheidungen:**

- `[live_announce-template-3]` **MEDI/C** — Validierungs-/Preview-Helfer + UI->Template-Schema-Mapping ohne Rust-Pendant
    - *Wirkung:* validate_live_announcement_config (Discord-Limit-Pruefung: Titel 256, Beschreibung 4096, Felder 25/256/1024, http/https-URL-Validierung von button/th…
- `[title_generator — DB-Zugriff (title_db)-1]` **MEDI/B** — get_latest_insights nicht nach Rust portiert (Dashboard-Insight-Anzeige nur via Python-Proxy)
    - *Wirkung:* Die Lese-Funktion fuer den zuletzt gespeicherten Wochen-Insight (SELECT strengths, weaknesses, patterns, recommendations, generated_at FROM title_gen…
- `[highlight_clipper-1]` **LOW/E** — Worker-Start in Rust an Helix gebunden, in Python bedingungslos + selbstheilend
    - *Wirkung:* Ist beim Bot-Start kein HelixClient verfuegbar, startet der Highlight-Clipper-Loop in Rust gar nicht und bleibt dauerhaft aus (nur einmal warn). Pyth…
- `[live_announce-template-1]` **LOW/D** — Button: force_stream_url + url_template aus Python-Modell weggefallen
    - *Wirkung:* Python AnnouncementButton hat label_template, url_template UND force_stream_url. render_announcement_payload setzt button.url = url-Kontext wenn forc…
- `[entitlements-1]` **LOW/D** — normalize_plan_id akzeptiert Legacy-Aliase/Case in DB-Resolution, wo Python strikt-kanonisch prüft
    - *Wirkung:* Pythons repository.py nutzt die strikte normalize_plan_id: manual_override_from_row verwirft jede manual_plan_id, die nicht buchstabengetreu in KNOWN…
- `[steam_lookup-1]` **LOW/E** — Rust-Lookup ist live, Python-Lookup ist nachweislich tot (bewusste Schema-/Pfad-Korrektur)
    - *Wirkung:* Strikte 1:1-Treue verletzt: In Python liefert die Rang-/Live-Anreicherung des !title-Befehls IMMER None — der Default-DB-Pfad (~/Documents/Deadlock/s…
- `[stream_coaching_audit-1]` **LOW/C** — stream_coaching_audit komplett ohne Rust-Port (Admin-CLI, nie im Bot-Runtime)
    - *Wirkung:* Gesamtes Subsystem (regelbasierte Slur-/Threat-Erkennung via Regex _N_WORD_RE/_HOMOPHOBIC_SLUR_RE/_THREAT_RE, ffmpeg-Audio-Split, faster-whisper-Chun…
- `[title_ai-2]` **LOW/E** — Key-Resolution: Rust kennt nur Env-Vars, Python zusaetzlich keyring-Pfad
    - *Wirkung:* Python _load_secret prueft zuerst keyring (wenn keyring_enabled()), dann Env-Var MINIMAX_TOKEN_PLAN_KEY→MINIMAX_API_KEY→MINMAX. Rust resolve_minimax_…

**Empfehlung (Default):** **Gezielt fixen, Tote-Pfade droppen.** Live-Announce-Validierung + Title-Insights-Lesepfad + Highlight-Clipper-Helix-Bindung (sonst Loop bleibt dauerhaft aus) fixen. Entitlements-Shape-Drift tolerierbar. stream_coaching_audit (Admin-CLI, nie im Runtime) + keyring-Pfade droppen.

---

## Abdeckung

Summe aller Cluster-Findings = **309** (= 309 needsDecision-Befunde). Keine Orphans, keine Doppelzuordnung.

| Cluster | Findings | crit | high | med | low |
|---|---:|---:|---:|---:|---:|
| 1. Dashboard Admin/Partner-Verwaltung — LIVE tot (H | 27 | 3 | 12 | 10 | 2 |
| 2. Affiliate-/Billing-/Stripe-Programm — Welle-C-Dr | 14 | 2 | 8 | 4 | 0 |
| 3. Dashboard-Auth / Session / Partner-Login / Finge | 18 | 0 | 4 | 5 | 9 |
| 4. Token-Lifecycle: Bot-Ban-Recovery, Grace-Periods | 9 | 0 | 5 | 2 | 2 |
| 5. EventSub-/Telemetrie-Subscriptions + WS-Transpor | 20 | 0 | 2 | 7 | 11 |
| 6. Analytics-Datensammlung verloren: Chatters-Lurke | 2 | 0 | 1 | 1 | 0 |
| 7. Raid-Subsystem: verlorene Tracking-/Arrival-/Rec | 33 | 0 | 4 | 10 | 19 |
| 8. Chat-Connection: Mod-Recovery, Ban-Blacklist, Ka | 14 | 0 | 1 | 4 | 9 |
| 9. Chat-Commands / Promos / Lurker-Tax / Scam-Spam- | 10 | 0 | 0 | 2 | 8 |
| 10. Internal-API Streamer-CRUD-Lifecycle + Loopback- | 16 | 0 | 3 | 4 | 9 |
| 11. Storage / Crypto / Schema-Bootstrap / Sessions ( | 14 | 0 | 4 | 4 | 6 |
| 12. Migrations: Schema-Ownership-Cutover (Social-Med | 8 | 0 | 1 | 2 | 5 |
| 13. Community: Admin-Add/Remove-Lifecycle + Discord- | 7 | 0 | 1 | 3 | 3 |
| 14. voice_reaction — komplettes Subsystem (default-o | 7 | 0 | 0 | 0 | 7 |
| 15. social_media — Rest-Lücken (Approval-DM, Whisper | 20 | 0 | 0 | 6 | 14 |
| 16. Analytics-Divergenzen: Bezahlschranken, Response | 32 | 0 | 4 | 7 | 21 |
| 17. core_runtime: Hardening, Lifecycle, Hot-Reload,  | 26 | 0 | 0 | 3 | 23 |
| 18. monitoring-Rest: Poll-Loop, Circuit-Breaker, Emb | 11 | 0 | 0 | 3 | 8 |
| 19. engagement-KI — Divergenzen (Ledger, Loop-Start, | 9 | 0 | 0 | 0 | 9 |
| 20. Kleine Subsysteme & Drift: Live-Announce-Templat | 12 | 0 | 0 | 2 | 10 |
| **Summe** | **309** | 5 | 50 | 79 | 175 |
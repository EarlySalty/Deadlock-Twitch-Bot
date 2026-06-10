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

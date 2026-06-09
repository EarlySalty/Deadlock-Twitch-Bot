# 01 — Architektur (Cargo-Workspace)

Der gesamte Rust-Code lebt unter `rust/` als ein Cargo-Workspace mit drei Schichten:
**Foundation-Crates** (querschnittlich, kein Domänenwissen), **Feature-Crates** (je ein
Subsystem) und **Binaries** (setzen Crates zu laufenden Prozessen zusammen). Zusammengesetzt
wird über ein geteiltes `AppState` + Traits — keine Mixins.

```
rust/
  Cargo.toml            # [workspace] members
  crates/
    tb-error/  tb-domain/  tb-config/  tb-observability/  tb-crypto/
    tb-db/  tb-transport-twitch/  tb-transport-discord/  tb-llm/  tb-http-core/
    tb-chat/  tb-monitoring/  tb-raid/  tb-analytics/  tb-billing/
    tb-social-media/  tb-community/  tb-dashboard-api/
  bin/
    tb-bot/             # laufender Bot-Prozess (+ interne API 8776)
    tb-dashboard/       # Web-Host 8765
  docs/                 # diese Doku
  migrations/           # einzige DDL-SSOT (refinery)
```

## Foundation-Crates

| Crate | Verantwortung | hängt an | Libs |
|---|---|---|---|
| `tb-error` | zentrale `Error`/`Result`, `thiserror`-Enums je Domäne, HTTP-Mapping | — | thiserror |
| `tb-domain` | reine Domänen-Typen (StreamerLogin, PartnerStatus, Score, PlanTier, RaidPlan) — **kein I/O, keine sqlx-Kopplung** (DB-Row-Structs liegen in `tb-db`) | tb-error | serde, time |
| `tb-config` | typisierte Settings aus Env (Closure-injizierbarer Loader, kein globaler Mutable-State) | tb-error | serde (kein figment) |
| `tb-observability` | tracing-Setup, Observability-Event-Writer (mpsc→DB-Task), Metrics | tb-config, tb-db | tracing |
| `tb-crypto` | AES-256-GCM-Feldverschlüsselung (raid_auth, social-media), Session-Krypto, keyring | tb-error | aes-gcm, keyring, hmac, sha2 |
| `tb-db` | sqlx-Pool, **eine** Migrations-SSOT (sqlx-native), Row-Structs/-Mapping, Tx-Helper, Idempotency-Store | tb-domain, tb-config, tb-error | sqlx (postgres, tokio, migrate) |
| `tb-transport-twitch` | Helix-Wrapper (shared `Arc<Client>`), App-Token-Manager (Auto-Refresh); OAuth2-PKCE → Raid-Phase | tb-config, tb-crypto, tb-error | reqwest (rustls) — **0c hand-rolled**; `twitch_api`/`oauth2` erst bei vielen Helix-Endpoints (YAGNI) |
| `tb-transport-discord` | `DiscordBackend`-Trait + `BrokerRelay`-Impl (HTTP an 8770) + `Noop`, Embed-Builder | tb-config, tb-error | reqwest |
| `tb-llm` | LLM-Dispatcher-Trait (Anthropic/MiniMax/Ollama) + think-Strip + Rate-Limit | tb-config, tb-error | reqwest, serde_json |
| `tb-http-core` | gemeinsame axum-Bausteine: Auth-Layer, CSRF, Loopback-Middleware, Session-Cookies, Error-Response | tb-error, tb-crypto | axum, tower, tower-http |

## Feature-Crates

| Crate | Verantwortung | hängt an | Libs |
|---|---|---|---|
| `tb-chat` | IRC-Connect, Moderation (Spam-Score als pure logic), Promo, Commands, Lurker-Tracking | transport-twitch, db, llm, transport-discord | twitch_irc, regex |
| `tb-monitoring` | **4a–4d umgesetzt:** Guard-Store + Processing-Inbox (4a); Write-Core: Live-State/Sessions/Stats/exp (4b); Poll-Engine mit `StreamSource`/`AnnouncementSink`/`PollHooks`-Ports (4c); EventSub-Ingress: Dispatcher (Bridge-Vertrag, Message-Dedup 600 s), Inbox-Handler (online/offline/update mit Business-Effect-Guards), Telemetrie-Writes, Subscription-Manager (Core-Subs App-Token, Cleanup, Capacity-Snapshot) + `EventSubHooks`-Port (4d). Folgt: Announcements (4e). Webhook-only, kein WS-Pool — eigene `tb-eventsub`-Crate entfällt (ADR 0004) | db (sqlx direkt) — Helix/Broker-Adapter im Composition-Root `tb-bot` (`wiring.rs`) | sqlx, tokio, chrono, uuid, serde |
| `tb-raid` | `RaidAuth` (DB-only State), Scoring, CandidateSelection, Executor, Recruitment; `raid::util` | transport-twitch, db, crypto, transport-discord | rand |
| `tb-analytics` | Datenerfassung (Helix→DB), SQL-Aggregation, CoachingEngine, Home-Service | transport-twitch, db, llm | regex |
| `tb-billing` | Stripe (Subscription/Checkout/Invoice/Webhook), Connect+Payout, Gutschrift-PDF | db, crypto, http-core | async-stripe, printpdf* |
| `tb-social-media` | Clip-Fetch/Queue/Enrichment/Approval/Upload (YT/TikTok/IG), `VideoProcessor` (ffmpeg) | transport-twitch, db, crypto, llm | reqwest, tokio::process |
| `tb-community` | Leaderboard (ein parametrisierter Query), Slur-Audit | db, transport-discord, llm | regex |
| `tb-dashboard-api` | axum-Router `/twitch/api/v2/*` + `/admin/*` (separate Router-Module, shared AppState) | analytics, billing, raid, http-core | minijinja (Legacy-HTML) |

\* PDF-Lib noch offen, siehe [`06-open-questions.md`](06-open-questions.md).

## Binaries

| Binary | Setzt zusammen | Ersetzt |
|---|---|---|
| `tb-bot` | chat + monitoring + raid + analytics-Erfassung + community + transport-discord(Relay) + interne-API-Router (8776) | discord.py-Cog + headless Runtime |
| `tb-dashboard` | dashboard-api + billing + social-media-UI + http-core (8765) | `dashboard` + `dashboard_service` |

## Abhängigkeitsgraph (Schichten)

```
Foundation:   tb-error → tb-domain → tb-config → tb-crypto → tb-db
              tb-transport-twitch · tb-transport-discord · tb-eventsub · tb-llm · tb-http-core
                 ▲ alles darüber hängt hier dran

Feature:      tb-chat · tb-monitoring · tb-raid · tb-analytics
              tb-billing · tb-social-media · tb-community
                 ▲ hängen an Foundation, untereinander minimal gekoppelt

Konsumenten:  tb-dashboard-api  (hängt an analytics, billing, raid)

Binaries:     tb-bot  ·  tb-dashboard
```

Fundament = `tb-db` + `tb-transport-twitch` + `tb-domain`/`tb-config`. Echte Blätter (späte
Cutover-Schritte) = `tb-dashboard-api` (read-only Teile), `tb-community`, `tb-social-media`-UI.

## Schnitt-Begründung

1. **`transport-twitch` / `transport-discord` / `eventsub` als eigene Crates**, weil mehrere
   Feature-Crates dieselbe Twitch-/Discord-Anbindung brauchen — Single-Client-Pattern statt
   eines HTTP-Calls pro Aufruf (wie heute via aiohttp).
2. **`tb-db` ist die einzige Migrations-SSOT.** Die heute über `engagement/`, `chat/`,
   `analytics/`, `social_media/`, `migrations/` verstreuten DDLs ziehen hierher.
3. **`tb-crypto` bündelt die heute dreifach kopierte AES-GCM-Logik** (raid, social-media, oauth).
4. **`tb-http-core` zentralisiert Auth/CSRF/Session**, statt 10 Mixins mit impliziter MRO.
5. **Discord ist ein Relay-Crate, kein Gateway** — siehe ADR 0001.
6. **`dashboard_service` + `dashboard` verschmelzen** zu `tb-dashboard`: Die HTTP-Delegation an
   8776 wird ein In-Process-Aufruf, sobald `tb-bot` Rust ist. (Während der Migration bleibt der
   HTTP-Hop, weil Python-`tb-bot`-Pendant und Rust-Dashboard noch getrennt laufen können.)

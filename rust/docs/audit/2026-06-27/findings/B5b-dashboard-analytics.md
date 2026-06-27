# B5b Dashboard Analytics/Public/Raid/Market/Title Parity Audit

Scope: Frische Read-only-Verifikation der nutzer-/partnersichtbaren Dashboard-Analytics-, Public-, Raid-, Market- und Title-HTTP-Flaeche. Python-Referenz: `bot/analytics/api_*.py`, `bot/dashboard/routes_market.py`, `bot/dashboard/routes_title.py`, `bot/dashboard/raids/*`. Rust-Ziel: `rust/crates/tb-dashboard-api/src/handlers/{overview,viewers,audience,performance,raid_analytics,raid_network_analytics,raid_pages,market,title,roadmap,demo,leaderboard,internal_home,system/*}` plus Router-Wiring in `lib.rs`. Vorab gelesen: `rust/docs/audit/2026-06-27/00-baseline.md` Abschnitt 2; `findings/B4-analytics.md` war nicht vorhanden. Methode: Refute-by-default, statische Route-/Shape-/Auth-Pruefung per Source-Inspection; kein Git, keine Secrets, keine Runtime-Tests.

## Endpoint-Map

| Flaeche | Python-Referenz | Rust-Referenz | Status |
|---|---|---|---|
| Core v2 Analytics GET/POST (`overview`, Stats, Audience, Viewer, Chat, Exp, AI, Stream-Report, Billing/Affiliate) | `bot/analytics/api_overview.py:52-129` | `rust/crates/tb-dashboard-api/src/lib.rs:120-548` | Route-Coverage weitgehend parity; Shape-/Fehler-/Auth-Drifts siehe B5b-07, B5b-08 und B5b-11. |
| Public API (`recent-bans`, `recent-raids`, `network`) | `bot/analytics/api_public.py:30-40` | `rust/crates/tb-dashboard-api/src/lib.rs:77-113` | Public/no-auth/CORS parity; Null-/500-Body-Drift siehe B5b-06. |
| Roadmap CRUD | `bot/analytics/api_overview.py:122-126` | `rust/crates/tb-dashboard-api/src/lib.rs:1053-1066` | parity fuer URL/Methode; GET und Admin-CRUD nativ, mit Security-Hardening laut Baseline. |
| Market (`/twitch/market`, `/twitch/api/market_data`, `/twitch/api/v2/market-share`) | `bot/dashboard/routes_market.py:17-23` | `rust/crates/tb-dashboard-api/src/lib.rs:1080-1093` | Success-Shape parity fuer `market_data`; clean-SQL/Native market-share ist deliberate Verbesserung; Fehler-Shape siehe B5b-09. |
| Title (`suggest`, `insights`) | `bot/dashboard/routes_title.py:186-190` | `rust/crates/tb-dashboard-api/src/lib.rs:316-318`, `title.rs` | Route parity plus deliberate `PATCH /channel/title`; Request-/History-Shape-Drift siehe B5b-10. |
| Raid-Seiten (`auth`, `go`, `requirements`, `history`, `analytics`) | `bot/dashboard/routes_mixin.py:621-625` | `rust/crates/tb-dashboard-api/src/lib.rs:1104-1119`, `lib.rs:310-315`, `lib.rs:1182-1186` | `auth/go/requirements/history` nativ bzw. verdrahtet; alte Analytics-Datensicht nicht erreichbar, siehe B5b-05. |
| Demo Dashboard API | `bot/analytics/api_overview.py:288-328` | `rust/crates/tb-dashboard-api/src/handlers/demo.rs:1-12`, `demo.rs:527-564` | deliberate lean: nicht 1:1, bewusst nur Kern-/Kachelset. |

## Findings

| ID | Feature | Python-Ref | Rust-Ref | Klassifikation | Severity | Notiz |
|---|---|---|---|---|---|---|
| B5b-01 | Core-v2-Route-Coverage | `bot/analytics/api_overview.py:52-129` | `rust/crates/tb-dashboard-api/src/lib.rs:120-548` | parity | - | Die grosse Analytics-HTTP-Flaeche ist nativ registriert, inkl. `raid-analytics`, `viewer-*`, `chat-*`, `exp/*`, AI, Stream-Report, Billing/Affiliate. Kein harter fehlender Haupt-API-Pfad gefunden; die folgenden Findings betreffen Contract-Details. |
| B5b-02 | Public-Route-Coverage/Auth | `bot/analytics/api_public.py:30-40` | `rust/crates/tb-dashboard-api/src/lib.rs:77-113` | parity | - | Die drei Python-public Endpunkte sind in Rust oeffentlich und mit permissivem CORS verdrahtet. Keine versehentliche Auth-Verschaerfung gefunden. |
| B5b-03 | Roadmap/Market/Title-Routen | `bot/analytics/api_overview.py:122-126`, `bot/dashboard/routes_market.py:17-23`, `bot/dashboard/routes_title.py:186-190` | `rust/crates/tb-dashboard-api/src/lib.rs:1053-1066`, `lib.rs:1080-1093`, `lib.rs:316-318` | parity | - | URL/Methode sind vorhanden. Market-share ist nativ statt Python-Proxy; Title hat zusaetzlich `PATCH /twitch/api/v2/channel/title` als belegte Erweiterung. |
| B5b-04 | Demo-API nicht vollstaendig 1:1 | `bot/analytics/api_overview.py:297-323` | `rust/crates/tb-dashboard-api/src/handlers/demo.rs:1-12`, `demo.rs:527-564` | deliberate | Info | Rust dokumentiert `B6-DEMO-LEAN` und portiert bewusst nicht alle Demo-Fixtures (`viewer-overlap`, `tag-analysis`, `raid-retention`, `viewer-detail`, `audience-sharing`, `exp/*` usw.). Das ist keine stille Regression, aber keine 1:1-Demo-Paritaet. |
| B5b-05 | Legacy Raid-Analytics-Seite: Partner-Balance/Leecher/Manual-Raids nicht erreichbar | `bot/dashboard/routes_mixin.py:621-625`, `bot/dashboard/raids/raid_mixin.py:426-520` | `rust/crates/tb-dashboard-api/src/lib.rs:1182-1186`, `rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs:1-15`, `raid_network_analytics.rs:177-185` | regression | P1 | Python rendert `/twitch/raid/analytics` mit `partner_stats`, `leechers`, `manual_list`, Zeitraum und Total. Rust routet `/twitch/raid/analytics` nur auf `/analyse`; der passende JSON-Handler existiert mit `WIRING-TODO` und Payload (`partner_stats`, `leechers`, `manual_raids`), ist aber nirgends registriert (`rg raid_network_analytics_handler` findet nur Modul/Tests). Das ist mehr als SSR->SPA: die alte Datensicht ist nicht nutzbar. |
| B5b-06 | Public-API Nullability und Fehlerbody driften | `bot/analytics/api_public.py:93-99`, `api_public.py:160-166`, `api_public.py:134-135`, `api_public.py:177-178`, `api_public.py:238-239` | `rust/crates/tb-dashboard-api/src/handlers/bans.rs:12-16`, `bans.rs:59-62`, `raids.rs:13-17`, `raids.rs:46-49`, `network.rs:76-79` | bug | P2 | Python normalisiert public `moderator_login`/`reason` zu `""` und `viewers` zu `0`; Rust serialisiert dort `null`. Bei DB-Fehler liefert Python JSON `{"error":"internal_error"}`, Rust gibt fuer diese Public-Handler einen nackten 500 ohne JSON-Body zurueck. |
| B5b-07 | Nicht alle Analytics-401-Antworten behalten Python-`loginUrl`-Shape | `bot/analytics/api_v2.py:1236-1268` | `rust/crates/tb-dashboard-api/src/auth/mod.rs:135-145`, `rust/crates/tb-dashboard-api/src/handlers/performance.rs:53-59`, `audience.rs:36-41`, `chat_analytics.rs:37-42` | bug | P2 | Rust hat zwar `unauthorized_v2_response()` mit Python-kompatiblem `error` + `loginUrl`, mehrere Handler nutzen aber lokale `require_auth`-Varianten mit nur `{"error":"unauthorized"}` bzw. `{"error":"unauthorized","message":"not authenticated"}`. Clients verlieren dadurch den Login-Redirect-Key. |
| B5b-08 | Analytics-500-Body verliert `code: analytics_request_failed` | `bot/analytics/error_utils.py:6-17` | `rust/crates/tb-dashboard-api/src/handlers/chat_analytics.rs:72-80`, `exp_analytics.rs:50-58`, `performance.rs:121-128` | bug | P2 | Python zentralisiert DB-/Analytics-Fehler als JSON mit `error` und `code`. Rust ist uneinheitlich: viele Handler liefern nur `{"error":"internal"}` oder `{"error":"internal_error"}`. Beispiele decken Chat, Exp und Performance ab; `rg` zeigt denselben Drift u.a. in Audience, Viewers, Raid Analytics, Watch-Time und Category-Handlern. |
| B5b-09 | Market-Data-Fehler verliert `error_id` | `bot/dashboard/routes_market.py:303-312` | `rust/crates/tb-dashboard-api/src/handlers/market.rs:156-165` | bug | P3 | Success-Payload ist auf dieselben Keys portiert, aber Python gibt bei Aggregationsfehlern `{"error":"market_data_failed","error_id":...}` aus. Rust loggt intern und sendet nur `{"error":"market_data_failed"}`; Support-/Log-Korrelation geht fuer Admins verloren. |
| B5b-10 | Title-Suggest Request-/History-Shape driftet | `bot/dashboard/routes_title.py:105-113`, `routes_title.py:159-160`, `bot/title_generator/title_db.py:34-56` | `rust/crates/tb-dashboard-api/src/handlers/title.rs:27-34`, `title.rs:172-184`, `title.rs:210-222`, `title.rs:258-264` | bug | P3 | Python faengt invalid JSON und fehlende/blanke `keywords` selbst als JSON-400 ab; Rust nutzt `Json<TitleSuggestBody>` mit required `keywords`, sodass fehlender Key/kaputtes JSON vor dem Handler als Axum-Rejection aus der Python-Shape fallen kann. Zudem enthaelt Python `title_analysis` die History-Felder `followers_start` und `started_at`; Rust rekonstruiert nur `title`, `avg_viewers`, `peak_viewers`, `relative_perf`, `engagement_rate` und fuegt `live_context_used` als deliberate Zusatz hinzu. |
| B5b-11 | Viewer-Analytics Bot-Exklusion verliert dynamische Bot-Logins | `bot/analytics/api_viewers.py:32-52`, `api_viewers.py:55-65` | `rust/crates/tb-dashboard-api/src/handlers/viewers.rs:19-49` | bug | P3 | Python schliesst neben Known-Bots und Streamer-Self auch Laufzeit-Bot-Logins aus `_bot_token_manager`, `_twitch_chat_bot` und `_raid_bot.auth_manager.token_manager` aus. Rust nutzt nur statische `KNOWN_CHAT_BOTS` plus Streamer-Self und dokumentiert, dass dynamische Bot-Accounts in diesem Crate nicht greifbar sind. Bei abweichendem Bot-Nick koennen Viewer Directory/Segments/Detail bot-verunreinigt sein. |

## Summary

- parity: 3
- deliberate: 1
- regression: 1
- missing: 0
- bug: 6

Regression-Liste:

- B5b-05 / P1: Alte `/twitch/raid/analytics`-Datensicht fuer Partner-Balance, Leecher und Manual-Raids ist nicht erreichbar; vorhandener Rust-Handler ist unverdrahtet.

Missing-Liste:

- Keine neue harte Haupt-Endpoint-Luecke gefunden. Demo-Luecken sind als deliberate lean dokumentiert, nicht als missing gewertet.

Bug-Liste:

- B5b-06 / P2: Public-API Nullability und 500-JSON-Body driften.
- B5b-07 / P2: Mehrere Analytics-401-Antworten verlieren `loginUrl`.
- B5b-08 / P2: Mehrere Analytics-500-Antworten verlieren `code: analytics_request_failed`.
- B5b-09 / P3: `market_data_failed` verliert `error_id`.
- B5b-10 / P3: Title-Suggest invalid/missing JSON und `title_analysis`-History-Keys driften.
- B5b-11 / P3: Viewer-Bot-Exklusion verliert dynamische Bot-Logins.

Rest-Risiko: Statischer Read-only-Audit. Ich habe keine HTTP-Requests, DB-Fixtures oder Frontend-Builds ausgefuehrt; Shape-Befunde beruhen auf direkten Source-Vergleichen und `rg`-Negativnachweisen.

# 03 — HTTP-Vertrag

Drei Stabilitätsklassen:

- **A · Frontend-konsumiert** — von `dashboard_v2` + `admin_dashboard` aufgerufen. Muss
  byte-stabil bleiben (Pfad, Methode, JSON-Form), sonst brechen die Frontends.
- **B · Intern 8776** — Prozess-zu-Prozess (`dashboard_service` → bot). Loopback + Token.
- **C · Browser-UI/HTML** — OAuth-Callbacks, Webhooks, Legal. Pfade müssen halten; das Rendering
  darf beim Rewrite von HTML zu Template/JSON wandern.

## A · Frontend-Verträge

### analytics-api — `/twitch/api/v2/*` (~60 GET, einige POST)

- **Auth/Home:** `auth-status`, `internal-home` (GET/POST `…/changelog`)
- **Overview/Stats:** `overview`, `monthly-stats`, `weekly-stats`, `hourly-heatmap`, `calendar-heatmap`, `rankings`
- **Viewer:** `viewer-directory`, `viewer-detail`, `viewer-segments`, `viewer-profiles`, `viewer-overlap`, `viewer-timeline`, `{streamer}/viewer-timeline(/profile)`, `watch-time-distribution`, `loyalty-curve`, `retention-curve`, `follower-funnel`
- **Chat:** `chat-analytics`, `chat-hype-timeline`, `chat-content-analysis`, `chat-social-graph`, `chat-deep-minimax`
- **Category/Tag/Title:** `category-comparison`, `category-leaderboard`, `category-timings`, `category-activity-series`, `tag-analysis(-extended)`, `title-performance`
- **Audience/Raids:** `audience-insights`, `audience-demographics`, `audience-sharing`, `lurker-analysis`, `raid-retention`, `raid-analytics`
- **Sessions:** `session/{id}`, `session/{id}/events`, `streamers`, `coaching`, `monetization`, `ads-schedule`
- **AI/Report:** `ai/analysis`, `ai/chat` (POST), `ai/history`, `stream-report`, `stream-report/rate` (POST), `stream-report/ab-vote` (GET/POST)
- **Exp (Feature-Flag):** `exp/overview`, `exp/game-breakdown`, `exp/game-transitions`, `exp/growth-curves`
- **Public (kein Auth):** `public/recent-bans`, `public/recent-raids`, `public/network`
- **Roadmap:** `roadmap` (GET/POST/PATCH/DELETE); **Billing:** `billing/catalog`; **Affiliate:** `affiliate/portal`
- **Title (über dashboard):** `title/suggest` (POST), `title/insights`, `channel/title`

### analytics-api/admin — `/twitch/api/admin/*`

- `streamers`, `streamers/{login}`
- `system/health`, `system/oauth-scopes`, `system/eventsub`, `system/database`, `system/errors`, `system/query` *(Raw-SQL — Sicherheits-Review, siehe offene Fragen)*
- `config/overview`, `config/promo` (POST), `config/raids` (POST), `config/chat` (POST)
- `announcements` (GET/POST), `roadmap` (GET/POST), `legal/{slug}` (GET/POST), `audit-log`
- `billing/subscriptions`, `billing/affiliates`, `affiliates`, `affiliates/stats`, `affiliates/{login}(/toggle)`, `affiliates/{login}/gutschriften`, `affiliates/gutschriften`, `affiliates/gutschriften/{id}/pdf`, `affiliates/generate-gutschriften`

### engagement — `/twitch/api/v2/engagement/*`

`settings`, `toggle` (POST), `update` (POST), `log`

### social-media — `/social-media/api/admin/*` (~30) + Upload

`streamer-layout`, `clips` (+ `detail`/`layout`/`discard`/`enrichment`/`approval`/`analytics`),
`auto-approve`, `reports`, `vocab`, `templates`; Upload `POST /social-media/api/clips/upload`
*(multipart, bis 200 MB)*; OAuth `/social-media/oauth/start|callback|disconnect/{platform}`.

### Legacy-Form-Actions (admin_dashboard, `x-www-form-urlencoded` + HTML-CSRF)

`/twitch/add_streamer`, `/remove`, `/verify`, `/archive`, `/discord_link`, `/discord_flag`,
`/admin/manual-plan(/clear)`, `/admin/chat_action`, `/reload` — CSRF-Quelle ist heute das
**HTML-Scraping** von `GET /twitch/admin/announcements`.
→ **Vor der Migration des admin_dashboard** auf einen dedizierten JSON-CSRF-Endpoint umstellen
(siehe [`05-cleanup-decisions.md`](05-cleanup-decisions.md) Punkt 10).

## B · Intern 8776 — `/internal/twitch/v1/*`

Auth: Header `X-Internal-Token`, Loopback-Pflicht, `X-Idempotency-Key` für mutierende Calls.

- **Streamers:** `streamers` (GET/POST), `streamers/{login}` (DELETE), `…/verify|archive|discord-flag|discord-profile|chat-action` (POST), `streamers/link-candidates`
- **Stats/Analytics:** `stats`, `analytics/streamer/{login}`, `analytics/comparison`, `sessions/{id}`
- **Raid:** `raid/auth-url`, `raid/auth-state`, `raid/block-state`, `raid/go-url`, `raid/requirements` (POST), `raid/oauth-callback` (POST), `raid/blacklist(/add|/remove|/check)`
- **Global-Ban:** `globalban(/add|/remove|/check)`
- **Spam-Learning:** `spam-learning` (POST) — schreibt manuell bestätigte Spam-/Safe-Muster in die vorhandenen Lernlisten.
- **EventSub/Telemetry:** `eventsub/dispatch` (POST), `eventsub/processing/requeue` (POST), `debug/eventsub-processing`, `debug/observability`, `debug/chatters/{login}`, `live/active-announcements`, `live/link-click` (POST)
- **Discord-Relay:** `discord/self-explainer-log` (POST)
- `healthz`

## C · Browser-UI/HTML (dashboard, 8765)

- **OAuth-Callbacks:** `/twitch/auth/*`, `/callback/twitch`, `/callback/discord`, `/twitch/auth/discord/*`, affiliate/stripe-connect
- **Billing-UI:** `/twitch/abbo/*`
- **Legal:** `/twitch/impressum|datenschutz|agb|sicherheit`, `/twitch/legal/*` — **NATIV** seit 12.6.
  (`tb-dashboard-api/src/handlers/legal.rs`, Binary tb-dashboard auf **8769**, Caddy-Flip vollzogen;
  Live-Diff: alle Renderings byte-identisch gegen Python 8765)
- **Stripe-Webhook:** `POST /twitch/api/billing/stripe/webhook` *(raw-body, Signatur-Verify)*
- **EventSub-Webhook:** `POST /twitch/eventsub/callback` *(HMAC)*
- `healthz` / `readyz`

## Vertrags-Teststrategie

Beim Cutover read-only Endpoints (Phase 1–2) per **Shadow-Diff** absichern: Python- und
Rust-Antwort für dieselbe Anfrage vergleichen, Toleranz definieren, erst bei Deckung den Proxy
umschalten. Für mutierende interne Calls (8776) zählt der `X-Idempotency-Key`-Dedup als Sicherung.

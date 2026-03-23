# Architektur-Uebersicht

## Mixin-Komposition

`TwitchStreamCog` (bot/cog.py) setzt sich aus diesen Mixins zusammen (MRO-Reihenfolge):

```
TwitchStreamCog
  LegacyTokenAnalyticsMixin   bot/analytics/legacy_token.py
  TwitchAnalyticsMixin        bot/analytics/mixin.py
  TwitchRaidMixin             bot/raid/mixin.py
  RaidCommandsMixin           bot/raid/commands.py
  TwitchPartnerRecruitMixin   bot/community/partner_recruit.py
  TwitchDashboardMixin        bot/dashboard/mixin.py
  TwitchLeaderboardMixin      bot/community/leaderboard.py
  TwitchAdminMixin            bot/community/admin.py
  TwitchMonitoringMixin       bot/monitoring/monitoring.py
  TwitchReloadMixin           bot/reload_mixin.py
  TwitchBaseCog               bot/base.py
```

`TwitchDashboardMixin` selbst fasst weitere Dashboard-Mixins zusammen:

```
TwitchDashboardMixin (bot/dashboard/mixin.py)
  _DashboardAuthMixin         bot/dashboard/auth/auth_mixin.py
  _DashboardRaidMixin         bot/dashboard/raids/raid_mixin.py
  _DashboardAffiliateMixin    bot/dashboard/affiliate/affiliate_mixin.py
  _DashboardBillingMixin      bot/dashboard/billing/billing_mixin.py
  DashboardLiveMixin          bot/dashboard/live/live.py
  DashboardLiveAnnouncementMixin  bot/dashboard/live/live_announcement_mixin.py
  DashboardAdminAnnouncementMixin bot/dashboard/live/announcement_mode_mixin.py
  _DashboardAdminLegalMixin   bot/dashboard/admin/legal_mixin.py
  _DashboardRoutesMixin       bot/dashboard/routes_mixin.py
```

## Boot-Flow

```
Discord Bot start
  twitch_cog.py (Shim)
    bot/__init__.py  setup()
      TwitchStreamCog.__init__()
        TwitchBaseCog.__init__()   DB-Verbindung, aiohttp-App aufbauen
        TwitchMonitoringMixin      EventSub registrieren
        TwitchDashboardMixin       Routes registrieren, aiohttp starten
        TwitchRaidMixin            Raid-Manager initialisieren
```

Dashboard-Service standalone:
```
bot/dashboard_service/__main__.py
  bot/dashboard_service/app.py
    aiohttp-App mit Analytics + Dashboard-Routes
```

## Polling-Loops

| Loop | Intervall | Datei | Zweck |
|------|-----------|-------|-------|
| Haupt-Tick | 15s (`POLL_INTERVAL_SECONDS`) | bot/monitoring/monitoring.py | Live-Status, Viewer, Stats |
| Stats-Log | jeder Tick (Default 15s) | bot/monitoring/monitoring.py | Schreibt in twitch_stats_tracked |
| Kategorie-Sample | jeder Tick (Default 15s) | bot/monitoring/monitoring.py | Schreibt in twitch_stats_category |
| EventSub WS | persistent | bot/monitoring/eventsub_ws.py | Bits, Follows, Raids, Subs, etc. |
| EventSub Webhook | pro Event | bot/monitoring/eventsub_webhook.py | Empfaengt Twitch-Events |
| Analytics-Hintergrund | varies | bot/analytics/mixin.py | Engagement-Metriken aufbauen |

## Caddy Reverse Proxy

```
twitch.earlysalty.com   -> 127.0.0.1:8765  (Dashboard-Service, Whitelist)
raid.earlysalty.com     -> 127.0.0.1:8765  (Raid-Callback)
admin.earlysalty.de     -> 127.0.0.1:8765  (Admin-Panel, separates Zertifikat)
```

Config: `C:\caddy\Caddyfile`
Binary: `C:\ProgramData\chocolatey\bin\caddy.exe`

Whitelist-Architektur: `twitch.earlysalty.com` erlaubt nur explizit gelistete Pfade in `@public_twitch`. Neue Routes muessen dort eingetragen werden.

## Verschluesselung / Secrets

| Bereich | Storage | Verschluesselung |
|---------|---------|------------------|
| Web-Sessions | PostgreSQL `dashboard_sessions` | Fernet (AES-128) |
| Social Media Tokens | PostgreSQL | AES-256-GCM |
| Raid OAuth Tokens | PostgreSQL | AES-256-GCM |
| OAuth State Tokens | PostgreSQL | Klartext (10min TTL, ephemaer) |
| Stripe Keys | Windows Credential Manager / ENV | - |
| DB-DSN | Windows Credential Manager / ENV | - |

DSN-Lookup: ENV `TWITCH_ANALYTICS_DSN` → Windows Keyring (`DeadlockBot`)

## dashboard/ Ordnerstruktur

```
bot/dashboard/
  auth/           OAuth, Sessions, Token-Management
  live/           Go-Live, Embeds, Discord-Announcements
  raids/          Raid-Dashboard, Raid-History
  affiliate/      Affiliate-Links, Tracking
  billing/        Stripe, Abo-Plaene, Payment-Events
  admin/          Admin-Panel, AGB/ToS
  core/           Templates, HTML-Helpers, Stats-Endpunkte
  mixin.py        Haupt-Assembler (importiert alle Sub-Mixins)
  routes_mixin.py Route-Registrierung (~159KB, alle Hauptroutes)
  server_v2.py    aiohttp App-Factory
  *.py (root)     Compat-Shims fuer Legacy-Imports (thin wrappers)
```

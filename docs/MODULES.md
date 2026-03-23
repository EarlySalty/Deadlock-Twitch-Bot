# Module-Uebersicht

Alle Python-Dateien mit Pfad, Zweck und Zugriffslevel (A=Admin, S=Streamer, I=Intern).

## Einstiegspunkte

| Datei | Zweck | Level |
|-------|-------|-------|
| `twitch_cog.py` | Compat-Shim, leitet zu `bot/` weiter | I |
| `twitch_cog/__init__.py` | Weiterer Shim fuer alten Import-Pfad | I |
| `bot/__init__.py` | `setup()` / `teardown()` fuer Discord.py | I |
| `bot/cog.py` | `TwitchStreamCog` — Mixin-Komposition | I |
| `bot/base.py` | `TwitchBaseCog` — DB-Init, aiohttp-Start | I |
| `bot/runtime_mode.py` | Bestimmt ob Bot im Standalone-Modus laeuft | I |

## bot/core/

| Datei | Zweck | Level |
|-------|-------|-------|
| `constants.py` | Alle Konfigurationskonstanten (Ports, Channel-IDs, Intervalle) | I |
| `partner_utils.py` | Hilfsfunktionen fuer Partner-Status-Logik | I |
| `chat_bots.py` | Registry bekannter Chat-Bots (fuer Lurker-Erkennung) | I |

## bot/monitoring/

| Datei | Zweck | Level |
|-------|-------|-------|
| `monitoring.py` | Haupt-Polling-Loop (15s), Live-Status, Stats-Logging | I |
| `eventsub_ws.py` | WebSocket-Verbindung zu Twitch EventSub | I |
| `eventsub_webhook.py` | Empfaengt Twitch-Events per HTTP-Webhook | I |
| `eventsub_mixin.py` | Registriert EventSub-Subscriptions | I |
| `sessions_mixin.py` | Stream-Session-Verwaltung (Start/Ende/Viewer) | I |
| `exp_sessions_mixin.py` | Experimentelle Session-Erweiterungen | I |
| `embeds_mixin.py` | Discord-Embed-Generierung fuer Go-Live-Posts | I |

## bot/analytics/

| Datei | Zweck | Level |
|-------|-------|-------|
| `mixin.py` | `TwitchAnalyticsMixin` — Analytics-Loop, Hintergrund-Tasks | I |
| `backend.py` | SQL-Queries fuer alle Analytics-Endpunkte (Basis) | I |
| `backend_extended.py` | Erweiterte Analytics-Queries (Viewer-Profile, Coaching etc.) | I |
| `api_overview.py` | Registriert alle `/twitch/api/v2/*` Routes + Dashboard-Pages | S |
| `api_performance.py` | Performance-Metriken (Viewer, Peak, Wachstum) | S |
| `api_audience.py` | Audience-Analyse (Demografie, Overlap, Sharing) | S |
| `api_insights.py` | Coaching, Tag-Analyse, Title-Performance | S |
| `api_raids.py` | Raid-Statistiken und -Analyse | S |
| `api_viewers.py` | Viewer-Directory, -Profile, -Segmente | S |
| `api_chat_deep.py` | Chat-Tiefenanalyse (Hype, Social-Graph, Content) | S |
| `api_experimental.py` | EXP-Endpunkte (game-breakdown, growth-curves) | S |
| `api_v2.py` | Weitere v2-Endpunkte (retention, loyalty, monetization) | S |
| `api_ai.py` | KI-gestutzte Analyse und Verlauf | S |
| `api_roadmap.py` | Roadmap CRUD-Endpunkte | S/A |
| `engagement_metrics.py` | Berechnung von Engagement-Scores | I |
| `coaching_engine.py` | Coaching-Empfehlungen basierend auf Stats | I |
| `raid_metrics.py` | Raid-Effizienz-Metriken | I |
| `legacy_token.py` | `LegacyTokenAnalyticsMixin` — alter Token-Flow | I |
| `demo_data.py` | Demo-Daten fuer oeffentliches Demo-Dashboard | I |

## bot/dashboard/

| Datei | Zweck | Level |
|-------|-------|-------|
| `mixin.py` | Haupt-Assembler, importiert alle Dashboard-Mixins | I |
| `routes_mixin.py` | Alle Route-Handler und Route-Registrierung | A/S |
| `server_v2.py` | aiohttp App-Factory, CORS, Middleware | I |
| `_compat.py` | `export_lazy` / `export_name_map` fuer Compat-Shims | I |
| `auth/auth_mixin.py` | OAuth-Flow, Discord-Auth, Session-Handling, Token-Refresh | A/S |
| `live/live.py` | Live-Status-Seite, Go-Live-Embed-Konfiguration | S |
| `live/live_announcement_mixin.py` | API fuer Live-Announcement-Konfiguration | S |
| `live/announcement_mode_mixin.py` | Announcement/Broadcast-Mode-Steuerung | A |
| `raids/raid_mixin.py` | Raid-Dashboard, Raid-History, OAuth-Callback | S |
| `affiliate/affiliate_mixin.py` | Affiliate-Links, Tracking, Klick-Ereignisse | S |
| `billing/billing_mixin.py` | Stripe-Integration, Checkout, Webhook-Handler | A/S |
| `billing/billing_plans.py` | Plan-Definitionen, Preise, Stripe-Mapping | A |
| `admin/legal_mixin.py` | AGB/ToS/Impressum/Datenschutz-Verwaltung | A |
| `core/templates.py` | HTML-Template-Generierung fuer Server-Rendered-Seiten | I |
| `core/abbo_html.py` | HTML fuer Abo/Subscriber-Seiten | I |
| `core/stats.py` | Stats-Endpunkte (Infrastruktur-Ebene) | I |

## bot/raid/

| Datei | Zweck | Level |
|-------|-------|-------|
| `mixin.py` | `TwitchRaidMixin` — Raid-Logik, Auswahl, Trigger | I |
| `commands.py` | `RaidCommandsMixin` — Discord-Commands fuer Raids | A |
| `manager.py` | Raid-Manager, Zustand, Cooldowns | I |
| `executor.py` | Fuehrt Raids aus (Twitch API-Calls) | I |
| `auth.py` | Raid-OAuth-Token-Verwaltung (AES-256-GCM verschluesselt) | S |
| `views.py` | Discord-UI-Views fuer Raid-Interaktionen | I |

## bot/chat/

| Datei | Zweck | Level |
|-------|-------|-------|
| `bot.py` | IRC-Chat-Bot-Verbindung | I |
| `connection.py` | Verbindungs-Handling, Reconnect | I |
| `commands.py` | Chat-Commands (`!twl` etc.) | S |
| `tokens.py` | Chat-Token-Verwaltung | I |
| `moderation.py` | Moderations-Aktionen ueber Chat | A |
| `irc_lurker_tracker.py` | Erkennt Lurker vs. aktive Chatter | I |
| `promos.py` | Chat-Promo-Nachrichten | A |
| `service_pitch_warning.py` | Warnt vor Service-Pitches im Chat | I |
| `constants.py` | Chat-spezifische Konstanten | I |

## bot/community/

| Datei | Zweck | Level |
|-------|-------|-------|
| `admin.py` | `TwitchAdminMixin` — Streamer hinzufuegen/entfernen/verwalten | A |
| `leaderboard.py` | `TwitchLeaderboardMixin` — Viewer-Leaderboard | S |
| `partner_recruit.py` | `TwitchPartnerRecruitMixin` — Partner-Rekrutierung | A |

## bot/social_media/

| Datei | Zweck | Level |
|-------|-------|-------|
| `clip_manager.py` | Verwaltung und Planung von Clip-Uploads | S |
| `clip_fetcher.py` | Holt Clips von der Twitch API | I |
| `dashboard.py` | Web-Routen fuer Social-Media-Dashboard | S |
| `upload_worker.py` | Hintergrund-Worker fuer Clip-Uploads | I |
| `oauth_manager.py` | OAuth-Token-Verwaltung fuer Social-Platforms | S |
| `credential_manager.py` | Verschluesselte Credential-Verwaltung | I |
| `token_refresh_worker.py` | Erneuert OAuth-Tokens im Hintergrund | I |
| `uploaders/base.py` | Abstrakte Basis-Klasse fuer Uploader | I |
| `uploaders/youtube.py` | YouTube-Upload-Implementierung | S |
| `uploaders/instagram.py` | Instagram-Upload-Implementierung | S |
| `uploaders/tiktok.py` | TikTok-Upload-Implementierung | S |
| `uploaders/video_processor.py` | Video-Vorverarbeitung (Transcoding etc.) | I |

## bot/storage/

| Datei | Zweck | Level |
|-------|-------|-------|
| `pg.py` | PostgreSQL-Verbindung, Schema-Migration, Runtime-Storage | I |
| `sessions_db.py` | Web-Session-Storage auf PostgreSQL | I |

## bot/api/

| Datei | Zweck | Level |
|-------|-------|-------|
| `twitch_api.py` | Wrapper fuer die Twitch Helix API | I |
| `token_manager.py` | App-Token- und User-Token-Verwaltung | I |

## Sonstige

| Datei | Zweck | Level |
|-------|-------|-------|
| `bot/promo_mode.py` | Promo-Mode-Logik und Validierung | A |
| `bot/reload_manager.py` | Hot-Reload fuer Bot-Module | A |
| `bot/reload_mixin.py` | Discord-Command fuer Reload | A |
| `bot/live_announce/template.py` | Template-Engine fuer Live-Announcements | I |
| `bot/compat/field_crypto.py` | Feldweise AES-256-GCM Verschluesselung | I |
| `bot/compat/http_client.py` | HTTP-Client-Compat-Wrapper | I |
| `bot/discord_role_sync.py` | Synchronisiert Discord-Rollen fuer Streamer | I |
| `bot/dashboard_service/app.py` | Standalone Dashboard-Service Entry-Point | I |
| `bot/migrations/*.py` | Einmalige DB-Migrationsskripte | I |

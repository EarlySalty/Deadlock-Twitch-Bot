# Module-Uebersicht

Alle relevanten Python-Module mit Pfad, Zweck und Zugriffslevel (A=Admin, S=Streamer, I=Intern).

> Für die **funktionsgenaue** Architektur je Subsystem (Klassen/Funktionen, Datenfluss, Stolperfallen) siehe [`docs/architecture/`](architecture/README.md). Diese Übersicht ist die 1-Satz-Ebene; dort liegt die Tiefe. Stand: 2026-06-08, gegen den Ist-Code abgeglichen. Triviale `__init__.py` und 6-Zeilen-Compat-Shims sind je Paket gesammelt vermerkt.

## Einstiegspunkte

| Datei | Zweck | Level |
|-------|-------|-------|
| `twitch_cog/__init__.py` | Compat-Shim für den alten Import-Pfad → `bot/` | I |
| `bot/__init__.py` | `setup()`/`teardown()` für discord.py + `!twl`-Proxy | I |
| `bot/cog.py` | `TwitchStreamCog` — Mixin-Komposition aller Subsysteme | I |
| `bot/base.py` | `TwitchBaseCog` — Lebenszyklus, DB-Warmup, interne API, Background-Tasks | I |
| `bot/runtime_bootstrap.py` | Start-/Stopp-Stages (`BotRuntimeBootstrap`/`DashboardRuntimeBootstrap`) | I |
| `bot/bot_service/__main__.py` | CLI-Entry des Standalone-Twitch-Workers | I |
| `bot/dashboard_service/__main__.py` | CLI-Entry des Standalone-Dashboard-Service | I |

## bot/ (Runtime & Querschnitt)

| Datei | Zweck | Level |
|-------|-------|-------|
| `runtime_mode.py` | Rollen-/Port-Härtung für die getrennten Services | I |
| `runtime_lock.py` | Single-Instance-PID/File-Lock pro Service+Port | I |
| `runtime_security.py` | Loopback-Guards (No-Auth nur lokal) | I |
| `runtime_state.py` | Kompatibilitäts-Wrapper auf `runtime/`-Contracts | I |
| `reload_manager.py` | Hot-Reload einzelner Subsysteme ohne Cog-Neustart | A |
| `reload_mixin.py` | Slash-Commands `/twitch-reload`, `/twitch-status` | A |
| `promo_mode.py` | Globaler Admin-Override für Chat-Promos | A |
| `secret_store.py` | Secret-Lookup aus keyring/ENV | I |
| `logging_setup.py` | Log-Verzeichnisse/-Dateien, Test-Runtime-Erkennung | I |
| `discord_role_sync.py` | Live-Streamer-Discord-Rolle vergeben/entziehen | I |
| `app_keys.py` | aiohttp-App-Keys zum Durchreichen von Runtime-Objekten | I |
| `runtime/bot_runtime.py` | Bot-Runtime-Contract (Config/Services/State/Container) | I |
| `runtime/dashboard_runtime.py` | `DashboardBotService` — dashboard-sichere Sicht auf Bot-Dienste | I |
| `runtime/contracts.py` | Fassade über die Split-Runtime-Contracts | I |
| `runtime/shared_config.py` | `SharedRuntimeConfig` — geteilte reine Konfigwerte | I |

## bot/core/

| Datei | Zweck | Level |
|-------|-------|-------|
| `constants.py` | Konfig-Konstanten (Ports, Channel-IDs, Intervalle, Branding) | I |
| `partner_utils.py` | Partner-Gate: liest die kanonische Partner-View | I |
| `chat_bots.py` | Registry bekannter Chat-Bots + SQL-Ausschluss | I |
| `twitch_login.py` | Login-/Profil-URL-Normalisierung | I |
| `http_client.py` | `BaseInternalHttpClient` — abgesicherter aiohttp-Transport | I |
| `llm_providers.py` | Client-Factories für Anthropic/OpenAI/MiniMax | I |

## bot/storage/

| Datei | Zweck | Level |
|-------|-------|-------|
| `pg.py` | Postgres-Layer: Schema, Transaktionen, Global-Ban, Raid-Auth-IDs, Re-Export | I |
| `partner_registry.py` | Partner-Lebenszyklus (Promote/Reactivate/Departner) + Legacy-Migration | I |
| `sessions_db.py` | Fernet-verschlüsselte Dashboard-Sessions + Rate-Limit-Slots | I |
| `_pool.py` | Prozesslokaler psycopg-Connection-Pool (pro DSN) | I |
| `_rows.py` | `StorageRow` — dict-/sequence-artiger Zeilen-Wrapper | I |
| `auto_raid_pause.py` | Admin-gesetzte Auto-Raid-Pause (set/clear/get/is) | A |
| `promo_cooldowns.py` | Persistenz der Chat-Promo-Cooldowns | I |

## bot/api/

| Datei | Zweck | Level |
|-------|-------|-------|
| `twitch_api.py` | `TwitchAPI` — Helix-Wrapper (App-Token, Retries, EventSub) | I |
| `token_manager.py` | Bot-Chat-Token: Auto-Refresh + Persistenz | I |
| `token_error_handler.py` | Token-Fehler-Lebenszyklus: Blacklist, Grace-Period, Bot-Ban | I |
| `twitch_auth.py` | OAuth-Credential-Helfer + `TwitchClientConfigError` | I |

## bot/monitoring/

| Datei | Zweck | Level |
|-------|-------|-------|
| `monitoring.py` | `TwitchMonitoringMixin` — 15-s-Polling-Loop, Live-State, Stats | I |
| `eventsub_mixin.py` | EventSub-Kapazität + Listener-Orchestrierung, dynamische Raid-Subs | I |
| `eventsub_ws.py` | `EventSubWSListener` — ein WebSocket-Client (Reconnect/Dedup) | I |
| `eventsub_ws_pool.py` | `EventSubWSListenerPool` — verteilt Subs auf bis zu 3 Transporte | I |
| `eventsub_webhook.py` | Webhook-Handler für eingehende EventSub-Requests | I |
| `eventsub_processing_inbox.py` | Durable Leased-Work-Queue für asynchrone Verarbeitung | I |
| `eventsub_state_store.py` | Cross-Transport-Guard (Dedup/Throttle/Once-only) | I |
| `eventsub_core_callbacks.py` | Gemeinsame Callback-Registrierung für WS + Webhook | I |
| `sessions_mixin.py` | Stream-Session-Lebenszyklus (Start/Sample/Finalize) | I |
| `exp_sessions_mixin.py` | Parallele Session-Logik fürs Experimental-Analytics | I |
| `embeds_mixin.py` | Go-Live-/Offline-Embeds + Tracking-Button-Views | I |
| `partner_ops.py` | (Neu-)Berechnung der Partner-Raid-Scores anstoßen | I |

## bot/chat/

| Datei | Zweck | Level |
|-------|-------|-------|
| `bot.py` | Chat-Bot-Klasse + Factory `create_twitch_chat_bot`, Bot-Token | I |
| `connection.py` | IRC-Verbindung, NAMES-Polling, EventSub-Chat-Subscriptions | I |
| `moderation.py` | Auto-Moderation, Spam-Scoring (Homoglyphen), Ban/Cleanup | A |
| `promos.py` | Promo-Loop + Fake-Server-Warnung, Lurker-Tax, Cooldowns | A |
| `service_pitch_warning.py` | Scam-Pitch-Erkennung von Chattern (Account-Alter/Sequenz) | I |
| `commands.py` | Chat-/Prefix-Commands (`!twl`, Raid-Commands) | S |
| `engagement_commands.py` | Chat-Commands rund um Engagement | S |
| `irc_lurker_tracker.py` | Zweite IRC-Quelle fürs Lurker-/Presence-Tracking | I |
| `lurker_policy.py` | Policy: wann ist passives Lurken der Endzustand | I |
| `spam_ai_review.py` | Selbstlernender Spam-Filter via MiniMax (Spam-+Safe-Muster) | I |
| `global_ban_sweep.py` | Fällige Offline-Sweeps der globalen Bannliste | A |
| `targeted_promo.py` | Zielgerichtete Promos mit MiniMax-Preset-Wahl | A |
| `self_explainer.py` | Grounded Q&A über den Bot (Anti-Injection) | S |
| `timeout_guard.py` | Mute-Schwellen + „werbefrei"-Pitch | I |
| `tokens.py` | Bot-Token-Registrierung mit twitchio | I |
| `constants.py` | Chat-spezifische Konstanten | I |

## bot/raid/

| Datei | Zweck | Level |
|-------|-------|-------|
| `mixin.py` | `TwitchRaidMixin` — Host-Mixin, baut/exponiert alle Services | I |
| `commands.py` | `RaidCommandsMixin` — Discord-Commands (Auth/Enable/Status/History) | S/A |
| `executor.py` | `RaidExecutor` — Raid via Helix starten/abbrechen + History | I |
| `manager.py` | Raid-Manager (Zustand/Trigger-Verwaltung) | I |
| `views.py` | Discord-UI-Views für Raid-Interaktionen | I |
| `auth.py` | `RaidAuthManager` — OAuth-User-Token (FieldCrypto-verschlüsselt) | S |
| `scope_profiles.py` | OAuth-Scope-Profile (Scopes je Profil) | I |
| `partner_scores.py` | Vorberechneter Partner-Raid-Score-Cache | I |
| `partner_raid_score_tracking.py` | Bestätigte Raids + Post-Raid-Deadlock-Dauer tracken | I |
| `partner_resolution.py` | Partner-Lookup + Arrival-Klassifizierung | I |
| `raid_pipeline.py` | `RaidPipelineService` — Kandidaten → Auswahl → Ausführung | I |
| `raid_tracking_runtime.py` | Tracking offener/erwarteter Raids | I |
| `raid_arrival_runtime.py` | Ankunft bestätigen, Signal-Pläne ausführen | I |
| `signal_correlation.py` | Mehrere Raid-Signale korrelieren | I |
| `runtime_factories.py` | Composition-Root: `make_*`-Factories aller Services | I |
| `chat_targets.py` | `ChatTarget` + Outbound-Suppression-Lookup | I |
| `facades/tracking_arrival.py` | Runtime-Tracking/Arrival an den Cog binden | I |
| `facades/data_setup.py` | Datenaufbau-Helfer-Facade | I |
| `services/recruitment_messaging.py` | Recruitment-Nachrichten an externe Streamer | A |
| `services/raid_blacklist.py` | External-Recruitment-Blacklist + Ban-Check | A |
| `services/raid_data_sources.py` | Deadlock-Eligibility, Partner-Roster, Online-Kandidaten | I |
| `services/partner_setup_service.py` | Post-Auth-Setup (Rolle, Trial, First-Login, Aktivierung) | I |
| `services/candidate_selection.py` | Score-basierte + faire Kandidatenwahl | I |
| `services/*` (weitere) | RaidStateStore, ManualRaidSuppression, PartnerArrivalTracking, RaidMetricsStore, CandidateFollowers, OfflineRaidOrchestrator, ExternalRecruitment, ArrivalConfirmation, PartnerRaidDelivery, RaidObservability | I |

## bot/analytics/

| Datei | Zweck | Level |
|-------|-------|-------|
| `mixin.py` | `TwitchAnalyticsMixin` — bot-seitiges Sammeln (Chatter/Subs/Ads), Self-Heal | I |
| `api_v2.py` | Komponiert die Topic-Mixins + registriert `/twitch/api/v2/*` | S |
| `api_overview.py` | Overview-KPIs **+** SPA-/Asset-Serving (Dashboard/Admin/Demo) | S |
| `api_performance.py` | Performance-Metriken (Viewer, Peak, Wachstum) | S |
| `api_audience.py` | Audience (Watch-Time, Funnel, Overlap, Demografie, Loyalty) | S |
| `api_viewers.py` | Viewer-Directory, -Profile, -Segmente | S |
| `api_insights.py` | Coaching, Tag-Analyse, Title-Performance | S |
| `api_chat_deep.py` | Chat-Tiefenanalyse (Hype, Social-Graph, Content) | S |
| `api_ai.py` | KI-Analyse + Verlauf (MiniMax) | S |
| `api_post_stream.py` | Post-Stream-KI-Analyse (MiniMax) | S |
| `api_raids.py` | Raid-Statistiken und -Analyse | S |
| `api_viewer_timeline.py` | Viewer-Presence-Gantt-Daten | S |
| `api_experimental.py` | EXP-Endpunkte (game-breakdown, growth-curves) | S |
| `api_roadmap.py` | Roadmap-CRUD | S/A |
| `api_public.py` | Öffentliche Endpunkte (Demo/Public) | S |
| `api_admin.py` | Admin-only Endpunkte (Streamer, Health, Error-Log, Config, CSRF) | A |
| `admin_streamer_queries.py` | Admin-SQL: Streamer-Abfragen | A |
| `admin_affiliate_queries.py` | Admin-SQL: Affiliate-Abfragen | A |
| `admin_config_queries.py` | Admin-SQL: Config-Abfragen | A |
| `backend.py` | SQL-Queries für die v2-Endpunkte (Basis) | I |
| `backend_extended.py` | Erweiterte Analytics-Queries (Viewer-Profile, Coaching) | I |
| `coaching_engine.py` | `CoachingEngine` — datenbasierte Empfehlungen (keine KI) | I |
| `demo_data.py` | Demo-Daten fürs öffentliche Demo-Dashboard | I |
| `engagement_metrics.py` | Geteilte Engagement-KPI-Berechnung | I |
| `raid_metrics.py` | Raid-Effizienz-Metriken | I |
| `raw_chat_status.py` | Roh-Chat-Health-Status | I |
| `audit_log.py` | Audit-Logging der Admin-Aktionen | A |
| `chat_social_graph_loader.py` | Social-Graph-Daten laden | I |
| `insights_monetization_loader.py` | Monetarisierungs-Insights laden | I |
| `legacy_token.py` | `LegacyTokenAnalyticsMixin` — alter Token-Flow | I |
| `error_utils.py` | Fehler-Helfer | I |
| `services/internal_home.py` | Payload der Streamer-Dashboard-Startseite | S |
| `post_stream/report_builder.py` | Strukturierter Post-Stream-Report (Input für die KI) | I |

## bot/dashboard/

| Datei | Zweck | Level |
|-------|-------|-------|
| `server_v2.py` | `build_v2_app(...)` — aiohttp-App-Factory + Security-Middleware | I |
| `mixin.py` | `TwitchDashboardMixin` — bot-seitige Kompat-Brücke | I |
| `pages.py` | Wiederverwendbare HTML-Builder für Seiten | I |
| `routes_mixin.py` | Route-Registrierung + Handler | A/S |
| `routes_entry.py` / `routes_market.py` / `routes_billing.py` / `routes_self_explainer.py` / `routes_title.py` / `routes_settings.py` | Route-Gruppen (Entry/Market/Billing/Self-Explainer/Title/Settings) | A/S |
| `route_deps.py` | Geteilte Route-Abhängigkeiten | I |
| `streamer_admin_mixin.py` | Streamer-Verwaltung/Verifizierung (add/remove/verify/archive) | A |
| `dashboard_metrics_mixin.py` | Dashboard-Metriken | I |
| `abbo_routes.py` / `abbo_billing_routes.py` | Abo-Selfservice (pay/profile/cancel/invoices) | S |
| `auth/auth_mixin.py` | Discord-Admin-OAuth + Streamer-Auth + Sessions | A/S |
| `auth/services.py` | `PartnerAccessService`/`PartnerLoginTokenService` | I |
| `auth/state_store.py` | Auth-State-Store + Rate-Limit-Store | I |
| `auth/partner_auth_mixin.py` | Partner-One-Time-Login → Cookie-Session | S |
| `auth/fingerprint_mixin.py` | Post-Login-JS-Fingerprint | I |
| `billing/billing_mixin.py` | Stripe-Checkout/Status, Plan-Gating | A/S |
| `billing/billing_plans.py` | Plan-Katalog (Preise, Features, IDs) | A |
| `affiliate/affiliate_mixin.py` | Affiliate-Signup, Stripe-Connect, Claims, Provisionen | S |
| `affiliate/gutschrift.py` | Gutschrift-(Credit-Note-)Erzeugung + PDF | S |
| `affiliate/affiliate_pii.py` | Getrennte PII-Ablage | I |
| `affiliate/affiliate_email.py` | `AffiliateEmailSender` | I |
| `live/live.py` | Live-Status-Seite + Go-Live-Embed-Konfig | S |
| `live/live_announcement_mixin.py` | API für Live-Announcement-Konfig | S |
| `admin/legal_mixin.py` | Impressum/Datenschutz/AGB + Legal-Gate | A |
| `admin/announcement_mode_mixin.py` | Announcement-Mode-Steuerung | A |
| `raids/raid_mixin.py` | Raid-Dashboard-Seite + API | S |
| `raids/pages.py` | Raid-Seiten-Rendering | S |
| `raids/oauth_callback.py` | Twitch-OAuth-Callback (Raid-Scope-Flow) | S |
| `core/templates.py` | HTML-/Render-Helfer | I |
| `core/stats.py` | Stats-Seiten-Rendering (Legacy-Surface) | I |
| `core/abbo_html.py` | HTML der Abo-Seiten | I |
| `_compat.py` + Shims (`auth_mixin.py`, `billing_mixin.py`, `legal_mixin.py`, …) | Lazy-Re-Export für alte Importpfade | I |

## bot/engagement/

| Datei | Zweck | Level |
|-------|-------|-------|
| `pipeline.py` | Orchestrierung: Chat-Turn → Entscheidung → Antwort | I |
| `dashboard_api.py` | JSON-API `/twitch/api/v2/engagement/*` (Settings/Log/Sender-Auth) | S/A |
| `sender_auth.py` | OAuth des separaten Engagement-Sende-Accounts | A |
| `threads.py` | Konversations-Fäden mit Lebenszyklus (`Thread`) | I |
| `minimax_chat.py` | `EngagementMinimaxClient` + System-Prompt-Bau | I |
| `background.py` | Hintergrund-Tasks (Thread-Extraktion, Auto-Close) | I |
| `irc_reader.py` | Chat-Lesequelle fürs Engagement | I |
| `match_context.py` / `stream_transcripts.py` | Spiel-/Stream-Kontext fürs Prompting | I |
| `deadlock_wiki.py` / `deadlock_patches.py` | Wiki-/Patch-Grounding (gegen Halluzination) | I |
| `soul_store.py` / `style_examples.py` / `persona.py` | Charakter/Soul, Few-Shot-Stil, adaptiver Channel-Vibe | I |

## bot/community/

| Datei | Zweck | Level |
|-------|-------|-------|
| `admin.py` | `TwitchAdminMixin` — Streamer hinzufügen/entfernen/verwalten | A |
| `leaderboard.py` | `TwitchLeaderboardMixin` — interaktives `!twl`-Leaderboard | S |
| `partner_recruit.py` | `TwitchPartnerRecruitMixin` — Partner-Rekrutierung (Tageslimit) | A |
| `voice_reaction/scheduler.py` | Asyncio-Scheduler: Trigger → Capture → Brain → Reaktion | I |
| `voice_reaction/conversation_brain.py` | Anthropic-Claude-Adapter (`ConversationBrain`) | I |
| `voice_reaction/audio_capture.py` | Stream-Audio-Capture via streamlink | I |
| `voice_reaction/mixin.py` | Steckt Voice-Reaction in den `RaidChatBot` | I |
| `voice_reaction/*` (weitere) | state_store, prompts, sanity_filter, chat_listener, chat_message_sender, discord_notifier, audit_log | I |

## bot/social_media/

| Datei | Zweck | Level |
|-------|-------|-------|
| `clip_manager.py` | `ClipManager` — Pipeline-Orchestrierung + Lebenszyklus | S |
| `clip_fetcher.py` | Holt Twitch-Clips als Pipeline-Input | I |
| `dashboard.py` | Admin-Dashboard-Sektion für Social Media | S |
| `storage.py` / `settings.py` | Persistenz + Konfiguration der Pipeline | I |
| `enrichment.py` (+ `enrichment_worker.py`) | Titel/Hashtags/Beschreibung via LLM | I |
| `oauth_manager.py` | Plattform-OAuth-Flows (TikTok/Instagram/YouTube) | S |
| `credential_manager.py` | Verschlüsselte Token-/Credential-Verwaltung | I |
| `token_refresh_worker.py` | Refresht Plattform-Tokens im Hintergrund | I |
| `upload_worker.py` / `approval_worker.py` | Worker für Upload bzw. Freigabe | I |
| `retention.py` (+ `retention_worker.py`) | Aufräum-/Retention-Policy | I |
| `rendering.py` | Render-Helfer | I |
| `uploaders/base.py` | Abstrakte `PlatformUploader` | I |
| `uploaders/tiktok.py`, `uploaders/instagram.py`, `uploaders/youtube.py` | Plattform-Uploader (Upload + Analytics) | S |
| `uploaders/video_processor.py` | Plattformgerechtes Re-Encoding/Zuschnitt (ffmpeg) | I |
| `approval/approval_service.py` | Review-Workflow vor Upload | A/S |
| `transcription/whisper.py` | Whisper-Transkription | I |
| `transcription/vocab.py`, `transcription/seed_vocab.py`, `transcription/correction.py` | Deadlock-Vokabular + Transkript-Korrektur | I |
| `llm/dispatcher.py` | Wählt den LLM-Provider (Consent-gated) | I |
| `llm/claude_haiku.py`, `llm/minimax.py`, `llm/ollama.py`, `llm/base.py`, `llm/prompts.py`, `llm/_parsing.py` | LLM-Provider + Prompt/Parsing | I |
| `analytics/report_writer.py`, `analytics/report_dispatcher.py`, `analytics/insights_worker.py` | Plattform-Analytics + Reports | S |
| `layout/storage.py` | Video-Layout/Templates | S |

## bot/highlight_clipper/

| Datei | Zweck | Level |
|-------|-------|-------|
| `worker.py` | `HighlightClipperWorker` — Loop pro Streamer/Match | I |
| `demo_analyzer.py` | Demo-basierte Event-Erkennung (`KillMoment`) | I |
| `event_detector.py` | Match-basierte Event-Erkennung (Multikills/Teamfights) | I |
| `twitch_vod.py` | VOD zum Match finden + Clip schneiden | I |
| `demo_downloader.py` / `deadlock_client.py` | Demo laden / Deadlock-API-Client | I |
| `state.py` / `dm_sender.py` / `mixin.py` / `config.py` | State, DM-Versand, `HighlightClipperMixin`, Konfig | I |

## bot/title_generator/

| Datei | Zweck | Level |
|-------|-------|-------|
| `title_ai.py` | MiniMax-Titel + Rate-Limiter, Insight-Generierung | S |
| `title_db.py` | Persistenz Titel-Historie + Wissens-Titel | I |
| `knowledge_job.py` | Nächtlicher Job: erfolgreiche Titel je Größenklasse lernen | I |
| `insight_job.py` | Wöchentlicher Insight-Job je Partner | I |
| `steam_lookup.py` | Rang + Live-In-Game-State eines Discord-Users | I |

## rust/crates/tb-stream-audit, rust/bin/tb-stream-audit

| Datei | Zweck | Level |
|-------|-------|-------|
| `crates/tb-stream-audit/` | Regeln, Modellschritt, Plan, Bericht, Meldung des Coaching-Audits | A |
| `bin/tb-stream-audit/` | Aufsicht, Live-Aufnahme, Auswertung, Ablage, Aufraeumen | A |

Der alte Python-Pfad `bot/stream_coaching_audit/` ist abgeloest; VOD- und
Dateimodus gibt es nicht mehr.

## bot/entitlements/

| Datei | Zweck | Level |
|-------|-------|-------|
| `catalog.py` | Plan-Metadaten + abgeleitete Entitlements | I |
| `repository.py` | Plan-Snapshot auflösen (Override > Stripe) | I |
| `resolver.py` | Dünne Auflösungs-Fassade | I |

## bot/internal_api/

| Datei | Zweck | Level |
|-------|-------|-------|
| `app.py` | Baut die loopback-API-App (:8776), verdrahtet Routen/Middleware | I |
| `policy.py` | Token-Vergleich, Loopback-/Proxy-Checks, JSON/Fehler | I |
| `contracts.py` | `InternalApiCallbacks`, Idempotenz-Typen, Konstanten | I |
| `runner.py` | `InternalApiRunner` — Server-Lebenszyklus | I |
| `client.py` | Interner Bot-API-Client | I |
| `routes/streamers.py` | Streamer-/Admin-/Stats-Routen | I |
| `routes/raid.py` | Raid-Auth-/OAuth-Routen | I |
| `routes/telemetry.py` | Health/Observability/Chatters/Live | I |
| `routes/discord_log.py` | Self-Explainer-Q&A per Master-Broker nach Discord | I |
| `routes/global_ban.py` | Globale Bannliste (Add/Remove/Check/List) | I |
| `routes/streamer_link.py` | Unverknüpfte Streamer (Discord-Match-Kandidaten) | I |

## bot/bot_service/ + bot/dashboard_service/

| Datei | Zweck | Level |
|-------|-------|-------|
| `bot/bot_service/app.py` | `HeadlessBot` + `run_bot_service()` (Worker ohne Discord-Gateway) | I |
| `bot/dashboard_service/app.py` | `build_dashboard_service_app()` + `run_dashboard_service()` | I |
| `bot/dashboard_service/eventsub_bridge.py` | EventSub auf Dashboard-Seite → Bot (durable, Retry) | I |
| `bot/dashboard_service/client.py` | HTTP-Client für Bot-Operationen | I |

## bot/compat/

| Datei | Zweck | Level |
|-------|-------|-------|
| `field_crypto.py` | `FieldCrypto` — AES-256-GCM-Feldverschlüsselung (Raid-Tokens) | I |
| `http_client.py` | DNS-resilienter aiohttp-Connector | I |

## bot/migrations/

| Datei | Zweck | Level |
|-------|-------|-------|
| `*.py` | Einmalige, idempotente CLI-Migrationsskripte (Schema/Backfill); Social-Media-Phasen 0–4, `drop_legacy_tokens`, Viewer-Presence, Observability, Engagement-Layer u. a. | I |
| `*.sql` | Begleitende SQL-Schemata (affiliate, engagement_layer, channel_profile, global_sentiment …) | I |

## Frontends (kein Python)

| Bereich | Zweck | Level |
|---------|-------|-------|
| `bot/dashboard_v2/` | Streamer-Analytics-SPA (React/TS) — siehe [architecture/frontend-streamer-dashboard.md](architecture/frontend-streamer-dashboard.md) | S |
| `bot/dashboard_preview/` | Lokale Vorschau-Variante des Streamer-Dashboards | I |
| `bot/admin_dashboard/` | Admin-SPA (React/TS) — siehe [architecture/frontend-admin-dashboard.md](architecture/frontend-admin-dashboard.md) | A |
| `website/` | Öffentliche Landing-/Onboarding-/Affiliate-Site — siehe [architecture/frontend-website.md](architecture/frontend-website.md) | — |

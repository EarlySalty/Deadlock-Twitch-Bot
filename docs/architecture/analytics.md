# analytics/ — Architektur & Funktionsreferenz

> Pfad: `bot/analytics/` · Stand: 2026-06-08 · 36 Dateien, ~33.320 Zeilen (größtes Subsystem)
>
> Teil der [Architektur-Doku](README.md). Routen-Liste: [../API.md](../API.md). Verwandt: [monitoring.md](monitoring.md) (Session-/Stats-Quelle), [dashboard.md](dashboard.md) (App-Factory bindet die v2-Routen ein), [storage.md](storage.md), [internal/analytics-internal.md](../internal/analytics-internal.md).

## 1. Zweck & Abgrenzung

`analytics/` ist die **Auswertungs- und API-Schicht**. Es hat **zwei klar getrennte Seiten**:

1. **Sammeln (bot-seitig):** `mixin.py::TwitchAnalyticsMixin` läuft in der BotRuntime, pollt Chatter/Subs/Ads und schreibt Analytics-Rohdaten in die DB.
2. **Servieren (dashboard-seitig):** die vielen `_Analytics*Mixin`-Topic-Mixins (komponiert in `api_v2.py`) registrieren die `/twitch/api/v2/*`-Endpunkte, die das React-Frontend konsumiert — **und** servieren die statischen SPA-Assets (Dashboard-v2, Admin, Demo).

Dazu kommen: die **Coaching-Engine** (datenbasierte Empfehlungen, ohne KI), die **Demo-Daten** fürs öffentliche Demo-Dashboard, die **Internal-Home**-Payload (Streamer-Dashboard-Startseite), **Post-Stream-Reports** (KI via MiniMax) und das Admin-Query-Set.

Abgrenzung: Die reine DB-Mechanik liegt in [storage.md](storage.md); die Session-/Stats-Erfassung in [monitoring.md](monitoring.md). `analytics/` rechnet daraus Kennzahlen und liefert sie aus.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Bot-seitig** | `TwitchAnalyticsMixin` ist Teil von `TwitchStreamCog`; sammelt über `api/` (Helix Chatters/Subs/Ads) und schreibt via `storage/`. |
| **Dashboard-seitig** | Die v2-API-Mixins werden von `dashboard/server_v2.build_v2_app` registriert; lesen via `storage/`/`backend*.py`. |
| **Nutzt** | `storage/` (Reads), `api/` (Helix), MiniMax (Post-Stream/AI), `core/` (Bot-Ausschluss in SQL). |
| **DB-Tabellen** | Sessions/Stats/Viewer-Presence/Chat-Messages/Subs/Ads/Raids + `exp_*` (siehe [DATABASE.md](../DATABASE.md)). |
| **Externe Dienste** | Twitch-Helix (Sammeln), MiniMax (KI-Analyse). |

## 3. Dateien im Überblick (nach Funktion)

| Gruppe | Dateien (Zeilen) | Rolle |
|--------|------------------|-------|
| **Sammeln** | `mixin.py` (2472) | `TwitchAnalyticsMixin` — Analytics-Loop, Chatter/Subs/Ads-Polling, Moderator-Self-Heal, Observability. |
| **API-Assembler** | `api_v2.py` (2849) | Komponiert die Topic-Mixins, registriert `/twitch/api/v2/*`. |
| **API-Topics** | `api_overview.py` (2378), `api_performance.py` (1778), `api_audience.py` (1627), `api_viewers.py` (1012), `api_insights.py` (1031), `api_chat_deep.py` (1100), `api_ai.py` (1155), `api_post_stream.py` (1376), `api_raids.py` (473), `api_viewer_timeline.py` (445), `api_experimental.py` (296), `api_roadmap.py` (303), `api_public.py` (241) | je ein Themen-Mixin mit Endpunkten. |
| **Admin** | `api_admin.py` (2276), `admin_streamer_queries.py` (885), `admin_affiliate_queries.py` (598), `admin_config_queries.py` (174) | Admin-only Endpunkte + Queries. |
| **Query-Layer** | `backend.py` (862), `backend_extended.py` (839) | SQL-Queries (Basis + erweitert). |
| **Coaching** | `coaching_engine.py` (1633) | datenbasierte Coaching-Empfehlungen. |
| **Demo** | `demo_data.py` (3590) | Demo-Daten fürs öffentliche Demo-Dashboard. |
| **Internal-Home** | `services/internal_home.py` (1184) | Payload der Streamer-Dashboard-Startseite. |
| **Post-Stream** | `post_stream/report_builder.py` (801) | KI-Report-Aufbau nach Stream-Ende. |
| **Loader/Helfer** | `insights_monetization_loader.py` (355), `chat_social_graph_loader.py` (105), `engagement_metrics.py` (102), `raid_metrics.py` (271), `raw_chat_status.py` (386), `audit_log.py` (639), `error_utils.py` (19), `legacy_token.py` (46) | spezialisierte Datenlader/KPIs. |

## 4. Datenfluss / Lebenszyklus

**Sammeln:** `TwitchAnalyticsMixin.collect_analytics_data()` (Loop, via `_before_analytics`) und `collect_chatters_data()`/`_poll_chatters_single()` holen pro live-Kanal die Chatter-Liste, Subs (`_collect_subs_for_user`) und Ad-Schedule (`_collect_ads_schedule_for_user`) über die User-Token-Endpoints (mit Legacy-Fallback `_get_*_result_with_legacy_fallback`) und schreiben sie in die DB. Bot-Chatter werden ausgeschlossen (`core/chat_bots`). Nebenbei: **Moderator-Self-Heal** (`_attempt_bot_moderator_self_heal`, `_restore_bot_ban_opt_out_if_healthy`) versucht, einen kanalseitigen Bot-Mod-Verlust zu reparieren.

**Servieren:** `dashboard/server_v2.build_v2_app` bindet den v2-API-Mixin ein. Ein Request auf `/twitch/api/v2/<topic>` läuft in die jeweilige `_api_v2_*`-Methode des Topic-Mixins → liest über `backend*.py`/Loader → liefert JSON. `api_overview` serviert zusätzlich die SPA-HTML/-Assets (Auth-/Host-Gates, Dist-Root-Auflösung).

**Coaching:** `CoachingEngine.get_coaching_data(...)` aggregiert mehrere Analysen (Effizienz, Titel-Keywords, Schedule, Retention-Kurve, Doppel-Stream-Erkennung, Raid-Netzwerk, Peer-Vergleich, Konkurrenzdichte) und baut daraus priorisierte Empfehlungen — rein rechnerisch (Pearson-Korrelation etc.), ohne KI.

**Post-Stream:** Nach Stream-Ende baut `post_stream/report_builder.py` einen Report; `api_post_stream.py` lässt MiniMax daraus eine narrative KI-Analyse formulieren.

**Internal-Home:** `services/internal_home.build_internal_home_payload(...)` setzt die Dashboard-Startseite zusammen (Identität, OAuth-Status, KPIs, jüngste Ban-/Raid-Events, Chat-Count, Wochenvergleich, Health-Score, Live-Status) — mit Caching (vgl. CHANGELOG-Beispiel).

## 5. Funktionsreferenz pro Bereich

### mixin.py — `TwitchAnalyticsMixin` (Sammeln)
- `collect_analytics_data()` / `_before_analytics()` — die Sammel-Loop.
- `collect_chatters_data()` / `_poll_chatters_single()` / `_should_defer_chatters_collection_for_startup()` — Chatter-Polling (mit Startup-Defer).
- `_collect_subs_for_user(...)` / `_collect_ads_schedule_for_user(...)` — Subs/Ads pro User.
- Legacy-Fallback: `_get_chatters_result_with_legacy_fallback`, `_get_subscriptions_result_with_legacy_fallback`, `_get_ad_schedule_result_with_legacy_fallback`.
- Bot-Chatter-Auflösung: `_collect_bot_chatters_runtime_sources`, `_resolve_bot_chatters_fallback`, `_format_bot_chatters_diagnostics`.
- Moderator-Self-Heal: `_is_moderator_self_heal_target`, `_attempt_bot_moderator_self_heal`, `_restore_bot_ban_opt_out_if_healthy`, `_moderator_self_heal_cooldowns`.
- Observability: `_increment_analytics_observability_counter`, `_log_analytics_decision`, `get_analytics_observability_snapshot`, `_store_analytics_diagnostic`, `_build_analytics_runtime_state`, `_scope_presence_state`.
- IRC-Experiment: `_record_irc_lurker_experiment_sample`, `_finalize_irc_lurker_experiment_session`.

### api_v2.py + Topic-Mixins (Servieren)
`api_v2.py` komponiert die Topic-Mixins zu einem Server-Mixin und registriert die Routen (`_register_v2_routes`). Die Topic-Mixins:
- `_AnalyticsOverviewMixin` (`api_overview.py`) — Overview-KPIs (`_api_v2_overview`, `_get_overview_data`, `_calculate_overview_metrics`, `_calculate_health_scores`, `_get_network_stats`) **und** SPA-Serving: `_serve_dashboard`, `_serve_dashboard_v2`, `_serve_admin_dashboard`, `_serve_demo_dashboard`, `_serve_affiliate_portal`, `_serve_pricing`, Asset-Auflösung (`_resolve_dashboard_v2_asset_response`, …), Redirects, Runtime-Config-Injektion (`_inject_dashboard_runtime_config`), Host-/Auth-Gates.
- `_AnalyticsPerformanceMixin` (`api_performance.py`) — Performance-Metriken (Viewer, Peak, Wachstum).
- `_AnalyticsAudienceMixin` (`api_audience.py`) — `_api_v2_watch_time_distribution`, `_api_v2_follower_funnel`, `_api_v2_viewer_overlap`, `_api_v2_audience_insights`, `_api_v2_audience_demographics`, `_api_v2_loyalty_curve` (mit `_compute_weighted_peak_hours`, `_quantile`).
- weitere: `api_viewers.py` (Viewer-Directory/-Profile), `api_insights.py` (Coaching/Tags/Titel), `api_chat_deep.py` (Chat-Tiefenanalyse, Social-Graph), `api_ai.py` (KI-Analyse + Verlauf), `api_post_stream.py` (Post-Stream-KI), `api_raids.py` (Raid-Stats), `api_viewer_timeline.py` (Viewer-Presence-Gantt, siehe [VIEWER_PRESENCE_TIMELINE.md](../VIEWER_PRESENCE_TIMELINE.md)), `api_experimental.py` (EXP-Endpunkte), `api_roadmap.py` (Roadmap-CRUD), `api_public.py` (öffentliche Endpunkte).

### api_admin.py — `_AnalyticsAdminMixin`
Admin-only Endpunkte (`_register_v2_admin_api_routes`): Streamer-Liste/-Detail (`_api_admin_streamers`, `_api_admin_streamer_detail`), OAuth-Scopes (`_api_admin_system_oauth_scopes`), System-Health, Error-Log, Affiliate-Admin, Announcements-Config (`_admin_load_announcements_config`/`_admin_save_announcements_body`). CSRF: `_admin_extract_csrf`/`_admin_verify_csrf`. Secret-Maskierung (`_admin_mask_secret`). CTE-SQL-Builder: `_admin_partner_state_cte_sql`, `_admin_partner_live_state_cte_sql`, `_admin_partner_oauth_cte_sql`, `_admin_last_stream_session_cte_sql`. Read-only-Query-Runner (`_run_admin_readonly_query`) für die Admin-DB-Query-Konsole.

### backend.py / backend_extended.py
Die SQL-Query-Schicht: `backend.py` enthält die Basis-Queries für die v2-Endpunkte, `backend_extended.py` die erweiterten (Viewer-Profile, Coaching-Inputs etc.). Hier liegen die eigentlichen SELECTs hinter den API-Methoden.

### coaching_engine.py — `CoachingEngine`
- `get_coaching_data(...)` — Einstiegspunkt; baut alle Analysen + Empfehlungen.
- Analysen (privat): `_efficiency`, `_title_analysis`/`_extract_keywords`, `_schedule_optimizer`, `_duration_analysis`, `_retention_coaching`/`_build_viewer_curve`, `_double_stream_detection`, `_chat_concentration`, `_raid_network`, `_peer_comparison`, `_competition_density`, `_cross_community`, `_tag_optimization`/`_split_tags_from_rows`, `_build_recommendations`, Statistik-Helfer `_pearson`.

### demo_data.py
Erzeugt realistisch wirkende **Demo-Daten** fürs öffentliche Demo-Dashboard (keine echten Streamer-Daten) — größte Einzeldatei, weil sie alle v2-Antwortformen mit Beispieldaten nachbildet.

### services/internal_home.py
Baut die Streamer-Dashboard-Startseite (`build_internal_home_payload`). Bausteine: `internal_home_identity_block`, `internal_home_oauth_status_from_conn`, `internal_home_kpis_and_recent_from_conn`, `internal_home_ban_events_from_conn`, `internal_home_raid_events_from_conn`, `internal_home_chat_count_from_conn`, `internal_home_week_comparison`, `internal_home_health_score`, `internal_home_live_status`. Liest u. a. Service-Warn-/Autoban-Log-Events (`load_internal_home_service_warning_events`, `load_internal_home_autoban_events`). Config `InternalHomeServiceConfig`.

### post_stream/report_builder.py
Baut den strukturierten Post-Stream-Report (Kennzahlen + Vergleich), den `api_post_stream` per MiniMax in eine Analyse übersetzt.

### Loader & KPIs
- `engagement_metrics.py` — `calculate_engagement(EngagementInputs) -> EngagementOutputs` (geteilte Engagement-KPI-Berechnung).
- `raid_metrics.py` — Raid-Effizienz-Metriken. `chat_social_graph_loader.py` — Social-Graph-Daten. `insights_monetization_loader.py` — Monetarisierungs-Insights. `raw_chat_status.py` — Roh-Chat-Health-Status. `audit_log.py` — Audit-Logging der Admin-Aktionen. `legacy_token.py` — `LegacyTokenAnalyticsMixin` (alter Token-Flow). `error_utils.py` — Fehler-Helfer.

## 6. Datenbank & externe Schnittstellen

- **DB (lesend, vieles):** Sessions/Stats/Viewer-Presence/Chat-Messages/Subs/Ads/Raids + `exp_*` — Spalten in [DATABASE.md](../DATABASE.md).
- **HTTP:** `/twitch/api/v2/*` (Liste in [../API.md](../API.md)) + SPA-Serving (`/analyse`, `/twitch/dashboard`, Demo).
- **Extern:** Twitch-Helix (Sammeln), MiniMax (Post-Stream/AI).

## 7. Stolperfallen / Besonderheiten

- **Zwei Laufzeiten, ein Modul:** `mixin.py` läuft in der BotRuntime (schreibt), die API-Mixins in der DashboardRuntime (lesen). Wer „warum kommen keine Daten an?“ debuggt, muss wissen, **welche** Seite betroffen ist.
- **`api_overview` serviert auch die Frontends:** Static-Asset-Serving + Auth-/Host-Gates liegen hier, nicht in `dashboard/`. Ein 404 auf `/analyse`-Assets ist oft ein Overview-Mixin-Thema.
- **Coaching ≠ KI:** `coaching_engine` rechnet deterministisch (Korrelationen, Heuristiken). Nur `api_post_stream`/`api_ai` rufen MiniMax. Nicht verwechseln, wenn „die KI sagt …“ debuggt wird.
- **Demo-Daten dürfen nie mit echten gemischt werden:** `demo_data.py` ist eine eigene Antwortquelle fürs öffentliche Demo — Änderungen an den v2-Antwortformen müssen hier nachgezogen werden, sonst bricht das Demo.
- **Internal-Home ist gecacht:** Die KPIs der Startseite werden zwischengespeichert (Stunden-Cache); der Live-Status wird separat/öfter abgefragt. Beim Ändern beide Pfade bedenken.
- **Admin-Query-Konsole ist read-only:** `_run_admin_readonly_query` erlaubt nur lesende Queries — kein Schreibpfad über die Admin-DB-Konsole.

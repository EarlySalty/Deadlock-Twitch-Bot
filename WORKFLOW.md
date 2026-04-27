## Viewer Presence Tracking

- 2026-04-04: Aufgabe aufgenommen. Relevante Analytics-/Dashboard-Muster geprüft, fehlende `WORKFLOW.md` angelegt.
- 2026-04-04: Presence-Ticks in Schema und `collect_chatters_data` ergänzt. Neues `api_viewer_timeline.py` mit Session- und Profil-Endpoint eingebaut, `api_v2.py` für Pfad-Streamer/Plan-Gating erweitert.
- 2026-04-04: Dashboard ergänzt: neue Viewer-Timeline-Fetcher/Hooks/Typen, neue `ViewerTimeline.tsx`-Seite, Session-Detail-Tabs erweitert, bestehende Overview-Timeline auf `useViewerCountTimeline` umgestellt.
- 2026-04-04: Verifikation: `python3 -m py_compile ...` erfolgreich, `./node_modules/.bin/tsc -b` erfolgreich, `npm run build` erfolgreich.

## Dashboard Polish · Landing-Redesign + Spotlight-Onboarding

- Ziel: Landing-Page `/twitch/dashboard` auf Command-Center-Layout (Sidebar + Main) umbauen, `WelcomeTour` von zentralem Modal auf nicht-blockierende Spotlight-Tour für die Landing-Page umstellen.
- Stand 2026-04-16: Visuelles Brainstorming abgeschlossen. User-Wahl:
  - Landing-Layout: **C · Command Center** (Sidebar 200px links, Main-Area rechts; Profil + Nav-Sections "Main"/"Tools"; Topbar; 3-col Row mit Health + Stream; 4-col Week-KPIs; 2-col Updates/Aktionen).
  - Onboarding: **B · Spotlight-Tour** (Dashboard bleibt sichtbar, einzelne Bereiche per Ring + Popover, Skip/Weiter). Nur für Landing, nicht für Analyse-Dashboard.
- Entscheidungen:
  - Farbpalette/Tokens in `src/index.css` bleiben. Manrope/Sora bleibt.
  - Onboarding-Persistenz weiter über `localStorage` (Key `welcome-tour-dismissed`).
  - Admin-Streamer-Switch bleibt oben rechts erreichbar (in Topbar oder Sidebar-Footer).
- Betroffene Dateien:
  - `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx` — Layout-Refactor auf Sidebar+Main.
  - `bot/dashboard_v2/src/components/onboarding/WelcomeTour.tsx` — von Modal zu Spotlight-Komponente.
  - ggf. neue Tour-Steps auf echte Landing-Bereiche verlinken via `data-tour-id`.
- 2026-04-16: Implementierung an parallele GPT-Worker delegiert (Landing + Spotlight-Tour). User: "Bau das erstmal so ein, Änderungen machen wir später."
- 2026-04-16: Worker 1: `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx` auf Command-Center-Layout umgebaut (Sidebar/Main, mobile Nav-Row, Tour-Anker `tour-nav|health|stream|week`, bestehende Queries/Action-Log/Changelog beibehalten).
- 2026-04-16: Worker 2 hat `WelcomeTour.tsx` auf Spotlight-Overlay mit `data-tour-id`-Ankern, animiertem Ring, Popover-Navigation, Resize/Scroll-Repositioning und LocalStorage-Persistenz umgestellt.
- 2026-04-16: Verifikation Worker 2: gezielter TypeScript-Check für `src/components/onboarding/WelcomeTour.tsx` erfolgreich. Gesamt-`./node_modules/.bin/tsc -b` und `npm run build` derzeit durch fremden Typfehler in `src/pages/InternalHomeLanding.tsx:683` blockiert (`item.active` fehlt auf Teilen des Union-Typs).
- 2026-04-16: Abschluss Worker 1: `src/pages/InternalHomeLanding.tsx` Typfehler bereinigt; `./node_modules/.bin/tsc -b` und `npm run build` im `bot/dashboard_v2` erfolgreich. Build lief mit bestehender Node/Vite-Hinweismeldung (Node 18.19.1 vs. empfohlen 20.19+), aber ohne Build-Abbruch.

## CI Workflow · Ruff + mypy

- 2026-04-19: `.github/workflows/lint-and-typecheck.yml` angepasst: Ruff von kuratierten Einzeldateien auf `bot/`, `twitch_cog/`, `tests/`, `scripts/` erweitert, SARIF-Export/Upload ergänzt, neuer `mypy`-Job vor `pytest` eingefügt; bestehende SHA-Pins unverändert gelassen.

## Local Check Script

- 2026-04-19: Aufgabe aufgenommen, bestehende `WORKFLOW.md` geprüft und neues lokales Prüfskript für Lint/Format/Typing/SAST/Tests unter `scripts/check-local.sh` angelegt.

## Security Fortress Workflow

- 2026-04-19: Parallel Worker 2 bearbeitet `.github/workflows/security-fortress.yml`.
- 2026-04-19: Bandit scannt jetzt `twitch_cog/` als Verzeichnis, erzeugt zusätzlich `bandit.sarif` samt SARIF-Upload, und Semgrep lädt `p/default` plus `p/python`. Bestehende SHA-Pins unverändert gelassen.

## Stream-Titel-Generator

- Spec: `docs/superpowers/specs/2026-04-19-stream-title-generator-design.md`
- Plan: `docs/superpowers/plans/2026-04-19-stream-title-generator.md`
- Arbeitsteilung: Claude = UI (React/TS) + Review; GPT = Backend
- 2026-04-19: Wave 1 gestartet (parallel): Task 1 DB-Migration, Task 3 steam_lookup, Task 4 title_ai
- 2026-04-19: Parallel Worker 1: `bot/migrations/title_generator_schema.sql` mit Tabellen `title_generator_knowledge` und `title_generator_insights` angelegt; Migration gegen `TWITCH_ANALYTICS_DSN` erfolgreich angewendet und Tabellenbestand verifiziert (`count=2`). `psql` fehlt lokal, daher Ausführung/Prüfung direkt via `psycopg` gegen denselben DSN.
- 2026-04-19: Parallel Worker 2: `bot/title_generator/steam_lookup.py` und `tests/title_generator/test_steam_lookup.py` angelegt; gezielter Pytest-Lauf folgt.
- 2026-04-19: Parallel Worker 2 Verifikation: `tests/title_generator/test_steam_lookup.py` grün (`4 passed`) via `.venv/bin/python -m pytest ...`; `python` fehlte im PATH und `aiosqlite` sowie `pytest-asyncio` mussten lokal in `.venv` installiert werden.
- 2026-04-19: Parallel Worker 3: `bot/title_generator/title_ai.py` mit MiniMax-Anbindung, Prompt/Response-Parsing und per-Streamer-Rate-Limiting ergänzt; `tests/title_generator/test_title_ai.py` deckt Limits, Prompt und `generate_title()` via Mock ab.
- 2026-04-19: Parallel Worker 3 Verifikation: `tests/title_generator/test_title_ai.py` grün (`9 passed`) via `.venv/bin/python -m pytest ...`; `python` fehlt lokal im PATH, daher nicht mit bare `python -m pytest` ausführbar.
- Wave 1 Status: laufend
- Wave 2: Task 2 title_db.py (nach Wave 1)
- Wave 3: Tasks 5,6,7,8 (nach Wave 2)
- Wave 4: Task 9 React-Tab (Claude) + Task 10 Job-Startup (GPT)
- 2026-04-19: Wave 2 Task 2 gestartet: `bot/title_generator/title_db.py` sowie `tests/title_generator/conftest.py` und `tests/title_generator/test_title_db.py` im bestehenden Postgres-Storage-Muster ergänzt; gezielter Pytest-Lauf folgt.
- 2026-04-19: Wave 2 Task 2 verifiziert: `.venv/bin/python -m pytest tests/title_generator/test_title_db.py -v` erfolgreich mit `9 passed`; Abweichung zur Erwartung `8 passed`, da die vorgegebene Testdatei tatsächlich 9 Tests enthält.
- 2026-04-19: Parallel Worker 3 (Dashboard-Routen): `bot/dashboard/routes_title.py` mit `/twitch/api/v2/title/suggest` und `/twitch/api/v2/title/insights` angelegt; `bot/dashboard/routes_mixin.py` registriert die neue Route-Gruppe.
- 2026-04-19: Parallel Worker 2: `bot/title_generator/insight_job.py` für den wöchentlichen MiniMax-Insight-Job angelegt; Import-Verifikation via `.venv/bin/python -c "from bot.title_generator.insight_job import run_insight_job; print('OK')"` folgt.
- 2026-04-19: Parallel Worker 2 Verifikation: Import von `run_insight_job` aus `bot/title_generator/insight_job.py` erfolgreich, Ausgabe `OK`.
- 2026-04-19: Parallel Worker 1 startet Task `knowledge_job.py` für nächtliche Knowledge-Population; Ziel ist ein reiner Import-/Startup-fähiger Async-Job auf Basis von `title_db.py` und Postgres-Storage.
- 2026-04-19: Parallel Worker 1 abgeschlossen: `bot/title_generator/knowledge_job.py` exakt angelegt; Import-Verifikation via `.venv/bin/python -c "from bot.title_generator.knowledge_job import run_knowledge_job; print('OK')"` erfolgreich (`OK`).
- 2026-04-19: Parallel Worker 4 ergänzt in `bot/chat/commands.py` den Twitch-Chat-Command `!title`/`!titel` mit lazy Imports aus `bot.title_generator.*`, Streamer-Lookup via `readonly_connection()` und Rate-Limit-/Fehlerbehandlung; Syntax-Check folgt.

## Minimax Post-Stream Chat-Analyse + Übersicht-Integration

- Ziel: Nach jedem Stream automatisch via Minimax Chat-Wortgruppen kategorisieren, Post-Stream-Report (gut/schlecht/Änderungen/Empfehlungen) erstellen, im Übersicht-Tab anzeigen – nur für Plan-User
- Status: 🔄 GPT-Worker (Backend + Frontend) gestartet
- 2026-04-21: Plan erstellt und genehmigt. Parallel-Worker gestartet.
- Backend: DB-Migration (2 neue Tabellen), `api_post_stream.py`, EventSub-Trigger in `eventsub_mixin.py`, API-Endpunkt in `api_v2.py`
- Frontend: Typen + FeatureId, `fetchStreamReport`, `useStreamReport`, `PostStreamReportCard.tsx`, Integration in `Overview.tsx`
- Kritische Dateien: `bot/analytics/api_ai.py`, `bot/monitoring/eventsub_mixin.py:1883-1891`, `bot/analytics/api_v2.py`, `bot/dashboard_v2/src/pages/Overview.tsx`
- 2026-04-21: Parallel Worker 1 (Backend) umgesetzt: Schema in `bot/migrations/twitch_analytics_schema.sql` erweitert, neues `bot/analytics/api_post_stream.py` fuer KI-Wortgruppen + Report + API-Mixin angelegt, Offline-Trigger in `bot/monitoring/eventsub_mixin.py` eingebaut, v2-Mixin in `bot/analytics/api_v2.py` verdrahtet und Route in `bot/analytics/api_overview.py` registriert.
- 2026-04-21: Parallel Worker 1 verifiziert: `.venv/bin/python -m py_compile bot/analytics/api_post_stream.py bot/monitoring/eventsub_mixin.py bot/analytics/api_v2.py bot/analytics/api_overview.py` erfolgreich, Import von `trigger_post_stream_analysis` erfolgreich (`OK`), DB-Migration per `psycopg` gegen `TWITCH_ANALYTICS_DSN` ausgefuehrt und Tabellen `twitch_chat_word_groups` sowie `twitch_stream_ai_reports` vorhanden.
- 2026-04-21: Parallel Worker 2 (Frontend) abgeschlossen: `src/types/billing.ts` um `post_stream_report` erweitert, `src/types/analytics.ts` um `StreamReport`-Typen ergänzt, `src/api/analytics.ts` + `src/hooks/useAnalytics.ts` um Stream-Report-Fetch/Hook erweitert, neue Card `src/components/cards/PostStreamReportCard.tsx` angelegt und in `src/pages/Overview.tsx` eingebunden. Verifikation per `./node_modules/.bin/tsc -b` und `npm run build` folgt.

## Dashboard: Thematische Neuordnung der Tab-Inhalte

- Ziel: Sektionen innerhalb bestehender Tabs neu ordnen, thematisch zusammengehörige Inhalte zusammenbringen, rohe Bereiche ausbauen
- Status: ✅ Abgeschlossen (2026-04-22)
- Betroffene Dateien: `chatAnalyticsContent.tsx`, `useChatAnalyticsPage.ts`, `ChatAnalytics.tsx`, `Audience.tsx`, `Growth.tsx`, `Schedule.tsx`
- Entscheidungen:
  - ViewerProfiles aus Chat → Audience (nach Demographics/Lurker-Sektion)
  - Wochentags-Analyse + Schedule-Empfehlungen aus Growth → Schedule
  - Mock-Daten in Growth (TagPerformance) entfernen → NoDataCard
  - Audience Insights auf 4 Cards ausbauen (Watch Time, Funnel, Lurker, Demographics)
  - Generische Hardcoded-InsightCards in Schedule durch datengetriebene ersetzen (aus weeklyData via generateScheduleInsights)
  - ViewerProfiles-Hook: `useViewerProfiles` aus `@/hooks/useAnalytics` (bereits in useChatAnalyticsPage.ts vorhanden)
- Hook-Infos: `useViewerProfiles(streamer, days)` → viewerProfilesData; `useLurkerAnalysis(streamer, days)` → lurkerData (bereits in Audience)
- 2026-04-22: Umsetzung gestartet: `ViewerProfiles` aus Chat entfernt, in `Audience.tsx` als eigene Segment-Sektion ergänzt und Audience-Insights anhand des realen Lurker-Typs (`lurkerStats.ratio`) erweitert.
- 2026-04-22: `Growth.tsx` bereinigt: Wochentags-/Schedule-Blöcke sowie Tag/Title-Mockdaten entfernt; dieselbe WeekdayCards-/Insight-Logik nach `Schedule.tsx` verschoben und dort die Empfehlungen datengetrieben umgebaut.
- 2026-04-22: Verifikation erfolgreich: `cd bot/dashboard_v2 && ./node_modules/.bin/tsc -b` ohne Fehler; `npm run build` erfolgreich. Hinweis: Vite meldet lokal weiter die bekannte Node-Warnung zu `18.19.1` statt `20.19+`, der Build lief dennoch durch.

## Stripe Checkout Diagnose

- Ziel: Stripe Checkout Diagnose
- Status: In Arbeit
- 2026-04-21: `bot/dashboard/billing/billing_mixin.py`, `bot/dashboard/abbo_billing_routes.py`, `bot/dashboard/abbo_routes.py`, `bot/dashboard/billing/billing_plans.py` und `bot/dashboard/server_v2.py` gelesen; Stripe-Readiness und Redirect-Pfade nachvollzogen.
- Gefundene Probleme:
  - `build_billing_catalog()` setzt `payment.integration_state="planned"` und `payment.checkout_enabled=False` hart kodiert, auch wenn die echte Readiness bereits separat berechnet wird.
  - `/twitch/abbo` rendert Bezahlen-/Checkout-Aktionen aktuell ohne Bindung an Stripe-Readiness oder Price-Mapping; Nutzer koennen daher in bekannte Fehlerpfade laufen.
  - `abbo_pay()` redirectet bei `checkout_ready=False`, `price_map_ready=False` oder fehlender `stripe_price_id` aktuell ohne `reason`-Parameter zurueck; dadurch erscheint nur die generische Meldung.
  - In dieser lokalen Laufzeit sind `STRIPE_SECRET_KEY`, `STRIPE_PUBLISHABLE_KEY`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_CHECKOUT_SUCCESS_URL`, `STRIPE_CHECKOUT_CANCEL_URL`, `STRIPE_PRICE_ID_MAP`, `TWITCH_BILLING_STRIPE_SECRET_KEY` und `TWITCH_BILLING_STRIPE_PUBLISHABLE_KEY` im Env nicht gesetzt; `STRIPE_SECRET_KEY`, `STRIPE_PUBLISHABLE_KEY` und `STRIPE_WEBHOOK_SECRET` sind auch im Keyring-Service `DeadlockBot` nicht gesetzt.
- Empfohlene Fixes:
  - `build_billing_catalog()` mit optionalem Readiness-Payload verdrahten und `integration_state`/`checkout_enabled` daraus ableiten.
  - `/twitch/abbo` serverseitig an Readiness und Price-IDs koppeln, damit nicht-bereite Checkout-Aktionen gar nicht erst angeboten werden.
  - `abbo_pay()` fuer nicht-bereiten Checkout und fehlende Price-IDs auf explizite `reason`-Codes umstellen, damit die UI differenzierte Hinweise anzeigen kann.
  - Extern noch noetig: echte Stripe-Credentials und `STRIPE_PRICE_ID_MAP` bzw. Alias-Keys ueber Env oder Keyring hinterlegen.
- 2026-04-21: Implementiert ohne externe Credentials:
  - `bot/dashboard/billing/billing_plans.py`: Katalog akzeptiert jetzt optionales Stripe-Readiness-Payload und leitet `payment.integration_state` sowie `payment.checkout_enabled` dynamisch daraus ab.
  - `bot/dashboard/abbo_routes.py`: `/twitch/abbo` koppelt Pay-Aktionen jetzt an Readiness plus vorhandene Stripe Price IDs; nicht-bereiter Checkout wird als gesperrt angezeigt statt blind verlinkt.
  - `bot/dashboard/abbo_billing_routes.py`: fruehe Redirects liefern jetzt explizite Ursachen (`checkout_not_ready`, `stripe_price_id_map_missing`, `missing_stripe_price_id`).
- 2026-04-21: Verifikation:
  - `python3 -m py_compile bot/dashboard/billing/billing_plans.py bot/dashboard/abbo_billing_routes.py bot/dashboard/abbo_routes.py bot/dashboard/routes_billing.py` erfolgreich.
  - `python3 -m unittest tests.test_billing_helpers.BillingHelperTests.test_catalog_payment_state_uses_readiness_payload` erfolgreich.
  - `python3 -m unittest tests.test_dashboard_lurker_tax_settings.DashboardLurkerTaxTests.test_abbo_entry_shows_locked_teaser_for_free_plan tests.test_dashboard_lurker_tax_settings.DashboardLurkerTaxTests.test_abbo_entry_shows_toggle_and_scope_warning_for_paid_plan` erfolgreich.
  - Voller Lauf `python3 -m unittest tests.test_billing_helpers` bleibt durch bestehende, nicht von diesem Patch verursachte Erwartung bei Entitlements rot (`analytics.ai_mini` zusaetzlich vorhanden).
- Relevante Dateipfade:
  - `bot/dashboard/billing/billing_mixin.py`
  - `bot/dashboard/abbo_billing_routes.py`
  - `bot/dashboard/abbo_routes.py`
  - `bot/dashboard/billing/billing_plans.py`
  - `bot/dashboard/server_v2.py`

## Security Code Review

- 2026-04-24: Security-Review fuer `bot/dashboard/`, `bot/api/`, `bot/internal_api/`, `bot/runtime_security.py` und `bot/secret_store.py` gestartet; Auth-, Session-, Template-, SQL-, Dateisystem- und subprocess-Pfade gelesen.
- 2026-04-24: Hochsichere Findings bestaetigt:
  - IDOR auf `twitch/api/v2/session/{id}` und `twitch/api/v2/session/{id}/events`: nur allgemeine Auth, keine Eigentuemerpruefung gegen die angeforderte Stream-Session.
  - Breiter Cross-Streamer-Auth-Bypass in mehreren Analytics-v2-Endpunkten mit `streamer`-Parameter: Partner-Session darf fremde Streamer-Daten abfragen; Plan-Gating prueft Ziel-Plan statt Session-Bindung.

## Backend internal-home parallelisieren

- 2026-04-26: Aufgabe aufgenommen. `bot/analytics/api_v2.py` und `bot/storage/pg.py` gelesen; `readonly_connection()` bestaetigt als reentrant ueber Prozess-Pool, aber Repo-Default `TWITCH_ANALYTICS_POOL_MAXSIZE=4` ist fuer Voll-Parallelisierung zu klein.
- 2026-04-26: `_build_internal_home_payload` auf async umgebaut: Identity-Block laeuft zuerst, danach Fallback-Parallelisierung mit `asyncio.gather()`/`asyncio.to_thread()` fuer Health-Score, Week-Comparison, Autoban-Events, Service-Warnings und Changelog; restliche Core-Aggregate bleiben gebuendelt sequenziell auf einer Readonly-Connection.
- 2026-04-26: Fehlerpfad gehaertet: Gather-Subblocks loggen Exceptions und fallen auf bestehende Defaults zurueck; API-Caller awaited jetzt den Payload-Build und laedt den Changelog parallel.

## Social Media Phase 0 - Stabilisierung

- 2026-04-26: Aufgabe aufgenommen. `bot/social_media/oauth_manager.py`, `bot/social_media/dashboard.py`, `bot/social_media/credential_manager.py`, `bot/social_media/token_refresh_worker.py`, `bot/storage/pg.py` und bestehende Social-Media-/Storage-Tests gelesen; verbotene Dashboard-Route-Migrationsdateien nicht angeruehrt.
- 2026-04-26: Diagnose festgezogen:
  - OAuth-Callback behandelt Provider-Exchange-Fehler aktuell genauso wie State-Fehler (`ValueError`), wodurch Redirect-/PKCE-Probleme als "ungueltiger Callback/State" erscheinen.
  - Social-Media-OAuth verwendet einen generischen Callback-Pfad ohne Plattform-Bindung; Redirect-URI wird ohne explizite Callback-Pfadbindung gespeichert.
  - Token-Refresh-Worker loggt harte Refresh-Fehler nur, ohne Discord-DM oder 24h-Drosselung.
  - Sequence-Drift ist fuer mehrere Social-Media-Tabellen moeglich, nicht nur `twitch_clips_social_media`.
- 2026-04-26: Umsetzung gestartet:
  - Neues Helper-Modul `bot/social_media/storage.py` fuer Phase-0-DDL (OAuth-State-Haertung mit `consumed_at`/`TIMESTAMPTZ`, Tabelle `social_media_reauth_notifications`, Sequence-Repair fuer Social-Media-Tabellen).
  - Neue Migration `bot/migrations/social_media_phase0_stabilization.py`.
  - `bot/social_media/oauth_manager.py` haertet State-Consume plattform-/redirect-gebunden und trennt State-, Exchange- und Refresh-Fehler per eigenen Exception-Typen.
  - `bot/social_media/dashboard.py` verwendet plattformspezifische Callback-URIs `/social-media/oauth/callback/{platform}` und priorisiert konfigurierte Public-Origin-Werte vor localhost-Fallback.
  - `bot/social_media/token_refresh_worker.py` verschickt bei nicht-transienten Refresh-Fehlern eine gedrosselte Admin-DM (24h pro Streamer×Plattform×Fehlerart).
- 2026-04-26: Verifikation laeuft: Python-Syntaxcheck fuer die geaenderten Social-Media-Dateien und die neue Migration erfolgreich; Postgres-Regressionstests werden als Naechstes ausgefuehrt.

## Social Media Phase 1 - Backend (Layout, Upload, Retention)

- Status: ✅ Abgeschlossen (2026-04-26)
- 2026-04-26: Phase-1-Backend umgesetzt in `bot/social_media/` und `bot/migrations/`: Streamer-Layout-Storage mit Clip-Override/Auto-Apply, Admin-API fuer Layouts und Pending-Clips, MP4-Upload fuer manuelle Clips, Retention-Helper/Cog sowie FFmpeg-Komposition aus Layout-JSON.
- 2026-04-26: Verifikation erfolgreich: `py_compile` fuer die geaenderten Social-Media-Module/Migration, neue Phase-1-Tests (`tests/test_social_media_phase1_backend.py`, `tests/test_social_media_phase1_video.py`) sowie bestehende Social-Media-Regressionssuites (`phase0`, `dashboard_rendering`, `auth_regressions`) gruen; optionaler Real-FFmpeg-Test in `test_social_media_phase1_video.py` lokal nur bei vorhandenem `ffmpeg/ffprobe`.

## Social Media Phase 4 - Approval + Cross-Posting

- 2026-04-27: Aufgabe aufgenommen. `WORKFLOW.md`, `bot/social_media/{storage,clip_manager,enrichment,upload_worker,dashboard,settings}.py`, `bot/runtime_{bootstrap,bot_runtime}.py`, bestehende Upload-Module, Frontend-Dateien unter `bot/dashboard_v2/src/` und relevante Social-Media-Tests gelesen. Phase-3-Konfliktfelder (`analytics`, Report-Routen, Phase-3-Migrationen) bewusst ausgespart.
- 2026-04-27: Implementiert: Phase-4-Migration `bot/migrations/social_media_phase4_approval.py`, Storage-Helfer `apply_phase4_approval`, Approval-Service + Discord-View/Cog unter `bot/social_media/approval*/`, Upload-Gate per Approval, neue Admin-API fuer Approval/Auto-Approve und Runtime-Verdrahtung fuer den Approval-Worker.
- 2026-04-27: Frontend `bot/dashboard_v2/src/pages/SocialMedia.tsx` um Approval-Aktionen pro Clip sowie Auto-Approve-Toggles erweitert; API-/Typdefinitionen fuer Approval/Settings nachgezogen.
- 2026-04-27: Verifikation erfolgreich: `.venv/bin/python -m pytest tests/test_social_media_phase0_stabilization.py tests/test_social_media_phase1_backend.py tests/test_social_media_phase1_video.py tests/test_social_media_phase2_backend.py tests/test_social_media_phase4_backend.py tests/test_social_media_dashboard_rendering.py tests/test_social_media_auth_regressions.py -q` -> `61 passed`; `cd bot/dashboard_v2 && npx tsc --noEmit` erfolgreich; `cd bot/dashboard_v2 && npx vite build` erfolgreich.

## Social Media Phase 3 - Analytics + Reports

- 2026-04-27: Aufgabe aufgenommen. Relevante Phase-2-/Runtime-/Dashboard-/Uploader-Dateien sowie bestehende Worker- und Testmuster gelesen; vorhandene Analytics-Tabelle und Admin-Sperren geprüft.
- 2026-04-27: Phase 3 umgesetzt: Migration `social_media_phase3_analytics.py`, Analytics-Storage/Worker, LLM-Report-Writer + Admin-DM-Dispatcher, neue Admin-API-Endpunkte und Analytics-Tab im React-Dashboard.
- 2026-04-27: Verifikation erfolgreich: `.venv/bin/python -m pytest tests/test_social_media_phase3_backend.py -q`, bestehende Social-Media-Regressionen (`phase0`, `phase1_backend`, `phase2_backend`, `dashboard_rendering`, `auth_regressions`) sowie `bot/dashboard_v2` via `npx tsc --noEmit` und `npx vite build` gruen.

# Evidence: KI-Hilfe-Assistent im Streamer-Dashboard

status: aktiv
datum: 2026-09-04
stand: HEAD 1540aebb (origin/main), Worktree feat/dashboard-assistent

Jede Zeile eine echte Fundstelle. Beobachtung, keine Hypothese.

## Vorlage und Bestand

- website/src/components/layout/SiteChatbot.tsx:5  Landing-Widget ruft `POST /twitch/api/v2/self-explainer/ask`, Body `{question, history}`, bevorzugt `parts` vor `answer`, zeigt `sources` als Quellenzeile (:87, :102, :195).
- rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:616  `pub async fn self_explainer_ask(State(pool), connect, body: String)`, öffentlich, Peer-IP-Rate-Limit 10/60 s (:47-48, :437), DB-Log per tokio::spawn mit 3 s Timeout (:688).
- rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:388  `async fn answer_question(kb, history, question) -> SelfExplainerAnswer` ist privat; ebenso `fireworks_generate` (:327), `build_system_prompt` (:92), `looks_like_injection` (:122), `parse_history` (:128), `split_message` (:205), `evaluate_answer` (:272), `knowledge_base()` (:308), `RateLimiter` (:437).
- rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:934  Test `fireworks_fehlerpfade_werden_geloggt` liest den eigenen Quelltext und erwartet `fireworks_generate` vor `answer_question` in derselben Datei; Verschieben bricht ihn, Sichtbarkeit heben nicht.
- rust/crates/tb-dashboard-api/src/lib.rs:103  Route des öffentlichen Endpoints im `build_public_router`; authentifizierte v2-Routen leben in `build_authed_router` (:162), Beispiel `.route("/twitch/api/v2/ai/chat", post(ai_chat::ai_chat_handler))` (:741).
- rust/crates/tb-dashboard-api/src/handlers/ai_chat.rs:130  Vorbild für authentifizierten POST mit JSON-Body: `auth: DashboardAuthLevel` zuerst, `body: String` zuletzt, 401 über `crate::auth::unauthorized_v2_response()` (:134), eigenes `serde_json::from_str` mit 400 (:138).
- rust/crates/tb-dashboard-api/src/handlers/ai_chat.rs:167  Plan-Gate 403 `plan_required`; der Hilfe-Assistent bekommt laut Contract kein Plan-Gate.

## Auth und Identität

- rust/crates/tb-dashboard-api/src/auth/level.rs:52  `DashboardAuthLevel::{Admin{actor}, Partner{twitch_login, twitch_user_id, display_name}, None}`; Extractor lehnt nie ab (Rejection Infallible, :293), Handler prüfen selbst.
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:66  `fn twitch_identitaet(auth) -> Result<(&str, &str), Response>` liefert (user_id, login) für Partner und Admin-Actor, 401/403 sonst.
- rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs:40  `fn identity(auth) -> Result<(String, String), IdentityError>` gleiche Logik, allokierend; `:55 scopes(pool, uid)` liest `twitch_raid_auth.scopes`.
- rust/crates/tb-dashboard-api/src/auth/csrf.rs:89  `csrf_protect` auf allen Write-Methoden; gültig bei korrektem `x-csrf-token` ODER same-origin plus gültiger DB-Session (Origin/Referer-Gate :105); Loopback-Bypass (:97).
- bot/dashboard_v2/src/api/auth.ts:23  Auth-Status liefert `csrfToken`/`csrf_token`; Seiten nutzen `authStatus?.csrfToken ?? authStatus?.csrf_token` (InternalHomeLanding.tsx:396).

## Wissen und LLM

- rust/crates/tb-knowledge/src/base.rs:83  `select(query, namespace, audience: Option<&str>, k)`; `None` = nur öffentlich, `Some("streamer")` lässt leere und passende Zielgruppe durch (:101).
- rust/crates/tb-knowledge/src/grounding.rs:11  `assemble_grounding(docs) -> Grounding{facts, sources}`, Format `## Titel\nBody`.
- rust/crates/tb-knowledge/src/doc.rs:17  `ist_oeffentlich` = `"" | "streamer" | "public" | "viewer"`.
- rust/knowledge/bot/  21 Docs, alle `audience: streamer`; rust/knowledge/deadlock/ 1 Platzhalter. `KNOWLEDGE_DIR` Default `rust/knowledge` (self_explainer.rs:301); das Release kopiert rust/knowledge mit (ops/systemd/install-twitch-release.sh:123).
- rust/crates/tb-llm/src/hub.rs:70  `Request{system, messages, max_tokens, temperature, json_object, timeout, total_deadline, ledger, strip_think, allow_reasoning_content, accept, retry_on_429, failover, endpoint}`; `complete(use_case, request)` (:279); Body kennt kein `tools` (:567-578), also kein Function-Calling.
- rust/crates/tb-llm/src/selection.rs:10  Default-Modell `accounts/fireworks/models/deepseek-v4-flash-0731`, `endpoint_for(use_case)` ignoriert Overrides (:26).

## Datenkarten (bestehende Lesefunktionen)

- rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:932  `access_state_block(pool, login, user_id) -> AccessState` (Partner-Status, operational_state, technical_pause_reason), privat.
- rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:1149  `oauth_block(pool, login, user_id) -> OauthData` plus `scope_snapshot` (:1204) für granted/missing scopes, privat.
- rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:1261  `kpis_recent_block(pool, login, since) -> KpisData{streams_count, avg_viewers, follower_delta, recent_streams}`, privat.
- rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:1452  `ban_events_block(pool, user_id, since) -> BanData`; `:1563 raid_events_block(pool, login, user_id, since) -> Vec<Value>`; `:1382 last_stream_summary`, alle privat.
- rust/crates/tb-dashboard-api/src/handlers/moderation_settings.rs:94  GET liest `twitch_moderation_settings` per `channel_user_id`, Defaults alle true (:120), SQL liegt inline im Handler.
- rust/crates/tb-dashboard-api/src/handlers/scam_guard_settings.rs:71  GET liest `twitch_scam_guard_settings` per `channel_login`, Defaults true/auto_ban/0.90/0.70 (:98), SQL inline im Handler.
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:249  `live_status(pool, streamer_id)`; `:276 pub fn verbindungs_status(hat_tokens, needs_reauth, scopes)`; `:296 verbindungen_lesen`.
- rust/crates/tb-dashboard-api/src/handlers/raids.rs:39  `recent_raids_handler` ist öffentlich und ungefiltert, für Personalisierung ungeeignet.

## Protokoll und Migration

- rust/crates/tb-analytics/src/self_explainer_log.rs:41  `insert(pool, question, answer, grounded, flagged_injection, peer)`; Tabelle hat keine Spalten für Twitch-User-ID oder Seite (:12-21).
- rust/crates/tb-db/src/migrate.rs:35  `sqlx::migrate!("../../migrations")` = rust/migrations; jüngste Datei 20260903090000_twitch_moderation_settings.sql.

## Frontend

- bot/dashboard_v2/src/App.tsx:414-439  `QueryClientProvider > LanguageProvider > ErrorBoundary > Routen-Ternär`; ein globales Widget gehört als Geschwister der Ternär-Ausgabe in den LanguageProvider.
- bot/dashboard_v2/src/preview/routes.ts:4-11  Routen-Konstanten (`/twitch/dashboard`, `/twitch/verwaltung`, `/twitch/uplink`, `/twitch/pricing`, Overlay, Analyse); `tabAliases.ts:28 resolveTabParam`.
- bot/dashboard_v2/src/api/core.ts:88  `buildApiUrl`, `:81 withCookieCredentials`, `:125 fetchJson` (401 leitet zum Login); `api/ai.ts:36 fetchAIChat` als POST-Vorbild mit 429-Sonderfehler (:4) und 240 s Timeout (:16).
- bot/dashboard_v2/src/context/LanguageContext.tsx:69  `useLanguage()`, `:74 useT()`; Wörterbuch `i18n/dictionary.ts` (deutscher Text ist Schlüssel, en-Übersetzung daneben).
- bot/dashboard_v2/src/ddc-design-tokens.css:22-40  Gold `--color-primary #C5A059`, Hover `#F1D299`, Soft `rgba(197,160,89,0.20)`, Flächen `--color-bg #140D0A`, `--color-card #1F1815`.
- bot/dashboard_v2/tests/brandPalette.test.ts:19  Hex-Erlaubnisliste über src; Fremdfarben fallen durch.
- bot/dashboard_v2/package.json:9  `test` listet jede Testdatei einzeln; neue Tests dort eintragen.
- bot/dashboard_v2/vite.config.ts:17-19  `base: /twitch/dashboard-v2/`, `outDir: ../analytics/dashboard_v2/dist`; spa.rs:39 liest daraus.

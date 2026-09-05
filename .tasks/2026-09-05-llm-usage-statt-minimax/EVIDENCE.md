# Evidence (origin/main 34b25ac6, 2026-09-05)

## Connector-Stand: alles läuft schon über tb-llm

- rust/crates/tb-llm/src/selection.rs:4 "Fireworks. Frühere Provider- und Modell-Overrides werden bewusst ignoriert."; :53 `provider: "fireworks"`, :56 `crate::keys::fireworks_api_key()`
- rust/crates/tb-llm/src/hub.rs:828-835 Test: Endpunkt `provider: "minimax"` / `MiniMax-M3` wird im Hub abgewiesen
- rust/crates/tb-social-media/src/llm_dispatch.rs: 12 `tb_llm::`-Referenzen, 0 eigene `.post(`
- rust/crates/tb-chat/src/crew_guard.rs: 16 `tb_llm::`-Referenzen, 0 eigene `.post(`
- rust/crates/tb-chat/src/claude_chat.rs existiert nicht mehr
- Kein Produktivcode außerhalb tb-llm ruft `chat/completions`, `api.openai.com`, `api.minimax*`, `api.anthropic.com` (Suche über rust/, alle Treffer sind wiremock-Pfade in Tests)
- Einziger eigener HTTP-Client mit KI-Bezug: rust/crates/tb-engagement/src/transcribe.rs:27 `DEFAULT_STT_URL = "http://127.0.0.1:8791/v1/audio/transcriptions"` (lokaler STT-Server, Nicht-Ziel)

## Ledger-Tabelle

- Live-DB twitch_analytics: `minimax_usage`, 3119 Zeilen, einziger Schreiber `source = 'twitch-bot'` (letzte 7 Tage 676 Zeilen, keine andere Quelle). Grants: postgres alle; twitchbot, twitchdash, twitchlegacy SELECT/INSERT/UPDATE/DELETE. Keine Views hängen daran.
- rust/migrations/20260703000000_minimax_usage_ledger.sql:8 `CREATE TABLE IF NOT EXISTS public.minimax_usage`, :21-22 Indizes `idx_mmu_ts`, `idx_mmu_source`
- rust/crates/tb-llm/src/ledger.rs:177 `INSERT INTO minimax_usage`, :245 `FROM minimax_usage`, :338 Test-DDL, :379/:406/:439 Test-Abfragen; :49 `const ENV_BUDGET: &str = "MINIMAX_5H_TOKEN_BUDGET"`, :84 `fn budget_from_env()`
- rust/crates/tb-llm/sqlx/schema.sql:7,19,20
- rust/crates/tb-llm/Cargo.toml:9,18 Kommentare
- rust/crates/tb-engagement/tests/ledger_side_effects.rs:82,106,129 `public.minimax_usage`
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt:173-182 Spalten von minimax_usage
- Andere Repos: TradingBot `rust/crates/tb-ai/src/llm/usage_ledger.rs:40` legt eine eigene Tabelle gleichen Namens in eigener DB an; Deadlock-Bots nur Doku-Erwähnung; Python-Helfer `~/Documents/.claude/minimax-usage/minimax_usage.py` nutzt SQLite (`datetime('now', ...)`). Kein fremder Schreiber in twitch_analytics.

## Bezeichner (92 Dateien mit "minimax" in rust/ und bot/, davon 4 Testdateien, 3 Migrationen)

Treffer je Datei (Produktivcode, Top): tb-llm/src/ledger.rs 38, tb-engagement/src/pipeline.rs 30, tb-engagement/src/minimax_chat.rs 27, tb-chat/src/conversation_scam.rs 23, tb-engagement/src/background.rs 21, tb-chat/src/title_ai.rs 21, tb-analytics/src/post_stream.rs 20, tb-dashboard-api/src/handlers/chat_deep_minimax.rs 18, tb-bot/src/chat_wiring.rs 13, tb-chat/src/pipeline.rs 12, tb-engagement/src/global_sentiment.rs 11, tb-chat/src/invite_question.rs 11, tb-engagement/src/threads.rs 10, tb-engagement/src/soul_store.rs 10, tb-dashboard-api/src/ai_state.rs 10, tb-analytics/src/ai_analysis.rs 10.

- rust/crates/tb-engagement/src/minimax_chat.rs:708-735 `EngagementMinimaxClient` (base_url-Override), :1058 Test prüft `fireworks.ai`
- rust/crates/tb-dashboard-api/src/lib.rs:727-728 Route `/twitch/api/v2/chat-deep-minimax` -> `chat_deep_minimax::chat_deep_minimax_handler`
- rust/crates/tb-dashboard-api/src/handlers/chat_deep_minimax.rs:21 `use tb_analytics::chat_deep_minimax::{...}`
- rust/crates/tb-dashboard-api/src/ai_state.rs:16 `MINIMAX_HOURLY_FOLLOW_UP_LIMIT`, :21 `AI_MODEL_MINIMAX: &str = "minimax"` (Wert wird als Modellschlüssel in Sitzungszustand geschrieben, siehe :202; REQ-4 prüfen)
- rust/crates/tb-llm/src/selection.rs:69-70,104-105,136 Legacy-ENV-Namen in Ignorier-Liste und Tests (bleiben laut REQ-3)
- /etc/caddy/Caddyfile enthält keinen Treffer für `chat-deep-minimax`; die Route liegt unter `/twitch/api/v2/` und braucht keinen Allowlist-Eintrag.

## Frontend (bot/dashboard_v2/src)

- api/ai.ts:81 `fetchChatMinimaxDeep`, :90 `fetchApi('/chat-deep-minimax', ...)`
- pages/chatAnalyticsDeepSections.tsx:28,592,608,635 `ChatMinimaxDeepSection`, Überschrift "MiniMax Chat-Analyse"
- pages/chatAnalyticsContent.tsx:23,374
- components/verwaltung/AIEngagementSection.tsx:104 "MiniMax-M3 liest deinen Chat mit und mischt sich situativ ein" (falsche Modellangabe, läuft Deepseek V4 Flash)
- components/socialmedia/EnrichmentPanel.tsx:225 Hinweis `MINIMAX_API_KEY`
- components/cards/PostStreamReportCard.tsx:255 `data.model === 'opus' ? 'Claude Opus' : 'Minimax'`
- pages/StreamReports.tsx:1108 `{data?.model || 'MiniMax'}`
- types/analytics.ts: 2 Treffer

## Toolchain und Tests

- Toolchain 1.97.1 unter `~/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin` (System-cargo 1.75 bricht)
- Baseline-Rot auf main (Test-Schema ohne Tabellen): `ad_manager_store::queue_lease_idempotenz_und_state_sind_atomar`, `ledger_side_effects::engagement_client_verbucht_usage_ins_zentrale_ledger`

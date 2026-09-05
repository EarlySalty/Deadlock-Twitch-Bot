# Evidence (origin/main fb62a7a5, 2026-09-06)

## Ledger llm_usage, letzte 7 Tage (purpose | Aufrufe | Ø tokens_out | max tokens_out)

- chat_message_type | 1484 | 385 | 1216 (vor Fix acd62f21 exakt 1216 = Budget, danach ~380)
- title-insight | 407 | 1413 | 1500 (Budget 1500, fast immer am Deckel)
- engagement | 41 | 206 | 739 (Chat, kein Budget, Nicht-Ziel)
- promo_pitch | 38 | 192 | 1300 (Judge JSON plus Pitch-Texte, gemeinsamer Use-Case)
- spam-review | 7 | 277 | 675 (Scam-Judge)
- dashboard-assistent | 2 | 330 | 338 (Nicht-Ziel)

## Vorbild (bereits live)

- rust/crates/tb-llm/src/hub.rs:177 `pub fn denken_aus(mut self) -> Self`
- rust/crates/tb-llm/src/hub.rs:586 `body["reasoning_effort"] = serde_json::json!("none");`
- rust/crates/tb-analytics/src/chat_typen.rs:352-357 Request mit `.max_tokens(...)`, `.json_object()`, `.denken_aus()`
- rust/crates/tb-analytics/src/chat_typen.rs:593-613 Test `body_schaltet_das_denken_ab` (wiremock, prüft `reasoning_effort` im Body)

## Zu ändernde Aufrufe

- rust/crates/tb-chat/src/title_ai.rs:515-521 `titel_completion(endpoint, use_case, prompt, temperature, max_tokens)`: `Request::prompt(prompt).temperature(..).max_tokens(max_tokens)`; :737-741 Aufruf `"title-insight"` mit 0.5 und 1500; Tests mit MockServer ab :794, :1020, :1140 (`generate_insight_with_parst_recommendations`)
- rust/crates/tb-chat/src/promo_pitch.rs:362-368 `FireworksPitchJudge::decide`: `Request::simple(PITCH_SYSTEM_PROMPT, user).temperature(0.0).json_object().timeout(PITCH_TIMEOUT)`; keine wiremock-Tests im Modul (0 Treffer MockServer), Tests für Pitch-Logik in rust/crates/tb-chat/tests/lfg_pitch.rs
- rust/crates/tb-chat/src/promo_pitch.rs:431-434, :440-443, :453 Pitch-Text-Builder mit Temperatur 0.7 ohne Budget (Nicht-Ziel)
- rust/crates/tb-chat/src/scam_pitch.rs:1939-1950 Judge: `Request::simple(..).max_tokens(i64::from(JUDGE_MAX_TOKENS)).temperature(0.0).timeout(20s)` plus `.allow_reasoning_content()` (:1950) und Kommentar zum Denktext (:1946 ff.)
- rust/crates/tb-chat/src/crew_guard.rs:553-561 `Request::simple(..).temperature(0.0).json_object().timeout(self.timeout)`
- rust/crates/tb-engagement/src/crew_review.rs:204-209 `Request::simple(REVIEW_SYSTEM_PROMPT, user_data).temperature(0.0).json_object().timeout(self.timeout)`
- rust/crates/tb-engagement/src/outreach_shadow.rs:231-236 `Request::simple(OUTREACH_SYSTEM_PROMPT, user_data).temperature(0.0).json_object().timeout(FIREWORKS_TIMEOUT)`
- rust/crates/tb-social-media/src/llm_dispatch.rs:113-121 `Request::simple(system, user).max_tokens(max_tokens).temperature(..)`, `json_object()` bedingt (:120)
- rust/bin/tb-stream-audit/src/main.rs:2885-2889 `Request::simple(llm::SYSTEM_PROMPT, llm::anfrage_json(stapel)).json_object().timeout(MODELL_ZEITGRENZE)`

## Nicht-Ziele (freie Text- oder Chatantworten)

- rust/crates/tb-engagement/src/llm_chat.rs:819, :853, :875, :965
- rust/crates/tb-dashboard-api/src/handlers/ai_chat.rs:95 (max_tokens 4000)
- rust/crates/tb-dashboard-api/src/handlers/ai_analysis.rs:144 (max_tokens 60000)
- rust/crates/tb-analytics/src/post_stream.rs:366 (16000), :375 (6000)

## Test-Umgebung

- Toolchain: `PATH=/home/nathanael/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:$PATH`, `SQLX_OFFLINE=1`
- DB-gatete Tests brauchen den Test-Container `tb-test-postgres` (127.0.0.1:33113, Passwort aus `rust/scripts/test_db.sh`, Variable PASS): `TB_TEST_DATABASE_URL=postgres://postgres:<PASS>@127.0.0.1:33113/postgres`. Docker nur per `sudo docker`. Nicht `tb_bb_test` auf dem lokalen Cluster nehmen (31 Verbindungsfehler in tb-db/tb-raid).
- Baseline-Rot: `ad_manager_store::queue_lease_idempotenz_und_state_sind_atomar` (fehlende Tabelle `twitch_raw_chat_ingest_health` im Test-Schema), `ledger_side_effects::engagement_client_verbucht_usage_ins_zentrale_ledger` (fehlende `public.llm_usage` in der Test-DB)

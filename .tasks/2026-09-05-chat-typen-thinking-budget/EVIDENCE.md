# Evidence

## Live-Befund (Release 7dcae83f, Bot-PID 2085980, 2026-09-05 16:52 UTC)

- Journal tb_bot::chat_typen_wiring, 5 Pakete in 60 s: `Modell-Paket fehlgeschlagen ... error=LLM-Antwort unbrauchbar: JSON nicht lesbar: EOF while parsing a value at line 1 column 35` (erstes Paket, abgeschnitten) und `... column 0` (vier Pakete, leer). Danach `zu viele Modellfehler in Folge, Lauf wird abgebrochen fehler_serie=5` und `Lauf abgeschlossen geladen=20000 regel=13600 modell=0 modell_aufrufe=5 offen=6400`.
- Ledger `minimax_usage`, purpose `chat_message_type`, ids 2771-2776: tokens_in 512-602, tokens_out exakt 1216 bei jedem Aufruf, success=1. 1216 = 40*24+256 = gesetztes max_tokens.
- Vergleich anderer Deepseek-Aufrufer (3 Tage): `engagement` Ø 166 tokens_out ohne max_tokens; `title-insight` Ø 1418 bei Deckel 1500 (gleiche Fehlerklasse, Nicht-Ziel).

## Code

- rust/crates/tb-analytics/src/chat_typen.rs:353 `.max_tokens((pakete.len() as i64) * 24 + 256)`
- rust/crates/tb-analytics/src/chat_typen.rs:355 `.json_object()`
- rust/crates/tb-analytics/src/chat_typen.rs:360 `serde_json::from_str(response.text.trim())` -> `LlmError::Unparsable("JSON nicht lesbar: ...")`
- rust/crates/tb-llm/src/hub.rs:69-100 `pub struct Request` (Felder max_tokens, json_object, allow_reasoning_content, strip_think)
- rust/crates/tb-llm/src/hub.rs:560-582 `build_openai_body`: setzt model, messages, max_tokens, temperature, response_format; kein Thinking-Parameter
- rust/crates/tb-llm/src/hub.rs:500-503 usage `completion_tokens` wird gelesen; hub.rs:202 `pub completion_tokens: Option<i64>` in der Antwort
- rust/crates/tb-llm/src/hub.rs:665-671 `extract_openai_text`: `reasoning_content` nur mit allow_reasoning_content
- rust/bin/tb-bot/src/chat_typen_wiring.rs:8-12 Konstanten LAUF_INTERVALL 3600 s, PAKET_GROESSE 40, TAGES_KAPPE 2000, MAX_FEHLER_SERIE 5
- rust/bin/tb-bot/src/chat_typen_wiring.rs:98-105 Warnung je Paket plus separate Abbruch-Warnung
- rust/bin/tb-bot/src/chat_typen_wiring.rs:109-114 Abschlusszeile `Lauf abgeschlossen`
- Bestehende wiremock-Tests: rust/crates/tb-analytics/src/chat_typen.rs:531 `mock_endpoint`, :541 `modell_bekommt_nur_other_und_wird_genau_einmal_gerufen`
- Vergleich funktionierender JSON-Aufrufer ohne max_tokens: rust/crates/tb-chat/src/promo_pitch.rs:328-333

## Externe Doku (Stand Recherche 2026-09-05)

- DeepSeek Chat Completions: Denkmodus per `thinking: {"type": "disabled"}` abschaltbar (api-docs.deepseek.com/guides/thinking_mode). `reasoning_effort: none` allein reicht laut litellm-Issue 27453 nicht.
- Fireworks-Modellseite deepseek-v4-flash-0731 (app.fireworks.ai/models/fireworks/deepseek-v4-flash-0731): Parameter für Fireworks-Hosting vom Implementierer nachschlagen.

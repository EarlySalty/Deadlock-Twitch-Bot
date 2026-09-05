# Contract: Denkmodus für Judge- und Klassifizierer-Aufrufe abschalten

Datum: 2026-09-06. Klasse: medium (Verhaltensänderung an Live-KI-Aufrufen, mehrere Crates, kleine Diffs je Stelle).

## Ziel

Deepseek V4 Flash antwortet bei Fireworks im Denkmodus. Bei Aufrufen mit strukturierter Kurzantwort (JSON-Urteil, Titelvorschläge, Klassifikation) frisst das Denken das Token-Budget: `title-insight` liegt im Schnitt bei 1413 von 1500 Ausgabe-Tokens (407 Aufrufe in 7 Tagen), der Chat-Klassifizierer lief vor dem Fix in acd62f21 exakt ans Budget und lieferte leere Antworten. Nach diesem Auftrag schalten alle Judge- und Klassifizierer-Aufrufe das Denken über den vorhandenen Builder `tb_llm::Request::denken_aus()` ab (setzt `reasoning_effort: none`), so wie es `chat_typen.rs` seit acd62f21 tut.

## Anforderungen

- REQ-1: Regel: Jeder `tb_llm::Request` im Produktivcode, der `.json_object()` setzt oder ein `max_tokens` von höchstens 2000 trägt, ruft zusätzlich `.denken_aus()`. Betroffen laut EVIDENCE: `title_ai.rs` (`titel_completion`, alle Use-Cases dieser Funktion inklusive `title-insight`), `promo_pitch.rs` (Judge mit `json_object`), `scam_pitch.rs` (Judge mit `JUDGE_MAX_TOKENS`), `crew_guard.rs`, `crew_review.rs`, `outreach_shadow.rs`, `llm_dispatch.rs` (nur wenn `json_object` gesetzt oder `max_tokens` ≤ 2000), `tb-stream-audit/src/main.rs`. Der Implementierer listet in PLAN.md jeden `tb_llm::Request`-Aufruf im Produktivcode mit Entscheidung (geändert oder warum nicht).
- REQ-2: Nicht angefasst werden Aufrufe für freie Text- oder Chatantworten ohne kleines Budget: `llm_chat.rs` (Engagement-Chat), `ai_chat.rs` (Dashboard-Assistent), `ai_analysis.rs`, `post_stream.rs` (6000 und 16000 Tokens) und die drei Pitch-Text-Builder in `promo_pitch.rs` (`build_channel_promo_text`, `build_targeted_pitch_text`, Partner-Pitch, Temperatur 0.7 ohne Budget).
- REQ-3: Regressionstests mit wiremock (Muster: `chat_typen.rs` Test `body_schaltet_das_denken_ab`, `title_ai.rs` Tests ab Zeile 794): (a) `titel_completion` schickt `reasoning_effort` im Body; (b) der Pitch-Judge (`FireworksPitchJudge::decide`) schickt `reasoning_effort` im Body; (c) der Scam-Judge in `scam_pitch.rs` schickt `reasoning_effort`. Für Module ohne Test-Endpunkt-Injektion nutzt der Test den Weg, den die bestehenden Tests des Moduls nutzen (`Request::endpoint`, `LlmEndpoint` per Mock). Alle drei Tests müssen vor dem Fix rot sein; roter Lauf mit Testname und Fehlermeldung in PLAN.md.
- REQ-4: `SQLX_OFFLINE=1 cargo test -p tb-llm -p tb-chat -p tb-engagement -p tb-social-media -p tb-stream-audit-bin -p tb-analytics` mit Toolchain 1.97.1 und `TB_TEST_DATABASE_URL` des Test-Containers (siehe EVIDENCE) grün gegen die Baseline (Baseline-Rot: `ad_manager_store::queue_lease_idempotenz_und_state_sind_atomar`, `ledger_side_effects::engagement_client_verbucht_usage_ins_zentrale_ledger`; werden in einem parallelen Auftrag repariert, hier nicht anfassen).

## Invarianten

- INV-1: Modell, Endpunkt, Prompts, Temperatur, Budgets und Timeouts bleiben; einzige Änderung am Request ist `.denken_aus()`.
- INV-2: Kein neuer Builder in tb-llm; `denken_aus` und der Body-Aufbau in `hub.rs` bleiben unverändert.
- INV-3: Keine ENV-Variablen, keine Config.
- INV-4: Keine Code-Kommentare neu schreiben; der bestehende Kommentar in `scam_pitch.rs` über den Denktext (Zeile 1946 ff.) wird gelöscht, wenn er durch `denken_aus` gegenstandslos ist.
- INV-5: `allow_reasoning_content` bleibt dort, wo es gesetzt ist (Scam-Judge), da ein Modellwechsel ohne Denk-Aus-Parameter das weiter braucht.

## Nicht-Ziele

- Keine Änderung an Budgets (`max_tokens`) oder Prompts, auch nicht an `title-insight` 1500.
- Kein Umbau der Pitch-Texte.

## Erlaubter Bereich

- rust/crates/tb-chat/src
- rust/crates/tb-engagement/src
- rust/crates/tb-social-media/src
- rust/bin/tb-stream-audit/src
- rust/crates/tb-analytics/src
- rust/.sqlx
- .tasks/2026-09-06-denken-aus-judges

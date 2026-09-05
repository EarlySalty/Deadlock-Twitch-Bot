# Plan: Denkmodus für Judge- und Klassifizierer-Aufrufe abschalten

Basis: origin/main fb62a7a5, Branch fix/denken-aus-judges.

## Vorgehen

1. Drei Regressionstests (REQ-3) geschrieben und rot bewiesen.
2. `.denken_aus()` an allen betroffenen Produktivaufrufen (REQ-1) gesetzt.
3. Voller Testlauf grün gegen die Baseline (REQ-4).

## Endpunkt-Injektion für den Pitch-Judge

`FireworksPitchJudge::decide` bekam eine interne Funktion `decide_intern(input, Option<LlmEndpoint>)`
nach dem Muster von `chat_typen::klassifiziere_modell_intern`. Die öffentliche
`PitchJudge::decide`-API bleibt unverändert (ruft `decide_intern(input, None)`). Nur so
lässt sich der Judge im wiremock-Test gegen einen Mock-Endpunkt schicken.

## Roter Lauf (vor dem Fix)

Befehl: `cargo test -j 4 -p tb-chat schaltet_das_denken_ab --no-fail-fast`
(Toolchain 1.97.1, SQLX_OFFLINE=1, Test-Container-DB).

```
test promo_pitch::tests::pitch_judge_schaltet_das_denken_ab ... FAILED
test title_ai::tests::titel_completion_schaltet_das_denken_ab ... FAILED
test scam_pitch::tests::call_judge_schaltet_das_denken_ab ... FAILED
test result: FAILED. 0 passed; 3 failed; 0 ignored
```

Fehlermeldung (identisch je Test): `assertion failed: body.contains("reasoning_effort")`,
der ausgegebene Request-Body enthielt kein `reasoning_effort`-Feld.

- `pitch_judge_schaltet_das_denken_ab` panicked at promo_pitch.rs:553 (Body ohne `reasoning_effort`).
- `titel_completion_schaltet_das_denken_ab` panicked at title_ai.rs:1203 (Body ohne `reasoning_effort`).
- `call_judge_schaltet_das_denken_ab` panicked at scam_pitch.rs:3162 (Body ohne `reasoning_effort`).

## Inventar aller `tb_llm::Request`-Aufrufe im Produktivcode

Regel REQ-1: `.json_object()` gesetzt oder `max_tokens` hoechstens 2000 fuehrt zu `.denken_aus()`.

| Datei:Zeile | Aufruf | json_object / max_tokens | Entscheidung |
|---|---|---|---|
| tb-chat/src/title_ai.rs:519 | `titel_completion` (Titel + `title-insight`) | max_tokens 1500 | geändert: `.denken_aus()` |
| tb-chat/src/promo_pitch.rs:367 | `FireworksPitchJudge::decide_intern` | json_object | geändert: `.denken_aus()` |
| tb-chat/src/promo_pitch.rs:444 | `build_channel_promo_text` | Temp 0.7, kein Budget, kein json_object | nicht geändert (Freitext-Pitch, Nicht-Ziel REQ-2) |
| tb-chat/src/promo_pitch.rs:453 | `build_targeted_pitch_text` | Temp 0.7, kein Budget | nicht geändert (Freitext-Pitch, Nicht-Ziel) |
| tb-chat/src/promo_pitch.rs:466 | `build_partner_pitch_text` | Temp 0.7, kein Budget | nicht geändert (Freitext-Pitch, Nicht-Ziel) |
| tb-chat/src/scam_pitch.rs:1939 | `call_judge` (Scam-Judge) | max_tokens 1500 (JUDGE_MAX_TOKENS) | geändert: `.denken_aus()`, Denktext-Kommentar gelöscht (INV-4), `allow_reasoning_content` bleibt (INV-5) |
| tb-chat/src/crew_guard.rs:555 | Crew-Judge | json_object | geändert: `.denken_aus()` |
| tb-engagement/src/crew_review.rs:206 | `CrewReview::decide` | json_object | geändert: `.denken_aus()` |
| tb-engagement/src/outreach_shadow.rs:233 | `OutreachShadow::decide` | json_object | geändert: `.denken_aus()` |
| tb-engagement/src/llm_chat.rs:819,853,875,965 | Engagement-Chat | Freitext, großes Budget | nicht geändert (Nicht-Ziel REQ-2) |
| tb-social-media/src/llm_dispatch.rs:113 | `FireworksProvider::generate_text` | json_object bedingt / max_tokens variabel (Aufrufer 600, 1400) | geändert: `.denken_aus()` bedingt bei strict json oder max_tokens hoechstens 2000 |
| tb-stream-audit/src/main.rs:2887 | Audit-Klassifizierer | json_object | geändert: `.denken_aus()` |
| tb-analytics/src/chat_typen.rs:352 | Chat-Klassifizierer | json_object + max_tokens | bereits `.denken_aus()` (Vorbild, live seit acd62f21) |
| tb-analytics/src/post_stream.rs:366,375 | Post-Stream-Analyse | 16000 / 6000 Tokens Freitext | nicht geändert (Nicht-Ziel REQ-2) |
| tb-dashboard-api/src/handlers/ai_chat.rs:95 | Dashboard-Assistent (Freitext) | max_tokens 4000 Freitext | nicht geändert (Nicht-Ziel REQ-2, kein kleines Budget) |
| tb-dashboard-api/src/handlers/ai_analysis.rs:144 | Dashboard-Analyse | max_tokens 60000 Freitext | nicht geändert (Nicht-Ziel REQ-2) |
| tb-dashboard-api/src/handlers/dashboard_assistent.rs:595 | Assistent-Antwort (`Request::history`) | max_tokens 768 (ANSWER_TOKEN_CEILING), strip_think, accept nicht-leer | REQ-1-Klasse, aber nicht geändert: liegt außerhalb des erlaubten Bereichs (tb-dashboard-api). Folgeauftrag nötig (siehe unten). |
| tb-dashboard-api/src/handlers/self_explainer.rs:339 | Self-Explainer (`Request::history`) | max_tokens 768 (ANSWER_TOKEN_CEILING), strip_think, accept nicht-leer | REQ-1-Klasse, aber nicht geändert: außerhalb des erlaubten Bereichs. Folgeauftrag nötig. |
| tb-llm/src/hub.rs, lib.rs | Builder-Definition + Tests | n/a | nicht geändert (tb-llm, INV-2) |

Der EVIDENCE-Grep suchte nach `tb_llm::Request::` und übersah die beiden
Dashboard-Handler, weil sie `use tb_llm::Request` importieren und `Request::history(...)`
ohne Pfad-Präfix bauen. Beide gehören formal in die REQ-1-Klasse (max_tokens 768) und
tragen dasselbe gefährliche Muster wie der Chat-Klassifizierer vor acd62f21:
`strip_think()` plus `accept(|text| !output_unusable(text))` heißt, dass ein
budgetfressender Denktext zu leerer Antwort und damit `Unparsable` führt, der
Assistent antwortet dem Streamer dann gar nicht.

## Folgeauftrag (Kritiker-Runde 1)

`tb-dashboard-api` steht nicht im erlaubten Bereich dieses Contracts; eine
Scope-Erweiterung braucht laut Repo-Regeln eine vom Nutzer angelegte
Freigabedatei. Deshalb bleiben beide Stellen hier unangetastet und brauchen einen
eigenen Auftrag mit erweitertem Scope:

- `rust/crates/tb-dashboard-api/src/handlers/dashboard_assistent.rs:595`: `.denken_aus()` ergänzen.
- `rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:339`: `.denken_aus()` ergänzen.

Beide sind die letzten verbliebenen Live-Aufrufe der Zielklasse und sollten zeitnah
denselben Fix bekommen.

## Grüner Lauf (nach dem Fix)

Befehl: `cargo test -j 4 -p tb-llm -p tb-chat -p tb-engagement -p tb-social-media -p tb-stream-audit-bin -p tb-analytics --no-fail-fast`

```
passed=1917 failed=1 ignored=9
```

Die drei neuen Tests grün:
```
test promo_pitch::tests::pitch_judge_schaltet_das_denken_ab ... ok
test title_ai::tests::titel_completion_schaltet_das_denken_ab ... ok
test scam_pitch::tests::call_judge_schaltet_das_denken_ab ... ok
```

Einziger Fehlschlag ist die dokumentierte Baseline
`ad_manager_store::queue_lease_idempotenz_und_state_sind_atomar`
(fehlende Tabelle `twitch_raw_chat_ingest_health` im Test-Schema, wird in einem
parallelen Auftrag repariert). Die zweite Baseline
`ledger_side_effects::engagement_client_verbucht_usage_ins_zentrale_ledger`
läuft in dieser Umgebung inzwischen grün.

## sqlx

Keine neuen Queries, `rust/.sqlx` unverändert.

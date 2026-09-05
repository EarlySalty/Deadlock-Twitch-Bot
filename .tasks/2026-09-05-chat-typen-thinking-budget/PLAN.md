# Plan: Chat-Typen-Klassifizierer, Denken abschalten

## Recherche REQ-1: Fireworks-Parameter zum Abschalten des Denkens

Quelle: Fireworks Chat-Completions-Referenz, Feld `reasoning_effort`
(https://docs.fireworks.ai/api-reference/post-chatcompletions) und der
Reasoning-Leitfaden (https://docs.fireworks.ai/guides/reasoning), abgerufen
2026-09-05.

Befund:

- `reasoning_effort` steuert das Denken der Modelle. Akzeptierte Werte:
  OpenAI-kompatibel `'low'`, `'medium'`, `'high'`, `'max'` zum Aktivieren und
  `'none'` zum Abschalten. Fireworks-Erweiterung zusätzlich Bool (`false` wird
  intern zu `'none'` normalisiert) und Integer (hartes Denk-Token-Limit).
- Der separate Parameter `reasoning_history` steuert laut Doku "prompt
  formatting only". Genau dort steht der entscheidende Satz: "To disable
  reasoning computation entirely, use `reasoning_effort='none'`." Die
  `reasoning_history`-Modelltabelle (DeepSeek V4: nur `interleaved`) betrifft
  also nicht das Abschalten der Berechnung, sondern nur den Umgang mit
  historischem Denktext.

Entscheidung: `reasoning_effort: "none"` im OpenAI-kompatiblen Body. Das ist
der von Fireworks ausdrücklich genannte Weg, die Denk-Berechnung vollständig
abzuschalten, passt ohne Umbau in den bestehenden OpenAI-Body und lässt den
DeepSeek-native `thinking`-Block (bei Fireworks im Anthropic-Format) außen vor.

Restunsicherheit: Die Fireworks-Doku führt keine explizite Pro-Modell-Tabelle
für `reasoning_effort='none'`; der Warnhinweis "if a model doesn't support
'none' ... will produce an error" nennt DeepSeek V4 nicht als Ausnahme. Sollte
DeepSeek V4 auf Fireworks `'none'` doch ablehnen, meldet der Aufruf einen
HTTP-Fehler statt eines leeren Bodys; der Live-Beweis nach dem Deploy klärt es
(Journal `tb_bot::chat_typen_wiring`, `minimax_usage` purpose
`chat_message_type`: `tokens_out` muss unter `max_tokens` fallen und
`modell` > 0).

## Umsetzung

- REQ-1: `Request.reasoning_off` plus Builder `denken_aus()` in
  `rust/crates/tb-llm/src/hub.rs`; `openai_compatible_body` setzt
  `reasoning_effort: "none"` nur bei gesetztem Flag. Ohne Builder-Aufruf bleibt
  der Body byteidentisch (INV-2).
- REQ-2: `klassifiziere_modell_intern` ruft `.denken_aus()`; `max_tokens`,
  Temperatur 0, `json_object` und Timeout bleiben.
- REQ-3: Budget-Fehler mit `max_tokens` und `completion_tokens`, wenn die
  Antwort nicht parsbar ist und `completion_tokens >= max_tokens`.
- REQ-4: Wiring sammelt Fehlerzahl und letzten Fehlertext und meldet einmal je
  Lauf; Abbruch nach `MAX_FEHLER_SERIE` bleibt.
- REQ-5: Zwei wiremock-Tests in `chat_typen.rs`.

## Roter Lauf (vor dem Fix)

Befehl: `SQLX_OFFLINE=1 cargo test -j 4 -p tb-analytics chat_typen --no-fail-fast`
(Toolchain 1.97.1), Log `/tmp/chat-typen-budget-rot.log`.

- `chat_typen::tests::body_schaltet_das_denken_ab` FAILED: Body enthält kein
  `reasoning_effort` (Panic mit Body-Dump, nur `max_tokens`, `messages`,
  `model`, `response_format`, `temperature`).
- `chat_typen::tests::erschoepftes_budget_nennt_die_ursache` FAILED:
  `Fehlertext: LLM-Antwort unbrauchbar: JSON nicht lesbar: EOF while parsing a
  value at line 1 column 0` statt "Budget".

Ergebnis: `9 passed; 2 failed`.

# Contract: Chat-Typen-Klassifizierer liefert live keine Labels (Thinking frisst das Token-Budget)

Datum: 2026-09-05. Klasse: medium (Bugfix im Live-Pfad, ein Crate-Zusatz in tb-llm).

## Ziel

Der stündliche Modell-Lauf des Chat-Typen-Jobs (`tb_bot::chat_typen_wiring`) liefert live gespeicherte Labels mit Quelle `modell` statt 5 Fehlpakete und Abbruch. Ursache: Deepseek V4 Flash antwortet im Denkmodus; das Denken verbraucht exakt das gesetzte `max_tokens` (1216 = 40*24+256), `content` bleibt leer oder abgeschnitten, `serde_json` scheitert mit "EOF while parsing".

## Anforderungen

- REQ-1: `tb_llm::Request` bekommt einen Builder, der das Denken des Modells für genau diesen Aufruf abschaltet und im OpenAI-kompatiblen Request-Body den dafür passenden Parameter setzt (`build_openai_body` in `rust/crates/tb-llm/src/hub.rs`). Welcher Parameter bei Fireworks für `deepseek-v4-flash-0731` gilt (`thinking: {type: disabled}` nach DeepSeek-Doku oder `reasoning_effort` nach Fireworks-Doku), recherchiert der Implementierer in der Fireworks-Dokumentation und hält Quelle und Entscheidung in PLAN.md fest. Ohne Builder-Aufruf ändert sich der Body nicht.
- REQ-2: `klassifiziere_modell_intern` in `rust/crates/tb-analytics/src/chat_typen.rs` nutzt diesen Builder. `max_tokens`, Temperatur 0, `json_object` und Timeout bleiben.
- REQ-3: Ist die Antwort leer oder nicht als JSON lesbar und `completion_tokens` der Antwort ist größer oder gleich dem gesetzten `max_tokens`, nennt der Fehler die Ursache ("Ausgabe-Budget erschöpft", mit `max_tokens` und `completion_tokens`) statt nur "JSON nicht lesbar".
- REQ-4: Das Wiring meldet Modellfehler je Lauf höchstens einmal: eine Warnung am Ende des Laufs mit Anzahl fehlgeschlagener Pakete und letztem Fehlertext, integriert in die bestehende Abschlusszeile oder direkt davor. Die bisherige Warnung je Paket und die separate Abbruch-Warnung entfallen; der Abbruch nach `MAX_FEHLER_SERIE` bleibt als Verhalten erhalten.
- REQ-5: Regressionstests ohne Live-DB in `chat_typen.rs` (wiremock wie die bestehenden Tests): (a) der gesendete Body enthält den Parameter aus REQ-1; (b) eine Antwort mit leerem `content` und `usage.completion_tokens == max_tokens` liefert einen Fehler, dessen Text "Budget" enthält. Beide Tests müssen vor dem Fix rot sein; der rote Lauf wird mit Testname und Fehlermeldung in PLAN.md festgehalten.
- REQ-6: `SQLX_OFFLINE=1 cargo test -p tb-llm -p tb-analytics -p tb-bot` mit Toolchain 1.97.1 gegen die Baseline grün (Baseline main 7dcae83f: `ad_manager_store::queue_lease_idempotenz_und_state_sind_atomar` und `ledger_side_effects::engagement_client_verbucht_usage_ins_zentrale_ledger` sind vorbestehend rot, fehlende Tabellen im Test-Schema).

## Invarianten

- INV-1: Modell bleibt Deepseek V4 Flash über den tb-llm-Hub; kein anderes Modell, kein eigener HTTP-Client.
- INV-2: Kein anderer Aufrufer von tb-llm ändert sein Verhalten (Body anderer Use-Cases byteidentisch).
- INV-3: Keine ENV-Variablen, keine neue Config; Limits bleiben Konstanten im Wiring.
- INV-4: Keine Code-Kommentare neu schreiben; bestehende in angefassten Zeilen löschen statt erweitern.
- INV-5: Kein Änderung an `twitch_chat_messages`, `twitch_chat_message_labels` oder Migrationen.

## Nicht-Ziele

- Andere Use-Cases mit ausgeschöpftem Budget (etwa `title-insight`, 1418 von 1500 Tokens im Schnitt) werden nicht angefasst.
- Kein Umbau des Paket- oder Tageskappen-Modells, keine Prompt-Änderung.

## Erlaubter Bereich

- rust/crates/tb-llm/src/hub.rs
- rust/crates/tb-llm/src/lib.rs
- rust/crates/tb-analytics/src/chat_typen.rs
- rust/bin/tb-bot/src/chat_typen_wiring.rs
- rust/.sqlx
- .tasks/2026-09-05-chat-typen-thinking-budget

# LLM-Aufrufe: ein Eingang statt fuenfzehn Clients

Stand der Bestandsaufnahme: 2026-08-21, Branch `refactor/llm-zentral` auf Basis
`origin/main` (19021bfa).

## TLDR

Vor dem Umbau gab es 15 Stellen, die selbst HTTP gegen ein Sprachmodell
sprechen, mit sieben eigenen Client-Structs, fuenf Kopien der Endpunkt-Konstanten
und fuenf unterschiedlichen Fehler-Enums. Nach dem Umbau gibt es genau einen
Eingang: `tb_llm::complete(use_case, Request)`. Provider-Auswahl, Failover,
Ledger, Timeout, `<think>`-Strip und Fehlerklassifikation liegen dort.

## Bestandsaufnahme vor dem Umbau

Bestandssuche zuerst ueber Graphify (`graphify query` auf dem Repo-Graphen zu
LLM-Clients, `endpoint_for`/`endpoint_chain`-Aufrufern und dem tb-llm-Aufbau),
danach mit `rg` auf Endpunkt-Literale (`api.fireworks.ai`, `api.minimax.io`,
`api.anthropic.com`, `chat/completions`, `v1/messages`) gegengeprueft.

| # | Datei:Zeile | Use-Case | Provider heute | Besonderheiten |
|---|---|---|---|---|
| 1 | `rust/crates/tb-llm/src/minimax.rs:112` | generischer Port `MiniMaxClient::complete` | MiniMax (OpenAI-kompatibel) | Ledger, Timeout 240 s, Temperatur-Default 0.5. **Null externe Aufrufer** |
| 2 | `rust/crates/tb-llm/src/anthropic.rs:139` | generischer Port `AnthropicClient::complete` | Anthropic Messages | kein Ledger, keine Temperatur, Fehlerbody wird durchgereicht. **Null externe Aufrufer** |
| 3 | `rust/crates/tb-engagement/src/minimax_chat.rs:941` | `engagement`, dazu `chat-deep-analysis`, Soul-Reflexion, Folgechat | `endpoint_for("engagement")` plus MiniMax-Altlast-Sonderpfad | Ledger, Temperatur 0.7 bzw. frei, `max_tokens` optional weglassbar, Timeout 30 s, Sprecher wird in den Content gefaltet, Nachbehandlung `process_response_text` mit `<think>`-Strip |
| 4 | `rust/crates/tb-engagement/src/claude_chat.rs:100` | `ai_analysis`, `ai_chat` (Dashboard-Opus-Pfad) | Anthropic, eigene Konstanten `DEFAULT_BASE_URL`/`DEFAULT_MODEL` | Timeout 240 s, kein Ledger, liefert das rohe `content`-Block-Array, Fehlerbody traegt "credit balance is too low" |
| 5 | `rust/crates/tb-engagement/src/crew_review.rs:213` | `ricky_crew_review` (Schatten-Review) | Fireworks, eigene Konstanten, fail-closed | Temperatur 0.0, `response_format: json_object`, Timeout 20 s, Redirects aus, Fehlerklassen Timeout/HttpStatus/Decode/Validation, verweigert den Start bei abweichender Adresse oder abweichendem Modell |
| 6 | `rust/crates/tb-engagement/src/outreach_shadow.rs:225` | `outreach_shadow` | Fireworks, Adresse und Modell hart im Aufruf | wie 5, zusaetzlich Evidenz-Validierung der Antwort |
| 7 | `rust/crates/tb-chat/src/crew_guard.rs:540` | `crew_guard` | OpenAI-kompatibel ueber `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `CREW_GUARD_MODEL` | Temperatur 0.0, `json_object`, Timeout 12 s, Ausfallzaehler mit Warn-Sentinel, fail-safe `unsure` ohne Key oder Modell |
| 8 | `rust/crates/tb-chat/src/title_ai.rs:562` | `title_ai` | `endpoint_for("title_ai")` | eigene 429-Wiederholung (3 Versuche, `Retry-After` bis 5 s), Temperatur 0.35, `max_tokens` 2000, Timeout 240 s, Ledger im Aufrufer |
| 9 | `rust/crates/tb-chat/src/scam_pitch.rs:1829` | `spam_judge` | `endpoint_chain("spam_judge")` | eigene Failover-Schleife ueber die Kette, Temperatur 0.0, Timeout 20 s, Fallback auf `reasoning_content`, `<think>`-Strip, Ledger, Failover auch bei unparsebarer Antwort |
| 10 | `rust/crates/tb-analytics/src/post_stream.rs:411` | `post_stream` (MiniMax-Zweig) | `endpoint_for("post_stream")` | Temperatur 0.3, `max_tokens` 16000, Timeout 180 s, Ledger unter `post-stream-report` |
| 11 | `rust/crates/tb-analytics/src/post_stream.rs:467` | `post_stream` (Opus-Zweig) | Anthropic, eigene Konstanten `ANTHROPIC_BASE_URL`/`CLAUDE_MODEL` | `max_tokens` 6000, Timeout 240 s, kein Ledger, Fehlerbody auf 300 Zeichen gekuerzt |
| 12 | `rust/crates/tb-social-media/src/llm_dispatch.rs:195` | `social_media` (MiniMax-Zweig) | MiniMax, eigene Konstanten | Timeout 60 s, `json_object` nur wenn der System-Prompt "strict json" enthaelt, Kostenschaetzung aus Tokens, Temperatur 0.4 |
| 13 | `rust/crates/tb-social-media/src/llm_dispatch.rs:265` | `social_media` (Claude-Haiku-Zweig) | Anthropic Haiku, eigene Konstanten | wie 12, aber mit Temperatur im Body |
| 14 | `rust/crates/tb-social-media/src/llm_dispatch.rs:90` | `social_media` (Ollama-Zweig) | lokaler Ollama-Dienst | `/api/generate` statt Chat-Schema, keine Kosten, Einwilligungs-Gate aus der Datenbank |
| 15 | `rust/bin/tb-stream-audit/src/main.rs:2483` | `stream_audit` | `endpoint_for("stream_audit")` | Erlaubnis-Gate fuer fremde Anbieter, `json_object`, kein `max_tokens`, Stapel von 20 Segmenten, kein Ledger, Fehler landen als Hinweis im Bericht |

Nicht in dieser Liste, bewusst:

- `rust/crates/tb-engagement/src/transcribe.rs:442` spricht die
  OpenAI-Audio-Transkription an. Das ist Sprache zu Text, kein Chat-Modell, und
  hat mit der Provider-Auswahl nichts zu tun.
- Die Judges `MiniMaxScamJudge`, `MiniMaxLfgJudge`, `MiniMaxInviteQuestionJudge`
  bauen keinen eigenen HTTP-Pfad, sondern nutzen `EngagementMinimaxClient`
  (Nummer 3). Sie erben den Umbau.

## Der eine Eingang

```rust
tb_llm::complete(use_case: &str, request: Request) -> Result<Response, LlmError>
```

`Request` traegt alles, was die Aufrufer bisher selbst gebaut haben:

| Feld | Bedeutung |
|---|---|
| `system` | System-Prompt; landet bei OpenAI-kompatiblen Anbietern als erste Nachricht, bei Anthropic im `system`-Feld |
| `messages` | Verlauf |
| `max_tokens` | `None` laesst das Feld weg (der Folgechat braucht das) |
| `temperature` | `None` laesst das Feld weg (Anthropic-Paritaet) |
| `json_object` | setzt `response_format` |
| `timeout` | pro Aufruf, sonst 240 s |
| `ledger` | `Off` oder `Purpose(name)`; Default ist der Use-Case-Name |
| `strip_think` | entfernt `<think>`-Bloecke aus dem Antworttext |
| `accept` | Praedikat auf dem Antworttext; schlaegt es fehl, geht der Hub zum naechsten Anbieter der Kette |
| `retry_on_429` | Anzahl Wiederholungen bei HTTP 429, `Retry-After` wird beachtet |
| `failover` | Kette abarbeiten oder nur den ersten Anbieter |
| `endpoint` | expliziter Endpunkt statt Auswahl, fuer Tests und fuer fail-closed-Aufrufer |

`Response` liefert `text`, `provider`, `model`, `prompt_tokens`,
`completion_tokens`, `latency_ms`. `LlmError` unterscheidet `Unavailable`,
`Timeout`, `Http { status, body }`, `Transport` und `Unparsable`, damit die
Aufrufer ihre bisherigen Fehlerklassen weiter bilden koennen.

## Provider

Nur in `tb-llm`, nirgends sonst:

- Fireworks: `https://api.fireworks.ai/inference/v1`, Default-Modell
  `accounts/fireworks/models/deepseek-v4-flash`
- MiniMax: `https://api.minimax.io/v1`, Default-Modell `MiniMax-M3`
- Anthropic: `https://api.anthropic.com/v1/messages`, Default-Modell
  `claude-opus-4-6`

Ohne Konfiguration entscheidet weiterhin der Key: Fireworks, wenn einer gesetzt
ist, sonst MiniMax. Anthropic kommt nur, wenn ein Use-Case ihn ausdruecklich
waehlt, also genau fuer die Faelle, die ihn heute schon nutzen. Kein OpenAI,
keine neuen Anbieter (Direktive in `tb-llm/src/lib.rs`).

## Umgebungsvariablen

| Variable | Wirkung |
|---|---|
| `TB_LLM_PROVIDER_DEFAULT` | Anbieter fuer alles |
| `TB_LLM_PROVIDER_<USE_CASE>` | Anbieter fuer einen Use-Case |
| `TB_LLM_MODEL_<USE_CASE>` | Modell fuer einen Use-Case, unabhaengig vom Anbieter |
| `FIREWORK_API_KEY`, `FIREWORKS_API_KEY` | Fireworks-Key |
| `FIREWORK_BASE_URL`, `FIREWORKS_BASE_URL`, `FIREWORK_MODEL`, `FIREWORKS_MODEL` | Fireworks-Adresse und -Modell |
| `MINIMAX_TOKEN_PLAN_KEY`, `MINIMAX_API_KEY`, `MINMAX` | MiniMax-Key |
| `MINIMAX_BASE_URL`, `MINIMAX_MODEL` | MiniMax-Adresse und -Modell |
| `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, `ANTHROPIC_MODEL` | Anthropic |

Entfallen mit diesem Umbau:

| Alt | Neu |
|---|---|
| `ENGAGEMENT_MINIMAX_MODEL` | `TB_LLM_MODEL_ENGAGEMENT` |
| `FIREWORKS_RICKY_REVIEW_MODEL` | `TB_LLM_MODEL_RICKY_CREW_REVIEW` |
| `ANTHROPIC_HAIKU_MODEL` | `TB_LLM_MODEL_SOCIAL_MEDIA_CLAUDE` |
| `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `CREW_GUARD_MODEL` | bleiben, siehe "Bewusst nicht migriert" |
| MiniMax-Sonderpfad in `minimax_chat.rs` (`MINIMAX_TOKEN_PLAN_KEY`, `MINIMAX_API_KEY`, `MINIMAX_BASE_URL` nur wenn die Auswahl MiniMax ergab) | faellt weg; die Variablen wirken weiterhin, aber ueber die zentrale Auswahl in `selection.rs` |

**Wichtig beim Umstellen eines Modells:** `TB_LLM_MODEL_<USE_CASE>` gilt fuer
jeden Anbieter. Wer ein MiniMax-Modell setzt, waehrend die Auswahl Fireworks
ergibt, bekommt einen Modellfehler vom Anbieter. Deshalb setzt der
Dashboard-Wrapper `TB_LLM_PROVIDER_ENGAGEMENT=minimax` neben
`TB_LLM_MODEL_ENGAGEMENT=MiniMax-Text-01`. Frueher hielt ein Sonderpfad im Code
das auseinander; die Bedingung steht jetzt im Wrapper, wo sie sichtbar ist.

Der Schluessel eines Anbieters kommt jetzt ueberall aus `tb_llm::keys`. Fuer
Fireworks heisst das: `FIREWORK_API_KEY` gewinnt vor `FIREWORKS_API_KEY`. Das
Crew-Review bevorzugte vorher den Plural. Beide Namen zeigen auf dasselbe
Fireworks-Konto; die Rangfolge ist jetzt an einer Stelle statt an dreien.

Der Sonderpfad in `minimax_chat.rs` war eine Ruecksprungmoeglichkeit aus der
Zeit, als der Engagement-Client als einziger auf MiniMax festgenagelt war. Er
liest dieselben Variablen, die `selection.rs` ohnehin liest, nur mit einer
zweiten Rangfolge. Zwei Rangfolgen fuer dieselben Variablen sind eine
Fehlerquelle, kein Sicherheitsnetz.

## Anwendungsfaelle nach dem Umbau

| Anwendungsfall | Aufrufer | Anbieter ohne Konfiguration |
|---|---|---|
| `engagement` | `minimax_chat.rs`, dazu alle Judges und Dashboard-Pfade darauf | Fireworks, sonst MiniMax |
| `title_ai` | `title_ai.rs` (Titel und Insight) | Fireworks, sonst MiniMax |
| `spam_judge` | `scam_pitch.rs`, mit eigener Kette | Fireworks, sonst MiniMax |
| `crew_guard` | `crew_guard.rs`, mit eigenem Endpunkt | siehe unten |
| `ricky_crew_review` | `crew_review.rs`, fail-closed | Fireworks |
| `outreach_shadow` | `outreach_shadow.rs`, fail-closed | Fireworks |
| `post_stream` | `post_stream.rs`, MiniMax-Zweig | Fireworks, sonst MiniMax |
| `post_stream_opus` | `post_stream.rs`, Opus-Zweig | Anthropic |
| `ai_analysis` | `handlers/ai_analysis.rs` | Anthropic |
| `ai_chat` | `handlers/ai_chat.rs` | Anthropic |
| `social_media` | `llm_dispatch.rs`, MiniMax-Zweig | Fireworks, sonst MiniMax |
| `social_media_claude` | `llm_dispatch.rs`, Haiku-Zweig | Anthropic Haiku |
| `stream_audit` | `bin/tb-stream-audit` | Fireworks, sonst MiniMax |

## Was der Umbau geloescht hat

- `tb-llm/src/minimax.rs`, `tb-llm/src/anthropic.rs`, `tb-llm/src/provider.rs`:
  zwei Einzelclients und ein Trait-Port ohne einen einzigen Aufrufer ausserhalb
  der Crate.
- `tb-engagement/src/claude_chat.rs`: der zweite Anthropic-Client.
- Sieben eigene `reqwest::Client`-Aufbauten, fuenf Kopien der
  Antwort-Structs, vier Kopien des Token-Auslesens und drei Kopien der
  Env-Fallback-Kette.

## Bewusst nicht migriert

- **Ollama-Zweig in `llm_dispatch.rs`.** Ein lokaler Dienst mit eigenem
  Endpunkt-Schema (`/api/generate`), ohne Key, ohne Kosten, hinter einem
  Einwilligungs-Gate aus der Datenbank. Ihn in `tb-llm` zu ziehen hiesse, einen
  dritten Anbieter samt zweitem Request-Schema aufzunehmen, und der Grund fuer
  seine Existenz ist gerade, dass er nicht Teil der Anbieterauswahl ist.
- **`crew_guard.rs`.** Als einziger Aufrufer spricht er einen Endpunkt hinter
  `OPENAI_API_KEY`/`OPENAI_BASE_URL` an. Der Hub uebernimmt Transport, Timeout
  und Fehlerklassifikation ueber den expliziten `endpoint`-Weg, die
  OpenAI-Konstante bleibt aber in `crew_guard.rs`: `tb-llm` nimmt keinen
  OpenAI-Anbieter auf, das verbietet die Direktive im Crate-Kopf.
- **`transcribe.rs`.** Sprache zu Text, kein Chat-Modell.

# tb-llm — gemeinsame LLM-Client-Foundation (Phase 0)

> **Stand nach dem Umbau "LLM zentral"** (siehe
> `docs/architecture/llm-zentral.md`): Die Module `provider`, `minimax` und
> `anthropic` samt `LlmProvider`-Trait, `MiniMaxClient`, `AnthropicClient` und
> dem `ClaudeClient` in `tb-engagement` sind geloescht. An ihre Stelle treten
> `hub` (ein HTTP-Weg fuer alle Anbieter, `tb_llm::complete`) und `selection`
> (Anbieter- und Modellwahl je Use-Case). Die Abschnitte unten beschreiben
> den urspruenglichen Phase-0-Stand und gelten nur noch dort, wo sie `keys`
> und `ledger` betreffen.

Crate: `rust/crates/tb-llm`. Liefert die geteilte LLM-Schicht für den Twitch-Bot-
Cutover. Urspruenglich **Foundation only**: Clients + Ledger, bestehende
Aufrufer (scam_pitch, title_ai, post_stream, Dashboard-AI-Handler) wurden erst
im Umbau "LLM zentral" umgestellt.

## Struktur (heute)

| Modul | Inhalt |
|-------|--------|
| `hub` | `complete`/`complete_detailed`, `Request`, `Response`, `LlmError`, `LlmFailure`; ein HTTP-Weg fuer OpenAI-kompatible Anbieter und Anthropic, Retry bei 429, `<think>`-Strip, Ledger-Verbuchung. |
| `selection` | `endpoint_for`/`endpoint_chain`, `LlmEndpoint`, alle Anbieter-Konstanten, `ANTHROPIC_USE_CASES`. |
| `keys` | Konsolidierter API-Key-Resolver (kein Dup). |
| `ledger` | Best-effort-Writer ins geteilte Usage-Ledger (Postgres, Tabelle `minimax_usage`). |

Genau **drei** Anbieter (Fireworks, MiniMax, Anthropic). **Kein OpenAI** (Client,
Pfad oder Dep) — Querschnitts-Direktive 2 des Grillme-Audits. (Die
`openai`-Erwähnungen in den Doc-Comments betreffen nur das *OpenAI-kompatible*
Endpunkt-Schema von Fireworks und MiniMax.)

## Provider-Port (historisch, geloescht)

```rust
// Gab es in Phase 0; heute ersetzt durch tb_llm::complete(use_case, Request).
trait LlmProvider {
    fn name(&self) -> &'static str;       // "minimax" | "anthropic"
    fn model(&self) -> &str;
    async fn complete(&self, req: &CompletionRequest, purpose: &str)
        -> Result<CompletionResponse, LlmError>;
}
```

Die **Provider-Auswahl** lag in Phase 0 bewusst beim Aufrufer (Python-Orakel:
`api_ai.py:_plan_ai_model`, `analytics.ai_full` → Anthropic, `analytics.ai_mini`
→ MiniMax). Heute liegt sie in `selection.rs`.

## Key-Resolver (`keys`)

- MiniMax: `MINIMAX_TOKEN_PLAN_KEY` → `MINIMAX_API_KEY` → `MINMAX`.
- Anthropic: `ANTHROPIC_API_KEY`.

Nur aus Env (Infisical/systemd injiziert). Kein Keyring-Fallback (Grillme
`fernet-crypto-5`). Secrets werden NIE geloggt.

## Usage-Ledger (`ledger`)

Geteiltes SQLite (WAL), cross-bot/cross-prozess/cross-sprache — dasselbe File
nutzen der Python-Helfer (`~/Documents/.claude/minimax-usage/minimax_usage.py`)
und der Rust-TradingBot (`tb-ai`). Schema **byte-identisch** (Tabelle
`minimax_usage`, Spalten `id, ts, source, purpose, model, tokens_in, tokens_out,
total, success, meta`).

- Pfad: Env `MINIMAX_USAGE_DB`, sonst `~/.claude/minimax-usage/ledger.db`.
- `source` immer `twitch-bot`.
- `ts`: ISO-8601 UTC mit `+00:00`-Offset (Python-kompatibel).
- 5h-Fenster + Budget-Warnung (`MINIMAX_5H_TOKEN_BUDGET`, 0 = aus) — nur messend.
- **Best-effort:** jeder DB-Fehler → `warn`-Log, nie ein Hard-Fail des Calls.

`purpose`-Werte (aus dem Python-Orakel, je Feature): `engagement`, `spam-review`,
`title`, `title-insight`, `social-media`, `analytics`, `analytics-chat`,
`chat-deep-analysis`, `post-stream-report`, `coaching-audit`.

## Divergenz zu Python (bewusst)

Das Python-Orakel verbucht **nur MiniMax**-Tokens ins Ledger; der Anthropic-Pfad
schreibt dort nichts. Diese Foundation verbucht auch den Anthropic-/Premium-
Verbrauch (mit dem Anthropic-Modellnamen als Unterscheidungsmerkmal), damit der
gesamte LLM-Verbrauch dieses Bots an einer Stelle messbar ist. Siehe offene
Frage in `docs/06-open-questions.md`.

## Tests

16 Unit-/Integrationstests inline (wiremock für die HTTP-Pfade, Temp-SQLite für
das Ledger): Completion-Parse + Token-Verbuchung (MiniMax + Anthropic),
HTTP-Fehlerpropagation, Unavailable ohne Key, Ledger-Schema-Parität,
Token-Clamping, 5h-Fenster, Key-Resolver-Reihenfolge.

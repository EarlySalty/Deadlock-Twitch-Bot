# tb-llm — gemeinsame LLM-Client-Foundation (Phase 0)

Crate: `rust/crates/tb-llm`. Liefert die geteilte LLM-Schicht für den Twitch-Bot-
Cutover. **Foundation only** — sie stellt Clients + Ledger bereit, bestehende
Aufrufer (scam_pitch, title_ai, post_stream, Dashboard-AI-Handler) wurden NICHT
umgebaut. Das ist der spätere Schritt im F2-Ticket des Build-DAG.

## Struktur

| Modul | Inhalt |
|-------|--------|
| `provider` | Port `LlmProvider` (Trait) + geteilte Typen `Message`, `CompletionRequest`, `CompletionResponse`, `LlmError`. |
| `minimax` | `MiniMaxClient` — Primär-Provider, OpenAI-kompatibles `/chat/completions`. |
| `anthropic` | `AnthropicClient` — Premium/`ai_full`, Messages API; `extract_text` für das `content`-Block-Array. |
| `keys` | Konsolidierter API-Key-Resolver (kein Dup). |
| `ledger` | Best-effort-Writer ins geteilte MiniMax-Usage-Ledger (SQLite). |

Genau **zwei** Provider. **Kein OpenAI** (Client, Pfad oder Dep) — Querschnitts-
Direktive 2 des Grillme-Audits. (Die `openai`-Erwähnungen in den Doc-Comments
betreffen nur das *OpenAI-kompatible* MiniMax-Endpunkt-Schema.)

## Provider-Port

```rust
trait LlmProvider {
    fn name(&self) -> &'static str;       // "minimax" | "anthropic"
    fn model(&self) -> &str;
    async fn complete(&self, req: &CompletionRequest, purpose: &str)
        -> Result<CompletionResponse, LlmError>;
}
```

Bei Erfolg verbucht `complete` die Tokens best-effort ins Ledger (siehe unten).
Die **Provider-Auswahl** (welcher Provider je Feature/Entitlement) ist bewusst
NICHT Teil der Schicht — sie bleibt im Aufrufer (Python-Orakel:
`api_ai.py:_plan_ai_model`, `analytics.ai_full` → Anthropic, `analytics.ai_mini`
→ MiniMax).

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

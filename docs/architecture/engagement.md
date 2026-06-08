# engagement/ — Architektur & Funktionsreferenz

> Pfad: `bot/engagement/` · Stand: 2026-06-08 · 25 Dateien, ~5.120 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [chat.md](chat.md) (Chat-Transport), [core.md](core.md) (LLM-Provider), [storage.md](storage.md). Hintergrund: [Engagement-Layer-Spec](../../features/), Memory „Beziehungsführung statt Trivia“ + „AI-Sprache Stil-Few-Shot“.

## 1. Zweck & Abgrenzung

`engagement/` ist die **KI-Konversations-Schicht**: ein MiniMax-getriebener „Stammgast“, der in Partner-Kanälen mitliest und gezielt, menschlich und *grounded* antwortet — als Beziehungsführung über **Konversations-Fäden** (Threads), nicht als Trivia-Dump. Sprache wird per **Few-Shot-Stilbeispielen** geformt; Spielfakten werden gegen **Deadlock-Wiki/Patches** geerdet, um Halluzinationen zu vermeiden.

Abgrenzung: Das eigentliche Senden/Empfangen läuft über [chat.md](chat.md); `engagement/` entscheidet **was** und **ob** geantwortet wird. Im Twitch-Chat laufen die Antworten zunächst Shadow (siehe Memory „AI-Antwort Shadow-Rollout“).

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | Chat-Bot (Engagement-Hook), Dashboard (v2-Engagement-API). |
| **Nutzt** | MiniMax (`EngagementMinimaxClient`), `storage/` (Threads, Settings, Log), `chat/` (Senden), Deadlock-Wiki/Patches-Grounding. |
| **DB-Tabellen** | Engagement-Settings je Channel, Konversations-Threads, Engagement-Log, Sende-Account-Auth. |
| **Externe Dienste** | MiniMax-API, Deadlock-Wiki. |
| **Secret-Namen** | MiniMax-Key, Engagement-Sende-Account-OAuth. |

## 3. Dateien im Überblick (Auswahl)

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `pipeline.py` | 509 | Orchestrierung: aus Chat-Turn → Entscheidung → Antwort. |
| `dashboard_api.py` | 476 | JSON-API `/twitch/api/v2/engagement/*` (Settings, Log, Sender-Auth). |
| `sender_auth.py` | 351 | OAuth des separaten Engagement-Sende-Accounts. |
| `threads.py` | 334 | Konversations-Fäden mit Lebenszyklus (`Thread`). |
| `minimax_chat.py` | 298 | `EngagementMinimaxClient` + System-Prompt-Bau. |
| `background.py` | 292 | Hintergrund-Tasks (Thread-Extraktion, Auto-Close). |
| `irc_reader.py` | 272 | Zweite Chat-Lesequelle für Engagement. |
| `match_context.py` | 257 | Aktueller Deadlock-Match-Kontext fürs Prompting. |
| `stream_transcripts.py` | 244 | Stream-Transkript-Kontext. |
| `deadlock_wiki.py` | 239 | Wiki-Grounding (Spielfakten). |
| `soul_store.py` | 188 | „Soul“/Charakter-Definition. |
| `style_examples.py` | 186 | Few-Shot-Stilbeispiele (show, don’t tell). |
| `persona.py` | 181 | Adaptive Channel-Vibe-Stichprobe (`PersonaSnapshot`). |
| `deadlock_patches.py` | 179 | Patch-Grounding. |

## 4. Datenfluss / Lebenszyklus

1. **Mitlesen:** `irc_reader`/Chat-Hook sammeln jüngste Chat-Turns je Channel.
2. **Kontext bauen:** `persona.sample_tone(channel)` liefert einen 5-min-gecachten `PersonaSnapshot` (Channel-Vibe); `threads.load_open_threads_for_user` lädt offene Konversations-Fäden; `match_context`/`stream_transcripts` ergänzen Spiel-/Stream-Kontext; `deadlock_wiki`/`deadlock_patches` liefern Fakten-Guardrails.
3. **Prompt:** `minimax_chat.build_baseline_system_prompt` setzt Soul (`soul_store`) + Fakten-Guardrails + Stil/Format (`style_examples`) zusammen.
4. **Generieren:** `EngagementMinimaxClient.generate(...)` ruft MiniMax (M3); `_sanitize_chat_text` kappt/säubert die Antwort (max. 480 Zeichen) vor dem Senden.
5. **Threads pflegen:** `background` extrahiert periodisch neue Threads aus den Turns (`threads.extract_threads`) und schließt veraltete (`auto_close_stale`); referenzierte Threads werden markiert (`mark_referenced`).

## 5. Funktionsreferenz pro Datei

### minimax_chat.py
- `EngagementMinimaxClient(*, api_key=None, base_url=None, model=None, timeout=30.0)` mit `generate(*, system_prompt, history, max_output_tokens=200, max_answer_len=480) -> ChatResponse` — der MiniMax-M3-Aufruf; `ChatMessage`/`ChatResponse` als Datentypen. `LLMProviderUnavailable` bei fehlendem SDK/Key.
- `build_baseline_system_prompt(*, streamer_login) -> str` — Soul + Fakten-Guardrails + Stil/Format.
- `_sanitize_chat_text(text, *, max_len=480)` — Bot-Text vor dem Senden säubern.

### threads.py
- `Thread` — ein Konversations-Faden (Typ, Summary, Fälligkeit).
- `load_open_threads_for_user(user_id, channel_login, *, limit=5) -> list[Thread]` — offene Fäden eines Users.
- `threads_to_prompt_fragment(user_login, threads) -> str` — Fäden als Prompt-Kontext.
- `extract_threads(channel_login, *, minimax, hours=6, limit=80) -> int` — neue Fäden aus jüngsten Turns extrahieren (Insert nur bei neuem `(user, type, summary)`).
- `mark_referenced(thread_ids)` / `auto_close_stale() -> dict` — referenziert markieren bzw. veraltete schließen.

### persona.py
- `PersonaSnapshot` (`to_prompt_fragment`) + `sample_tone(channel_login, *, limit=50)` — adaptiver Channel-Vibe, 5 min gecacht; `_compute(texts)` rechnet ihn aus den letzten User-Turns.

### dashboard_api.py
- `register_engagement_v2_routes(router, server)` — mountet die JSON-Endpoints.
- Handler: `_handle_get_settings*`, `_handle_post_update` (Settings je Channel: enabled, steam_id, persona_override, tabu_topics), `_handle_get_log` (Engagement-Log), `_handle_sender_auth_start` (Admin: Authorize-Link für Sende-Account), `_handle_sender_auth_callback` (öffentlicher OAuth-Callback, State-gesichert). Serializer `_serialize_settings`/`_serialize_log`.

### pipeline.py / background.py / irc_reader.py
- `pipeline.py` — orchestriert den kompletten „Turn → Entscheidung → Antwort“-Fluss (Gate, Kontextaufbau, Generierung, Sende-Übergabe).
- `background.py` — periodische Tasks (Thread-Extraktion, Auto-Close, Persona-Refresh).
- `irc_reader.py` — eigene IRC-Lesequelle, die Engagement mit Chat-Turns füttert.

### Grounding & Stil
- `deadlock_wiki.py` / `deadlock_patches.py` — Spiel-/Patch-Fakten als Guardrails (gegen Halluzination).
- `soul_store.py` — Charakter/„Soul“. `style_examples.py` — echte Chat-Beispiele als Few-Shot-Stil. `match_context.py` / `stream_transcripts.py` — Spiel-/Stream-Kontext. `sender_auth.py` — OAuth des Sende-Accounts (eigene Identität, nicht der Mod-Bot).

## 6. Datenbank & externe Schnittstellen

- **DB:** Engagement-Settings (je Channel), Konversations-Threads, Engagement-Log, Sende-Account-Auth.
- **HTTP:** `/twitch/api/v2/engagement/*` (Settings/Log/Sender-Auth).
- **Extern:** MiniMax-M3, Deadlock-Wiki.

## 7. Stolperfallen / Besonderheiten

- **Grounding ist Pflicht:** Spielfakten kommen aus `deadlock_wiki`/`deadlock_patches`, nicht aus dem Modellwissen — sonst halluziniert MiniMax Deadlock-Fakten (siehe Memory). Der Bot nennt nie eigene Mechanik/Wissenslücken.
- **Beziehung statt Trivia:** Threads haben einen Lebenszyklus (offen → referenziert → auto-closed). Sie sind Gesprächsfäden, keine Fakten-DB zum Auspacken.
- **Shadow im Chat:** Twitch-Antworten laufen zunächst nur ins Logging/Discord, nicht live — bis zur Freischaltung. Die Website-Frage-Box (Self-Explainer, siehe [chat.md](chat.md)) antwortet dagegen direkt.
- **Antwortlänge gekappt:** `_sanitize_chat_text`/`max_answer_len=480` kappt die Ausgabe — lange MiniMax-Antworten werden hart beschnitten.
- **Eigene Sende-Identität:** Engagement nutzt einen separaten OAuth-Account (`sender_auth`), nicht den Moderations-Bot — Verwechslung führt zu falschem Absender.

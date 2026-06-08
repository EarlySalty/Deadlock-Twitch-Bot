# community/ — Architektur & Funktionsreferenz

> Pfad: `bot/community/` · Stand: 2026-06-08 · 16 Dateien, ~4.890 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [chat.md](chat.md) (Chat-Bot, in den Voice-Reaction einsteckt), [monitoring.md](monitoring.md) (Kategorie-Streams für Recruit), [core.md](core.md) (LLM-Provider).

## 1. Zweck & Abgrenzung

`community/` bündelt drei community-bezogene Features:

1. **Leaderboard** (`leaderboard.py`): das interaktive `!twl`-Discord-Leaderboard der getrackten/Kategorie-Streamer (sortierbar, filterbar, als Discord-View).
2. **Partner-Recruit** (`partner_recruit.py`): erkennt häufig auftauchende Deadlock-Streamer und spricht sie automatisch an (Outreach mit Tageslimit).
3. **Voice-Reaction** (`voice_reaction/`): nimmt kurze Stream-Audio-Schnipsel auf, transkribiert sie und lässt einen **Claude-Conversation-Brain** eine passende Chat-Reaktion entscheiden — der Bot „hört zu“ und reagiert.

Abgrenzung: Das Senden der Chat-Reaktion läuft über den `RaidChatBot` aus [chat.md](chat.md); `community/` liefert die Inhalte/Entscheidungen.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `TwitchStreamCog` (`TwitchLeaderboardMixin`, `TwitchPartnerRecruitMixin`), `RaidChatBot` (Voice-Reaction-Mixin). |
| **Nutzt** | `storage/` (Stats, Recruit-Log, Voice-State), `api/` (Streams), Anthropic-Claude (Conversation-Brain), `streamlink`/`ffmpeg` (Audio-Capture), Discord. |
| **DB-Tabellen** | Recruit-/Outreach-Log, Voice-Reaction-State + Audit-Log. |
| **Externe Dienste** | Anthropic (Claude-Brain), Whisper/Transkription, Twitch-HLS (Audio), Discord. |
| **Secret-Namen** | `ANTHROPIC_API_KEY` (Brain), ggf. Transkriptions-Key. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `leaderboard.py` | 1371 | Interaktives `!twl`-Leaderboard (Options, View, Mixin). |
| `voice_reaction/scheduler.py` | 829 | Asyncio-Scheduler: Trigger → Capture → Brain → Reaktion. |
| `voice_reaction/state_store.py` | 351 | Persistenter Voice-Reaction-Zustand. |
| `voice_reaction/prompts.py` | 332 | Prompts/Anweisungen fürs Brain. |
| `voice_reaction/conversation_brain.py` | 328 | Claude-Adapter (`ConversationBrain`). |
| `partner_recruit.py` | 322 | Auto-Erkennung + Ansprache frequenter Deadlock-Streamer. |
| `admin.py` | 296 | Community-Admin-Aktionen. |
| `voice_reaction/audio_capture.py` | 269 | Stream-Audio-Capture via streamlink. |
| `voice_reaction/mixin.py` | 187 | Steckt Voice-Reaction in den Chat-Bot. |
| `voice_reaction/chat_message_sender.py` | 146 | Sendet die Reaktion in den Chat. |
| `voice_reaction/discord_notifier.py` | 130 | Discord-Review-Benachrichtigung. |
| `voice_reaction/sanity_filter.py` | 120 | Filtert unpassende Brain-Antworten. |
| `voice_reaction/chat_listener.py` | 117 | Chat-Trigger fürs Voice-Reaction-Gespräch. |
| `voice_reaction/audit_log.py` | 76 | Audit-Trail (Eingabe/Entscheidung/Ausgabe). |

## 4. Datenfluss / Lebenszyklus

**Leaderboard:** `!twl` ruft `TwitchLeaderboardMixin.twitch_leaderboard` → `_compute_stats` aggregiert tracked + Kategorie-Streamer → `TwitchLeaderboardView` rendert eine interaktive Discord-Nachricht (Buttons zum Umsortieren/Filtern via `LeaderboardOptions`).

**Partner-Recruit:** `_run_partner_recruit(category_streams)` läuft auf den Kategorie-Streams: `_detect_recruit_candidates` findet Kanäle, die an genug **distinct Tagen** auftauchten; `_send_partner_outreach` spricht sie an (bis zum Tageslimit, `_count_outreach_sent_today`); `_record_outreach` protokolliert.

**Voice-Reaction:** `VoiceReactionScheduler` (gestartet über `mixin._ensure_voice_reaction_started`) prüft live-Kanäle, nimmt mit `audio_capture.capture(...)` ~N Sekunden Audio auf (streamlink, hartes Kappen bei 1,5×), transkribiert, und ruft `ConversationBrain.respond(...)` (Claude, Tool-Use → `BrainDecision`). `sanity_filter` verwirft unpassende Antworten; passende gehen via `chat_message_sender` in den Chat und/oder als Review an Discord (`discord_notifier`). Alles wird auditiert (`audit_log`).

## 5. Funktionsreferenz pro Bereich

### leaderboard.py
- `LeaderboardOptions` — Sortier-/Filter-Zustand (`clone`, `cycle_sort_key`, `toggle_sort_order`, `cycle_partner_filter`, `cycle_min_samples`, `cycle_min_avg`, `cycle_limit`, `reset`, `clamp` + Label-Helfer).
- `TwitchLeaderboardView(discord.ui.View)` — interaktive Ansicht (`send_initial`, `interaction_check`, Buttons `refresh_button`/`reset_button`/`close_button`).
- `TwitchLeaderboardMixin.twitch_leaderboard(ctx=None, *maybe_filters, filters="")` — der `!twl`-Command; `_compute_stats(*, hour_from=…, …)` aggregiert die Werte.

### partner_recruit.py — `TwitchPartnerRecruitMixin`
- `_run_partner_recruit(category_streams)` — Hauptlauf.
- `_detect_recruit_candidates() -> list[dict]` — Kandidaten nach Auftritts-Tagen.
- `_send_partner_outreach(login, user_id, distinct_days)` — Ansprache.
- `_count_outreach_sent_today() -> int` / `_record_outreach(login, user_id, success)` — Tageslimit + Protokoll.

### voice_reaction/
- `scheduler.py` — `VoiceReactionScheduler` (DI: `chat_bot`, `brain`, `transcribe`, `live_check`, `webhook_url_override`); `VoiceReactionConfig.from_env()`; `_Trigger`. Treibt den Capture→Brain→Reaktion-Zyklus.
- `conversation_brain.py` — `ConversationBrain.respond(*, streamer_context, history, latest_signal_kind, latest_signal_text, latest_signal_meta=None) -> (BrainCallInput, BrainCallOutput)`; nutzt Anthropic-Tool-Use (`_parse_tool_use` → `BrainDecision`), Kosten-/Token-Schätzung; `BrainUnavailable`/`BrainError`.
- `audio_capture.py` — `capture(login, *, duration_seconds, quality, …) -> CaptureResult` (streamlink, `_run_streamlink` kappt bei 1,5×), `cleanup_workdir`, `cleanup_stale_capture_dirs(*, max_age_seconds=3600)` (Boot-GC), `streamlink_bin()`.
- `mixin.py` — `TwitchPartnerVoiceReactionMixin`: `_ensure_voice_reaction_started`, `_shutdown_voice_reaction`, `_open_conversation(login, user_id, *, source, initial_text=None)`, `_voice_reaction_dispatch_message(...)`, `_build_voice_reaction_transcriber`, `_build_voice_reaction_live_check`.
- `state_store.py` — persistenter Zustand (offene Gespräche, Cooldowns). `prompts.py` — Brain-Prompts. `sanity_filter.py` — Antwort-Filter. `chat_message_sender.py` — Reaktion senden. `discord_notifier.py` — Review-Post. `chat_listener.py` — Chat-Trigger. `audit_log.py` — Audit-Trail.

### admin.py
Community-Admin-Aktionen (Steuerung von Recruit/Voice-Reaction).

## 6. Datenbank & externe Schnittstellen

- **DB:** Outreach-/Recruit-Log, Voice-Reaction-State + Audit.
- **Extern:** Anthropic-Claude (Brain), Whisper/Transkription, Twitch-HLS (Audio via streamlink), Discord.

## 7. Stolperfallen / Besonderheiten

- **Voice-Reaction nutzt Claude, nicht MiniMax:** Der Conversation-Brain hängt an `ANTHROPIC_API_KEY`. Das ist die Ausnahme zur „nur MiniMax“-Regel der reinen Text-Features.
- **Audio-Capture ist GC-pflichtig:** Capture-Verzeichnisse müssen aufgeräumt werden (`cleanup_stale_capture_dirs` beim Boot), sonst füllt sich `/tmp`. streamlink wird hart bei 1,5× der Dauer gekappt.
- **ffmpeg-Falle:** Für Twitch-HLS den System-`/usr/bin/ffmpeg` nutzen (`FFMPEG_BIN`); der statische `~/.local`-Build segfaultet (siehe Memory).
- **Sanity-Filter vor dem Senden:** Brain-Antworten gehen erst durch `sanity_filter` — direktes Senden ohne Filter würde unpassende Reaktionen durchlassen.
- **Recruit hat ein Tageslimit:** `_count_outreach_sent_today` begrenzt Ansprachen pro Tag — kein Spam.

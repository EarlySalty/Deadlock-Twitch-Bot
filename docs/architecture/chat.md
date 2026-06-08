# chat/ — Architektur & Funktionsreferenz

> Pfad: `bot/chat/` · Stand: 2026-06-08 · 17 Dateien, ~12.950 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [api.md](api.md) (Bot-Token, Helix-Send), [monitoring.md](monitoring.md) (Live-State, EventSub-Chat), [storage.md](storage.md) (Promo-Cooldowns, Global-Ban), [engagement.md](engagement.md), [LURKER_TAX.md](../LURKER_TAX.md).

## 1. Zweck & Abgrenzung

`chat/` ist der **Twitch-Chat-Bot**: er joint die Partner-Kanäle, **moderiert automatisch**, postet **Promos** und die **Fake-Server-Warnung**, erkennt **Scam-Pitches** von Chattern, trackt **Lurker/Presence** und beantwortet Streamer-Fragen über den Bot. Er läuft als eigener Twitch-Account (Bot-Token), nicht als App-Token.

Wichtige Leitplanken (siehe Memory/Regeln): **keine URLs im Twitch-Chat** (AutoMod) → Verweise laufen über die Bio; Moderation ist **immer an** und konservativ (Anti-Viewer-Bot, Fehlbann fast unmöglich).

Abgrenzung: `chat/` ist die **Echtzeit-Chat-Schicht**. Die KI-Konversation (Stammgast-Persona) liegt in [engagement.md](engagement.md); die Datenanalyse des Chats in [analytics.md](analytics.md).

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `TwitchBaseCog` (erzeugt den Chat-Bot via `create_twitch_chat_bot`), `monitoring/` (live-Kanäle), `raid/`. |
| **Nutzt** | `api/` (Bot-Token-Manager, Helix-Send/Ban/Announcement), `storage/` (Promo-Cooldowns, Global-Ban, Lurker/Presence), `core/` (Partner-Gate, bekannte Bots, Login-Normalisierung), MiniMax (Spam-Review, Targeted-Promo, Self-Explainer). |
| **DB-Tabellen** | Promo-Cooldowns, globale Bannliste, Chat-Messages/Presence, gelernte Spam-/Safe-Muster, Scam-Warn-Timer. |
| **Externe Dienste** | Twitch IRC (`irc.chat.twitch.tv`), Twitch-EventSub-Chat, Twitch-Helix (Ban/Announcement/Send), MiniMax-API. |
| **Secret-Namen** | `TWITCH_BOT_TOKEN`/`TWITCH_BOT_REFRESH_TOKEN` (über `api/token_manager`), MiniMax-Key. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `moderation.py` | 2415 | `ModerationMixin` — Auto-Moderation, Spam-Scoring (Homoglyphen), Ban/Cleanup, Outbound-Send, Streamer-Blacklist. |
| `bot.py` | 2295 | Chat-Bot-Klasse + Factory `create_twitch_chat_bot`, Bot-Token-Laden/-Persistenz. |
| `connection.py` | 2089 | `ConnectionMixin` — IRC-Verbindung, NAMES-Polling, EventSub-Chat-Subscriptions, Observability. |
| `promos.py` | 1671 | Promo-Loop + Fake-Server-Warnung, Lurker-Tax-Reminder, Viewer-Spike-Promo, Cooldowns. |
| `service_pitch_warning.py` | 1031 | Scam-Pitch-Erkennung von Chattern (Account-Alter, Sequenz-/Combo-Signale). |
| `commands.py` | 840 | Chat-/Prefix-Commands (z. B. `!twl`, Raid-Commands). |
| `irc_lurker_tracker.py` | 523 | `IRCLurkerTracker` — zweite, experimentelle IRC-Quelle fürs Lurker-Tracking. |
| `spam_ai_review.py` | 367 | MiniMax-Auto-Improvement des Spam-Filters (lernt Spam- + Safe-Muster). |
| `global_ban_sweep.py` | 317 | Fällige Offline-Sweeps der globalen Bannliste abarbeiten. |
| `constants.py` | 305 | Chat-Konstanten (Join-Verhalten, Limits). |
| `targeted_promo.py` | 283 | Zielgerichtete Discord-Promos mit MiniMax-Preset-Auswahl. |
| `self_explainer.py` | 251 | Grounded Q&A über den Bot (Anti-Injection). |
| `engagement_commands.py` | 209 | Chat-Commands rund um Engagement. |
| `tokens.py` | 150 | Bot-Token-Registrierung mit twitchio. |
| `timeout_guard.py` | 83 | `TimeoutGuard` — Mute-Schwellen + „werbefrei“-Pitch. |
| `lurker_policy.py` | 24 | Policy: wann ist passives Lurken der erwartete Endzustand. |

## 4. Datenfluss / Lebenszyklus

**Start & Verbindung:** `create_twitch_chat_bot(...)` baut die Bot-Instanz (twitchio + Mixins), lädt das Bot-Token (`load_bot_tokens`: ENV/Datei/keyring) und startet die `ConnectionMixin`-Loops. `_connection_loop` hält die IRC-Verbindung mit Auto-Reconnect; `_poll_names_loop` fragt alle 2 min die NAMES-Liste je Kanal ab (Presence/Lurker). Parallel werden — wo verfügbar — EventSub-Chat-Subscriptions registriert.

**Eingehende Nachricht:** `_handle_message` → Moderation: `_score_mention_patterns` + Homoglyph-Normalisierung bilden einen Spam-Score; über der Schwelle greift `_auto_ban_and_cleanup` (Nachricht löschen + Chatter bannen als Bot). Parallel prüft `service_pitch_warning` über Account-Alter und Nachrichten-Sequenz, ob ein **Scam-Pitch** vorliegt, und warnt ggf.

**Promo-Schleife:** `_periodic_promo_loop` prüft regelmäßig alle live-Kanäle. Pro Kanal entscheidet `_promo_activity_ready` (genug frische Chat-Aktivität seit letzter Promo) und Cooldown, ob eine Promo fällig ist. Im fälligen Slot kann statt der Promo die **Fake-Server-Warnung** kommen (`_scam_warning_due` → `_maybe_send_scam_warning`, rotierender Text). Daneben: Lurker-Tax-Reminder, Viewer-Spike-Promo, zielgerichtete Promos (`targeted_promo`).

**Selbstlernen:** Auffällige Nachrichten triggern `run_spam_ai_review` (fire-and-forget): MiniMax bewertet, ob ein neues Spam-Muster oder ein **Safe-Muster** (False-Positive-Whitelist) gelernt werden soll; beides wird in der DB persistiert und beim Scoring berücksichtigt.

**Global-Ban-Sweep:** `run_due_sweeps` arbeitet fällige Offline-Sweeps ab (≈1 h nach Stream-Ende), um Einträge der globalen Bannliste proaktiv in den (offline) Partner-Kanälen zu bannen — koordiniert über die `storage`-Global-Ban-Funktionen.

## 5. Funktionsreferenz pro Datei

### bot.py
- `create_twitch_chat_bot(client_id, client_secret, redirect_uri, raid_bot=None, bot_token=None, bot_refresh_token=None, log_missing=True, token_manager=None) -> RaidChatBot | None` — baut den Chat-Bot (twitchio) mit Bot-Account-Token; `None`, wenn twitchio/Token fehlt.
- `load_bot_tokens(*, log_missing=True) -> (access, refresh, expires)` / `load_bot_token(...)` — Bot-OAuth-Token aus ENV/Datei/keyring laden.
- `TokenPersistenceMixin._persist_bot_tokens(*, access_token, refresh_token, expires_in, scopes=None, user_id=None)` — Tokens im keyring persistieren.
- `_setup_twitch_logging()` — twitchio-Logging konfigurieren. Konstante `TWITCHIO_AVAILABLE` zeigt SDK-Verfügbarkeit.

### connection.py — `ConnectionMixin`
- `_connection_loop()` — IRC-Verbindung mit Auto-Reconnect halten.
- `_read_loop()` / `_handle_message(msg)` — IRC-Frames lesen/verarbeiten.
- `_on_user_join(channel, nick)` / `_on_user_part(channel, nick)` / `_on_names_list(channel, nicks)` — Presence-Events.
- `_update_chatter_seen(channel, nick)` — `last_seen` aktualisieren.
- `_join_channel(channel)` / `_request_names(channel)` / `_poll_names_loop()` — Kanäle joinen, NAMES anfragen (alle 2 min).
- `track_channel(channel, *, mode="partner")` / `untrack_channel(channel)` / `get_chatters(channel)` — Tracking-Liste pflegen.
- EventSub-Chat: `_build_required_chat_subscription_payloads(*, broadcaster_id, user_id)`, `_load_remote_chat_subscription_statuses(...)`, `_refresh_remote_chat_subscription_tracking(...)`, `_has_active_transport_restart()`.
- Observability: `_increment_chat_observability_counter`, `_chat_observability_normalize`, `_format_chat_observability_fields`, `_log_chat_join_decision`, `get_observability_snapshot()`.
- Token-Helfer: `_normalize_managed_twitch_token`, `_normalize_optional_refresh_token`, `_ensure_non_refreshable_twitchio_request_support`.

### moderation.py — `ModerationMixin`
Konstanten: `_SPAM_HOMOGLYPH_TRANSLATION` (Homoglyph-Tabelle via `_build_homoglyph_table`), `_RAW_CHAT_HEALTH_UNSET`, `_OUTBOUND_CHAT_CHANNEL_SETTINGS_SUPPRESSION_SEC`.
- `_score_mention_patterns(content, host_login="", *, allow_host_bonus=False) -> (score, reasons)` — Spam-Score über Mention-/Pattern-Heuristik (homoglyph-normalisiert).
- `_auto_ban_and_cleanup(message, *, ban=True, reason_text=…, notice_text=None, alert_kind="ban") -> bool` — Nachricht löschen + Chatter bannen (als Bot).
- `_send_announcement(channel, text, color="purple", source=None)` — hervorgehobene Announcement via Helix.
- `_send_chat_message(channel, text, source=None)` — Chat-Nachricht best-effort (EventSub-kompatibel).
- `_extract_message_id(message)` — Message-ID für Moderations-APIs.
- Streamer-Blacklist (Selbstschutz): `_blacklist_streamer_for_source(channel, status, text, source)`, `_should_blacklist_for_source(source)`, `_maybe_blacklist_for_drop_reason(...)`, `_is_partner_channel_for_blacklist_skip(login)`, `_is_manual_partner_opt_out_for_chat(channel)` — wenn ausgehende Bot-Nachrichten in einem Kanal scheitern (Bot gebannt/keine Rechte), wird der Kanal für Sends gesperrt.
- `_resolve_existing_twitch_users(logins) -> (gefunden, lookup_ok)` — Logins via Twitch auflösen.
- Chat-Health-Logging: `_chat_health_message_shape(...)`, `_log_chat_health_event(...)`.

### promos.py
Promo-/Werbe-Logik + die Fake-Server-Warnung.
- `_periodic_promo_loop()` — Hauptschleife: prüft regelmäßig, ob in einem Kanal eine Promo fällig ist.
- `_send_promo_if_due()` / `_send_promo_message(login, channel_id, now, *, reason)` / `_maybe_send_promo_with_stats(...)` — Promo senden.
- `_maybe_send_viewer_spike_promo(...)`, `_maybe_send_lurker_tax_reminder(...)`, `_maybe_send_activity_promo(message)` — Spezial-Promos.
- Fake-Server-Warnung: `_scam_warning_due(login, now, *, reason)`, `_build_scam_warning_text(login, invite)` (rotiert, nie zweimal hintereinander derselbe), `_maybe_send_scam_warning(...)`.
- Gating: `_promo_blocked_by_plan_or_flag(login)`, `_promo_activity_ready(login, now)`, `_has_recent_chat_activity(login, now)`, `_latest_chat_activity_age_sec(...)`, `_get_viewer_spike_context(login)`.
- Cooldowns: `_mark_promo_sent(...)`, `_restore_promo_cooldowns()` (aus DB), `_build_promo_text(...)`.
- Kanal-Listen: `_get_live_channels_for_promo()`, `_get_live_channels_for_lurker_tax()`.

### service_pitch_warning.py
Erkennt Scam-/Service-Pitches von Chattern und warnt.
- `_maybe_warn_service_pitch(message, *, channel_login) -> bool` — Hauptentscheidung.
- `_get_account_age_days(author_id, author_login)` — Account-Alter (neue Accounts sind verdächtiger).
- Scoring: `_score_sequence_signals(...)`, `_score_combo_signals(features)`, `_early_window_score(...)`, `_has_high_confidence_single_message_signal(features)`, `_is_quick_action_eligible(*, is_new_account, is_first_observed_message)`, `_is_benign_social_checkin(content, features)`.
- `_build_service_warning_text(*, chatter_login, strong, new_account, account_age_days)` — Warntext.
- Verlauf/State: `_observe_service_message_position(...)`, Aktivitäts-/Message-History-Buckets (`_get_service_*_bucket`, `_prune_*`), `_get_streamer_followers_hint(...)`, `_token_count(content)`.

### commands.py / engagement_commands.py
Chat-/Prefix-Commands (z. B. `!twl` für Leaderboard-Stats, Raid-Commands, Admin-Aktionen) sowie engagement-bezogene Commands. Die Commands sind als Mixin in den Chat-Bot eingebunden.

### irc_lurker_tracker.py — `IRCLurkerTracker`
Zweite, experimentelle IRC-Verbindung nur fürs Lurker-Tracking + Presence-Snapshots.
- `start()` / `stop()` / `_connect()` / `_disconnect()` / `_connection_loop()` — eigene IRC-Verbindung mit Reconnect, unabhängig vom Haupt-Bot.

### lurker_policy.py
- `is_passive_lurker_channel(*, is_monitored_only, is_partner_active, has_raid_auth) -> bool` — True, wenn passives Beobachten der erwartete Endzustand ist (kein Chat-Runtime nötig).
- `should_attempt_runtime_heal(*, is_monitored_only, is_ready) -> bool` — Monitored-only-Lurker-Kanäle werden **nicht** als Heal-Ziel behandelt.

### spam_ai_review.py
Selbstlernender Spam-Filter via MiniMax M3.
- `run_spam_ai_review(*, content, channel, chatter_login, spam_score, spam_reasons)` — Fire-and-forget-Entrypoint (`asyncio.create_task`).
- `_review_worthwhile(content, spam_reasons)` / `_should_review_now(channel, chatter_login)` — Review-Gating (Cooldown `_REVIEW_COOLDOWN_SEC`).
- `_call_minimax(content)` — MiniMax-Aufruf.
- `load_learned_patterns()` / `load_safe_patterns()` — gelernte Spam- bzw. Safe-Muster aus DB (gecacht, TTL `_PATTERN_CACHE_TTL`).
- `_save_pattern(...)` / `_save_safe_pattern(...)` — neue Muster persistieren; `_invalidate_pattern_cache`/`_invalidate_safe_cache`.

### global_ban_sweep.py
- `run_due_sweeps(chat_bot) -> int` — fällige Stream-Ende-Sweeps (≈1 h nach Offline) abarbeiten und Global-Ban-Einträge proaktiv in den Kanälen bannen.

### self_explainer.py
Grounded Q&A über den Bot. Konstanten: `BOT_FACTS`, `_SYSTEM_PROMPT`, `MAX_QUESTION_LEN`, `MAX_ANSWER_LEN`, `SPLIT_LIMIT`, `_INJECTION_PATTERNS`.
- `build_system_prompt() -> str` — System-Prompt aus den Bot-Fakten.
- `looks_like_injection(question) -> bool` — Prompt-Injection-Heuristik.
- `SelfExplainerAnswer` — Ergebnis-Container. (Aufgerufen von der Website-Frage-Box und dem Self-Explainer-Endpoint; Chat-Antworten laufen Shadow.)

### targeted_promo.py
Zielgerichtete Discord-Promos. Konstanten: `_STAMMGAST_MIN_MESSAGES`, `_STAMMGAST_DAYS`, `_USER_PITCH_COOLDOWN_SEC`, `_CHANNEL_TARGETED_COOLDOWN_SEC`.
- `maybe_send_targeted_promo(*, bot, channel_login, channel_id, active_chatters, invite_url, now) -> bool` — versucht einen zielgerichteten oder globalen Pitch.
- `_pick_user_target(active_chatters, channel_login, now)` — wählt einen Chatter, der heute noch nicht gepitcht wurde und kein Stammgast ist.
- `_pick_preset_with_minimax(...)` — MiniMax wählt das passende Promo-Preset (antwortet nur mit Preset-ID).
- `_sync_is_stammgast(twitch_user_id, channel_login)` / `_sync_user_context_snippets(...)` — Stammgast-Check + Kontext-Snippets.

### timeout_guard.py — `TimeoutGuard`
Mute-Schwellen + „werbefrei“-Pitch. Konstanten `_MUTE_DAILY_THRESHOLD`, `_MUTE_WEEKLY_THRESHOLD`, `_MUTE_DURATION_SEC`, `WERBEFREI_PITCH_*`, `_BOT_TIMEOUT_DROP_CODES`.

### tokens.py
Bot-Token-Registrierung mit twitchio (`_register_bot_token_with_twitchio`) + verwandte Token-Helfer.

### constants.py
Chat-Konstanten, u. a. `CHAT_JOIN_OFFLINE` (ob offline-Kanäle gejoined werden).

## 6. Datenbank & externe Schnittstellen

- **DB:** Promo-Cooldowns (`storage.promo_cooldowns`), globale Bannliste + Sweeps (`storage` Global-Ban), Chat-Messages/Presence, gelernte Spam-/Safe-Muster, Scam-Warn-Timer (überlebt Neustarts).
- **Twitch:** IRC (`irc.chat.twitch.tv`), EventSub-Chat, Helix (Ban, Announcement, Chat-Send) über `api/`.
- **MiniMax:** Spam-Review, Targeted-Promo-Preset-Wahl, Self-Explainer-Antworten.
- **Discord:** Promo-/Warn-Texte verweisen auf den Community-Discord (über Bio/Invite, **nicht** als Chat-URL).

## 7. Stolperfallen / Besonderheiten

- **Keine URLs im Chat:** Twitch-AutoMod blockt Links. Promos/Warnungen verweisen über die Bio bzw. den Invite-Mechanismus, nie als nackte URL — sonst verschwindet die Nachricht.
- **Fake-Server-Warnung „stiehlt“ den Promo-Slot:** Ist `_scam_warning_due`, kommt im fälligen Slot **statt** der Promo die Warnung — nicht zusätzlich. Sonst doppelte Bot-Nachrichten.
- **Outbound-Fehler → Streamer-Blacklist:** Schlägt ein Bot-Send fehl (Bot gebannt/keine Mod-Rechte), sperrt `_blacklist_streamer_for_source` den Kanal für weitere Sends — bewusst, um nicht gegen eine Wand zu spammen. Partner mit Opt-out werden übersprungen.
- **Homoglyph-Normalisierung:** Spam-Scoring übersetzt Homoglyphen vor dem Matching (`_SPAM_HOMOGLYPH_TRANSLATION`) — kyrillische/ähnliche Zeichen umgehen den Filter sonst.
- **Self-Learning kann False-Positives erzeugen:** Deshalb lernt `spam_ai_review` auch **Safe-Muster** (Whitelist). Beim Tunen beide Tabellen betrachten, nicht nur die Spam-Muster.
- **Zwei IRC-Quellen:** Haupt-`ConnectionMixin` **und** `IRCLurkerTracker` verbinden sich per IRC. Wer Presence-Daten debuggt, muss wissen, aus welcher Quelle sie stammen.
- **Self-Explainer ist im Chat Shadow:** Antworten gehen (laut Rollout) zunächst nur ins Logging/Discord, nicht live in den Twitch-Chat — die Website-Frage-Box antwortet dagegen direkt.

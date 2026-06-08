# api/ — Architektur & Funktionsreferenz

> Pfad: `bot/api/` · Stand: 2026-06-08 · 5 Dateien, ~3.180 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [bot-core.md](bot-core.md) (`secret_store`, Token-Bootstrap), [monitoring.md](monitoring.md) (EventSub-Konsumenten), [raid.md](raid.md) (per-Streamer-OAuth), [BOT_TOKEN_SCOPES.md](../BOT_TOKEN_SCOPES.md).

## 1. Zweck & Abgrenzung

`bot/api/` ist die **Twitch-Anbindung**: alles, was direkt mit Twitch-Helix oder mit Twitch-OAuth-Tokens spricht. Drei klar getrennte Aufgaben:

1. **Helix-Wrapper** (`twitch_api.py`): async Aufrufe gegen `https://api.twitch.tv/helix` mit App-Access-Token (Client-Credentials) für lesende Calls und mit per-Streamer-User-Token für autorisierte Calls (Subs, Ads, Chatters, Clip-Erstellung, EventSub).
2. **Bot-Chat-Token** (`token_manager.py`): verwaltet das User-Token des zentralen Chat-Bot-Accounts inkl. automatischem Refresh und Persistenz.
3. **Token-Fehler-Lebenszyklus** (`token_error_handler.py`): was passiert, wenn der Token eines Streamers wiederholt scheitert — Blacklist, Grace-Period, Re-Auth-Aufforderung, Bot-Ban-Erkennung, Discord-Benachrichtigungen.

Abgrenzung: `bot/api/` macht **keine** Geschäftslogik (kein Raid-Scoring, kein Analytics). Es liefert nur saubere Twitch-Daten und kümmert sich um Token-Gesundheit. Die OAuth-*Flows* (Authorize-URL, Callback) für einzelne Streamer liegen in [raid/auth.py](raid.md); hier werden deren Tokens nur *benutzt* und ihre *Fehler* behandelt.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `monitoring/` (Stream-Polling, EventSub-Subscriptions), `raid/` (Clips, Chatters, per-Streamer-Token-Aufrufe), `analytics/` & `dashboard/` (Follower-/Sub-/Ad-Zahlen), `chat/` (Bot-Token über `token_manager`). |
| **Nutzt selbst** | `bot.secret_store` (`keyring_enabled`), `bot.discord_role_sync` (Live-Rollen-Sync bei Re-Auth), extern `aiohttp`, optional `keyring`, `discord` (für DMs/Channel-Posts). |
| **DB-Tabellen** | `twitch_token_blacklist` (Fehler-Counter, Grace-Period, Notify-Flags), lesend Streamer-Tabellen für `discord_user_id`. |
| **Externe Dienste** | Twitch-OAuth (`https://id.twitch.tv/oauth2/token`), Twitch-Helix (`https://api.twitch.tv/helix`), Discord (DMs + Admin-Channel). |
| **Secret-Namen** | `TWITCH_BOT_TOKEN`, `TWITCH_BOT_REFRESH_TOKEN`, `TWITCH_BOT_TOKEN_FILE` (Bot-Token); Twitch-Client-ID/-Secret werden injiziert. Keyring-Service: `DeadlockBot`. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `token_error_handler.py` | 1403 | Lebenszyklus für scheiternde Streamer-Tokens: Blacklist, Grace-Period, Re-Auth, Bot-Ban, Discord-Notifications. |
| `twitch_api.py` | 1300 | `TwitchAPI` — async Helix-Wrapper mit App-Token, Retries, Fehler-Mapping und allen genutzten Endpoints. |
| `token_manager.py` | 440 | `TwitchBotTokenManager` — Auto-Refresh + Persistenz des zentralen Chat-Bot-Tokens; OAuth-Code-Tausch. |
| `twitch_auth.py` | 40 | Geteilte OAuth-Credential-Helfer + `TwitchClientConfigError`. |
| `__init__.py` | 1 | Package-Marker. |

## 4. Datenfluss / Lebenszyklus

**App-Token (lesend):** `TwitchAPI._ensure_token()` holt bei Bedarf ein App-Access-Token via Client-Credentials von `TWITCH_TOKEN_URL`. Scheitert die Credential-Prüfung (ungültige Client-ID/Secret), wird Auth für **900 s** blockiert (`_block_auth`), damit nicht in einer Schleife gegen die Wand gelaufen wird. Helix-Requests (`_get`/`_post`) laufen mit Backoff über bis zu `max_attempts` (Default 3, max 5) Versuche; HTTP 429 wird als `helix_429_rate_limited` gemappt.

**Bot-Chat-Token (refreshend):** `TwitchBotTokenManager.initialize()` lädt Tokens (ENV → Token-Datei → keyring), validiert sie und startet `_auto_refresh_loop()`. Die Loop refresht **vor** Ablauf (anhand `time_until_expiry`), persistiert neue Tokens (keyring/Credential-Manager) und ruft einen registrierten Refresh-Callback (für die laufende Chat-Verbindung).

**Streamer-Token-Fehler (eskalierend):** Wenn ein per-Streamer-Token beim Refresh scheitert, ruft der Aufrufer `TokenErrorHandler.add_to_blacklist()`. Pro `twitch_user_id` läuft ein Fehler-Counter:
1. Unter der Schwelle (`< BLACKLIST_DISABLE_THRESHOLD = 3`) gilt ein Cooldown (`RETRY_COOLDOWN_HOURS = 2`), bevor erneut versucht wird.
2. Ab **3** aufeinanderfolgenden Fehlern (Fenster `CONSECUTIVE_FAILURE_WINDOW_HOURS = 12`) wird der Token gesperrt, der Raid-Bot für diesen Streamer deaktiviert (`_disable_raid_bot`), Re-Auth markiert (`_mark_reauth_required`) und eine **Grace-Period** von `GRACE_PERIOD_DAYS = 7` Tagen gesetzt.
3. Der Streamer bekommt eine Discord-DM mit konkreten Schritten; läuft die Grace-Period ab (`check_grace_periods()`, stündlich), wird der Admin-Channel (`TOKEN_ERROR_CHANNEL_ID`) informiert und die Live-Rolle entzogen.

Ein **kanalseitiger Bot-Ban** ist ein Sonderfall: `handle_bot_banned_channel()` behandelt ihn wie ein temporäres Opt-out (ohne harten Re-Auth-Zwang) und schickt Recovery-Schritte; `restore_bot_banned_channel()` hebt das auf, sobald der Bot wieder gesund ist.

## 5. Funktionsreferenz pro Datei

### twitch_auth.py
- `TwitchClientConfigError(RuntimeError)` — wird geworfen, wenn Twitch-Client-Credentials fehlen oder von Twitch selbst abgelehnt werden.
- `normalize_twitch_credential(value: str | None) -> str` — säubert aus ENV gelesene Credentials (Whitespace etc.).
- `is_invalid_client_response(status: int, body: str | None) -> bool` — True, wenn Twitchs Antwort die Credentials selbst ablehnt (zur Unterscheidung von „nur abgelaufenes Token“).

### twitch_api.py
Konstanten: `TWITCH_TOKEN_URL`, `TWITCH_API_BASE` (Helix).

`TwitchAPI` — async Helix-Wrapper mit App-Access-Token. Als Async-Context-Manager nutzbar (`__aenter__`/`__aexit__`).

*Session & Auth-Schutz:*
- `__init__(client_id, client_secret, session=None)` — optional fremde aiohttp-Session; sonst lazy eigene.
- `get_http_session()` / `aclose()` — Session holen/schließen; `_ensure_session`, `_is_closed_session_error` als Helfer.
- `is_auth_blocked()` / `_block_auth(reason, *, cooldown_seconds=900.0)` / `_raise_if_auth_blocked()` — Auth-Block-Mechanik: nach ungültigen Credentials wird Auth für 15 min gesperrt; ein erfolgreicher Token-Hol hebt den Block auf.
- `_ensure_client_credentials()` / `_ensure_token()` — stellt gültige Credentials bzw. ein frisches App-Token sicher.
- `_headers()` / `_normalize_bearer_token(token)` — Request-Header inkl. Bearer.

*Fehler-/Ergebnis-Mapping:*
- `_helix_result(*, ok, data, http_status, error_code, message, request_attempted)` (classmethod) — einheitliches strukturiertes Ergebnis-Dict.
- `_map_helix_error_code(*, http_status, message, default)` — mappt HTTP-Status/Message auf einen stabilen Fehlercode (z. B. 429 → `helix_429_rate_limited`).
- `_sanitize_error_text(text, *, limit=240)` / `_format_exception_summary(exc)` — sichere, gekappte Fehlertexte fürs Logging (keine Token-Leaks).
- `_post(path, json=None, *, log_on_error=True, oauth_token=None, max_attempts=3, request_timeout_total=None)` / `_get(path, params=None, *, log_on_error=True)` — die zwei Transport-Kerne mit Retry/Backoff (1–5 Versuche) und Token-Wahl (App- oder mitgegebenes User-Token).

*Such-/Stream-Endpoints:*
- `search_category_id(query)` / `get_category_id(name)` — Helix-Kategorie-Lookup (z. B. „Deadlock“ → game_id).
- `get_users(logins)` / `get_user_info(login)` — Userdaten (inkl. Bio/Description).
- `get_app_access_token()` — gültiges App-Token holen/zurückgeben.
- `_fetch_stream_page(...)` (privat, paginiert) → `get_streams_for_game(*, game_id, game_name, language=None, limit=500)`, `get_streams_by_logins(logins, language=None)`, `get_streams_by_category(category_id, language=None, limit=500)` — Live-Stream-Listen (für Monitoring/Stats).
- `get_archive_videos(user_id, first=20)` / `get_latest_vod_thumbnail(*, user_id=None, login=None)` — VODs bzw. bestes Thumbnail (1280×720).

*Autorisierte (User-Token-)Endpoints:* jeweils als „Roh“- und als `*_result`-Variante (strukturiertes Ergebnis):
- `create_clip(broadcaster_id, *, user_token, title=None, duration=None, has_delay=False)` — Clip mit User-OAuth-Token erstellen.
- `get_followers_total[_result](user_id, user_token=None)` — Follower-Gesamtzahl (best-effort über `/channels/followers`).
- `get_broadcaster_subscriptions[_result](user_id, user_token)` — Subs.
- `get_ad_schedule[_result](user_id, user_token)` — Werbe-Schedule.
- `get_chatters[_result](broadcaster_id, moderator_id, user_token, first=1000)` — aktuelle Chatter-Liste.

*EventSub-Verwaltung* (siehe [monitoring.md](monitoring.md)):
- `subscribe_eventsub_websocket(*, session_id, sub_type, condition, version="1", oauth_token=None)` — WS-Subscription (z. B. `stream.offline`).
- `subscribe_eventsub_webhook(*, sub_type, condition, webhook_url, secret, version="1", oauth_token=None)` — Webhook-Subscription.
- `delete_eventsub_subscription(subscription_id, oauth_token=None)` / `list_eventsub_subscriptions(*, status="enabled", oauth_token=None)` — löschen/auflisten (paginiert).

### token_manager.py
`TwitchBotTokenManager` — Token-Verwaltung des zentralen Chat-Bot-Accounts mit Auto-Refresh und Persistenz.

- `__init__(client_id, client_secret, *, keyring_service="DeadlockBot")`.
- `set_refresh_callback(callback)` — registriert eine Coroutine, die nach jedem erfolgreichen Refresh mit `(access_token, refresh_token, expires_at)` aufgerufen wird (damit die laufende Chat-Verbindung das neue Token übernimmt).
- `initialize(access_token=None, refresh_token=None) -> bool` — lädt/übernimmt Tokens, validiert sie und startet die Refresh-Loop.
- `get_valid_token(force_refresh=False) -> tuple[str, str | None]` — liefert ein gültiges Access-Token (refresht bei Bedarf).
- `_validate_and_fetch_info()` — validiert das Token bei Twitch und holt Bot-Metadaten.
- `_refresh_access_token()` — Refresh über das Refresh-Token; setzt neue `expires_at`.
- `_auto_refresh_loop()` — Hintergrund-Task, der vor Ablauf refresht.
- `_load_tokens()` — Quelle in Reihenfolge: ENV (`TWITCH_BOT_TOKEN`/`TWITCH_BOT_REFRESH_TOKEN`), Token-Datei (`TWITCH_BOT_TOKEN_FILE`), keyring.
- `_save_tokens()` — persistiert nach keyring (Windows Credential Manager), wenn verfügbar.
- `cleanup()` — stoppt die Refresh-Loop.
- `generate_oauth_tokens(client_id, client_secret, authorization_code, redirect_uri) -> dict` (Modul-Funktion) — tauscht einen OAuth-Authorization-Code gegen Access-/Refresh-Token.
- `_exc_name(exc)` (privat) — Exception-Klassenname fürs Logging ohne Secret-Payload.

### token_error_handler.py
Konstanten: `TOKEN_ERROR_CHANNEL_ID = 1374364800817303632`, `GRACE_PERIOD_DAYS = 7`. Klassenattribute: `BLACKLIST_DISABLE_THRESHOLD = 3`, `CONSECUTIVE_FAILURE_WINDOW_HOURS = 12`, `RETRY_COOLDOWN_HOURS = 2`.

`TokenErrorHandler` — verwaltet Token-Fehler und verhindert endlose Refresh-Versuche.

- `__init__(discord_bot=None)` — optional ein Discord-Client für DMs/Channel-Posts.
- `_migrate_db()` (staticmethod) — fügt idempotent neue Spalten zu `twitch_token_blacklist` hinzu.
- `add_to_blacklist(twitch_user_id, twitch_login, error_message)` — legt Eintrag an oder erhöht den Fehler-Counter; ab Schwelle 3 wird gesperrt + Raid-Bot deaktiviert + Grace-Period gesetzt.
- `is_token_blacklisted(twitch_user_id) -> bool` — True bei `>= BLACKLIST_DISABLE_THRESHOLD` Fehlern.
- `has_recent_failure(twitch_user_id) -> bool` — True, wenn innerhalb `RETRY_COOLDOWN_HOURS` ein Fehler war (nur relevant solange noch nicht voll blacklisted).
- `clear_failure_count(twitch_user_id)` / `remove_from_blacklist(twitch_user_id)` — Reset nach erfolgreichem Refresh bzw. Re-Auth.
- `cleanup_old_entries(days=30)` — alte Einträge entfernen.
- `_mark_reauth_required(...)` — sperrt Twitch-Auth-Nutzung, bis der Streamer im Dashboard re-autorisiert.
- `_mark_partner_opt_out_only(...)` — deaktiviert Partner-Bot-Features ohne harten Re-Auth-Zwang.
- `_disable_raid_bot(twitch_user_id)` — hält Raid/Auth deaktiviert bei wiederholten Fehlern.
- `handle_bot_banned_channel(...)` / `restore_bot_banned_channel(...)` — kanalseitigen Bot-Ban als temporäres Opt-out behandeln bzw. zurücknehmen.
- `notify_token_error(...)` / `_send_user_dm_token_error(..., is_reminder=False)` / `_send_user_dm_bot_banned(...)` — Discord-Benachrichtigungen (Admin-Channel + DM an Streamer mit Recovery-Schritten).
- `check_grace_periods()` — stündlich; prüft abgelaufene Grace-Periods, schickt Reminder bzw. eskaliert an den Admin-Channel und entzieht die Live-Rolle.
- `_notify_admin_grace_expired(...)` / `_get_discord_user_id(...)` / `schedule_streamer_role_sync(...)` / `_normalize_discord_user_id(...)` — Hilfen für Admin-Notify, Discord-ID-Lookup und Rollen-Sync.
- `_mask_log_identifier(value, *, visible_prefix=3, visible_suffix=2)` (Modul-Funktion) — maskiert IDs im Log.

## 6. Datenbank & externe Schnittstellen

- **DB:** `twitch_token_blacklist` (Spalten u. a. Fehler-Counter, `error_message`, `notified`, `reminder_sent`, Grace-Timestamp; idempotent migriert). Lesend: Streamer-Tabelle für `discord_user_id`.
- **Twitch-OAuth:** `https://id.twitch.tv/oauth2/token` (App-Token, Bot-Token-Refresh, Code-Tausch).
- **Twitch-Helix:** `https://api.twitch.tv/helix` — Endpoints: `users`, `streams`, `games`/`search/categories`, `clips`, `videos`, `channels/followers`, `subscriptions`, `channels/ads`, `chat/chatters`, `eventsub/subscriptions`.
- **Discord:** DMs an Streamer, Posts in `TOKEN_ERROR_CHANNEL_ID`.

## 7. Stolperfallen / Besonderheiten

- **Drei Token-Typen nicht verwechseln:** (1) App-Token (`twitch_api`, lesend), (2) zentrales Bot-Chat-Token (`token_manager`, refreshend), (3) per-Streamer-User-Tokens (in `raid/auth.py` erzeugt, hier nur benutzt + Fehler behandelt). `token_error_handler` betrifft ausschließlich Typ 3.
- **Auth-Block ist absichtlich „stumm“:** Bei ungültigen Client-Credentials blockt `TwitchAPI` Auth 900 s und loggt den Grund nur einmal — verhindert Log-Spam und Rate-Limit-Strafen.
- **Eskalation ist gestaffelt, nicht sofort:** Erst ab 3 aufeinanderfolgenden Fehlern (innerhalb 12 h) greift Sperre/Re-Auth; davor nur 2-h-Cooldown. Das schützt vor Fehlalarmen bei kurzen Twitch-Aussetzern.
- **Bot-Ban ≠ Token-Fehler:** Ein kanalseitiger Bot-Ban erzwingt **keine** Re-Auth (das Token ist gültig), sondern nur ein temporäres Opt-out — sonst würde der Streamer fälschlich zum Neu-Verbinden aufgefordert.
- **Persistenz ist plattformabhängig:** `_save_tokens` nutzt keyring/Windows Credential Manager; auf Hosts ohne keyring (`keyring_enabled()` falsch) wird das Bot-Token **nicht** persistiert und muss aus ENV/Datei kommen.
- **Re-Auth läuft über das Dashboard:** `_mark_reauth_required` sperrt die Nutzung; die eigentliche Neu-Autorisierung passiert im Streamer-Dashboard (OAuth-Flow in `raid/`).

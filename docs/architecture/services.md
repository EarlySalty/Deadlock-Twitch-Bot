# bot_service/ + dashboard_service/ — Architektur & Funktionsreferenz

> Pfad: `bot/bot_service/` (3 Dateien, ~117 Z.) + `bot/dashboard_service/` (5 Dateien, ~1.480 Z.) · Stand: 2026-06-08
>
> Teil der [Architektur-Doku](README.md). Verwandt: [ARCHITECTURE.md](../ARCHITECTURE.md) (Startpfade), [bot-core.md](bot-core.md) (Runtime), [internal-api.md](internal-api.md), [dashboard.md](dashboard.md), [monitoring.md](monitoring.md) (EventSub).

## 1. Zweck & Abgrenzung

Diese beiden Pakete sind die **eigenständigen Service-Entrypoints** des Split-Runtime:

- **`bot_service/`** startet die **BotRuntime** (Twitch-Worker) — ohne Discord-Gateway, als `HeadlessBot`-Stub.
- **`dashboard_service/`** startet die **DashboardRuntime** (aiohttp-Web-App) und bindet den Bot **nur** über die interne API an (kein Bot-Import).

Abgrenzung: Sie enthalten kaum Fachlogik — sie **wiren** den jeweiligen Runtime-Pfad zusammen und halten ihn am Leben. Die Logik liegt in `bot/` (Cog) bzw. `bot/dashboard/`.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Eintritt** | `python -m bot.bot_service` bzw. `python -m bot.dashboard_service` (CLI in `__main__.py`). |
| **bot_service nutzt** | `TwitchStreamCog` + `HeadlessBot` (discord-loser Stub), `internal_api` (Host). |
| **dashboard_service nutzt** | `dashboard/server_v2.build_v2_app`, `BotApiClient` (interne API), `eventsub_bridge`. |
| **Externe Dienste** | Twitch (bot), keine direkten im Dashboard (alles über interne API). |
| **Secret-Namen** | Runtime-Rollen/Ports (ENV), interne API-Tokens, OAuth-Credentials. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `bot_service/app.py` | 86 | `HeadlessBot` + `run_bot_service()`. |
| `bot_service/__main__.py` | 29 | CLI-Entrypoint (`main()`). |
| `dashboard_service/app.py` | 901 | `build_dashboard_service_app(...)` + `run_dashboard_service(...)`. |
| `dashboard_service/eventsub_bridge.py` | 477 | `DashboardEventSubBridge` — EventSub auf Dashboard-Seite → Bot. |
| `dashboard_service/client.py` | 62 | HTTP-Client (Bot-Operationen). |
| `dashboard_service/__main__.py` | 29 | CLI-Entrypoint (`main()`). |

## 4. Datenfluss / Lebenszyklus

**Bot-Service:** `bot_service/__main__.main()` → `run_bot_service(port=…)`. Da kein Discord-Gateway gebraucht wird, instanziiert es einen **`HeadlessBot`** — einen Stub, dessen Discord-Methoden (`get_channel`, `fetch_user`, `add_view`, `load_extension`, …) No-Ops sind bzw. `None` liefern. So läuft `TwitchStreamCog` (Monitoring/Raid/Chat/Analytics-Sammeln + interne API) ohne echten Discord-Client. `wait_until_ready()` signalisiert Bereitschaft; der Service läuft bis Cancel.

**Dashboard-Service:** `dashboard_service/__main__.main()` → `run_dashboard_service(host, port)`. `build_dashboard_service_app(*, internal_api_base_url, internal_api_token, dashboard_token, partner_token, noauth, oauth_*, session_ttl_seconds, legacy_stats_url)` baut die Dashboard-App (`build_v2_app`) und verdrahtet die Callbacks **durch einen `BotApiClient`** an die interne API — der Dashboard-Prozess besitzt keine Bot-Objekte. `_require_noauth_opt_in_if_enabled` erzwingt, dass No-Auth nur per explizitem Opt-in (+ Loopback) möglich ist.

**EventSub-Brücke:** Trifft ein EventSub-Event den Dashboard-Prozess, nimmt `DashboardEventSubBridge.dispatch_or_enqueue(...)` es entgegen und leitet es an den Bot weiter (über die interne API) — durable mit Retry (`_process_due_batch`, `_retry_delay_seconds`), und tolerant gegenüber „Bot startet noch“-Fehlern (`_is_startup_pending_error`).

## 5. Funktionsreferenz

### bot_service/
- `run_bot_service(*, port=None)` — startet den Worker-Service bis Cancel.
- `HeadlessBot` — discord-loser Stub: `wait_until_ready`, `is_closed`, `get_guild/get_channel/fetch_channel/get_user/fetch_user` (No-Op/None), `add_view`, `get_command/remove_command`, `load_extension/unload_extension`.
- `__main__.main()` — CLI-Einstieg.

### dashboard_service/
- `build_dashboard_service_app(*, internal_api_base_url=None, internal_api_token=None, internal_api_allow_non_loopback=None, internal_api_timeout_seconds=None, dashboard_token=None, partner_token=None, noauth=None, oauth_client_id=None, oauth_client_secret=None, oauth_redirect_uri=None, session_ttl_seconds=None, legacy_stats_url=None) -> web.Application` — baut die App, verdrahtet die Callbacks über den `BotApiClient`.
- `run_dashboard_service(*, host=None, port=None, app=None)` — bis Cancel laufen lassen.
- ENV-Helfer: `_dashboard_host_setting`, `_default_internal_api_base_url`, `_require_noauth_opt_in_if_enabled`, `_parse_env_bool/int/float`.
- `eventsub_bridge.py` — `DashboardEventSubBridge(*, client, logger=None, store=None, now=None)`: `start`/`stop`/`active`, `dispatch_or_enqueue(*, sub_type, payload, message_id)`, `_run`/`_process_due_batch`/`_dispatch_once`; `DashboardEventSubBridgeStore` (durable Queue). `client.py` — HTTP-Client für Bot-Operationen.
- `__main__.main()` — CLI-Einstieg.

## 6. Datenbank & externe Schnittstellen

- **bot_service:** hostet die interne API (8776), spricht Twitch; nutzt `storage`.
- **dashboard_service:** spricht den Bot **nur** über `http://127.0.0.1:8776` (interne API); serviert das Dashboard (Port 8765). EventSub-Brücke nutzt eine durable Store-Tabelle.

## 7. Stolperfallen / Besonderheiten

- **`HeadlessBot` ist Absicht:** Im Bot-Service gibt es keinen echten Discord-Client — Discord-abhängige Pfade müssen No-Op-tolerant sein. Wer „warum kommt kein Discord-Post?“ debuggt: im reinen Bot-Service gehen Postings über den Master-Broker, nicht über `discord_bot`.
- **Dashboard besitzt keinen Bot:** Alle Bot-Daten kommen über die interne API. Ein `None` aus `DashboardBotService` ist normal (siehe [bot-core.md](bot-core.md)).
- **No-Auth nur mit Opt-in:** `_require_noauth_opt_in_if_enabled` blockt ein versehentlich offenes Dashboard.
- **EventSub-Brücke toleriert Startup-Races:** `_is_startup_pending_error` verhindert, dass Events verloren gehen, wenn der Bot noch hochfährt — sie werden re-enqueued.
- **Startpfade:** `python -m bot.bot_service` / `python -m bot.dashboard_service` (systemd-User-Services, Restart via `systemctl --user restart …`).

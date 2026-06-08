# core/ — Architektur & Funktionsreferenz

> Pfad: `bot/core/` · Stand: 2026-06-08 · 7 Dateien, ~1.260 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [storage.md](storage.md) (DB-Schicht), [api.md](api.md) (Twitch-API), [internal-api.md](internal-api.md) (Gegenstelle des HTTP-Clients), [bot-core.md](bot-core.md) (`secret_store`).

## 1. Zweck & Abgrenzung

`bot/core/` ist die **geteilte Low-Level-Schicht** des Bots: kleine, zustandsarme Helfer, die quer von Chat, Raid, Monitoring, Analytics und Dashboard benutzt werden. Hier liegt **keine Fachlogik** eines einzelnen Features, sondern Querschnitt:

- **Konfiguration** (`constants.py`): alle nicht-geheimen Stellschrauben an einem Ort.
- **Identitäts-Normalisierung** (`twitch_login.py`, `chat_bots.py`): Twitch-Logins kanonisieren, bekannte Service-Bots erkennen.
- **Partner-Status** (`partner_utils.py`): die zentrale Wahrheit „ist dieser Kanal ein aktiver Partner?“.
- **LLM-Zugang** (`llm_providers.py`): einheitliche Client-Factories für Anthropic/OpenAI/MiniMax.
- **HTTP-Transport** (`http_client.py`): die abgesicherte Basisklasse aller Clients, die mit der internen Bot-API sprechen.

Abgrenzung: `core/` ruft **nach unten** nur `storage` (DB) und `secret_store` auf, aber **nie** Chat/Raid/Analytics. Damit bleibt es importsicher und frei von Zyklen.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | nahezu allen Subsystemen — `chat/` (Lurker-/Bot-Erkennung, Login-Normalisierung), `raid/` & `monitoring/` (Partner-Gate), `analytics/` (Bot-Ausschluss in SQL), `dashboard_service/` (HTTP-Client gegen interne API), `engagement/` & `title_generator/` (LLM-Clients). |
| **Nutzt selbst** | `bot.storage` (`readonly_connection`), `bot.secret_store` (`keyring_enabled`), extern `aiohttp`, optional `anthropic`/`openai`-SDKs. |
| **DB-Tabellen** | (über `partner_utils`) View `twitch_streamers_partner_state`, Tabellen `twitch_streamers`, `twitch_live_state`. |
| **Externe Dienste** | Anthropic-, OpenAI- und MiniMax-API (LLM); die interne Bot-API (über `http_client`). |
| **Secret-Namen** | `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `MINIMAX_TOKEN_PLAN_KEY` / `MINIMAX_API_KEY` / `MINMAX`. Werte kommen aus keyring oder ENV — **nie** aus `constants.py`. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `constants.py` | 49 | Alle nicht-geheimen Konfig-Konstanten (Ports, Channel-IDs, Intervalle, Branding). |
| `partner_utils.py` | 261 | Partner-Gate: liest den kanonischen Partner-Status aus der DB-View. |
| `http_client.py` | 667 | `BaseInternalHttpClient` — abgesicherter aiohttp-Transport + Endpoint-Helfer für die interne Bot-API. |
| `llm_providers.py` | 151 | Gecachte Client-Factories für Anthropic/OpenAI/MiniMax inkl. Secret-Lookup. |
| `chat_bots.py` | 71 | Registry bekannter Chat-Bots + SQL-Ausschluss-Klausel. |
| `twitch_login.py` | 63 | Kanonisierung von Twitch-Login oder Profil-URL zu einem sauberen Login. |
| `__init__.py` | 1 | Leeres Package-Marker-Modul. |

## 4. Datenfluss / Lebenszyklus

Zwei wiederkehrende Muster prägen `core/`:

**a) Login kommt rein → wird kanonisiert → wird gegen Partner-Status geprüft.**
Beispiel IRC-Join-Entscheidung: ein roher Channel-Name (`#Foo`, `https://twitch.tv/Foo`, `@foo`) läuft durch `normalize_twitch_login` → `is_partner_channel_for_chat_tracking(login)` fragt die View `twitch_streamers_partner_state` → nur bei `is_partner_active = 1` wird gejoined. Bots im Chat werden später über `is_known_chat_bot` aus Lurker-/Engagement-Zählungen ausgeschlossen.

**b) Dashboard braucht Bot-Zustand → HTTP-Client.**
Der `dashboard_service` besitzt **keine** direkten Python-Referenzen auf den Bot (siehe [Architektur-Split](README.md)). Stattdessen ruft er eine Subklasse von `BaseInternalHttpClient`, die als Methode (`get_streamers()`, `get_raid_auth_url()`, …) einen HTTP-Request gegen `http://127.0.0.1:8776/...` absetzt, die Antwort validiert und Fehler auf einen sicheren `HttpClientError` abbildet.

## 5. Funktionsreferenz pro Datei

### constants.py
Reines Konstanten-Modul, keine Funktionen. Wichtigste Werte:

- `TWITCH_DASHBOARD_HOST = "127.0.0.1"`, `TWITCH_DASHBOARD_PORT = 8765` — Bind-Adresse des Dashboard-Service.
- `TWITCH_INTERNAL_API_HOST = "127.0.0.1"`, `TWITCH_INTERNAL_API_PORT = 8776` — die interne Bot-API (loopback-only).
- `TWITCH_DASHBOARD_NOAUTH = False` — No-Auth-Schalter; laut Kommentar nur per ENV-Override aktivieren.
- `POLL_INTERVAL_SECONDS = 15` — Haupt-Polling-Takt; laut Kommentar der „Sweet Spot“ zwischen Raid-Reaktion und API-Spam-Schutz.
- `TWITCH_LOG_EVERY_N_TICKS = 1` — Stats werden bei jedem N-ten Poll-Tick in die DB geschrieben (15 s effektiv).
- `TWITCH_CATEGORY_SAMPLE_LIMIT = 400` — max. zusätzliche Deadlock-Kategorie-Streams je Tick fürs Stats-Sampling.
- `TWITCH_TARGET_GAME_NAME = "Deadlock"`, `TWITCH_LANGUAGE = "de de-de de-at de-ch"` — Filter für relevante Streams.
- `TWITCH_BRAND_COLOR_HEX = 0x9146FF` — Twitch-Lila für Discord-Embeds.
- `TWITCH_RAID_REDIRECT_URI` — OAuth-Callback-URL für den Raid-Flow.
- Channel-IDs: `TWITCH_NOTIFY_CHANNEL_ID` (Live-Postings), `TWITCH_ALERT_CHANNEL_ID` (Warnungen/Re-Checks), `TWITCH_STATS_CHANNEL_IDS` (Liste der Kanäle, in denen `!twl` reagiert).
- `INVITES_REFRESH_INTERVAL_HOURS = 24` — Discord-Invite-Refresh-Takt (Rate-Limit-Schutz).

### chat_bots.py
Single Source of Truth für bekannte Service-/Chat-Bot-Accounts (`KNOWN_CHAT_BOTS`: nightbot, streamelements, streamlabs, fossabot, moobot, wizebot, botrix, soundalerts, pretzelrocks, `deutschedeadlockcommunity`).

- `normalize_chat_login(login: str | None) -> str` — trimmt und lowercased einen Login (leerer String bei `None`).
- `is_known_chat_bot(login: str | None) -> bool` — True, wenn der normalisierte Login in der Bot-Menge liegt. Genutzt zur Lurker-/Echte-Zuschauer-Abgrenzung.
- `build_known_chat_bot_not_in_clause(*, column_expr, placeholder="?", bots=None) -> tuple[str, list[str]]` — baut eine SQL-`NOT IN`-Klausel samt Parameterliste, um Bot-Logins aus Queries auszuschließen. **Wichtig:** Zeilen mit `NULL`/leerem Login bleiben erhalten (anonyme `chatter_id`-Zeilen werden nicht fälschlich gefiltert). Ohne Bot-Liste liefert es `"1=1", []`.

### twitch_login.py
- `normalize_twitch_login(raw: object) -> str | None` — kanonisiert einen beliebigen Eingabewert zu einem gültigen Twitch-Login oder `None`. Akzeptiert nackte Logins, `@name` und Profil-URLs. Bei URLs wird der Host geprüft (muss `twitch.tv` oder Subdomain davon sein) und das erste Pfadsegment als Login genommen; **reservierte Pfade** (`videos`, `settings`, `directory`, `clips`, … aus `_RESERVED_TWITCH_PATH_SEGMENTS`) werden verworfen. Das Ergebnis muss `TWITCH_LOGIN_RE = ^[a-z0-9_]{3,25}$` erfüllen.

### partner_utils.py
> Doku-Header: trennt **PARTNER** (volle Features: IRC, Raids, Analytics, Chat-Bot) von **MONITORED-ONLY** (nur Stats-Tracking, keine Chat-Features). Alle DB-Abfragen laufen über `readonly_connection()` aus `bot.storage`.

- `is_partner(row: dict, now_utc: datetime | None = None) -> bool` — **In-Memory-Prüfung** auf einer bereits geladenen Streamer-Zeile. Partner ist, wer `manual_verified_permanent` hat **oder** ein nicht abgelaufenes `manual_verified_until` — und **nicht** `manual_partner_opt_out` und **nicht** `is_monitored_only`. (Achtung: rechnet aus Rohfeldern; die unten genannten View-Funktionen sind die robustere Wahrheit.)
- `get_all_partners(include_archived=False) -> list[dict]` — alle Partner aus der View `twitch_streamers_partner_state` (`is_partner = 1`); ohne `include_archived` zusätzlich `is_partner_active = 1`. Liefert Login, User-ID, Verify-Felder, Discord-Felder.
- `get_live_partners() -> list[dict]` — aktive Partner, die gerade live sind (Join `twitch_live_state` auf `is_live = 1`), inkl. Titel/Spiel/Viewerzahl.
- `get_monitored_only() -> set[str]` — Lowercase-Logins aller `is_monitored_only`-Streamer aus `twitch_streamers`.
- `is_partner_channel_for_chat_tracking(login: str) -> bool` — soll dieser Channel von IRC/Chat-Bots gejoined werden? Liest `is_partner_active` aus der View; `False` bei unbekanntem oder Monitored-Only-Login.
- `is_operational_partner_channel(login: str) -> bool` — **strenge** Variante für Blacklist-/Bot-Ban-Schutz: True nur für operativ aktive Partner. Stützt sich allein auf die View-Wahrheit `is_partner_active` und schließt damit archivierte, opt-out, pausierte und `bot_banned` Partner aus. (Docstring-Notiz: frühere Roh-Berechnung übersah `operational_state` — jetzt entscheidet die View.)
- `get_partner_stats() -> dict` — Zähl-Statistik: `total_partners`, `live_partners`, `archived_partners`, `monitored_only`.
- `_parse_db_datetime(value) -> datetime | None` (privat) — ISO-Parsing aus der DB, normalisiert nach UTC.

### llm_providers.py
Einheitliche Factories für LLM-SDK-Clients. Fehlerklassen: `LLMProviderBootstrapError` (Basis), `LLMSecretNotFoundError` (Key fehlt), `LLMSDKUnavailableError` (SDK nicht installiert).

- `get_anthropic_client(*, api_key=None, timeout=None, async_client=True, client_factory=None) -> Any` — liefert einen (`Async`)`Anthropic`-Client; Key aus Argument oder `_load_secret("ANTHROPIC_API_KEY")`. Mit `client_factory` injizierbar (Tests).
- `get_openai_client(*, api_key=None, base_url=None, timeout=None, async_client=True) -> Any` — (`Async`)`OpenAI`-Client; Key aus `OPENAI_API_KEY`.
- `get_minimax_client(*, api_key=None, base_url=None, timeout=None, async_client=True) -> Any` — nutzt den **OpenAI-kompatiblen** Client gegen `MINIMAX_DEFAULT_BASE_URL = https://api.minimax.io/v1`. Key-Lookup-Kette: `MINIMAX_TOKEN_PLAN_KEY` → `MINIMAX_API_KEY` → `MINMAX`.
- `_load_secret(*secret_names) -> str` (privat) — versucht der Reihe nach jeden Namen: zuerst keyring (falls `keyring_enabled()`), dann `os.environ`; erster Treffer gewinnt, sonst `""`.
- `_build_anthropic_client(...)` / `_build_openai_client(...)` (privat, `@lru_cache`) — bauen den echten SDK-Client und **cachen** ihn (gleicher Key+Parameter → gleiche Instanz). Import-Fehler des SDK werden zu `LLMSDKUnavailableError`.

### http_client.py
`HttpClientError(RuntimeError)` — sicherer, nach außen zeigbarer Upstream-Fehler mit `status`, `code`, `message`.

`BaseInternalHttpClient` — abstrakte Basis aller Clients gegen die interne Bot-API. Subklassen setzen die Klassenattribute `api_base_path`, `token_header`, `error_type` und implementieren `_map_http_error`.

*Konstruktion & Transport:*
- `__init__(*, base_url, token, allow_non_loopback=False, timeout_seconds=10.0, session=None)` — validiert `base_url` (siehe unten), verlangt einen nicht-leeren Token, erzwingt min. 0,5 s Timeout, übernimmt optional eine fremde `aiohttp.ClientSession` (sonst eigene, die `close()` schließt).
- `_normalize_base_url(value, *, allow_non_loopback)` (classmethod) — **Sicherheitskern**: ergänzt fehlendes Schema, lehnt URLs mit Credentials ab, erlaubt nur `http`/`https`, erzwingt **Loopback-Host** außer bei `allow_non_loopback`, verlangt HTTPS für Nicht-Loopback und streift einen bereits enthaltenen `api_base_path` ab.
- `_is_loopback_host(host)` — erkennt `localhost` und Loopback-IPs.
- `_get_session()` / `close()` — Lazy-Session-Verwaltung; schließt nur selbst erzeugte Sessions.
- `_request_json(method, path, *, headers=None, query=None, payload=None)` — der zentrale Request: setzt den Token-Header, hängt Query-Parameter an (None wird verworfen), sendet JSON-Body, **folgt keinen Redirects**. Mappt Transportfehler auf `HttpClientError` (`Timeout`→504 `upstream_timeout`, `ClientError`→502 `upstream_connection_failed`), parst die Antwort und ruft bei `status >= 400` `_map_http_error`; ungültiges JSON → 502 `upstream_invalid_json`.
- `_map_http_error(status, payload)` — abstrakt (`NotImplementedError`), pro Subklasse zu implementieren.
- `_map_http_error_common(status, payload, *, preserve_server_error_code=False, preserve_server_message=False)` — gemeinsame Mapping-Hilfe: 400/404 → bad_request/not_found (mit sanitisierter Upstream-Message), **401/403 → 502 `upstream_auth_failed`** (Auth-Detail leckt nicht weiter), 429 → 503 `upstream_rate_limited`, ≥500 → 502 `upstream_unavailable`.

*Validierung von Eingaben* (werfen 400 `bad_request`): `_normalize_login_path_segment`, `_normalize_discord_user_id_value`, `_normalize_positive_id_value`, `_normalize_optional_positive_id_value`, `_normalize_required_text`, `_normalize_tracking_token_value`.

*Validierung von Antworten* (werfen 502 `upstream_invalid_shape`): `_validate_dict_payload`, `_validate_raid_state_payload`, `_validate_live_announcements_payload` (prüft Pflichtfelder je Eintrag), `_message_or_default`, `_sanitize_message`, `_parse_json`, `_extract_error_text`.

*Endpoint-Helfer* (1:1 zu den internen API-Routen, siehe [internal-api.md](internal-api.md)):
- Health: `healthz()`.
- Streamer-Verwaltung: `get_streamers()`, `add_streamer(login, *, require_link=False)`, `remove_streamer(login)`, `verify_streamer(login, *, mode)`, `archive_streamer(login, *, mode)`, `set_discord_flag(login, *, is_on_discord)`, `send_partner_chat_action(login, *, mode, color, message)`, `save_discord_profile(login, *, discord_user_id, discord_display_name, mark_member)`.
- Statistik/Analytics: `get_stats(*, hour_from=None, hour_to=None, streamer=None)`, `get_streamer_analytics(login, *, days=30)`, `get_analytics_comparison(*, days=30)`, `get_session(session_id)`.
- Raid: `get_raid_auth_url(login, *, discord_user_id=None, scope_profile=None)`, `get_raid_auth_state(*, discord_user_id)`, `get_raid_block_state(*, discord_user_id=None, twitch_login=None)`, `get_raid_go_url(state)` (gibt `None` bei `not_found` zurück), `send_raid_requirements(login)`, `process_raid_oauth_callback(*, code, state, error)`.

## 6. Datenbank & externe Schnittstellen

- **DB-View** `twitch_streamers_partner_state` ist die kanonische Partner-Wahrheit (`is_partner`, `is_partner_active`). Siehe [storage.md](storage.md) / [DATABASE.md](../DATABASE.md).
- **DB-Tabellen** (lesend): `twitch_streamers`, `twitch_live_state`.
- **HTTP raus:** alle `http_client`-Methoden sprechen die interne Bot-API auf `http://127.0.0.1:8776` an (Pfade siehe [internal-api.md](internal-api.md)).
- **LLM raus:** Anthropic / OpenAI / MiniMax (`https://api.minimax.io/v1`).
- **Secrets:** ausschließlich Namen, geladen via keyring/ENV — siehe Tabelle in Abschnitt 2.

## 7. Stolperfallen / Besonderheiten

- **`is_partner(row)` vs. View-Funktionen:** Die Roh-Zeilen-Variante kennt `operational_state` (z. B. `bot_banned`) nicht. Für Schutz-Entscheidungen (Blacklist, Bot-Ban) immer `is_operational_partner_channel` nutzen, nicht `is_partner`.
- **Loopback-Zwang im HTTP-Client:** Wer einen Client mit nicht-lokaler `base_url` baut, braucht explizit `allow_non_loopback=True` **und** HTTPS — sonst `ValueError` schon im Konstruktor. Schützt davor, interne Tokens versehentlich über das Netz zu schicken.
- **Fehler-Maskierung ist Absicht:** 401/403 vom Upstream werden bewusst zu 502 `upstream_auth_failed` — der Client verrät nach außen nie, ob der interne Token falsch war.
- **LLM-Client-Cache:** `_build_*`-Clients sind `lru_cache`-gecacht über den Key. Ein rotierter Key liefert einen neuen Client; derselbe Key über den Prozesslebenszyklus immer dieselbe Instanz.
- **`build_known_chat_bot_not_in_clause` behält NULL-Logins** — anonyme/loginlose Zeilen werden nicht herausgefiltert. Wer „echte Menschen“ zählt, muss zusätzlich auf `chatter_login IS NOT NULL` filtern.
- **Doku-Drift:** `MODULES.md` listet von `core/` nur `constants.py`, `partner_utils.py`, `chat_bots.py` — `http_client.py`, `llm_providers.py`, `twitch_login.py` fehlen dort.

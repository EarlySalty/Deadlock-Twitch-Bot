# internal_api/ — Architektur & Funktionsreferenz

> Pfad: `bot/internal_api/` · Stand: 2026-06-08 · 14 Dateien, ~3.800 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [ARCHITECTURE.md](../ARCHITECTURE.md) (Split-Runtime-Brücke), [core.md](core.md) (`BaseInternalHttpClient` = Gegenstelle), [services.md](services.md) (Dashboard nutzt diese API), [API.md](../API.md).

## 1. Zweck & Abgrenzung

`internal_api/` ist die **Brücke von der DashboardRuntime zur BotRuntime**: eine kleine, **loopback-only** aiohttp-API auf Port **8776**, über die der Dashboard-Prozess Bot-Zustand abfragt und Aktionen auslöst (Streamer hinzufügen, Raid-Auth-URL holen, Live-Announcements, Global-Ban …), ohne den Bot direkt zu importieren (siehe [Split-Runtime](../ARCHITECTURE.md)). Auth: ein geteilter Header-Token.

Abgrenzung: Sie hostet die Bot-internen Endpoints; der **Client** dafür ist `BotApiClient` (Subklasse von `BaseInternalHttpClient`, siehe [core.md](core.md)). Sie ist **nicht** das öffentliche Dashboard (das ist [dashboard.md](dashboard.md)).

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Gehostet von** | der BotRuntime (`TwitchBaseCog._start_internal_api` über den `InternalApiRunner`). |
| **Aufgerufen von** | der DashboardRuntime (`dashboard_service` über `BotApiClient`/internen Client). |
| **Nutzt** | die `InternalApiCallbacks` (vom Bot bereitgestellte Funktionen), `storage/`, `core/` (Login-Norm). |
| **Externe Dienste** | Master-Broker (für Discord-Log-Relay). |
| **Secret-Namen** | `TWITCH_INTERNAL_API_TOKEN` (Header `X-Internal-Token`), Broker-Token (Fallback-Kette). |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `app.py` | 1048 | Baut die aiohttp-App, verdrahtet Middleware + Route-Gruppen. |
| `policy.py` | 456 | Querschnitt: Token-Vergleich, Loopback/Proxy-Checks, JSON/Fehler. |
| `routes/streamers.py` | 529 | Streamer-/Admin-/Stats-Routen. |
| `routes/raid.py` | 442 | Raid-Auth-/OAuth-Routen. |
| `routes/telemetry.py` | 354 | Health/Observability/Chatters/Live. |
| `contracts.py` | 251 | `InternalApiCallbacks`, Konstanten, Idempotenz-Typen. |
| `runner.py` | 215 | `InternalApiRunner` — Server-Lebenszyklus. |
| `routes/discord_log.py` | 143 | Self-Explainer-Q&A per Master-Broker nach Discord. |
| `client.py` | 129 | Interner Client (Bot-API). |
| `routes/global_ban.py` | 118 | Globale Bannliste (Add/Remove/Check/List). |
| `routes/streamer_link.py` | 54 | Unverknüpfte Streamer (Discord-Match-Kandidaten). |
| `routes/_helpers.py` | 25 | `bind(server, handler)` — Handler an Server binden. |

## 4. Datenfluss / Lebenszyklus

**Start:** Der Bot erzeugt einen `InternalApiRunner(host, port=8776, token, callbacks=…)` und startet ihn. `app.py` baut die App: jede Route-Gruppe (`routes/*`) liefert `build_*_route_defs(server)` + `attach_*_routes(app, server)`; die Handler werden via `_helpers.bind` an die Server-Instanz gebunden.

**Request:** Jeder Request läuft durch die Policy-Schicht: `compare_internal_token` (konstante-Zeit-Token-Vergleich), Loopback-/Trusted-Proxy-Prüfung (`is_loopback_host`, `request_peer_host`, `forwarded_for`/`x_real_ip` nur von vertrauenswürdigen Proxys). Schreibende Calls verlangen einen **Idempotency-Key** (`IdempotencyInFlight` verhindert Doppelausführung bei Retries).

**Callbacks:** Die fachliche Arbeit machen **nicht** die Routen, sondern die `InternalApiCallbacks` — ein Bündel von Funktionen, das der Bot beim Start injiziert (Streamer hinzufügen, Raid-Auth bauen, Live-Link-Klick verbuchen …). Fehlt ein Callback, greifen Leer-Implementierungen (`_empty_*`).

**Discord-Log-Relay:** `discord_log.py` nimmt eine Self-Explainer-Q&A entgegen und schickt sie als Embed über den **Master-Broker** (Port 8770) nach Discord — mit stabilem Idempotency-Key (`_idempotency_key`) und Token-Fallback-Kette (`_master_broker_token`).

## 5. Funktionsreferenz pro Datei

### app.py
Baut die `web.Application`, registriert die Middleware (Policy) und ruft die `attach_*_routes`-Funktionen aller Route-Gruppen. Assembliert nur — die Logik liegt in den Modulen (vgl. [ARCHITECTURE.md](../ARCHITECTURE.md)).

### contracts.py
- `InternalApiCallbacks` (Dataclass) — das Callback-Bündel (add/remove/list/stats/verify/archive-Streamer, Discord-Flag/-Profil, Partner-Chat-Action, Live-Announcements/-Link-Click, Raid-Auth/-State/-Block/-Go/-Requirements/-OAuth-Callback, Global-Ban add/remove/check/list, Observability/Chatters). `coalesce(...)` mischt einzeln übergebene Callbacks zu einem Bündel.
- `IdempotencyInFlight` — In-Flight-Tracking für Idempotency-Keys. Konstanten: `INTERNAL_API_BASE_PATH`, Header-Namen.

### policy.py
- `compare_internal_token(presented, expected) -> bool` — konstante-Zeit-Vergleich.
- `host_without_port(raw)`, `is_loopback_host(raw)`, `is_loopback_origin(raw_origin)`, `request_peer_host(request)`, `is_trusted_proxy_host(peer_host, *, trusted_proxy_networks)`, `forwarded_client_host(...)` — Host-/Proxy-Sicherheit (der `trusted_proxy_networks`-Parameter bestimmt, welche Proxys Forwarded-Header setzen dürfen).
- `json_default(value)` + JSON-/Fehler-Helfer (`_json_response`, `_json_error`).

### runner.py
- `InternalApiRunner(*, host, port, token, base_path=INTERNAL_API_BASE_PATH, callbacks=None, …)` — Server-Lebenszyklus: `start()`, `stop()`, `is_running()`, `last_start_error()`. Nimmt entweder ein `InternalApiCallbacks`-Bündel oder einzelne `*_cb`-Argumente.

### client.py
Interner Client (Subklasse von `BaseInternalHttpClient`) mit `_map_http_error` + den Endpoint-Methoden (`get_observability_snapshot`, `get_chatters_debug`, `get_raid_auth_url`, `live_link_click(... idempotency_key=…)`, …).

### routes/
- `streamers.py` — `streamers`, `streamer_add`, `streamer_remove`, `streamer_verify`, `streamer_archive`, `streamer_discord_flag`, `streamer_discord_profile`, `stats`, `streamer_analytics`, `analytics_comparison`, `session_detail` + `build/attach`.
- `raid.py` — `build_raid_route_defs`/`attach_raid_routes`: Raid-Auth-URL/-State/-Block/-Go/-Requirements/-OAuth-Callback.
- `telemetry.py` — Health, `observability_debug`, `chatters_debug`, `live_active_announcements`.
- `global_ban.py` — `global_ban_add`, `global_ban_remove` (+ check/list).
- `discord_log.py` — `discord_self_explainer_log` (Relay via Master-Broker; `_master_broker_base_url`/`_token`, `_idempotency_key`).
- `streamer_link.py` — `link_candidates` (unverknüpfte Streamer für Discord-Matching).
- `_helpers.py` — `bind(server, handler)`.

## 6. Datenbank & externe Schnittstellen

- **HTTP (Server):** `http://127.0.0.1:8776/internal/twitch/v1/*` — Auth via `X-Internal-Token`. Beispiel (Raid-Blacklist-Add) steht in der Projekt-`CLAUDE.md`.
- **HTTP (raus):** Master-Broker (8770) für den Discord-Log-Relay.
- **DB:** indirekt über die Callbacks/`storage`.

## 7. Stolperfallen / Besonderheiten

- **Loopback-only + Token:** Die API bindet auf 127.0.0.1 und prüft den Token konstante-Zeit. Forwarded-Header werden nur von **vertrauenswürdigen** Proxys akzeptiert — sonst ließe sich die Peer-IP fälschen.
- **Idempotency-Key ist Pflicht für Schreib-Calls:** Ohne ihn würde ein Retry (z. B. nach Timeout) eine Aktion doppelt ausführen. `IdempotencyInFlight` + persistente Dedup verhindern das.
- **Routen sind dünn, Callbacks sind die Logik:** Wer Verhalten ändert, fasst meist den **Callback** (bot-seitig) an, nicht die Route. Fehlt ein Callback, antwortet die Route mit der Leer-Implementierung (kein Crash, aber auch kein Effekt).
- **`app.py` assembliert nur:** Fachlogik gehört in die Route-Module bzw. Callbacks, gemeinsame Regeln in `policy.py`/`contracts.py` — nicht in `app.py`.
- **Discord-Log braucht den Broker:** Läuft das Dashboard headless ohne Broker-Token, scheitert der Self-Explainer-Discord-Log — daher die Token-Fallback-Kette.

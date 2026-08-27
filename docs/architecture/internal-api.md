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

## 8. Interne Rust-Routen für rs-relay (Uplink)

Nicht Teil von `bot/internal_api/`, sondern des Rust-Dashboards
(`tb-dashboard-api`, `build_platform_token_router`). Gleiche Idee, gleiche
Tür: Loopback plus `X-Internal-Token`, kein Cookie, kein CSRF. Gegenstelle ist
rs-relay, nicht der Bot-Prozess.

| Route | Zweck |
|-------|-------|
| `GET /twitch/api/v2/internal/platform-token?streamer=&platform=` | Gültiger Access-Token des Streamers, ohne Refresh-Token (REQ-7). |
| `GET /twitch/api/v2/internal/stream-kennzahlen?streamer=` | Kennzahlen des laufenden Streams für das Karussell im Chat-Dock. |

`streamer` ist in beiden Fällen die Twitch-Nutzernummer.

### stream-kennzahlen

Antworten: `200` mit dem Rumpf unten, `400` ohne `streamer`, `401` ohne
Loopback oder ohne gültigen Token, `404 {"error":"nicht_live"}` wenn gerade
kein Stream läuft, `503 {"error":"nicht_verfuegbar"}` wenn die Auswertung
nicht antwortet.

Jede Kennzahl kommt in zwei Sichten: `session` ist der laufende Stream,
`gesamt` geht über alle Streams des Kanals (GRILLME C4-A1). Ausgegeben werden
nur Logins und Zahlen, nie eine Chatter-ID. Bots und der Streamer selbst
stehen in keiner Liste; die Ausschlussliste ist dieselbe wie in der
Zuschauer-Zeitleiste (`viewer_exclusion_logins`).

```json
{
  "streamer_login": "earlysalty",
  "session_id": 91,
  "session_started_at": "2026-08-27T18:00:00Z",
  "stand": "2026-08-27T19:30:00Z",
  "zuschauer": { "jetzt": 37, "spitze_session": 44, "spitze_gesamt": 300 },
  "top_chatter": {
    "session": [{ "login": "anna", "nachrichten": 12 }],
    "gesamt":  [{ "login": "cara", "nachrichten": 900 }]
  },
  "laengster_zuschauer": {
    "session": [{ "login": "anna", "minuten": 10.0 }],
    "gesamt":  [{ "login": "bert", "minuten": 52.0 }]
  },
  "haeufigster_zuschauer": {
    "gesamt": [{ "login": "bert", "sessions": 9 }]
  },
  "lurker": {
    "session": { "anwesend": 10, "still": 4, "anteil": 0.4 },
    "gesamt":  { "anteil_durchschnitt": 0.55 }
  }
}
```

Woher die Zahlen kommen (`tb-analytics::stream_kennzahlen`):

- **Minuten** aus `twitch_viewer_presence_ticks`; ein Tick je Zuschauer je
  30 Sekunden, Minuten sind also `Ticks / 2`.
- **Häufigkeit** als `COUNT(DISTINCT session_id)` aus
  `twitch_session_chatters`. `twitch_chatter_rollup.total_sessions` zählt nur
  Sessions mit Nachricht und taugt dafür nicht.
- **Nachrichten gesamt** aus `twitch_chatter_rollup.total_messages`,
  Nachrichten der Session aus `twitch_session_chatters.messages`.
- **Still mitlesend** heißt: über die Zuschauerliste gesehen, aber ohne
  Nachricht. `lurker.gesamt.anteil_durchschnitt` ist das Mittel der
  Session-Anteile, nicht Summe durch Summe; sonst bestimmte ein einziger
  großer Stream die Zahl allein.
- **Zuschauerspitzen** aus `twitch_stream_sessions.peak_viewers`.

Die Listen sind auf drei Namen begrenzt und bei Gleichstand nach Login
sortiert, damit die Karte im Dock zwischen zwei Abrufen nicht springt.

Die Gesamt-Sichten laufen über alle Sessions eines Kanals, die Anwesenheit
sogar über eine Hypertable. Sie liegen deshalb fünf Minuten in einem Cache je
Kanal (`GESAMT_CACHE_FRIST`); die Werte des laufenden Streams werden bei jeder
Anfrage frisch gerechnet. Das Dock fragt alle 30 Sekunden.

### chatter-verlauf

`GET /twitch/api/v2/internal/chatter-verlauf?streamer=&logins=a,b,c`

Beantwortet für bis zu 50 Logins auf einmal, wer zum allerersten Mal in
diesem Kanal schreibt. Das Chat-Dock hebt diese Nachrichten hervor, so wie
Twitch die erste Nachricht eines Zuschauers lila färbt (GRILLME C5-A2).
Ein Raid bringt viele neue Namen; deshalb im Bund und nicht je Login.

Antworten: `200` mit `{"eintraege":[...]}`, `400` ohne `streamer`, ohne einen
einzigen Login (`{"error":"logins_fehlen"}`) oder bei mehr als 50 Namen
(`{"error":"zu_viele_logins"}`), `401` ohne Loopback oder ohne Token,
`404 {"error":"nicht_live"}` ohne laufenden Stream,
`503 {"error":"nicht_verfuegbar"}`. Die Liste kommt nach Login sortiert.

```json
{
  "eintraege": [
    { "login": "neuling",   "erster_chat_ueberhaupt": true,  "sessions": 0 },
    { "login": "stammgast", "erster_chat_ueberhaupt": false, "sessions": 9 }
  ]
}
```

Jeder gefragte Login kommt zurück, auch ein unbekannter; er gilt dann als
erster Chat überhaupt. Als erster Chat zählt außerdem, wessen Verlauf erst in
der laufenden Session beginnt: wer gerade eben zum ersten Mal geschrieben hat,
steht schon im Verlauf und wäre sonst eine Sekunde später kein Erstchatter
mehr. Das eigene Kennzeichen des Bots (`confirmed_first_ever`,
`is_first_time_streamer` der laufenden Session) gewinnt über beides.

Als Verlauf zählen nur Zeilen aus `twitch_session_chatters` mit Nachrichten.
`twitch_chatter_rollup.first_seen_at` taugt dafür nicht: es steht schon, sobald
der Chatters-Poller jemanden im Kanal sieht, und bleibt danach unverändert; wer
lange nur zuschaut und heute zum ersten Mal schreibt, sähe darüber wie ein
alter Bekannter aus.

Einen Zeitpunkt der ersten Nachricht gibt die Antwort bewusst nicht aus. Auch
`first_message_at` steht auf einer Zeile des Pollers auf dem Moment der
Sichtung, nicht der Nachricht. Für "schon mal geschrieben, ja oder nein?"
reicht das, für eine Uhrzeit im Dock nicht. `sessions` zählt die Anwesenheit,
also bei wie vielen Streams jemand dabei war.

# Architektur-Uebersicht

## Zielbild

Das System laeuft als Split-Runtime aus zwei logisch getrennten Diensten:

```
BotRuntime
  Twitch API, Chat, Raid, Monitoring, EventSub, Social/Clip-Worker

DashboardRuntime
  aiohttp-App, Auth, Templates, Public API, Bot/Internal API-Clients
```

Wichtige Regeln:

- Keine geteilten In-Memory-Runtime-Objekte zwischen Bot und Dashboard.
- Kommunikation nur ueber PostgreSQL, Internal API und explizite Clients.
- Dashboard darf keine `TwitchStreamCog`-Referenz mehr benoetigen.
- Bot bootstrapped kein Dashboard.
- Shared Config ist erlaubt, aber nur als Konfiguration, nicht als gemeinsam besessene Runtime.

## Aktueller Code-Stand

Der Uebergang wird im Repo noch ueber `bot/runtime_state.py` abgefangen. Dort liegt derzeit ein
transitionaler Container mit `config`, `state` und `services`. Das ist keine gemeinsame
Systemzentrale mehr, sondern eine Kompatibilitaets-Schicht fuer die Bot-Seite.

Bot-only-Beispiele:

- `api`
- `_raid_bot`
- `_twitch_chat_bot`
- `_bot_token_manager`
- `clip_manager`
- `clip_fetcher`
- `upload_worker`
- `_internal_api_runner`
- `_reload_manager`
- `_eventsub_webhook_handler`

Dashboard-only-Beispiele:

- Host, Port, Token und No-Auth-Schalter fuer den Dashboard-Service
- Auth- und Session-Handling
- Template-/HTML-Wiring
- Bot/Internal API Clients
- Dashboard-Startpfad und Routenregistrierung

## Startpfade

Bot service:

```
bot/bot_service/__main__.py
  -> bot/bot_service/app.py
  -> run_bot_service()
```

Dashboard service:

```
bot/dashboard_service/__main__.py
  -> bot/dashboard_service/app.py
  -> run_dashboard_service()
```

Das ist der relevante Split:

- `run_bot_service()` startet den Bot-/Worker-Pfad und bleibt dashboard-frei.
- `run_dashboard_service()` startet den Dashboard-Pfad und bindet den Bot nur ueber
  `BotApiClient`/Internal API an.

## Architekturvertrag

BotRuntime besitzt:

- Twitch API und Token-Handling
- Chat- und Raid-Logik
- Monitoring, EventSub und Polling
- Clip-/Social-Media-Worker
- Internal API Host/Runner fuer bot-interne Rueckkopplung

DashboardRuntime besitzt:

- `aiohttp`-App und Route-Registrierung
- Auth und Session-Management
- Templates und Render-Helpers
- Public API / Service-Clients
- Bot-API-Client und Internal-API-Client

Geteilt sein duerfen nur:

- Konfigurationswerte aus ENV/Secrets
- Daten in PostgreSQL
- explizite API-Contracts und Response-Modelle

## Dashboard-Struktur

`bot/dashboard/` bleibt die serverseitige Dashboard-Funktionssammlung. Das Verzeichnis ist
feature-orientiert aufgebaut:

- `auth/` OAuth und Session-Handling
- `live/` Go-Live, Embeds und Discord-Announcements
- `raids/` Raid-Dashboard, History und OAuth-Callback-Flows
- `affiliate/` Affiliate-Tracking und PII-bezogene Hilfen
- `billing/` Stripe und Plan-Gating
- `admin/` Admin-Panel und rechtliche Seiten
- `core/` Templates, HTML- und Infrastruktur-Helpers
- `server_v2.py` Dashboard-App-Factory
- `mixin.py` Kompatibilitaets-Assembler fuer die Bot-Seite

`bot/dashboard_service/app.py` ist der Standalone-Einstieg fuer den Dashboard-Service und
wired die Dashboard-App nur ueber Clients/Callbacks.

## Internal API

`bot/internal_api/app.py` definiert die bot-interne HTTP-Schnittstelle. Diese Schicht ist die
bevorzugte Bruecke zwischen DashboardRuntime und BotRuntime, wenn Dashboard-Funktionalitaet
Bot-Zustand benoetigt.

## Kompatibilitaet

Die Bot-Seite darf temporaer Legacy-Aliasse auf den Runtime-Container behalten. Die Dashboard-Seite
soll diese Compatibility-Schicht nicht mehr direkt benoetigen. Ziel ist, die alte Cog-zentrierte
Verdrahtung schrittweise zu entfernen, ohne einen Big-Bang-Refactor zu erzwingen.

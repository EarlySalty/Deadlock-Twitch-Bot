# Deadlock Twitch Bot — Projekt-Index

Twitch-Monitoring, Analytics, Auto-Raids, Dashboard und Social-Media-Automatisierung fuer Deadlock-Streamer. Laeuft als Discord-Cog (Twitch Bot) plus eigenstaendiger Dashboard-Service.

## Schnell-Navigation

| Dokument | Inhalt |
|----------|--------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System-Uebersicht, Boot-Flow, Mixin-Komposition, Caddy-Setup |
| [docs/MODULES.md](docs/MODULES.md) | Alle Python-Module in 1 Satz erklaert |
| [docs/DATABASE.md](docs/DATABASE.md) | Alle DB-Tabellen mit Spalten und Zweck |
| [docs/API.md](docs/API.md) | Alle HTTP-Routes mit Methode und Zugriffslevel |
| [docs/ADMIN.md](docs/ADMIN.md) | Admin-Features: Panel, Billing, Partner-Management |
| [docs/STREAMER.md](docs/STREAMER.md) | Streamer-Features: Dashboard, Analytics, Raids, Social Media |
| [docs/BOT_TOKEN_SCOPES.md](docs/BOT_TOKEN_SCOPES.md) | Zentraler Bot-Token, Scope-Satz, CLI-Reauth und Credential-Store |
| [docs/streamer/README.md](docs/streamer/README.md) | Aktueller Streamer-Guide mit Dashboard, Commands und Feature-Doku |

## Feature-Matrix

| Feature | Admin | Streamer |
|---------|-------|----------|
| Admin-Panel (`/twitch/admin`) | Ja | Nein |
| Streamer hinzufuegen/entfernen | Ja | Nein |
| Billing / Stripe-Verwaltung | Ja | Teilweise (eigenes Abo) |
| Promo-Mode | Ja | Nein |
| Dashboard (`/twitch/dashboard`) | Ja | Ja |
| Dashboard V2 (`/twitch/dashboard-v2`) | Ja | Ja |
| Analytics-API (`/twitch/api/v2/*`) | Ja | Ja |
| Live-Announcements | Ja | Ja (eigene Config) |
| Raid-System | Ja | Ja (eigene Auth) |
| Affiliate-Links | Nein | Ja |
| Social Media / Clips | Nein | Ja |
| Chat-Commands (`!twl`, `!raid_*`) | Ja | Nein |

## Kern-Einstiegspunkte

- **Bot laden**: `twitch_cog.py` (Shim) → `bot/__init__.py` → `setup()`
- **Cog-Klasse**: `bot/cog.py` → `TwitchStreamCog`
- **Konstanten**: `bot/core/constants.py`
- **DB-Bootstrap**: `bot/storage/pg.py` → `prepare_runtime_storage()` (nutzt versionierten Runtime-Schema-Status)
- **Dashboard-Service standalone**: `bot/dashboard_service/app.py`

## 2 unabhaengige Services

```
Twitch Bot (Discord-Cog)          Dashboard-Service
  bot/cog.py                        bot/dashboard_service/app.py
  bot/monitoring/                   bot/analytics/
  bot/raid/                         bot/dashboard/
  bot/chat/                         bot/storage/
  bot/community/                    (eigenstaendig startbar)
  (eigenstaendig startbar)
```

Beide teilen sich die PostgreSQL-DB, aber keine direkten Python-Imports voneinander.

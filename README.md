# Deadlock Twitch Bot

Twitch bot, dashboard service, analytics, raid automation, and streamer tooling for the Deadlock ecosystem.

## Project Layout

- `bot/`: Python bot runtime, dashboard backend, internal API, raids, analytics, and storage
- `bot/dashboard_v2/`: React dashboard frontend with analytics views and fuzz tests
- `bot/admin_dashboard/`: admin-focused React frontend
- `website/`: public-facing landing pages and onboarding content
- `tests/`: Python regression suite
- `docs/`: architecture, API, database, and product surface documentation

## Key Entry Points

- `twitch_cog.py`: Discord cog shim
- `bot/cog.py`: main cog implementation
- `bot/dashboard_service/app.py`: standalone dashboard service
- `bot/internal_api/app.py`: internal API application

## Documentation

- [`INDEX.md`](INDEX.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/API.md`](docs/API.md)
- [`docs/DATABASE.md`](docs/DATABASE.md)
- [`docs/ADMIN.md`](docs/ADMIN.md)
- [`docs/STREAMER.md`](docs/STREAMER.md)
- [`docs/BOT_TOKEN_SCOPES.md`](docs/BOT_TOKEN_SCOPES.md)

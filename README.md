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
- [`docs/LEGAL_ACCESS_GATE.md`](docs/LEGAL_ACCESS_GATE.md)
- [`docs/STREAMER.md`](docs/STREAMER.md)
- [`docs/BOT_TOKEN_SCOPES.md`](docs/BOT_TOKEN_SCOPES.md)

## Local Legal Preview

Produktiver Public-Flow, Secrets, Caddy-Routing und Troubleshooting sind in [`docs/LEGAL_ACCESS_GATE.md`](docs/LEGAL_ACCESS_GATE.md) beschrieben.

- Quick preview server for `/twitch/impressum`, `/twitch/datenschutz`, and `/twitch/agb`:
  `python scripts/preview_legal_pages.py`
- Windows shortcut with optional browser launch:
  `powershell -ExecutionPolicy Bypass -File .\scripts\preview_legal_pages.ps1 -OpenBrowser`
- No-server export as plain HTML files:
  `python scripts/export_legal_preview.py`
- Windows shortcut for static export and opening `index.html`:
  `powershell -ExecutionPolicy Bypass -File .\scripts\export_legal_preview.ps1 -OpenIndex`
- IIS-ready: the export also writes a `web.config`, so you can point an IIS site or virtual directory directly at the exported folder.
- Full IIS setup with a dedicated local site:
  `powershell -ExecutionPolicy Bypass -File .\scripts\setup_legal_preview_iis.ps1 -OpenBrowser`

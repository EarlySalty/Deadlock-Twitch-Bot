# Deadlock Twitch Bot

Twitch bot, dashboard service, analytics, raid automation, and streamer tooling for the Deadlock ecosystem.

## Project Layout

- `rust/`: production runtime (Cargo workspace). Binaries in `rust/bin/`, shared crates in `rust/crates/`
- `bot/`: legacy Python implementation, kept as a reference only; it is no longer extended
- `bot/dashboard_v2/`: React dashboard frontend (German default, English on `dead-lock.de`)
- `bot/admin_dashboard/`: admin-focused React frontend
- `website/`: public landing pages and onboarding content
- `ops/`: systemd units, deploy scripts, deploy queue and Caddy snippets
- `tests/`: Python regression suite for the legacy code
- `docs/`: architecture, API, database, and product surface documentation

## Key Entry Points

Production services run as system units (`sudo systemctl ...`, not `systemctl --user`). Verify live state before relying on a port or unit name.

| Binary | Unit | Listens | Notes |
| --- | --- | --- | --- |
| `rust/bin/tb-bot` | `deadlock-twitch-bot-rust.service` | `127.0.0.1:8776` internal API, `127.0.0.1:8786` EventSub receiver | Twitch bot, chat, raids, monitoring |
| `rust/bin/tb-dashboard` | `deadlock-twitch-dashboard-rust.service` | `127.0.0.1:8769` | Dashboard and public web routes behind Caddy |
| `rust/bin/tb-discord-hub` | `deadlock-twitch-discord-hub-rust.service` | see `ops/systemd/` | Discord hub |
| `rust/bin/tb-deploy-queue` | `deploy-queue.service` (user unit) | see `ops/deploy-queue/` | Serialised Twitch release deploys |

Languages, domains and multi-server scope: [`docs/architecture/LANGUAGES-AND-DOMAINS.md`](docs/architecture/LANGUAGES-AND-DOMAINS.md).

## Documentation

- [`INDEX.md`](INDEX.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/architecture/`](docs/architecture/README.md) — function-level architecture per subsystem (backend + frontends)
- [`docs/API.md`](docs/API.md)
- [`docs/DATABASE.md`](docs/DATABASE.md)
- [`docs/ADMIN.md`](docs/ADMIN.md)
- [`docs/LEGAL_ACCESS_GATE.md`](docs/LEGAL_ACCESS_GATE.md)
- [`docs/STREAMER.md`](docs/STREAMER.md)
- [`docs/BOT_TOKEN_SCOPES.md`](docs/BOT_TOKEN_SCOPES.md)

## Local Legal Preview

Produktiver Public-Flow, Secrets, Caddy-Routing und Troubleshooting sind in [`docs/LEGAL_ACCESS_GATE.md`](docs/LEGAL_ACCESS_GATE.md) beschrieben.

The preview scripts that used to live in `scripts/` are no longer in the repository. Use the live pages under `https://deutsche-deadlock-community.de/twitch/impressum`, `/twitch/datenschutz` and `/twitch/agb` for checks.

<!-- DDC-LINK-START -->

## Deutsche Deadlock Community

Dieser Bot ist Teil der **[Deutschen Deadlock Community](https://deutsche-deadlock-community.de/)** — postet Live-Streamer der Community.

- 🌐 Website: [deutsche-deadlock-community.de](https://deutsche-deadlock-community.de/)
- 💬 Discord: [discord.gg/PhkP3WgY7w](https://discord.gg/PhkP3WgY7w)
- 🎮 [Helden-Übersicht auf Deutsch](https://deutsche-deadlock-community.de/helden/) · [Anfänger-Guide](https://deutsche-deadlock-community.de/guides/anfaenger/)

<!-- DDC-LINK-END -->

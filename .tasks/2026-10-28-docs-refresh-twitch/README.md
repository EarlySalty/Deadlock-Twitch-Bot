# Docs refresh: twitch (2026-10-28)

## Checked
- Live system units: deadlock-twitch-bot-rust, deadlock-twitch-dashboard-rust, deadlock-twitch-discord-hub-rust and deadlock-twitch-stream-coaching-watch are active. Watchdog and llm-usage-recover are inactive (timer and oneshot). deploy-queue.service is a user unit and enabled.
- Live listeners: 8776 and 8786 tb-bot, 8769 tb-dashboard, 8891 uplink-service. 8765 (Python dashboard) is not listening.
- Caddy (~/repos/Caddy/hosts/v50671/Caddyfile): the dead-lock.de HTTPS site imports dead_lock_english_dashboard (proxy 8769). The port-80 block redirects OAuth entry paths to the main domain with lang.
- Code: i18n (bot/dashboard_v2/src/i18n), landing locale (website/src/lib/landingLocale.ts), guild membership and raid scope (rust/crates/tb-db, rust/crates/tb-raid), invite replies (rust/bin/tb-bot/src/chat_wiring.rs).

## Changed
- docs/architecture/LANGUAGES-AND-DOMAINS.md (new, English): domains, language selection, English vs German features, multi-server scope, raid and invite rules, dictionary workflow.
- README.md: project layout and entry points (Rust binaries, system units, ports). Removed references to preview scripts that no longer exist.
- docs/ARCHITECTURE.md, docs/architecture/services.md, docs/architecture/README.md: legacy status notes pointing to the live Rust units and ports. docs/architecture/README.md gained a section on features without a dedicated page.
- docs/HOST_ROUTING_CONTRACT.md: Caddy path corrected.
- docs/STREAM_COACHING_AUDIT.md, docs/internal/ops-runbooks.md: user-unit commands replaced with system units (sudo -n systemctl).
- docs/LEGAL_ACCESS_GATE.md: dashboard port 8765 replaced by 8769.
- docs/ADMIN.md: dashboard port row corrected to the config key default 8769.
- docs/DOMAINS_OAUTH.md: live-state section added; /etc/caddy paths replaced by the Caddy repository checkout. The HTTPS dead-lock.de OAuth behaviour is documented as served by the dashboard, not redirected.
- rust/docs/discord-hub.md and rust/docs/04-cutover-plan.md: status banners (hub is live; cutover plan is historical).

## Open items
- Not documented, not verified in this pass: command aliases, extra command names, the `!raid <name>` form.
- Dated specs under rust/docs/specs and rust/docs/superpowers still contain `systemctl --user` examples. They are plans, not current state; left unchanged.
- Python-era reference pages (docs/architecture/core.md, bot-core.md and others) still describe the legacy runtime in detail; the banner in docs/architecture/README.md covers them.
- This repository has no docs deploy path; nothing to deploy. Deadlock-Docs public/ is not part of this slice.

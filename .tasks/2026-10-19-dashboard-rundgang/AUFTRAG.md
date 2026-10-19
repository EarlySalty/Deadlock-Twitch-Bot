# Dashboard-Rundgang (public, read-only)

## Goal

A public walkthrough of the full streamer dashboard, same idea as the Analyse demo at `/twitch/demo`, covering Home, Analyse, Social, Uplink, Verwaltung, Overlay, Titel and Pricing. No login. No writes. Shareable on the public host.

## Keep

- `/twitch/demo` stays the analytics-only demo.
- Admin host stays TABU. Public origin: `https://deutsche-deadlock-community.de`.

## URLs

- `https://deutsche-deadlock-community.de/rundgang` → `/twitch/rundgang/`
- Pages under `/twitch/rundgang/...` (analyse, social-media, uplink, verwaltung, overlay, titel, pricing).
- JSON stays on `/twitch/demo/api/v2`.

## Contract

- Fake partner MidCore Live, Studio KI entitlements, not admin.
- GET fixtures / demo JSON. POST/PUT/PATCH/DELETE return 403 with "In der Demo kannst du nichts ändern."
- Sidebar visible. Banner on every tour page.
- No DB, no cookies, no real stream keys.

## Reuse

- `handlers/demo.rs`, `runtimeConfig.ts`, `preview/fixtures.ts`, Caddy demo handle pattern.

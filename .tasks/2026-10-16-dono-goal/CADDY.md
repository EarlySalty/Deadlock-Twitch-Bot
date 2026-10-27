# Caddy routes for /twitch/dono-goal

Two Caddy sources serve this page:

- dead-lock.de: `ops/caddy/dead-lock-de.caddy` in this repo (lines 35 and 48).
- deutsche-deadlock-community.de: `hosts/v50671/Caddyfile` in EarlySalty/caddy-config, merged to master as cd5f2ed (gate ALLOW). `/twitch/dono-goal` was added to `@non_demo_embed` (line 230), `@overlay_embed` (line 305) and the dashboard proxy path list (line 697).

The checkout at /home/nathanael/repos/Caddy is dirty and its local master trails origin. Verify against the merged state with:

    git -C /home/nathanael/repos/Caddy show origin/master:hosts/v50671/Caddyfile | grep -n twitch/dono-goal

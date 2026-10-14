# English dashboard host and OAuth

`dead-lock.de` and `www.dead-lock.de` change the dashboard UI host only. The backend, OAuth applications, credentials, provider redirect URIs and callbacks remain exactly as configured for `https://deutsche-deadlock-community.de`. The two existing dead-lock.de Discord redirect entries remain unused. Do not add provider redirects or derive OAuth redirect URIs from the request host.

## Routing

`ops/caddy/dead-lock-de.caddy` replaces only the two dead-lock.de HTTP and HTTPS site blocks. On HTTPS it serves `/twitch` and `/twitch/*` through the same public dashboard upstream and special handlers as the main domain, without changing the incoming Host header. Main-domain admin redirects, static onboarding/FAQ pages, legal handlers, EventSub upstream and dashboard security headers are retained.

OAuth starts and callbacks, including `/twitch/auth/*`, `/callback/*`, raid authorization, social-media OAuth and platform connects, return a 302 to the same path and query on the main domain. An absent `lang` parameter is appended as `lang=en`; existing hints and query bytes are preserved. EventSub is a webhook, not an OAuth callback. HTTP dashboard requests first return a 308 to the same English host over HTTPS, so no dashboard HTML, JavaScript or API response is served over plaintext. HTTP OAuth navigation goes directly to the main domain over HTTPS with a 302. Other paths return a 308 to the main domain, except the existing common probe blockers. Social OAuth disconnect endpoints are mutation APIs, not OAuth navigation: they remain on the dashboard upstream and retain their POST method.

## Language and navigation

Resolution order is valid `lang=en|de` query, saved `dashboard.language` choice, English-host default, then the existing browser-language fallback. The provider saves valid query hints and removes only `lang` from the current URL, retaining other parameters and the fragment. Main-domain language behavior otherwise stays unchanged.

Dashboard login/connect links on the English host use absolute main-domain URLs, never new provider redirect URIs. The selected language is added to the dashboard login and Discord-link flows' existing `next` parameter. `auth/oauth_login.rs:56` preserves allowed next-path queries and `handlers/auth_login.rs:451` returns that stored path after login. Discord linking stores `next_path` in existing broker metadata (`handlers/discord_link.rs:85`) and restores it on completion (`:177` and `:246`), preserving both the language query and account fragment. These successful flows can return to the main-domain dashboard with English selected.

### Unchanged-backend limitations

A bare `lang` on an OAuth start URL does not ensure language persistence through every provider round trip. `handlers/auth_login.rs:80` accepts only `next` at login start; a direct Caddy redirect without a language-bearing `next` therefore cannot preserve the hint. Raid callbacks use their existing destination (`handlers/auth_login.rs:599` and `:667`) without propagating a start-query language hint, and social-media callbacks build their destination in `handlers/social_media.rs:3803` and `:3807` without language state. These flows still use the main domain, but no new cross-domain cookie or guaranteed language propagation is implemented for them. Steam callbacks and Discord error fallbacks likewise remain unchanged. The frontend does not claim that an unused start-query hint changes provider state.

## Exact splice and validation steps

1. Make a full temporary copy of `/etc/caddy/Caddyfile`, leaving the live file untouched.
2. In that copy, replace the entire `http://dead-lock.de, http://www.dead-lock.de { ... }` block and the immediately following `dead-lock.de, www.dead-lock.de { ... }` block with the complete contents of `ops/caddy/dead-lock-de.caddy`, including its snippet definition. Keep the old `(dead_lock_domain_alias)` snippet and every unrelated site block unchanged.
3. Run `caddy adapt --config /tmp/ttb-opus-o-Caddyfile --adapter caddyfile` and `caddy validate --config /tmp/ttb-opus-o-Caddyfile --adapter caddyfile`. If the unprivileged validator cannot open existing production log writers, replace only their `output file` destinations in the temporary copy with unique `/tmp` log paths before validation. This does not alter the routing splice.
4. Deployment, copying into `/etc/caddy`, and reload are integrator actions, not part of this package. No live Caddy change or reload was performed by the worker.

# B6 Internal API + Twitch Transport/Auth/Token

Scope: read-only parity audit for `bot/internal_api/*`, `bot/api/{twitch_api.py,twitch_auth.py,token_manager.py,token_error_handler.py}`, `bot/core/{twitch_login.py,partner_utils.py,http_client.py}` against `rust/crates/tb-internal-api/*`, `rust/crates/tb-transport-twitch/*`, and `rust/bin/tb-bot/src/{main.rs,wiring.rs}`. Baseline read first: `rust/docs/audit/2026-06-27/00-baseline.md` Section 2 intent ledger. No git, no secrets, no code changes outside this findings file.

## Count Summary

| Area | Count | Result |
|---|---:|---|
| Python internal API endpoints found by route defs | 36 | Source route defs: telemetry `bot/internal_api/routes/telemetry.py:324-334`, streamers `bot/internal_api/routes/streamers.py:490-504`, raid `bot/internal_api/routes/raid.py:407-419`, global ban `bot/internal_api/routes/global_ban.py:94-102`, streamer link `bot/internal_api/routes/streamer_link.py:36-42`, discord log `bot/internal_api/routes/discord_log.py:127-131`. |
| Rust routes covering Python endpoints | 36/36 | Mounted in `rust/crates/tb-internal-api/src/lib.rs:74-295`. No missing route after hard `rg`. |
| Rust-only internal routes in scope | 11 | `/raid/manual`, `/streamer/:login/discord-invite`, `/streamer-invites`, `/chat/command`, `/diagnose`, `/streamers/monitoring`, `/market-share`, `/scam-guard/revoke`, `/scam-guard/enforce`, `/raid/reauth-all`, `/stats/extended`. |
| Auth-gate weakenings | 0 | Python global gate is loopback + `X-Internal-Token` (`app.py:447-456`, `1005-1025`). Rust preserves both globally (`security.rs:147-199`, router layers `lib.rs:311-320`) and many handlers also require `AuthLevel::Admin`. |
| Token-type confusions found | 0 confirmed | Followers, ads/subs, chatters, raids, clips, moderator setup use user/bot tokens where required; app token remains for public reads/list/delete where valid. |

## Internal API Route Map

All rows inherit Python global auth (`Origin/peer loopback` + `X-Internal-Token`) and Rust global auth (`internal_api_loopback_guard` + `internal_api_auth_guard`) unless noted.

| Python endpoint | Python effect / extra gate | Rust route / handler | Status |
|---|---|---|---|
| `GET /healthz` | Health probe | `GET /healthz` -> `healthz::healthz_handler` (`lib.rs:75`) | OK |
| `GET /debug/observability` | Observability snapshot | `python_stubs::observability_handler` (`lib.rs:243-246`) | OK |
| `GET /debug/eventsub-processing` | EventSub processing debug | `telemetry_routes::eventsub_processing_debug_handler` (`lib.rs:251-254`) | OK |
| `GET /debug/chatters/{login}` | Chatter debug | `python_stubs::chatters_debug_handler` (`lib.rs:247-250`) | OK |
| `GET /live/active-announcements` | Active live announcements | `telemetry_routes::live_active_announcements_handler` (`lib.rs:203-206`) | OK |
| `POST /live/link-click` | Persist click; Discord action allowlist | `telemetry_routes::live_link_click_handler` (`lib.rs:207-209`); allowlist at `telemetry_routes.rs:466-484` | OK |
| `POST /eventsub/dispatch` | Dispatch EventSub payload | `eventsub::dispatch_handler` (`lib.rs:76-79`) | OK |
| `POST /eventsub/processing/requeue` | Requeue dead-letter processing | `python_stubs::eventsub_requeue_handler` (`lib.rs:255-258`) | OK |
| `GET /streamers` | List streamers | `streamers::list_handler` (`lib.rs:271-274`) | OK |
| `POST /streamers` | Add streamer + lifecycle/backfill | `streamers::add_handler` (`lib.rs:271-274`) | OK |
| `DELETE /streamers/{login}` | Remove/departner | `streamers::remove_handler` (`lib.rs:275-278`) | OK |
| `POST /streamers/{login}/verify` | Verify/clear/failed + roles; Python sent DMs | `streamers::verify_handler` (`lib.rs:279-282`) | OK with documented no-DM drop (`streamers.rs:38-39`, `656-664`, `799-800`). |
| `POST /streamers/{login}/archive` | Archive/unarchive/block-state messages | `streamers::archive_handler` (`lib.rs:283-286`) | OK |
| `POST /streamers/{login}/discord-flag` | Set Discord flag; Discord action allowlist | `streamers::discord_flag_handler` (`lib.rs:287-290`); allowlist at `streamers.rs:900-918`, call at `954` | OK |
| `POST /streamers/{login}/discord-profile` | Set Discord profile; Discord action allowlist | `streamers::discord_profile_handler` (`lib.rs:291-293`); allowlist call at `streamers.rs:1048` | OK |
| `POST /streamers/{login}/chat-action` | Send partner chat action | `python_stubs::chat_action_handler` (`lib.rs:259-262`) | OK; optional owner header gate is a strengthening, port returns 503 if native chat is disabled (`main.rs:1492-1496`). |
| `GET /stats` | Stats read | `stats_native::stats_handler` (`lib.rs:221`) | OK |
| `GET /analytics/streamer/{login}` | Streamer analytics | `streamer_analytics_native_handler` (`lib.rs:229-232`) | OK |
| `GET /analytics/comparison` | Analytics comparison | `streamers::analytics_comparison_handler` (`lib.rs:217-220`) | OK |
| `GET /sessions/{session_id}` | Session detail | `session_detail::session_detail_handler` (`lib.rs:233-235`) | OK |
| `GET /raid/auth-url` | OAuth auth URL/state | `oauth::auth_url_handler` (`lib.rs:176-179`) | OK |
| `GET /raid/auth-state` | OAuth state | `oauth::auth_state_handler` (`lib.rs:184-187`) | OK |
| `GET /raid/block-state` | OAuth block state | `oauth::block_state_handler` (`lib.rs:188-191`) | OK |
| `GET /raid/go-url` | Raid go URL | `oauth::go_url_handler` (`lib.rs:192`) | OK |
| `POST /raid/requirements` | Python sends requirements Discord-DM; Discord action allowlist | Live Rust route is `python_stubs::raid_requirements_handler` (`lib.rs:263-266`) returning `410 Gone feature_removed` (`python_stubs.rs:377-395`) | Deliberate no-DM divergence, not 1:1. Native `raid_oauth::requirements_handler` exists but is not mounted in prod (`rg`: only test helper mounts it at `raid_oauth.rs:967-977`); `tb-bot` port says it would return 503 and should be legacy-proxied (`raid_oauth_impl.rs:830-845`). |
| `POST /raid/oauth-callback` | OAuth callback; Discord action allowlist; idempotency | `oauth::oauth_callback_handler` (`lib.rs:180-183`); allowlist at `raid_oauth.rs:749-757` | OK |
| `POST /raid/blacklist/add` | Token/raid blacklist add | `raid_blacklist::add_handler` (`lib.rs:116-119`) | OK |
| `POST /raid/blacklist/remove` | Blacklist remove | `raid_blacklist::remove_handler` (`lib.rs:120-123`) | OK |
| `GET /raid/blacklist/check` | Blacklist check | `raid_blacklist::check_handler` (`lib.rs:124-126`) | OK |
| `GET /raid/blacklist` | Blacklist list | `raid_blacklist::list_handler` (`lib.rs:112-115`) | OK |
| `POST /globalban/add` | Global ban add | `global_ban::add_handler` (`lib.rs:94-97`) | OK |
| `POST /globalban/remove` | Global ban remove | `global_ban::remove_handler` (`lib.rs:98-101`) | OK |
| `GET /globalban/check` | Global ban check | `global_ban::check_handler` (`lib.rs:102-105`) | OK |
| `GET /globalban` | Global ban list | `global_ban::list_handler` (`lib.rs:93`) | OK |
| `GET /streamers/link-candidates` | Unlinked streamer candidates | `streamer_link::list_handler` (`lib.rs:130-132`) | OK |
| `POST /discord/self-explainer-log` | Relay Discord rich message through master broker | `self_explainer_log::handler` (`lib.rs:149-151`) | OK; matches intent to use broker instead of local Discord client. |

## Twitch Transport / Auth / Token

| Contract | Python reference | Rust target | Result |
|---|---|---|---|
| Helix timeout | `twitch_api.py` uses 20s total timeout | `REQUEST_TIMEOUT = 20s` (`client.rs:21-28`) | OK |
| Retry policy | Python retries 3 attempts on transient network/errors and HTTP 500/502/503/504; not 401/403 | Rust `MAX_HELIX_ATTEMPTS=3`, transient statuses only 500/502/503/504, transient reqwest only (`client.rs:30-50`, `207-263`); tests assert 403 no retry (`client.rs:466-478`, `555-572`) | OK |
| App-token manager | Python app token + 15min invalid-client block | Rust `AppTokenManager` has cache, expiry margin, cooldown (`token.rs:14-19`, `156-258`) | BUG below: `error`-only invalid-client body can miss cooldown. |
| User-token refresh/exchange | Python classifies `invalid_client` vs `invalid_grant`, blocks client auth on invalid-client | Rust checks cooldown before request, parses raw body `message`/`error`, blocks for 15min on invalid-client, does not blacklist streamer (`user_token.rs:58-70`, `144-190`) | OK |
| OAuth token normalization | Python strips `oauth:` (`twitch_api.py:189`, `token_manager.py`) | Bot seed/validate/refresh strips `oauth:` (`tb-chat/src/token.rs:39-49`, `388-457`); follower bot context strips case-insensitively (`tb-raid/src/bot_oauth.rs:55-100`); follower wiring uses resolver (`wiring.rs:187-225`) | OK for scoped live callers. Transport helpers expect already-normalized user tokens. |
| Public app-token reads | users/streams/videos/categories use app token | Rust `get/post/delete` build App-token requests (`client.rs:171-205`) | OK |
| Clip creation | Needs user token with `clips:edit` | `create_clip` uses `post_with_user_token` (`client.rs:302-319`) | OK |
| Followers total | Needs moderator/bot/streamer user token for real `total`; app-token fallback best-effort | `get_followers_total` selects user token when present, app token when absent (`streams.rs:264-312`); `main.rs:360-389` wires bot-token source + streamer fallback; `wiring.rs:248-330` tries bot then streamer fallback | OK; no app-token-only regression found. |
| Subscriptions / ads | Broadcaster user token only | `get_broadcaster_subscriptions` and `get_ad_schedule` use `get_with_user_token` (`streams.rs:345-381`); collector checks scopes before call (`main.rs:1070-1146`) | OK |
| Chatters | User token + moderator id; 403 must not retry blindly | `get_chatters` uses bearer user token and maps 403 to `NotModerator` (`chat.rs:430-471`); monitoring collector self-excludes bot login (`chatters_poller.rs:586-626`) | OK |
| Raid start/cancel | Source broadcaster user token | `start_raid`/`cancel_raid` use user-token builders (`raid.rs:1-59`) | OK |
| Moderator setup | Broadcaster user token | `add_channel_moderator` uses `post_with_user_token` (`moderation.rs:18-50`) | OK |
| EventSub | Create may use app token or user/bot override; list/delete app token | `create_eventsub_webhook_subscription` overrides bearer when provided, list/delete use app token (`eventsub.rs:57-160`) | OK |

## Explicit Old-Finding Closure

| ID | Closure |
|---|---|
| P3.7 reauth_all port injection | Confirmed still real. Handler exists and returns 503 when `BulkReauthExt(None)` (`reauth_all.rs:41-63`); `tb-bot` composition passes `None` with TODO (`main.rs:1501-1517`). |
| P2.100 verification-result Discord-DM | Closed as conscious drop, not a missing internal-api bug. Rust documents B10 no-DM at module top and verify branches (`streamers.rs:38-39`, `656-664`, `799-800`); role sync remains. |
| P3.22 runtime bot-logins excluded | Still open, but outside this B6 internal/transport scope. Python dashboard analytics dynamically excludes bot manager/chat/raid bot logins (`bot/analytics/api_viewers.py:32-52`). Rust dashboard viewer handler uses static `KNOWN_CHAT_BOTS` + streamer self and explicitly says dynamic bot config is not available (`viewers.rs:19-49`, `770-775`). Separate monitoring chatters poller does runtime bot-login self-exclude (`chatters_poller.rs:586-626`), but that does not close dashboard viewer analytics. |
| P3.28 port-bind retry/backoff | Confirmed still real. Python retries bind on EADDRINUSE with exponential delay (`bot/internal_api/runner.py:144-190`); Rust `tb-bot` does one `TcpListener::bind(...).await` and exits on error (`main.rs:1534-1540`). |

## Regression / Missing / Bug List

### B6-P2-001 App-token invalid-client circuit breaker misses `error`-only Twitch bodies

Severity: P2. Type: bug/regression in token domain.

Evidence: Rust `TokenErrorBody` has `message` and `error` (`token.rs:91-100`), and `is_invalid_client(status, body)` knows how to inspect raw text and both JSON fields (`token.rs:141-154`). But `fetch_app_token` parses the non-2xx response and returns only `.message` in `TokenError::HttpStatus` (`token.rs:120-131`). `AppTokenManager::access_token` then calls `is_invalid_client(status, &message)` (`token.rs:248-255`). If Twitch returns `{"error":"invalid client"}` with empty/missing `message`, the body evidence is discarded before the circuit breaker sees it. Python checks raw body/JSON `message`/`error` (`bot/api/twitch_auth.py:17` referenced by `twitch_api.py:148`), so this is weaker than Python for App-token invalid-client throttling.

Impact: app-token `invalid_client` may not set the 15-minute block; repeated app-token fetch attempts can continue, and `HelixClient::is_auth_blocked()` stays false.

### B6-P3-002 `/raid/reauth-all` is mounted but live-wired to 503

Severity: P3. Type: implemented-not-wired.

Evidence: Route is mounted (`lib.rs:193-199`) and handler is correct when a port exists (`reauth_all.rs:41-63`), but `tb-bot` passes `None` with an explicit TODO (`main.rs:1513-1515`). The live handler therefore returns `ApiError::unavailable()`/503 (`reauth_all.rs:52-54`). This closes P3.7 as still open.

### B6-P3-003 Internal API bind retry/backoff not ported

Severity: P3. Type: resilience regression.

Evidence: Python retries bind five times on address-in-use with 0.5/1/2/4s backoff (`runner.py:144-190`). Rust exits immediately on bind failure (`main.rs:1534-1540`). This closes P3.28 as still open.

### B6-P3-004 `/raid/requirements` live behavior is not 1:1 and has stale/contradictory wiring comments

Severity: P3. Type: documented divergence / dead native implementation.

Evidence: Python endpoint exists (`raid.py:414`) and sends requirements DM. Rust prod router mounts `python_stubs::raid_requirements_handler` (`lib.rs:263-266`), which returns `410 Gone feature_removed` (`python_stubs.rs:377-395`). A native `raid_oauth::requirements_handler` exists (`raid_oauth.rs:624-678`) and tests mount it (`raid_oauth.rs:967-977`), but prod does not. The `tb-bot` port implementation says the route should remain unregistered and legacy-proxy to Python (`raid_oauth_impl.rs:830-839`), while prod routing preempts fallback with the 410 stub. Under the B10 no-DM intent this is a conscious drop, but it is not a 1:1 port and not a broker replacement.

## Negative Findings

- Missing internal routes: none found after `rg` over Python route builders and Rust router mounts.
- Auth-gate weakening: none found. Rust preserves loopback + token globally, keeps Discord action allowlists for link-click, discord-flag/profile, and OAuth callback, and adds an owner header gate for chat-action.
- Token-type confusion: no confirmed live app-token/user-token mix-up in scoped callers. Follower totals use bot token first and streamer fallback; ads/subs/chatters/raids/moderator/clip paths use explicit user tokens.
- Reauth-All Discord DM loop: intentionally not ported per baseline; SQL operation exists behind a port, but port injection is missing.

Verification: code audit only. Commands used: `rg`, `sed`, `nl`, `ls`. No tests run; no service started.

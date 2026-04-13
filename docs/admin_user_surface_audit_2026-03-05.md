# Admin/User Surface Audit (2026-03-05)

## Scope
- Repository: `Deadlock-Twitch-Bot`
- Goal: ensure `admin.deutsche-deadlock-community.de/twitch/dashboard*` is not served as user dashboard and keep admin/user surfaces separated.

## Implemented Separation (App Layer)
- Admin host gate for dashboard pages:
  - `bot/analytics/api_overview.py`
  - `/twitch/dashboard`, `/twitch/dashboard-v2`, `/twitch/dashboard-v2/*` now return `404` on admin host (for all auth levels).
- Admin host gate for API auth path:
  - `bot/analytics/api_v2.py`
  - `_check_v2_auth()` now enforces admin-only access on admin host.
  - `_require_v2_auth()` now returns `403` for partner-level auth on admin host.
- Permission tightening:
  - `auth-status` now reports `canViewAllStreamers=true` only for `admin|localhost`.

## Implemented Separation (Proxy Layer)
- `C:\caddy\Caddyfile`:
  - Added explicit admin-domain block for user dashboard paths:
    - `/twitch/dashboard*`, `/twitch/dashboard-v2*`, legacy dashboard aliases.
    - response: `404 Not Found`.
  - Removed duplicate `twitch.earlysalty.com` site block that previously also proxied `/twitch/*` (now path-based on main domain).
  - Kept `/health` available on public Twitch host in the primary block.
- Runtime checks:
  - `https://admin.deutsche-deadlock-community.de/twitch/dashboard` -> `404`
  - `https://admin.deutsche-deadlock-community.de/twitch/dashboard-v2` -> `404`
  - `https://admin.deutsche-deadlock-community.de/twitch/admin` stays admin flow (`302` to Discord admin login when unauthenticated).

## Remaining Admin-Only Registrations (Expected)
- Admin routes and actions (Discord-admin protected):
  - `bot/dashboard/routes_mixin.py` (`/twitch/admin`, `/twitch/admin/chat_action`, etc.)
  - `bot/dashboard/server_v2.py` (`_require_token` admin-only prefixes)
- Admin callback/public origin defaults:
  - `bot/dashboard/server_v2.py`
  - `bot/dashboard/routes_mixin.py`
  - `bot/dashboard/auth_mixin.py`
  - `bot/social_media/dashboard.py`

## Validation
- Added regression tests:
  - `tests/test_dashboard_security_regressions.py`
  - `test_api_v2_auth_rejects_partner_token_on_admin_host`
  - `test_dashboard_route_on_admin_host_returns_404_without_auth`
  - `test_dashboard_route_on_admin_host_returns_404_for_partner`
  - `test_dashboard_route_on_admin_host_returns_404_for_admin`
- Test run:
  - `pytest -q tests/test_dashboard_security_regressions.py` -> `62 passed`
  - `pytest -q tests/test_split_bot_api_client.py tests/test_raid_oauth_success_redirect.py` -> `15 passed`

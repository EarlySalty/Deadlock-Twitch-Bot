# Dashboard Feature Map

The dashboard is organized by feature, not by role. Roles are documented in [ADMIN.md](ADMIN.md) and [STREAMER.md](STREAMER.md).

## Service Boundary

- `bot/dashboard/` stays part of the dashboard service.
- `bot/dashboard/server_v2.py` remains the dashboard server entry surface.
- `bot/dashboard/mixin.py` remains the main assembler used by the bot-side integration.
- `bot/analytics/`, `bot/raid/` und das Rust-Crate `tb-social-media` bleiben außerhalb dieses Feature-Splits.
- New imports must not make the Twitch Bot require dashboard-only startup code, and must not make the Dashboard require bot runtime entrypoints.

## Root Files Kept In Place

- [`bot/dashboard/mixin.py`](../../../bot/dashboard/mixin.py) main assembler
- [`bot/dashboard/routes_mixin.py`](../../../bot/dashboard/routes_mixin.py) large route table and handlers
- [`bot/dashboard/server_v2.py`](../../../bot/dashboard/server_v2.py) standalone dashboard server

## Feature Packages

- [`bot/dashboard/auth/`](../../../bot/dashboard/auth/)
  - `auth_mixin.py`
  - OAuth flow, session handling, token refresh
  - Used by admin and streamer flows
- [`bot/dashboard/live/`](../../../bot/dashboard/live/)
  - `live.py`
  - `live_announcement_mixin.py`
  - `announcement_mode_mixin.py`
  - Go-live state, embeds, Discord announcements, announcement mode
- [`bot/dashboard/raids/`](../../../bot/dashboard/raids/)
  - `raid_mixin.py`
  - Raid dashboard, history, OAuth callback handling
- [`bot/dashboard/affiliate/`](../../../bot/dashboard/affiliate/)
  - `affiliate_mixin.py`
  - Affiliate onboarding, Stripe Connect, tracking
- [`bot/dashboard/billing/`](../../../bot/dashboard/billing/)
  - `billing_mixin.py`
  - `billing_plans.py`
  - Billing catalog, checkout, Stripe webhook helpers
- [`bot/dashboard/admin/`](../../../bot/dashboard/admin/)
  - `legal_mixin.py`
  - Admin-only legal pages and related admin-only handlers
- [`bot/dashboard/core/`](../../../bot/dashboard/core/)
  - `templates.py`
  - `abbo_html.py`
  - `stats.py`
  - Shared dashboard HTML and infrastructure endpoints

## Compatibility

- Each feature package exposes re-exports through its own `__init__.py`.
- Legacy module paths such as `bot.dashboard.auth_mixin` remain available through lightweight shim modules in `bot/dashboard/`.

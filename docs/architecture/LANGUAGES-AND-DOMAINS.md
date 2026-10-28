# Languages, Domains and Multi-Server Scope

This document describes how the Twitch bot decides which language and which domain a user sees, which features are English, which are German-only, and how multi-server (guild) membership limits raids and invites. Every claim names the file or Caddy route it comes from. Verified against origin/main and the live Caddy configuration on 2026-10-28.

## Domains

| Host | Purpose | Source of truth |
| --- | --- | --- |
| `https://deutsche-deadlock-community.de` | Main German site and origin for all dashboards, OAuth callbacks and public pages | `~/repos/Caddy/hosts/v50671/Caddyfile`, site block for the main host |
| `https://dead-lock.de`, `https://www.dead-lock.de` | English entry host. Serves the dashboard shell in English and redirects the root to `/streamer/` | Caddy `dead-lock.de, www.dead-lock.de` block with snippet `dead_lock_english_dashboard` (about line 1659) |
| `http://dead-lock.de` (port 80, bound to the public IP) | Redirects OAuth entry points (`/twitch/auth`, `/callback`, `/twitch/raid/*`, `/twitch/discord_link`, `/twitch/social-media/*`) to the main host, keeping `lang` | Snippet `dead_lock_oauth_redirects` (about line 1407) |
| `admin.deutsche-deadlock-community.de` | Admin sites only (`/admin`, `/twitch/admin`, `/twitch/auth/*`). Not used for user-facing pages, OAuth done-URLs or bot buttons | Caddy admin block; skill `caddy-admin-host-tabu` |
| `twitch.earlysalty.com`, `earlysalty.com/trading` | Legacy hosts that only forward over Tailscale or redirect to the main host | Caddy legacy blocks at the end of the Twitch section |

Rules that follow from this:

- User-facing links and OAuth `redirect_uri` values use the main host. The English host serves its own OAuth entry paths through the dashboard, but `mainDomainOAuthUrl()` rewrites the links it renders to the main host with `lang=en` (see below). The HTTP port-80 block for `dead-lock.de` redirects the OAuth entry points to the main host.
- Do not put the admin host into user-facing URLs (`bot/dashboard_v2/src/i18n/oauthLinks.ts`, `MAIN_DASHBOARD_ORIGIN`).
- The English host and the main host both proxy to the Rust dashboard on `127.0.0.1:8769` (`deadlock-twitch-dashboard-rust.service`). The host decides the default language, not a separate backend.

## How the language is chosen

### Dashboard (`bot/dashboard_v2`)

Order of precedence, from `readInitialLanguage()` in `bot/dashboard_v2/src/i18n/dictionary.ts`:

1. `?lang=de` or `?lang=en` in the URL (`isLanguage`, validated against `LANGUAGES`).
2. The stored choice under localStorage key `dashboard.language.v2` (`LANGUAGE_STORAGE_KEY`). The legacy key `dashboard.language` maps to German only (`readPersistedLanguage`).
3. On `dead-lock.de` and `www.dead-lock.de`: English (`isEnglishDashboardHost`).
4. Otherwise the browser language: German if any entry of `navigator.languages` starts with `de`, English for everything else (`browserLanguage`).
5. Fallback `DEFAULT_LANGUAGE = 'de'`.

`LanguageContext` (`bot/dashboard_v2/src/context/LanguageContext.tsx`) starts from `readInitialLanguage()`, stores the choice and removes `lang` from the URL.

Translation works with German as the key:

- `translate(language, text, params)` in `dictionary.ts` looks up the German text in `TRANSLATIONS[language]`. A missing entry returns the German text unchanged, so an untranslated string is never empty.
- Placeholders use `{name}` syntax and are filled from `params`.
- English entries come from `bot/dashboard_v2/src/i18n/en/*.ts`, one file per area (`shell`, `social`, `socialRecording`, `analytics`, `charts`, `chat`, `coaching`, `verwaltung`, `overlay`, `uplink`, `monetization`, `backend`). `bot/dashboard_v2/src/i18n/en/index.ts` merges them into `EN_AREA_ENTRIES`.

### OAuth and the English host

`mainDomainOAuthUrl()` in `bot/dashboard_v2/src/i18n/oauthLinks.ts` rewrites links from the English host to `https://deutsche-deadlock-community.de` and adds `lang` when it is missing. For `/twitch/auth/login` and `/twitch/auth/discord/link` it also carries the language inside the `next` parameter. The Caddy OAuth redirect snippet keeps `lang` on the 302 redirect, so a `lang` query survives the hop through the bare HTTP host.

### Public website (`website/`)

`isEnglishLanding()` in `website/src/lib/landingLocale.ts`:

- `?lang=de` always wins and forces German.
- Any request on `dead-lock.de` or `www.dead-lock.de` is English.
- On the main host, English is used only for an allowlisted path set (`/`, `/streamer`, `/streamer/faq`, `/streamer/onboarding`, `/streamer/vergleich`, `/streamer/clips`, `/twitch/faq`, `/twitch/onboarding`, `/clips`, `/streamer/affiliate-portal`, `/streamer/vertriebler`, `/streamer/v1|v3|v4`, `/affiliate-portal`, `/vertriebler`, `/v1|v3|v4`, `/twitch/affiliate/portal`) and only with `?lang=en`.
- English landing copy lives in `website/src/data/landingEnglish.ts`. `landingLink()` keeps `lang` on links to the allowlisted paths.

Legal page paths (`/impressum`, `/datenschutz`, `/agb`, `/privacy`, `/imprint`, `/terms`, `/legal`) get `X-Robots-Tag: noindex, nofollow, noarchive, nosnippet, noimageindex` from the main site block in the Caddyfile (matcher `legal_noindex`).

## What is English and what is German only

| Area | Language | Evidence |
| --- | --- | --- |
| Dashboard shell and the areas listed above | English when selected | `bot/dashboard_v2/src/i18n/en/*.ts` |
| Dashboard pages without an English entry | German (fallback) | `translate()` in `dictionary.ts` |
| Public landing pages on `dead-lock.de` and the allowlisted paths with `?lang=en` | English | `website/src/lib/landingLocale.ts`, `website/src/data/landingEnglish.ts` |
| Discord and Twitch chat replies from the bot | German | Strings in `rust/bin/tb-bot/src/chat_wiring.rs` (invite replies around lines 2765 to 2840; a German fallback message at line 419) |
| Internal docs, commits, code identifiers | English | Working rule in `CLAUDE.md` |

The bot's chat replies are not localised by domain. Only the dashboard and the website switch language.

## Multi-server onboarding and the hub

A streamer can belong to several Discord servers ("guilds"). The data model is in `rust/migrations/20261015100000_guild_membership_onboarding.sql` and the role policy in `rust/migrations/20261017120000_guild_streamer_role_policy.sql` and `rust/migrations/20261017180000_guild_streamer_role_keep_min_days.sql`.

Tables used by the code:

- `streamer_guilds`: links a Twitch user to a guild, with `active` and `joined_at`.
- `discord_guilds`: guild record with `status`, `removed_at`, `language`, `cross_server_raids`, `promo_invite_url`.
- `twitch_streamer_invites`: streamer-specific invite URLs per guild (`invite_url`, `streamer_login`, `twitch_user_id`).
- `twitch_streamer_identities`: Twitch identity with `broadcaster_language`.

The Discord hub runs the `tb-discord-hub` binary as the Rust unit `deadlock-twitch-discord-hub-rust.service` (system unit, user `twitchhub`, active on 2026-10-28; `ops/systemd/deadlock-twitch-discord-hub-rust.service`). `rust/docs/discord-hub.md` still describes an older "dormant H1" state and is marked stale in this refresh.

Onboarding is handled in `guild_onboarding_sessions` (created by the migration above). The community guild is the one configured as `discord.streamer_link.guild_id` in the runtime settings (`community_guild_id()` in `rust/crates/tb-db/src/guild_membership.rs`).

## Raid and invite scope

### Home guild

`home_guild_id()` in `rust/crates/tb-db/src/guild_membership.rs` picks the streamer's home guild: an active, non-removed guild in `streamer_guilds`, preferring the community guild, then the oldest join. The home guild decides the language group and which invite is shown.

### Language group

`language_group()` in the same file reduces a language tag to its primary subtag (`de-AT` and `de_ch` become `de`; `und` and `other` have no group). `source_language()` in `rust/crates/tb-raid/src/partner_roster.rs` uses `broadcaster_language` first and falls back to the home guild's `language`.

### Raid rules

`raid_scope_allows()` in `rust/crates/tb-db/src/guild_membership.rs` decides whether a target may be raided:

- Shared guild: a target in any guild the source also belongs to is always allowed. The language check does not apply.
- Cross-server: allowed only when the target and source share a language group, the source's home guild has `cross_server_raids` on, and the target has at least one guild with `cross_server_raids` on.

`load_scope()` in `partner_roster.rs` builds the candidate list from these rules. Raid toggles belong to the source streamer.

### Invites

`scoped_invite_for_id()` and `scoped_invite_for_login()` in `rust/crates/tb-db/src/guild_membership.rs` return:

1. A streamer-specific invite from `twitch_streamer_invites` in the home guild, matched by Twitch user ID or by login (case-insensitive) when the row has no user ID yet.
2. Otherwise the guild's `promo_invite_url` from `discord_guilds`.

Chat replies that show an invite are built in `rust/bin/tb-bot/src/chat_wiring.rs` (`discord_invite`, `invite_url`, `invite_line`).

## Dictionaries: how to add or change a string

Dashboard:

1. Add or change the German text where it is used, and keep it as the key.
2. Add the English text to the matching area file in `bot/dashboard_v2/src/i18n/en/<area>.ts`, keyed by the exact German text. Placeholders must be identical (`{name}`).
3. A missing English entry falls back to German; check the English view in the browser after the change (`?lang=en`).

Website: English landing copy goes into `website/src/data/landingEnglish.ts`; add the page path to the allowlist in `website/src/lib/landingLocale.ts` if the page should be reachable in English.

Bot replies: German, in `rust/bin/tb-bot/src/chat_wiring.rs` or the owning crate. There is no translation table for chat.

## Open points

- No English variant exists for the Discord and Twitch chat replies, the legal pages or the Deadlock-Docs `public/` pages.
- The English host sends the root to `/streamer/`, not to a separate English home page.

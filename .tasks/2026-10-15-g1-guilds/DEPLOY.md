# G1 deployment procedure

## Migration order

Apply the existing migration chain, then `rust/migrations/20261015100000_guild_membership_onboarding.sql`, through the normal migration runner before restarting the dashboard API or enabling the Hub gateway. Do not replay the reverted G0 migration separately. The migration reconciles existing G0 tables and removes its shadow-only constraints.

The migration leaves `guild_runtime.enabled = false`. This preserves the existing home-server partner definition and prevents foreign-server signup until parity has been reviewed. No worker has enabled this switch in production.

## Production backfill expectations

The home guild is `1289721245281292288`. Its existing broker-managed behavior remains unchanged; Hub setup and membership changes ignore that guild.

Existing identities with a numeric Twitch ID and a partner row receive one home membership unless it already exists. Active status derives from the old canonical active-partner view, excluding exclusions, signup denylist entries and hard administrative blocks. Existing membership rows and administrator deactivations are not overwritten. Unknown broadcaster languages remain NULL. Recognized historical channel/session observations supply the latest known language; subsequent channel observations update it without replacing newer information or accepting `und`/`other`.

A streamer may have several guild memberships. Foreign onboarding requires a fresh Discord identity and guild-list verification. Nonautomatic admission stays inactive and creates no partner row. Administrator-deactivated memberships cannot be self-reactivated. Guild or member removal disables the affected memberships; reinstalling the Hub does not restore old active memberships automatically. Returning members can submit fresh Discord proof through onboarding; pending admission and administrator deactivation remain protected. Automatic admission can restore a nonadministratively deactivated partner row, but historical administrative flags do not override a currently active row. Periodic REST reconciliation checks foreign guild/member removals missed while the gateway was offline.

Changing an existing linked Discord identity invalidates its stored membership proofs and deactivates foreign memberships. The home broker must refresh home membership; foreign membership needs fresh onboarding proof or administrator approval. This prevents a legacy Discord relink from transferring previously verified guild access to another Discord account.

## Configuration and provider registration

Supply the existing Hub bot credential and a database connection through the existing secret manager/service injection. Without both, the new gateway and reconciliation tasks do not start. Enable the Discord GUILD_MEMBERS privileged intent for this application. The Hub registers `/setup`; only Manage Server or Administrator callers can invoke its effects. Its invite does not request Administrator or Mention Everyone.

The dashboard needs `DISCORD_HUB_CLIENT_ID` and `DISCORD_HUB_CLIENT_SECRET` injected through the existing secret manager. Register the exact Twitch and Discord redirect URIs for each deployed allowlisted origin:

- `/twitch/guild-onboarding/twitch/callback`
- `/twitch/guild-onboarding/discord/callback`

Allowed origins are HTTPS `dead-lock.de`, `www.dead-lock.de`, `deutsche-deadlock-community.de` and its `www` variant. Register only origins actually served. Keep the existing ordinary dashboard OAuth redirects and partner/admin gates unchanged. The dedicated onboarding cookie is HttpOnly, Secure, SameSite=Lax, scoped to `/twitch/guild-onboarding`, expires after at most twenty minutes and is stored only as a lookup hash. Discord OAuth tokens remain in memory and are not persisted.

Serve the existing dashboard SPA shell at `/twitch/guild-onboarding`. The generic-domain website signup CTA opens this page; the community-domain signup CTA retains its previous behavior. No real-user OAuth exchange or Discord message is part of this worker's checks.

## Read-only verification

Run only through the approved production DB access path after credential rotation is complete. These queries reveal aggregate state, not account credentials or personal identity rows.

```sql
SELECT * FROM public.twitch_partner_shadow_summary();
SELECT enabled, enabled_at FROM public.guild_runtime WHERE singleton;
SELECT status, count(*) FROM public.discord_guilds GROUP BY status ORDER BY status;
SELECT source, active, count(*) FROM public.streamer_guilds GROUP BY source, active ORDER BY source, active;
SELECT count(*) AS missing_home_backfills
FROM public.twitch_partners p
JOIN public.twitch_streamer_identities i USING (twitch_user_id)
WHERE p.twitch_user_id ~ '^[0-9]{1,20}$'
  AND NOT EXISTS (
    SELECT 1 FROM public.streamer_guilds sg
    WHERE sg.twitch_user_id = p.twitch_user_id AND sg.guild_id = 1289721245281292288
  );
SELECT count(*) AS invalid_active_links
FROM public.streamer_guilds sg
WHERE sg.active AND sg.require_discord_link AND NOT sg.is_on_discord;
SELECT count(*) AS foreign_ai_enabled FROM public.discord_guilds
WHERE guild_id <> 1289721245281292288 AND ai_features;
SELECT count(*) AS invalid_session_hashes FROM public.guild_onboarding_sessions
WHERE session_hash !~ '^[0-9a-f]{64}$';
SELECT count(*) AS known_languages FROM public.twitch_streamer_identities
WHERE broadcaster_language IS NOT NULL;
```

Investigate every shadow mismatch before activation, including legacy Discord-link requirements. Do not rewrite administrator decisions to force parity. After aggregate verification and the integrator's acceptance, activate with `SELECT public.enable_guild_runtime();` in the approved migration/administration path. This operation locks the relevant guild and partner tables and refuses a parity mismatch. Foreign installations can be prepared with `/setup` beforehand, but foreign membership assignment remains blocked until activation.

## Rollback and live acceptance

The complete 200-migration chain passed on disposable PostgreSQL with TimescaleDB, and its schema matched the committed snapshot. Dashboard and website builds, targeted lint and existing tests passed. Before integration, require successful Rust check, clippy, build and relevant regression evidence from the final worker handover. Do not count an earlier targeted authentication run that timed out before executing any tests. Moli timed out twice and produced no usable screenshot, so browser-based functional and visual acceptance remains an integrator prerequisite, not a completed worker check.

If runtime behavior regresses, the integrator can set `guild_runtime.enabled = false` using the approved administration path. Disabled mode preserves legacy home-guild access but does not authorize foreign-only enrollments, including partners whose foreign membership was revoked. Legacy home signup continues creating its first membership after activation; synchronization aggregates partner history instead of copying an individual historical row. Leave the additive tables, hashes and membership history intact. Do not roll back by dropping tables or replaying old migrations. Rolling back application binaries requires checking compatibility with the additive schema.

After deployment, verify the generic-domain website signup link, `/twitch/guild-onboarding`, both registered OAuth callbacks, no-server guidance, single-server automatic selection, multi-server choice and pending admission using an approved test account. Verify that the onboarding-only cookie cannot access any dashboard, partner or admin API. Confirm that the home guild still uses the broker, then check foreign `/setup`, member departure and Hub removal in an approved test guild. No production deployment or real-user messaging was performed by this worker.

CREATE TABLE public.discord_guilds (
    guild_id bigint PRIMARY KEY,
    ai_features boolean NOT NULL DEFAULT false,
    removed_at timestamptz,
    status text NOT NULL DEFAULT 'active'
);
CREATE TABLE public.streamer_guilds (
    twitch_user_id text NOT NULL,
    guild_id bigint NOT NULL REFERENCES public.discord_guilds (guild_id),
    active boolean NOT NULL DEFAULT true,
    PRIMARY KEY (twitch_user_id, guild_id)
);
CREATE TABLE IF NOT EXISTS public.twitch_streamer_identities (
    twitch_user_id text PRIMARY KEY,
    twitch_login text
);
CREATE TABLE public.twitch_partners_all_state (
    twitch_user_id text PRIMARY KEY,
    is_partner_active integer NOT NULL
);
CREATE TABLE public.twitch_exclusions (
    twitch_user_id text PRIMARY KEY,
    reactivated_at timestamptz
);
CREATE TABLE public.twitch_partner_signup_denylist (
    twitch_user_id text PRIMARY KEY,
    twitch_login text
);
CREATE TABLE public.twitch_partners (
    twitch_user_id text PRIMARY KEY,
    technical_pause_reason text
);

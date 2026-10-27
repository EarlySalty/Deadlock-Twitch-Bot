ALTER TABLE public.discord_guilds
    ADD COLUMN IF NOT EXISTS live_role_id bigint,
    ADD COLUMN IF NOT EXISTS role_activity_method text NOT NULL DEFAULT 'deadlock_days',
    ADD COLUMN IF NOT EXISTS role_keep_window_days integer NOT NULL DEFAULT 28,
    ADD COLUMN IF NOT EXISTS role_gain_window_days integer NOT NULL DEFAULT 30,
    ADD COLUMN IF NOT EXISTS role_gain_min_days integer NOT NULL DEFAULT 4,
    ADD COLUMN IF NOT EXISTS role_require_bot_access boolean NOT NULL DEFAULT true;

ALTER TABLE public.discord_guilds
    DROP CONSTRAINT IF EXISTS discord_guilds_live_role_positive,
    DROP CONSTRAINT IF EXISTS discord_guilds_role_activity_method,
    DROP CONSTRAINT IF EXISTS discord_guilds_role_windows;

ALTER TABLE public.discord_guilds
    ADD CONSTRAINT discord_guilds_live_role_positive
        CHECK (live_role_id IS NULL OR live_role_id > 0),
    ADD CONSTRAINT discord_guilds_role_activity_method
        CHECK (role_activity_method IN ('deadlock_days', 'any_stream_days', 'none')),
    ADD CONSTRAINT discord_guilds_role_windows
        CHECK (
            role_keep_window_days BETWEEN 1 AND 365
            AND role_gain_window_days BETWEEN 1 AND 365
            AND role_gain_min_days BETWEEN 1 AND role_gain_window_days
        );

UPDATE public.discord_guilds
SET live_role_id = 1560993753643098153,
    updated_at = now(),
    updated_by = 'migration'
WHERE guild_id = 1289721245281292288
  AND live_role_id IS NULL;

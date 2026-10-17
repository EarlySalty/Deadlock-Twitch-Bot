ALTER TABLE public.discord_guilds
    ADD COLUMN IF NOT EXISTS role_keep_min_days integer NOT NULL DEFAULT 4,
    ALTER COLUMN role_keep_window_days SET DEFAULT 60;

UPDATE public.discord_guilds
SET role_keep_window_days = 60,
    updated_at = now(),
    updated_by = 'migration'
WHERE role_keep_window_days = 28;

ALTER TABLE public.discord_guilds
    DROP CONSTRAINT IF EXISTS discord_guilds_role_windows;

ALTER TABLE public.discord_guilds
    ADD CONSTRAINT discord_guilds_role_windows
        CHECK (
            role_keep_window_days BETWEEN 1 AND 365
            AND role_keep_min_days BETWEEN 1 AND role_keep_window_days
            AND role_gain_window_days BETWEEN 1 AND 365
            AND role_gain_min_days BETWEEN 1 AND role_gain_window_days
        );

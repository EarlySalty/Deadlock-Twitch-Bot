-- WS-B: Session-Schema-Drift beheben.
-- Prod fuehrt twitch_stream_sessions.is_mature und
-- twitch_stream_sessions.had_deadlock_in_session als BOOLEAN. Aeltere
-- Rust-Baselines/Fixtures hatten hier INTEGER 0/1, wodurch die Rust-Binds
-- gegen Prod scheiterten.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'twitch_stream_sessions'
           AND column_name = 'is_mature'
    ) THEN
        IF (
            SELECT data_type
              FROM information_schema.columns
             WHERE table_schema = 'public'
               AND table_name = 'twitch_stream_sessions'
               AND column_name = 'is_mature'
        ) <> 'boolean' THEN
            ALTER TABLE public.twitch_stream_sessions
                ALTER COLUMN is_mature DROP DEFAULT;
            ALTER TABLE public.twitch_stream_sessions
                ALTER COLUMN is_mature TYPE boolean
                USING (COALESCE(is_mature, 0) <> 0);
        END IF;

        ALTER TABLE public.twitch_stream_sessions
            ALTER COLUMN is_mature SET DEFAULT false;
    END IF;

    IF EXISTS (
        SELECT 1
          FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'twitch_stream_sessions'
           AND column_name = 'had_deadlock_in_session'
    ) THEN
        IF (
            SELECT data_type
              FROM information_schema.columns
             WHERE table_schema = 'public'
               AND table_name = 'twitch_stream_sessions'
               AND column_name = 'had_deadlock_in_session'
        ) <> 'boolean' THEN
            ALTER TABLE public.twitch_stream_sessions
                ALTER COLUMN had_deadlock_in_session DROP DEFAULT;
            ALTER TABLE public.twitch_stream_sessions
                ALTER COLUMN had_deadlock_in_session TYPE boolean
                USING (COALESCE(had_deadlock_in_session, 0) <> 0);
        END IF;

        ALTER TABLE public.twitch_stream_sessions
            ALTER COLUMN had_deadlock_in_session SET DEFAULT false;
    END IF;
END $$;

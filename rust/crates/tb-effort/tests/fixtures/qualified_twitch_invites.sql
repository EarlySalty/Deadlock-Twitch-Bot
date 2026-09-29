CREATE TABLE bot.twitch_invite_joins (
    join_id BIGINT PRIMARY KEY,
    guild_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    streamer_login TEXT NOT NULL,
    streamer_twitch_user_id TEXT,
    inviter_twitch_user_id TEXT,
    invite_code TEXT NOT NULL,
    joined_at TIMESTAMPTZ NOT NULL,
    eligible BOOLEAN NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'qualified', 'expired')),
    qualified_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    reason TEXT,
    UNIQUE (guild_id, user_id, joined_at),
    CHECK ((status = 'qualified') = (qualified_at IS NOT NULL)),
    CHECK (qualified_at IS NULL OR (
        eligible AND qualified_at >= joined_at + INTERVAL '336 hours'
        AND qualified_at <= joined_at + INTERVAL '720 hours'
    ))
);
CREATE UNIQUE INDEX twitch_invite_joins_one_credit_idx
    ON bot.twitch_invite_joins (guild_id, user_id) WHERE status = 'qualified';
CREATE INDEX twitch_invite_joins_pending_idx
    ON bot.twitch_invite_joins (guild_id, join_id) WHERE status = 'pending';
CREATE INDEX twitch_invite_joins_changes_idx
    ON bot.twitch_invite_joins (guild_id, updated_at, join_id);

CREATE FUNCTION bot.guard_twitch_invite_join() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Twitch invite joins cannot be deleted';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.status <> 'pending' THEN
            RAISE EXCEPTION 'Twitch invite joins must start pending';
        END IF;
    ELSE
        IF OLD.status <> 'pending' OR NEW.status NOT IN ('qualified', 'expired') THEN
            RAISE EXCEPTION 'Twitch invite terminal state is immutable';
        END IF;
        IF (to_jsonb(NEW) - ARRAY['status', 'qualified_at', 'updated_at', 'reason'])
            IS DISTINCT FROM (to_jsonb(OLD) - ARRAY['status', 'qualified_at', 'updated_at', 'reason']) THEN
            RAISE EXCEPTION 'Twitch invite attribution is immutable';
        END IF;
    END IF;
    NEW.updated_at := clock_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER twitch_invite_join_guard
    BEFORE INSERT OR UPDATE OR DELETE ON bot.twitch_invite_joins
    FOR EACH ROW EXECUTE FUNCTION bot.guard_twitch_invite_join();

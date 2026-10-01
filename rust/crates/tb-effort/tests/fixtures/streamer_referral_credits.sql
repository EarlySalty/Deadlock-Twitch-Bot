CREATE TABLE twitch_streamer_referral_credits (
    referred_twitch_user_id TEXT PRIMARY KEY CHECK (referred_twitch_user_id ~ '^[1-9][0-9]{0,19}$'),
    source_id TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL DEFAULT 'streamer_referral' CHECK (event_type = 'streamer_referral'),
    streamer_twitch_user_id TEXT NOT NULL CHECK (streamer_twitch_user_id ~ '^[1-9][0-9]{0,19}$'),
    streamer_login TEXT NOT NULL,
    referred_login TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'affiliate_streamer_claim' CHECK (source_type = 'affiliate_streamer_claim'),
    source_claimed_at TIMESTAMPTZ NOT NULL,
    credited_at TIMESTAMPTZ NOT NULL,
    CHECK (streamer_twitch_user_id <> referred_twitch_user_id),
    CHECK (source_claimed_at <= credited_at),
    CHECK (source_id = 'streamer_referral:' || referred_twitch_user_id)
);
CREATE INDEX twitch_streamer_referral_credits_streamer_time_idx
    ON twitch_streamer_referral_credits (streamer_twitch_user_id, credited_at);

CREATE FUNCTION guard_streamer_referral_credit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Streamer referral credits are immutable';
END;
$$;
CREATE TRIGGER streamer_referral_credit_guard
    BEFORE UPDATE OR DELETE ON twitch_streamer_referral_credits
    FOR EACH ROW EXECUTE FUNCTION guard_streamer_referral_credit();

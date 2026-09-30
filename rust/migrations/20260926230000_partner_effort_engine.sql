CREATE TABLE IF NOT EXISTS partner_effort_events (
    id BIGSERIAL PRIMARY KEY,
    partner_twitch_user_id TEXT NOT NULL,
    partner_login TEXT NOT NULL,
    event_type TEXT NOT NULL,
    source_id TEXT NOT NULL,
    points INTEGER NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    viewer_twitch_user_id TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT partner_effort_events_type_chk CHECK (
        event_type IN (
            'qualified_invite',
            'streamer_referral',
            'co_stream',
            'party_play',
            'clip_submitted',
            'clip_top3',
            'quest_done'
        )
    ),
    CONSTRAINT partner_effort_events_source_chk CHECK (length(trim(source_id)) > 0),
    UNIQUE (partner_twitch_user_id, event_type, source_id)
);

CREATE INDEX IF NOT EXISTS partner_effort_events_partner_time_idx
    ON partner_effort_events (partner_twitch_user_id, occurred_at DESC);

CREATE INDEX IF NOT EXISTS partner_effort_events_viewer_invites_idx
    ON partner_effort_events (partner_twitch_user_id, viewer_twitch_user_id, occurred_at DESC)
    WHERE event_type = 'qualified_invite' AND viewer_twitch_user_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS partner_effort_streaks (
    partner_twitch_user_id TEXT PRIMARY KEY,
    current_streak INTEGER NOT NULL DEFAULT 0 CHECK (current_streak >= 0),
    longest_streak INTEGER NOT NULL DEFAULT 0 CHECK (longest_streak >= 0),
    last_qualified_week DATE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS partner_effort_weekly_quests (
    partner_twitch_user_id TEXT NOT NULL,
    week_start DATE NOT NULL,
    position SMALLINT NOT NULL CHECK (position BETWEEN 1 AND 3),
    quest_key TEXT NOT NULL CHECK (
        quest_key IN (
            'community_match',
            'stream_together',
            'active_discord_invite',
            'submit_clip',
            'stream_above_average'
        )
    ),
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (partner_twitch_user_id, week_start, position),
    UNIQUE (partner_twitch_user_id, week_start, quest_key)
);

CREATE TABLE IF NOT EXISTS partner_effort_shared_chat_observations (
    partner_twitch_user_id TEXT NOT NULL,
    stream_session_id BIGINT NOT NULL,
    other_partner_twitch_user_id TEXT NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (
        partner_twitch_user_id,
        stream_session_id,
        other_partner_twitch_user_id
    )
);

CREATE INDEX IF NOT EXISTS partner_effort_shared_chat_last_seen_idx
    ON partner_effort_shared_chat_observations (last_seen_at DESC);

CREATE OR REPLACE FUNCTION partner_effort_events_reject_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'partner_effort_events is append-only';
END
$$;

DROP TRIGGER IF EXISTS partner_effort_events_no_update ON partner_effort_events;
CREATE TRIGGER partner_effort_events_no_update
    BEFORE UPDATE ON partner_effort_events
    FOR EACH ROW
    EXECUTE FUNCTION partner_effort_events_reject_mutation();

DROP TRIGGER IF EXISTS partner_effort_events_no_delete ON partner_effort_events;
CREATE TRIGGER partner_effort_events_no_delete
    BEFORE DELETE ON partner_effort_events
    FOR EACH ROW
    EXECUTE FUNCTION partner_effort_events_reject_mutation();

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON partner_effort_events TO twitchdash;
        GRANT SELECT ON partner_effort_streaks TO twitchdash;
        GRANT SELECT ON partner_effort_weekly_quests TO twitchdash;
    END IF;
END
$$;

CREATE TABLE partner_effort_program (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO partner_effort_program(singleton) VALUES (TRUE);

CREATE TABLE partner_effort_events (
    id BIGSERIAL PRIMARY KEY,
    partner_twitch_user_id TEXT NOT NULL CHECK (partner_twitch_user_id ~ '^[0-9]+$'),
    partner_login TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('qualified_invite','streamer_referral','co_stream','party_play','clip_submitted','clip_top3','quest_done')),
    source_id TEXT NOT NULL CHECK (length(source_id) BETWEEN 1 AND 512),
    points INTEGER NOT NULL CHECK (points >= 0),
    occurred_at TIMESTAMPTZ NOT NULL,
    viewer_twitch_user_id TEXT CHECK (viewer_twitch_user_id ~ '^[0-9]+$'),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
    rules_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (partner_twitch_user_id,event_type,source_id),
    CHECK (viewer_twitch_user_id IS NULL OR event_type = 'qualified_invite')
);
CREATE INDEX partner_effort_events_time_idx ON partner_effort_events (occurred_at,partner_twitch_user_id);
CREATE INDEX partner_effort_events_partner_time_idx ON partner_effort_events (partner_twitch_user_id,occurred_at);
CREATE INDEX partner_effort_events_viewers_idx ON partner_effort_events (partner_twitch_user_id,viewer_twitch_user_id) WHERE event_type='qualified_invite';
CREATE UNIQUE INDEX partner_effort_events_single_attribution_idx ON partner_effort_events(event_type,source_id) WHERE event_type IN ('qualified_invite','streamer_referral','clip_submitted','clip_top3');

CREATE TABLE partner_effort_weekly_quests (
    partner_twitch_user_id TEXT NOT NULL,
    week_start DATE NOT NULL CHECK (extract(isodow FROM week_start)=1),
    position SMALLINT NOT NULL CHECK (position BETWEEN 1 AND 3),
    quest_key TEXT NOT NULL CHECK (quest_key IN ('community_match','stream_together','active_discord_invite','submit_clip','stream_above_average')),
    goal BIGINT NOT NULL CHECK (goal > 0),
    baseline_minutes BIGINT NOT NULL DEFAULT 0 CHECK (baseline_minutes >= 0),
    reward_points INTEGER NOT NULL CHECK (reward_points > 0),
    bonus_points INTEGER NOT NULL CHECK (bonus_points > 0),
    rules_hash TEXT NOT NULL,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (partner_twitch_user_id,week_start,position),
    UNIQUE (partner_twitch_user_id,week_start,quest_key)
);

CREATE TABLE partner_effort_stream_weeks (
    partner_twitch_user_id TEXT NOT NULL,
    stream_id TEXT NOT NULL,
    week_start DATE NOT NULL,
    deadlock_seconds BIGINT NOT NULL CHECK (deadlock_seconds >= 0),
    observed_through TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (partner_twitch_user_id,stream_id,week_start)
);

CREATE TABLE partner_effort_weeks (
    partner_twitch_user_id TEXT NOT NULL,
    week_start DATE NOT NULL,
    streamed BOOLEAN NOT NULL DEFAULT FALSE,
    effort BOOLEAN NOT NULL DEFAULT FALSE,
    frozen BOOLEAN NOT NULL DEFAULT FALSE,
    streak INTEGER NOT NULL DEFAULT 0 CHECK (streak >= 0),
    evaluated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (partner_twitch_user_id,week_start)
);
CREATE UNIQUE INDEX partner_effort_one_freeze_per_month_idx ON partner_effort_weeks(partner_twitch_user_id,(date_trunc('month',week_start::timestamp))) WHERE frozen;

CREATE TABLE partner_effort_streaks (
    partner_twitch_user_id TEXT PRIMARY KEY,
    current_streak INTEGER NOT NULL DEFAULT 0 CHECK (current_streak >= 0),
    longest_streak INTEGER NOT NULL DEFAULT 0 CHECK (longest_streak >= current_streak),
    last_qualified_week DATE,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE partner_effort_achievements (
    partner_twitch_user_id TEXT NOT NULL,
    achievement_key TEXT NOT NULL,
    target BIGINT NOT NULL CHECK (target > 0),
    earned_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (partner_twitch_user_id,achievement_key,target)
);

CREATE TABLE partner_effort_shared_chat_observations (
    partner_twitch_user_id TEXT NOT NULL,
    stream_id TEXT NOT NULL,
    other_partner_twitch_user_id TEXT NOT NULL,
    shared_chat_session_id TEXT NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    confirmed_seconds BIGINT NOT NULL DEFAULT 0 CHECK (confirmed_seconds >= 0),
    PRIMARY KEY (partner_twitch_user_id,stream_id,other_partner_twitch_user_id)
);

CREATE TABLE partner_effort_party_observations (
    partner_twitch_user_id TEXT NOT NULL,
    stream_id TEXT NOT NULL,
    party_id TEXT NOT NULL,
    steam_id TEXT NOT NULL,
    other_steam_id TEXT NOT NULL CHECK (other_steam_id <> steam_id),
    discord_id BIGINT NOT NULL,
    other_discord_id BIGINT NOT NULL CHECK (other_discord_id <> discord_id),
    observed_at TIMESTAMPTZ NOT NULL,
    match_started_at TIMESTAMPTZ NOT NULL,
    stream_started_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (partner_twitch_user_id,stream_id,steam_id,other_steam_id,observed_at)
);
CREATE INDEX partner_effort_party_pending_idx ON partner_effort_party_observations(observed_at);

CREATE TABLE partner_effort_source_receipts (
    source TEXT NOT NULL,
    source_id TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source,source_id)
);

CREATE TABLE partner_effort_source_cursors (
    source TEXT NOT NULL,
    lane TEXT NOT NULL CHECK (lane IN ('recent','reconcile')),
    source_id BIGINT NOT NULL DEFAULT 0 CHECK (source_id >= 0),
    occurred_at TIMESTAMPTZ NOT NULL,
    completed_once BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (source,lane)
);

CREATE TABLE partner_effort_source_state (
    source TEXT PRIMARY KEY,
    checked_at TIMESTAMPTZ NOT NULL,
    successful_at TIMESTAMPTZ,
    healthy BOOLEAN NOT NULL DEFAULT FALSE,
    error_code TEXT
);

CREATE FUNCTION partner_effort_reject_mutation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% is append-only', TG_TABLE_NAME USING ERRCODE='55000';
END
$$;
CREATE TRIGGER partner_effort_events_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON partner_effort_events FOR EACH STATEMENT EXECUTE FUNCTION partner_effort_reject_mutation();
CREATE TRIGGER partner_effort_achievements_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON partner_effort_achievements FOR EACH STATEMENT EXECUTE FUNCTION partner_effort_reject_mutation();
CREATE TRIGGER partner_effort_quests_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON partner_effort_weekly_quests FOR EACH STATEMENT EXECUTE FUNCTION partner_effort_reject_mutation();

DO $$
DECLARE t TEXT;
BEGIN
    FOREACH t IN ARRAY ARRAY['partner_effort_program','partner_effort_events','partner_effort_weekly_quests','partner_effort_stream_weeks','partner_effort_weeks','partner_effort_streaks','partner_effort_achievements','partner_effort_shared_chat_observations','partner_effort_party_observations','partner_effort_source_receipts','partner_effort_source_cursors','partner_effort_source_state'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
            EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON %I TO twitchbot',t);
        END IF;
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchdash') THEN
            EXECUTE format('GRANT SELECT ON %I TO twitchdash',t);
        END IF;
    END LOOP;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
        GRANT USAGE, SELECT ON SEQUENCE partner_effort_events_id_seq TO twitchbot;
        REVOKE UPDATE, DELETE, TRUNCATE ON partner_effort_events,partner_effort_weekly_quests,partner_effort_achievements FROM twitchbot;
    END IF;
END
$$;
